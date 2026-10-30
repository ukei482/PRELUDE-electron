//! Web のペイン(docs/redesign-rust.md の判断 3)。Chromium を PRELUDE 専用のプロファイルで 1 つだけ動かし、CDP で操作する。
//!
//! - CDP は `--remote-debugging-pipe`(fd 3 で送り、fd 4 で受ける。1 つの JSON の終わりは NUL)。ポートを開けないので、ほかのプロセスから操作されない。
//!   コアが落ちると通信の口が閉じ、Chromium も終わる
//! - Web のペイン 1 つ = Chromium のアプリの窓 1 つ(アドレスバー・タブなし)。動いている Chromium に `--app=file:///…/prelude-pane.html#<印>` を頼んで作る。
//!   その小さなページが題名を `PRELUDE-PANE:<印>` にするので、KWin スクリプト(embed.js)はその題名で窓を見分けて取り込む。取り込んだら本当の URL へ移す
//! - URL・題名は CDP のイベント(Target.targetInfoChanged)で受けてコアに知らせる(コアはペインの url を書き換えて残す。起動し直すとその URL で開く)
//! - 新しい窓: noopener のリンク(新しいタブで開く)は PRELUDE の新しいタブにする。opener のある小さな窓(ログインのポップアップなど)は取り込まずそのまま
//! - ダウンロードの保存先は、今のタブのブックマークの paths の download
use crate::embed::Note;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::os::fd::{FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};

/// Web のペイン(コアの状態から作る)
#[derive(Debug, Clone, PartialEq)]
pub struct WebWant {
    pub pane: u64,
    pub url: String,
}

/// 戻る・進む・読み直す
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Go {
    Back,
    Forward,
    Reload,
}

enum Msg {
    Sync(Vec<WebWant>, String),
    Claimed(u64),
    Go(u64, Go),
    Event(String, Value, Option<String>),
    Closed,
    Shutdown(oneshot::Sender<()>),
}

#[derive(Clone)]
pub struct Handle {
    tx: mpsc::UnboundedSender<Msg>,
}

impl Handle {
    /// Web のペインと、ダウンロードの保存先
    pub fn sync(&self, wants: Vec<WebWant>, download_dir: String) {
        let _ = self.tx.send(Msg::Sync(wants, download_dir));
    }
    /// ペインの窓を KWin が取り込んだ(本当の URL へ移してよい)
    pub fn claimed(&self, pane: u64) {
        let _ = self.tx.send(Msg::Claimed(pane));
    }
    pub fn go(&self, pane: u64, g: Go) {
        let _ = self.tx.send(Msg::Go(pane, g));
    }
    pub async fn shutdown(&self) {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(Msg::Shutdown(tx)).is_ok() {
            let _ = rx.await;
        }
    }
}

const BROWSERS: [&str; 5] =
    ["/snap/bin/chromium", "/usr/bin/chromium", "/usr/bin/chromium-browser", "/usr/bin/google-chrome-stable", "/usr/bin/google-chrome"];

/// 使う Chromium と、プロファイル・印を出すページの置き場所。snap 版は ~/snap/chromium/common の外の隠しフォルダに書けないので、そこに置く。
/// tag の付いたコア(試すとき)は、ふだんのプロファイルと取り合わないように別のプロファイルを使う
fn locate(tag: &str) -> Option<(String, PathBuf, PathBuf)> {
    let bin = BROWSERS.iter().find(|p| Path::new(p).exists())?;
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let base = if bin.starts_with("/snap/") { home.join("snap/chromium/common") } else { home.join(".config/PRELUDE-electron") };
    let profile =
        if bin.starts_with("/snap/") { base.join(format!("prelude-profile{tag}")) } else { base.join(format!("browser-profile{tag}")) };
    Some((bin.to_string(), profile, base.join("prelude-pane.html")))
}

/// 題名に印を出すだけのページ。窓を取り込んだら本当の URL へ移るので、見えるのは一瞬
const PANE_PAGE: &str = "<!doctype html><meta charset=\"utf-8\"><title>PRELUDE-PANE</title>\
<script>document.title = 'PRELUDE-PANE:' + decodeURIComponent(location.hash.slice(1));</script><body style=\"background:#fff\">\n";

/// ペインの窓の題名に出す印(KWin スクリプトはこれで窓を見分ける)
pub fn marker(tag: &str, pane: u64) -> String {
    format!("PRELUDE-PANE:{}", marker_key(tag, pane))
}
fn marker_key(tag: &str, pane: u64) -> String {
    format!("{tag}:{pane}")
}

/// Web のペインを始める。Chromium が無ければ None(Web のペインは止まったものになる)
pub fn start(tag: &str, notes: mpsc::UnboundedSender<Note>) -> Option<Handle> {
    let Some((bin, profile, page)) = locate(tag) else {
        eprintln!("prelude-core: Chromium is not installed. Web panes will not open");
        return None;
    };
    if let Err(e) = std::fs::create_dir_all(page.parent().unwrap_or(Path::new("/"))).and_then(|_| std::fs::write(&page, PANE_PAGE)) {
        eprintln!("prelude-core: cannot write {} ({e})", page.display());
        return None;
    }
    let (tx, rx) = mpsc::unbounded_channel();
    let w = Web {
        tag: tag.to_string(),
        bin,
        profile,
        page,
        tx: tx.clone(),
        notes,
        cdp: None,
        panes: HashMap::new(),
        download_dir: String::new(),
    };
    tokio::spawn(w.run(rx));
    Some(Handle { tx })
}

// ------------------------------------------------------------------ CDP

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

/// CDP の通信の口
struct Cdp {
    out: mpsc::UnboundedSender<Vec<u8>>,
    pending: Pending,
    next: AtomicU64,
    child: tokio::process::Child,
}

impl Cdp {
    /// コマンドを送って返事を待つ(10 秒まで)
    async fn call(&self, method: &str, params: Value, session: Option<&str>) -> Result<Value, String> {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let mut m = json!({ "id": id, "method": method, "params": params });
        if let Some(s) = session {
            m["sessionId"] = json!(s);
        }
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let mut bytes = serde_json::to_vec(&m).map_err(|e| e.to_string())?;
        bytes.push(0);
        self.out.send(bytes).map_err(|_| "Chromium closed".to_string())?;
        match tokio::time::timeout(Duration::from_secs(10), rx).await {
            Ok(Ok(r)) => r,
            _ => {
                self.pending.lock().unwrap().remove(&id);
                Err(format!("{method}: no answer"))
            }
        }
    }
}

/// 子プロセスに渡す fd(fd 3・4 にする前の、閉じないように印を外したもの)
fn pipe() -> std::io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: 2 つの fd を受け取る配列を渡す
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: pipe2 が作った fd で、ほかに持ち主はいない
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// Chromium を CDP の通信の口つきで起動する。events に CDP のイベントを送る
fn launch(bin: &str, profile: &Path, events: mpsc::UnboundedSender<Msg>) -> std::io::Result<Cdp> {
    use std::os::fd::AsRawFd;
    let (cmd_r, cmd_w) = pipe()?; // こちら → Chromium(fd 3)
    let (out_r, out_w) = pipe()?; // Chromium → こちら(fd 4)
    let (a, b) = (cmd_r.as_raw_fd(), out_w.as_raw_fd());
    let mut c = tokio::process::Command::new(bin);
    c.arg("--remote-debugging-pipe")
        .arg(format!("--user-data-dir={}", profile.display()))
        .args(["--no-first-run", "--no-default-browser-check", "--no-startup-window", "--hide-crash-restore-bubble"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0);
    // SAFETY: fork と exec の間で、fd を 3・4 に付け替えるだけ(dup2 は async-signal-safe)。
    // 先に 10 以上へ写してから付け替える(元の fd が 3・4 だったときに上書きしないように)
    unsafe {
        c.pre_exec(move || {
            let a2 = libc::fcntl(a, libc::F_DUPFD, 10);
            let b2 = libc::fcntl(b, libc::F_DUPFD, 10);
            if a2 < 0 || b2 < 0 || libc::dup2(a2, 3) < 0 || libc::dup2(b2, 4) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = c.spawn()?;
    drop((cmd_r, out_w));

    let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
    let (out, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let mut w = tokio::net::unix::pipe::Sender::from_owned_fd(cmd_w)?;
    tokio::spawn(async move {
        while let Some(b) = out_rx.recv().await {
            if w.write_all(&b).await.is_err() {
                break;
            }
        }
    });
    let mut r = tokio::net::unix::pipe::Receiver::from_owned_fd(out_r)?;
    let p = pending.clone();
    tokio::spawn(async move {
        let mut buf = Vec::new();
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            let n = match r.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            buf.extend_from_slice(&chunk[..n]);
            while let Some(i) = buf.iter().position(|b| *b == 0) {
                let line: Vec<u8> = buf.drain(..=i).collect();
                let Ok(m) = serde_json::from_slice::<Value>(&line[..line.len() - 1]) else { continue };
                if let Some(id) = m.get("id").and_then(Value::as_u64) {
                    if let Some(tx) = p.lock().unwrap().remove(&id) {
                        let r = match m.get("error") {
                            Some(e) => Err(e.get("message").and_then(Value::as_str).unwrap_or("error").to_string()),
                            None => Ok(m.get("result").cloned().unwrap_or(Value::Null)),
                        };
                        let _ = tx.send(r);
                    }
                } else if let Some(method) = m.get("method").and_then(Value::as_str) {
                    let session = m.get("sessionId").and_then(Value::as_str).map(String::from);
                    let _ = events.send(Msg::Event(method.to_string(), m.get("params").cloned().unwrap_or(Value::Null), session));
                }
            }
        }
        let _ = events.send(Msg::Closed);
    });
    Ok(Cdp { out, pending, next: AtomicU64::new(0), child })
}

// ------------------------------------------------------------------ ペイン

struct Pane {
    /// 開きたい URL(コアの状態)
    url: String,
    /// 最後に Chromium へ頼んだ URL
    asked: String,
    /// Chromium が知らせた今の URL
    now: String,
    target: Option<String>,
    session: Option<String>,
    claimed: bool,
}

struct Web {
    tag: String,
    bin: String,
    profile: PathBuf,
    page: PathBuf,
    tx: mpsc::UnboundedSender<Msg>,
    notes: mpsc::UnboundedSender<Note>,
    cdp: Option<Cdp>,
    panes: HashMap<u64, Pane>,
    download_dir: String,
}

impl Web {
    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Msg>) {
        while let Some(m) = rx.recv().await {
            match m {
                Msg::Sync(w, dl) => self.sync(w, dl).await,
                Msg::Claimed(p) => {
                    if let Some(x) = self.panes.get_mut(&p) {
                        x.claimed = true;
                    }
                    self.navigate(p).await;
                }
                Msg::Go(p, g) => self.go(p, g).await,
                Msg::Event(method, params, session) => self.event(&method, &params, session).await,
                Msg::Closed => {
                    // Chromium が終わった(落ちた・使用者が全部閉じて終わった)。窓が消えるので、ペインは取り込みの側から止まったものになる
                    eprintln!("prelude-core: Chromium exited");
                    self.cdp = None;
                    for p in self.panes.values_mut() {
                        (p.target, p.session, p.claimed, p.asked) = (None, None, false, String::new());
                    }
                    self.panes.clear();
                }
                Msg::Shutdown(done) => {
                    self.shutdown().await;
                    let _ = done.send(());
                    return;
                }
            }
        }
    }

    /// Chromium が動いていなければ起動する
    async fn ensure(&mut self) -> bool {
        if self.cdp.is_some() {
            return true;
        }
        let cdp = match launch(&self.bin, &self.profile, self.tx.clone()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("prelude-core: cannot start Chromium ({e})");
                return false;
            }
        };
        if let Err(e) = cdp.call("Target.setDiscoverTargets", json!({ "discover": true }), None).await {
            eprintln!("prelude-core: Chromium does not answer ({e})");
            return false;
        }
        self.cdp = Some(cdp);
        self.set_download_dir().await;
        true
    }

    async fn sync(&mut self, wants: Vec<WebWant>, dl: String) {
        if dl != self.download_dir {
            self.download_dir = dl;
            self.set_download_dir().await;
        }
        let ids: Vec<u64> = wants.iter().map(|w| w.pane).collect();
        for id in self.panes.keys().copied().filter(|id| !ids.contains(id)).collect::<Vec<_>>() {
            let p = self.panes.remove(&id);
            if let (Some(c), Some(t)) = (&self.cdp, p.and_then(|p| p.target)) {
                let _ = c.call("Target.closeTarget", json!({ "targetId": t }), None).await;
            }
        }
        for w in wants {
            match self.panes.get_mut(&w.pane) {
                Some(p) => {
                    if p.url != w.url {
                        p.url = w.url;
                        self.navigate(w.pane).await;
                    }
                }
                None => {
                    if !self.ensure().await {
                        continue;
                    }
                    self.panes.insert(
                        w.pane,
                        Pane { url: w.url, asked: String::new(), now: String::new(), target: None, session: None, claimed: false },
                    );
                    self.open_window(w.pane);
                }
            }
        }
    }

    /// アプリの窓を作る(動いている Chromium に頼む。頼んだプロセスはすぐ終わる)
    fn open_window(&self, pane: u64) {
        let url = format!("file://{}#{}", self.page.display(), marker_key(&self.tag, pane));
        let mut c = tokio::process::Command::new(&self.bin);
        c.arg(format!("--user-data-dir={}", self.profile.display())).arg(format!("--app={url}"));
        c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).process_group(0);
        match c.spawn() {
            Ok(mut child) => {
                tokio::spawn(async move {
                    let _ = child.wait().await;
                });
            }
            Err(e) => eprintln!("prelude-core: cannot open a Chromium window ({e})"),
        }
    }

    /// 取り込んだペインを、開きたい URL へ移す(今の URL と違い、まだ頼んでいなければ)
    async fn navigate(&mut self, pane: u64) {
        let Some(p) = self.panes.get_mut(&pane) else { return };
        let (Some(session), true, Some(c)) = (p.session.clone(), p.claimed, &self.cdp) else { return };
        if p.url.is_empty() || p.url == p.now || p.url == p.asked {
            return;
        }
        p.asked = p.url.clone();
        let url = p.url.clone();
        if let Err(e) = c.call("Page.navigate", json!({ "url": url }), Some(&session)).await {
            eprintln!("prelude-core: cannot open {url} ({e})");
        }
    }

    async fn go(&mut self, pane: u64, g: Go) {
        let (Some(c), Some(session)) = (&self.cdp, self.panes.get(&pane).and_then(|p| p.session.clone())) else { return };
        if g == Go::Reload {
            let _ = c.call("Page.reload", json!({}), Some(&session)).await;
            return;
        }
        let Ok(h) = c.call("Page.getNavigationHistory", json!({}), Some(&session)).await else { return };
        let cur = h["currentIndex"].as_i64().unwrap_or(0);
        let i = cur + if g == Go::Back { -1 } else { 1 };
        if let Some(e) = h["entries"].as_array().and_then(|a| a.get(i as usize)).filter(|_| i >= 0) {
            // 最初の印のページには戻らない
            if !e["url"].as_str().unwrap_or("").starts_with("file://") {
                let _ = c.call("Page.navigateToHistoryEntry", json!({ "entryId": e["id"] }), Some(&session)).await;
            }
        }
    }

    fn pane_of_target(&self, target: &str) -> Option<u64> {
        self.panes.iter().find(|(_, p)| p.target.as_deref() == Some(target)).map(|(id, _)| *id)
    }

    async fn event(&mut self, method: &str, params: &Value, _session: Option<String>) {
        match method {
            "Target.targetCreated" => {
                let ti = &params["targetInfo"];
                if ti["type"] != "page" {
                    return;
                }
                let url = ti["url"].as_str().unwrap_or("");
                let target = ti["targetId"].as_str().unwrap_or("").to_string();
                // 印のページ = 頼んだペインの窓
                let prefix = format!("file://{}#", self.page.display());
                if let Some(key) = url.strip_prefix(&prefix) {
                    let pane = key.rsplit(':').next().and_then(|s| s.parse::<u64>().ok());
                    let Some(pane) = pane.filter(|p| key == marker_key(&self.tag, *p) && self.panes.contains_key(p)) else {
                        self.close_target(&target).await; // もう要らないペインの窓
                        return;
                    };
                    let Some(c) = &self.cdp else { return };
                    let session = c.call("Target.attachToTarget", json!({ "targetId": target, "flatten": true }), None).await;
                    if let Some(p) = self.panes.get_mut(&pane) {
                        p.target = Some(target);
                        p.session = session.ok().and_then(|s| s["sessionId"].as_str().map(String::from));
                    }
                    self.navigate(pane).await;
                    return;
                }
                // ペインのページから開いた新しい窓
                let opener = ti["openerId"].as_str().and_then(|o| self.pane_of_target(o));
                if let Some(from) = opener {
                    if ti["canAccessOpener"] == true {
                        return; // opener を使う小さな窓(ログインのポップアップなど)はそのまま
                    }
                    // 新しいタブで開くリンク: PRELUDE の新しいタブにする。URL が決まるまで待つ(まだ about:blank のことがある)
                    let url = if url.is_empty() || url == "about:blank" { self.wait_url(&target).await } else { url.to_string() };
                    self.close_target(&target).await;
                    if !url.is_empty() {
                        let _ = self.notes.send(Note::OpenTab { url, from });
                    }
                }
            }
            "Target.targetInfoChanged" => {
                let ti = &params["targetInfo"];
                let Some(pane) = ti["targetId"].as_str().and_then(|t| self.pane_of_target(t)) else { return };
                let url = ti["url"].as_str().unwrap_or("").to_string();
                let title = ti["title"].as_str().unwrap_or("").to_string();
                if url.starts_with("file://") {
                    return; // 印のページ
                }
                if let Some(p) = self.panes.get_mut(&pane) {
                    p.now = url.clone();
                    p.url = url.clone(); // ページの中で移った先を、開きたい URL にする(コアにも知らせる)
                }
                let _ = self.notes.send(Note::Web { pane, url, title });
            }
            "Browser.downloadWillBegin" | "Browser.downloadProgress" => {
                let _ = self.notes.send(Note::Download(params.clone()));
            }
            _ => {}
        }
    }

    /// 新しいページの URL が決まるのを待つ(1 秒まで)
    async fn wait_url(&self, target: &str) -> String {
        let Some(c) = &self.cdp else { return String::new() };
        for _ in 0..10 {
            if let Ok(r) = c.call("Target.getTargetInfo", json!({ "targetId": target }), None).await {
                let u = r["targetInfo"]["url"].as_str().unwrap_or("");
                if !u.is_empty() && u != "about:blank" {
                    return u.to_string();
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        String::new()
    }

    async fn close_target(&self, target: &str) {
        if let Some(c) = &self.cdp {
            let _ = c.call("Target.closeTarget", json!({ "targetId": target }), None).await;
        }
    }

    async fn set_download_dir(&self) {
        let Some(c) = &self.cdp else { return };
        let dir = if self.download_dir.is_empty() { None } else { Some(self.download_dir.clone()) };
        let mut p = json!({ "behavior": "allow", "eventsEnabled": true });
        if let Some(d) = dir {
            p["downloadPath"] = json!(d);
        }
        if let Err(e) = c.call("Browser.setDownloadBehavior", p, None).await {
            eprintln!("prelude-core: cannot set the download folder ({e})");
        }
    }

    /// 終わる: ペインの窓を閉じてから、CDP の通信の口を閉じる(Chromium は口が閉じると自分で終わる)。終わるのを 3 秒まで待つ。
    /// Browser.close は使わない(snap 版では落ちた扱いになり、次の起動で「復元しますか」が出る)
    async fn shutdown(&mut self) {
        let Some(c) = self.cdp.take() else { return };
        for p in self.panes.values() {
            if let Some(t) = &p.target {
                let _ = c.call("Target.closeTarget", json!({ "targetId": t }), None).await;
            }
        }
        let Cdp { out, mut child, .. } = c;
        drop(out);
        if tokio::time::timeout(Duration::from_secs(3), child.wait()).await.is_err() {
            eprintln!("prelude-core: Chromium did not exit");
        }
    }
}

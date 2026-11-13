//! Web のペイン(docs/redesign-rust.md の「Web のペイン」)。Chromium を PRELUDE 専用のプロファイルで 1 つだけ動かし、小さな拡張機能とやり取りする。
//!
//! - CDP は使わない。`--remote-debugging-pipe` で起動すると `navigator.webdriver` が true になり、Google のログインに拒まれる
//! - 拡張機能(`extension/`)は事実を知らせて頼まれたことをするだけで、判断はここでする([`decide`])。起動のときにファイルへ書き出す
//! - 拡張機能とは native messaging。Chromium が起動した `prelude-core --web-host`(webhost.rs)が Unix ソケットへつなぐ
//! - Web のペイン 1 つ = Chromium のアプリの窓 1 つ(アドレスバー・タブなし)。`--app=file:///…/prelude-pane.html#<印>` で作る。
//!   その小さなページが題名を `PRELUDE-PANE:<印>` にするので、コンポジタの裏側はその題名で窓を見分けて取り込む。取り込んだら本当の URL へ移す
//! - URL・題名は拡張機能が知らせる(コアはペインの url を書き換えて残す。起動し直すとその URL で開く)
//! - 新しい窓: タブ付きの普通の窓に出たページは閉じて PRELUDE の新しいタブにする。ポップアップ(ログインなど)はそのまま
//! - ダウンロードの保存先は Chromium に絶対パスで頼めないので、終わってからコアが paths の download へ移す
//! - Chromium は窓が無くなると終わる。次のペインで起動し直す。起動したプロセスは、同じプロファイルの Chromium が既に動いていると
//!   頼むだけで終わるので、本体はプロファイルの `SingletonLock`(`<ホスト名>-<pid>` へのリンク)で見分ける
use crate::compositor::Note;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{mpsc, oneshot};

/// Web のペイン(コアの状態から作る)
#[derive(Debug, Clone, PartialEq)]
pub struct WebWant {
    pub pane: u64,
    pub url: String,
    /// 音を消すか
    pub muted: bool,
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
    /// 拡張機能がつながった。ここへ書くと拡張機能へ届く
    Connected(mpsc::UnboundedSender<String>),
    /// 拡張機能からの知らせ
    Event(Value),
    /// Chromium が終わった
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
    /// ペインの窓をコンポジタの裏側が取り込んだ(本当の URL へ移してよい)
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

/// Chromium 系のブラウザ(好きな順)。PATH から探す(Electron 版の appembed.js と同じ)
const BROWSERS: [&str; 4] = ["chromium", "chromium-browser", "google-chrome-stable", "google-chrome"];

/// snap 版か(/snap/bin/chromium か、その先が /snap/ の中)
fn is_snap(bin: &Path) -> bool {
    bin.starts_with("/snap/") || std::fs::canonicalize(bin).is_ok_and(|p| p.starts_with("/snap/"))
}

/// 使う Chromium と、プロファイル・印を出すページの置き場所。snap 版は ~/snap/chromium/common の外の隠しフォルダに書けないので、そこに置く。
/// tag の付いたコア(試すとき)は、ふだんのプロファイルと取り合わないように別のプロファイルを使う。
/// /snap/bin は systemd のユーザーのサービスの PATH に無いことがあるので足して探す
fn locate(tag: &str) -> Option<(String, PathBuf, PathBuf)> {
    let bin = crate::xdg::which(&BROWSERS, &["/snap/bin"])?;
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let snap = is_snap(&bin);
    let base = if snap { home.join("snap/chromium/common") } else { home.join(".config/PRELUDE-electron") };
    let profile = if snap { base.join(format!("prelude-profile{tag}")) } else { base.join(format!("browser-profile{tag}")) };
    Some((bin.to_string_lossy().into_owned(), profile, base.join("prelude-pane.html")))
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

/// 拡張機能の固定の id(manifest.json の key から決まる)
const EXTENSION_ID: &str = "gopcccjkfpojcemicmiipmkbfeeeefil";
const EXTENSION_MANIFEST: &str = include_str!("../extension/manifest.json");
const EXTENSION_BG: &str = include_str!("../extension/bg.js");
const HOST_NAME: &str = "org.prelude.web";

/// 拡張機能と、native messaging の宣言を書き出す。`--web-host` のパスは版で変わるので、起動のたびに書く
fn install_extension(dir: &Path, profile: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("manifest.json"), EXTENSION_MANIFEST)?;
    std::fs::write(dir.join("bg.js"), EXTENSION_BG)?;
    let exe = std::env::current_exe()?;
    let host = dir.join("web-host");
    std::fs::write(&host, format!("#!/bin/sh\nexec '{}' --web-host\n", exe.display()))?;
    std::fs::set_permissions(&host, std::fs::Permissions::from_mode(0o755))?;
    let hosts = profile.join("NativeMessagingHosts");
    std::fs::create_dir_all(&hosts)?;
    let manifest = json!({
        "name": HOST_NAME,
        "description": "PRELUDE",
        "path": host,
        "type": "stdio",
        "allowed_origins": [format!("chrome-extension://{EXTENSION_ID}/")],
    });
    std::fs::write(hosts.join(format!("{HOST_NAME}.json")), manifest.to_string())
}

/// `SingletonLock` のリンク先から本体の pid を読む
pub fn singleton_pid(link: &str) -> Option<u32> {
    link.rsplit('-').next()?.parse().ok()
}

/// 動いている Chromium の本体の pid
fn browser_pid(profile: &Path) -> Option<u32> {
    let link = std::fs::read_link(profile.join("SingletonLock")).ok()?;
    singleton_pid(&link.to_string_lossy()).filter(|p| crate::procs::alive(*p))
}

/// 拡張機能が知らせたタブの様子
#[derive(Debug, Clone, PartialEq)]
pub struct TabNote {
    pub tab: i64,
    pub url: String,
    pub title: String,
    /// 窓の種類(`app`・`normal`・`popup`)
    pub win: String,
}

/// タブの知らせへの返事
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// 印のページ: このタブはペインの窓
    Pane(u64),
    /// もう要らないペインの窓、印の違う窓: 閉じる
    Close,
    /// ペインのタブの URL・題名が変わった
    Update(u64),
    /// タブ付きの窓に出たページ: 閉じて、PRELUDE の新しいタブにする
    Capture(String),
    /// 何もしない(URL がまだ決まっていない・ポップアップ・chrome:// など)
    Ignore,
}

/// タブの知らせをどう扱うか。owner はこのタブのペイン(分かっていれば)
pub fn decide(t: &TabNote, page: &str, tag: &str, owner: Option<u64>, live: &dyn Fn(u64) -> bool) -> Decision {
    if let Some(key) = t.url.strip_prefix(&format!("file://{page}#")) {
        let pane = key.rsplit(':').next().and_then(|s| s.parse::<u64>().ok());
        return match pane.filter(|p| key == marker_key(tag, *p) && live(*p)) {
            Some(p) => Decision::Pane(p),
            None if owner.is_some() => Decision::Ignore, // 戻って印のページに来た
            None => Decision::Close,
        };
    }
    if let Some(p) = owner {
        return if t.url.is_empty() || t.url.starts_with("file://") { Decision::Ignore } else { Decision::Update(p) };
    }
    if t.win == "normal" && (t.url.starts_with("http://") || t.url.starts_with("https://")) {
        return Decision::Capture(t.url.clone());
    }
    Decision::Ignore
}

/// ダウンロードの移し先。同じ名前があれば「名前 (1)」にする。移さなくてよければ None
pub fn download_target(from: &Path, dir: &str) -> Option<PathBuf> {
    if dir.is_empty() || from.parent() == Some(Path::new(dir)) {
        return None;
    }
    let name = from.file_name()?;
    let first = Path::new(dir).join(name);
    if !first.exists() {
        return Some(first);
    }
    let (stem, ext) =
        (from.file_stem()?.to_string_lossy(), from.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default());
    (1..1000).map(|i| Path::new(dir).join(format!("{stem} ({i}){ext}"))).find(|p| !p.exists())
}

/// Web のペインを始める。Chromium が無ければ None(Web のペインは止まったものになる。Chromium はパッケージが入れるので、それ以上の扱いはしない。「Web のペイン」)。
/// socket は拡張機能がつなぐ Unix ソケット
pub fn start(tag: &str, socket: PathBuf, notes: mpsc::UnboundedSender<Note>) -> Option<Handle> {
    let Some((bin, profile, page)) = locate(tag) else {
        eprintln!("prelude-core: Chromium is not installed. Web panes will not open");
        return None;
    };
    let base = page.parent().unwrap_or(Path::new("/")).to_path_buf();
    let extension = base.join(format!("prelude-extension{tag}"));
    let ready =
        std::fs::create_dir_all(&base).and_then(|_| std::fs::write(&page, PANE_PAGE)).and_then(|_| install_extension(&extension, &profile));
    if let Err(e) = ready {
        eprintln!("prelude-core: cannot prepare Chromium ({e})");
        return None;
    }
    let _ = std::fs::remove_file(&socket);
    let listener = match UnixListener::bind(&socket) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("prelude-core: cannot listen on {} ({e})", socket.display());
            return None;
        }
    };
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(accept(listener, tx.clone()));
    let w = Web {
        tag: tag.to_string(),
        bin,
        profile,
        page,
        extension,
        socket,
        tx: tx.clone(),
        notes,
        chromium: None,
        out: None,
        panes: HashMap::new(),
        captured: HashSet::new(),
        moved: HashSet::new(),
        download_dir: String::new(),
    };
    tokio::spawn(w.run(rx));
    Some(Handle { tx })
}

/// 拡張機能の橋渡しのつながりを受ける。つなぎ直したら新しいつながりを使う
async fn accept(listener: UnixListener, tx: mpsc::UnboundedSender<Msg>) {
    while let Ok((sock, _)) = listener.accept().await {
        let (r, mut w) = sock.into_split();
        let (out, mut out_rx) = mpsc::unbounded_channel::<String>();
        if tx.send(Msg::Connected(out)).is_err() {
            return;
        }
        tokio::spawn(async move {
            while let Some(line) = out_rx.recv().await {
                if w.write_all(line.as_bytes()).await.is_err() || w.write_all(b"\n").await.is_err() {
                    break;
                }
            }
        });
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(r).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                if let Ok(v) = serde_json::from_str::<Value>(&l) {
                    if tx.send(Msg::Event(v)).is_err() {
                        break;
                    }
                }
            }
        });
    }
}

// ------------------------------------------------------------------ ペイン

struct Pane {
    /// 開きたい URL(コアの状態)
    url: String,
    /// 最後に Chromium へ頼んだ URL
    asked: String,
    /// Chromium が知らせた今の URL
    now: String,
    tab: Option<i64>,
    claimed: bool,
    /// 音を消すか(タブが分かったら、そのたびに頼む)
    muted: bool,
}

struct Web {
    tag: String,
    bin: String,
    profile: PathBuf,
    page: PathBuf,
    extension: PathBuf,
    socket: PathBuf,
    tx: mpsc::UnboundedSender<Msg>,
    notes: mpsc::UnboundedSender<Note>,
    /// 動いている Chromium の pid
    chromium: Option<u32>,
    /// 拡張機能へ書く口
    out: Option<mpsc::UnboundedSender<String>>,
    panes: HashMap<u64, Pane>,
    /// 閉じると決めたタブ(閉じるまでに知らせがまた来ても、2 度タブにしない)
    captured: HashSet<i64>,
    /// 移し終えたダウンロード
    moved: HashSet<i64>,
    download_dir: String,
}

impl Web {
    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Msg>) {
        while let Some(m) = rx.recv().await {
            match m {
                Msg::Sync(w, dl) => self.sync(w, dl),
                Msg::Claimed(p) => {
                    if let Some(x) = self.panes.get_mut(&p) {
                        x.claimed = true;
                    }
                    self.navigate(p);
                }
                Msg::Go(p, g) => self.go(p, g),
                Msg::Connected(out) => self.out = Some(out),
                Msg::Event(v) => self.event(&v),
                Msg::Closed => {
                    // Chromium が終わった(使用者が全部閉じた・落ちた)。窓が消えるので、ペインは取り込みの側から閉じられる
                    eprintln!("prelude-core: Chromium exited");
                    self.chromium = None;
                    self.out = None;
                    self.panes.clear();
                    self.captured.clear();
                }
                Msg::Shutdown(done) => {
                    self.shutdown().await;
                    let _ = done.send(());
                    return;
                }
            }
        }
    }

    fn tell(&self, m: Value) {
        if let Some(o) = &self.out {
            let _ = o.send(m.to_string());
        }
    }

    fn sync(&mut self, wants: Vec<WebWant>, dl: String) {
        self.download_dir = dl;
        let ids: Vec<u64> = wants.iter().map(|w| w.pane).collect();
        for id in self.panes.keys().copied().filter(|id| !ids.contains(id)).collect::<Vec<_>>() {
            if let Some(t) = self.panes.remove(&id).and_then(|p| p.tab) {
                self.tell(json!({ "cmd": "close", "tab": t }));
            }
        }
        for w in wants {
            match self.panes.get_mut(&w.pane) {
                Some(p) => {
                    let mute = p.muted != w.muted;
                    p.muted = w.muted;
                    if p.url != w.url {
                        p.url = w.url;
                        self.navigate(w.pane);
                    }
                    if mute {
                        self.apply_mute(w.pane);
                    }
                }
                None => {
                    let p = Pane { url: w.url, asked: String::new(), now: String::new(), tab: None, claimed: false, muted: w.muted };
                    self.panes.insert(w.pane, p);
                    self.open_window(w.pane);
                }
            }
        }
    }

    /// アプリの窓を作る。Chromium が動いていなければ、これで起動する。動いていれば頼むだけで、このプロセスはすぐ終わる
    fn open_window(&mut self, pane: u64) {
        let url = format!("file://{}#{}", self.page.display(), marker_key(&self.tag, pane));
        let mut c = tokio::process::Command::new(&self.bin);
        c.arg(format!("--user-data-dir={}", self.profile.display()))
            .arg(format!("--load-extension={}", self.extension.display()))
            .args(["--no-first-run", "--no-default-browser-check", "--hide-crash-restore-bubble"])
            .arg(format!("--app={url}"))
            .env("PRELUDE_WEB_SOCK", &self.socket)
            .env_remove("CHROME_DESKTOP")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .process_group(0);
        let first = self.chromium.is_none();
        match c.spawn() {
            Ok(mut child) => {
                if first {
                    self.chromium = child.id();
                }
                let (tx, profile) = (self.tx.clone(), self.profile.clone());
                tokio::spawn(async move {
                    let _ = child.wait().await;
                    if !first {
                        return;
                    }
                    // 既に動いている本体へ頼んだだけなら、本体が終わるまで見張る
                    while browser_pid(&profile).is_some() {
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                    let _ = tx.send(Msg::Closed);
                });
            }
            Err(e) => eprintln!("prelude-core: cannot open a Chromium window ({e})"),
        }
    }

    /// 取り込んだペインを、開きたい URL へ移す(今の URL と違い、まだ頼んでいなければ)
    fn navigate(&mut self, pane: u64) {
        let Some(p) = self.panes.get_mut(&pane) else { return };
        let (Some(tab), true) = (p.tab, p.claimed) else { return };
        if p.url.is_empty() || p.url == p.now || p.url == p.asked {
            return;
        }
        p.asked = p.url.clone();
        let url = p.url.clone();
        self.tell(json!({ "cmd": "go", "tab": tab, "url": url }));
    }

    /// ペインのタブの消音を、コアの状態に合わせる(タブが分かっていれば)
    fn apply_mute(&self, pane: u64) {
        if let Some((tab, on)) = self.panes.get(&pane).and_then(|p| Some((p.tab?, p.muted))) {
            self.tell(json!({ "cmd": "mute", "tab": tab, "on": on }));
        }
    }

    /// 拡張機能が送ったファビコン(PNG の base64)を保存して、コアに知らせる
    fn icon(&self, v: &Value) {
        let tab = v["tab"].as_i64().unwrap_or(0);
        let Some(pane) = self.panes.iter().find(|(_, p)| p.tab == Some(tab)).map(|(id, _)| *id) else { return };
        let Some(png) = v["data"].as_str().and_then(base64_decode).filter(|b| b.starts_with(b"\x89PNG")) else { return };
        let dir = favicon_dir();
        // 中身が変わったら別のファイル名にする(同じパスのままだと、UI は読み直さない)
        let path = dir.join(format!("{}-{:08x}.png", favicon_name(v["url"].as_str().unwrap_or(""), tab), fnv32(&png)));
        if std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, png)).is_ok() {
            let _ = self.notes.send(Note::Icon { pane, path: path.to_string_lossy().into_owned() });
        }
    }

    fn go(&self, pane: u64, g: Go) {
        let Some(tab) = self.panes.get(&pane).and_then(|p| p.tab) else { return };
        let cmd = match g {
            Go::Back => "back",
            Go::Forward => "forward",
            Go::Reload => "reload",
        };
        self.tell(json!({ "cmd": cmd, "tab": tab }));
    }

    fn event(&mut self, v: &Value) {
        match v["ev"].as_str().unwrap_or("") {
            "tab" => self.tab_note(v),
            "removed" => {
                let tab = v["tab"].as_i64().unwrap_or(0);
                self.captured.remove(&tab);
                for p in self.panes.values_mut().filter(|p| p.tab == Some(tab)) {
                    p.tab = None;
                }
            }
            "download" => self.download(v),
            "icon" => self.icon(v),
            "error" => eprintln!("prelude-core: browser extension: {v}"),
            _ => {}
        }
    }

    fn tab_note(&mut self, v: &Value) {
        let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
        let t = TabNote { tab: v["tab"].as_i64().unwrap_or(0), url: s("url"), title: s("title"), win: s("win") };
        let owner = self.panes.iter().find(|(_, p)| p.tab == Some(t.tab)).map(|(id, _)| *id);
        let page = self.page.to_string_lossy().into_owned();
        let live = |p: u64| self.panes.contains_key(&p);
        match decide(&t, &page, &self.tag, owner, &live) {
            Decision::Pane(pane) => {
                if let Some(p) = self.panes.get_mut(&pane) {
                    p.tab = Some(t.tab);
                }
                self.navigate(pane);
                if self.panes.get(&pane).is_some_and(|p| p.muted) {
                    self.apply_mute(pane);
                }
            }
            Decision::Close => self.tell(json!({ "cmd": "close", "tab": t.tab })),
            Decision::Update(pane) => {
                if let Some(p) = self.panes.get_mut(&pane) {
                    p.now = t.url.clone();
                    p.url = t.url.clone(); // ページの中で移った先を、開きたい URL にする(コアにも知らせる)
                }
                let _ = self.notes.send(Note::Web { pane, url: t.url, title: t.title });
            }
            Decision::Capture(url) => {
                if self.captured.insert(t.tab) {
                    self.tell(json!({ "cmd": "close", "tab": t.tab }));
                    let _ = self.notes.send(Note::OpenTab { url, from: 0 });
                }
            }
            Decision::Ignore => {}
        }
    }

    /// 進み具合を配り、終わったら paths の download へ移す
    fn download(&mut self, v: &Value) {
        let _ = self.notes.send(Note::Download(v.clone()));
        let (id, file) = (v["id"].as_i64().unwrap_or(0), v["filename"].as_str().unwrap_or(""));
        if v["state"] != "complete" || file.is_empty() || !self.moved.insert(id) {
            return;
        }
        let from = Path::new(file);
        let Some(to) = download_target(from, &self.download_dir) else { return };
        let moved = std::fs::rename(from, &to).or_else(|_| std::fs::copy(from, &to).and_then(|_| std::fs::remove_file(from)));
        if let Err(e) = moved {
            eprintln!("prelude-core: cannot move {} to {} ({e})", from.display(), to.display());
        }
    }

    /// 終わる: ペインの窓を閉じる(窓が無くなると Chromium は終わる)。3 秒まで待ち、残っていれば終わらせる
    async fn shutdown(&mut self) {
        if self.chromium.take().is_none() {
            return;
        }
        let Some(pid) = browser_pid(&self.profile) else { return };
        for p in self.panes.values() {
            if let Some(t) = p.tab {
                self.tell(json!({ "cmd": "close", "tab": t }));
            }
        }
        let end = std::time::Instant::now() + Duration::from_secs(3);
        while crate::procs::alive(pid) && std::time::Instant::now() < end {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if crate::procs::alive(pid) {
            crate::procs::signal(pid, libc::SIGTERM);
        }
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// ファビコンを置くフォルダ(`$XDG_CACHE_HOME/prelude/favicons`)
fn favicon_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    let cache = std::env::var("XDG_CACHE_HOME").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| format!("{home}/.cache"));
    PathBuf::from(cache).join("prelude/favicons")
}

/// ファビコンのファイル名: URL のホスト(使える字だけ)。無ければタブの番号
fn favicon_name(url: &str, tab: i64) -> String {
    let host = url.split("://").nth(1).unwrap_or("").split(['/', '?', '#']).next().unwrap_or("");
    let host: String = host.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')).take(100).collect();
    if host.is_empty() || host.starts_with('.') {
        format!("tab-{tab}")
    } else {
        host
    }
}

/// FNV-1a(32 ビット)。ファビコンのファイル名に中身を表す印を付ける
fn fnv32(b: &[u8]) -> u32 {
    b.iter().fold(0x811c9dc5u32, |h, x| (h ^ *x as u32).wrapping_mul(0x01000193))
}

/// base64(標準の字。= の埋め・改行は無視)を戻す。字が合わなければ None
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes().filter(|c| !matches!(c, b'=' | b'\n' | b'\r')) {
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ファビコンの名前と_base64() {
        assert_eq!(favicon_name("https://www.example.com:8080/a?b", 3), "www.example.com8080");
        assert_eq!(favicon_name("https://../etc/passwd", 3), "tab-3");
        assert_eq!(favicon_name("", 4), "tab-4");
        assert_eq!(base64_decode("aGVsbG8gd29ybGQ="), Some(b"hello world".to_vec()));
        assert_eq!(base64_decode("iVBORw0K\nGgo="), Some(b"\x89PNG\r\n\x1a\n".to_vec()));
        assert_eq!(base64_decode("@@"), None);
    }

    const PAGE: &str = "/c/prelude-pane.html";

    fn t(url: &str, win: &str) -> TabNote {
        TabNote { tab: 1, url: url.into(), title: String::new(), win: win.into() }
    }
    fn d(n: &TabNote, owner: Option<u64>) -> Decision {
        decide(n, PAGE, ".d1", owner, &|p| p == 5)
    }

    #[test]
    fn 本体の_pid_を読む() {
        assert_eq!(singleton_pid("ukeiyamada-cflv11-12345"), Some(12345));
        assert_eq!(singleton_pid("host-1"), Some(1));
        assert_eq!(singleton_pid("host"), None);
    }

    #[test]
    fn 印のページはペインの窓() {
        assert_eq!(d(&t("file:///c/prelude-pane.html#.d1:5", "app"), None), Decision::Pane(5));
        // もう要らないペイン・印の違うコアのもの
        assert_eq!(d(&t("file:///c/prelude-pane.html#.d1:6", "app"), None), Decision::Close);
        assert_eq!(d(&t("file:///c/prelude-pane.html#.d2:5", "app"), None), Decision::Close);
        // 戻って印のページに来たペインは閉じない
        assert_eq!(d(&t("file:///c/prelude-pane.html#.d2:5", "app"), Some(5)), Decision::Ignore);
    }

    #[test]
    fn ペインのタブは_url_を知らせる() {
        assert_eq!(d(&t("https://a/", "app"), Some(5)), Decision::Update(5));
        assert_eq!(d(&t("", "app"), Some(5)), Decision::Ignore);
        assert_eq!(d(&t("file:///x", "app"), Some(5)), Decision::Ignore);
    }

    #[test]
    fn タブ付きの窓に出たページだけ捕まえる() {
        assert_eq!(d(&t("https://a/b", "normal"), None), Decision::Capture("https://a/b".into()));
        assert_eq!(d(&t("http://a/", "normal"), None), Decision::Capture("http://a/".into()));
        // URL がまだ決まっていない
        assert_eq!(d(&t("", "normal"), None), Decision::Ignore);
        assert_eq!(d(&t("about:blank", "normal"), None), Decision::Ignore);
        // ログインのポップアップ・Chromium の設定はそのまま
        assert_eq!(d(&t("https://accounts.google.com/", "popup"), None), Decision::Ignore);
        assert_eq!(d(&t("chrome://settings/", "normal"), None), Decision::Ignore);
    }

    #[test]
    fn ダウンロードの移し先() {
        let d = std::env::temp_dir().join(format!("prelude-dl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let dir = d.to_str().unwrap();
        let from = Path::new("/home/u/Downloads/a.pdf");
        assert_eq!(download_target(from, ""), None);
        assert_eq!(download_target(from, "/home/u/Downloads"), None);
        assert_eq!(download_target(from, dir), Some(d.join("a.pdf")));
        std::fs::write(d.join("a.pdf"), "x").unwrap();
        assert_eq!(download_target(from, dir), Some(d.join("a (1).pdf")));
        std::fs::remove_dir_all(&d).unwrap();
    }
}

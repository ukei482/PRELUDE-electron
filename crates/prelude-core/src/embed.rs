//! アプリの窓の取り込み(KDE Plasma 6 の Wayland。Electron 版の appembed.js の移植)。
//!
//! - コアの状態から「取り込むペイン」([`Want`]: 起動コマンドと場所)を受け取り、今の取り込みとの差を埋める(起動する・閉じる・置き直す)
//! - KWin スクリプト(embed.js)を、状態を埋め込んで読み直す。スクリプトからの報告(どの窓をどのペインのものにしたか・窓の数・題名)は
//!   D-Bus の `org.prelude.Core`(印つき)の `Report` で受ける
//! - 窓が無くなったペインは [`Note::Exited`] で知らせる(コアはペインを残して止まったものにする)
//! - 終わるときは、起動したアプリに終了の合図を送り、閉じるのを少し待つ。落ちたときに備えて、取り込み中の窓の id をファイルに控え、次の起動で普通の窓に戻す
use crate::{apps, procs};
use prelude_proto::PaneRect;
use serde::Deserialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

/// 取り込むペイン(コアの状態から作る)
#[derive(Debug, Clone, PartialEq)]
pub struct Want {
    pub pane: u64,
    /// シェルのコマンド
    pub cmd: String,
    /// 窓を見分ける名前(省略できる)
    pub cls: String,
    /// 作業フォルダ
    pub cwd: String,
    /// 見えていれば場所
    pub rect: Option<PaneRect>,
    /// 窓の題名の印(Web のペイン)。あれば起動はせず、この題名の窓を取り込む(窓は web.rs が作る)
    pub marker: String,
}

/// 取り込みからコアへの知らせ
#[derive(Debug, Clone, PartialEq)]
pub enum Note {
    /// ペインの窓が無くなった(had_window: 一度は窓があった)
    Exited { pane: u64, had_window: bool },
    /// ペインの本体の窓の題名
    Title { pane: u64, title: String },
    /// ペインの窓を取り込んだ
    Claimed { pane: u64 },
    /// Web のペインの今の URL と題名(web.rs から)
    Web { pane: u64, url: String, title: String },
    /// Web のペインから新しいタブで開くリンク(web.rs から)
    OpenTab { url: String, from: u64 },
    /// ダウンロードの始まり・進み具合(CDP の Browser.downloadWillBegin / downloadProgress の params)
    Download(serde_json::Value),
}

enum Msg {
    Want(Vec<Want>),
    Report(String, String),
    ShExited(u64, u32),
    CheckAdopt(u64),
    /// 読み直す(少し後に読み直したいとき)
    Reload,
    Shutdown(oneshot::Sender<()>),
}

/// 取り込みに頼む口
#[derive(Clone)]
pub struct Handle {
    tx: mpsc::UnboundedSender<Msg>,
}

impl Handle {
    pub fn want(&self, w: Vec<Want>) {
        let _ = self.tx.send(Msg::Want(w));
    }
    /// 起動したアプリを閉じ、スクリプトを外す。終わるまで待つ
    pub async fn shutdown(&self) {
        let (tx, rx) = oneshot::channel();
        if self.tx.send(Msg::Shutdown(tx)).is_ok() {
            let _ = rx.await;
        }
    }
}

/// この環境で取り込めるか(KDE の Wayland)
pub fn supported() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.to_lowercase().contains("kde")) && std::env::var_os("WAYLAND_DISPLAY").is_some()
}

struct Iface {
    tx: mpsc::UnboundedSender<Msg>,
}

#[zbus::interface(name = "org.prelude.Core")]
impl Iface {
    /// KWin スクリプトの callDBus(…, 'Report', kind, json)
    fn report(&self, kind: String, json: String) {
        let _ = self.tx.send(Msg::Report(kind, json));
    }
}

/// 取り込みを始める。tag はこのコアの印(別のソケットで動かすコアと、D-Bus の名前・スクリプトの名前を取り合わないように)。
/// できない環境(KDE でない・D-Bus に繋がらない)なら None
pub async fn start(tag: &str, notes: mpsc::UnboundedSender<Note>) -> Option<Handle> {
    if !supported() {
        eprintln!("prelude-core: window embedding needs KDE Plasma (Wayland). Panes will not show windows");
        return None;
    }
    let (tx, rx) = mpsc::unbounded_channel();
    let service = format!("org.prelude.Core{tag}");
    let conn = async {
        zbus::connection::Builder::session()?.name(service.as_str())?.serve_at("/org/prelude/Core", Iface { tx: tx.clone() })?.build().await
    }
    .await;
    let conn = match conn {
        Ok(c) => c,
        Err(e) => {
            eprintln!("prelude-core: D-Bus unavailable ({e}). Panes will not show windows");
            return None;
        }
    };
    let runtime = std::env::var("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir()).join("prelude");
    let _ = std::fs::create_dir_all(&runtime);
    let mut e = Embedder {
        conn,
        tx: tx.clone(),
        notes,
        service,
        script_name: format!("prelude-core-embed{tag}"),
        script_file: runtime.join(format!("embed{tag}.js")),
        wids_file: runtime.join(format!("embed-wids{tag}.json")),
        panes: HashMap::new(),
        known: None,
        close: vec![],
        close_pids: vec![],
        release: vec![],
        dirty: false,
    };
    e.recover();
    tokio::spawn(e.run(rx));
    Some(Handle { tx })
}

/// 取り込み中のペイン
struct Pane {
    want: Want,
    keys: Vec<String>,
    /// 起動した sh の PID(終わったら None)
    sh: Option<u32>,
    sh_pid: u32,
    /// 起動の後に始まった、取り込んだ窓の持ち主(閉じるときに終了させてよい)
    owned: HashSet<u32>,
    wids: Vec<String>,
    /// 起動前からあって借りた窓(閉じずに返す)
    adopted: Vec<String>,
    /// 起動前からあった窓(名前での照合の対象外)
    old: Vec<String>,
    adopt: bool,
    launched_at: f64,
    t0: Instant,
    had_window: bool,
    gone: u32,
    dead: u32,
    activate_until: Option<Instant>,
}

struct Embedder {
    conn: zbus::Connection,
    tx: mpsc::UnboundedSender<Msg>,
    notes: mpsc::UnboundedSender<Note>,
    service: String,
    script_name: String,
    script_file: PathBuf,
    wids_file: PathBuf,
    panes: HashMap<u64, Pane>,
    /// 直近の報告にあった全部の窓の id。まだ報告が無ければ None
    known: Option<Vec<String>>,
    close: Vec<String>,
    close_pids: Vec<u32>,
    release: Vec<String>,
    dirty: bool,
}

#[derive(Deserialize)]
struct Claim {
    pane: u64,
    wid: String,
    pid: u32,
    how: String,
}

#[derive(Deserialize)]
struct Windows {
    counts: HashMap<String, u32>,
    #[serde(default)]
    titles: HashMap<String, String>,
    #[serde(default)]
    all: Vec<String>,
}

/// 起動を任せるだけのラッパー(窓の名前にならない)
const WRAPPERS: [&str; 18] = [
    "sh",
    "bash",
    "env",
    "nohup",
    "setsid",
    "pkexec",
    "sudo",
    "lxqt-sudo",
    "xdg-open",
    "gtk-launch",
    "kioclient",
    "kioclient5",
    "flatpak",
    "snap",
    "dbus-launch",
    "x-terminal-emulator",
    "exec",
    "",
];

/// 起動したアプリの窓を見分ける名前(小文字)。.desktop の id・StartupWMClass・実行ファイル名
fn keys_of(cmd: &str, cls: &str) -> Vec<String> {
    if cls.starts_with("chrome-") {
        return vec![cls.to_lowercase()]; // 実ブラウザのペインは、そのペイン専用のクラスだけで照合する
    }
    let mut keys: Vec<String> = Vec::new();
    let mut add = |k: String| {
        if !k.is_empty() && !keys.contains(&k) {
            keys.push(k);
        }
    };
    add(cls.to_lowercase());
    if let Some(a) = apps::cached().into_iter().find(|a| a.exec == cmd.trim()) {
        add(a.cls);
        add(a.id.to_lowercase());
    }
    let base = procs::cmd_base(cmd);
    if !WRAPPERS.contains(&base.as_str()) {
        add(base);
    }
    keys
}

impl Embedder {
    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Msg>) {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        let mut debounce: Option<tokio::time::Instant> = None;
        loop {
            let sleep = async {
                match debounce {
                    Some(t) => tokio::time::sleep_until(t).await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                m = rx.recv() => {
                    let Some(m) = m else { break };
                    if let Msg::Shutdown(done) = m {
                        self.shutdown().await;
                        let _ = done.send(());
                        return;
                    }
                    self.handle(m, &mut rx).await;
                }
                _ = tick.tick() => self.tick(),
                _ = sleep => {
                    debounce = None;
                    self.dirty = false;
                    self.reload().await;
                }
            }
            if self.dirty && debounce.is_none() {
                debounce = Some(tokio::time::Instant::now() + Duration::from_millis(40));
            }
        }
    }

    async fn handle(&mut self, m: Msg, rx: &mut mpsc::UnboundedReceiver<Msg>) {
        match m {
            Msg::Want(w) => self.sync(w, rx).await,
            Msg::Report(kind, json) => self.report(&kind, &json),
            Msg::ShExited(pane, pid) => {
                if let Some(p) = self.panes.get_mut(&pane).filter(|p| p.sh_pid == pid) {
                    p.sh = None;
                    // 窓を出さずにすぐ終わった = 既に動いているプロセスに起動を任せた。その窓を取り込む
                    let tx = self.tx.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_millis(1500)).await;
                        let _ = tx.send(Msg::CheckAdopt(pane));
                    });
                }
            }
            Msg::CheckAdopt(pane) => {
                let others = self.others_pids(pane);
                if let Some(p) = self.panes.get_mut(&pane) {
                    if p.wids.is_empty() && pids_of(p, &others).is_empty() {
                        p.adopt = true;
                        self.dirty = true;
                    }
                }
            }
            Msg::Reload => self.dirty = true,
            Msg::Shutdown(_) => {}
        }
    }

    /// 取り込むペインを、コアの状態に合わせる
    async fn sync(&mut self, wants: Vec<Want>, rx: &mut mpsc::UnboundedReceiver<Msg>) {
        let ids: HashSet<u64> = wants.iter().map(|w| w.pane).collect();
        for id in self.panes.keys().copied().filter(|id| !ids.contains(id)).collect::<Vec<_>>() {
            self.close_pane(id);
        }
        for w in wants {
            match self.panes.get_mut(&w.pane) {
                Some(p) => {
                    if p.want.rect.is_none() && w.rect.is_some() {
                        // 最小化から戻した窓は、少しの間アクティブにする(しないと描かれないことがある)。終わったら読み直して外す
                        p.activate_until = Some(Instant::now() + Duration::from_millis(700));
                        reload_after(&self.tx, 800);
                    }
                    if p.want.rect != w.rect {
                        self.dirty = true;
                    }
                    p.want.rect = w.rect;
                }
                None if !w.marker.is_empty() => self.watch(w),
                None if !w.cmd.trim().is_empty() => self.launch(w, rx).await,
                None => {}
            }
        }
    }

    /// 起動はせず、印の題名の窓が現れるのを待つ(Web のペイン)
    fn watch(&mut self, w: Want) {
        let p = Pane {
            keys: vec![],
            want: w,
            sh: None,
            sh_pid: 0,
            owned: HashSet::new(),
            wids: vec![],
            adopted: vec![],
            old: vec![],
            adopt: false,
            launched_at: procs::uptime(),
            t0: Instant::now(),
            had_window: false,
            gone: 0,
            dead: 0,
            activate_until: None,
        };
        self.panes.insert(p.want.pane, p);
        self.dirty = true;
    }

    /// 起動する。起動前からある窓を知るため、まだ報告が無ければ一度スクリプトを読んで報告を待つ
    async fn launch(&mut self, w: Want, rx: &mut mpsc::UnboundedReceiver<Msg>) {
        if self.known.is_none() {
            self.reload_with(true).await;
            let end = tokio::time::Instant::now() + Duration::from_millis(800);
            let mut later = Vec::new();
            while self.known.is_none() {
                match tokio::time::timeout_at(end, rx.recv()).await {
                    Ok(Some(Msg::Report(k, j))) => self.report(&k, &j),
                    Ok(Some(m)) => later.push(m),
                    _ => break,
                }
            }
            for m in later {
                // 待っている間に来たほかの頼みは、送り直して後で処理する
                let _ = self.tx.send(m);
            }
        }
        let cwd = if std::path::Path::new(&w.cwd).is_dir() { w.cwd.clone() } else { std::env::var("HOME").unwrap_or_else(|_| "/".into()) };
        let mut cmd = tokio::process::Command::new("/bin/sh");
        cmd.arg("-c")
            .arg(&w.cmd)
            .current_dir(&cwd)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        cmd.process_group(0); // PRELUDE の端末のシグナル(Ctrl+C など)を受けない
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("prelude-core: cannot launch {} ({e})", w.cmd);
                let _ = self.notes.send(Note::Exited { pane: w.pane, had_window: false });
                return;
            }
        };
        let pid = child.id().unwrap_or(0);
        let (tx, pane) = (self.tx.clone(), w.pane);
        tokio::spawn(async move {
            let _ = child.wait().await;
            let _ = tx.send(Msg::ShExited(pane, pid));
        });
        let p = Pane {
            keys: keys_of(&w.cmd, &w.cls),
            want: w,
            sh: Some(pid),
            sh_pid: pid,
            owned: HashSet::new(),
            wids: vec![],
            adopted: vec![],
            old: self.known.clone().unwrap_or_default(),
            adopt: false,
            launched_at: procs::uptime(),
            t0: Instant::now(),
            had_window: false,
            gone: 0,
            dead: 0,
            activate_until: None,
        };
        self.panes.insert(pane, p);
        self.dirty = true;
    }

    fn report(&mut self, kind: &str, json: &str) {
        match kind {
            "claim" => {
                let Ok(c) = serde_json::from_str::<Claim>(json) else { return };
                let Some(p) = self.panes.get_mut(&c.pane) else { return };
                if p.wids.contains(&c.wid) {
                    return;
                }
                p.wids.push(c.wid.clone());
                let _ = self.notes.send(Note::Claimed { pane: c.pane });
                match c.how.as_str() {
                    "pid" => {
                        p.owned.insert(c.pid);
                    }
                    "adopt" => p.adopted.push(c.wid),
                    _ => {}
                }
                self.save_wids();
            }
            "windows" => {
                let Ok(r) = serde_json::from_str::<Windows>(json) else { return };
                self.known = Some(r.all);
                for (id, t) in r.titles {
                    if let Ok(pane) = id.parse() {
                        if self.panes.contains_key(&pane) {
                            let _ = self.notes.send(Note::Title { pane, title: t.chars().take(300).collect() });
                        }
                    }
                }
                for (id, n) in r.counts {
                    let Ok(pane) = id.parse::<u64>() else { continue };
                    let Some(p) = self.panes.get_mut(&pane) else { continue };
                    if n > 0 {
                        p.had_window = true;
                        p.gone = 0;
                        continue;
                    }
                    if !p.had_window {
                        continue;
                    }
                    // 一瞬の 0(窓の作り直しなど)を避けるため、少し待ってもう一度報告させ、続けて 0 なら終わったとみなす
                    p.gone += 1;
                    if p.gone == 1 {
                        reload_after(&self.tx, 800);
                    } else if p.gone == 2 {
                        self.close_pane(pane);
                        let _ = self.notes.send(Note::Exited { pane, had_window: true });
                    }
                }
            }
            _ => {}
        }
    }

    /// 1 秒ごと: 起動して 30 秒の間は読み直して PID を取り直す。窓が一度も出ないまま、20 秒たってプロセスも居なければ終わったとみなす
    fn tick(&mut self) {
        let mut exited = vec![];
        let ids: Vec<u64> = self.panes.keys().copied().collect();
        for id in ids {
            let others = self.others_pids(id);
            let Some(p) = self.panes.get_mut(&id) else { continue };
            let age = p.t0.elapsed();
            if age < Duration::from_secs(30) {
                self.dirty = true;
            }
            if p.had_window || age < Duration::from_secs(20) {
                continue;
            }
            let alive = pids_of(p, &others).into_iter().any(|x| Some(x) != p.sh && procs::alive(x));
            p.dead = if alive { 0 } else { p.dead + 1 };
            if p.dead >= 3 {
                exited.push(id);
            }
        }
        for id in exited {
            self.close_pane(id);
            let _ = self.notes.send(Note::Exited { pane: id, had_window: false });
        }
    }

    /// ほかのペインのプロセス(同じアプリを 2 つ開いたとき、先のペインが後のペインのプロセスを取らないように)
    fn others_pids(&self, me: u64) -> HashSet<u32> {
        let mut s = HashSet::new();
        for (id, o) in &self.panes {
            if *id != me {
                s.extend(o.owned.iter().copied());
                if let Some(sh) = o.sh {
                    s.extend(procs::descendants(sh));
                }
            }
        }
        s
    }

    /// ペインの窓を手放し、起動したプロセスを終わらせる
    fn close_pane(&mut self, id: u64) {
        let Some(p) = self.panes.remove(&id) else { return };
        for w in &p.wids {
            if p.adopted.contains(w) {
                self.release.push(w.clone());
            } else {
                self.close.push(w.clone());
            }
        }
        for pid in own_pids(&p).into_iter().rev() {
            if !procs::signal(pid, libc::SIGTERM) {
                self.close_pids.push(pid); // snap のアプリなど。窓を閉じて終わらせる
            }
        }
        self.save_wids();
        self.dirty = true;
    }

    fn script(&mut self) -> String {
        let now = Instant::now();
        let ids: Vec<u64> = self.panes.keys().copied().collect();
        let mut panes = vec![];
        for id in ids {
            let others = self.others_pids(id);
            let p = &self.panes[&id];
            let rect = p.want.rect.filter(|r| r.width > 0 && r.height > 0);
            panes.push(json!({
                "id": id,
                "pids": pids_of(p, &others),
                "keys": p.keys,
                "marker": p.want.marker,
                "wids": p.wids,
                "old": p.old,
                "adopt": p.adopt,
                "rect": rect.map(|r| json!({ "x": r.x, "y": r.y, "width": r.width, "height": r.height })),
                "activate": p.activate_until.is_some_and(|t| t > now),
            }));
        }
        let state = json!({
            "service": self.service,
            "self": [std::process::id()],
            "panes": panes,
            "close": std::mem::take(&mut self.close),
            "closePids": std::mem::take(&mut self.close_pids),
            "release": std::mem::take(&mut self.release),
        });
        include_str!("embed.js").replace("/*STATE*/", &state.to_string())
    }

    async fn kwin(&self, method: &str, body: &(impl serde::Serialize + zbus::zvariant::DynamicType)) -> Option<zbus::Message> {
        self.conn.call_method(Some("org.kde.KWin"), "/Scripting", Some("org.kde.kwin.Scripting"), method, body).await.ok()
    }

    async fn reload(&mut self) {
        self.reload_with(false).await
    }

    /// スクリプトを読み直す。force: 取り込むものが無くても読む(報告だけ欲しいとき)
    async fn reload_with(&mut self, force: bool) {
        let _ = self.kwin("unloadScript", &(self.script_name.as_str(),)).await;
        if !force && self.panes.is_empty() && self.close.is_empty() && self.close_pids.is_empty() && self.release.is_empty() {
            return;
        }
        let text = self.script();
        if let Err(e) = std::fs::write(&self.script_file, text) {
            eprintln!("prelude-core: cannot write KWin script ({e})");
            return;
        }
        let file = self.script_file.to_string_lossy().into_owned();
        let Some(m) = self.kwin("loadScript", &(file.as_str(), self.script_name.as_str())).await else { return };
        // 読み込んだ番号の Script{id}.run は使わない(番号が重なることがある)。start は未実行のものを全部動かす
        if m.body().deserialize::<i32>().is_ok_and(|id| id >= 0) {
            let _ = self.kwin("start", &()).await;
        }
    }

    fn save_wids(&self) {
        let all: Vec<&String> = self.panes.values().flat_map(|p| &p.wids).collect();
        if all.is_empty() {
            let _ = std::fs::remove_file(&self.wids_file);
        } else {
            let _ = std::fs::write(&self.wids_file, serde_json::to_string(&all).unwrap_or_default());
        }
    }

    /// 前に落ちたときに取り込んだままだった窓を、閉じずに普通の窓に戻す(保存していない作業があるかもしれない)
    fn recover(&mut self) {
        let Ok(text) = std::fs::read_to_string(&self.wids_file) else { return };
        let _ = std::fs::remove_file(&self.wids_file);
        if let Ok(v) = serde_json::from_str::<Vec<String>>(&text) {
            if !v.is_empty() {
                eprintln!("prelude-core: releasing {} window(s) left by the last run", v.len());
                self.release.extend(v);
                self.dirty = true;
            }
        }
    }

    /// 終わる: 起動したアプリに終了の合図を送り、窓を閉じ、3 秒まで待つ。残ったものだけ強制終了する
    async fn shutdown(&mut self) {
        let mut pids: Vec<u32> = vec![];
        for p in self.panes.values() {
            pids.extend(own_pids(p));
        }
        for id in self.panes.keys().copied().collect::<Vec<_>>() {
            self.close_pane(id);
        }
        self.reload_with(true).await;
        let end = Instant::now() + Duration::from_secs(3);
        while pids.iter().any(|p| procs::alive(*p)) && Instant::now() < end {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        for p in pids.into_iter().filter(|p| procs::alive(*p)) {
            procs::signal(p, libc::SIGKILL);
        }
        tokio::time::sleep(Duration::from_millis(200)).await; // 閉じる・戻す処理をスクリプトが終えるのを待つ
        let _ = self.kwin("unloadScript", &(self.script_name.as_str(),)).await;
        let _ = std::fs::remove_file(&self.script_file);
        let _ = std::fs::remove_file(&self.wids_file);
    }
}

/// ms ミリ秒後に読み直す
fn reload_after(tx: &mpsc::UnboundedSender<Msg>, ms: u64) {
    let tx = tx.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(ms)).await;
        let _ = tx.send(Msg::Reload);
    });
}

/// ペインのプロセス: 起動した sh の子孫と、取り込んだ窓の持ち主。窓がまだ無い間は「起動の後に始まった同じ名前のプロセス」も(ほかのペインのものは除く)
fn pids_of(p: &Pane, others: &HashSet<u32>) -> Vec<u32> {
    let mut out: Vec<u32> = p.sh.map(procs::descendants).unwrap_or_default();
    out.extend(p.owned.iter().copied());
    if p.wids.is_empty() {
        out.extend(procs::fresh_pids(&p.want.cmd, p.launched_at).into_iter().filter(|x| !others.contains(x)));
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// 終了させてよいプロセス: 起動した sh の子孫と、起動の後に始まった取り込み済みの窓の持ち主
fn own_pids(p: &Pane) -> Vec<u32> {
    let mut out: Vec<u32> = p.sh.map(procs::descendants).unwrap_or_default();
    out.extend(p.owned.iter().copied());
    out.sort_unstable();
    out.dedup();
    out
}

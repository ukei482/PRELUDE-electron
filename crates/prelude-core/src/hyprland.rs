//! Hyprland の裏側([`Compositor`])。docs/redesign-rust.md の「窓のつなぎ」の段 1(「作業領域だけ指定する形」)まで。
//! Omarchy(Hyprland の上の完成形)のように、窓の並べ方・大きさ・浮かせ方が環境で決まっているところに合わせる。
//!
//! - ペインのアプリは Hyprland の exec に **タブの作業領域** を付けて起動する(`[workspace N silent]`)。並べ方は Hyprland に任せる
//! - 出てきた窓は、環境変数 `PRELUDE_PANE`(起動した sh から子へ受け継がれる)か、Web のペインなら題名の印でペインに結び付ける。
//!   exec の決まりが効かなかった窓(既に動いているブラウザが開いた窓など)だけ、タブの作業領域へ移す。移した後は動かさない
//! - 作業領域が切り替わったことは、イベント(`.socket2.sock`)を **読むだけ** で知り、タブの切り替えとしてコアに知らせる
//! - PRELUDE でタブを選んだ・開いたときだけ、その作業領域へ移る(頼まれたときだけ。「取り合わない決まり」)
//! - 間・角丸・窓の決まりは触らない(環境の見た目を上書きしない)
//!
//! Hyprland は 0.5x で設定と dispatch が Lua(`hl.dsp.…`)になった。Lua の形を先に試し、通らなければ前の形(`movetoworkspacesilent` など)で送る。
use crate::compositor::{Compositor, Level, Note, Options, Scene, Tree};
use crate::procs;
use crate::sway::{ws_name, ws_num, ws_suffix};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, oneshot};

/// Web のペインの印の窓を待つ時間。過ぎても出なければ止まったことにする(sway のつなぎと同じ)
const WEB_WAIT: Duration = Duration::from_secs(30);
/// 窓が閉じてから、次の窓を待つ時間(スプラッシュの後に本体の窓を出し直すアプリのため)
const GONE_WAIT: Duration = Duration::from_secs(2);

/// Hyprland のソケットのフォルダ。今の場所(`$XDG_RUNTIME_DIR/hypr/<印>`)、無ければ前の場所(`/tmp/hypr/<印>`)
pub fn socket_dir(get: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let sig = get("HYPRLAND_INSTANCE_SIGNATURE")?;
    let mut dirs = vec![];
    if let Some(r) = get("XDG_RUNTIME_DIR") {
        dirs.push(Path::new(&r).join("hypr").join(&sig));
    }
    dirs.push(Path::new("/tmp/hypr").join(&sig));
    dirs.into_iter().find(|d| d.join(".socket.sock").exists())
}

/// socket2 のイベント(使うものだけ)
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// 窓が出た(アドレスは 0x の無い 16 進)
    Open {
        addr: String,
        ws: String,
        class: String,
        title: String,
    },
    Close {
        addr: String,
    },
    Title {
        addr: String,
        title: String,
    },
    /// 作業領域を選んだ(名前)
    Workspace {
        name: String,
    },
}

/// socket2 の 1 行(`名前>>中身`)を読む。知らないイベントは None
pub fn parse_event(line: &str) -> Option<Event> {
    let (name, data) = line.split_once(">>")?;
    match name {
        "openwindow" => {
            let mut it = data.splitn(4, ',');
            let addr = it.next()?.to_string();
            let ws = it.next()?.to_string();
            let class = it.next()?.to_string();
            let title = it.next().unwrap_or("").to_string();
            Some(Event::Open { addr, ws, class, title })
        }
        "closewindow" => Some(Event::Close { addr: data.to_string() }),
        "windowtitlev2" => {
            let (addr, title) = data.split_once(',')?;
            Some(Event::Title { addr: addr.into(), title: title.into() })
        }
        "workspacev2" => Some(Event::Workspace { name: data.split_once(',')?.1.to_string() }),
        _ => None,
    }
}

/// アドレスを揃える(イベントは 0x 無し、`j/clients` は 0x 付き)
pub fn norm_addr(a: &str) -> String {
    a.trim().trim_start_matches("0x").to_lowercase()
}

/// タブの作業領域を Hyprland で指す言葉。名前の後ろが無ければ番号の作業領域(Omarchy の `Super + 数字` がそのまま効く)、あれば名前の作業領域
pub fn ws_target(num: u32, suffix: &str) -> String {
    if suffix.is_empty() {
        num.to_string()
    } else {
        format!("name:{}", ws_name(num, suffix))
    }
}

/// シェルの 1 語にする('…')
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Lua の文字列にする("…")
pub fn lua_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Hyprland の exec に渡すコマンド。作業フォルダ(無ければホーム)と `PRELUDE_PANE` を付けて、ペインのコマンドを sh で動かす。
/// Hyprland が起動するので、コアの環境は受け継がれない。別のソケットのコアなら、そのソケットも渡す
pub fn exec_line(cmd: &str, cwd: &str, pane: u64, core_sock: Option<&str>) -> String {
    let sock = core_sock.map(|s| format!("PRELUDE_CORE_SOCK={} ", sh_quote(s))).unwrap_or_default();
    format!("cd {} 2>/dev/null || cd; {sock}{}={pane} exec /bin/sh -c {}", sh_quote(cwd), crate::launch::PANE_ENV, sh_quote(cmd))
}

/// Hyprland への頼みごと
#[derive(Debug, Clone, PartialEq)]
pub enum Dispatch {
    /// 作業領域を付けて起動する(作業領域は選ばない)
    Exec {
        line: String,
        ws: Option<String>,
    },
    /// 窓を作業領域へ移す(付いていかない)
    Move {
        addr: String,
        ws: String,
    },
    /// 作業領域を選ぶ
    Focus {
        ws: String,
    },
    Close {
        addr: String,
    },
}

impl Dispatch {
    /// Lua の形(Hyprland 0.5x から。Omarchy 4 はこちら)
    pub fn lua(&self) -> String {
        match self {
            Dispatch::Exec { line, ws: Some(ws) } => {
                format!("hl.dsp.exec_cmd({}, {{ workspace = {} }})", lua_str(line), lua_str(&format!("{ws} silent")))
            }
            Dispatch::Exec { line, ws: None } => format!("hl.dsp.exec_cmd({})", lua_str(line)),
            Dispatch::Move { addr, ws } => format!(
                "hl.dsp.window.move({{ workspace = {}, follow = false, window = {} }})",
                lua_str(ws),
                lua_str(&format!("address:0x{addr}"))
            ),
            Dispatch::Focus { ws } => format!("hl.dsp.focus({{ workspace = {} }})", lua_str(ws)),
            Dispatch::Close { addr } => format!("hl.dsp.window.close({{ window = {} }})", lua_str(&format!("address:0x{addr}"))),
        }
    }

    /// 前の形(Lua の設定より前の Hyprland)
    pub fn legacy(&self) -> String {
        match self {
            Dispatch::Exec { line, ws: Some(ws) } => format!("exec [workspace {ws} silent] {line}"),
            Dispatch::Exec { line, ws: None } => format!("exec {line}"),
            Dispatch::Move { addr, ws } => format!("movetoworkspacesilent {ws},address:0x{addr}"),
            Dispatch::Focus { ws } => format!("workspace {ws}"),
            Dispatch::Close { addr } => format!("closewindow address:0x{addr}"),
        }
    }
}

/// `j/clients` の 1 つから、ペインを見分けるのに使うもの
#[derive(Debug, Clone, PartialEq)]
pub struct Client {
    pub addr: String,
    pub pid: u32,
    pub title: String,
    pub ws: String,
    pub floating: bool,
}

pub fn clients(v: &Value) -> Vec<Client> {
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|c| Client {
                    addr: norm_addr(c["address"].as_str().unwrap_or("")),
                    pid: c["pid"].as_i64().unwrap_or(0).max(0) as u32,
                    title: c["title"].as_str().unwrap_or("").to_string(),
                    ws: c["workspace"]["name"].as_str().unwrap_or("").to_string(),
                    floating: c["floating"].as_bool().unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn leaves(t: &Tree, out: &mut Vec<u64>) {
    match t {
        Tree::Pane(p) => out.push(*p),
        Tree::Split { a, b, .. } => {
            leaves(a, out);
            leaves(b, out);
        }
    }
}

/// ペイン → そのタブの番号
pub fn tab_nums(scene: &Scene) -> HashMap<u64, u32> {
    let mut m = HashMap::new();
    for t in &scene.tabs {
        let mut l = vec![];
        leaves(&t.tree, &mut l);
        for p in l {
            m.insert(p, t.num);
        }
    }
    m
}

/// まだ窓の無いペインのうち、この窓のもの。Web のペインは題名の印、ほかは窓のプロセスの `PRELUDE_PANE`
pub fn match_pane<'a>(pending: impl Iterator<Item = (u64, &'a str)>, c: &Client, has_env: &dyn Fn(u32, &str) -> bool) -> Option<u64> {
    let mut by_env = None;
    for (pane, marker) in pending {
        if !marker.is_empty() {
            if c.title.contains(marker) {
                return Some(pane);
            }
        } else if by_env.is_none() && c.pid > 0 && has_env(c.pid, &format!("{}={pane}", crate::launch::PANE_ENV)) {
            by_env = Some(pane);
        }
    }
    by_env
}

/// コマンドのソケット(`.socket.sock`)に 1 つ送って、返事を読む(Hyprland は返事の後に閉じる)
async fn request(dir: &Path, cmd: &str) -> std::io::Result<String> {
    let mut s = UnixStream::connect(dir.join(".socket.sock")).await?;
    s.write_all(cmd.as_bytes()).await?;
    let mut out = String::new();
    s.read_to_string(&mut out).await?;
    Ok(out)
}

enum Msg {
    Arrange(Scene),
    Event(Event),
    /// 窓が閉じた後、次の窓が無ければ止まったことにする
    Gone(u64),
    /// Web のペインの窓が出なかった
    WebTimeout(u64),
    Shutdown(oneshot::Sender<()>),
}

/// Hyprland の裏側に頼む口
pub struct Handle {
    tx: mpsc::UnboundedSender<Msg>,
    level: Level,
}

impl Compositor for Handle {
    fn name(&self) -> &'static str {
        "hyprland"
    }
    fn level(&self) -> Level {
        self.level
    }
    fn arrange(&self, scene: Scene) {
        let _ = self.tx.send(Msg::Arrange(scene));
    }
    fn shutdown(&self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let (tx, rx) = oneshot::channel();
            if self.tx.send(Msg::Shutdown(tx)).is_ok() {
                let _ = rx.await;
            }
        })
    }
}

/// 始める。`HYPRLAND_INSTANCE_SIGNATURE` のソケットにつなぐ。つながらなければ None
pub async fn start(tag: &str, notes: mpsc::UnboundedSender<Note>, opts: Options) -> Option<Handle> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let Some(dir) = socket_dir(&env) else {
        eprintln!("prelude-core: cannot find the Hyprland socket (HYPRLAND_INSTANCE_SIGNATURE). Apps are only started");
        return None;
    };
    start_in(dir, tag, notes, opts).await
}

/// ソケットのフォルダを決めて始める(テストは偽の Hyprland のフォルダを渡す)
async fn start_in(dir: PathBuf, tag: &str, notes: mpsc::UnboundedSender<Note>, opts: Options) -> Option<Handle> {
    let ev = match UnixStream::connect(dir.join(".socket2.sock")).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("prelude-core: cannot connect to Hyprland ({e}). Apps are only started");
            return None;
        }
    };
    let (tx, rx) = mpsc::unbounded_channel();
    {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(ev).lines();
            while let Ok(Some(l)) = lines.next_line().await {
                if let Some(e) = parse_event(&l) {
                    if tx.send(Msg::Event(e)).is_err() {
                        break;
                    }
                }
            }
        });
    }
    let suffix = ws_suffix(&opts.workspace_prefix, tag);
    let cur_ws = request(&dir, "j/activeworkspace")
        .await
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| ws_num(v["name"].as_str()?, &suffix));
    let level = opts.level(Level::Workspace);
    let h = Hypr {
        dir,
        tx: tx.clone(),
        notes,
        suffix,
        core_sock: std::env::var("PRELUDE_CORE_SOCK").ok().filter(|s| !s.is_empty()),
        lua: None,
        panes: HashMap::new(),
        placed: HashMap::new(),
        scene: Scene::default(),
        cur_ws,
        last_active: None,
    };
    tokio::spawn(h.run(rx));
    Some(Handle { tx, level })
}

struct Pane {
    marker: String,
    /// 結び付けた窓(0x 無し)
    addr: Option<String>,
}

struct Hypr {
    dir: PathBuf,
    tx: mpsc::UnboundedSender<Msg>,
    notes: mpsc::UnboundedSender<Note>,
    /// タブの作業領域の名前の、番号の後ろ
    suffix: String,
    core_sock: Option<String>,
    /// Lua の dispatch が通るか(分かるまで None)
    lua: Option<bool>,
    panes: HashMap<u64, Pane>,
    /// 作業領域へ移した(または exec で置いた)ペインとタブの番号。移し済みはもう動かさない
    placed: HashMap<u64, u32>,
    scene: Scene,
    /// 今選ばれている作業領域のタブの番号(タブでない作業領域なら None)
    cur_ws: Option<u32>,
    /// 最後に合わせた今のタブ(変わったときだけ作業領域を選ぶ)
    last_active: Option<u64>,
}

impl Hypr {
    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Msg>) {
        while let Some(m) = rx.recv().await {
            match m {
                Msg::Arrange(scene) => self.sync(scene).await,
                Msg::Event(e) => self.event(e).await,
                Msg::Gone(pane) => {
                    if self.panes.get(&pane).is_some_and(|p| p.addr.is_none()) {
                        self.panes.remove(&pane);
                        let _ = self.notes.send(Note::Exited { pane, had_window: true });
                    }
                }
                Msg::WebTimeout(pane) => {
                    if self.panes.get(&pane).is_some_and(|p| p.addr.is_none()) {
                        self.panes.remove(&pane);
                        let _ = self.notes.send(Note::Exited { pane, had_window: false });
                    }
                }
                // 窓の持ち主は Hyprland(と使用者)。終わるときも窓は閉じない
                Msg::Shutdown(done) => {
                    let _ = done.send(());
                    return;
                }
            }
        }
    }

    fn later(&self, m: Msg, d: Duration) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(d).await;
            let _ = tx.send(m);
        });
    }

    /// Lua の形を先に試し、通らなければ前の形で送る。通ったほうを覚える
    async fn dispatch(&mut self, d: Dispatch) {
        let ok = |r: &std::io::Result<String>| r.as_ref().is_ok_and(|t| t.trim() == "ok");
        if self.lua != Some(false) {
            let r = request(&self.dir, &format!("dispatch {}", d.lua())).await;
            if ok(&r) {
                self.lua = Some(true);
                return;
            }
            if self.lua == Some(true) {
                eprintln!("prelude-core: hyprland refused {} ({})", d.lua(), r.unwrap_or_else(|e| e.to_string()).trim());
                return;
            }
        }
        let r = request(&self.dir, &format!("dispatch {}", d.legacy())).await;
        if ok(&r) {
            self.lua = Some(false);
        } else {
            eprintln!("prelude-core: hyprland refused {} ({})", d.legacy(), r.unwrap_or_else(|e| e.to_string()).trim());
        }
    }

    async fn clients(&self) -> Vec<Client> {
        request(&self.dir, "j/clients")
            .await
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .map(|v| clients(&v))
            .unwrap_or_default()
    }

    /// コアの状態に合わせる: 閉じたペインの窓を閉じる・新しいペインを起動する・番号の変わったタブの窓を移す・PRELUDE で選んだタブへ移る
    async fn sync(&mut self, scene: Scene) {
        let ids: HashSet<u64> = scene.panes.iter().map(|w| w.pane).collect();
        let closing: Vec<u64> = self.panes.keys().copied().filter(|id| !ids.contains(id)).collect();
        for id in closing {
            // PRELUDE でペインを閉じた(頼まれた)ときだけ窓を閉じる
            if let Some(addr) = self.panes.remove(&id).and_then(|p| p.addr) {
                self.dispatch(Dispatch::Close { addr }).await;
            }
            self.placed.remove(&id);
        }
        let nums = tab_nums(&scene);
        for w in &scene.panes {
            // 既にある窓を渡されるのは段 4 の引き取りだけ。このつなぎは引き取らない
            if self.panes.contains_key(&w.pane) || w.window.is_some() || w.cmd.trim().is_empty() {
                continue;
            }
            let num = nums.get(&w.pane).copied();
            let line = exec_line(&w.cmd, &w.cwd, w.pane, self.core_sock.as_deref());
            self.dispatch(Dispatch::Exec { line, ws: num.map(|n| ws_target(n, &self.suffix)) }).await;
            if !w.marker.is_empty() {
                self.later(Msg::WebTimeout(w.pane), WEB_WAIT);
            }
            self.panes.insert(w.pane, Pane { marker: w.marker.clone(), addr: None });
        }
        self.scene = scene;
        // タブの番号が変わった(前のタブを閉じた)窓を、新しい作業領域へ移す
        let moves: Vec<(String, u32, u64)> = self
            .panes
            .iter()
            .filter_map(|(id, p)| {
                let n = *nums.get(id)?;
                let addr = p.addr.clone()?;
                (self.placed.get(id).is_some_and(|old| *old != n)).then_some((addr, n, *id))
            })
            .collect();
        for (addr, n, id) in moves {
            self.dispatch(Dispatch::Move { addr, ws: ws_target(n, &self.suffix) }).await;
            self.placed.insert(id, n);
        }
        // PRELUDE でタブを選んだ・開いたときだけ、その作業領域へ移る
        if self.scene.active != self.last_active {
            self.last_active = self.scene.active;
            let num = self.scene.tabs.iter().find(|t| Some(t.id) == self.scene.active).map(|t| t.num);
            if let Some(n) = num.filter(|n| Some(*n) != self.cur_ws) {
                self.dispatch(Dispatch::Focus { ws: ws_target(n, &self.suffix) }).await;
                self.cur_ws = Some(n);
            }
        }
    }

    fn pane_of(&self, addr: &str) -> Option<u64> {
        self.panes.iter().find(|(_, p)| p.addr.as_deref() == Some(addr)).map(|(id, _)| *id)
    }

    /// 出てきた窓(または題名が変わった窓)を、まだ窓の無いペインに結び付ける
    async fn try_claim(&mut self, addr: &str) {
        if self.panes.values().all(|p| p.addr.is_some()) || self.pane_of(addr).is_some() {
            return;
        }
        let Some(c) = self.clients().await.into_iter().find(|c| c.addr == addr) else { return };
        // 浮いた窓は触らない(ダイアログなど。「取り合わない決まり」)
        if c.floating {
            return;
        }
        let pending: Vec<(u64, String)> =
            self.panes.iter().filter(|(_, p)| p.addr.is_none()).map(|(id, p)| (*id, p.marker.clone())).collect();
        let Some(pane) = match_pane(pending.iter().map(|(id, m)| (*id, m.as_str())), &c, &procs::has_env) else { return };
        if let Some(p) = self.panes.get_mut(&pane) {
            p.addr = Some(c.addr.clone());
        }
        let _ = self.notes.send(Note::Claimed { pane });
        let _ = self.notes.send(Note::Title { pane, title: c.title.chars().take(300).collect() });
        // exec の決まりが効かなかった窓(既に動いているプロセスが開いた窓)だけ、タブの作業領域へ移す
        if let Some(n) = tab_nums(&self.scene).get(&pane).copied() {
            if ws_num(&c.ws, &self.suffix) != Some(n) {
                self.dispatch(Dispatch::Move { addr: c.addr, ws: ws_target(n, &self.suffix) }).await;
            }
            self.placed.insert(pane, n);
        }
    }

    async fn event(&mut self, e: Event) {
        match e {
            Event::Open { addr, .. } => self.try_claim(&norm_addr(&addr)).await,
            Event::Title { addr, title } => {
                let addr = norm_addr(&addr);
                match self.pane_of(&addr) {
                    Some(pane) => {
                        let _ = self.notes.send(Note::Title { pane, title: title.chars().take(300).collect() });
                    }
                    // Web のペインは、読み込んでから題名に印が出る
                    None if self.panes.values().any(|p| p.addr.is_none() && !p.marker.is_empty()) => self.try_claim(&addr).await,
                    None => {}
                }
            }
            Event::Close { addr } => {
                let addr = norm_addr(&addr);
                if let Some(pane) = self.pane_of(&addr) {
                    if let Some(p) = self.panes.get_mut(&pane) {
                        p.addr = None;
                    }
                    self.later(Msg::Gone(pane), GONE_WAIT);
                }
            }
            Event::Workspace { name } => {
                // 読むだけ: 使用者が Hyprland で選んだ作業領域が、タブの切り替えになる。タブでない作業領域なら何もしない
                self.cur_ws = ws_num(&name, &self.suffix);
                let tab = self.cur_ws.and_then(|n| self.scene.tabs.iter().find(|t| t.num == n)).map(|t| t.id);
                if let Some(tab) = tab {
                    self.last_active = Some(tab);
                    if self.scene.active != Some(tab) {
                        let _ = self.notes.send(Note::Focus { tab, pane: None });
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compositor::{TabScene, Want};
    use serde_json::json;

    #[test]
    fn イベントを読む() {
        assert_eq!(
            parse_event("openwindow>>5603a0b0c0d0,3,foot,~/src, a: b"),
            Some(Event::Open { addr: "5603a0b0c0d0".into(), ws: "3".into(), class: "foot".into(), title: "~/src, a: b".into() })
        );
        assert_eq!(parse_event("closewindow>>abc"), Some(Event::Close { addr: "abc".into() }));
        assert_eq!(parse_event("windowtitlev2>>abc,x, y"), Some(Event::Title { addr: "abc".into(), title: "x, y".into() }));
        assert_eq!(parse_event("workspacev2>>2,2:work"), Some(Event::Workspace { name: "2:work".into() }));
        assert_eq!(parse_event("workspace>>2"), None);
        assert_eq!(parse_event("garbage"), None);
        assert_eq!(norm_addr("0x5603ABC"), "5603abc");
    }

    #[test]
    fn 作業領域の指し方() {
        assert_eq!(ws_target(3, ""), "3");
        assert_eq!(ws_target(3, ":work"), "name:3:work");
        // 作業領域の名前からタブの番号へ戻せる
        assert_eq!(ws_num("3", ""), Some(3));
        assert_eq!(ws_num("3:work", ":work"), Some(3));
        assert_eq!(ws_num("special:magic", ""), None);
    }

    #[test]
    fn 頼みごとの形() {
        let e = Dispatch::Exec { line: "echo \"a\"".into(), ws: Some("2".into()) };
        assert_eq!(e.lua(), r#"hl.dsp.exec_cmd("echo \"a\"", { workspace = "2 silent" })"#);
        assert_eq!(e.legacy(), r#"exec [workspace 2 silent] echo "a""#);
        let m = Dispatch::Move { addr: "abc".into(), ws: "name:2:w".into() };
        assert_eq!(m.lua(), r#"hl.dsp.window.move({ workspace = "name:2:w", follow = false, window = "address:0xabc" })"#);
        assert_eq!(m.legacy(), "movetoworkspacesilent name:2:w,address:0xabc");
        assert_eq!(Dispatch::Focus { ws: "4".into() }.lua(), r#"hl.dsp.focus({ workspace = "4" })"#);
        assert_eq!(Dispatch::Focus { ws: "4".into() }.legacy(), "workspace 4");
        assert_eq!(Dispatch::Close { addr: "abc".into() }.legacy(), "closewindow address:0xabc");
        assert_eq!(lua_str("a\\b\nc"), r#""a\\b\nc""#);
    }

    #[test]
    fn 起動のコマンド() {
        let l = exec_line("foot -e 'nvim'", "/home/u/it's", 7, None);
        assert_eq!(l, r#"cd '/home/u/it'\''s' 2>/dev/null || cd; PRELUDE_PANE=7 exec /bin/sh -c 'foot -e '\''nvim'\'''"#);
        assert!(exec_line("x", "/", 1, Some("/tmp/c.sock")).contains("PRELUDE_CORE_SOCK='/tmp/c.sock' PRELUDE_PANE=1 "));
    }

    #[tokio::test]
    async fn 起動のコマンドを_sh_で動かす() {
        let d = std::env::temp_dir().join(format!("prelude-hypr-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let out = d.join("out");
        let cmd = format!("printf '%s %s' \"$PRELUDE_PANE\" \"$(pwd)\" > '{}'", out.display());
        let st =
            tokio::process::Command::new("/bin/sh").arg("-c").arg(exec_line(&cmd, &d.to_string_lossy(), 9, None)).status().await.unwrap();
        assert!(st.success());
        assert_eq!(std::fs::read_to_string(&out).unwrap(), format!("9 {}", d.display()));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 窓の一覧を読む() {
        let v = json!([{ "address": "0xABC", "pid": 42, "title": "t", "workspace": { "id": 2, "name": "2" }, "floating": true }, {}]);
        let c = clients(&v);
        assert_eq!(c[0], Client { addr: "abc".into(), pid: 42, title: "t".into(), ws: "2".into(), floating: true });
        assert_eq!(c[1].pid, 0);
    }

    #[test]
    fn 窓をペインに結び付ける() {
        let c = Client { addr: "a".into(), pid: 10, title: "Docs — prelude:5 — Browser".into(), ws: "1".into(), floating: false };
        let env = |pid: u32, e: &str| pid == 10 && e == "PRELUDE_PANE=3";
        // 題名の印が先
        assert_eq!(match_pane([(3, ""), (5, "prelude:5")].into_iter(), &c, &env), Some(5));
        // 印が無ければプロセスの環境変数
        assert_eq!(match_pane([(4, ""), (3, "")].into_iter(), &c, &env), Some(3));
        assert_eq!(match_pane([(4, ""), (6, "prelude:6")].into_iter(), &c, &env), None);
    }

    #[test]
    fn ペインのタブの番号() {
        let s = Scene {
            panes: vec![Want {
                pane: 1,
                cmd: "x".into(),
                cls: String::new(),
                cwd: "/".into(),
                rect: None,
                marker: String::new(),
                window: None,
            }],
            tabs: vec![
                TabScene { id: 10, num: 1, tree: Tree::Pane(1), active_pane: 1 },
                TabScene {
                    id: 11,
                    num: 2,
                    tree: Tree::Split { dir: prelude_format::Dir::Row, ratio: 0.5, a: Box::new(Tree::Pane(2)), b: Box::new(Tree::Pane(3)) },
                    active_pane: 2,
                },
            ],
            active: Some(10),
        };
        let m = tab_nums(&s);
        assert_eq!((m[&1], m[&2], m[&3]), (1, 2, 2));
    }

    #[test]
    fn ソケットの場所() {
        let d = std::env::temp_dir().join(format!("prelude-hypr-sock-{}", std::process::id()));
        std::fs::create_dir_all(d.join("hypr/sig")).unwrap();
        std::fs::write(d.join("hypr/sig/.socket.sock"), "").unwrap();
        let rt = d.to_string_lossy().to_string();
        let get = |k: &str| match k {
            "HYPRLAND_INSTANCE_SIGNATURE" => Some("sig".to_string()),
            "XDG_RUNTIME_DIR" => Some(rt.clone()),
            _ => None,
        };
        assert_eq!(socket_dir(&get), Some(d.join("hypr/sig")));
        assert_eq!(socket_dir(&|_| None), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 偽の Hyprland: Lua の dispatch は断り(前の Hyprland)、前の形だけ ok を返す。受けた頼みごとを送り出す
    async fn fake(dir: &Path) -> (mpsc::UnboundedReceiver<String>, tokio::net::UnixListener) {
        let cmd = tokio::net::UnixListener::bind(dir.join(".socket.sock")).unwrap();
        let ev = tokio::net::UnixListener::bind(dir.join(".socket2.sock")).unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = cmd.accept().await else { break };
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let reply = match req.as_str() {
                    "j/activeworkspace" => r#"{"id":1,"name":"1"}"#.to_string(),
                    "j/clients" => {
                        r#"[{"address":"0xbeef","pid":1,"title":"Docs prelude:5","workspace":{"id":1,"name":"1"},"floating":false}]"#
                            .to_string()
                    }
                    r if r.starts_with("dispatch hl.") => "Invalid dispatcher".to_string(),
                    _ => "ok".to_string(),
                };
                let _ = s.write_all(reply.as_bytes()).await;
                if req.starts_with("dispatch ") && !req.starts_with("dispatch hl.") {
                    let _ = tx.send(req["dispatch ".len()..].to_string());
                }
            }
        });
        (rx, ev)
    }

    async fn next(rx: &mut mpsc::UnboundedReceiver<String>) -> String {
        tokio::time::timeout(Duration::from_secs(2), rx.recv()).await.unwrap().unwrap()
    }

    #[tokio::test]
    async fn 偽の_hyprland_で通す() {
        let dir = std::env::temp_dir().join(format!("prelude-hypr-fake-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (mut cmds, ev) = fake(&dir).await;
        let (ntx, mut notes) = mpsc::unbounded_channel();
        let h = start_in(dir.clone(), "", ntx, Options::default()).await.unwrap();
        let (mut ev, _) = ev.accept().await.unwrap();
        assert_eq!((h.name(), h.level()), ("hyprland", Level::Workspace));
        let web =
            Want { pane: 5, cmd: "true".into(), cls: String::new(), cwd: "/".into(), rect: None, marker: "prelude:5".into(), window: None };
        let scene = |active| Scene {
            panes: vec![web.clone()],
            tabs: vec![TabScene { id: 20, num: 2, tree: Tree::Pane(5), active_pane: 5 }],
            active: Some(active),
        };
        // 起動はタブの作業領域を付けて(Lua が通らないので前の形で)。PRELUDE が選んだタブへ移る
        h.arrange(scene(20));
        let exec = next(&mut cmds).await;
        assert!(exec.starts_with("exec [workspace 2 silent] cd '/'") && exec.contains("PRELUDE_PANE=5"), "{exec}");
        assert_eq!(next(&mut cmds).await, "workspace 2");
        // 窓が出たら印で結び付け、今の作業領域(1)がタブ(2)と違うので移す
        ev.write_all(b"openwindow>>beef,1,browser,Docs prelude:5\n").await.unwrap();
        assert_eq!(next(&mut cmds).await, "movetoworkspacesilent 2,address:0xbeef");
        assert_eq!(notes.recv().await, Some(Note::Claimed { pane: 5 }));
        assert!(matches!(notes.recv().await, Some(Note::Title { pane: 5, .. })));
        // 使用者が Hyprland で作業領域を選んだら、読むだけでタブの切り替えとして知らせる
        h.arrange(Scene { active: Some(99), ..scene(20) });
        ev.write_all(b"workspacev2>>2,2\n").await.unwrap();
        assert_eq!(tokio::time::timeout(Duration::from_secs(2), notes.recv()).await.unwrap(), Some(Note::Focus { tab: 20, pane: None }));
        // PRELUDE でペインを閉じたら、その窓を閉じる
        h.arrange(Scene { panes: vec![], ..scene(20) });
        assert_eq!(next(&mut cmds).await, "closewindow address:0xbeef");
        h.shutdown().await;
        let _ = std::fs::remove_dir_all(&dir);
    }
}

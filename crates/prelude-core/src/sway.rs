//! sway・SwayFX の裏側([`Compositor`]。i3 の IPC)。判断 10〜12 の「コンポジタに窓を並べさせる」形。
//!
//! - タブ 1 つ = 作業領域 1 つ(名前は `<タブの番号><印>`)、ペイン 1 つ = 窓 1 つ。配置の木([`Tree`])を sway の分割の木として組む
//! - 起動はコアがする(どのプロセスの窓かを知るため)。窓が出たら、起動したプロセス・窓の名前・題名の印(Web のペイン)で見分けて
//!   そのペインのものにし、sway の印(`_prelude<印>_<ペイン>`。`_` で始まる印は題名の帯に出ない)を付ける
//! - 木はコアの木が変わったときだけ組み直す。sway の中で手で動かした窓をコアの木に戻すことはしない(使用者と取り合わない)
//! - 使用者が作業領域・窓を選んだら [`Note::Focus`] でコアに知らせる(コアの今のタブ・ペインが付いてくる)
//! - 浮いた窓(ダイアログなど)は取り込まない。sway がそのまま浮かせる(判断 12)
use crate::compositor::{Compositor, Note, Scene, TabScene, Tree, Want};
use crate::launch::{self, keys_of};
use crate::procs;
use prelude_format::Dir as FDir;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, oneshot};

// ------------------------------------------------------------------ IPC

/// i3 の IPC の種類
mod kind {
    pub const RUN_COMMAND: u32 = 0;
    pub const SUBSCRIBE: u32 = 2;
    pub const GET_TREE: u32 = 4;
    /// イベントは上の 1 ビットが立つ
    pub const EVENT: u32 = 0x8000_0000;
    pub const EV_WORKSPACE: u32 = EVENT;
    pub const EV_WINDOW: u32 = EVENT | 3;
    pub const EV_SHUTDOWN: u32 = EVENT | 6;
}

const MAGIC: &[u8; 6] = b"i3-ipc";

/// 送る 1 通(i3-ipc・長さ・種類・中身。数はこの機械の並び)
pub fn encode(kind: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(14 + payload.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(payload.len() as u32).to_ne_bytes());
    out.extend_from_slice(&kind.to_ne_bytes());
    out.extend_from_slice(payload);
    out
}

async fn read_msg(s: &mut UnixStream) -> std::io::Result<(u32, Value)> {
    let mut head = [0u8; 14];
    s.read_exact(&mut head).await?;
    if &head[..6] != MAGIC {
        return Err(std::io::Error::other("not an i3-ipc message"));
    }
    let len = u32::from_ne_bytes(head[6..10].try_into().unwrap()) as usize;
    let kind = u32::from_ne_bytes(head[10..14].try_into().unwrap());
    let mut body = vec![0u8; len];
    s.read_exact(&mut body).await?;
    Ok((kind, serde_json::from_slice(&body).unwrap_or(Value::Null)))
}

/// 頼んで返事を待つつなぎ(イベントは別のつなぎで受ける)
struct Ipc {
    s: UnixStream,
}

impl Ipc {
    async fn connect(path: &str) -> std::io::Result<Ipc> {
        Ok(Ipc { s: UnixStream::connect(path).await? })
    }
    async fn request(&mut self, kind: u32, payload: &str) -> std::io::Result<Value> {
        self.s.write_all(&encode(kind, payload.as_bytes())).await?;
        loop {
            let (k, v) = read_msg(&mut self.s).await?;
            if k == kind {
                return Ok(v);
            }
        }
    }
    async fn tree(&mut self) -> Value {
        self.request(kind::GET_TREE, "").await.unwrap_or(Value::Null)
    }
    /// コマンドを送る(; でつないで 1 通にする)。失敗したものはログに出す
    async fn run(&mut self, cmds: &[String]) {
        if cmds.is_empty() {
            return;
        }
        match self.request(kind::RUN_COMMAND, &cmds.join("; ")).await {
            Ok(Value::Array(rs)) => {
                for (r, c) in rs.iter().zip(cmds) {
                    if r["success"] != Value::Bool(true) {
                        eprintln!("prelude-core: sway refused `{c}` ({})", r["error"].as_str().unwrap_or("?"));
                    }
                }
            }
            Ok(_) => {}
            Err(e) => eprintln!("prelude-core: sway IPC failed ({e})"),
        }
    }
}

// ------------------------------------------------------------------ sway の木を読む

/// sway の窓 1 つ
#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub id: i64,
    pub pid: u32,
    /// Wayland の app_id、X11 の窓ならクラス(小文字)
    pub app: String,
    pub title: String,
    pub marks: Vec<String>,
    pub workspace: String,
    pub floating: bool,
}

/// 木の中の窓を全部(スクラッチパッドのものも)
pub fn views(tree: &Value) -> Vec<View> {
    fn walk(n: &Value, ws: &str, floating: bool, out: &mut Vec<View>) {
        let ws = if n["type"] == "workspace" { n["name"].as_str().unwrap_or("") } else { ws };
        let is_view = n["pid"].as_u64().is_some() && (n["app_id"].is_string() || n["window_properties"].is_object());
        if is_view {
            let app = n["app_id"].as_str().or_else(|| n["window_properties"]["class"].as_str()).unwrap_or("").to_lowercase();
            out.push(View {
                id: n["id"].as_i64().unwrap_or(0),
                pid: n["pid"].as_u64().unwrap_or(0) as u32,
                app,
                title: n["name"].as_str().unwrap_or("").to_string(),
                marks: n["marks"].as_array().map(|m| m.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
                workspace: ws.to_string(),
                floating,
            });
        }
        for c in n["nodes"].as_array().into_iter().flatten() {
            walk(c, ws, floating, out);
        }
        for c in n["floating_nodes"].as_array().into_iter().flatten() {
            walk(c, ws, true, out);
        }
    }
    let mut out = vec![];
    walk(tree, "", false, &mut out);
    out
}

/// 窓から根までの親(id の並び。窓が先)
fn ancestors(tree: &Value, id: i64) -> Option<Vec<i64>> {
    fn walk(n: &Value, id: i64, path: &mut Vec<i64>) -> bool {
        path.push(n["id"].as_i64().unwrap_or(0));
        if n["id"].as_i64() == Some(id) {
            return true;
        }
        for c in n["nodes"].as_array().into_iter().flatten().chain(n["floating_nodes"].as_array().into_iter().flatten()) {
            if walk(c, id, path) {
                return true;
            }
        }
        path.pop();
        false
    }
    let mut path = vec![];
    walk(tree, id, &mut path).then(|| {
        path.reverse();
        path
    })
}

// ------------------------------------------------------------------ 木を組むコマンド(計算だけ。テストする)

/// タブの作業領域の名前。番号で始めるので、sway の `workspace number <n>` のキーがそのまま効く
pub fn ws_name(num: u32, tag: &str) -> String {
    if tag.is_empty() {
        num.to_string()
    } else {
        format!("{num}:prelude{tag}")
    }
}

/// 窓のあるペインだけの木。片側が空の分割はもう片側だけになる
pub fn prune(t: &Tree, has: &impl Fn(u64) -> bool) -> Option<Tree> {
    match t {
        Tree::Pane(p) => has(*p).then_some(Tree::Pane(*p)),
        Tree::Split { dir, ratio, a, b } => match (prune(a, has), prune(b, has)) {
            (Some(a), Some(b)) => Some(Tree::Split { dir: *dir, ratio: *ratio, a: Box::new(a), b: Box::new(b) }),
            (x, None) | (None, x) => x,
        },
    }
}

/// 比率を除いた形が同じか
pub fn same_shape(x: &Tree, y: &Tree) -> bool {
    match (x, y) {
        (Tree::Pane(a), Tree::Pane(b)) => a == b,
        (Tree::Split { dir: d1, a: a1, b: b1, .. }, Tree::Split { dir: d2, a: a2, b: b2, .. }) => {
            d1 == d2 && same_shape(a1, a2) && same_shape(b1, b2)
        }
        _ => false,
    }
}

fn first(t: &Tree) -> u64 {
    match t {
        Tree::Pane(p) => *p,
        Tree::Split { a, .. } => first(a),
    }
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

/// 木を作業領域 ws に組み直すコマンド。窓をいったん置き場(tmp)に移し、最初の窓を ws に置いてから、
/// 分割ごとに「a の最初の窓を分ける → b の最初の窓をその隣(印の場所)へ」を繰り返す。どれもフォーカスを動かさない
pub fn build_cmds(t: &Tree, con: &impl Fn(u64) -> i64, mark: &impl Fn(u64) -> String, ws: &str, tmp: &str) -> Vec<String> {
    let mut ps = vec![];
    leaves(t, &mut ps);
    let mut out: Vec<String> = vec![];
    for p in &ps {
        out.push(format!("[con_id={}] floating disable", con(*p)));
        out.push(format!("[con_id={}] move container to workspace {}", con(*p), quote(tmp)));
    }
    out.push(format!("[con_id={}] move container to workspace {}", con(first(t)), quote(ws)));
    fn expand(t: &Tree, con: &impl Fn(u64) -> i64, mark: &impl Fn(u64) -> String, out: &mut Vec<String>) {
        let Tree::Split { dir, a, b, .. } = t else { return };
        let f = first(a);
        let split = if *dir == FDir::Row { "h" } else { "v" };
        out.push(format!("[con_id={}] split {split}", con(f)));
        out.push(format!("[con_id={}] move container to mark {}", con(first(b)), quote(&mark(f))));
        expand(a, con, mark, out);
        expand(b, con, mark, out);
    }
    expand(t, con, mark, &mut out);
    out
}

/// 比率を付けるコマンド。分割ごとに、a の側の容器(sway の木で、a の最初の窓と b の最初の窓が分かれる所の a 側)の幅か高さを親の割合で決める
pub fn ratio_cmds(t: &Tree, con: &impl Fn(u64) -> i64, sway: &Value) -> Vec<String> {
    let mut out = vec![];
    fn walk(t: &Tree, con: &impl Fn(u64) -> i64, sway: &Value, out: &mut Vec<String>) {
        let Tree::Split { dir, ratio, a, b } = t else { return };
        if let (Some(pa), Some(pb)) = (ancestors(sway, con(first(a))), ancestors(sway, con(first(b)))) {
            // 根の側から見て、最後に同じ所の次が a 側の容器
            let (ra, rb): (Vec<i64>, Vec<i64>) = (pa.into_iter().rev().collect(), pb.into_iter().rev().collect());
            let common = ra.iter().zip(&rb).take_while(|(x, y)| x == y).count();
            if let Some(side) = ra.get(common) {
                let what = if *dir == FDir::Row { "width" } else { "height" };
                let pct = (ratio.clamp(0.05, 0.95) * 100.0).round() as i64;
                out.push(format!("[con_id={side}] resize set {what} {pct} ppt"));
            }
        }
        walk(a, con, sway, out);
        walk(b, con, sway, out);
    }
    walk(t, con, sway, &mut out);
    out
}

/// sway のコマンドの文字列の引用
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

// ------------------------------------------------------------------ 裏側

enum Msg {
    Arrange(Scene),
    Event(u32, Value),
    ShExited(u64, u32),
    /// 窓が閉じた後、少し待っても次の窓が無ければ終わったとみなす
    Gone(u64),
    Shutdown(oneshot::Sender<()>),
}

/// sway の裏側に頼む口
#[derive(Clone)]
pub struct Handle {
    tx: mpsc::UnboundedSender<Msg>,
}

impl Compositor for Handle {
    fn name(&self) -> &'static str {
        "sway"
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

/// 始める。`SWAYSOCK`(無ければ `I3SOCK`)につなぐ。つながらなければ None
pub async fn start(tag: &str, notes: mpsc::UnboundedSender<Note>) -> Option<Handle> {
    let Some(path) = ["SWAYSOCK", "I3SOCK"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty())) else {
        eprintln!("prelude-core: SWAYSOCK is not set. Panes will not show windows");
        return None;
    };
    let (cmd, mut ev) = match (Ipc::connect(&path).await, UnixStream::connect(&path).await) {
        (Ok(c), Ok(e)) => (c, e),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("prelude-core: cannot connect to sway ({e}). Panes will not show windows");
            return None;
        }
    };
    let (tx, rx) = mpsc::unbounded_channel();
    // イベント: 作業領域(選んだ)・窓(出た・閉じた・題名・選んだ)・sway の終わり
    if ev.write_all(&encode(kind::SUBSCRIBE, br#"["workspace","window","shutdown"]"#)).await.is_err() {
        return None;
    }
    {
        let tx = tx.clone();
        tokio::spawn(async move {
            while let Ok((k, v)) = read_msg(&mut ev).await {
                if k & kind::EVENT != 0 && tx.send(Msg::Event(k, v)).is_err() {
                    break;
                }
            }
        });
    }
    let mut s = Sway {
        ipc: cmd,
        tx: tx.clone(),
        notes,
        tag: tag.to_string(),
        panes: HashMap::new(),
        scene: Scene::default(),
        applied: HashMap::new(),
        want_focus: (None, None),
        echoes: vec![],
        quiet_until: Instant::now(),
    };
    s.recover().await;
    tokio::spawn(s.run(rx));
    Some(Handle { tx })
}

struct Pane {
    want: Want,
    keys: Vec<String>,
    /// 起動した sh(終わったら None)
    sh: Option<u32>,
    sh_pid: u32,
    /// 取り込んだ窓の持ち主(閉じるときに終わらせてよい)
    owned: HashSet<u32>,
    /// 取り込んだ窓
    con: Option<i64>,
    /// 起動前からあった窓(名前での照合の対象外)
    old: HashSet<i64>,
    /// 起動前からあった窓を借りてよい(既に動いているアプリに起動を任せたとき)
    adopt: bool,
    launched_at: f64,
    t0: Instant,
    had_window: bool,
    dead: u32,
}

struct Sway {
    ipc: Ipc,
    tx: mpsc::UnboundedSender<Msg>,
    notes: mpsc::UnboundedSender<Note>,
    tag: String,
    panes: HashMap<u64, Pane>,
    scene: Scene,
    /// タブごとの、最後に組んだ木(窓のあるペインだけ)
    applied: HashMap<u64, Tree>,
    /// 最後にコアが望んだ今のタブとペイン(変わったときだけ sway に合わせる)
    want_focus: (Option<u64>, Option<u64>),
    /// sway で選ばれてコアに知らせたもの。コアから同じものが返ってきたら、もう合わせない
    echoes: Vec<(u64, Option<u64>, Instant)>,
    /// 自分で窓を動かした直後は、選ばれた知らせをコアに送らない
    quiet_until: Instant,
}

impl Sway {
    fn mark(&self, pane: u64) -> String {
        format!("_prelude{}_{pane}", self.tag)
    }
    fn mark_prefix(&self) -> String {
        format!("_prelude{}_", self.tag)
    }
    fn tmp_ws(&self) -> String {
        format!("prelude-tmp{}", self.tag)
    }

    async fn run(mut self, mut rx: mpsc::UnboundedReceiver<Msg>) {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                m = rx.recv() => {
                    let Some(m) = m else { break };
                    match m {
                        Msg::Shutdown(done) => {
                            self.shutdown().await;
                            let _ = done.send(());
                            return;
                        }
                        Msg::Arrange(scene) => self.sync(scene).await,
                        Msg::Event(k, v) => self.event(k, v).await,
                        Msg::ShExited(pane, pid) => {
                            if let Some(p) = self.panes.get_mut(&pane).filter(|p| p.sh_pid == pid) {
                                p.sh = None;
                                // 窓を出さずにすぐ終わった = 既に動いているプロセスに起動を任せた。少し後に、その窓を借りる
                                if p.con.is_none() && p.t0.elapsed() < Duration::from_secs(5) {
                                    let tx = self.tx.clone();
                                    tokio::spawn(async move {
                                        tokio::time::sleep(Duration::from_millis(1500)).await;
                                        let _ = tx.send(Msg::Event(0, serde_json::json!({ "adopt": pane })));
                                    });
                                }
                            }
                        }
                        Msg::Gone(pane) => {
                            if self.panes.get(&pane).is_some_and(|p| p.con.is_none()) {
                                self.close_pane(pane).await;
                                let _ = self.notes.send(Note::Exited { pane, had_window: true });
                            }
                        }
                    }
                }
                _ = tick.tick() => self.tick().await,
            }
        }
    }

    /// 前に落ちたときに印を付けたままの窓から、印だけ外す(閉じない。保存していない作業があるかもしれない)
    async fn recover(&mut self) {
        let prefix = self.mark_prefix();
        let tree = self.ipc.tree().await;
        let cmds: Vec<String> = views(&tree)
            .into_iter()
            .flat_map(|v| v.marks.into_iter().filter(|m| m.starts_with(&prefix)))
            .map(|m| format!("unmark {}", quote(&m)))
            .collect();
        if !cmds.is_empty() {
            eprintln!("prelude-core: releasing {} window(s) left by the last run", cmds.len());
        }
        self.ipc.run(&cmds).await;
    }

    /// コアの状態に合わせる
    async fn sync(&mut self, scene: Scene) {
        let ids: HashSet<u64> = scene.panes.iter().map(|w| w.pane).collect();
        for id in self.panes.keys().copied().filter(|id| !ids.contains(id)).collect::<Vec<_>>() {
            self.close_pane(id).await;
        }
        let mut known: Option<HashSet<i64>> = None;
        for w in &scene.panes {
            if self.panes.contains_key(&w.pane) {
                continue;
            }
            if w.marker.is_empty() && w.cmd.trim().is_empty() {
                continue;
            }
            if known.is_none() {
                known = Some(views(&self.ipc.tree().await).into_iter().map(|v| v.id).collect());
            }
            self.launch(w.clone(), known.clone().unwrap_or_default());
        }
        self.scene = scene;
        self.claim().await;
        self.layout().await;
    }

    /// 起動する(Web のペインは窓を web.rs が作るので、待つだけ)
    fn launch(&mut self, w: Want, old: HashSet<i64>) {
        let (mut sh, mut sh_pid) = (None, 0);
        if w.marker.is_empty() {
            match launch::spawn(&w.cmd, &w.cwd) {
                Ok(mut child) => {
                    let pid = child.id().unwrap_or(0);
                    let (tx, pane) = (self.tx.clone(), w.pane);
                    tokio::spawn(async move {
                        let _ = child.wait().await;
                        let _ = tx.send(Msg::ShExited(pane, pid));
                    });
                    (sh, sh_pid) = (Some(pid), pid);
                }
                Err(e) => {
                    eprintln!("prelude-core: cannot launch {} ({e})", w.cmd);
                    let _ = self.notes.send(Note::Exited { pane: w.pane, had_window: false });
                    return;
                }
            }
        }
        let p = Pane {
            keys: if w.marker.is_empty() { keys_of(&w.cmd, &w.cls) } else { vec![] },
            want: w,
            sh,
            sh_pid,
            owned: HashSet::new(),
            con: None,
            old,
            adopt: false,
            launched_at: procs::uptime(),
            t0: Instant::now(),
            had_window: false,
            dead: 0,
        };
        self.panes.insert(p.want.pane, p);
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

    /// まだ窓の無いペインに、出てきた窓を結び付ける
    async fn claim(&mut self) {
        if self.panes.values().all(|p| p.con.is_some()) {
            return;
        }
        let tree = self.ipc.tree().await;
        let all = views(&tree);
        let mut taken: HashSet<i64> = self.panes.values().filter_map(|p| p.con).collect();
        // まだ窓の無いペインと、そのプロセス(起動した順)
        let mut ids: Vec<u64> = self.panes.iter().filter(|(_, p)| p.con.is_none()).map(|(id, _)| *id).collect();
        ids.sort_unstable();
        let pids: HashMap<u64, HashSet<u32>> = ids
            .iter()
            .map(|id| {
                let others = self.others_pids(*id);
                let p = &self.panes[id];
                let mut s: HashSet<u32> = p.sh.map(procs::descendants).unwrap_or_default().into_iter().collect();
                s.extend(p.owned.iter().copied());
                if p.want.marker.is_empty() {
                    s.extend(procs::fresh_pids(&p.want.cmd, p.launched_at).into_iter().filter(|x| !others.contains(x)));
                }
                (*id, s)
            })
            .collect();
        let any_pane: HashSet<u32> =
            pids.values().flatten().copied().chain(self.panes.values().flat_map(|p| p.owned.iter().copied())).collect();
        let mut got: Vec<(u64, View)> = vec![];
        // 1 回目は確かなもの(題名の印・プロセス)だけ、2 回目に窓の名前。同じアプリを一度に開いたとき、名前で取り違えないように
        for strong in [true, false] {
            for id in &ids {
                if got.iter().any(|(g, _)| g == id) {
                    continue;
                }
                let p = &self.panes[id];
                let mine = &pids[id];
                let pick = all.iter().filter(|v| !v.floating && !taken.contains(&v.id)).find(|v| {
                    if !p.want.marker.is_empty() {
                        return strong && v.title.contains(&p.want.marker);
                    }
                    if strong {
                        return mine.contains(&v.pid);
                    }
                    !any_pane.contains(&v.pid) && (p.adopt || !p.old.contains(&v.id)) && p.keys.contains(&v.app)
                });
                if let Some(v) = pick {
                    taken.insert(v.id);
                    got.push((*id, v.clone()));
                }
            }
        }
        let mut cmds = vec![];
        for (id, v) in got {
            cmds.push(format!("[con_id={}] mark --add {}", v.id, quote(&self.mark(id))));
            let p = self.panes.get_mut(&id).unwrap();
            p.con = Some(v.id);
            p.had_window = true;
            if p.want.marker.is_empty() {
                p.owned.insert(v.pid);
                let _ = self.notes.send(Note::Title { pane: id, title: v.title.chars().take(300).collect() });
            }
            let _ = self.notes.send(Note::Claimed { pane: id });
        }
        self.ipc.run(&cmds).await;
    }

    /// タブの木と、今のタブ・ペインを sway に合わせる
    async fn layout(&mut self) {
        let con: HashMap<u64, i64> = self.panes.iter().filter_map(|(id, p)| Some((*id, p.con?))).collect();
        let tabs: Vec<TabScene> = self.scene.tabs.clone();
        let mut moved = false;
        for t in &tabs {
            let Some(want) = prune(&t.tree, &|p| con.contains_key(&p)) else {
                self.applied.remove(&t.id);
                continue;
            };
            if self.applied.get(&t.id) == Some(&want) {
                continue;
            }
            let c = |p: u64| con[&p];
            let ws = ws_name(t.num, &self.tag);
            if !self.applied.get(&t.id).is_some_and(|old| same_shape(old, &want)) {
                let m = |p: u64| self.mark(p);
                let cmds = build_cmds(&want, &c, &m, &ws, &self.tmp_ws());
                self.ipc.run(&cmds).await;
                moved = true;
            }
            if matches!(want, Tree::Split { .. }) {
                let tree = self.ipc.tree().await;
                let cmds = ratio_cmds(&want, &c, &tree);
                self.ipc.run(&cmds).await;
            }
            self.applied.insert(t.id, want);
        }
        let live: HashSet<u64> = tabs.iter().map(|t| t.id).collect();
        self.applied.retain(|id, _| live.contains(id));

        // 今のタブ・ペイン。コアの望みが変わったときだけ(使用者が sway で選んだものと取り合わない)
        let tab = self.scene.active.and_then(|a| tabs.iter().find(|t| t.id == a));
        let want = (tab.map(|t| t.id), tab.map(|t| t.active_pane));
        let changed = want != self.want_focus;
        self.want_focus = want;
        self.echoes.retain(|e| e.2.elapsed() < Duration::from_secs(2));
        let echoed = self.echoes.iter().position(|e| Some(e.0) == want.0 && (e.1.is_none() || e.1 == want.1));
        if let Some(i) = echoed {
            self.echoes.drain(..=i);
        }
        if let Some(t) = tab.filter(|_| (changed && echoed.is_none()) || moved) {
            let mut cmds = vec![format!("workspace {}", quote(&ws_name(t.num, &self.tag)))];
            if let Some(c) = con.get(&t.active_pane) {
                cmds.push(format!("[con_id={c}] focus"));
            }
            self.ipc.run(&cmds).await;
        }
        if moved {
            self.quiet_until = Instant::now() + Duration::from_millis(300);
        }
    }

    async fn event(&mut self, k: u32, v: Value) {
        if k == 0 {
            // 自分の知らせ: 起動を任せたペインに、前からある窓を借りさせる
            if let Some(p) = v["adopt"].as_u64().and_then(|id| self.panes.get_mut(&id)) {
                if p.con.is_none() {
                    p.adopt = true;
                    self.claim().await;
                    self.layout().await;
                }
            }
            return;
        }
        match k {
            kind::EV_WINDOW => {
                let id = v["container"]["id"].as_i64().unwrap_or(0);
                let owner = self.panes.iter().find(|(_, p)| p.con == Some(id)).map(|(id, _)| *id);
                match v["change"].as_str().unwrap_or("") {
                    "new" => {
                        self.claim().await;
                        self.layout().await;
                    }
                    "close" => {
                        if let Some(pane) = owner {
                            self.panes.get_mut(&pane).unwrap().con = None;
                            // すぐに次の窓を出すアプリ(窓の作り直し)に備えて、少し待つ
                            let tx = self.tx.clone();
                            tokio::spawn(async move {
                                tokio::time::sleep(Duration::from_millis(800)).await;
                                let _ = tx.send(Msg::Gone(pane));
                            });
                        }
                    }
                    "title" => {
                        if let Some(pane) = owner.filter(|p| self.panes[p].want.marker.is_empty()) {
                            let t = v["container"]["name"].as_str().unwrap_or("").chars().take(300).collect();
                            let _ = self.notes.send(Note::Title { pane, title: t });
                        }
                    }
                    "focus" => {
                        let Some(pane) = owner else { return };
                        if Instant::now() < self.quiet_until {
                            return;
                        }
                        let Some(tab) = self.scene.tabs.iter().find(|t| {
                            let mut l = vec![];
                            leaves(&t.tree, &mut l);
                            l.contains(&pane)
                        }) else {
                            return;
                        };
                        if self.want_focus != (Some(tab.id), Some(pane)) {
                            self.echoes.push((tab.id, Some(pane), Instant::now()));
                            let _ = self.notes.send(Note::Focus { tab: tab.id, pane: Some(pane) });
                        }
                    }
                    _ => {}
                }
            }
            kind::EV_WORKSPACE => {
                if v["change"] != "focus" || Instant::now() < self.quiet_until {
                    return;
                }
                let name = v["current"]["name"].as_str().unwrap_or("");
                let Some(tab) = self.scene.tabs.iter().find(|t| ws_name(t.num, &self.tag) == name) else { return };
                if self.want_focus.0 != Some(tab.id) {
                    self.echoes.push((tab.id, None, Instant::now()));
                    let _ = self.notes.send(Note::Focus { tab: tab.id, pane: None });
                }
            }
            kind::EV_SHUTDOWN => eprintln!("prelude-core: sway is shutting down"),
            _ => {}
        }
    }

    /// 1 秒ごと: 起動して 30 秒の間は窓を探し直す(プロセスが後から見つかることがある)。
    /// 窓が一度も出ないまま、20 秒たってプロセスも居なければ終わったとみなす
    async fn tick(&mut self) {
        if self.panes.values().any(|p| p.con.is_none() && p.t0.elapsed() < Duration::from_secs(30)) {
            self.claim().await;
            self.layout().await;
        }
        let mut exited = vec![];
        let ids: Vec<u64> = self.panes.keys().copied().collect();
        for id in ids {
            let others = self.others_pids(id);
            let p = self.panes.get_mut(&id).unwrap();
            if p.had_window || !p.want.marker.is_empty() || p.t0.elapsed() < Duration::from_secs(20) {
                continue;
            }
            let mut pids: Vec<u32> = p.sh.map(procs::descendants).unwrap_or_default();
            pids.extend(procs::fresh_pids(&p.want.cmd, p.launched_at).into_iter().filter(|x| !others.contains(x)));
            let alive = pids.into_iter().any(|x| Some(x) != p.sh && procs::alive(x));
            p.dead = if alive { 0 } else { p.dead + 1 };
            if p.dead >= 3 {
                exited.push(id);
            }
        }
        for id in exited {
            self.close_pane(id).await;
            let _ = self.notes.send(Note::Exited { pane: id, had_window: false });
        }
    }

    /// ペインの窓を閉じ、起動したプロセスを終わらせる。借りた窓は印だけ外す
    async fn close_pane(&mut self, id: u64) {
        let Some(p) = self.panes.remove(&id) else { return };
        let mut pids: Vec<u32> = p.sh.map(procs::descendants).unwrap_or_default();
        pids.extend(p.owned.iter().copied());
        pids.sort_unstable();
        pids.dedup();
        if let Some(c) = p.con {
            let borrowed = p.adopt && p.old.contains(&c);
            let cmd = if borrowed { format!("[con_id={c}] unmark {}", quote(&self.mark(id))) } else { format!("[con_id={c}] kill") };
            self.ipc.run(&[cmd]).await;
            if borrowed {
                return;
            }
        }
        for pid in pids.into_iter().rev() {
            procs::signal(pid, libc::SIGTERM);
        }
    }

    /// 終わる: 起動したアプリを閉じ、3 秒まで待つ。残ったものだけ強制終了する
    async fn shutdown(&mut self) {
        let mut pids: Vec<u32> = vec![];
        for p in self.panes.values() {
            pids.extend(p.sh.map(procs::descendants).unwrap_or_default());
            pids.extend(p.owned.iter().copied());
        }
        for id in self.panes.keys().copied().collect::<Vec<_>>() {
            self.close_pane(id).await;
        }
        let end = Instant::now() + Duration::from_secs(3);
        while pids.iter().any(|p| procs::alive(*p)) && Instant::now() < end {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        for p in pids.into_iter().filter(|p| procs::alive(*p)) {
            procs::signal(p, libc::SIGKILL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(a: Tree, b: Tree, ratio: f64) -> Tree {
        Tree::Split { dir: FDir::Row, ratio, a: Box::new(a), b: Box::new(b) }
    }
    fn col(a: Tree, b: Tree, ratio: f64) -> Tree {
        Tree::Split { dir: FDir::Col, ratio, a: Box::new(a), b: Box::new(b) }
    }
    use Tree::Pane as P;

    #[test]
    fn 送る形() {
        let m = encode(kind::RUN_COMMAND, b"nop");
        assert_eq!(&m[..6], b"i3-ipc");
        assert_eq!(u32::from_ne_bytes(m[6..10].try_into().unwrap()), 3);
        assert_eq!(u32::from_ne_bytes(m[10..14].try_into().unwrap()), 0);
        assert_eq!(&m[14..], b"nop");
    }

    #[test]
    fn 作業領域の名前() {
        assert_eq!(ws_name(3, ""), "3");
        assert_eq!(ws_name(3, ".d0011aabb"), "3:prelude.d0011aabb");
    }

    #[test]
    fn 窓の無いペインを除く() {
        let t = row(P(1), col(P(2), P(3), 0.3), 0.6);
        assert_eq!(prune(&t, &|p| p != 2), Some(row(P(1), P(3), 0.6)));
        assert_eq!(prune(&t, &|p| p == 3), Some(P(3)));
        assert_eq!(prune(&t, &|_| false), None);
        assert_eq!(prune(&t, &|_| true), Some(t.clone()));
    }

    #[test]
    fn 形だけを比べる() {
        let a = row(P(1), col(P(2), P(3), 0.3), 0.6);
        assert!(same_shape(&a, &row(P(1), col(P(2), P(3), 0.5), 0.5)));
        assert!(!same_shape(&a, &row(P(1), row(P(2), P(3), 0.3), 0.6)));
        assert!(!same_shape(&a, &row(P(1), col(P(3), P(2), 0.3), 0.6)));
    }

    #[test]
    fn 木を組むコマンド() {
        let t = row(P(1), col(P(2), P(3), 0.3), 0.6);
        let con = |p: u64| 100 + p as i64;
        let mark = |p: u64| format!("_prelude_{p}");
        let cmds = build_cmds(&t, &con, &mark, "1", "prelude-tmp");
        assert_eq!(
            cmds,
            vec![
                "[con_id=101] floating disable",
                r#"[con_id=101] move container to workspace "prelude-tmp""#,
                "[con_id=102] floating disable",
                r#"[con_id=102] move container to workspace "prelude-tmp""#,
                "[con_id=103] floating disable",
                r#"[con_id=103] move container to workspace "prelude-tmp""#,
                r#"[con_id=101] move container to workspace "1""#,
                "[con_id=101] split h",
                r#"[con_id=102] move container to mark "_prelude_1""#,
                "[con_id=102] split v",
                r#"[con_id=103] move container to mark "_prelude_2""#,
            ]
        );
        // ペイン 1 つなら置くだけ
        assert_eq!(build_cmds(&P(1), &con, &mark, "2", "t").last().unwrap(), r#"[con_id=101] move container to workspace "2""#);
    }

    /// sway の get_tree の形(組んだ後の Row(1, Col(2, 3)))
    fn sway_tree() -> Value {
        let view = |id: i64, pid: u64, app: &str, name: &str| json!({ "id": id, "type": "con", "pid": pid, "app_id": app, "name": name, "marks": [format!("_prelude_{}", id - 100)], "nodes": [], "floating_nodes": [] });
        json!({ "id": 1, "type": "root", "nodes": [
            { "id": 3, "type": "output", "name": "HEADLESS-1", "nodes": [
                { "id": 9, "type": "workspace", "name": "1", "nodes": [
                    view(101, 11, "foot", "a"),
                    { "id": 10, "type": "con", "layout": "splitv", "nodes": [view(102, 12, "foot", "b"), view(103, 13, "org.kde.Konsole", "c")], "floating_nodes": [] }
                ], "floating_nodes": [
                    { "id": 50, "type": "floating_con", "pid": 14, "app_id": null, "window_properties": { "class": "Gimp" }, "name": "dlg", "nodes": [], "floating_nodes": [] }
                ] }
            ] }
        ] })
    }

    #[test]
    fn 窓を読む() {
        let vs = views(&sway_tree());
        assert_eq!(vs.iter().map(|v| v.id).collect::<Vec<_>>(), vec![101, 102, 103, 50]);
        assert_eq!(vs[2].app, "org.kde.konsole");
        assert_eq!(vs[0].marks, vec!["_prelude_1"]);
        assert!(vs.iter().all(|v| v.workspace == "1"));
        assert!(vs[3].floating && vs[3].app == "gimp");
        assert!(!vs[0].floating);
    }

    #[test]
    fn 比率のコマンド() {
        let t = row(P(1), col(P(2), P(3), 0.3), 0.6);
        let con = |p: u64| 100 + p as i64;
        assert_eq!(
            ratio_cmds(&t, &con, &sway_tree()),
            vec!["[con_id=101] resize set width 60 ppt", "[con_id=102] resize set height 30 ppt"]
        );
        // a の側が分割なら、その容器
        let t = row(col(P(2), P(3), 0.25), P(1), 0.7);
        assert_eq!(
            ratio_cmds(&t, &con, &sway_tree()),
            vec!["[con_id=10] resize set width 70 ppt", "[con_id=102] resize set height 25 ppt"]
        );
    }
}

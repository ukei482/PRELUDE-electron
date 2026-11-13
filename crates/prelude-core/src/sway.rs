//! sway・SwayFX の裏側([`Compositor`]。i3 の IPC)。docs/redesign-rust.md の「窓のつなぎ」の段 4(設定の windows.level で下げられる)。
//!
//! - タブ 1 つ = 作業領域 1 つ(名前は `<タブの番号><印>`)、ペイン 1 つ = 窓 1 つ。配置の木([`Tree`])を sway の分割の木として組む
//! - 起動はコアがする(どのプロセスの窓かを知るため)。窓が出たら、起動したプロセス・窓の名前・題名の印(Web のペイン)で見分けて
//!   そのペインのものにし、sway の印(`_prelude<印>_<ペイン>`。`_` で始まる印は題名の帯に出ない)を付ける
//! - 木はコアの木が変わったときだけ組み直す。使用者が sway で動かした窓・変えた大きさは、sway の木を読んでコアの木に戻す
//!   ([`Note::Layout`]。読むのは動かした知らせのときと 1 秒ごと。sway は大きさの変更を知らせない)。戻した木は組み直さない
//! - 使用者が作業領域・窓を選んだら [`Note::Focus`] でコアに知らせる(コアの今のタブ・ペインが付いてくる)
//! - 浮いた窓(ダイアログなど)は取り込まない。sway がそのまま浮かせる(「PRELUDE を通さずに開いた窓」)
//! - PRELUDE を通さずに開いた普通の窓は、タブの作業領域に出たら、sway が置いた場所のままペインにする([`Note::Foreign`]。「PRELUDE を通さずに開いた窓」)。
//!   タブでない作業領域の窓には触らない。少し待ってから決める(起動したペインの窓が遅れて見分けられることがある)
//! - アプリのダイアログの選ぶ画面は親のある窓として扱い、すぐ浮かせる。どのプログラムでもよいように、起動の環境変数の印([`DIALOG_ENV`])で見分ける
use crate::compositor::{Compositor, Level, Note, Options, Scene, TabScene, Tree, Want};
use crate::launch::{self, keys_of};
use crate::portal::DIALOG_ENV;
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
    pub const GET_WORKSPACES: u32 = 1;
    pub const SUBSCRIBE: u32 = 2;
    pub const GET_TREE: u32 = 4;
    pub const GET_INPUTS: u32 = 100;
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
    /// 今選ばれている作業領域の名前
    async fn focused_ws(&mut self) -> Option<String> {
        let v = self.request(kind::GET_WORKSPACES, "").await.ok()?;
        focused_name(&v)
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

/// get_workspaces の返事から、選ばれている作業領域の名前
fn focused_name(v: &Value) -> Option<String> {
    v.as_array()?.iter().find(|w| w["focused"] == true)?["name"].as_str().map(str::to_string)
}

/// ペインを閉じる前に選んでおく窓: 閉じるペインがあるとき、今のタブの今のペイン(閉じるものでなく、窓があるもの)の窓
fn pre_focus(scene: &Scene, closing: &[u64], con: impl Fn(u64) -> Option<i64>) -> Option<i64> {
    if closing.is_empty() {
        return None;
    }
    let tab = scene.tabs.iter().find(|t| Some(t.id) == scene.active)?;
    if closing.contains(&tab.active_pane) {
        return None;
    }
    con(tab.active_pane)
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
    pub focused: bool,
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
                focused: n["focused"] == true,
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

/// 作業領域を全部(スクラッチパッドの `__i3_scratch` も入る)
fn workspaces(tree: &Value) -> Vec<&Value> {
    fn walk<'a>(n: &'a Value, out: &mut Vec<&'a Value>) {
        if n["type"] == "workspace" {
            out.push(n);
        } else {
            for c in n["nodes"].as_array().into_iter().flatten() {
                walk(c, out);
            }
        }
    }
    let mut out = vec![];
    walk(tree, &mut out);
    out
}

/// 全画面の窓があるか(そのときの大きさは配置の比率ではない)
fn has_fullscreen(n: &Value) -> bool {
    (n["type"] == "con" && n["fullscreen_mode"].as_u64().unwrap_or(0) != 0)
        || n["nodes"].as_array().into_iter().flatten().chain(n["floating_nodes"].as_array().into_iter().flatten()).any(has_fullscreen)
}

/// sway の容器 → 配置の木。pane_of が知らない窓は飛ばす。sway の分割は子がいくつでもあるので、右へ畳み込んで 2 つずつにする。
/// 比率は `percent`(親の中の割合。隙間の分だけ合計が 1 より小さい)を、残りの子の合計で割ったもの
pub fn read_tree(n: &Value, pane_of: &impl Fn(i64) -> Option<u64>) -> Option<Tree> {
    let nodes = n["nodes"].as_array().filter(|a| !a.is_empty());
    let Some(nodes) = nodes else { return n["id"].as_i64().and_then(pane_of).map(Tree::Pane) };
    let layout = n["layout"].as_str().unwrap_or("");
    let dir = if matches!(layout, "splitv" | "stacked") { FDir::Col } else { FDir::Row };
    let equal = matches!(layout, "tabbed" | "stacked");
    let kids: Vec<(Tree, f64)> = nodes
        .iter()
        .filter_map(|c| Some((read_tree(c, pane_of)?, if equal { 1.0 } else { c["percent"].as_f64().unwrap_or(1.0 / nodes.len() as f64) })))
        .collect();
    let mut it = kids.into_iter().rev();
    let (mut t, mut sum) = it.next()?;
    for (a, w) in it {
        sum += w;
        let ratio = if sum > 0.0 { ((w / sum) * 1000.0).round() / 1000.0 } else { 0.5 };
        t = Tree::Split { dir, ratio, a: Box::new(a), b: Box::new(t) };
    }
    Some(t)
}

/// ペインの場所(全体を 1 とする)
fn rects(t: &Tree, r: [f64; 4], out: &mut HashMap<u64, [f64; 4]>) {
    let [x, y, w, h] = r;
    match t {
        Tree::Pane(p) => {
            out.insert(*p, r);
        }
        Tree::Split { dir: FDir::Row, ratio, a, b } => {
            rects(a, [x, y, w * ratio, h], out);
            rects(b, [x + w * ratio, y, w * (1.0 - ratio), h], out);
        }
        Tree::Split { ratio, a, b, .. } => {
            rects(a, [x, y, w, h * ratio], out);
            rects(b, [x, y + h * ratio, w, h * (1.0 - ratio)], out);
        }
    }
}

/// 画面の上で同じ並びか。sway は同じ向きの分割をまとめることがあるので、木の形ではなくペインの場所を比べる。
/// 比率を付けるときの丸めがあるので、少しの差は同じとみなす
pub fn looks_same(x: &Tree, y: &Tree) -> bool {
    let (mut a, mut b) = (HashMap::new(), HashMap::new());
    rects(x, [0.0, 0.0, 1.0, 1.0], &mut a);
    rects(y, [0.0, 0.0, 1.0, 1.0], &mut b);
    a.len() == b.len() && a.iter().all(|(p, r)| b.get(p).is_some_and(|s| r.iter().zip(s).all(|(u, v)| (u - v).abs() < 0.03)))
}

// ------------------------------------------------------------------ 木を組むコマンド(計算だけ。テストする)

/// 設定の windows.gaps_inner・gaps_outer → sway のコマンド。PRELUDE の outer は「窓と画面の端の間」だが、
/// sway は端にも inner を足すので、sway の outer は outer − inner にする(負でもよい)。
/// 既定(これから作る作業領域)と、今ある作業領域(all set)の両方に掛ける
pub fn gaps_cmds(inner: Option<u32>, outer: Option<u32>) -> Vec<String> {
    let mut out = vec![];
    if let Some(i) = inner {
        out.push(format!("gaps inner {i}"));
        out.push(format!("gaps inner all set {i}"));
    }
    if let Some(o) = outer {
        let o = o as i64 - inner.unwrap_or(0) as i64;
        out.push(format!("gaps outer {o}"));
        out.push(format!("gaps outer all set {o}"));
    }
    out
}

/// 設定の windows.corner_radius → SwayFX のコマンド(素の sway は角丸を持たないので、コマンドは失敗するだけ)
pub fn corner_cmds(radius: Option<u32>) -> Vec<String> {
    radius.map(|r| vec![format!("corner_radius {r}")]).unwrap_or_default()
}

/// タブの作業領域の名前の、番号の後ろ。設定の windows.workspace_prefix と、別のソケットのコアの印から作る
pub fn ws_suffix(prefix: &str, tag: &str) -> String {
    match (prefix.is_empty(), tag.is_empty()) {
        (true, true) => String::new(),
        (false, true) => format!(":{prefix}"),
        (true, false) => format!(":prelude{tag}"),
        (false, false) => format!(":{prefix}-prelude{tag}"),
    }
}

/// タブの作業領域の名前。番号で始めるので、sway の `workspace number <n>` のキーがそのまま効く
pub fn ws_name(num: u32, suffix: &str) -> String {
    format!("{num}{suffix}")
}

/// 作業領域の名前 → タブの番号([`ws_name`] の逆)。タブにできない名前は None
pub fn ws_num(name: &str, suffix: &str) -> Option<u32> {
    let n = if suffix.is_empty() { name } else { name.strip_suffix(suffix)? };
    n.parse().ok().filter(|n| *n > 0)
}

/// 「作業領域だけ指定する形」(段 1): 窓のあるペインを、タブの作業領域へ移すコマンド。移したもの(ペイン → タブの番号)は placed に覚え、
/// 移し済みのものはもう動かさない(使用者が動かしたら、そのまま)。タブの番号が変わったら移し直す
pub fn workspace_cmds(tabs: &[TabScene], con: &HashMap<u64, i64>, suffix: &str, placed: &mut HashMap<u64, u32>) -> Vec<String> {
    let mut out = vec![];
    for t in tabs {
        let mut l = vec![];
        leaves(&t.tree, &mut l);
        for p in l {
            let Some(c) = con.get(&p) else { continue };
            if placed.get(&p) != Some(&t.num) {
                out.push(format!("[con_id={c}] move container to workspace {}", quote(&ws_name(t.num, suffix))));
                placed.insert(p, t.num);
            }
        }
    }
    out
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

/// 浮かせた選ぶ画面を、画面の中に収まる大きさで真ん中に置くコマンド
pub fn dialog_cmds(con: i64) -> Vec<String> {
    vec![format!("[con_id={con}] resize set width 60 ppt height 70 ppt"), format!("[con_id={con}] move position center")]
}

/// sway のコマンドの文字列の引用
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

// ------------------------------------------------------------------ 裏側

/// 席のキーボードを決め直すコマンド(値は変えない)。fcitx5 の入力欄が閉じると、仮想キーボードが外れて席のキーボードが無くなることがある。
/// その間に開いた窓には keymap が届かず、GPUI のアプリ(Zed など)は落ちる。sway は席にキーボードが無ければ、入力の設定を適用したときに決め直す
pub fn keyboard_cmd(inputs: &Value) -> Option<String> {
    let k = inputs.as_array()?.iter().find(|i| i["type"] == "keyboard" && i["repeat_delay"].is_u64())?;
    Some(format!("input type:keyboard repeat_delay {}", k["repeat_delay"]))
}

/// 窓が出てから、外から開いた窓と決めるまでの間。起動を動いているプロセスに任せたペインは、1.5 秒後に窓を借りる
const FOREIGN_WAIT: Duration = Duration::from_secs(2);

enum Msg {
    Arrange(Scene),
    Event(u32, Value),
    ShExited(u64, u32),
    /// 窓が閉じた後、少し待っても次の窓が無ければ終わったとみなす
    Gone(u64),
    /// 出てきた窓をもう一度見て、どのペインのものでもなければペインにする。数は見直した回数
    Foreign(i64, u32),
    /// 窓が動いた。sway の木を読んでコアの木に戻す
    Reconcile,
    /// 選ぶ画面の大きさと場所を決める
    Dialog(i64),
    /// 席のキーボードを決め直す
    Keyboard,
    Shutdown(oneshot::Sender<()>),
}

/// sway の裏側に頼む口
#[derive(Clone)]
pub struct Handle {
    tx: mpsc::UnboundedSender<Msg>,
    /// 使う段(つなぎの最大の段 4 と、設定の上限の小さいほう)
    level: Level,
}

impl Compositor for Handle {
    fn name(&self) -> &'static str {
        "sway"
    }
    fn level(&self) -> crate::compositor::Level {
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

/// 始める。`SWAYSOCK`(無ければ `I3SOCK`)につなぐ。つながらなければ None
/// opts は設定の [windows](任せる段の上限・引き取り・浮かせる窓・触らない窓・作業領域の名前の頭)
pub async fn start(tag: &str, notes: mpsc::UnboundedSender<Note>, opts: Options) -> Option<Handle> {
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
        ws_suffix: ws_suffix(&opts.workspace_prefix, tag),
        level: opts.level(Level::Follow),
        opts,
        placed: HashMap::new(),
        panes: HashMap::new(),
        scene: Scene::default(),
        applied: HashMap::new(),
        applied_num: HashMap::new(),
        fresh: HashMap::new(),
        loose: HashSet::new(),
        asked: HashSet::new(),
        want_focus: (None, None),
        echoes: vec![],
        quiet_until: Instant::now(),
    };
    let level = s.level;
    let mut gaps = gaps_cmds(s.opts.gaps_inner, s.opts.gaps_outer);
    gaps.extend(corner_cmds(s.opts.corner_radius));
    if !gaps.is_empty() {
        s.ipc.run(&gaps).await;
    }
    s.recover().await;
    tokio::spawn(s.run(rx));
    Some(Handle { tx, level })
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
    /// PRELUDE を通さずに開いた窓のペイン。起動したものではないので、窓を探さない
    foreign: bool,
}

struct Sway {
    ipc: Ipc,
    tx: mpsc::UnboundedSender<Msg>,
    notes: mpsc::UnboundedSender<Note>,
    tag: String,
    /// タブの作業領域の名前の、番号の後ろ([`ws_suffix`])
    ws_suffix: String,
    /// 使う段(「窓のつなぎ」の段)。並べる・読む・追うを、ここまでに止める
    level: Level,
    /// 設定の [windows]
    opts: Options,
    /// 段 1(作業領域だけ)で、作業領域へ移したペインとタブの番号
    placed: HashMap<u64, u32>,
    panes: HashMap<u64, Pane>,
    scene: Scene,
    /// タブごとの、最後に組んだ木(窓のあるペインだけ)
    applied: HashMap<u64, Tree>,
    /// タブごとの、最後に組んだときの番号(作業領域の名前)。番号が変わったら組み直して、窓を新しい作業領域へ移す
    applied_num: HashMap<u64, u32>,
    /// sway で作られた作業領域の木(タブの番号ごと)。コアがタブにしたら applied へ移す
    fresh: HashMap<u32, Tree>,
    /// 窓はあるが木に並んでいないペイン(使用者が浮かせた窓など)
    loose: HashSet<u64>,
    /// コアにペインにしてもらうのを待っている窓
    asked: HashSet<i64>,
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
                        Msg::Foreign(con, tries) => self.foreign(con, tries).await,
                        Msg::Reconcile => self.reconcile().await,
                        Msg::Dialog(con) => self.ipc.run(&dialog_cmds(con)).await,
                        Msg::Keyboard => {
                            if let Some(c) = self.ipc.request(kind::GET_INPUTS, "").await.ok().as_ref().and_then(keyboard_cmd) {
                                self.ipc.run(&[c]).await;
                            }
                        }
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
        let closing: Vec<u64> = self.panes.keys().copied().filter(|id| !ids.contains(id)).collect();
        // 分割したタブのペインを閉じるときは、先にコアが選んだ次のペインへフォーカスを移してから窓を閉じる。
        // 先に閉じると、sway が自分の決まりで別の窓を選んで知らせてきて、それが使用者の選んだものとして今のペインになる
        if let Some(c) = pre_focus(&scene, &closing, |p| self.panes.get(&p).and_then(|w| w.con)) {
            self.ipc.run(&[format!("[con_id={c}] focus")]).await;
        }
        for id in closing {
            self.close_pane(id).await;
        }
        let mut known: Option<HashSet<i64>> = None;
        let mut adopted = vec![];
        for w in &scene.panes {
            if self.panes.contains_key(&w.pane) {
                continue;
            }
            if let Some(c) = w.window {
                self.take_window(w.clone(), c, &mut adopted).await;
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
        // 今の sway の並びから作った木は、組み直さない
        let con: HashMap<u64, i64> = self.panes.iter().filter_map(|(id, p)| Some((*id, p.con?))).collect();
        let loose = self.loose.clone();
        for t in &self.scene.tabs {
            let shown = || prune(&t.tree, &|p| con.contains_key(&p) && !loose.contains(&p));
            let fresh = self.fresh.remove(&t.num);
            let mut l = vec![];
            leaves(&t.tree, &mut l);
            if adopted.iter().any(|p| l.contains(p)) || (fresh.is_some() && !self.applied.contains_key(&t.id)) {
                if let Some(s) = shown() {
                    self.applied.insert(t.id, s);
                    self.applied_num.insert(t.id, t.num);
                }
            }
        }
        self.fresh.clear();
        self.claim().await;
        self.layout().await;
    }

    /// PRELUDE を通さずに開いた窓をペインにする。もう閉じていたら、コアに知らせる
    async fn take_window(&mut self, w: Want, c: i64, adopted: &mut Vec<u64>) {
        self.asked.remove(&c);
        let id = w.pane;
        let tree = self.ipc.tree().await;
        let Some(v) = views(&tree).into_iter().find(|v| v.id == c) else {
            let _ = self.notes.send(Note::Exited { pane: id, had_window: true });
            return;
        };
        self.ipc.run(&[format!("[con_id={c}] mark --add {}", quote(&self.mark(id)))]).await;
        let p = Pane {
            keys: vec![],
            want: w,
            sh: None,
            sh_pid: 0,
            owned: HashSet::new(),
            con: Some(c),
            old: HashSet::new(),
            adopt: false,
            launched_at: 0.0,
            t0: Instant::now(),
            had_window: true,
            dead: 0,
            foreign: true,
        };
        self.panes.insert(id, p);
        adopted.push(id);
        let _ = self.notes.send(Note::Title { pane: id, title: v.title.chars().take(300).collect() });
        let _ = self.notes.send(Note::Claimed { pane: id });
    }

    /// 出てきた窓がどのペインのものでもなければ、コアにペインにしてもらう
    async fn foreign(&mut self, con: i64, tries: u32) {
        self.claim().await;
        self.layout().await;
        // PRELUDE を通さずに開いた窓を引き取るのは、段 4(追う)で、設定の windows.adopt が tabs のときだけ
        if self.level < Level::Follow || !self.opts.adopt {
            return;
        }
        if self.asked.contains(&con) || self.panes.values().any(|p| p.con == Some(con)) {
            return;
        }
        let tree = self.ipc.tree().await;
        let Some(v) = views(&tree).into_iter().find(|v| v.id == con).filter(|v| !v.floating) else { return };
        // PRELUDE の UI(サイドバー・ランチャー)の窓はペインではない。設定で触らない窓も
        if crate::compositor::is_prelude_ui(&v.app) || self.opts.ignores(&v.app) {
            return;
        }
        // Web のペインの窓は、題名の印が付くまで見分けられない
        let web_pending = self.panes.values().any(|p| !p.want.marker.is_empty() && p.con.is_none());
        if web_pending && v.app.contains("chrom") && tries < 10 {
            self.later(Msg::Foreign(con, tries + 1), Duration::from_secs(2));
            return;
        }
        let Some(tab) = self.scene.tabs.iter().find(|t| ws_name(t.num, &self.ws_suffix) == v.workspace) else { return };
        let mine: HashMap<i64, u64> = self.panes.iter().filter_map(|(id, p)| Some((p.con?, *id))).chain([(con, 0)]).collect();
        let Some(ws) = workspaces(&tree).into_iter().find(|w| w["name"] == v.workspace.as_str()) else { return };
        let Some(t) = read_tree(ws, &|c| mine.get(&c).copied()) else { return };
        self.asked.insert(con);
        let _ = self.notes.send(Note::Foreign { tab: tab.id, con, app: v.app, title: v.title, focused: v.focused, tree: t });
    }

    /// 少し後に自分へ知らせる
    fn later(&self, m: Msg, d: Duration) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(d).await;
            let _ = tx.send(m);
        });
    }

    /// sway の今の並びを読み、組んだ木と違っていたら(使用者が動かした)コアの木に戻す
    async fn reconcile(&mut self) {
        // 使用者が動かした並びを読んでコアに合わせるのは、段 3(読む)から
        if self.level < Level::Read {
            return;
        }
        let mine: HashMap<i64, u64> = self.panes.iter().filter_map(|(id, p)| Some((p.con?, *id))).collect();
        // 窓が閉じたばかりのペインは、次の窓を待っている間(窓を作り直すアプリ)。今の木はまだ確かでない
        if mine.is_empty() || self.panes.values().any(|p| p.con.is_none() && p.had_window) {
            return;
        }
        let tree = self.ipc.tree().await;
        let spaces = workspaces(&tree);
        if spaces.iter().any(|w| has_fullscreen(w)) {
            return;
        }
        let tmp = self.tmp_ws();
        let mut read: HashMap<String, Tree> = spaces
            .iter()
            .filter_map(|w| {
                let name = w["name"].as_str()?;
                (name != tmp).then_some(())?;
                Some((name.to_string(), read_tree(w, &|c| mine.get(&c).copied())?))
            })
            .collect();
        let mut changes = vec![];
        let mut placed = vec![];
        for t in &self.scene.tabs {
            // 番号を変えたばかりで、まだ窓を新しい作業領域へ移していないタブは読まない(窓が無くなったと取り違えないように)
            if self.applied_num.get(&t.id).is_some_and(|n| *n != t.num) {
                continue;
            }
            let cur = read.remove(&ws_name(t.num, &self.ws_suffix));
            if let Some(c) = &cur {
                leaves(c, &mut placed);
            }
            match (&cur, self.applied.get(&t.id)) {
                (None, None) => continue,
                (Some(c), Some(a)) if looks_same(c, a) => continue,
                _ => {}
            }
            match &cur {
                Some(c) => self.applied.insert(t.id, c.clone()),
                None => self.applied.remove(&t.id),
            };
            self.applied_num.insert(t.id, t.num);
            changes.push((t.num, cur));
        }
        // タブでない作業領域へ移した窓: 番号の作業領域なら新しいタブ、そうでなければ木に並んでいないものとして扱う
        for (name, t) in read {
            if let Some(num) = ws_num(&name, &self.ws_suffix) {
                leaves(&t, &mut placed);
                self.fresh.insert(num, t.clone());
                changes.push((num, Some(t)));
            }
        }
        self.loose = mine.values().copied().filter(|p| !placed.contains(p)).collect();
        if !changes.is_empty() {
            let _ = self.notes.send(Note::Layout(changes));
        }
    }

    /// 起動する(Web のペインは窓を web.rs が作るので、待つだけ)
    fn launch(&mut self, w: Want, old: HashSet<i64>) {
        let (mut sh, mut sh_pid) = (None, 0);
        // ランチャーが閉じるのはこの後なので、少し待ってからも決め直す
        for ms in [0, 250, 600] {
            self.later(Msg::Keyboard, Duration::from_millis(ms));
        }
        if w.marker.is_empty() {
            match launch::spawn(&w.cmd, &w.cwd, w.pane) {
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
            foreign: false,
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
        if self.panes.values().all(|p| p.con.is_some() || p.foreign) {
            return;
        }
        let tree = self.ipc.tree().await;
        let all = views(&tree);
        let mut taken: HashSet<i64> = self.panes.values().filter_map(|p| p.con).collect();
        // まだ窓の無いペインと、そのプロセス(起動した順)
        let mut ids: Vec<u64> = self.panes.iter().filter(|(_, p)| p.con.is_none() && !p.foreign).map(|(id, _)| *id).collect();
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
                let pick = all.iter().filter(|v| !v.floating && !taken.contains(&v.id) && !self.opts.ignores(&v.app)).find(|v| {
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
        let con: HashMap<u64, i64> =
            self.panes.iter().filter(|(id, _)| !self.loose.contains(id)).filter_map(|(id, p)| Some((*id, p.con?))).collect();
        let tabs: Vec<TabScene> = self.scene.tabs.clone();
        let mut moved = false;
        // 段 1(作業領域だけ): 窓をタブの作業領域へ移すだけで、分割・比率は組まない
        if self.level == Level::Workspace {
            let suffix = self.ws_suffix.clone();
            let cmds = workspace_cmds(&tabs, &con, &suffix, &mut self.placed);
            if !cmds.is_empty() {
                self.ipc.run(&cmds).await;
                moved = true;
            }
            let live: HashSet<u64> = self.panes.keys().copied().collect();
            self.placed.retain(|p, _| live.contains(p));
        }
        for t in tabs.iter().filter(|_| self.level >= Level::Arrange) {
            // 番号(作業領域の名前)が変わったタブは、木が同じでも組み直して、窓を新しい作業領域へ移す
            if self.applied_num.get(&t.id).is_some_and(|n| *n != t.num) {
                self.applied.remove(&t.id);
            }
            let Some(want) = prune(&t.tree, &|p| con.contains_key(&p)) else {
                self.applied.remove(&t.id);
                self.applied_num.insert(t.id, t.num);
                continue;
            };
            if self.applied.get(&t.id) == Some(&want) {
                continue;
            }
            let c = |p: u64| con[&p];
            let ws = ws_name(t.num, &self.ws_suffix);
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
            self.applied_num.insert(t.id, t.num);
        }
        let live: HashSet<u64> = tabs.iter().map(|t| t.id).collect();
        self.applied.retain(|id, _| live.contains(id));
        self.applied_num.retain(|id, _| live.contains(id));

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
            let mut cmds = vec![format!("workspace {}", quote(&ws_name(t.num, &self.ws_suffix)))];
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
                        if v["container"]["pid"].as_u64().is_some_and(|p| procs::has_env(p as u32, DIALOG_ENV)) {
                            // タイルの大きさのまま浮くと画面からはみ出すので、大きさと場所も決める。
                            // 窓が出た直後の resize は効かないので、少し待つ
                            self.ipc.run(&[format!("[con_id={id}] floating enable")]).await;
                            self.later(Msg::Dialog(id), Duration::from_millis(300));
                            return;
                        }
                        // 設定でいつも浮かせる窓は、浮かせるだけ(浮いた窓はペインに取り込まない)
                        let app = v["container"]["app_id"]
                            .as_str()
                            .or_else(|| v["container"]["window_properties"]["class"].as_str())
                            .unwrap_or("")
                            .to_string();
                        if self.opts.floats(&app) {
                            self.ipc.run(&[format!("[con_id={id}] floating enable")]).await;
                            return;
                        }
                        self.claim().await;
                        self.layout().await;
                        self.later(Msg::Foreign(id, 0), FOREIGN_WAIT);
                    }
                    "floating" | "move" => {
                        // 浮いていた窓をタイルにしたときは、外から開いた窓と同じに扱う
                        if owner.is_none() && v["change"] == "floating" {
                            self.later(Msg::Foreign(id, 0), Duration::from_millis(100));
                        }
                        self.later(Msg::Reconcile, Duration::from_millis(200));
                    }
                    "close" => {
                        self.asked.remove(&id);
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
                        // 使用者が選んだ窓をコアの今のペインにするのは、段 3(読む)から
                        if self.level < Level::Read {
                            return;
                        }
                        let Some(pane) = owner else { return };
                        if Instant::now() < self.quiet_until {
                            return;
                        }
                        let Some((tab, num)) = self
                            .scene
                            .tabs
                            .iter()
                            .find(|t| {
                                let mut l = vec![];
                                leaves(&t.tree, &mut l);
                                l.contains(&pane)
                            })
                            .map(|t| (t.id, t.num))
                        else {
                            return;
                        };
                        // サイドバーを押した後の切り替えの途中で、sway は元の窓を選び直して知らせてくる。
                        // 今もその作業領域が選ばれているときだけ、使用者が選んだものとして扱う
                        if self.ipc.focused_ws().await.as_deref() != Some(ws_name(num, &self.ws_suffix).as_str()) {
                            return;
                        }
                        // 開いたばかりのペインの窓を待っている間に、ランチャーが閉じて元の窓が選ばれ直しても、今のペインを戻さない。
                        // 窓が出たら layout がそのペインを選ぶ
                        if self.want_focus.0 == Some(tab)
                            && self.want_focus.1.and_then(|p| self.panes.get(&p)).is_some_and(|p| p.con.is_none() && !p.had_window)
                        {
                            return;
                        }
                        if self.want_focus != (Some(tab), Some(pane)) {
                            self.echoes.push((tab, Some(pane), Instant::now()));
                            let _ = self.notes.send(Note::Focus { tab, pane: Some(pane) });
                        }
                    }
                    _ => {}
                }
            }
            kind::EV_WORKSPACE => {
                if v["change"] != "focus" || Instant::now() < self.quiet_until || self.level < Level::Read {
                    return;
                }
                let name = v["current"]["name"].as_str().unwrap_or("").to_string();
                let Some(tab) = self.scene.tabs.iter().find(|t| ws_name(t.num, &self.ws_suffix) == name).map(|t| t.id) else { return };
                if self.ipc.focused_ws().await.as_deref() != Some(name.as_str()) {
                    return;
                }
                if self.want_focus.0 != Some(tab) {
                    self.echoes.push((tab, None, Instant::now()));
                    let _ = self.notes.send(Note::Focus { tab, pane: None });
                }
            }
            kind::EV_SHUTDOWN => eprintln!("prelude-core: sway is shutting down"),
            _ => {}
        }
    }

    /// 1 秒ごと: 起動して 30 秒の間は窓を探し直す(プロセスが後から見つかることがある)。
    /// 窓が一度も出ないまま、20 秒たってプロセスも居なければ終わったとみなす
    async fn tick(&mut self) {
        if self.panes.values().any(|p| p.con.is_none() && !p.foreign && p.t0.elapsed() < Duration::from_secs(30)) {
            self.claim().await;
            self.layout().await;
        }
        self.reconcile().await;
        let mut exited = vec![];
        let ids: Vec<u64> = self.panes.keys().copied().collect();
        for id in ids {
            let others = self.others_pids(id);
            let p = self.panes.get_mut(&id).unwrap();
            if p.had_window || p.foreign || !p.want.marker.is_empty() || p.t0.elapsed() < Duration::from_secs(20) {
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
        // 外から開いた窓は PRELUDE のものではないので、印だけ外して残す
        for (id, p) in self.panes.iter().filter(|(_, p)| p.foreign) {
            if let Some(c) = p.con {
                let cmd = format!("[con_id={c}] unmark {}", quote(&self.mark(*id)));
                self.ipc.run(&[cmd]).await;
            }
        }
        self.panes.retain(|_, p| !p.foreign);
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

    fn con(id: i64, percent: f64) -> Value {
        json!({ "id": id, "type": "con", "pid": 1, "app_id": "foot", "name": "x", "percent": percent, "nodes": [], "floating_nodes": [] })
    }
    fn split(id: i64, layout: &str, percent: f64, nodes: Vec<Value>) -> Value {
        json!({ "id": id, "type": "con", "layout": layout, "percent": percent, "nodes": nodes, "floating_nodes": [] })
    }

    #[test]
    fn sway_の木を配置にする() {
        // 実際の sway の形: 隙間の分だけ percent の合計は 1 より小さい
        let ws = json!({ "id": 4, "type": "workspace", "layout": "splith", "percent": null, "floating_nodes": [], "nodes": [
            con(5, 0.3), split(7, "splitv", 0.697, vec![con(6, 0.498), con(8, 0.497)])
        ] });
        let of = |c: i64| Some(c as u64);
        assert_eq!(read_tree(&ws, &of), Some(row(P(5), col(P(6), P(8), 0.501), 0.301)));
        // 知らない窓は飛ばす
        assert_eq!(read_tree(&ws, &|c| (c != 5).then_some(c as u64)), Some(col(P(6), P(8), 0.501)));
        assert_eq!(read_tree(&ws, &|_| None), None);
        // 3 つ並びは右へ畳む
        let ws = json!({ "id": 4, "type": "workspace", "layout": "splith", "floating_nodes": [], "nodes": [con(1, 0.25), con(2, 0.25), con(3, 0.5)] });
        assert_eq!(read_tree(&ws, &of), Some(row(P(1), row(P(2), P(3), 0.333), 0.25)));
        // タブの並びは大きさを持たない
        let ws = json!({ "id": 4, "type": "workspace", "layout": "tabbed", "floating_nodes": [], "nodes": [con(1, 1.0), con(2, 0.0)] });
        assert_eq!(read_tree(&ws, &of), Some(row(P(1), P(2), 0.5)));
        // 空の作業領域
        assert_eq!(read_tree(&json!({ "id": 4, "type": "workspace", "nodes": [] }), &|_| None), None);
    }

    #[test]
    fn 画面の上の並びを比べる() {
        // 木の形が違っても、同じ場所なら同じ
        assert!(looks_same(&row(row(P(1), P(2), 0.5), P(3), 0.667), &row(P(1), row(P(2), P(3), 0.5), 0.333)));
        // 少しの差は同じ
        assert!(looks_same(&row(P(1), P(2), 0.6), &row(P(1), P(2), 0.61)));
        assert!(!looks_same(&row(P(1), P(2), 0.6), &row(P(1), P(2), 0.4)));
        assert!(!looks_same(&row(P(1), P(2), 0.5), &col(P(1), P(2), 0.5)));
        assert!(!looks_same(&row(P(1), P(2), 0.5), &row(P(2), P(1), 0.5)));
        assert!(!looks_same(&row(P(1), P(2), 0.5), &P(1)));
    }

    #[test]
    fn 選ぶ画面は真ん中に浮かせる() {
        assert_eq!(dialog_cmds(7)[0], "[con_id=7] resize set width 60 ppt height 70 ppt");
        assert_eq!(dialog_cmds(7).last().unwrap(), "[con_id=7] move position center");
    }

    #[test]
    fn 席のキーボードを決め直すコマンド() {
        let v = json!([{ "type": "pointer" }, { "type": "keyboard", "repeat_delay": 600, "repeat_rate": 25 }]);
        assert_eq!(keyboard_cmd(&v).as_deref(), Some("input type:keyboard repeat_delay 600"));
        assert_eq!(keyboard_cmd(&json!([{ "type": "pointer" }])), None);
    }

    #[test]
    fn ペインを閉じる前に次のペインを選ぶ() {
        let tab = |active_pane| TabScene { id: 1, num: 1, tree: P(2), active_pane };
        let scene = Scene { panes: vec![], tabs: vec![tab(2)], active: Some(1) };
        let con = |p: u64| (p == 2).then_some(20);
        // 閉じるものがあり、今のペインに窓があれば、その窓を先に選ぶ
        assert_eq!(pre_focus(&scene, &[3], con), Some(20));
        // 閉じるものが無い・今のペインそのものを閉じる・窓がまだ無い・今のタブが無いときは何もしない
        assert_eq!(pre_focus(&scene, &[], con), None);
        assert_eq!(pre_focus(&scene, &[2], con), None);
        assert_eq!(pre_focus(&scene, &[3], |_| None), None);
        let none = Scene { active: None, ..scene.clone() };
        assert_eq!(pre_focus(&none, &[3], con), None);
    }

    #[test]
    fn 選ばれている作業領域() {
        let v = json!([{ "name": "1", "focused": false }, { "name": "3", "focused": true }]);
        assert_eq!(focused_name(&v).as_deref(), Some("3"));
        assert_eq!(focused_name(&json!([])), None);
    }

    #[test]
    fn 作業領域の名前からタブの番号() {
        assert_eq!(ws_num("3", ""), Some(3));
        assert_eq!(ws_num("3:prelude.d01", &ws_suffix("", ".d01")), Some(3));
        assert_eq!(ws_num("3", &ws_suffix("", ".d01")), None);
        assert_eq!(ws_num("3:work", &ws_suffix("work", "")), Some(3));
        assert_eq!(ws_num("mail", ""), None);
        assert_eq!(ws_num("0", ""), None);
        assert_eq!(ws_num("__i3_scratch", ""), None);
    }

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
        assert_eq!(ws_name(3, &ws_suffix("", ".d0011aabb")), "3:prelude.d0011aabb");
        // 設定の windows.workspace_prefix。番号は先頭のまま($mod+数字 のキーが効く)
        assert_eq!(ws_name(3, &ws_suffix("work", "")), "3:work");
        assert_eq!(ws_name(3, &ws_suffix("work", ".d1")), "3:work-prelude.d1");
    }

    #[test]
    fn 間のコマンド() {
        assert!(gaps_cmds(None, None).is_empty());
        assert_eq!(gaps_cmds(Some(8), None), vec!["gaps inner 8", "gaps inner all set 8"]);
        // 端までの間 12 = sway の outer 4 + inner 8
        assert_eq!(gaps_cmds(Some(8), Some(12))[2..], ["gaps outer 4", "gaps outer all set 4"]);
        // 窓どうしは 8、端は 0: sway の outer は負
        assert_eq!(gaps_cmds(Some(8), Some(0))[2], "gaps outer -8");
    }

    #[test]
    fn 角丸のコマンド() {
        assert!(corner_cmds(None).is_empty());
        assert_eq!(corner_cmds(Some(0)), vec!["corner_radius 0"]);
        assert_eq!(corner_cmds(Some(10)), vec!["corner_radius 10"]);
    }

    #[test]
    fn 作業領域だけ指定する形の移すコマンド() {
        let tabs = vec![
            TabScene { id: 1, num: 2, tree: row(P(10), P(11), 0.5), active_pane: 10 },
            TabScene { id: 2, num: 5, tree: P(12), active_pane: 12 },
        ];
        let con: HashMap<u64, i64> = [(10, 100), (12, 120)].into_iter().collect(); // 11 はまだ窓が無い
        let mut placed = HashMap::new();
        let cmds = workspace_cmds(&tabs, &con, "", &mut placed);
        assert_eq!(cmds, vec!["[con_id=100] move container to workspace \"2\"", "[con_id=120] move container to workspace \"5\""]);
        // 移し済みは動かさない(使用者が動かしても戻さない)
        assert!(workspace_cmds(&tabs, &con, "", &mut placed).is_empty());
        // 窓が出たペインと、番号が変わったタブは移す
        let con: HashMap<u64, i64> = [(10, 100), (11, 110), (12, 120)].into_iter().collect();
        let tabs2 = vec![tabs[0].clone(), TabScene { num: 7, ..tabs[1].clone() }];
        assert_eq!(
            workspace_cmds(&tabs2, &con, ":w", &mut placed),
            vec!["[con_id=110] move container to workspace \"2:w\"", "[con_id=120] move container to workspace \"7:w\""]
        );
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

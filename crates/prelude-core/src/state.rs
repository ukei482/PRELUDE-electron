//! コアの状態(スペース・タブ・分割・開いているブックマーク)と、コマンドの処理。
//! 画面・ソケットには触らない(テストは tests/state.rs)。ファイル(スペースのフォルダ・`.prelude`)は読み書きする。
//!
//! - タブはスペースごと。切り替えると今のスペースのタブはしまい、切り替え先のタブを出す(取り込んだ窓を閉じるのは段階 3)
//! - ペインの id・タブの id はコアの中で一意(スペースをまたいでも重ならない)。タブの番号はスペースの中で 1 から、空いた番号を使う
//! - 残すもの(スペース・タブ・分割・開いているブックマーク)は [`Saved`] にして書き出す。ペインの場所(UI から来る)は残さない
use crate::embed::Want;
use crate::recipes::{self, Recipe};
use crate::web::WebWant;
use prelude_format::{Bookmark, Dir as FDir, Leaf, Node, Resolver, Space, Table, Value};
use prelude_proto::{BookmarkInfo, BookmarkOpened, Command, Dir, ErrorCode, PaneRect, PaneSpec, RecipeInfo, State, TabInfo, TabRef};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value as Json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// 既定のスペース(スペースが 1 つも無いときに作る)
pub const SPACE_DEFAULT: &str = "Main";
/// 読む `.prelude` の大きさの上限
const BM_MAX: u64 = 256 * 1024;
/// スペースの中で読むブックマークの数の上限
const BM_LIST_MAX: usize = 1000;
const PATH_KEYS: [&str; 5] = ["files", "open", "save", "download", "terminal"];

pub type Fail = (ErrorCode, String);
pub type Reply = Result<Json, Fail>;

fn fail(code: ErrorCode, msg: impl Into<String>) -> Fail {
    (code, msg.into())
}
fn not_found(what: &str) -> Fail {
    fail(ErrorCode::NotFound, format!("No such {what}"))
}

// ------------------------------------------------------------------ 配置

/// タブの配置。ペインに id が付いた [`Node`]
#[derive(Debug, Clone, PartialEq)]
pub enum Layout {
    Split { dir: FDir, ratio: f64, a: Box<Layout>, b: Box<Layout> },
    Pane { id: u64, leaf: Leaf },
}

impl Layout {
    fn from_node(n: Node, ids: &mut u64) -> Layout {
        match n {
            Node::Split { dir, ratio, a, b } => {
                Layout::Split { dir, ratio, a: Box::new(Layout::from_node(*a, ids)), b: Box::new(Layout::from_node(*b, ids)) }
            }
            Node::Leaf(leaf) => {
                *ids += 1;
                Layout::Pane { id: *ids, leaf }
            }
        }
    }

    pub fn to_node(&self) -> Node {
        match self {
            Layout::Split { dir, ratio, a, b } => {
                Node::Split { dir: *dir, ratio: *ratio, a: Box::new(a.to_node()), b: Box::new(b.to_node()) }
            }
            Layout::Pane { leaf, .. } => Node::Leaf(leaf.clone()),
        }
    }

    pub fn panes(&self) -> Vec<(u64, &Leaf)> {
        match self {
            Layout::Split { a, b, .. } => [a.panes(), b.panes()].concat(),
            Layout::Pane { id, leaf } => vec![(*id, leaf)],
        }
    }

    fn first(&self) -> u64 {
        match self {
            Layout::Split { a, .. } => a.first(),
            Layout::Pane { id, .. } => *id,
        }
    }

    fn has(&self, pane: u64) -> bool {
        self.panes().iter().any(|(id, _)| *id == pane)
    }

    fn leaf_mut(&mut self, pane: u64) -> Option<&mut Leaf> {
        match self {
            Layout::Split { a, b, .. } => a.leaf_mut(pane).or_else(|| b.leaf_mut(pane)),
            Layout::Pane { id, leaf } => (*id == pane).then_some(leaf),
        }
    }

    /// pane を f(元のペイン) に置き換える。見つかれば true
    fn replace(&mut self, pane: u64, f: &mut dyn FnMut(Layout) -> Layout) -> bool {
        match self {
            Layout::Split { a, b, .. } => a.replace(pane, f) || b.replace(pane, f),
            Layout::Pane { id, .. } if *id == pane => {
                let old = std::mem::replace(self, Layout::Pane { id: 0, leaf: Leaf { kind: String::new(), fields: Table::new() } });
                *self = f(old);
                true
            }
            Layout::Pane { .. } => false,
        }
    }

    /// pane を消し、その分割をもう片方に置き換える。残った側の最初のペインを返す。分割の中に無ければ None
    fn remove(&mut self, pane: u64) -> Option<u64> {
        let Layout::Split { a, b, .. } = self else { return None };
        let keep = if matches!(**a, Layout::Pane { id, .. } if id == pane) {
            Some(std::mem::replace(&mut **b, Layout::Pane { id: 0, leaf: empty_leaf() }))
        } else if matches!(**b, Layout::Pane { id, .. } if id == pane) {
            Some(std::mem::replace(&mut **a, Layout::Pane { id: 0, leaf: empty_leaf() }))
        } else {
            None
        };
        if let Some(keep) = keep {
            let first = keep.first();
            *self = keep;
            return Some(first);
        }
        let Layout::Split { a, b, .. } = self else { unreachable!() };
        a.remove(pane).or_else(|| b.remove(pane))
    }

    /// pane を含む一番近い分割の比率
    fn ratio_mut(&mut self, pane: u64) -> Option<&mut f64> {
        let Layout::Split { a, b, ratio, .. } = self else { return None };
        let direct = |n: &Layout| matches!(n, Layout::Pane { id, .. } if *id == pane);
        if direct(a.as_ref()) || direct(b.as_ref()) {
            return Some(ratio);
        }
        if a.has(pane) {
            a.ratio_mut(pane)
        } else {
            b.ratio_mut(pane)
        }
    }

    /// 約束(prelude-proto)の形。`.prelude` の `[layout]` と同じで、ペインに `id` が付く
    pub fn to_json(&self) -> Json {
        match self {
            Layout::Split { dir, ratio, a, b } => json!({
                "k": "split", "dir": if *dir == FDir::Col { "col" } else { "row" }, "ratio": ratio, "a": a.to_json(), "b": b.to_json(),
            }),
            Layout::Pane { id, leaf } => {
                let mut v = serde_json::to_value(Node::Leaf(leaf.clone()).to_value("")).unwrap_or(Json::Null);
                if let Some(o) = v.as_object_mut() {
                    o.insert("id".into(), json!(id));
                }
                v
            }
        }
    }
}

fn empty_leaf() -> Leaf {
    Leaf { kind: String::new(), fields: Table::new() }
}

/// 約束のペイン → 配置のペイン
fn leaf_of(spec: &PaneSpec) -> Result<Leaf, Fail> {
    let kind = spec.kind.trim();
    if kind.is_empty() {
        return Err(fail(ErrorCode::BadRequest, "Pane type is empty"));
    }
    let mut fields = Table::new();
    for (k, v) in &spec.fields {
        if matches!(k.as_str(), "k" | "id") {
            continue;
        }
        if let Some(v) = json_to_toml(v) {
            fields.insert(k.clone(), v);
        }
    }
    Ok(Leaf { kind: kind.to_string(), fields })
}

fn json_to_toml(v: &Json) -> Option<Value> {
    serde_json::from_value::<Value>(v.clone()).ok()
}

/// ペインの名前(タブの名前に使う)
fn leaf_title(l: &Leaf) -> String {
    let base = |p: &str| p.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
    let s = match l.kind.as_str() {
        "web" => {
            let u = l.str("url");
            u.split("://").nth(1).unwrap_or(u).split('/').next().unwrap_or("").to_string()
        }
        "files" | "field" => base(l.str("path")),
        "app" => l.str("name").to_string(),
        _ => String::new(),
    };
    if s.is_empty() {
        l.kind.clone()
    } else {
        s
    }
}

// ------------------------------------------------------------------ タブ

#[derive(Debug, Clone, PartialEq)]
pub struct Tab {
    pub id: u64,
    pub num: u32,
    pub layout: Layout,
    pub active_pane: u64,
    /// 開いたブックマーク(`.prelude` のパス)と、そのタイトル
    pub bookmark: Option<(String, String)>,
}

impl Tab {
    fn title(&self) -> String {
        match &self.bookmark {
            Some((_, t)) if !t.is_empty() => t.clone(),
            _ => self.layout.panes().iter().map(|(_, l)| leaf_title(l)).collect::<Vec<_>>().join(" | "),
        }
    }
}

/// スペース 1 つのタブ
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tabs {
    pub tabs: Vec<Tab>,
    pub active: Option<u64>,
}

// ------------------------------------------------------------------ 書き出す形

/// 残す状態(`~/.local/state/prelude/core.json`)。使用者が決めたものではないので、スペースのフォルダには書かない(判断 5)
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub space: String,
    pub spaces: BTreeMap<String, SavedTabs>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedTabs {
    /// 今のタブの位置
    pub active: usize,
    pub tabs: Vec<SavedTab>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedTab {
    pub num: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bookmark: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// 今のペインの位置(配置のペインの順)
    #[serde(default)]
    pub active_pane: usize,
    /// `.prelude` の `[layout]` と同じ形(ホームの下は `~/…`)
    pub layout: Json,
}

// ------------------------------------------------------------------ コア

pub struct Core {
    home: String,
    spaces_root: PathBuf,
    space: String,
    tabs: Tabs,
    /// ほかのスペースのタブ
    others: BTreeMap<String, Tabs>,
    /// 最後に配った id(タブとペインで共通)
    last_id: u64,
    /// UI が知らせたペインの場所。残さない
    places: HashMap<u64, PaneRect>,
    /// ペインのレシピ(窓を取り込むペインの種類)
    recipes: Vec<Recipe>,
    /// 窓が閉じて止まったペイン(タブを開き直すと起動し直す)。残さない
    stopped: HashSet<u64>,
    /// 取り込んだ窓の題名。残さない
    titles: HashMap<u64, String>,
    /// 起動コマンドがインストール済みのアプリのものか(テストで差し替える)
    installed: fn(&str) -> bool,
    /// このコアの印(Web のペインの窓の題名の印に入れる)
    tag: String,
}

/// コマンドの結果。changed なら状態が変わった(イベントを配り、書き出す)
pub struct Outcome {
    pub reply: Reply,
    pub changed: bool,
}

impl Core {
    /// spaces_root の下のフォルダがスペース。saved があれば、前の状態に戻す(読めないタブは捨てる)
    pub fn new(home: impl Into<String>, spaces_root: impl Into<PathBuf>, saved: Option<Saved>) -> Core {
        let mut c = Core {
            home: home.into(),
            spaces_root: spaces_root.into(),
            space: SPACE_DEFAULT.into(),
            tabs: Tabs::default(),
            others: BTreeMap::new(),
            last_id: 0,
            places: HashMap::new(),
            recipes: vec![],
            stopped: HashSet::new(),
            titles: HashMap::new(),
            installed: crate::apps::is_installed,
            tag: String::new(),
        };
        let names = c.space_names();
        if let Some(s) = saved {
            for (name, st) in &s.spaces {
                let tabs = c.load_tabs(st);
                c.others.insert(name.clone(), tabs);
            }
            c.space = s.space.clone();
        }
        // 前のスペースが無くなっていたら(フォルダを消した・名前を変えた)、既定か最初のスペースにする
        if !names.contains(&c.space) {
            c.space = if names.iter().any(|n| n == SPACE_DEFAULT) {
                SPACE_DEFAULT.into()
            } else {
                names.first().cloned().unwrap_or(SPACE_DEFAULT.into())
            };
        }
        c.tabs = c.others.remove(&c.space).unwrap_or_default();
        c
    }

    pub fn space_dir(&self) -> PathBuf {
        self.spaces_root.join(&self.space)
    }

    fn next_id(&mut self) -> u64 {
        self.last_id += 1;
        self.last_id
    }

    // -------------------------------------------------------------- 書き出す・戻す

    pub fn saved(&self) -> Saved {
        let mut spaces = BTreeMap::new();
        for (name, t) in self.others.iter().chain([(&self.space, &self.tabs)]) {
            spaces.insert(name.clone(), self.save_tabs(t));
        }
        Saved { space: self.space.clone(), spaces }
    }

    fn save_tabs(&self, t: &Tabs) -> SavedTabs {
        let tabs = t
            .tabs
            .iter()
            .map(|tab| SavedTab {
                num: tab.num,
                bookmark: tab.bookmark.as_ref().map(|(p, _)| p.clone()),
                title: tab.bookmark.as_ref().map(|(_, t)| t.clone()).unwrap_or_default(),
                active_pane: tab.layout.panes().iter().position(|(id, _)| *id == tab.active_pane).unwrap_or(0),
                layout: serde_json::to_value(tab.layout.to_node().to_value(&self.home)).unwrap_or(Json::Null),
            })
            .collect();
        SavedTabs { active: t.active.and_then(|a| t.tabs.iter().position(|x| x.id == a)).unwrap_or(0), tabs }
    }

    fn load_tabs(&mut self, s: &SavedTabs) -> Tabs {
        let mut out = Tabs::default();
        for st in &s.tabs {
            let Some(node) = json_to_toml(&st.layout).and_then(|v| Node::from_value(&v, &self.home).ok()) else { continue };
            let id = self.next_id();
            let layout = Layout::from_node(node, &mut self.last_id);
            let panes = layout.panes();
            let active_pane = panes.get(st.active_pane).unwrap_or(&panes[0]).0;
            // 番号が重なっていたら(手で書き換えたなど)、空いた番号にする
            let num = if st.num > 0 && !out.tabs.iter().any(|t: &Tab| t.num == st.num) { st.num } else { free_num(&out.tabs) };
            let bookmark = st.bookmark.clone().map(|p| (p, st.title.clone()));
            out.tabs.push(Tab { id, num, layout, active_pane, bookmark });
        }
        out.active = out.tabs.get(s.active).or(out.tabs.first()).map(|t| t.id);
        out
    }

    // -------------------------------------------------------------- 状態

    pub fn state(&self) -> State {
        State {
            space: self.space.clone(),
            space_dir: self.space_dir().to_string_lossy().into_owned(),
            active: self.tabs.active,
            tabs: self
                .tabs
                .tabs
                .iter()
                .map(|t| TabInfo {
                    id: t.id,
                    num: t.num,
                    title: t.title(),
                    bookmark: t.bookmark.as_ref().map(|(p, _)| p.clone()),
                    active_pane: t.active_pane,
                    layout: self.live(t.layout.to_json()),
                })
                .collect(),
        }
    }

    /// 配置の JSON に、取り込んだ窓の今の様子(題名・止まっているか)を付ける
    fn live(&self, mut v: Json) -> Json {
        if let Some(o) = v.as_object_mut() {
            if o.get("k").and_then(Json::as_str) == Some("split") {
                for k in ["a", "b"] {
                    if let Some(c) = o.remove(k) {
                        o.insert(k.into(), self.live(c));
                    }
                }
            } else if let Some(id) = o.get("id").and_then(Json::as_u64) {
                if let Some(t) = self.titles.get(&id) {
                    o.insert("window_title".into(), json!(t));
                }
                if self.stopped.contains(&id) {
                    o.insert("stopped".into(), json!(true));
                }
            }
        }
        v
    }

    pub fn set_recipes(&mut self, r: Vec<Recipe>) {
        self.recipes = r;
    }

    /// 起動コマンドを確かめる関数を差し替える(テスト用)
    pub fn set_installed(&mut self, f: fn(&str) -> bool) {
        self.installed = f;
    }

    pub fn set_tag(&mut self, tag: &str) {
        self.tag = tag.to_string();
    }

    /// Web のペインの URL。Electron 版の「実ブラウザのペイン」(アプリのペインの webUrl)も Web のペインとして開く
    fn web_url(l: &Leaf) -> Option<&str> {
        match l.kind.as_str() {
            "web" => Some(l.str("url")),
            "app" if !l.str("webUrl").is_empty() => Some(l.str("webUrl")),
            _ => None,
        }
    }

    /// Web のペイン(今のスペースのもの。止まったものは除く)と、ダウンロードの保存先(今のタブの paths の download)
    pub fn web_wants(&self) -> (Vec<WebWant>, String) {
        let mut out = vec![];
        for t in &self.tabs.tabs {
            for (id, l) in t.layout.panes() {
                if let Some(url) = Self::web_url(l).filter(|_| !self.stopped.contains(&id)) {
                    out.push(WebWant { pane: id, url: url.to_string() });
                }
            }
        }
        let active = self.tab_index(None).ok().map(|i| &self.tabs.tabs[i]);
        let dl = self.paths(active).get("download").and_then(Json::as_str).unwrap_or("").to_string();
        (out, dl)
    }

    /// Web のペインの今の URL と題名(Chromium から)。URL は残す(起動し直すとその URL で開く)。変われば (状態が変わった, 残すものが変わった)
    pub fn note_web(&mut self, pane: u64, url: String, title: String) -> (bool, bool) {
        let Ok((i, _)) = self.find_pane(pane) else { return (false, false) };
        let mut saved = false;
        if let Some(l) = self.tabs.tabs[i].layout.leaf_mut(pane) {
            let key = if l.kind == "app" { "webUrl" } else { "url" };
            if !url.is_empty() && l.str(key) != url {
                l.fields.insert(key.into(), url.into());
                saved = true;
            }
        }
        let titled = self.note_title(pane, title);
        (saved || titled, saved)
    }

    /// 窓を取り込むペイン(今のスペースのもの)。アプリのペインは起動コマンド、レシピのペインはレシピのコマンド、
    /// Web のペインは窓の題名の印(窓は web.rs が作る)。止まったペイン・コマンドの無いペインは入れない
    pub fn wants(&self) -> Vec<Want> {
        let mut out = vec![];
        for t in &self.tabs.tabs {
            let mut paths: Option<Map<String, Json>> = None;
            for (id, l) in t.layout.panes() {
                if self.stopped.contains(&id) {
                    continue;
                }
                if Self::web_url(l).is_some() {
                    let marker = crate::web::marker(&self.tag, id);
                    out.push(Want {
                        pane: id,
                        cmd: String::new(),
                        cls: String::new(),
                        cwd: String::new(),
                        rect: self.places.get(&id).copied(),
                        marker,
                    });
                    continue;
                }
                let (cmd, cls) = if l.kind == "app" {
                    (l.str("cmd").to_string(), l.str("cls").to_string())
                } else if let Some(r) = self.recipes.iter().find(|r| r.kind == l.kind) {
                    let p = paths.get_or_insert_with(|| self.paths(Some(t)));
                    let get = |k: &str| p.get(k).and_then(Json::as_str).unwrap_or(&self.home).to_string();
                    (recipes::command(r, &get), r.class.clone())
                } else {
                    continue;
                };
                if cmd.trim().is_empty() {
                    continue;
                }
                let p = paths.get_or_insert_with(|| self.paths(Some(t)));
                let cwd = p.get("terminal").and_then(Json::as_str).unwrap_or(&self.home).to_string();
                out.push(Want { pane: id, cmd, cls, cwd, rect: self.places.get(&id).copied(), marker: String::new() });
            }
        }
        out
    }

    /// 取り込んだ窓が無くなった。ペインは残して止まったものにする。変われば true
    pub fn note_exited(&mut self, pane: u64) -> bool {
        self.titles.remove(&pane);
        self.find_pane(pane).is_ok() && self.stopped.insert(pane)
    }

    /// 取り込んだ窓の題名。変われば true
    pub fn note_title(&mut self, pane: u64, title: String) -> bool {
        if self.find_pane(pane).is_err() || self.titles.get(&pane) == Some(&title) {
            return false;
        }
        self.titles.insert(pane, title);
        true
    }

    /// UI が知らせたペインの場所(見えているペインだけ)
    pub fn place_of(&self, pane: u64) -> Option<PaneRect> {
        self.places.get(&pane).copied()
    }

    // -------------------------------------------------------------- コマンド

    /// コマンドを 1 つ処理する(`hello` はソケットの側で受ける)
    pub fn handle(&mut self, cmd: Command) -> Outcome {
        let before = self.saved();
        let reply = self.run(cmd);
        let changed = reply.is_ok() && self.saved() != before;
        Outcome { reply, changed }
    }

    fn run(&mut self, cmd: Command) -> Reply {
        match cmd {
            Command::Hello { .. } => Err(fail(ErrorCode::Protocol, "hello is only accepted first")),
            Command::StateGet {} => Ok(json!(self.state())),
            Command::SpaceList {} => Ok(json!(self.space_names())),
            Command::SpaceSwitch { name } => self.space_switch(&name),
            Command::SpaceCreate { name } => self.space_create(&name),
            Command::BookmarkList {} => Ok(json!(self.bookmarks())),
            Command::BookmarkOpen { path, allow } => self.bookmark_open(&path, &allow),
            Command::RecipeList {} => {
                Ok(json!(self.recipes.iter().map(|r| RecipeInfo { kind: r.kind.clone(), name: r.name.clone() }).collect::<Vec<_>>()))
            }
            Command::TabOpen { pane, index } => {
                let leaf = leaf_of(&pane)?;
                let id = self.insert_tab(Layout::Pane { id: 0, leaf }, None, index);
                Ok(json!(id))
            }
            Command::TabClose { tab } => {
                let i = self.tab_index(tab)?;
                self.close_tab(i);
                Ok(Json::Null)
            }
            Command::TabActivate { tab } => {
                let i = self.tab_index(Some(tab))?;
                self.tabs.active = Some(self.tabs.tabs[i].id);
                // 止まっていたペインは起動し直す
                for (id, _) in self.tabs.tabs[i].layout.panes() {
                    self.stopped.remove(&id);
                }
                Ok(Json::Null)
            }
            Command::TabMove { tab, index } => {
                let i = self.tab_index(tab)?;
                let t = self.tabs.tabs.remove(i);
                let at = index.min(self.tabs.tabs.len());
                self.tabs.tabs.insert(at, t);
                Ok(Json::Null)
            }
            Command::PaneSplit { pane, dir, new } => self.pane_split(pane, dir, &new),
            Command::PaneClose { pane } => self.pane_close(pane),
            Command::PaneUpdate { pane, fields } => {
                let (i, _) = self.find_pane(pane)?;
                let leaf = self.tabs.tabs[i].layout.leaf_mut(pane).ok_or_else(|| not_found("pane"))?;
                for (k, v) in fields {
                    if matches!(k.as_str(), "k" | "id" | "type") {
                        continue;
                    }
                    match json_to_toml(&v) {
                        Some(v) => leaf.fields.insert(k, v),
                        None => leaf.fields.remove(&k), // null は消す
                    };
                }
                Ok(Json::Null)
            }
            Command::PaneFocus { pane } => {
                let (i, _) = self.find_pane(pane)?;
                self.tabs.tabs[i].active_pane = pane;
                self.tabs.active = Some(self.tabs.tabs[i].id);
                Ok(Json::Null)
            }
            Command::SplitRatio { pane, ratio } => {
                let (i, _) = self.find_pane(pane)?;
                if !ratio.is_finite() {
                    return Err(fail(ErrorCode::BadRequest, "Ratio is not a number"));
                }
                let r = self.tabs.tabs[i].layout.ratio_mut(pane).ok_or_else(|| fail(ErrorCode::NotFound, "Pane is not in a split"))?;
                *r = ratio.clamp(0.05, 0.95);
                Ok(Json::Null)
            }
            Command::PanePlace { rects } => {
                let live: Vec<u64> = self.tabs.tabs.iter().flat_map(|t| t.layout.panes().into_iter().map(|(id, _)| id)).collect();
                self.places = rects.into_iter().filter(|r| live.contains(&r.pane)).map(|r| (r.pane, r)).collect();
                Ok(Json::Null)
            }
            Command::WebGo { pane, .. } => {
                // 動かすのはソケットの側(web.rs)。ここでは Web のペインかだけ確かめる
                let (_, l) = self.find_pane(pane)?;
                if Self::web_url(l).is_none() {
                    return Err(fail(ErrorCode::BadRequest, "Not a web pane"));
                }
                Ok(Json::Null)
            }
            Command::PathsGet { tab } => {
                let i = self.tab_index(tab).ok();
                Ok(json!(self.paths(i.map(|i| &self.tabs.tabs[i]))))
            }
        }
    }

    // -------------------------------------------------------------- タブ

    fn tab_index(&self, r: Option<TabRef>) -> Result<usize, Fail> {
        let found = match r {
            None => self.tabs.active.and_then(|a| self.tabs.tabs.iter().position(|t| t.id == a)),
            Some(TabRef::Id { id }) => self.tabs.tabs.iter().position(|t| t.id == id),
            Some(TabRef::Num { num }) => self.tabs.tabs.iter().position(|t| t.num == num),
        };
        found.ok_or_else(|| not_found("tab"))
    }

    /// layout のペインに id を振り直して、新しいタブにする(今のタブにする)
    fn insert_tab(&mut self, layout: Layout, bookmark: Option<(String, String)>, index: Option<usize>) -> u64 {
        let id = self.next_id();
        let layout = Layout::from_node(layout.to_node(), &mut self.last_id);
        let active_pane = layout.first();
        let tab = Tab { id, num: free_num(&self.tabs.tabs), layout, active_pane, bookmark };
        let at = index.unwrap_or(0).min(self.tabs.tabs.len());
        self.tabs.tabs.insert(at, tab);
        self.tabs.active = Some(id);
        id
    }

    /// i のタブを閉じる。今のタブなら、同じ位置(無ければ 1 つ上)のタブを今のタブにする
    fn close_tab(&mut self, i: usize) {
        let t = self.tabs.tabs.remove(i);
        for (id, _) in t.layout.panes() {
            self.places.remove(&id);
            self.stopped.remove(&id);
            self.titles.remove(&id);
        }
        if self.tabs.active == Some(t.id) {
            let n = self.tabs.tabs.len();
            self.tabs.active = (n > 0).then(|| self.tabs.tabs[i.min(n - 1)].id);
        }
    }

    fn find_pane(&self, pane: u64) -> Result<(usize, &Leaf), Fail> {
        for (i, t) in self.tabs.tabs.iter().enumerate() {
            if let Some((_, l)) = t.layout.panes().into_iter().find(|(id, _)| *id == pane) {
                return Ok((i, l));
            }
        }
        Err(not_found("pane"))
    }

    fn pane_split(&mut self, pane: u64, dir: Dir, new: &PaneSpec) -> Reply {
        let (i, _) = self.find_pane(pane)?;
        let leaf = leaf_of(new)?;
        let nid = self.next_id();
        let fdir = if dir == Dir::Col { FDir::Col } else { FDir::Row };
        let mut leaf = Some(leaf);
        self.tabs.tabs[i].layout.replace(pane, &mut |old| Layout::Split {
            dir: fdir,
            ratio: 0.5,
            a: Box::new(old),
            b: Box::new(Layout::Pane { id: nid, leaf: leaf.take().unwrap_or_else(empty_leaf) }),
        });
        self.tabs.tabs[i].active_pane = nid;
        Ok(json!(nid))
    }

    fn pane_close(&mut self, pane: u64) -> Reply {
        let (i, _) = self.find_pane(pane)?;
        self.places.remove(&pane);
        let tab = &mut self.tabs.tabs[i];
        if matches!(tab.layout, Layout::Pane { .. }) {
            self.close_tab(i);
            return Ok(Json::Null);
        }
        let rest = tab.layout.remove(pane).ok_or_else(|| not_found("pane"))?;
        if tab.active_pane == pane {
            tab.active_pane = rest;
        }
        Ok(Json::Null)
    }

    // -------------------------------------------------------------- スペース

    /// スペースの名前(スペースのフォルダの下の、隠しでないフォルダ)。1 つも無ければ既定のスペースを作る
    pub fn space_names(&self) -> Vec<String> {
        let read = || -> Vec<String> {
            let Ok(rd) = std::fs::read_dir(&self.spaces_root) else { return vec![] };
            let mut v: Vec<String> = rd
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| !n.starts_with('.'))
                .collect();
            v.sort();
            v
        };
        let names = read();
        if names.is_empty() && std::fs::create_dir_all(self.spaces_root.join(SPACE_DEFAULT)).is_ok() {
            return vec![SPACE_DEFAULT.into()];
        }
        names
    }

    fn space_switch(&mut self, name: &str) -> Reply {
        if name == self.space {
            return Ok(Json::Null);
        }
        if !self.space_names().iter().any(|n| n == name) {
            return Err(not_found("space"));
        }
        let cur = std::mem::take(&mut self.tabs);
        self.others.insert(std::mem::replace(&mut self.space, name.to_string()), cur);
        self.tabs = self.others.remove(name).unwrap_or_default();
        self.places.clear(); // 前のスペースのペインはもう見えない
        Ok(Json::Null)
    }

    fn space_create(&mut self, name: &str) -> Reply {
        let name = valid_name(name)?;
        let dir = self.spaces_root.join(name);
        if dir.exists() {
            return Err(fail(ErrorCode::Conflict, "A space with this name already exists"));
        }
        std::fs::create_dir_all(&dir).map_err(|e| fail(ErrorCode::Io, e.to_string()))?;
        Ok(json!(name))
    }

    /// スペースの設定(`space.toml`)。無い・読めなければ空
    fn space_conf(&self) -> Space {
        std::fs::read_to_string(self.space_dir().join(Space::FILE)).ok().and_then(|t| Space::parse(&t).ok()).unwrap_or_default()
    }

    // -------------------------------------------------------------- ブックマーク

    /// 今のスペースのブックマーク。直下の `.prelude` と、サブフォルダ(1 段だけ)の中の `.prelude`。名前順
    pub fn bookmarks(&self) -> Vec<BookmarkInfo> {
        let dir = self.space_dir();
        let mut out = Vec::new();
        let read = |d: &Path, folder: &str, out: &mut Vec<BookmarkInfo>| -> Vec<String> {
            let mut subs = Vec::new();
            let Ok(rd) = std::fs::read_dir(d) else { return subs };
            for e in rd.flatten() {
                let Ok(name) = e.file_name().into_string() else { continue };
                if name.starts_with('.') {
                    continue;
                }
                if e.file_type().is_ok_and(|t| t.is_dir()) {
                    subs.push(name);
                } else if let Some(title) = name.strip_suffix(".prelude") {
                    if out.len() < BM_LIST_MAX {
                        out.push(BookmarkInfo {
                            path: e.path().to_string_lossy().into_owned(),
                            title: title.into(),
                            folder: folder.into(),
                        });
                    }
                }
            }
            subs
        };
        for sub in read(&dir, "", &mut out) {
            read(&dir.join(&sub), &sub, &mut out);
        }
        out.sort_by(|a, b| (&a.folder, &a.title).cmp(&(&b.folder, &b.title)));
        out
    }

    fn read_bookmark(&self, path: &str) -> Result<Bookmark, Fail> {
        let p = Path::new(path);
        if !p.is_absolute() || p.extension().and_then(|e| e.to_str()) != Some(prelude_format::EXT) {
            return Err(fail(ErrorCode::BadRequest, "Not a bookmark file"));
        }
        let meta = std::fs::metadata(p).map_err(|e| fail(ErrorCode::NotFound, e.to_string()))?;
        if meta.len() > BM_MAX {
            return Err(fail(ErrorCode::BadRequest, "File is too large"));
        }
        let text = std::fs::read_to_string(p).map_err(|e| fail(ErrorCode::Io, e.to_string()))?;
        Bookmark::parse(&text, &self.home).map_err(|e| fail(ErrorCode::BadRequest, e.to_string()))
    }

    /// ブックマークを開く。もう開いていればそのタブにする。パスの無いフォルダのペインは paths の files で開く。
    /// ファイルは人からもらうこともあるので、インストール済みのアプリと一致しない起動コマンドは、allow に無ければ外す(そのペインは起動しない)
    fn bookmark_open(&mut self, path: &str, allow: &[String]) -> Reply {
        if let Some(t) = self.tabs.tabs.iter().find(|t| t.bookmark.as_ref().is_some_and(|(p, _)| p == path)) {
            self.tabs.active = Some(t.id);
            return Ok(json!(BookmarkOpened { tab: t.id, held: vec![] }));
        }
        let bm = self.read_bookmark(path)?;
        let dir = parent(path);
        let space = self.space_conf();
        let sdir = self.space_dir().to_string_lossy().into_owned();
        let files = Resolver { home: &self.home, bookmark: Some((&bm.paths, &dir)), space: Some((&space.paths, &sdir)) }.get("files");
        let mut layout = Layout::from_node(bm.layout, &mut 0);
        if let Some(files) = files {
            for (id, l) in layout.panes().into_iter().map(|(id, l)| (id, l.clone())).collect::<Vec<_>>() {
                if matches!(l.kind.as_str(), "files" | "field") && l.str("path").is_empty() {
                    if let Some(l) = layout.leaf_mut(id) {
                        l.fields.insert("path".into(), files.clone().into());
                    }
                }
            }
        }
        let mut held = vec![];
        for (id, l) in layout.panes().into_iter().map(|(id, l)| (id, l.clone())).collect::<Vec<_>>() {
            let cmd = l.str("cmd").trim().to_string();
            if l.kind == "app"
                && l.str("webUrl").is_empty()
                && !cmd.is_empty()
                && !(self.installed)(&cmd)
                && !allow.iter().any(|a| a.trim() == cmd)
            {
                if let Some(l) = layout.leaf_mut(id) {
                    l.fields.remove("cmd");
                }
                if !held.contains(&cmd) {
                    held.push(cmd);
                }
            }
        }
        let title = if bm.title.is_empty() { stem(path) } else { bm.title };
        let tab = self.insert_tab(layout, Some((path.to_string(), title)), None);
        Ok(json!(BookmarkOpened { tab, held }))
    }

    /// タブで使うフォルダ。スペースの既定(`space.toml`)の上に、タブが開いたブックマークの paths を重ねる。
    /// どこにも無い項目はホーム
    fn paths(&self, tab: Option<&Tab>) -> Map<String, Json> {
        let space = self.space_conf();
        let sdir = self.space_dir().to_string_lossy().into_owned();
        let bm = tab.and_then(|t| t.bookmark.as_ref()).and_then(|(p, _)| Some((self.read_bookmark(p).ok()?.paths, parent(p))));
        let r = Resolver { home: &self.home, bookmark: bm.as_ref().map(|(p, d)| (p, d.as_str())), space: Some((&space.paths, &sdir)) };
        PATH_KEYS.iter().map(|k| (k.to_string(), json!(r.resolve(k)))).collect()
    }
}

/// 使われていない一番小さい番号
fn free_num(tabs: &[Tab]) -> u32 {
    (1..).find(|n| !tabs.iter().any(|t| t.num == *n)).unwrap_or(1)
}

fn parent(p: &str) -> String {
    Path::new(p).parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default()
}

fn stem(p: &str) -> String {
    Path::new(p).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// フォルダの名前に使えるか。使えれば前後の空白を除いたもの
fn valid_name(name: &str) -> Result<&str, Fail> {
    let n = name.trim();
    if n.is_empty() || n.starts_with('.') || n.contains('/') || n.contains('\0') || n.len() > 255 {
        return Err(fail(ErrorCode::BadRequest, "Invalid name"));
    }
    Ok(n)
}

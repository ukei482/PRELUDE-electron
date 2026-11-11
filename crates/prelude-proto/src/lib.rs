//! コアと UI・`prelude` コマンド・プラグインの間の約束(docs/redesign-rust.md の「コアと UI の境目」「ソケットの約束」)。
//!
//! - Unix ソケット([`socket_path`])の上に、1 行 1 つの JSON を流す
//! - 送る側: [`Request`] `{"id": 1, "cmd": "tab.open", "args": {...}}`。返事は同じ `id` の [`Response`]
//! - コアからは、頼まれなくても [`Event`] `{"event": "state", "data": {...}}` が来る(`hello` で `subscribe` したときだけ)
//! - つないだら最初に `hello` を送り、約束の版([`PROTOCOL`])を確かめる
//!
//! 版の決まり: コマンド・項目・イベントを **足す** だけなら版は上げない(受け取る側は知らない項目・イベントを無視する)。
//! 意味を変える・消すときは版を上げ、コアは古い版([`SUPPORTED`])も話し続ける。
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::PathBuf;

pub mod client;
pub mod sidebar;
pub use client::Client;

/// 今の約束の版
pub const PROTOCOL: u32 = 1;
/// コアが話せる版
pub const SUPPORTED: &[u32] = &[1];
/// 1 行の長さの上限(これより長い行は読まずに切る)
pub const MAX_LINE: usize = 1 << 20;

/// コアのソケットの場所。`PRELUDE_CORE_SOCK` → [`default_socket_path`]
pub fn socket_path() -> PathBuf {
    match std::env::var_os("PRELUDE_CORE_SOCK").filter(|p| !p.is_empty()) {
        Some(p) => p.into(),
        None => default_socket_path(),
    }
}

/// ふだんのソケットの場所。`$XDG_RUNTIME_DIR/prelude/core.sock` → `/tmp/prelude-$USER/core.sock`
pub fn default_socket_path() -> PathBuf {
    let dir = match std::env::var_os("XDG_RUNTIME_DIR").filter(|p| !p.is_empty()) {
        Some(d) => PathBuf::from(d).join("prelude"),
        None => PathBuf::from(format!("/tmp/prelude-{}", std::env::var("USER").unwrap_or_else(|_| "user".into()))),
    };
    dir.join("core.sock")
}

// ------------------------------------------------------------------ 送るもの

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    #[serde(flatten)]
    pub command: Command,
}

impl Request {
    /// 受け取った JSON → Request。`args` は省いてよい(`{}` と同じ)
    pub fn from_value(mut v: Value) -> Result<Request, serde_json::Error> {
        if let Some(o) = v.as_object_mut() {
            if o.get("args").is_none_or(Value::is_null) {
                o.insert("args".into(), Value::Object(Map::new()));
            }
        }
        serde_json::from_value(v)
    }
}

/// コマンド。`cmd` が名前、`args` が引数。`tab` を省くと今のタブ
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args")]
pub enum Command {
    /// 最初に送る。`subscribe` なら、状態が変わるたびに `state` のイベントを受け取る → [`Hello`]
    #[serde(rename = "hello")]
    Hello {
        protocol: u32,
        #[serde(default)]
        name: String,
        #[serde(default)]
        subscribe: bool,
    },
    /// 今の状態 → [`State`]
    #[serde(rename = "state.get")]
    StateGet {},

    /// スペースの名前の一覧 → `[String]`
    #[serde(rename = "space.list")]
    SpaceList {},
    /// スペースを切り替える。今のタブは閉じ、切り替え先で前に開いていたタブを開き直す
    #[serde(rename = "space.switch")]
    SpaceSwitch { name: String },
    /// スペース(フォルダ)を作る。切り替えはしない
    #[serde(rename = "space.create")]
    SpaceCreate { name: String },

    /// 今のスペースのブックマーク → `[`[`BookmarkInfo`]`]`
    #[serde(rename = "bookmark.list")]
    BookmarkList {},
    /// ブックマーク(`.prelude` のパス)を開く。もう開いていればそのタブにする(1 ブックマーク = 最大 1 タブ) → [`BookmarkOpened`]。
    /// インストール済みのアプリと一致しない起動コマンドは、`allow` に無ければ外して開く(人からもらったファイルが勝手に実行しないように)
    #[serde(rename = "bookmark.open")]
    BookmarkOpen {
        path: String,
        #[serde(default)]
        allow: Vec<String>,
    },
    /// ペインのレシピ(窓を取り込むペインの種類) → `[`[`RecipeInfo`]`]`
    #[serde(rename = "recipe.list")]
    RecipeList {},

    /// ランチャーの候補(タブ・ブックマーク・アプリ・スペース) → `[`[`Item`]`]`。タブが先、残りは点数(最近 + よく使う)の高い順
    #[serde(rename = "item.list")]
    ItemList {},
    /// 候補を開く(`key` は [`Item`] の key)。開いたものは点数に足す。ブックマークは `bookmark.open` と同じ(→ [`BookmarkOpened`])
    #[serde(rename = "item.open")]
    ItemOpen {
        key: String,
        #[serde(default)]
        allow: Vec<String>,
        /// アプリを新しいタブではなく、このペインを分割して開く(ほかの種類の候補では無視する)
        #[serde(default)]
        split: Option<SplitAt>,
    },

    /// ペイン 1 つの新しいタブ → タブの id。`index` を省くと一番上
    #[serde(rename = "tab.open")]
    TabOpen {
        pane: PaneSpec,
        #[serde(default)]
        index: Option<usize>,
    },
    #[serde(rename = "tab.close")]
    TabClose {
        #[serde(default)]
        tab: Option<TabRef>,
    },
    #[serde(rename = "tab.activate")]
    TabActivate { tab: TabRef },
    #[serde(rename = "tab.move")]
    TabMove {
        #[serde(default)]
        tab: Option<TabRef>,
        index: usize,
    },

    /// ペインを分けて、新しいペインを右(`row`)か下(`col`)に置く → 新しいペインの id
    #[serde(rename = "pane.split")]
    PaneSplit { pane: u64, dir: Dir, new: PaneSpec },
    /// ペインを閉じる。タブの最後のペインならタブを閉じる
    #[serde(rename = "pane.close")]
    PaneClose { pane: u64 },
    /// ペインの中身を変える(`url` など。`type` は変えない)。値が null の項目は消す
    #[serde(rename = "pane.update")]
    PaneUpdate { pane: u64, fields: Map<String, Value> },
    #[serde(rename = "pane.focus")]
    PaneFocus { pane: u64 },
    /// ペインを含む一番近い分割の比率(0〜1。a の側の割合)
    #[serde(rename = "split.ratio")]
    SplitRatio { pane: u64, ratio: f64 },
    /// UI が決めたペインの場所を知らせる(KWin のつなぎで使う。「窓のつなぎ」)。見えているペインを全部送る。送られなかったペインは隠れている
    #[serde(rename = "pane.place")]
    PanePlace { rects: Vec<PaneRect> },

    /// Web のペインで戻る・進む・読み直す。URL を開くのは `pane.update` で `url` を変える
    #[serde(rename = "web.go")]
    WebGo { pane: u64, action: WebAction },

    /// タブで使うフォルダ(`.prelude`・`space.toml` の paths) → `{ files, open, save, download, terminal }`
    #[serde(rename = "paths.get")]
    PathsGet {
        #[serde(default)]
        tab: Option<TabRef>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WebAction {
    Back,
    Forward,
    Reload,
}

/// タブの指し方。`{"id": 7}` か、画面に出ている番号 `{"num": 3}`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TabRef {
    Id { id: u64 },
    Num { num: u32 },
}

/// 作るペイン。`{"type": "web", "url": "https://…"}` のように、`.prelude` の `[layout]` のペインと同じ形
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneSpec {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(flatten)]
    pub fields: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    /// 左右に並べる
    Row,
    /// 上下に並べる
    Col,
}

/// ペインの場所(画面の座標。論理ピクセル)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PaneRect {
    pub pane: u64,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

// ------------------------------------------------------------------ 返ってくるもの

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

impl Response {
    pub fn ok(id: u64, result: impl Serialize) -> Response {
        Response { id, result: Some(serde_json::to_value(result).unwrap_or(Value::Null)), error: None }
    }
    pub fn err(id: u64, code: ErrorCode, message: impl Into<String>) -> Response {
        Response { id, result: None, error: Some(ErrorBody { code, message: message.into() }) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// JSON として読めない・引数の形が違う
    BadRequest,
    /// 知らないコマンド(新しい版のコマンドかもしれない)
    UnknownCommand,
    /// 約束の版が合わない・`hello` の前にほかのコマンドを送った
    Protocol,
    /// タブ・ペイン・スペース・ブックマークが無い
    NotFound,
    /// 今はできない(同じ名前のスペースがある、など)
    Conflict,
    /// ファイルの読み書きに失敗した
    Io,
}

/// コアから来るイベント。知らないイベントは無視する。
/// - `state`: data は [`State`]。状態が変わるたび
/// - `download`: data は CDP の `Browser.downloadWillBegin`(`guid`・`url`・`suggestedFilename`)か `Browser.downloadProgress`(`guid`・`state`・`receivedBytes`・`totalBytes`)の params
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub event: String,
    pub data: Value,
}

/// 受け取った 1 行(返事かイベント)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Incoming {
    Event(Event),
    Response(Response),
}

/// `hello` の返事
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    /// コアの版
    pub core: String,
}

/// 状態(`state.get` の返事と `state` のイベント)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub space: String,
    /// 今のスペースのフォルダ
    pub space_dir: String,
    /// 今のタブの id
    pub active: Option<u64>,
    /// 上から順
    pub tabs: Vec<TabInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabInfo {
    pub id: u64,
    /// 画面に出す番号(スペースの中で 1 から。空いた番号から使う)
    pub num: u32,
    pub title: String,
    /// 開いたブックマーク(`.prelude` のパス)
    pub bookmark: Option<String>,
    pub active_pane: u64,
    /// 配置。`.prelude` の `[layout]` と同じ形で、ペインに `id` が付く。
    /// 窓を取り込むペインには、今の様子も付く: `window_title`(本体の窓の題名)、`stopped`(窓が閉じて止まっている。タブを開き直すと起動し直す)
    pub layout: Value,
}

/// `bookmark.open` の返事
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BookmarkOpened {
    pub tab: u64,
    /// 外した起動コマンド(インストール済みのアプリと一致しないもの)。そのペインは起動しない
    pub held: Vec<String>,
}

/// 配置(`TabInfo::layout`)の中の、ペインの文字の項目
pub fn pane_str<'a>(layout: &'a serde_json::Value, pane: u64, key: &str) -> Option<&'a str> {
    if layout["k"] == "split" {
        return pane_str(&layout["a"], pane, key).or_else(|| pane_str(&layout["b"], pane, key));
    }
    (layout["id"].as_u64() == Some(pane)).then(|| layout[key].as_str()).flatten().filter(|s| !s.is_empty())
}

/// 配置の中のペイン 1 つ(サイドバーで、分割したタブのペインを並べて出すため)
#[derive(Debug, Clone, PartialEq)]
pub struct PaneView {
    pub id: u64,
    /// ペインの種類(`web`・`files`・`app` など)
    pub kind: String,
    /// 出す名前: 窓の題名 → URL のホスト・フォルダの名前・アプリの名前 → 種類
    pub title: String,
    /// 窓が閉じて止まっている
    pub stopped: bool,
}

/// 配置(`TabInfo::layout`)のペインを、左上から順に(分割の a が先)
pub fn panes(layout: &serde_json::Value) -> Vec<PaneView> {
    fn go(l: &serde_json::Value, out: &mut Vec<PaneView>) {
        if l["k"] == "split" {
            go(&l["a"], out);
            go(&l["b"], out);
            return;
        }
        let Some(id) = l["id"].as_u64() else { return };
        let s = |k: &str| l[k].as_str().unwrap_or("").to_string();
        let kind = s("type");
        let base = |p: &str| p.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
        let place = match kind.as_str() {
            "web" => {
                let u = s("url");
                u.split("://").nth(1).unwrap_or(&u).split('/').next().unwrap_or("").to_string()
            }
            "files" | "field" => base(&s("path")),
            "app" => s("name"),
            _ => String::new(),
        };
        let title = [s("window_title"), s("title"), place].into_iter().find(|t| !t.is_empty()).unwrap_or_else(|| kind.clone());
        out.push(PaneView { id, kind, title, stopped: l["stopped"] == true });
    }
    let mut out = vec![];
    go(layout, &mut out);
    out
}

/// 分割して開く場所(`item.open`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SplitAt {
    pub pane: u64,
    pub dir: Dir,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecipeInfo {
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
}

/// ランチャーの候補
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// 開くときに `item.open` へ渡すもの。`tab:<id>`・`bookmark:<パス>`・`app:<.desktop の id>`・`space:<名前>`
    pub key: String,
    /// `tab`・`bookmark`・`app`・`space`
    pub kind: String,
    pub label: String,
    /// 補い(タブの番号・ブックマークのフォルダなど。無ければ空)
    #[serde(default)]
    pub detail: String,
    /// 点数(最近 + よく使う)。タブは 0
    #[serde(default)]
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BookmarkInfo {
    pub path: String,
    pub title: String,
    /// ブックマークのフォルダ(スペースの直下なら空)
    pub folder: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn 配置のペインを並べる() {
        let l = json!({ "k": "split", "dir": "row", "ratio": 0.5,
            "a": { "k": "leaf", "id": 1, "type": "web", "url": "https://example.com/a" },
            "b": { "k": "split", "dir": "col", "ratio": 0.5,
                "a": { "k": "leaf", "id": 2, "type": "app", "name": "kate", "window_title": "x.txt - Kate" },
                "b": { "k": "leaf", "id": 3, "type": "files", "path": "/home/me/doc/", "stopped": true } } });
        let v = panes(&l);
        let t: Vec<(u64, &str, &str, bool)> = v.iter().map(|p| (p.id, p.kind.as_str(), p.title.as_str(), p.stopped)).collect();
        assert_eq!(t, [(1, "web", "example.com", false), (2, "app", "x.txt - Kate", false), (3, "files", "doc", true)]);
        assert_eq!(panes(&json!({ "k": "leaf", "id": 9, "type": "cli" }))[0].title, "cli");
    }

    #[test]
    fn コマンドの形() {
        let r: Request =
            serde_json::from_value(json!({"id": 3, "cmd": "tab.open", "args": {"pane": {"type": "web", "url": "https://a/"}}})).unwrap();
        let Command::TabOpen { pane, index } = &r.command else { panic!() };
        assert_eq!((pane.kind.as_str(), pane.fields["url"].as_str(), *index), ("web", Some("https://a/"), None));
        // 引数の無いコマンドは args を省ける・空にできる
        let r = Request::from_value(json!({"id": 1, "cmd": "state.get"})).unwrap();
        assert_eq!(r.command, Command::StateGet {});
        assert_eq!(Request::from_value(json!({"id": 1, "cmd": "state.get", "args": {}})).unwrap(), r);
        let r = Request::from_value(json!({"id": 2, "cmd": "tab.close"})).unwrap();
        assert_eq!(r.command, Command::TabClose { tab: None });
        // タブは id でも番号でも、省いても(今のタブ)よい
        let r: Request = serde_json::from_value(json!({"id": 2, "cmd": "tab.close", "args": {"tab": {"num": 3}}})).unwrap();
        assert_eq!(r.command, Command::TabClose { tab: Some(TabRef::Num { num: 3 }) });
        let r: Request = serde_json::from_value(json!({"id": 2, "cmd": "tab.close", "args": {}})).unwrap();
        assert_eq!(r.command, Command::TabClose { tab: None });
        // 書いて読むと同じ
        let back: Request = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn 返事とイベントを見分ける() {
        let ok: Incoming = serde_json::from_str(r#"{"id":1,"result":{"protocol":1,"core":"0.1.0"}}"#).unwrap();
        assert!(matches!(ok, Incoming::Response(Response { id: 1, result: Some(_), error: None })));
        let err: Incoming = serde_json::from_str(r#"{"id":2,"error":{"code":"not_found","message":"No such tab"}}"#).unwrap();
        let Incoming::Response(Response { error: Some(e), .. }) = err else { panic!() };
        assert_eq!(e.code, ErrorCode::NotFound);
        let ev: Incoming = serde_json::from_str(r#"{"event":"state","data":{},"extra":1}"#).unwrap();
        assert!(matches!(ev, Incoming::Event(Event { .. })));
    }
}

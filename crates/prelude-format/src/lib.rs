//! ブックマーク(`.prelude`)とスペース(`space.toml`)の読み書き(docs/redesign-rust.md の判断 5)。
//!
//! Electron 版(`src/bmfile.js`・`bmcodec.js`)と同じ形を読み書きする。両方のテストが `test/fixtures/format/` の同じファイルを読む。
//!
//! - `.prelude` はブックマーク 1 つ。TOML で `version`・`title`・`[paths]`・`[layout]`(タブの配置)を持つ
//! - 0.11 までの JSON の `.prelude`(`{ prelude: "bookmark", version: 1, root }`)も読める。書くときは TOML にする
//! - 知らない項目(`[ui.<部品名>]` など)は読んで残し、書き戻しても消さない。知らない種類のペインも中身ごと残す
//! - ホームの下のパスは `~/…` で書き、読むときに今の PC のホームへ戻す
use std::fmt;

/// 今の `.prelude` の版。これより新しい版のファイルは読まない
pub const VERSION: i64 = 2;
/// 拡張子
pub const EXT: &str = "prelude";
/// 分割の深さと、ペインの数の上限(壊れた・悪意のあるファイルで固まらないように)
const MAX_DEPTH: usize = 8;
const MAX_LEAVES: usize = 16;
/// `[paths]` の項目の数と、文字列の長さの上限
const MAX_PATHS: usize = 64;
const MAX_STR: usize = 4096;

pub type Table = toml::Table;
pub type Value = toml::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// ブックマークのファイルではない(TOML・JSON として読めない、`layout` が無い…)
    NotBookmark,
    /// 新しい版の PRELUDE が作ったファイル
    Newer,
    /// 配置が壊れている
    Corrupted,
    /// ペインが多すぎる
    TooManyPanes,
}

impl fmt::Display for Error {
    // Electron 版と同じ文(画面に出す)
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::NotBookmark => "Not a bookmark file",
            Error::Newer => "This file was made by a newer version of PRELUDE",
            Error::Corrupted => "Layout is corrupted",
            Error::TooManyPanes => "Too many panes",
        })
    }
}

impl std::error::Error for Error {}

// ------------------------------------------------------------------ paths

/// `[paths]`。書いたままの形(`~/…`・相対パス)を、書いた順に持つ。項目は自由で、知らない項目も残す
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Paths(Vec<(String, String)>);

impl Paths {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).filter(|v| !v.is_empty())
    }
    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        let value = value.into();
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some(e) => e.1 = value,
            None => self.0.push((key.to_string(), value)),
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn from_value(v: Option<&Value>) -> Paths {
        let mut p = Paths::default();
        if let Some(Value::Table(t)) = v {
            for (k, v) in t.iter().take(MAX_PATHS) {
                if let Value::String(s) = v {
                    if !s.is_empty() {
                        p.set(k, cut(s, MAX_STR));
                    }
                }
            }
        }
        p
    }
    fn to_table(&self, home: &str) -> Table {
        self.0.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (k.clone(), Value::String(to_home(v, home)))).collect()
    }
}

/// 使うフォルダを決める。項目ごとに ブックマーク → スペース → 代わりの項目 → ホーム の順(判断 5)。
/// 代わりの項目: `download` → `save` → `files`、ほか(知らない項目も)→ `files`。
/// `get` はどこにも無ければ `None`(使う側が既定を決める。Electron 版の `bmFilePaths` と同じ)、`resolve` はホームにする。
/// 相対パスは、ブックマークのものは `.prelude` があるフォルダ、スペースのものはスペースのフォルダから数える
#[derive(Debug, Clone, Copy)]
pub struct Resolver<'a> {
    pub home: &'a str,
    /// 今開いているブックマークの paths と、その `.prelude` があるフォルダ
    pub bookmark: Option<(&'a Paths, &'a str)>,
    /// スペースの既定の paths(`space.toml`)と、スペースのフォルダ
    pub space: Option<(&'a Paths, &'a str)>,
}

impl Resolver<'_> {
    pub fn get(&self, key: &str) -> Option<String> {
        for (paths, dir) in [self.bookmark, self.space].into_iter().flatten() {
            if let Some(p) = paths.get(key) {
                return Some(abs_path(p, self.home, dir));
            }
        }
        match key {
            "files" => None,
            "download" => self.get("save"),
            _ => self.get("files"),
        }
    }

    pub fn resolve(&self, key: &str) -> String {
        self.get(key).unwrap_or_else(|| abs_path("~", self.home, self.home))
    }
}

/// パスを絶対パスにする。`~` はホーム、相対パスは dir から。`.` と `..` はたどる
pub fn abs_path(p: &str, home: &str, dir: &str) -> String {
    let p = from_home(p, home);
    let full = if p.starts_with('/') { p } else { format!("{dir}/{p}") };
    let mut out: Vec<&str> = Vec::new();
    for s in full.split('/') {
        match s {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    format!("/{}", out.join("/"))
}

fn to_home(p: &str, home: &str) -> String {
    if !home.is_empty() && (p == home || p.strip_prefix(home).is_some_and(|r| r.starts_with('/'))) {
        format!("~{}", &p[home.len()..])
    } else {
        p.to_string()
    }
}

fn from_home(p: &str, home: &str) -> String {
    if !home.is_empty() && (p == "~" || p.starts_with("~/")) {
        format!("{home}{}", &p[1..])
    } else {
        p.to_string()
    }
}

fn cut(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

// ------------------------------------------------------------------ layout

/// タブの配置。分割か、ペイン 1 つ
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Split { dir: Dir, ratio: f64, a: Box<Node>, b: Box<Node> },
    Leaf(Leaf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// 左右に並べる
    Row,
    /// 上下に並べる
    Col,
}

/// ペイン 1 つ。`kind` は種類(`web`・`files`・`app`… や、レシピ・プラグインの名前)。
/// 中身(`url`・`path`・`cmd`…)は種類ごとに違うので、書いてあるまま持つ。`path` だけはホームを戻した絶対パス
#[derive(Debug, Clone, PartialEq)]
pub struct Leaf {
    pub kind: String,
    pub fields: Table,
}

impl Leaf {
    pub fn str(&self, key: &str) -> &str {
        self.fields.get(key).and_then(Value::as_str).unwrap_or("")
    }
}

impl Node {
    pub fn leaves(&self) -> Vec<&Leaf> {
        match self {
            Node::Split { a, b, .. } => [a.leaves(), b.leaves()].concat(),
            Node::Leaf(l) => vec![l],
        }
    }

    /// 配置の中で起動されるコマンド(重なりなし)。実ブラウザのペイン(`webUrl`)は PRELUDE がコマンドを作るので含めない。
    /// 人からもらったファイルのコマンドは、開く側で確かめてから実行する
    pub fn commands(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for l in self.leaves() {
            let cmd = l.str("cmd").trim();
            if l.kind == "app" && l.str("webUrl").is_empty() && !cmd.is_empty() && !out.iter().any(|c| c == cmd) {
                out.push(cmd.to_string());
            }
        }
        out
    }

    fn parse(v: &Value, home: &str) -> Result<Node, Error> {
        let mut count = 0;
        Node::walk(v, home, 0, &mut count)
    }

    fn walk(v: &Value, home: &str, depth: usize, count: &mut usize) -> Result<Node, Error> {
        let t = v.as_table().filter(|_| depth <= MAX_DEPTH).ok_or(Error::Corrupted)?;
        if t.get("k").and_then(Value::as_str) == Some("split") {
            let dir = if t.get("dir").and_then(Value::as_str) == Some("col") { Dir::Col } else { Dir::Row };
            let ratio = match t.get("ratio") {
                Some(Value::Float(f)) if *f > 0.0 && *f < 1.0 => *f,
                _ => 0.5,
            };
            let a = Node::walk(t.get("a").ok_or(Error::Corrupted)?, home, depth + 1, count)?;
            let b = Node::walk(t.get("b").ok_or(Error::Corrupted)?, home, depth + 1, count)?;
            return Ok(Node::Split { dir, ratio, a: Box::new(a), b: Box::new(b) });
        }
        *count += 1;
        if *count > MAX_LEAVES {
            return Err(Error::TooManyPanes);
        }
        let kind = t.get("type").and_then(Value::as_str).map(|s| cut(s, 64)).unwrap_or_else(|| "files".into());
        let mut fields = Table::new();
        for (k, v) in t {
            if k == "k" || k == "type" {
                continue;
            }
            let v = match v {
                Value::String(s) if k == "path" => Value::String(from_home(&cut(s, MAX_STR), home)),
                Value::String(s) => Value::String(cut(s, MAX_STR)),
                v => v.clone(),
            };
            fields.insert(k.clone(), v);
        }
        Ok(Node::Leaf(Leaf { kind, fields }))
    }

    /// ファイルに書く形。home を渡すと `path` をホームの下なら `~/…` にする
    pub fn to_value(&self, home: &str) -> Value {
        let mut t = Table::new();
        match self {
            Node::Split { dir, ratio, a, b } => {
                t.insert("k".into(), "split".into());
                t.insert("dir".into(), if *dir == Dir::Col { "col" } else { "row" }.into());
                t.insert("ratio".into(), Value::Float(*ratio));
                t.insert("a".into(), a.to_value(home));
                t.insert("b".into(), b.to_value(home));
            }
            Node::Leaf(l) => {
                t.insert("k".into(), "leaf".into());
                t.insert("type".into(), l.kind.clone().into());
                for (k, v) in &l.fields {
                    let v = match v {
                        Value::String(s) if s.is_empty() => continue,
                        Value::String(s) if k == "path" => Value::String(to_home(s, home)),
                        v => v.clone(),
                    };
                    t.insert(k.clone(), v);
                }
            }
        }
        Value::Table(t)
    }
}

// ------------------------------------------------------------------ bookmark

/// ブックマーク 1 つ(`.prelude`)
#[derive(Debug, Clone, PartialEq)]
pub struct Bookmark {
    pub title: String,
    pub paths: Paths,
    pub layout: Node,
    /// 知らない項目(`[ui.<部品名>]` など)。書き戻すときにそのまま残す
    pub rest: Table,
}

impl Bookmark {
    pub fn new(title: impl Into<String>, layout: Node) -> Bookmark {
        Bookmark { title: title.into(), paths: Paths::default(), layout, rest: Table::new() }
    }

    /// ファイルの中身を読む。TOML のほか、0.11 までの JSON も読める
    pub fn parse(text: &str, home: &str) -> Result<Bookmark, Error> {
        if text.trim_start().starts_with('{') {
            return Bookmark::parse_json(text, home);
        }
        let mut t: Table = toml::from_str(text).map_err(|_| Error::NotBookmark)?;
        match t.remove("version") {
            None => {}
            Some(Value::Integer(v)) if v <= VERSION => {}
            Some(Value::Integer(_)) => return Err(Error::Newer),
            Some(_) => return Err(Error::NotBookmark),
        }
        let layout = t.remove("layout").ok_or(Error::NotBookmark)?;
        let layout = Node::parse(&layout, home)?;
        let title = t.remove("title").and_then(|v| v.as_str().map(|s| cut(s, 256))).unwrap_or_default();
        let paths = Paths::from_value(t.remove("paths").as_ref());
        Ok(Bookmark { title, paths, layout, rest: t })
    }

    fn parse_json(text: &str, home: &str) -> Result<Bookmark, Error> {
        let d: serde_json::Value = serde_json::from_str(text).map_err(|_| Error::NotBookmark)?;
        if d.get("prelude").and_then(|v| v.as_str()) != Some("bookmark") {
            return Err(Error::NotBookmark);
        }
        if d.get("version").and_then(|v| v.as_f64()).is_some_and(|v| v > 1.0) {
            return Err(Error::Newer);
        }
        let root = d.get("root").and_then(json_to_toml).ok_or(Error::NotBookmark)?;
        let title = d.get("title").and_then(|v| v.as_str()).map(|s| cut(s, 256)).unwrap_or_default();
        let paths = Paths::from_value(d.get("paths").and_then(json_to_toml).as_ref());
        Ok(Bookmark { title, paths, layout: Node::parse(&root, home)?, rest: Table::new() })
    }

    /// ファイルに書く文字列(TOML)
    pub fn to_toml(&self, home: &str) -> String {
        let mut t = Table::new();
        t.insert("version".into(), Value::Integer(VERSION));
        t.insert("title".into(), self.title.clone().into());
        let paths = self.paths.to_table(home);
        if !paths.is_empty() {
            t.insert("paths".into(), Value::Table(paths));
        }
        t.insert("layout".into(), self.layout.to_value(home));
        for (k, v) in &self.rest {
            t.insert(k.clone(), v.clone());
        }
        toml::to_string(&t).expect("TOML の表は必ず書ける")
    }
}

/// JSON の値 → TOML の値。TOML に無い null は落とす
fn json_to_toml(v: &serde_json::Value) -> Option<Value> {
    use serde_json::Value as J;
    Some(match v {
        J::Null => return None,
        J::Bool(b) => Value::Boolean(*b),
        J::Number(n) => n.as_i64().map(Value::Integer).or_else(|| n.as_f64().map(Value::Float))?,
        J::String(s) => Value::String(s.clone()),
        J::Array(a) => Value::Array(a.iter().filter_map(json_to_toml).collect()),
        J::Object(o) => Value::Table(o.iter().filter_map(|(k, v)| Some((k.clone(), json_to_toml(v)?))).collect()),
    })
}

// ------------------------------------------------------------------ space

/// スペースの設定(`space.toml`)。ブックマークに書いていない項目の既定の paths を持つ
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Space {
    pub paths: Paths,
    /// 知らない項目(`[ui.<部品名>]` など)。書き戻すときにそのまま残す
    pub rest: Table,
}

impl Space {
    pub const FILE: &'static str = "space.toml";

    pub fn parse(text: &str) -> Result<Space, Error> {
        let mut t: Table = toml::from_str(text).map_err(|_| Error::NotBookmark)?;
        let paths = Paths::from_value(t.remove("paths").as_ref());
        Ok(Space { paths, rest: t })
    }

    pub fn to_toml(&self, home: &str) -> String {
        let mut t = Table::new();
        let paths = self.paths.to_table(home);
        if !paths.is_empty() {
            t.insert("paths".into(), Value::Table(paths));
        }
        for (k, v) in &self.rest {
            t.insert(k.clone(), v.clone());
        }
        toml::to_string(&t).expect("TOML の表は必ず書ける")
    }
}

// ------------------------------------------------------------------ file name

/// タイトル → ファイル名(拡張子つき)。`/` などファイル名に使えない字は「-」にする
pub fn file_name(title: &str) -> String {
    let bad = |c: char| matches!(c, '/' | '\\' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\n' | '\r' | '\t');
    let mut s = String::new();
    let mut prev_bad = false;
    for c in title.chars() {
        if !bad(c) {
            s.push(c);
        } else if !prev_bad {
            s.push('-'); // 続いた使えない字は「-」1 つにする
        }
        prev_bad = bad(c);
    }
    let s = s.trim_start_matches(|c: char| c == '.' || c == '-' || c.is_whitespace());
    let s = s.trim_end_matches(|c: char| c == '-' || c.is_whitespace());
    let s: String = s.chars().take(80).collect();
    let s = s.trim();
    format!("{}.{EXT}", if s.is_empty() { "ブックマーク" } else { s })
}

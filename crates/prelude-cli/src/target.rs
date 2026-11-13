//! コマンドの対象(開くもの・ブックマーク・フォルダ)を、名前・番号・パスから決める。`prelude` のコマンドと MCP(`prelude mcp`)が同じものを使う
use prelude_proto::{BookmarkInfo, Client};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// コアに命令を頼むもの。ふだんは [`Client`]、テストでは偽物
pub trait Call {
    fn call(&mut self, cmd: &str, args: Value) -> Result<Value, String>;
}

impl Call for Client {
    fn call(&mut self, cmd: &str, args: Value) -> Result<Value, String> {
        Client::call(self, cmd, args)
    }
}

/// 絶対パスにする(シンボリックリンクはたどる。無ければ今のフォルダから)
pub fn abs(p: &str) -> PathBuf {
    let p = Path::new(p);
    std::fs::canonicalize(p).unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join(p))
}

fn bookmarks(c: &mut impl Call) -> Result<Vec<BookmarkInfo>, String> {
    serde_json::from_value(c.call("bookmark.list", json!({}))?).map_err(|e| e.to_string())
}

/// 開くもの
#[derive(Debug, PartialEq)]
pub enum Target {
    Bookmark(String),
    Folder(PathBuf),
    Url(String),
}

/// 開くものを決める: .prelude のファイル → ブックマーク、フォルダ → フォルダのペイン、b3 → 番号のブックマーク、
/// 今のスペースのブックマークの名前(同じ名前、なければ名前の始まり)、URL らしければ Web
pub fn resolve(c: &mut impl Call, target: &str) -> Result<Target, String> {
    let p = Path::new(target);
    if target.ends_with(".prelude") && p.is_file() {
        return Ok(Target::Bookmark(abs(target).to_string_lossy().into_owned()));
    }
    if p.is_dir() {
        return Ok(Target::Folder(abs(target)));
    }
    let url_like = target.contains("://") || (!target.contains(' ') && !target.starts_with('/') && target.contains('.') && !p.exists());
    let list = bookmarks(c)?;
    // b3: 並びの番号(ランチャーと同じ)
    if let Some(n) = target.strip_prefix('b').and_then(|n| n.parse::<u32>().ok()) {
        if let Some(b) = list.iter().find(|b| b.num == n) {
            return Ok(Target::Bookmark(b.path.clone()));
        }
    }
    let lower = target.to_lowercase();
    let hit =
        list.iter().find(|b| b.title.to_lowercase() == lower).or_else(|| list.iter().find(|b| b.title.to_lowercase().starts_with(&lower)));
    match hit {
        Some(b) if !url_like || b.title.to_lowercase() == lower => Ok(Target::Bookmark(b.path.clone())),
        _ if url_like => Ok(Target::Url(if target.contains("://") { target.to_string() } else { format!("https://{target}") })),
        _ => Err(format!("Nothing to open: {target}")),
    }
}

/// ブックマークのパス。.prelude のファイルならそのパス、b3 なら並びの番号、ほかは `prelude bookmarks` の名前(フォルダ/題名、または題名)
pub fn bookmark_path(c: &mut impl Call, b: &str) -> Result<String, String> {
    if b.ends_with(".prelude") && Path::new(b).is_file() {
        return Ok(abs(b).to_string_lossy().into_owned());
    }
    let list = bookmarks(c)?;
    // b3: 並びの番号
    if let Some(b) = b.strip_prefix('b').and_then(|n| n.parse::<u32>().ok()).and_then(|n| list.iter().find(|x| x.num == n)) {
        return Ok(b.path.clone());
    }
    let name = |x: &BookmarkInfo| if x.folder.is_empty() { x.title.clone() } else { format!("{}/{}", x.folder, x.title) };
    let hits: Vec<&BookmarkInfo> = list.iter().filter(|x| name(x) == b).collect();
    let hits = if hits.is_empty() { list.iter().filter(|x| x.title == b).collect() } else { hits };
    match hits.as_slice() {
        [one] => Ok(one.path.clone()),
        [] => Err(format!("No such bookmark: {b}")),
        _ => Err(format!("More than one bookmark is named {b}; give it as folder/title")),
    }
}

/// ブックマークのフォルダのパス。フォルダならそのパス、ほかは今のスペースの中の名前。空なら空(スペースの直下)
pub fn folder_path(c: &mut impl Call, f: &str) -> Result<String, String> {
    if f.is_empty() {
        return Ok(String::new());
    }
    if f.contains('/') && Path::new(f).is_dir() {
        return Ok(abs(f).to_string_lossy().into_owned());
    }
    let state = c.call("state.get", json!({}))?;
    let dir = state["space_dir"].as_str().ok_or("No space folder in the state")?;
    Ok(Path::new(dir).join(f).to_string_lossy().into_owned())
}

// ブックマーク(.prelude)とスペース(space.toml)のテスト。cargo test で動かす
use prelude_format::{abs_path, file_name, Bookmark, Error, Node, Paths, Resolver, Space, Value};
use std::path::PathBuf;

const HOME: &str = "/home/u";

fn fixture(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/format").join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn to_json(v: &Value) -> serde_json::Value {
    serde_json::to_value(v).unwrap()
}

// 読んだ結果を、fixtures の *.expected.json と同じ形にする(Electron 版のテストも同じものと比べる)
fn view(b: &Bookmark) -> serde_json::Value {
    let paths: serde_json::Map<_, _> = b.paths.iter().map(|(k, v)| (k.to_string(), v.into())).collect();
    serde_json::json!({ "title": b.title, "paths": paths, "root": to_json(&b.layout.to_value("")) })
}

#[test]
fn 共通のファイルを読む() {
    for name in ["full", "legacy"] {
        let b = Bookmark::parse(&fixture(&format!("{name}.prelude")), HOME).unwrap();
        let want: serde_json::Value = serde_json::from_str(&fixture(&format!("{name}.expected.json"))).unwrap();
        assert_eq!(view(&b), want, "{name}");
    }
}

#[test]
fn 書いて読むと同じに戻る() {
    let b = Bookmark::parse(&fixture("full.prelude"), HOME).unwrap();
    let text = b.to_toml(HOME);
    assert!(text.contains("path = \"~/src\""), "{text}");
    assert_eq!(Bookmark::parse(&text, HOME).unwrap(), b);
    // 別の PC のホームで読むと、そのホームの下になる
    let other = Bookmark::parse(&text, "/home/other").unwrap();
    assert_eq!(other.layout.leaves()[1].str("path"), "/home/other/src");
}

#[test]
fn 知らない項目と知らない種類のペインは残す() {
    let text = r#"
version = 2
title = "x"
note = "自分のメモ"

[paths]
files = "~/a"
camera = "~/写真"

[layout]
k = "leaf"
type = "calendar"
source = "work"

[ui.field]
cards = [1, 2]
"#;
    let b = Bookmark::parse(text, HOME).unwrap();
    assert_eq!(b.paths.get("camera"), Some("~/写真"));
    let Node::Leaf(l) = &b.layout else { panic!() };
    assert_eq!((l.kind.as_str(), l.str("source")), ("calendar", "work"));
    let back = Bookmark::parse(&b.to_toml(HOME), HOME).unwrap();
    assert_eq!(back.rest.get("note").and_then(Value::as_str), Some("自分のメモ"));
    assert_eq!(back.rest["ui"]["field"]["cards"].as_array().map(Vec::len), Some(2));
    assert_eq!(back, b);
}

#[test]
fn 古い_json_を読んで_toml_で書く() {
    let b = Bookmark::parse(&fixture("legacy.prelude"), HOME).unwrap();
    let text = b.to_toml(HOME);
    assert!(text.starts_with("version = 2\n"), "{text}");
    assert_eq!(Bookmark::parse(&text, HOME).unwrap(), b);
}

#[test]
fn ブックマークでないファイル_壊れた配置は読まない() {
    assert_eq!(Bookmark::parse("not toml = = 1", HOME), Err(Error::NotBookmark));
    assert_eq!(Bookmark::parse("title = \"no layout\"", HOME), Err(Error::NotBookmark));
    assert_eq!(Bookmark::parse("{\"a\":1}", HOME), Err(Error::NotBookmark));
    assert_eq!(Bookmark::parse("version = 3\n[layout]\nk = \"leaf\"", HOME), Err(Error::Newer));
    assert_eq!(Bookmark::parse(r#"{"prelude":"bookmark","version":2,"root":{"k":"leaf"}}"#, HOME), Err(Error::Newer));
    // 深すぎる・ペインが多すぎる
    let mut deep = String::from(r#"{"k":"leaf","type":"web"}"#);
    for _ in 0..20 {
        deep = format!(r#"{{"k":"split","a":{deep},"b":{{"k":"leaf","type":"web"}}}}"#);
    }
    assert!(Bookmark::parse(&format!(r#"{{"prelude":"bookmark","root":{deep}}}"#), HOME).is_err());
    let mut many = String::from(r#"{"k":"leaf"}"#);
    for _ in 0..4 {
        many = format!(r#"{{"k":"split","a":{many},"b":{many}}}"#);
    }
    let many = format!(r#"{{"k":"split","a":{many},"b":{{"k":"leaf"}}}}"#); // 17 個
    assert_eq!(Bookmark::parse(&format!(r#"{{"prelude":"bookmark","root":{many}}}"#), HOME), Err(Error::TooManyPanes));
}

#[test]
fn 起動されるコマンドを取り出す() {
    let text = r#"
[layout]
k = "split"
[layout.a]
k = "leaf"
type = "app"
cmd = " kate "
[layout.b]
k = "split"
[layout.b.a]
k = "leaf"
type = "app"
cmd = "kate"
[layout.b.b]
k = "leaf"
type = "app"
cmd = "chromium --app=https://b/"
webUrl = "https://b/"
"#;
    assert_eq!(Bookmark::parse(text, HOME).unwrap().layout.commands(), vec!["kate".to_string()]);
}

#[test]
fn 使うフォルダはブックマーク_スペース_代わりの項目_ホームの順() {
    let bm = Bookmark::parse(&fixture("full.prelude"), HOME).unwrap();
    let space = Space::parse(&fixture("space.toml")).unwrap();
    let bm_dir = "/home/u/スペース/論文執筆/執筆";
    let sp_dir = "/home/u/スペース/論文執筆";
    let r = Resolver { home: HOME, bookmark: Some((&bm.paths, bm_dir)), space: Some((&space.paths, sp_dir)) };
    assert_eq!(r.resolve("files"), "/home/u/研究/論文");
    assert_eq!(r.resolve("open"), "/home/u/スペース/論文執筆/執筆/文献"); // ブックマークの相対パスは .prelude のフォルダから
    assert_eq!(r.resolve("terminal"), "/home/u/スペース/論文執筆/src"); // スペースの相対パスはスペースのフォルダから
    assert_eq!(r.resolve("download"), "/home/u/研究/論文/原稿"); // download → save
    assert_eq!(r.resolve("camera"), "/home/u/研究/論文"); // 知らない項目 → files
                                                          // ブックマークにもスペースにも無ければホーム
    let none = Paths::default();
    let r = Resolver { home: HOME, bookmark: Some((&none, bm_dir)), space: None };
    assert_eq!(r.resolve("save"), HOME);
    assert_eq!(r.get("save"), None);
}

#[test]
fn パスをたどる() {
    assert_eq!(abs_path("~/a/../b/./c", HOME, "/x"), "/home/u/b/c");
    assert_eq!(abs_path("../y", HOME, "/x/z"), "/x/y");
    assert_eq!(abs_path("/../..", HOME, "/x"), "/");
    assert_eq!(abs_path("~user2", HOME, "/x"), "/x/~user2"); // ~ の後に / が無いものはホームにしない
}

#[test]
fn スペースの設定を読み書きする() {
    let s = Space::parse(&fixture("space.toml")).unwrap();
    assert_eq!(s.paths.get("terminal"), Some("./src"));
    let back = Space::parse(&s.to_toml(HOME)).unwrap();
    assert_eq!(back, s);
    assert!(back.rest["ui"]["nav"]["order"].is_array());
    assert_eq!(Space::parse("").unwrap(), Space::default());
}

#[test]
fn タイトルからファイル名を作る() {
    assert_eq!(file_name("github.com | src"), "github.com - src.prelude");
    assert_eq!(file_name("a/b"), "a-b.prelude");
    assert_eq!(file_name("a-/b"), "a--b.prelude");
    assert_eq!(file_name("../.."), "ブックマーク.prelude");
    assert_eq!(file_name(""), "ブックマーク.prelude");
}

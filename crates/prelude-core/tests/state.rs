// コアの状態(タブ・分割・スペース・ブックマーク・書き出し)のテスト。cargo test で動かす
use prelude_core::state::{Core, SPACE_DEFAULT};
use prelude_proto::{Command, Dir, ErrorCode, PaneSpec, State, TabRef};
use serde_json::{json, Value};
use std::path::PathBuf;

const HOME: &str = "/home/u";

/// テストごとの空のスペースのフォルダ
fn spaces(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("state").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn web(url: &str) -> PaneSpec {
    serde_json::from_value(json!({ "type": "web", "url": url })).unwrap()
}

fn ok(c: &mut Core, cmd: Command) -> Value {
    let o = c.handle(cmd);
    o.reply.unwrap_or_else(|e| panic!("{e:?}"))
}

fn state(c: &Core) -> State {
    c.state()
}

fn pane_ids(layout: &Value) -> Vec<u64> {
    match layout["k"].as_str() {
        Some("split") => [pane_ids(&layout["a"]), pane_ids(&layout["b"])].concat(),
        _ => vec![layout["id"].as_u64().unwrap()],
    }
}

#[test]
fn タブを開く_閉じる_番号は空いたものから() {
    let mut c = Core::new(HOME, spaces("tabs"), None);
    let a = ok(&mut c, Command::TabOpen { pane: web("https://a.example/"), index: None }).as_u64().unwrap();
    let b = ok(&mut c, Command::TabOpen { pane: web("https://b.example/x"), index: None }).as_u64().unwrap();
    let s = state(&c);
    // 新しいタブは一番上で、今のタブになる
    assert_eq!(s.tabs.iter().map(|t| (t.id, t.num)).collect::<Vec<_>>(), vec![(b, 2), (a, 1)]);
    assert_eq!(s.active, Some(b));
    assert_eq!(s.tabs[0].title, "b.example");
    // 番号 1 を閉じると、次に開くタブが 1 を使う
    ok(&mut c, Command::TabClose { tab: Some(TabRef::Num { num: 1 }) });
    let n = ok(&mut c, Command::TabOpen { pane: web("https://c.example/"), index: Some(9) }).as_u64().unwrap();
    let s = state(&c);
    assert_eq!(s.tabs.last().map(|t| (t.id, t.num)), Some((n, 1))); // index が大きすぎれば一番下
                                                                    // 今のタブを閉じると、同じ位置(無ければ 1 つ上)のタブが今のタブになる
    ok(&mut c, Command::TabClose { tab: None });
    assert_eq!(state(&c).active, Some(b));
    // 無いタブ
    let o = c.handle(Command::TabActivate { tab: TabRef::Id { id: 999 } });
    assert_eq!(o.reply.unwrap_err().0, ErrorCode::NotFound);
    assert!(!o.changed);
}

#[test]
fn 分割する_比率を変える_ペインを閉じる() {
    let mut c = Core::new(HOME, spaces("split"), None);
    ok(&mut c, Command::TabOpen { pane: web("https://a/"), index: None });
    let first = pane_ids(&state(&c).tabs[0].layout)[0];
    let p2 = ok(&mut c, Command::PaneSplit { pane: first, dir: Dir::Row, new: web("https://b/") }).as_u64().unwrap();
    let p3 = ok(&mut c, Command::PaneSplit { pane: p2, dir: Dir::Col, new: web("https://c/") }).as_u64().unwrap();
    let s = state(&c);
    let l = &s.tabs[0].layout;
    assert_eq!(pane_ids(l), vec![first, p2, p3]);
    assert_eq!((l["dir"].as_str(), l["b"]["dir"].as_str()), (Some("row"), Some("col")));
    assert_eq!(s.tabs[0].active_pane, p3);
    // 比率は一番近い分割に効き、0.05〜0.95 に収める
    ok(&mut c, Command::SplitRatio { pane: p3, ratio: 2.0 });
    ok(&mut c, Command::SplitRatio { pane: first, ratio: 0.3 });
    let l = state(&c).tabs[0].layout.clone();
    assert_eq!((l["ratio"].as_f64(), l["b"]["ratio"].as_f64()), (Some(0.3), Some(0.95)));
    // ペインの中身を変える。null は消す。type は変えない
    let mut f = serde_json::Map::new();
    f.insert("url".into(), json!("https://b2/"));
    f.insert("type".into(), json!("files"));
    f.insert("title".into(), json!("B"));
    ok(&mut c, Command::PaneUpdate { pane: p2, fields: f });
    let mut f = serde_json::Map::new();
    f.insert("title".into(), Value::Null);
    ok(&mut c, Command::PaneUpdate { pane: p2, fields: f });
    let b = state(&c).tabs[0].layout["b"]["a"].clone();
    assert_eq!(b, json!({ "k": "leaf", "type": "web", "url": "https://b2/", "id": p2 }));
    // 今のペインを閉じると、残った側の最初のペインが今のペインになる
    ok(&mut c, Command::PaneClose { pane: p3 });
    let s = state(&c);
    assert_eq!(pane_ids(&s.tabs[0].layout), vec![first, p2]);
    assert_eq!(s.tabs[0].active_pane, p2);
    // 最後のペインを閉じるとタブが閉じる
    ok(&mut c, Command::PaneClose { pane: p2 });
    ok(&mut c, Command::PaneClose { pane: first });
    assert!(state(&c).tabs.is_empty());
}

#[test]
fn ペインの場所は覚えるが_状態は変わらない() {
    let mut c = Core::new(HOME, spaces("place"), None);
    ok(&mut c, Command::TabOpen { pane: web("https://a/"), index: None });
    let p = pane_ids(&state(&c).tabs[0].layout)[0];
    let rects = serde_json::from_value(
        json!([{ "pane": p, "x": 0, "y": 30, "width": 800, "height": 600 }, { "pane": 999, "x": 0, "y": 0, "width": 1, "height": 1 }]),
    )
    .unwrap();
    let o = c.handle(Command::PanePlace { rects });
    assert!(o.reply.is_ok() && !o.changed);
    assert_eq!(c.place_of(p).map(|r| r.width), Some(800));
    assert_eq!(c.place_of(999), None); // 無いペインは覚えない
}

#[test]
fn スペースを作る_切り替える_タブはスペースごと() {
    let root = spaces("spaces");
    let mut c = Core::new(HOME, &root, None);
    assert_eq!(ok(&mut c, Command::SpaceList {}), json!([SPACE_DEFAULT])); // 1 つも無ければ既定のスペースを作る
    ok(&mut c, Command::TabOpen { pane: web("https://main/"), index: None });
    assert_eq!(ok(&mut c, Command::SpaceCreate { name: " 論文執筆 ".into() }), json!("論文執筆"));
    assert_eq!(c.handle(Command::SpaceCreate { name: "論文執筆".into() }).reply.unwrap_err().0, ErrorCode::Conflict);
    assert_eq!(c.handle(Command::SpaceCreate { name: "../x".into() }).reply.unwrap_err().0, ErrorCode::BadRequest);
    assert_eq!(c.handle(Command::SpaceSwitch { name: "無い".into() }).reply.unwrap_err().0, ErrorCode::NotFound);

    ok(&mut c, Command::SpaceSwitch { name: "論文執筆".into() });
    let s = state(&c);
    assert_eq!((s.space.as_str(), s.tabs.len()), ("論文執筆", 0));
    assert!(s.space_dir.ends_with("/論文執筆"));
    ok(&mut c, Command::TabOpen { pane: web("https://paper/"), index: None });
    assert_eq!(state(&c).tabs[0].num, 1); // 番号はスペースごと
    ok(&mut c, Command::SpaceSwitch { name: SPACE_DEFAULT.into() });
    let s = state(&c);
    assert_eq!(s.tabs.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), vec!["main"]);
}

#[test]
fn ブックマークを開く_paths_は_ブックマーク_スペースの順() {
    let root = spaces("bookmarks");
    let dir = root.join(SPACE_DEFAULT);
    std::fs::create_dir_all(dir.join("執筆")).unwrap();
    std::fs::write(dir.join("space.toml"), "[paths]\nfiles = \"/data/論文\"\nterminal = \"./src\"\n").unwrap();
    std::fs::write(
        dir.join("執筆/本文.prelude"),
        "version = 2\ntitle = \"本文\"\n[paths]\nsave = \"./原稿\"\n[layout]\nk = \"split\"\n[layout.a]\nk = \"leaf\"\ntype = \"files\"\n[layout.b]\nk = \"leaf\"\ntype = \"web\"\nurl = \"https://x/\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("壊れた.prelude"), "not = = toml").unwrap();
    std::fs::write(dir.join(".隠し.prelude"), "").unwrap();
    let mut c = Core::new(HOME, &root, None);

    let list = ok(&mut c, Command::BookmarkList {});
    let names: Vec<(String, String)> =
        list.as_array().unwrap().iter().map(|b| (b["folder"].as_str().unwrap().into(), b["title"].as_str().unwrap().into())).collect();
    assert_eq!(names, vec![("".into(), "壊れた".into()), ("執筆".into(), "本文".into())]);

    let path = dir.join("執筆/本文.prelude").to_string_lossy().into_owned();
    let id = ok(&mut c, Command::BookmarkOpen { path: path.clone() });
    let s = state(&c);
    assert_eq!((s.tabs[0].title.as_str(), s.tabs[0].bookmark.as_deref()), ("本文", Some(path.as_str())));
    // パスの無いフォルダのペインは files(ここではスペースの既定)で開く
    assert_eq!(s.tabs[0].layout["a"]["path"], json!("/data/論文"));
    // もう開いていれば同じタブ(1 ブックマーク = 最大 1 タブ)
    ok(&mut c, Command::TabOpen { pane: web("https://other/"), index: None });
    assert_eq!(ok(&mut c, Command::BookmarkOpen { path: path.clone() }), id);
    assert_eq!(state(&c).active, id.as_u64());

    let p = ok(&mut c, Command::PathsGet { tab: None });
    let sdir = dir.to_string_lossy();
    assert_eq!(p["files"], json!("/data/論文"));
    assert_eq!(p["save"], json!(format!("{sdir}/執筆/原稿"))); // ブックマークの相対パスは .prelude のフォルダから
    assert_eq!(p["download"], p["save"]);
    assert_eq!(p["terminal"], json!(format!("{sdir}/src"))); // スペースの相対パスはスペースのフォルダから
                                                             // ブックマークから開いていないタブはスペースの既定
    let p = ok(&mut c, Command::PathsGet { tab: Some(TabRef::Num { num: 2 }) });
    assert_eq!(p["save"], json!("/data/論文"));

    let bad = dir.join("壊れた.prelude").to_string_lossy().into_owned();
    assert_eq!(c.handle(Command::BookmarkOpen { path: bad }).reply.unwrap_err().0, ErrorCode::BadRequest);
    assert_eq!(c.handle(Command::BookmarkOpen { path: "relative.prelude".into() }).reply.unwrap_err().0, ErrorCode::BadRequest);
}

#[test]
fn 書き出して戻す() {
    let root = spaces("saved");
    std::fs::create_dir_all(root.join("Work")).unwrap();
    std::fs::create_dir_all(root.join(SPACE_DEFAULT)).unwrap();
    let mut c = Core::new(HOME, &root, None);
    assert_eq!(c.state().space, SPACE_DEFAULT);
    ok(&mut c, Command::TabOpen { pane: serde_json::from_value(json!({ "type": "files", "path": "/home/u/src" })).unwrap(), index: None });
    let first = pane_ids(&state(&c).tabs[0].layout)[0];
    let p2 = ok(&mut c, Command::PaneSplit { pane: first, dir: Dir::Col, new: web("https://b/") }).as_u64().unwrap();
    ok(&mut c, Command::TabOpen { pane: web("https://second/"), index: Some(1) });
    ok(&mut c, Command::PaneFocus { pane: p2 });
    ok(&mut c, Command::SpaceSwitch { name: "Work".into() });
    ok(&mut c, Command::TabOpen { pane: web("https://work/"), index: None });
    let saved = c.saved();
    // ホームの下のパスは ~ で書く(.prelude と同じ)
    let text = serde_json::to_string(&saved).unwrap();
    assert!(text.contains("\"path\":\"~/src\""), "{text}");

    let back = Core::new(HOME, &root, Some(serde_json::from_str(&text).unwrap()));
    let s = back.state();
    assert_eq!((s.space.as_str(), s.tabs.len()), ("Work", 1));
    assert_eq!(back.saved(), saved);
    // 別のスペースのタブも、今のペイン・今のタブ・番号ごと戻る
    let mut back = back;
    ok(&mut back, Command::SpaceSwitch { name: SPACE_DEFAULT.into() });
    let s = back.state();
    assert_eq!(s.tabs.iter().map(|t| (t.num, t.title.as_str())).collect::<Vec<_>>(), vec![(1, "src | b"), (2, "second")]);
    assert_eq!(s.active, Some(s.tabs[0].id));
    assert_eq!(pane_ids(&s.tabs[0].layout)[1], s.tabs[0].active_pane);
    // 前のスペースが無くなっていたら既定のスペースにする
    std::fs::remove_dir_all(root.join("Work")).unwrap();
    assert_eq!(Core::new(HOME, &root, Some(saved)).state().space, SPACE_DEFAULT);
}

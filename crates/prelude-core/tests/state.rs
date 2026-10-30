// コアの状態(タブ・分割・スペース・ブックマーク・書き出し)のテスト。cargo test で動かす
use prelude_core::recipes::Recipe;
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
    let id = ok(&mut c, Command::BookmarkOpen { path: path.clone(), allow: vec![] })["tab"].clone();
    let s = state(&c);
    assert_eq!((s.tabs[0].title.as_str(), s.tabs[0].bookmark.as_deref()), ("本文", Some(path.as_str())));
    // パスの無いフォルダのペインは files(ここではスペースの既定)で開く
    assert_eq!(s.tabs[0].layout["a"]["path"], json!("/data/論文"));
    // もう開いていれば同じタブ(1 ブックマーク = 最大 1 タブ)
    ok(&mut c, Command::TabOpen { pane: web("https://other/"), index: None });
    assert_eq!(ok(&mut c, Command::BookmarkOpen { path: path.clone(), allow: vec![] })["tab"], id);
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
    assert_eq!(c.handle(Command::BookmarkOpen { path: bad, allow: vec![] }).reply.unwrap_err().0, ErrorCode::BadRequest);
    assert_eq!(
        c.handle(Command::BookmarkOpen { path: "relative.prelude".into(), allow: vec![] }).reply.unwrap_err().0,
        ErrorCode::BadRequest
    );
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

#[test]
fn インストール済みでない起動コマンドは外して開く() {
    let root = spaces("trust");
    let dir = root.join(SPACE_DEFAULT);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("もらった.prelude");
    std::fs::write(
        &path,
        "[layout]\nk = \"split\"\n[layout.a]\nk = \"leaf\"\ntype = \"app\"\ncmd = \"kate\"\n[layout.b]\nk = \"leaf\"\ntype = \"app\"\ncmd = \"sh -c evil\"\n",
    )
    .unwrap();
    let path = path.to_string_lossy().into_owned();
    let mut c = Core::new(HOME, &root, None);
    c.set_installed(|cmd| cmd == "kate");
    let r = ok(&mut c, Command::BookmarkOpen { path: path.clone(), allow: vec![] });
    assert_eq!(r["held"], json!(["sh -c evil"]));
    let cmds: Vec<String> = c.wants().into_iter().map(|w| w.cmd).collect();
    assert_eq!(cmds, vec!["kate"]); // 外したペインは起動しない
                                    // 許せば外さない
    ok(&mut c, Command::TabClose { tab: None });
    let r = ok(&mut c, Command::BookmarkOpen { path, allow: vec!["sh -c evil".into()] });
    assert_eq!(r["held"], json!([]));
    assert_eq!(c.wants().len(), 2);
}

#[test]
fn 取り込むペイン_レシピ_止まったペイン() {
    let root = spaces("wants");
    let dir = root.join(SPACE_DEFAULT);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("space.toml"), "[paths]\nterminal = \"/work/src\"\n").unwrap();
    let mut c = Core::new(HOME, &root, None);
    c.set_recipes(vec![Recipe {
        kind: "console".into(),
        name: "Console".into(),
        run: "konsole --workdir {paths.terminal}".into(),
        class: "org.kde.konsole".into(),
    }]);
    let spec = |v: Value| serde_json::from_value(v).unwrap();
    ok(&mut c, Command::TabOpen { pane: spec(json!({ "type": "console" })), index: None });
    let console = pane_ids(&state(&c).tabs[0].layout)[0];
    ok(&mut c, Command::TabOpen { pane: spec(json!({ "type": "app", "cmd": "kate", "cls": "kate" })), index: None });
    let kate = pane_ids(&state(&c).tabs[0].layout)[0];
    // フォルダ・知らない種類・コマンドの無いアプリは取り込まない
    for v in [json!({ "type": "files" }), json!({ "type": "calendar" }), json!({ "type": "app" })] {
        ok(&mut c, Command::TabOpen { pane: spec(v), index: Some(99) });
    }
    let rects = serde_json::from_value(json!([{ "pane": kate, "x": 10, "y": 20, "width": 300, "height": 200 }])).unwrap();
    ok(&mut c, Command::PanePlace { rects });

    let w = c.wants();
    let v: Vec<(u64, &str, &str, &str, bool)> =
        w.iter().map(|w| (w.pane, w.cmd.as_str(), w.cls.as_str(), w.cwd.as_str(), w.rect.is_some())).collect();
    assert_eq!(
        v,
        vec![(kate, "kate", "kate", "/work/src", true), (console, "konsole --workdir '/work/src'", "org.kde.konsole", "/work/src", false)]
    );
    assert!(w.iter().all(|w| w.marker.is_empty()));
    assert_eq!(ok(&mut c, Command::RecipeList {}), json!([{ "type": "console", "name": "Console" }]));

    // 窓が閉じたペインは残して止まったものにし、取り込まない。タブを開き直すと起動し直す
    assert!(c.note_exited(kate));
    assert!(!c.note_exited(kate));
    assert!(!c.note_exited(999));
    assert_eq!(state(&c).tabs[0].layout["stopped"], json!(true));
    assert_eq!(c.wants().len(), 1);
    assert!(c.note_title(console, "~ : bash".into()));
    assert!(!c.note_title(console, "~ : bash".into()));
    assert_eq!(state(&c).tabs[1].layout["window_title"], json!("~ : bash"));
    let first = state(&c).tabs[0].id;
    ok(&mut c, Command::TabActivate { tab: prelude_proto::TabRef::Id { id: first } });
    assert_eq!(c.wants().len(), 2);
    // スペースを切り替えると、前のスペースのペインは取り込まない(アプリは閉じる)
    std::fs::create_dir_all(root.join("Other")).unwrap();
    ok(&mut c, Command::SpaceSwitch { name: "Other".into() });
    assert!(c.wants().is_empty());
}

#[test]
fn web_のペイン_印_url_の知らせ_戻る() {
    let mut c = Core::new(HOME, spaces("web"), None);
    c.set_tag(".dtest");
    let spec = |v: Value| serde_json::from_value(v).unwrap();
    ok(&mut c, Command::TabOpen { pane: spec(json!({ "type": "web", "url": "https://a.example/" })), index: None });
    let web = pane_ids(&state(&c).tabs[0].layout)[0];
    // Electron 版の実ブラウザのペイン(アプリのペインの webUrl)も Web のペインとして開く
    ok(
        &mut c,
        Command::TabOpen { pane: spec(json!({ "type": "app", "cmd": "chromium --app=x", "webUrl": "https://b.example/" })), index: None },
    );
    let old = pane_ids(&state(&c).tabs[0].layout)[0];

    let w = c.wants();
    let m: Vec<(u64, &str, &str)> = w.iter().map(|w| (w.pane, w.marker.as_str(), w.cmd.as_str())).collect();
    let (mw, mo) = (format!("PRELUDE-PANE:.dtest:{web}"), format!("PRELUDE-PANE:.dtest:{old}"));
    assert_eq!(m, vec![(old, mo.as_str(), ""), (web, mw.as_str(), "")]); // 起動コマンドは使わない
    let (ww, dl) = c.web_wants();
    assert_eq!(
        ww.iter().map(|w| (w.pane, w.url.as_str())).collect::<Vec<_>>(),
        vec![(old, "https://b.example/"), (web, "https://a.example/")]
    );
    assert_eq!(dl, HOME); // ダウンロードの保存先(paths の download。どこにも無ければホーム)

    // ページの中で移った URL は残し、題名は今の様子として出す
    assert_eq!(c.note_web(web, "https://a.example/next".into(), "Next".into()), (true, true));
    assert_eq!(c.note_web(web, "https://a.example/next".into(), "Next".into()), (false, false));
    assert_eq!(c.note_web(old, "https://b.example/2".into(), String::new()), (true, true));
    let s = state(&c);
    assert_eq!(
        (s.tabs[1].layout["url"].as_str(), s.tabs[1].layout["window_title"].as_str()),
        (Some("https://a.example/next"), Some("Next"))
    );
    assert_eq!(s.tabs[0].layout["webUrl"], json!("https://b.example/2"));
    assert!(c.saved().spaces[SPACE_DEFAULT].tabs.iter().any(|t| t.layout["url"] == "https://a.example/next"));

    // 戻る・進むは Web のペインだけ
    ok(&mut c, Command::WebGo { pane: web, action: prelude_proto::WebAction::Back });
    ok(&mut c, Command::TabOpen { pane: spec(json!({ "type": "files", "path": "/x" })), index: None });
    let files = pane_ids(&state(&c).tabs[0].layout)[0];
    assert_eq!(
        c.handle(Command::WebGo { pane: files, action: prelude_proto::WebAction::Reload }).reply.unwrap_err().0,
        ErrorCode::BadRequest
    );
    // 止まった Web のペインは開かない
    c.note_exited(web);
    assert_eq!(c.web_wants().0.len(), 1);
}

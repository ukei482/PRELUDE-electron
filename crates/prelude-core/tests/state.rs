// コアの状態(タブ・分割・スペース・ブックマーク・書き出し)のテスト。cargo test で動かす
use prelude_core::apps::App;
use prelude_core::recipes::Recipe;
use prelude_core::state::{Core, SPACE_DEFAULT};
use prelude_proto::{Command, Dir, ErrorCode, PaneSpec, State, TabRef};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

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

/// 決まったアプリの一覧(XDG の .desktop を読まない)
fn fixed(apps: Vec<App>) -> std::sync::Arc<dyn prelude_core::apps::AppSource> {
    std::sync::Arc::new(prelude_core::apps::Fixed(apps))
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
    c.set_recipes(prelude_core::recipes::load(std::path::Path::new("/nonexistent"))); // 既定のレシピ(files は {path} を使う)

    let list = ok(&mut c, Command::BookmarkList {});
    let names: Vec<(String, String)> =
        list.as_array().unwrap().iter().map(|b| (b["folder"].as_str().unwrap().into(), b["title"].as_str().unwrap().into())).collect();
    assert_eq!(names, vec![("".into(), "壊れた".into()), ("執筆".into(), "本文".into())]);

    let path = dir.join("執筆/本文.prelude").to_string_lossy().into_owned();
    let id = ok(&mut c, Command::BookmarkOpen { path: path.clone(), allow: vec![] })["tab"].clone();
    let s = state(&c);
    assert_eq!((s.tabs[0].title.as_str(), s.tabs[0].bookmark.as_deref()), ("本文", Some(path.as_str())));
    // 場所({path})を使うレシピのペインに path が無ければ、files(ここではスペースの既定)で開く。タブの名前はその最後の名前
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
    c.set_apps(fixed(vec![App { name: "Kate".into(), exec: "kate".into(), ..App::default() }]));
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
    // cwd のあるコンソール(ファイラの「ここで端末を開く」)は paths.terminal の代わりにそこで開く
    let here = ok(
        &mut c,
        Command::PaneSplit { pane: console, dir: prelude_proto::Dir::Col, new: spec(json!({ "type": "console", "cwd": "/data/x y" })) },
    );
    let here = here.as_u64().unwrap();
    let w = c.wants();
    let h = w.iter().find(|w| w.pane == here).unwrap();
    assert_eq!((h.cmd.as_str(), h.cwd.as_str()), ("konsole --workdir '/data/x y'", "/data/x y"));
    ok(&mut c, Command::PaneClose { pane: here });

    // 窓が出ずに終わったペインは残して止まったものにし、取り込まない。タブを開き直すと起動し直す
    assert!(c.note_exited(kate, false));
    assert!(!c.note_exited(kate, false));
    assert!(!c.note_exited(999, false));
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
fn ランチャーの候補_開くと点数が上がる() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NOW: AtomicU64 = AtomicU64::new(1_000_000);
    const DAY: u64 = 24 * 3600 * 1000;
    let root = spaces("items");
    let main = root.join(SPACE_DEFAULT);
    std::fs::create_dir_all(main.join("研究")).unwrap();
    std::fs::create_dir_all(root.join("Work")).unwrap();
    let bm = |p: &std::path::Path| std::fs::write(p, "[layout]\ntype = \"web\"\nurl = \"https://a/\"\n").unwrap();
    bm(&main.join("Alpha.prelude"));
    bm(&main.join("研究/論文.prelude"));
    let mut c = Core::new(HOME, &root, None);
    c.set_clock(|| NOW.load(Ordering::Relaxed));
    c.set_apps(fixed({
        let app = |id: &str, name: &str| App { name: name.into(), exec: id.into(), cls: String::new(), id: id.into(), icon: String::new() };
        vec![app("kate", "Kate"), app("org.kde.konsole", "Konsole"), app("kate", "Kate (dup)")]
    }));
    let keys = |c: &Core| c.items().into_iter().map(|i| i.key).collect::<Vec<_>>();
    let alpha = format!("bookmark:{}", main.join("Alpha.prelude").display());
    let paper = format!("bookmark:{}", main.join("研究/論文.prelude").display());
    // 点数が無ければ種類(ブックマーク・アプリ・スペース)と名前の順。今のスペースは出さない。同じ id のアプリは 1 つ
    assert_eq!(keys(&c), vec![alpha.clone(), paper.clone(), "app:kate".into(), "app:org.kde.konsole".into(), "space:Work".into()]);
    assert_eq!(c.items()[1].detail, "研究");

    // アプリを開くとタブになり、タブが先に出る。開いたものは点数が上がって前に出る
    let tab = ok(&mut c, Command::ItemOpen { key: "app:org.kde.konsole".into(), allow: vec![], split: None }).as_u64().unwrap();
    let w = c.wants();
    assert_eq!((w.len(), w[0].cmd.as_str()), (1, "org.kde.konsole"));
    let it = c.items();
    assert_eq!((it[0].key.clone(), it[0].kind.as_str(), it[0].detail.as_str()), (format!("tab:{tab}"), "tab", "1"));
    assert_eq!(it[1].key, "app:org.kde.konsole");
    // 2 回開いたものは、後で 1 回開いたものより上。ただし古くなると減る
    ok(&mut c, Command::ItemOpen { key: paper.clone(), allow: vec![], split: None });
    ok(&mut c, Command::TabClose { tab: None });
    ok(&mut c, Command::BookmarkOpen { path: paper.trim_start_matches("bookmark:").into(), allow: vec![] });
    NOW.fetch_add(DAY, Ordering::Relaxed);
    ok(&mut c, Command::ItemOpen { key: alpha.clone(), allow: vec![], split: None });
    // タブで開いているブックマークはタブとして出すので、閉じてから比べる
    ok(&mut c, Command::TabClose { tab: None });
    ok(&mut c, Command::TabClose { tab: None });
    let rest: Vec<String> = c.items().into_iter().filter(|i| i.kind != "tab").map(|i| i.key).collect();
    assert_eq!(&rest[..3], &[paper.clone(), alpha.clone(), "app:org.kde.konsole".into()]);
    NOW.fetch_add(60 * DAY, Ordering::Relaxed); // 2 の点も 60 日で 1 より小さくなる
    ok(&mut c, Command::ItemOpen { key: "app:kate".into(), allow: vec![], split: None });
    assert_eq!(c.items().into_iter().find(|i| i.kind != "tab").unwrap().key, "app:kate");

    // タブを指して開く・スペースを切り替える
    ok(&mut c, Command::ItemOpen { key: format!("tab:{tab}"), allow: vec![], split: None });
    assert_eq!(state(&c).active, Some(tab));
    ok(&mut c, Command::ItemOpen { key: "space:Work".into(), allow: vec![], split: None });
    assert_eq!(state(&c).space, "Work");
    assert!(c.items().iter().any(|i| i.key == format!("space:{SPACE_DEFAULT}")));

    // 点数は書き出して戻す
    let saved = c.saved();
    assert_eq!(saved.history.len(), 5);
    let mut back = Core::new(HOME, &root, Some(saved.clone()));
    back.set_clock(|| NOW.load(Ordering::Relaxed));
    assert_eq!(back.saved().history, saved.history);

    // 知らない key
    for k in ["nope", "tab:x", "what:1", "app:none"] {
        assert!(c.handle(Command::ItemOpen { key: k.into(), allow: vec![], split: None }).reply.is_err(), "{k}");
    }
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
    assert_eq!(dl, ""); // ダウンロードの保存先: paths の download → 設定の download_dir。どこにも無ければ空(Chromium が置いた場所のまま)

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
    c.note_exited(web, false);
    assert_eq!(c.web_wants().0.len(), 1);
}

#[test]
fn コンポジタに渡す配置の木() {
    use prelude_core::compositor::Tree;
    use prelude_format::Dir as FDir;
    let mut c = Core::new(HOME, spaces("scene"), None);
    ok(&mut c, Command::TabOpen { pane: web("https://a/"), index: None });
    let first = pane_ids(&state(&c).tabs[0].layout)[0];
    let p2 = ok(&mut c, Command::PaneSplit { pane: first, dir: Dir::Row, new: web("https://b/") }).as_u64().unwrap();
    ok(&mut c, Command::SplitRatio { pane: first, ratio: 0.3 });
    ok(&mut c, Command::TabOpen { pane: web("https://c/"), index: None });
    let s = c.scene();
    assert_eq!(s.tabs.len(), 2);
    let split = s.tabs.iter().find(|t| matches!(t.tree, Tree::Split { .. })).unwrap();
    let single = s.tabs.iter().find(|t| matches!(t.tree, Tree::Pane(_))).unwrap();
    assert_eq!(s.active, Some(single.id)); // 後で開いたタブが今のタブ
    assert_eq!(split.tree, Tree::Split { dir: FDir::Row, ratio: 0.3, a: Box::new(Tree::Pane(first)), b: Box::new(Tree::Pane(p2)) });
    assert_eq!(split.active_pane, p2);
    // 窓を出すペインは wants と同じ
    assert_eq!(s.panes, c.wants());
    assert_eq!(s.panes.len(), 3);
}

#[test]
fn ファイラ_ペインのフォルダか_paths_の_files() {
    let root = spaces("filer");
    let dir = root.join(SPACE_DEFAULT);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("space.toml"), "[paths]\nfiles = \"/work/docs\"\n").unwrap();
    let mut c = Core::new(HOME, &root, None);
    c.set_recipes(vec![Recipe {
        kind: "files".into(),
        name: "Files".into(),
        run: "prelude-filer --pane {pane} {path}".into(),
        class: "prelude-filer".into(),
    }]);
    let spec = |v: Value| serde_json::from_value(v).unwrap();
    ok(&mut c, Command::TabOpen { pane: spec(json!({ "type": "files" })), index: None });
    let a = pane_ids(&state(&c).tabs[0].layout)[0];
    ok(&mut c, Command::TabOpen { pane: spec(json!({ "type": "files", "path": "/tmp/it's" })), index: Some(9) });
    let b = pane_ids(&state(&c).tabs[1].layout)[0];
    let w = c.wants();
    let cmds: Vec<(u64, String)> = w.iter().map(|w| (w.pane, w.cmd.clone())).collect();
    assert_eq!(
        cmds,
        vec![(a, format!("prelude-filer --pane '{a}' '/work/docs'")), (b, format!(r"prelude-filer --pane '{b}' '/tmp/it'\''s'")),]
    );
    // ファイラが移ったフォルダは pane.update で残す(開き直すとそのフォルダ)
    let mut f = serde_json::Map::new();
    f.insert("path".into(), json!("/work/src"));
    ok(&mut c, Command::PaneUpdate { pane: a, fields: f });
    assert!(c.wants()[0].cmd.ends_with("'/work/src'"));
}

#[test]
fn ファイラは_yazi_field_は_files_の別名_名前は_path_から() {
    let root = spaces("yazi");
    std::fs::create_dir_all(root.join(SPACE_DEFAULT)).unwrap();
    let mut c = Core::new(HOME, &root, None);
    let mut list = prelude_core::recipes::load(std::path::Path::new("/nonexistent"));
    // 自分で足したレシピでも、場所(path)を持てば、その最後の名前がタブの名前になる
    list.push(Recipe { kind: "photos".into(), name: "写真".into(), run: "viewer {path}".into(), class: String::new() });
    c.set_recipes(list);
    let spec = |v: Value| serde_json::from_value(v).unwrap();
    ok(&mut c, Command::TabOpen { pane: spec(json!({ "type": "field", "path": "/x/文献/" })), index: None });
    ok(&mut c, Command::TabOpen { pane: spec(json!({ "type": "photos", "path": "/x/旅行" })), index: Some(9) });
    let s = state(&c);
    assert_eq!((s.tabs[0].title.as_str(), s.tabs[1].title.as_str()), ("文献", "旅行"));
    // field の古いペインも files のレシピ(yazi)で開く。種類はそのまま残す
    assert_eq!(s.tabs[0].layout["type"], json!("field"));
    let w = c.wants();
    assert_eq!(w[0].cmd, "prelude term --cwd '/x/文献/' -- yazi '/x/文献/'");
    // 端末を開くレシピの窓の名前(@terminal)は、設定の端末のもの
    assert_eq!(w[0].cls, "");
    c.apply_config(&prelude_format::settings::Config::from_text("terminal = \"konsole\"\n", &|_| None));
    assert_eq!(c.wants()[0].cls, "org.kde.konsole");
    assert_eq!(w[1].cmd, "viewer '/x/旅行'");
}

#[test]
fn タブを複製する_番号を変える_閉じたタブを開き直す() {
    let mut c = Core::new(HOME, spaces("tabcmds"), None);
    let a = ok(&mut c, Command::TabOpen { pane: web("https://a/"), index: None }).as_u64().unwrap();
    let b = ok(&mut c, Command::TabOpen { pane: web("https://b/"), index: Some(9) }).as_u64().unwrap();
    // 複製はすぐ下に、新しいペインの id で。番号は空いたもの
    let d = ok(&mut c, Command::TabDuplicate { tab: Some(TabRef::Id { id: a }) }).as_u64().unwrap();
    let s = state(&c);
    assert_eq!(s.tabs.iter().map(|t| t.id).collect::<Vec<_>>(), vec![a, d, b]);
    assert_eq!(s.tabs[1].layout["url"], json!("https://a/"));
    assert_ne!(pane_ids(&s.tabs[1].layout), pane_ids(&s.tabs[0].layout));
    assert_eq!((s.tabs[1].num, s.active), (3, Some(d)));
    // 番号を変える。使われている番号なら入れ替える
    ok(&mut c, Command::TabRenumber { tab: Some(TabRef::Id { id: d }), num: 1 });
    let nums = |c: &Core| state(c).tabs.iter().map(|t| t.num).collect::<Vec<_>>();
    assert_eq!(nums(&c), vec![3, 1, 2]);
    ok(&mut c, Command::TabRenumber { tab: Some(TabRef::Id { id: d }), num: 7 });
    assert_eq!(nums(&c), vec![3, 7, 2]);
    assert_eq!(c.handle(Command::TabRenumber { tab: None, num: 0 }).reply.unwrap_err().0, ErrorCode::BadRequest);
    // 閉じたタブを、新しく閉じたものから、閉じた位置に開き直す
    assert_eq!(state(&c).closed, 0);
    ok(&mut c, Command::TabClose { tab: Some(TabRef::Id { id: b }) });
    ok(&mut c, Command::TabClose { tab: Some(TabRef::Id { id: a }) });
    assert_eq!(state(&c).closed, 2);
    let r = ok(&mut c, Command::TabReopen {}).as_u64().unwrap();
    let s = state(&c);
    assert_eq!((s.tabs[0].id, s.tabs[0].layout["url"].clone(), s.active), (r, json!("https://a/"), Some(r)));
    ok(&mut c, Command::TabReopen {});
    assert_eq!(
        state(&c).tabs.iter().map(|t| t.layout["url"].clone()).collect::<Vec<_>>(),
        vec![json!("https://a/"), json!("https://a/"), json!("https://b/")]
    );
    assert_eq!(c.handle(Command::TabReopen {}).reply.unwrap_err().0, ErrorCode::NotFound);
    // 閉じたタブはスペースごと
    ok(&mut c, Command::TabClose { tab: None });
    ok(&mut c, Command::SpaceCreate { name: "別".into() });
    ok(&mut c, Command::SpaceSwitch { name: "別".into() });
    assert_eq!(state(&c).closed, 0);
    ok(&mut c, Command::SpaceSwitch { name: "Main".into() });
    assert_eq!(state(&c).closed, 1);
}

#[test]
fn ブックマークを書く_保存_上書き_名前_移動_ゴミ箱() {
    let root = spaces("bmwrite");
    let dir = root.join(SPACE_DEFAULT);
    std::fs::create_dir_all(&dir).unwrap();
    let mut c = Core::new(HOME, &root, None);
    c.set_trash_dir(root.join("Trash"));
    let d = |name: &str| dir.join(name).to_string_lossy().into_owned();
    let read = |p: &str| prelude_format::Bookmark::parse(&std::fs::read_to_string(p).unwrap(), HOME).unwrap();
    let rev = |c: &Core| state(c).bookmarks_rev;

    // 保存: タブの配置を書き、タブはそのブックマークを開いたものになる。書いたら知らせる
    let tab = ok(&mut c, Command::TabOpen { pane: web("https://a/"), index: None }).as_u64().unwrap();
    let r0 = rev(&c);
    let o = c.handle(Command::BookmarkSave { tab: None, title: "論文/下書き".into(), folder: String::new() });
    assert!(o.changed);
    let p = o.reply.unwrap().as_str().unwrap().to_string();
    assert_eq!(p, d("論文-下書き.prelude"));
    assert_eq!(read(&p).title, "論文/下書き");
    assert_eq!(state(&c).tabs[0].bookmark.as_deref(), Some(p.as_str()));
    assert_eq!(state(&c).tabs[0].title, "論文/下書き");
    assert!(rev(&c) > r0);
    // 同じ名前なら「(2)」
    ok(&mut c, Command::TabOpen { pane: web("https://b/"), index: Some(9) });
    let p2 = ok(&mut c, Command::BookmarkSave { tab: Some(TabRef::Num { num: 2 }), title: "論文/下書き".into(), folder: String::new() });
    assert_eq!(p2, json!(d("論文-下書き (2).prelude")));

    // 上書き: 題名・paths・知らない項目は残し、配置だけを替える
    std::fs::write(&p, "version = 2\ntitle = \"下書き\"\nmemo = \"残す\"\n[paths]\nsave = \"/out\"\n[layout]\nk = \"leaf\"\ntype = \"web\"\nurl = \"https://old/\"\n").unwrap();
    let mut f = serde_json::Map::new();
    f.insert("url".into(), json!("https://new/"));
    let pane = pane_ids(&state(&c).tabs[0].layout)[0];
    ok(&mut c, Command::PaneUpdate { pane, fields: f });
    ok(&mut c, Command::BookmarkOverwrite { tab: Some(TabRef::Id { id: tab }) });
    let bm = read(&p);
    assert_eq!(
        (bm.title.as_str(), bm.paths.get("save"), bm.rest.get("memo").and_then(|v| v.as_str())),
        ("下書き", Some("/out"), Some("残す"))
    );
    assert!(std::fs::read_to_string(&p).unwrap().contains("https://new/"));
    assert_eq!(c.handle(Command::BookmarkOverwrite { tab: Some(TabRef::Num { num: 9 }) }).reply.unwrap_err().0, ErrorCode::NotFound);

    // 名前の変更: ファイル名と題名が変わり、開いているタブも追う
    let p3 = ok(&mut c, Command::BookmarkRename { path: p.clone(), title: "本文".into() }).as_str().unwrap().to_string();
    assert_eq!(p3, d("本文.prelude"));
    assert!(!Path::new(&p).exists());
    assert_eq!(read(&p3).title, "本文");
    assert_eq!((state(&c).tabs[0].bookmark.as_deref(), state(&c).tabs[0].title.as_str()), (Some(p3.as_str()), "本文"));
    let p2s = p2.as_str().unwrap().to_string();
    assert_eq!(c.handle(Command::BookmarkRename { path: p2s.clone(), title: "本文".into() }).reply.unwrap_err().0, ErrorCode::Conflict);

    // フォルダを作って移す・フォルダの名前を変える(中のブックマークを開いたタブも追う)
    let f1 = ok(&mut c, Command::FolderCreate { name: "執筆".into() }).as_str().unwrap().to_string();
    assert_eq!(c.handle(Command::FolderCreate { name: "執筆".into() }).reply.unwrap_err().0, ErrorCode::Conflict);
    let p4 = ok(&mut c, Command::BookmarkMove { path: p3.clone(), folder: f1.clone() }).as_str().unwrap().to_string();
    assert_eq!(p4, d("執筆/本文.prelude"));
    assert_eq!(state(&c).tabs[0].bookmark.as_deref(), Some(p4.as_str()));
    let f2 = ok(&mut c, Command::FolderRename { path: f1.clone(), name: "原稿".into() }).as_str().unwrap().to_string();
    assert_eq!(state(&c).tabs[0].bookmark.as_deref(), Some(d("原稿/本文.prelude").as_str()));
    assert_eq!(state(&c).tabs[0].title, "本文");

    // フォルダを消す: 中身は直下へ(同じ名前は「(2)」にして、題名も合わせる)
    std::fs::write(
        dir.join("本文.prelude"),
        "version = 2\ntitle = \"本文\"\n[layout]\nk = \"leaf\"\ntype = \"web\"\nurl = \"https://x/\"\n",
    )
    .unwrap();
    ok(&mut c, Command::FolderRemove { path: f2.clone() });
    assert!(!Path::new(&f2).exists());
    assert_eq!(state(&c).tabs[0].bookmark.as_deref(), Some(d("本文 (2).prelude").as_str()));
    assert_eq!(state(&c).tabs[0].title, "本文 (2)");

    // ゴミ箱: ファイルはゴミ箱へ、タブは残って結び付きだけが外れる
    let p5 = d("本文 (2).prelude");
    ok(&mut c, Command::BookmarkTrash { path: p5.clone() });
    assert!(!Path::new(&p5).exists());
    assert!(root.join("Trash/files/本文 (2).prelude").exists());
    assert_eq!((state(&c).tabs[0].id, state(&c).tabs[0].bookmark.clone()), (tab, None));

    // スペースの外・隠しフォルダ・ブックマークでないものは断る
    let outside = root.join("外.prelude");
    std::fs::write(&outside, "x").unwrap();
    for bad in [outside.to_string_lossy().into_owned(), "relative.prelude".into(), d("a.txt")] {
        assert!(c.handle(Command::BookmarkTrash { path: bad.clone() }).reply.is_err(), "{bad}");
    }
    assert_eq!(
        c.handle(Command::BookmarkMove { path: p2s.clone(), folder: root.to_string_lossy().into_owned() }).reply.unwrap_err().0,
        ErrorCode::BadRequest
    );
    assert!(Path::new(&p2s).exists());
}

#[test]
fn ブックマークの並び_番号_種類_フォルダの一覧() {
    let root = spaces("bmorder");
    let dir = root.join(SPACE_DEFAULT);
    std::fs::create_dir_all(dir.join("執筆")).unwrap();
    std::fs::create_dir_all(dir.join("空")).unwrap();
    std::fs::write(dir.join("space.toml"), "[paths]\nfiles = \"/data\"\n").unwrap();
    let leaf = |t: &str| format!("version = 2\n[layout]\nk = \"leaf\"\ntype = \"{t}\"\n");
    std::fs::write(dir.join("a.prelude"), leaf("web")).unwrap();
    std::fs::write(dir.join("b.prelude"), leaf("field")).unwrap();
    std::fs::write(
        dir.join("c.prelude"),
        "version = 2\n[layout]\nk = \"split\"\n[layout.a]\nk = \"leaf\"\ntype = \"web\"\n[layout.b]\nk = \"leaf\"\ntype = \"app\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("執筆/図.prelude"), leaf("app")).unwrap();
    let mut c = Core::new(HOME, &root, None);
    let d = |name: &str| dir.join(name).to_string_lossy().into_owned();
    let list = |c: &mut Core| -> Vec<(String, u32, String)> {
        let v: Vec<prelude_proto::BookmarkInfo> = serde_json::from_value(ok(c, Command::BookmarkList {})).unwrap();
        v.into_iter().map(|b| (if b.folder.is_empty() { b.title } else { format!("{}/{}", b.folder, b.title) }, b.num, b.kind)).collect()
    };
    // 直下 → フォルダの名前の順、その中は題名の順。番号は並びの順。種類は 1 つのペインならその種類(field は files)、分割は layout
    assert_eq!(
        list(&mut c),
        vec![
            ("a".into(), 1, "web".into()),
            ("b".into(), 2, "files".into()),
            ("c".into(), 3, "layout".into()),
            ("執筆/図".into(), 4, "app".into())
        ]
    );
    // 空のフォルダも一覧に出る
    let folders: Vec<prelude_proto::FolderInfo> = serde_json::from_value(ok(&mut c, Command::FolderList {})).unwrap();
    assert_eq!(folders.iter().map(|f| f.title.as_str()).collect::<Vec<_>>(), vec!["執筆", "空"]);
    assert_eq!(folders[0].path, d("執筆"));

    // 並べ替え: c を a の前へ、a を最後へ
    let o = c.handle(Command::BookmarkReorder { path: d("c.prelude"), before: d("a.prelude") });
    assert!(o.changed && o.reply.is_ok());
    ok(&mut c, Command::BookmarkReorder { path: d("a.prelude"), before: String::new() });
    let names = |c: &mut Core| list(c).into_iter().map(|x| x.0).collect::<Vec<_>>();
    assert_eq!(names(&mut c), vec!["c", "b", "a", "執筆/図"]);
    // space.toml のほかの項目は残る
    let conf = std::fs::read_to_string(dir.join("space.toml")).unwrap();
    assert!(conf.contains("files = \"/data\"") && conf.contains("order"), "{conf}");
    // 違うフォルダのものの前には置けない
    assert_eq!(
        c.handle(Command::BookmarkReorder { path: d("a.prelude"), before: d("執筆/図.prelude") }).reply.unwrap_err().0,
        ErrorCode::BadRequest
    );
    // 名前を変えても、並びの位置は変わらない
    ok(&mut c, Command::BookmarkRename { path: d("b.prelude"), title: "z".into() });
    assert_eq!(names(&mut c), vec!["c", "z", "a", "執筆/図"]);
    // ファイルを書き換えたら種類も読み直す
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(dir.join("a.prelude"), leaf("app")).unwrap();
    assert_eq!(list(&mut c)[2].2, "app");
}

#[test]
fn 消音とアイコンは状態に出すが書き出さない() {
    let root = spaces("live");
    std::fs::create_dir_all(root.join(SPACE_DEFAULT)).unwrap();
    let icon = root.join("kate.svg");
    std::fs::write(&icon, "<svg/>").unwrap();
    let mut c = Core::new(HOME, &root, None);
    fn apps() -> Vec<prelude_core::apps::App> {
        let icon = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("state/live/kate.svg");
        vec![prelude_core::apps::App {
            name: "Kate".into(),
            exec: "kate".into(),
            cls: "kate".into(),
            id: "org.kde.kate".into(),
            icon: icon.to_string_lossy().into(),
        }]
    }
    c.set_apps(fixed(apps()));
    ok(&mut c, Command::TabOpen { pane: web("https://a/"), index: None });
    let w = pane_ids(&state(&c).tabs[0].layout)[0];
    let app: PaneSpec = serde_json::from_value(json!({ "type": "app", "cmd": "kate" })).unwrap();
    ok(&mut c, Command::TabOpen { pane: app, index: Some(9) });
    let a = pane_ids(&state(&c).tabs[1].layout)[0];
    // アプリのペインには .desktop のアイコン
    assert_eq!(state(&c).tabs[1].layout["icon"], json!(icon.to_string_lossy()));
    // 消音: 状態と Web のペインのつなぎに出るが、書き出す状態は変わらない。知らせは配る
    let before = c.saved();
    let o = c.handle(Command::WebMute { pane: w, on: true });
    assert!(o.changed && o.reply.is_ok());
    assert_eq!(c.saved(), before);
    assert_eq!(state(&c).tabs[0].layout["muted"], json!(true));
    assert!(c.web_wants().0.iter().any(|x| x.pane == w && x.muted));
    assert!(!c.handle(Command::WebMute { pane: w, on: true }).changed); // 同じなら知らせない
    assert_eq!(c.handle(Command::WebMute { pane: a, on: true }).reply.unwrap_err().0, ErrorCode::BadRequest);
    ok(&mut c, Command::WebMute { pane: w, on: false });
    assert!(state(&c).tabs[0].layout.get("muted").is_none());
    // ファビコン
    assert!(c.note_icon(w, "/cache/a.png".into()));
    assert!(!c.note_icon(w, "/cache/a.png".into()));
    assert!(!c.note_icon(999, "/x.png".into()));
    assert_eq!(state(&c).tabs[0].layout["icon"], json!("/cache/a.png"));
}

#[test]
fn ブックマークを書く_リンクの下のスペース_外の上書き_隠しファイル() {
    let real = spaces("bmlink-real");
    let dir = real.join(SPACE_DEFAULT);
    std::fs::create_dir_all(dir.join("執筆")).unwrap();
    std::fs::write(dir.join("a.prelude"), "version = 2\n[layout]\nk = \"leaf\"\ntype = \"web\"\nurl = \"https://a/\"\n").unwrap();
    // スペースのフォルダをシンボリックリンクで指す
    let link = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("state").join("bmlink");
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let mut c = Core::new(HOME, &link, None);
    let in_link = link.join(SPACE_DEFAULT).join("a.prelude").to_string_lossy().into_owned();
    ok(&mut c, Command::BookmarkOpen { path: in_link.clone(), allow: vec![] });
    ok(&mut c, Command::BookmarkReorder { path: in_link.clone(), before: String::new() });
    // リンクをたどった形(実体のパス)で頼まれても、タブ・並びが持つ形にそろえて付け替える
    let by_real = dir.join("a.prelude").to_string_lossy().into_owned();
    let new = ok(&mut c, Command::BookmarkRename { path: by_real, title: "b".into() }).as_str().unwrap().to_string();
    assert_eq!(new, link.join(SPACE_DEFAULT).join("b.prelude").to_string_lossy());
    assert_eq!(state(&c).tabs[0].bookmark.as_deref(), Some(new.as_str()));
    let conf = std::fs::read_to_string(dir.join("space.toml")).unwrap();
    assert!(conf.contains("\"b.prelude\"") && !conf.contains("\"\""), "{conf}");

    // スペースの外の .prelude を開いたタブは、上書きしない
    let outside = real.join("外.prelude");
    std::fs::write(&outside, "version = 2\n[layout]\nk = \"leaf\"\ntype = \"web\"\nurl = \"https://o/\"\n").unwrap();
    ok(&mut c, Command::BookmarkOpen { path: outside.to_string_lossy().into_owned(), allow: vec![] });
    assert_eq!(c.handle(Command::BookmarkOverwrite { tab: None }).reply.unwrap_err().0, ErrorCode::BadRequest);

    // フォルダを消す: ファイラの表示の設定は一緒に消し、ほかの隠しファイルがあれば何も動かさずに断る
    let f = link.join(SPACE_DEFAULT).join("執筆").to_string_lossy().into_owned();
    std::fs::write(dir.join("執筆/図.prelude"), "version = 2\n[layout]\nk = \"leaf\"\ntype = \"web\"\n").unwrap();
    std::fs::write(dir.join("執筆/.secret"), "x").unwrap();
    assert_eq!(c.handle(Command::FolderRemove { path: f.clone() }).reply.unwrap_err().0, ErrorCode::Conflict);
    assert!(dir.join("執筆/図.prelude").exists());
    std::fs::remove_file(dir.join("執筆/.secret")).unwrap();
    std::fs::write(dir.join("執筆/.directory"), "x").unwrap();
    ok(&mut c, Command::FolderRemove { path: f });
    assert!(dir.join("図.prelude").exists() && !dir.join(".directory").exists() && !dir.join("執筆").exists());
}

#[test]
fn ブックマークのフォルダを外から変えたら知らせる() {
    let root = spaces("bmpoll");
    let dir = root.join(SPACE_DEFAULT);
    std::fs::create_dir_all(dir.join("執筆")).unwrap();
    let mut c = Core::new(HOME, &root, None);
    let leaf = "version = 2\n[layout]\nk = \"leaf\"\ntype = \"web\"\n";
    // 最初は印を取るだけ
    assert!(!c.poll_bookmarks());
    assert!(!c.poll_bookmarks());
    let rev = state(&c).bookmarks_rev;
    // 外から足す・1 段下に足す・書き換える・消すと知らせる
    std::fs::write(dir.join("a.prelude"), leaf).unwrap();
    assert!(c.poll_bookmarks());
    assert!(state(&c).bookmarks_rev > rev);
    assert!(!c.poll_bookmarks());
    std::fs::write(dir.join("執筆/b.prelude"), leaf).unwrap();
    assert!(c.poll_bookmarks());
    std::fs::write(dir.join("a.prelude"), format!("{leaf}url = \"https://x/\"\n")).unwrap();
    assert!(c.poll_bookmarks());
    std::fs::remove_file(dir.join("執筆/b.prelude")).unwrap();
    assert!(c.poll_bookmarks());
    // 関係ないもの(隠しファイル・ほかの種類のファイル)では知らせない
    std::fs::write(dir.join(".hidden"), "x").unwrap();
    std::fs::write(dir.join("memo.txt"), "x").unwrap();
    assert!(!c.poll_bookmarks());
    // コアが自分で書いた分は、二重に知らせない
    ok(&mut c, Command::FolderCreate { name: "新".into() });
    ok(&mut c, Command::TabOpen { pane: web("https://a/"), index: None });
    ok(&mut c, Command::BookmarkSave { tab: None, title: "保存".into(), folder: String::new() });
    assert!(!c.poll_bookmarks());
}

#[test]
fn 設定を当てはめる_閉じたタブの数_ダウンロード() {
    use prelude_format::settings::Config;
    let mut c = Core::new(HOME, spaces("config"), None);
    let rev = state(&c).config_rev;
    for i in 0..5 {
        ok(&mut c, Command::TabOpen { pane: web(&format!("https://{i}/")), index: None });
    }
    for _ in 0..5 {
        ok(&mut c, Command::TabClose { tab: None });
    }
    assert_eq!(state(&c).closed, 5);
    // 減らしたら古いものから捨てる。当てはめたら知らせのために config_rev が上がる
    c.apply_config(&Config::from_text("[tabs]\nclosed_max = 2\ndownload_dir = \"~/DL\"\n", &|_| None));
    assert_eq!(state(&c).closed, 2);
    assert!(state(&c).config_rev > rev);
    let reopened = ok(&mut c, Command::TabReopen {}).as_u64().unwrap();
    assert!(state(&c).tabs.iter().any(|t| t.id == reopened));
    // 0 なら覚えない
    c.apply_config(&Config::from_text("[tabs]\nclosed_max = 0\n", &|_| None));
    ok(&mut c, Command::TabClose { tab: None });
    assert_eq!(state(&c).closed, 0);
    // ダウンロードの保存先(設定の download_dir。~ はホーム)
    c.apply_config(&Config::from_text("download_dir = \"~/DL\"\n", &|_| None));
    ok(&mut c, Command::TabOpen { pane: web("https://x/"), index: None });
    assert_eq!(c.web_wants().1, format!("{HOME}/DL"));
}

#[test]
fn ダイアログの最初のフォルダは書いてあるときだけ() {
    let root = spaces("dialog");
    let dir = root.join(SPACE_DEFAULT);
    std::fs::create_dir_all(&dir).unwrap();
    let mut c = Core::new(HOME, &root, None);
    // どこにも書いていなければ None(アプリが頼んだフォルダが使われる)
    assert_eq!((c.dialog_folder(false), c.dialog_folder(true)), (None, None));
    // files だけ書けば、開く・保存のどちらもそこ。save を書けば保存だけそこ
    std::fs::write(dir.join("space.toml"), "[paths]\nfiles = \"/data\"\nsave = \"/data/out\"\n").unwrap();
    c = Core::new(HOME, &root, None);
    assert_eq!(c.dialog_folder(false).as_deref(), Some("/data"));
    assert_eq!(c.dialog_folder(true).as_deref(), Some("/data/out"));
    // ポータル(別のプロセス)はソケットの dialog.folder で聞く
    assert_eq!(ok(&mut c, Command::DialogFolder { save: true }), json!("/data/out"));
    std::fs::remove_file(dir.join("space.toml")).unwrap();
    c = Core::new(HOME, &root, None);
    assert_eq!(ok(&mut c, Command::DialogFolder { save: false }), Value::Null);
}

// ------------------------------------------------------------------ コンポジタの並びを受け取る

mod compositor_layout {
    use super::*;
    use prelude_core::compositor::Tree;
    use prelude_format::Dir as FDir;

    fn row(a: Tree, b: Tree, ratio: f64) -> Tree {
        Tree::Split { dir: FDir::Row, ratio, a: Box::new(a), b: Box::new(b) }
    }
    use Tree::Pane as P;

    fn app(cmd: &str) -> PaneSpec {
        serde_json::from_value(json!({ "type": "app", "cmd": cmd })).unwrap()
    }
    fn open(c: &mut Core, cmd: &str) -> u64 {
        c.handle(Command::TabOpen { pane: app(cmd), index: None }).reply.unwrap();
        let s = c.state();
        s.tabs.iter().find(|t| Some(t.id) == s.active).unwrap().active_pane
    }
    fn split(c: &mut Core, pane: u64, cmd: &str) -> u64 {
        c.handle(Command::PaneSplit { pane, dir: Dir::Row, new: app(cmd) }).reply.unwrap().as_u64().unwrap()
    }
    fn core(name: &str) -> Core {
        Core::new(HOME, spaces(&format!("compositor-{name}")), None)
    }
    fn tree_of(c: &Core, num: u32) -> Tree {
        c.scene().tabs.iter().find(|t| t.num == num).unwrap().tree.clone()
    }

    #[test]
    fn 手で動かした並びと比率を受け取る() {
        let mut c = core("moved");
        let a = open(&mut c, "a");
        let b = split(&mut c, a, "b");
        let moved = row(P(b), P(a), 0.3);
        assert!(c.note_layout(&[(1, Some(moved.clone()))]));
        assert_eq!(tree_of(&c, 1), moved);
        // 同じなら変わらない
        assert!(!c.note_layout(&[(1, Some(moved))]));
        // 残す形にも入る
        let s = serde_json::to_string(&c.saved()).unwrap();
        assert!(s.contains("0.3"), "{s}");
    }

    #[test]
    fn アプリを分割して開く() {
        let mut c = core("item-split");
        c.set_apps(fixed({
            vec![prelude_core::apps::App {
                name: "Kate".into(),
                exec: "kate".into(),
                cls: "kate".into(),
                id: "org.kde.kate".into(),
                icon: String::new(),
            }]
        }));
        let a = open(&mut c, "a");
        let at = prelude_proto::SplitAt { pane: a, dir: Dir::Col };
        let new =
            c.handle(Command::ItemOpen { key: "app:org.kde.kate".into(), allow: vec![], split: Some(at) }).reply.unwrap().as_u64().unwrap();
        let s = c.state();
        assert_eq!(s.tabs.len(), 1);
        assert_eq!(s.tabs[0].active_pane, new);
        assert_eq!(s.tabs[0].layout["dir"], "col");
        assert_eq!(s.tabs[0].layout["b"]["cmd"], "kate");
        // 無いペインなら失敗
        let at = prelude_proto::SplitAt { pane: 999_999, dir: Dir::Row };
        assert!(c.handle(Command::ItemOpen { key: "app:org.kde.kate".into(), allow: vec![], split: Some(at) }).reply.is_err());
    }

    #[test]
    fn 起動のときは_cli_のタブから始める() {
        let mut c = core("cli");
        c.set_recipes(prelude_core::recipes::load(std::path::Path::new("/nonexistent")));
        let a = open(&mut c, "a");
        c.show_cli();
        let s = c.state();
        assert_eq!(s.tabs.len(), 2);
        let cli = s.tabs.iter().find(|t| t.id == s.active.unwrap()).unwrap();
        assert_eq!(cli.layout["type"], "cli");
        // 既にあれば、作らずにそれを今のタブにする
        c.handle(Command::PaneFocus { pane: a }).reply.unwrap();
        let id = s.tabs.iter().find(|t| t.layout["type"] == "app").unwrap().id;
        c.handle(Command::TabActivate { tab: prelude_proto::TabRef::Id { id } }).reply.unwrap();
        c.show_cli();
        let s = c.state();
        assert_eq!(s.tabs.len(), 2);
        assert_eq!(s.tabs.iter().find(|t| t.id == s.active.unwrap()).unwrap().layout["type"], "cli");
    }

    #[test]
    fn 窓の無いペインはタブに残す() {
        let mut c = core("rest");
        let a = open(&mut c, "a");
        let b = split(&mut c, a, "b");
        let d = split(&mut c, b, "d");
        c.note_exited(d, false);
        assert!(c.note_layout(&[(1, Some(row(P(b), P(a), 0.4)))]));
        let want = row(row(P(b), P(a), 0.4), P(d), 0.7);
        assert_eq!(tree_of(&c, 1), want);
        assert!(c.state().tabs[0].layout.to_string().contains("stopped"));
    }

    #[test]
    fn 窓を閉じたペインはペインごと閉じる() {
        let mut c = core("closed");
        let a = open(&mut c, "a");
        let b = split(&mut c, a, "b");
        // 分割のうち 1 つの窓を閉じた: そのペインだけ無くなり、残りのペインが今のペインになる
        assert!(c.note_exited(b, true));
        assert_eq!(tree_of(&c, 1), P(a));
        assert!(!c.state().tabs[0].layout.to_string().contains("stopped"));
        assert_eq!(c.state().tabs[0].active_pane, a);
        // 最後のペインの窓を閉じた: タブごと無くなる
        assert!(c.note_exited(a, true));
        assert!(c.state().tabs.iter().all(|t| t.num != 1));
        assert!(!c.note_exited(a, true));
    }

    #[test]
    fn 別のタブへ移したペイン() {
        let mut c = core("tabs");
        let a = open(&mut c, "a");
        let b = split(&mut c, a, "b");
        c.handle(Command::PaneFocus { pane: b }).reply.unwrap();
        let x = open(&mut c, "x");
        // b をタブ 2 へ
        assert!(c.note_layout(&[(1, Some(P(a))), (2, Some(row(P(x), P(b), 0.5)))]));
        assert_eq!(tree_of(&c, 1), P(a));
        assert_eq!(tree_of(&c, 2), row(P(x), P(b), 0.5));
        let t1 = c.state().tabs.into_iter().find(|t| t.num == 1).unwrap();
        assert_eq!(t1.active_pane, a);
        // a もタブ 2 へ: タブ 1 は無くなる。今のタブは残る
        assert!(c.note_layout(&[(1, None), (2, Some(row(row(P(x), P(b), 0.5), P(a), 0.6)))]));
        assert_eq!(c.state().tabs.len(), 1);
        // 新しい番号の作業領域へ移したらタブになる
        assert!(c.note_layout(&[(2, Some(row(P(x), P(b), 0.5))), (7, Some(P(a)))]));
        assert_eq!(tree_of(&c, 7), P(a));
    }

    #[test]
    fn 外から開いた窓はペインになる() {
        let mut c = core("foreign");
        c.set_apps(fixed({
            vec![prelude_core::apps::App {
                name: "Okular".into(),
                exec: "okular".into(),
                cls: "".into(),
                id: "org.kde.okular".into(),
                icon: String::new(),
            }]
        }));
        let a = open(&mut c, "a");
        let tab = c.state().active.unwrap();
        // インストール済みのアプリ: 開き直せる
        assert!(c.note_foreign(tab, 500, "org.kde.okular", "x.pdf", true, &row(P(a), P(0), 0.5)));
        let s = c.state();
        let okular = s.tabs[0].active_pane;
        assert_ne!(okular, a);
        assert_eq!(tree_of(&c, 1), row(P(a), P(okular), 0.5));
        let w = c.wants().into_iter().find(|w| w.pane == okular).unwrap();
        assert_eq!((w.window, w.cmd.as_str()), (Some(500), "okular"));
        assert!(serde_json::to_string(&c.saved()).unwrap().contains("okular"));
        // 開き直せない窓: タブにはいるが書かない
        assert!(c.note_foreign(tab, 501, "foo", "題名", false, &row(row(P(a), P(okular), 0.5), P(0), 0.6)));
        let t = &c.state().tabs[0];
        assert_eq!(t.active_pane, okular);
        let foo = c.wants().into_iter().find(|w| w.window == Some(501)).unwrap();
        assert_eq!(foo.cmd, "");
        assert!(t.title.contains("題名"));
        assert!(!serde_json::to_string(&c.saved()).unwrap().contains("題名"));
        // 閉じるとペインごと消える(止まったものにしない)
        assert!(c.note_exited(foo.pane, true));
        assert_eq!(tree_of(&c, 1), row(P(a), P(okular), 0.5));
        assert!(c.note_exited(okular, true));
        assert_eq!(tree_of(&c, 1), P(a));
        // 知らないタブには入れない
        assert!(!c.note_foreign(tab + 999, 502, "foo", "", false, &P(0)));
    }

    #[test]
    fn スペースを出ると開き直せない窓は消える() {
        let mut c = core("space");
        let a = open(&mut c, "a");
        let tab = c.state().active.unwrap();
        c.note_foreign(tab, 500, "foo", "t", false, &row(P(a), P(0), 0.5));
        let space = c.state().space;
        c.handle(Command::SpaceCreate { name: "他".into() }).reply.unwrap();
        c.handle(Command::SpaceSwitch { name: "他".into() }).reply.unwrap();
        c.handle(Command::SpaceSwitch { name: space }).reply.unwrap();
        assert_eq!(tree_of(&c, 1), P(a));
        assert!(c.wants().iter().all(|w| w.window.is_none()));
    }

    #[test]
    fn 開き直せない窓だけのタブは閉じたタブに残さない() {
        let mut c = core("closedforeign");
        let a = open(&mut c, "a");
        let tab = c.state().active.unwrap();
        c.note_foreign(tab, 500, "foo", "t", false, &row(P(a), P(0), 0.5));
        let foo = c.wants().into_iter().find(|w| w.window == Some(500)).unwrap().pane;
        c.handle(Command::PaneClose { pane: a }).reply.unwrap();
        assert_eq!(c.state().closed, 0);
        // 残った開き直せない窓のペインを閉じるとタブも閉じるが、開き直せないので覚えない
        c.handle(Command::PaneClose { pane: foo }).reply.unwrap();
        assert_eq!((c.state().tabs.len(), c.state().closed), (0, 0));
    }
}

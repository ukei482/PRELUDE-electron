//! コアとのやり取り。組み込みの UI と読み込む UI(--load)の両方で使う
use prelude_proto::{Client, Incoming, State};
use serde_json::{Value, json};
use std::time::Duration;

/// サイドバーに出すもの(slint の型にする前の、スレッドをまたげる形)
#[derive(Debug, Clone, Default)]
pub struct Snap {
    pub online: bool,
    pub status: String,
    pub space: String,
    pub spaces: Vec<String>,
    /// (パス, 題名, フォルダ)
    pub bookmarks: Vec<(String, String, String)>,
    pub tabs: Vec<TabRow>,
}

#[derive(Debug, Clone)]
pub struct TabRow {
    pub id: i32,
    pub num: i32,
    pub title: String,
    pub kind: String,
    pub active: bool,
}

/// 今のペインの種類(タブの印に使う)
fn active_kind(layout: &Value, pane: u64) -> String {
    if layout["k"] == "split" {
        let a = active_kind(&layout["a"], pane);
        return if a.is_empty() { active_kind(&layout["b"], pane) } else { a };
    }
    if layout["id"].as_u64() == Some(pane) { layout["type"].as_str().unwrap_or("").to_string() } else { String::new() }
}

fn snap(state: State, query: &mut Client) -> Result<Snap, String> {
    let tabs = state
        .tabs
        .iter()
        .map(|t| TabRow {
            id: t.id as i32,
            num: t.num as i32,
            title: t.title.clone(),
            kind: active_kind(&t.layout, t.active_pane),
            active: state.active == Some(t.id),
        })
        .collect();
    let bookmarks = query.bookmarks()?.into_iter().map(|b| (b.path, b.title, b.folder)).collect();
    Ok(Snap { online: true, status: String::new(), space: state.space, spaces: query.spaces()?, bookmarks, tabs })
}

/// コアを見張る(別のスレッド)。状態が変わるたびに `apply` を UI のスレッドで呼ぶ。つながらなければ 2 秒ごとにつなぎ直す
pub fn watch(apply: impl Fn(Snap) + Send + Sync + Clone + 'static) {
    std::thread::spawn(move || {
        loop {
            let r = (|| -> Result<(), String> {
                let mut events = Client::connect("prelude-slint-proto", true)?;
                let mut query = Client::connect("prelude-slint-proto", false)?;
                loop {
                    let Incoming::Event(e) = events.read()? else { continue };
                    if e.event != "state" {
                        continue;
                    }
                    let state: State = serde_json::from_value(e.data).map_err(|e| e.to_string())?;
                    let s = snap(state, &mut query)?;
                    let apply = apply.clone();
                    slint::invoke_from_event_loop(move || apply(s)).map_err(|e| e.to_string())?;
                }
            })();
            let status = format!("コアにつながっていない({})。2 秒ごとにつなぎ直す", r.err().unwrap_or_default());
            let apply = apply.clone();
            if slint::invoke_from_event_loop(move || apply(Snap { status, ..Snap::default() })).is_err() {
                return; // UI が終わった
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
}

/// コアに頼む(UI を止めないよう別のスレッドで。失敗は表示せずログだけ)
pub fn request(cmd: &'static str, args: Value) {
    std::thread::spawn(move || {
        if let Err(e) = Client::connect("prelude-slint-proto", false).and_then(|mut c| c.call(cmd, args)) {
            eprintln!("{cmd}: {e}");
        }
    });
}

pub fn switch_space(name: String) {
    request("space.switch", json!({ "name": name }));
}
pub fn open_bookmark(path: String) {
    request("bookmark.open", json!({ "path": path }));
}
pub fn activate_tab(id: i32) {
    request("tab.activate", json!({ "tab": { "id": id } }));
}
pub fn close_tab(id: i32) {
    request("tab.close", json!({ "tab": { "id": id } }));
}
pub fn open_launcher() {
    if let Err(e) = std::process::Command::new("prelude-launcher").spawn() {
        eprintln!("prelude-launcher: {e}");
    }
}

//! PRELUDE のランチャー(docs/redesign-rust.md の判断 11・12、移行の段階 4)。
//!
//! - 画面の上の方に layer-shell で浮かび、キーボードを取る。選んだら開いて終わる(1 回ごとに起動する。fuzzel と同じ使い方)
//! - 候補と点数はコアが出す(`item.list`。タブが先、ほかは最近 + よく使う順)。開くのは `item.open`(点数も上がる)
//! - 絞り込みは Electron 版のランチャーと同じ: 名前の始まり → 言葉の始まり → 含む、の順。数字だけならその番号のタブを一番上に
//! - 最後の行は「Web で開く / 検索」。URL らしければその URL、フォルダならフォルダのペイン、それ以外は検索
//! - Ctrl+Enter: Web・フォルダの行を、今のタブの右に分割して開く
//! - `--pane <id> --url <URL>`: その Web のペインの今の URL を入れた状態で開き、Enter で同じペインに開く(判断 12)
//!
//! 使い方: `prelude-launcher [--window] [--pane <id> --url <URL>]`(`--window` は普通の窓で出す。試す用)
//! 検索エンジンは `PRELUDE_SEARCH_ENGINE`(`%s` が言葉。既定は Google。Electron 版の設定の既定と同じ)
use gpui::{
    App, Bounds, Context, Entity, FocusHandle, Focusable, KeyBinding, SharedString, Window, WindowBounds, WindowKind, WindowOptions,
    actions, div, layer_shell::*, prelude::*, px, size,
};
use gpui_platform::application;
use prelude_proto::{Client, Item, State};
use prelude_ui::{TextInput, badge, color, label, row, section_title};
use serde_json::{Value, json};
use std::path::Path;

actions!(launcher, [Up, Down, Confirm, ConfirmSplit, Cancel]);

const WIDTH: f32 = 640.;
const HEIGHT: f32 = 460.;
/// 種類ごとに出す数の上限(タブは全部)
const PER_KIND: usize = 8;

/// 1 行が開くもの
#[derive(Clone, Debug, PartialEq)]
enum Target {
    /// コアの候補(`item.open` の key)
    Item(String),
    /// Web のペイン(新しいタブ、または --pane のペイン)
    Web(String),
    /// フォルダのペイン
    Folder(String),
}

#[derive(Clone, Debug)]
struct Entry {
    group: &'static str,
    label: SharedString,
    detail: SharedString,
    target: Target,
}

/// 打った言葉と名前の合い方。名前の始まり 0、言葉の始まり 1、含む 2、合わない None(Electron 版の matchRank と同じ)
fn match_rank(text: &str, q: &str) -> Option<u8> {
    let t = text.to_lowercase();
    if t.starts_with(q) {
        return Some(0);
    }
    if t.split(|c: char| c.is_whitespace() || "-_.".contains(c)).any(|w| w.starts_with(q)) {
        return Some(1);
    }
    t.contains(q).then_some(2)
}

/// 打った言葉を開く先にする(Electron 版の normalizeInput と同じ)。URL らしければ URL、それ以外は検索
fn normalize_url(s: &str, engine: &str) -> String {
    let s = s.trim();
    let lower = s.to_lowercase();
    if ["http:", "https:", "file:", "about:"].iter().any(|p| lower.starts_with(p)) {
        return s.to_string();
    }
    let host_end = s.find(['/', '?', '#']).unwrap_or(s.len());
    let host = &s[..host_end];
    let local = host.eq_ignore_ascii_case("localhost")
        || host.to_lowercase().starts_with("localhost:")
        || host.split(':').next().is_some_and(|h| {
            h.split('.').count() == 4 && h.split('.').all(|p| !p.is_empty() && p.len() <= 3 && p.bytes().all(|b| b.is_ascii_digit()))
        });
    if local {
        return format!("http://{s}");
    }
    let name = host.split(':').next().unwrap_or("");
    let tld_ok =
        name.rsplit_once('.').is_some_and(|(a, tld)| !a.is_empty() && tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_alphabetic()));
    if !s.contains(char::is_whitespace) && tld_ok {
        return format!("https://{s}");
    }
    engine.replace("%s", &encode(s))
}

/// URL の言葉の部分に入れる形(英数字と -_.~ 以外は %XX)
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

fn group_of(kind: &str) -> &'static str {
    match kind {
        "tab" => "TABS",
        "bookmark" => "BOOKMARKS",
        "app" => "APPS",
        "space" => "SPACES",
        _ => "OTHER",
    }
}

/// 候補を絞って並べる。items はコアの並び(タブが先、ほかは点数順)
fn entries(items: &[Item], query: &str, pane_url: Option<&str>, engine: &str) -> Vec<Entry> {
    let q = query.trim().to_lowercase();
    let mut out: Vec<Entry> = vec![];
    let entry = |i: &Item| Entry {
        group: group_of(&i.kind),
        label: i.label.clone().into(),
        detail: i.detail.clone().into(),
        target: Target::Item(i.key.clone()),
    };
    // --pane: 一番上は「このペインで開く」
    if let Some(cur) = pane_url {
        let url = if q.is_empty() { cur.to_string() } else { normalize_url(query, engine) };
        out.push(Entry { group: "THIS PANE", label: url.clone().into(), detail: "".into(), target: Target::Web(url) });
    }
    if q.is_empty() {
        out.extend(items.iter().filter(|i| i.kind == "tab").map(entry));
        out.extend(items.iter().filter(|i| i.kind != "tab").take(PER_KIND * 2).map(entry));
        return group_sorted(out);
    }
    // 数字だけなら、その番号のタブを一番上に
    if q.bytes().all(|b| b.is_ascii_digit())
        && let Some(t) = items.iter().find(|i| i.kind == "tab" && i.detail == q)
    {
        let mut e = entry(t);
        e.group = "TABS";
        out.push(e);
    }
    for kind in ["tab", "bookmark", "app", "space"] {
        let mut hits: Vec<(u8, usize, &Item)> = items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.kind == kind)
            .filter_map(|(n, i)| match_rank(&format!("{} {}", i.label, i.detail), &q).map(|r| (r, n, i)))
            .collect();
        hits.sort_by_key(|(r, n, _)| (*r, *n));
        let limit = if kind == "tab" { usize::MAX } else { PER_KIND };
        for (_, _, i) in hits.into_iter().take(limit) {
            if !out.iter().any(|e| e.target == Target::Item(i.key.clone())) {
                out.push(entry(i));
            }
        }
    }
    if pane_url.is_none() {
        let t = query.trim();
        let dir = if let Some(rest) = t.strip_prefix("~/") {
            std::env::var("HOME").ok().map(|h| format!("{h}/{rest}"))
        } else {
            Some(t.to_string())
        };
        match dir.filter(|d| d.starts_with('/') && Path::new(d).is_dir()) {
            Some(d) => out.push(Entry { group: "OPEN", label: d.clone().into(), detail: "folder".into(), target: Target::Folder(d) }),
            None => {
                let url = normalize_url(t, engine);
                let what = if url.starts_with(&engine[..engine.find("%s").unwrap_or(engine.len())]) { "search" } else { "web" };
                out.push(Entry { group: "OPEN", label: t.to_string().into(), detail: what.into(), target: Target::Web(url) });
            }
        }
    }
    out
}

/// 空のときの並び: 見出しが混ざらないように、種類ごとにまとめる(種類の中の順はそのまま)
fn group_sorted(v: Vec<Entry>) -> Vec<Entry> {
    let order = ["THIS PANE", "TABS", "BOOKMARKS", "APPS", "SPACES", "OPEN"];
    let mut v: Vec<(usize, usize, Entry)> =
        v.into_iter().enumerate().map(|(n, e)| (order.iter().position(|g| *g == e.group).unwrap_or(9), n, e)).collect();
    v.sort_by_key(|(g, n, _)| (*g, *n));
    v.into_iter().map(|(_, _, e)| e).collect()
}

struct Launcher {
    input: Entity<TextInput>,
    items: Vec<Item>,
    state: Option<State>,
    pane: Option<(u64, String)>,
    engine: String,
    query: String,
    list: Vec<Entry>,
    selected: usize,
    error: Option<String>,
    focus: FocusHandle,
    /// 最後に見たマウスの位置。開いた瞬間にマウスの下にある行を選ばないように、動いたときだけ選ぶ
    mouse: Option<gpui::Point<gpui::Pixels>>,
}

impl Launcher {
    fn refresh(&mut self) {
        self.list = entries(&self.items, &self.query, self.pane.as_ref().map(|(_, u)| u.as_str()), &self.engine);
        self.selected = 0;
    }

    fn move_by(&mut self, d: isize, cx: &mut Context<Self>) {
        if self.list.is_empty() {
            return;
        }
        let n = self.list.len() as isize;
        self.selected = ((self.selected as isize + d).rem_euclid(n)) as usize;
        cx.notify();
    }

    /// 選んだ行を開いて終わる。split: 今のタブの右に分割して開く(Web・フォルダだけ)
    fn open(&mut self, ix: usize, split: bool, cx: &mut Context<Self>) {
        let Some(e) = self.list.get(ix).cloned() else { return };
        let active_pane = self.state.as_ref().and_then(|s| s.tabs.iter().find(|t| Some(t.id) == s.active)).map(|t| t.active_pane);
        let spec = |t: &Target| match t {
            Target::Web(u) => Some(json!({ "type": "web", "url": u })),
            Target::Folder(p) => Some(json!({ "type": "files", "path": p })),
            Target::Item(_) => None,
        };
        let (cmd, args): (&str, Value) = match (&e.target, &self.pane, split.then_some(active_pane).flatten()) {
            (Target::Web(u), Some((pane, _)), _) if e.group == "THIS PANE" => {
                ("pane.update", json!({ "pane": pane, "fields": { "url": u } }))
            }
            (Target::Item(key), _, _) => ("item.open", json!({ "key": key })),
            (t, _, Some(pane)) => ("pane.split", json!({ "pane": pane, "dir": "row", "new": spec(t) })),
            (t, _, None) => ("tab.open", json!({ "pane": spec(t) })),
        };
        match Client::connect("prelude-launcher", false).and_then(|mut c| c.call(cmd, args)) {
            Ok(_) => cx.quit(),
            Err(err) => {
                self.error = Some(format!("{cmd}: {err}"));
                cx.notify();
            }
        }
    }
}

impl Focusable for Launcher {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Launcher {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div().id("list").flex().flex_col().flex_1().min_h(px(0.)).overflow_y_scroll();
        let mut last = "";
        for (ix, e) in self.list.iter().enumerate() {
            if e.group != last {
                last = e.group;
                list = list.child(section_title(e.group));
            }
            let sel = ix == self.selected;
            list = list.child(
                row(("entry", ix), sel)
                    .child(label(e.label.clone()))
                    .child(badge(e.detail.clone()).when(sel, |b| b.text_color(color::bg())))
                    .on_mouse_move(cx.listener(move |this, ev: &gpui::MouseMoveEvent, _, cx| {
                        let moved = this.mouse.is_some_and(|p| p != ev.position);
                        this.mouse = Some(ev.position);
                        if moved && this.selected != ix {
                            this.selected = ix;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| this.open(ix, ev.modifiers().control, cx))),
            );
        }
        if self.list.is_empty() {
            list = list.child(div().px(px(6.)).text_color(color::dim()).child("(nothing)"));
        }
        let mut root = prelude_ui::root()
            .key_context("Launcher")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Up, _, cx| this.move_by(-1, cx)))
            .on_action(cx.listener(|this, _: &Down, _, cx| this.move_by(1, cx)))
            .on_action(cx.listener(|this, _: &Confirm, _, cx| this.open(this.selected, false, cx)))
            .on_action(cx.listener(|this, _: &ConfirmSplit, _, cx| this.open(this.selected, true, cx)))
            .on_action(cx.listener(|_, _: &Cancel, _, cx| cx.quit()))
            .border_1()
            .border_color(color::line())
            .p(px(8.))
            .gap(px(4.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(6.))
                    .border_b_1()
                    .border_color(color::panel2())
                    .child(">")
                    .child(self.input.clone()),
            )
            .child(list);
        if let Some(e) = &self.error {
            root = root.child(div().px(px(6.)).text_size(px(11.)).text_color(color::danger()).child(e.clone()));
        }
        root
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let usage = "usage: prelude-launcher [--window] [--pane ID --url URL]";
    let (mut window_mode, mut pane, mut url) = (false, None::<u64>, None::<String>);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--window" => window_mode = true,
            "--pane" => pane = args.next().and_then(|v| v.parse().ok()),
            "--url" => url = args.next(),
            "-h" | "--help" => {
                println!("{usage}");
                return;
            }
            _ => {
                eprintln!("{usage}");
                std::process::exit(2);
            }
        }
    }
    let pane = pane.map(|p| (p, url.unwrap_or_default()));
    let engine = std::env::var("PRELUDE_SEARCH_ENGINE")
        .ok()
        .filter(|e| e.contains("%s"))
        .unwrap_or_else(|| "https://www.google.com/search?q=%s".into());
    // 候補は開く前に 1 回だけ取る(コアは同じ PC の上なので速い)
    let (items, state, error) =
        match Client::connect("prelude-launcher", false).and_then(|mut c| Ok((c.get::<Vec<Item>>("item.list", json!({}))?, c.state()?))) {
            Ok((i, s)) => (i, Some(s), None),
            Err(e) => (vec![], None, Some(e)),
        };

    application().run(move |cx: &mut App| {
        prelude_ui::init(cx);
        prelude_ui::input::bind_keys(cx);
        let c = Some("Launcher");
        cx.bind_keys([
            KeyBinding::new("up", Up, c),
            KeyBinding::new("down", Down, c),
            KeyBinding::new("ctrl-p", Up, c),
            KeyBinding::new("ctrl-n", Down, c),
            KeyBinding::new("enter", Confirm, c),
            KeyBinding::new("ctrl-enter", ConfirmSplit, c),
            KeyBinding::new("escape", Cancel, c),
        ]);
        let kind = if window_mode {
            WindowKind::Normal
        } else {
            WindowKind::LayerShell(LayerShellOptions {
                namespace: "prelude-launcher".into(),
                layer: Layer::Overlay,
                anchor: Anchor::TOP,
                margin: Some((px(120.), px(0.), px(0.), px(0.))),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                ..Default::default()
            })
        };
        let opened = cx.open_window(
            WindowOptions {
                titlebar: None,
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(WIDTH), px(HEIGHT)), cx))),
                app_id: Some("prelude-launcher".into()),
                kind,
                ..Default::default()
            },
            |_, cx| {
                let first = pane.as_ref().map(|(_, u)| u.clone()).unwrap_or_default();
                let input = cx.new(|cx| TextInput::new(cx, "Open a tab, bookmark, app, space, URL or search", first));
                cx.new(|cx| {
                    cx.observe(&input, |this: &mut Launcher, input, cx| {
                        let q = input.read(cx).text().to_string();
                        if q != this.query {
                            this.query = q;
                            this.refresh();
                            cx.notify();
                        }
                    })
                    .detach();
                    let mut l = Launcher {
                        input,
                        items,
                        state,
                        pane,
                        engine,
                        query: String::new(),
                        list: vec![],
                        selected: 0,
                        error,
                        focus: cx.focus_handle(),
                        mouse: None,
                    };
                    l.query = l.input.read(cx).text().to_string();
                    l.refresh();
                    l
                })
            },
        );
        let window = match opened {
            Ok(w) => w,
            Err(e) => {
                eprintln!("prelude-launcher: cannot open the window: {e:#}");
                cx.quit();
                return;
            }
        };
        window
            .update(cx, |l, window, cx| {
                let input = l.input.clone();
                input.update(cx, |i, cx| i.select_everything(cx)); // --url のとき、打てばすぐ置き換わる
                window.focus(&input.focus_handle(cx), cx);
            })
            .ok();
        // ほかの窓に移ったら閉じる(ランチャーは 1 回ごと)
        window
            .update(cx, |_, window, cx| {
                cx.observe_window_activation(window, |_, window, cx| {
                    if !window.is_window_active() {
                        cx.quit();
                    }
                })
                .detach();
            })
            .ok();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: &str = "https://www.google.com/search?q=%s";

    fn item(kind: &str, label: &str, detail: &str) -> Item {
        Item { key: format!("{kind}:{label}"), kind: kind.into(), label: label.into(), detail: detail.into(), score: 0.0 }
    }

    #[test]
    fn 打った言葉を_url_か検索にする() {
        assert_eq!(normalize_url("example.com", G), "https://example.com");
        assert_eq!(normalize_url("example.com/a?b", G), "https://example.com/a?b");
        assert_eq!(normalize_url("https://x/y", G), "https://x/y");
        assert_eq!(normalize_url("localhost:3000/x", G), "http://localhost:3000/x");
        assert_eq!(normalize_url("192.168.0.1", G), "http://192.168.0.1");
        assert_eq!(normalize_url("rust 言語", G), "https://www.google.com/search?q=rust%20%E8%A8%80%E8%AA%9E");
        assert_eq!(normalize_url("v1.2", G), "https://www.google.com/search?q=v1.2"); // 数字の TLD は URL にしない
    }

    #[test]
    fn 合い方の順() {
        assert_eq!(match_rank("Firefox Web Browser", "fire"), Some(0));
        assert_eq!(match_rank("Firefox Web Browser", "web"), Some(1));
        assert_eq!(match_rank("my-notes.txt", "notes"), Some(1));
        assert_eq!(match_rank("Firefox", "ref"), Some(2));
        assert_eq!(match_rank("Firefox", "zz"), None);
    }

    #[test]
    fn 絞り込みと並び() {
        let items = vec![
            item("tab", "Docs", "1"),
            item("tab", "論文", "2"),
            item("bookmark", "論文メモ", "研究"),
            item("app", "Kate", ""),
            item("app", "Konsole", ""),
            item("space", "Work", ""),
        ];
        // 空なら全部を種類ごとに、最後の「開く」は無し
        let all = entries(&items, "", None, G);
        assert_eq!(all.iter().map(|e| e.group).collect::<Vec<_>>(), ["TABS", "TABS", "BOOKMARKS", "APPS", "APPS", "SPACES"]);
        // 種類の順に、合うものだけ。最後は検索
        let e = entries(&items, "論文", None, G);
        assert_eq!(e.iter().map(|e| e.label.to_string()).collect::<Vec<_>>(), ["論文", "論文メモ", "論文"]);
        assert_eq!(e[2].group, "OPEN");
        assert!(matches!(&e[2].target, Target::Web(u) if u.contains("search?q=")));
        // フォルダのとき(「研究」で絞るとブックマークのフォルダにも合う)
        assert_eq!(entries(&items, "研究", None, G)[0].label.as_ref(), "論文メモ");
        // 数字だけなら、その番号のタブが一番上
        assert_eq!(entries(&items, "2", None, G)[0].target, Target::Item("tab:論文".into()));
        // 名前の始まりが先: k → Kate・Konsole(言葉の始まり)の順は元の並び
        let k: Vec<_> = entries(&items, "k", None, G).into_iter().filter(|e| e.group == "APPS").map(|e| e.label.to_string()).collect();
        assert_eq!(k, ["Kate", "Konsole"]);
        // URL らしい言葉は Web で開く
        assert_eq!(entries(&items, "example.com", None, G).last().unwrap().target, Target::Web("https://example.com".into()));
        // フォルダ
        assert_eq!(entries(&items, "/tmp", None, G).last().unwrap().target, Target::Folder("/tmp".into()));
    }

    #[test]
    fn このペインで開く() {
        let items = vec![item("tab", "Docs", "1")];
        let e = entries(&items, "", Some("https://a/"), G);
        assert_eq!((e[0].group, e[0].target.clone()), ("THIS PANE", Target::Web("https://a/".into())));
        let e = entries(&items, "b.example", Some("https://a/"), G);
        assert_eq!(e[0].target, Target::Web("https://b.example".into()));
        assert!(e.iter().all(|e| e.group != "OPEN")); // 新しいタブで開く行は出さない
    }
}

//! PRELUDE のサイドバー(docs/redesign-rust.md の判断 10・11、移行の段階 4)。
//!
//! - 画面の左端に layer-shell で貼り付き、場所を予約する(ほかの窓はその右に並ぶ)
//! - コアのソケット(prelude-proto)だけでつながる。サイドバーにだけ許された操作は無い(判断 10)。`prelude` コマンドでできることしかしない
//! - 出すもの: スペース(押すと切り替え)・ブックマーク(押すと開く。フォルダは 1 段)・タブ(押すと切り替え、× で閉じる)
//! - TABS の見出しの + で新しいタブ。Web は URL を打つのでランチャーを出し、ほかは種類を選べばそのまま開く
//! - コアが落ちている・まだ動いていないときは、2 秒ごとにつなぎ直す
//!
//! 使い方: `prelude-sidebar [--window] [--width <px>]`(`--window` は普通の窓で出す。試す用)
use futures::StreamExt;
use futures::channel::mpsc::{UnboundedSender, unbounded};
use gpui::{
    App, Bounds, Context, SharedString, Window, WindowBounds, WindowKind, WindowOptions, div, layer_shell::*, prelude::*, px, relative,
    size,
};
use gpui_platform::application;
use prelude_proto::{BookmarkInfo, Client, Incoming, State};
use prelude_ui::{badge, color, label, num, row, rule, section_title};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::time::Duration;

/// TABS の + で選べる新しいタブ(出す名前, ペインの種類)。web だけはランチャーを出す
const NEW_TAB_KINDS: [(&str, &str); 5] =
    [("Web...", "web"), ("Folder", "files"), ("Field", "field"), ("PRELUDE CLI", "cli"), ("Settings", "settings")];

/// コアから受け取った今の様子
#[derive(Clone)]
struct Snapshot {
    state: State,
    bookmarks: Vec<BookmarkInfo>,
    spaces: Vec<String>,
}

enum Msg {
    Snapshot(Box<Snapshot>),
    /// つながらない・切れた
    Offline(String),
    /// コマンドが失敗した
    Failed(String),
}

/// コアを見張る(別のスレッド)。状態が変わるたびに、ブックマークとスペースの一覧も取り直して送る
fn watch(tx: UnboundedSender<Msg>) {
    loop {
        let r = (|| -> Result<(), String> {
            let mut events = Client::connect("prelude-sidebar", true)?;
            let mut query = Client::connect("prelude-sidebar", false)?;
            loop {
                let Incoming::Event(e) = events.read()? else { continue };
                if e.event != "state" {
                    continue;
                }
                let state: State = serde_json::from_value(e.data).map_err(|e| e.to_string())?;
                let snap = Snapshot { state, bookmarks: query.bookmarks()?, spaces: query.spaces()? };
                if tx.unbounded_send(Msg::Snapshot(Box::new(snap))).is_err() {
                    return Ok(()); // サイドバーが終わった
                }
            }
        })();
        if tx.is_closed() {
            return;
        }
        let why = r.err().unwrap_or_default();
        if tx.unbounded_send(Msg::Offline(why)).is_err() {
            return;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

struct Sidebar {
    snap: Option<Snapshot>,
    offline: Option<String>,
    error: Option<String>,
    space_menu: bool,
    /// 新しいタブの種類の一覧(TABS の見出しの +)を出しているか
    new_menu: bool,
    /// 閉じているブックマークのフォルダ
    folded: HashSet<String>,
    tx: UnboundedSender<Msg>,
}

impl Sidebar {
    fn apply(&mut self, m: Msg) {
        match m {
            Msg::Snapshot(s) => {
                self.snap = Some(*s);
                self.offline = None;
                self.error = None;
            }
            Msg::Offline(why) => {
                self.snap = None;
                self.offline = Some(why);
            }
            Msg::Failed(e) => self.error = Some(e),
        }
    }

    /// コマンドを送る(別のスレッド)。結果は状態のイベントで返ってくるので、失敗だけを知らせる
    fn send(&self, cmd: &'static str, args: Value) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let r = Client::connect("prelude-sidebar", false).and_then(|mut c| c.call(cmd, args));
            if let Err(e) = r {
                let _ = tx.unbounded_send(Msg::Failed(format!("{cmd}: {e}")));
            }
        });
    }

    fn render_space(&self, snap: &Snapshot, cx: &mut Context<Self>) -> impl IntoElement {
        let mut col = div().flex().flex_col().child(
            row("space", false)
                .child(label(SharedString::from(snap.state.space.to_uppercase())))
                .child(badge(if self.space_menu { "▴" } else { "▾" }))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.space_menu = !this.space_menu;
                    cx.notify();
                })),
        );
        if self.space_menu {
            for (i, name) in snap.spaces.iter().enumerate() {
                let cur = *name == snap.state.space;
                let n = name.clone();
                col = col.child(row(("space-item", i), cur).pl(px(18.)).child(label(name.clone())).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.space_menu = false;
                        if !cur {
                            this.send("space.switch", json!({ "name": n }));
                        }
                        cx.notify();
                    },
                )));
            }
        }
        col
    }

    /// TABS の見出し。右の + で新しいタブの種類の一覧を開く
    fn render_tabs_title(&self, snap: &Snapshot, cx: &mut Context<Self>) -> impl IntoElement {
        let plus = div()
            .id("new-tab")
            .flex_none()
            .px(px(6.))
            .cursor_pointer()
            .text_size(px(14.))
            .when(self.new_menu, |d| d.text_color(color::text()))
            .hover(|s| s.text_color(color::text()).bg(color::panel2()))
            .child("+")
            .on_click(cx.listener(|this, _, _, cx| {
                this.new_menu = !this.new_menu;
                cx.notify();
            }));
        section_title(format!("TABS {}", snap.state.tabs.len())).justify_between().pr(px(0.)).child(plus)
    }

    /// 新しいタブの種類の一覧。選ぶと一番上に新しいタブを開く
    fn render_new_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut col = div().flex().flex_col().pb(px(2.));
        for (i, (name, kind)) in NEW_TAB_KINDS.iter().enumerate() {
            col =
                col.child(row(("new-tab-kind", i), false).pl(px(18.)).child(label(*name)).on_click(cx.listener(move |this, _, _, cx| {
                    this.new_menu = false;
                    match *kind {
                        // Web は URL か検索の言葉が要るので、ランチャーで打ってもらう
                        "web" => this.spawn("prelude-launcher", &[]),
                        k => this.send("tab.open", json!({ "pane": { "type": k } })),
                    }
                    cx.notify();
                })));
        }
        col
    }

    /// 別のプログラムを起動する。失敗だけを知らせる
    fn spawn(&mut self, prog: &str, args: &[&str]) {
        if let Err(e) = std::process::Command::new(prog).args(args).spawn() {
            self.error = Some(format!("{prog}: {e}"));
        }
    }

    fn render_bookmarks(&self, snap: &Snapshot, cx: &mut Context<Self>) -> impl IntoElement {
        let open: HashSet<&str> = snap.state.tabs.iter().filter_map(|t| t.bookmark.as_deref()).collect();
        let mut list = div().id("bookmarks").flex().flex_col().max_h(relative(0.35)).overflow_y_scroll();
        let mut folder = None::<&str>;
        for (i, b) in snap.bookmarks.iter().enumerate() {
            if !b.folder.is_empty() && folder != Some(b.folder.as_str()) {
                folder = Some(b.folder.as_str());
                let name = b.folder.clone();
                let closed = self.folded.contains(&name);
                list = list.child(
                    row(("bm-folder", i), false).child(badge(if closed { "▸" } else { "▾" })).child(label(name.clone())).on_click(
                        cx.listener(move |this, _, _, cx| {
                            if !this.folded.remove(&name) {
                                this.folded.insert(name.clone());
                            }
                            cx.notify();
                        }),
                    ),
                );
            }
            if !b.folder.is_empty() && self.folded.contains(&b.folder) {
                continue;
            }
            let path = b.path.clone();
            let mut r = row(("bm", i), false).child(label(b.title.clone()));
            if !b.folder.is_empty() {
                r = r.pl(px(24.));
            }
            if open.contains(b.path.as_str()) {
                r = r.child(badge("●"));
            }
            list = list.child(r.on_click(cx.listener(move |this, _, _, _| this.send("bookmark.open", json!({ "path": path })))));
        }
        if snap.bookmarks.is_empty() {
            list = list.child(div().px(px(6.)).text_color(color::dim()).child("(none)"));
        }
        list
    }

    fn render_tabs(&self, snap: &Snapshot, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div().id("tabs").flex().flex_col().flex_1().min_h(px(0.)).overflow_y_scroll();
        for t in &snap.state.tabs {
            let active = Some(t.id) == snap.state.active;
            let (id, group) = (t.id, SharedString::from(format!("tab-{}", t.id)));
            let close = div()
                .id(("tab-x", id))
                .flex_none()
                .px(px(4.))
                .invisible()
                .group_hover(group.clone(), |s| s.visible())
                .hover(|s| s.text_color(color::danger()))
                .child("×")
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.send("tab.close", json!({ "tab": { "id": id } }));
                }));
            list = list.child(
                row(("tab", id), active)
                    .group(group)
                    .child(num(t.num.to_string(), active))
                    .child(label(t.title.clone()))
                    .child(close)
                    .on_click(cx.listener(move |this, _, _, _| this.send("tab.activate", json!({ "tab": { "id": id } })))),
            );
        }
        if snap.state.tabs.is_empty() {
            list = list.child(div().px(px(6.)).text_color(color::dim()).child("(no tabs)"));
        }
        list
    }
}

impl Render for Sidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = prelude_ui::root().p(px(6.)).gap(px(2.)).border_r_1().border_color(color::panel());
        match self.snap.clone() {
            Some(snap) => {
                root = root
                    .child(section_title("SPACE"))
                    .child(self.render_space(&snap, cx))
                    .child(section_title(format!("BOOKMARKS {}", snap.bookmarks.len())))
                    .child(self.render_bookmarks(&snap, cx))
                    .child(rule())
                    .child(self.render_tabs_title(&snap, cx))
                    .when(self.new_menu, |d| d.child(self.render_new_menu(cx)))
                    .child(self.render_tabs(&snap, cx));
            }
            None => {
                let why = self.offline.clone().unwrap_or_else(|| "connecting...".into());
                root = root
                    .child(section_title("PRELUDE"))
                    .child(div().px(px(6.)).child("prelude-core is not running"))
                    .child(div().px(px(6.)).text_color(color::dim()).text_size(px(11.)).child(why))
                    .child(div().flex_1());
            }
        }
        if let Some(e) = &self.error {
            root = root.child(div().px(px(6.)).text_size(px(11.)).text_color(color::danger()).child(e.clone()));
        }
        root
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let mut window_mode = false;
    let default_width = prelude_ui::THEME.sidebar_width;
    let mut width = default_width;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--window" => window_mode = true,
            "--width" => width = args.next().and_then(|w| w.parse().ok()).unwrap_or(default_width),
            "-h" | "--help" => {
                println!("usage: prelude-sidebar [--window] [--width PX]");
                return;
            }
            _ => {
                eprintln!("usage: prelude-sidebar [--window] [--width PX]");
                std::process::exit(2);
            }
        }
    }

    application().run(move |cx: &mut App| {
        prelude_ui::init(cx);
        let (tx, mut rx) = unbounded();
        {
            let tx = tx.clone();
            std::thread::spawn(move || watch(tx));
        }
        let kind = if window_mode {
            WindowKind::Normal
        } else {
            WindowKind::LayerShell(LayerShellOptions {
                namespace: "prelude-sidebar".into(),
                layer: Layer::Top,
                anchor: Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM,
                exclusive_zone: Some(px(width)),
                keyboard_interactivity: KeyboardInteractivity::OnDemand,
                ..Default::default()
            })
        };
        // layer-shell で上下に貼り付けるときは、高さ 0 で画面の高さいっぱいをコンポジタに任せる
        let height = if window_mode { 700. } else { 0. };
        let opened = cx.open_window(
            WindowOptions {
                titlebar: None,
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(width), px(height)), cx))),
                app_id: Some("prelude-sidebar".into()),
                kind,
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| Sidebar {
                    snap: None,
                    offline: None,
                    error: None,
                    space_menu: false,
                    new_menu: false,
                    folded: HashSet::new(),
                    tx,
                })
            },
        );
        let window = match opened {
            Ok(w) => w,
            Err(e) => {
                eprintln!("prelude-sidebar: cannot open the window: {e:#}");
                cx.quit();
                return;
            }
        };
        let view = window.update(cx, |_, _, cx| cx.entity()).expect("sidebar view");
        cx.spawn(async move |cx| {
            while let Some(m) = rx.next().await {
                view.update(cx, |v, cx| {
                    v.apply(m);
                    cx.notify();
                });
            }
        })
        .detach();
    });
}

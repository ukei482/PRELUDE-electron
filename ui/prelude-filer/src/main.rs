//! PRELUDE のファイラの枠(docs/redesign-rust.md の判断 10、移行の段階 5)。コアの `files`・`field` のペインのレシピの既定で、ペインごとに 1 つの窓として起動する。
//!
//! - 見せ方は `--view` で選ぶ部品([`FilerView`])。今あるのは `list`(`field` はまだ無いので `list` で出す)
//! - キー: ↑↓ で選ぶ、Enter で開く(フォルダなら入る、ファイルはアプリで開く)、Backspace・Alt+↑ で上のフォルダ、Alt+←/→ で戻る・進む、
//!   Ctrl+H で隠しファイル、F5 で読み直す
//! - 移ったフォルダはコアに知らせる(`pane.update` の `path`)。タブを開き直すとそのフォルダで開く
//! - フォルダの中が変わったら(ほかのアプリでファイルを足したなど)、1 秒以内に読み直す
//!
//! 使い方: `prelude-filer [--pane <id>] [--view list|field] [フォルダ]`(フォルダを省くとホーム)
use gpui::{
    App, Bounds, Context, FocusHandle, Focusable, KeyBinding, Window, WindowBounds, WindowOptions, actions, div, prelude::*, px, size,
};
use gpui_platform::application;
use prelude_filer::{DirState, FilerCommand, FilerView, views};
use prelude_proto::Client;
use prelude_ui::{badge, color};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

actions!(filer, [Up, Down, PageUp, PageDown, First, Last, Open, Parent, Back, Forward, ToggleHidden, Reload, Quit]);

struct Filer {
    dir: DirState,
    view: Box<dyn FilerView>,
    /// 頼まれた見せ方が無くて、代わりに一覧で出しているとき、その名前
    missing_view: Option<String>,
    pane: Option<u64>,
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
    focus: FocusHandle,
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/".into())
}

/// ホームの下を ~ にして見せる
fn short(p: &Path) -> String {
    let h = home();
    match p.strip_prefix(&h) {
        Ok(r) if r.as_os_str().is_empty() => "~".into(),
        Ok(r) => format!("~/{}", r.display()),
        Err(_) => p.display().to_string(),
    }
}

impl Filer {
    /// フォルダへ移る。record なら戻る履歴に積む
    fn go(&mut self, path: PathBuf, record: bool, window: &mut Window) {
        if path == self.dir.path {
            return;
        }
        let from = self.dir.path.clone();
        if record && !from.as_os_str().is_empty() {
            self.back.push(from.clone());
            self.forward.clear();
        }
        self.dir.load(&path);
        // 上のフォルダへ戻ったときは、出てきたフォルダを選ぶ
        if from.parent() == Some(path.as_path())
            && let Some(name) = from.file_name()
        {
            self.dir.select_name(&name.to_string_lossy());
        }
        self.title(window);
        self.report();
    }

    fn title(&self, window: &mut Window) {
        let name = self.dir.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| short(&self.dir.path));
        window.set_window_title(&format!("{name} - PRELUDE files"));
    }

    /// 今のフォルダをコアに知らせる(ペインの path。開き直すとそのフォルダ)
    fn report(&self) {
        let Some(pane) = self.pane else { return };
        let path = self.dir.path.to_string_lossy().into_owned();
        std::thread::spawn(move || {
            let r = Client::connect("prelude-filer", false)
                .and_then(|mut c| c.call("pane.update", json!({ "pane": pane, "fields": { "path": path } })));
            if let Err(e) = r {
                log::warn!("cannot tell prelude-core the folder: {e}");
            }
        });
    }

    fn open(&mut self, i: usize, window: &mut Window) {
        let Some(e) = self.dir.entries.get(i).cloned() else { return };
        self.dir.selected = Some(i);
        let p = self.dir.path.join(&e.name);
        if e.is_dir {
            self.go(p, true, window);
        } else if let Err(err) = std::process::Command::new("xdg-open").arg(&p).spawn() {
            self.dir.error = Some(format!("cannot open {} ({err})", p.display()));
        }
    }

    fn command(&mut self, c: FilerCommand, window: &mut Window, cx: &mut Context<Self>) {
        match c {
            FilerCommand::Select(i) => self.dir.selected = Some(i).filter(|i| *i < self.dir.entries.len()),
            FilerCommand::Open(i) => self.open(i, window),
            FilerCommand::Go(p) => self.go(p, true, window),
        }
        cx.notify();
    }

    fn act(&mut self, a: &dyn gpui::Action, window: &mut Window, cx: &mut Context<Self>) {
        let any = a.as_any();
        if any.is::<Up>() {
            self.dir.move_by(-1);
        } else if any.is::<Down>() {
            self.dir.move_by(1);
        } else if any.is::<PageUp>() {
            self.dir.move_by(-20);
        } else if any.is::<PageDown>() {
            self.dir.move_by(20);
        } else if any.is::<First>() {
            self.dir.move_by(-(self.dir.entries.len() as isize));
        } else if any.is::<Last>() {
            self.dir.move_by(self.dir.entries.len() as isize);
        } else if any.is::<Open>() {
            if let Some(i) = self.dir.selected {
                self.open(i, window);
            }
        } else if any.is::<Parent>() {
            if let Some(p) = self.dir.path.parent().map(Path::to_path_buf) {
                self.go(p, true, window);
            }
        } else if any.is::<Back>() {
            if let Some(p) = self.back.pop() {
                self.forward.push(self.dir.path.clone());
                self.go(p, false, window);
            }
        } else if any.is::<Forward>() {
            if let Some(p) = self.forward.pop() {
                self.back.push(self.dir.path.clone());
                self.go(p, false, window);
            }
        } else if any.is::<ToggleHidden>() {
            self.dir.show_hidden = !self.dir.show_hidden;
            self.dir.reload();
        } else if any.is::<Reload>() {
            self.dir.reload();
        } else if any.is::<Quit>() {
            cx.quit();
        }
        cx.notify();
    }
}

impl Focusable for Filer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Filer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let emit: prelude_filer::Emit = Rc::new(move |c, w, cx| {
            let _ = weak.update(cx, |f, cx| f.command(c, w, cx));
        });
        // 上の行: 上のフォルダへ・今のフォルダ・件数
        let parent = self.dir.path.parent().map(Path::to_path_buf);
        let up = div().id("up").flex_none().px(px(6.)).cursor_pointer().hover(|s| s.bg(color::panel2())).child("..").when_some(
            parent,
            |d, p| {
                let emit = emit.clone();
                d.on_click(move |_, w, cx| emit(FilerCommand::Go(p.clone()), w, cx))
            },
        );
        let mut info = format!("{}", self.dir.entries.len());
        if self.dir.show_hidden {
            info.push_str("  hidden");
        }
        if let Some(v) = &self.missing_view {
            info.push_str(&format!("  ({v}: not yet, list)"));
        }
        let head = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(30.))
            .flex_none()
            .px(px(4.))
            .border_b_1()
            .border_color(color::panel2())
            .child(up)
            .child(prelude_ui::label(short(&self.dir.path)))
            .child(badge(info));
        let mut root = prelude_ui::root()
            .key_context("Filer")
            .track_focus(&self.focus)
            .on_action(cx.listener(|f, a: &Up, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Down, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &PageUp, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &PageDown, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &First, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Last, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Open, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Parent, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Back, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Forward, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &ToggleHidden, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Reload, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Quit, w, cx| f.act(a, w, cx)))
            .child(head)
            .child(div().flex().flex_col().flex_1().min_h(px(0.)).child(self.view.render(&self.dir, emit)));
        // 開けなかったファイルなど(フォルダ自体が読めないときは、見せ方の部品が出す)
        if let (Some(e), false) = (&self.dir.error, self.dir.entries.is_empty()) {
            root = root.child(div().px(px(6.)).text_size(px(11.)).text_color(color::danger()).child(e.clone()));
        }
        root
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let usage = "usage: prelude-filer [--pane ID] [--view list|field] [FOLDER]";
    let (mut pane, mut view_name, mut path) = (None::<u64>, "list".to_string(), None::<PathBuf>);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--pane" => pane = args.next().and_then(|v| v.parse().ok()),
            "--view" => view_name = args.next().unwrap_or_default(),
            "-h" | "--help" => {
                println!("{usage}");
                return;
            }
            s if s.starts_with("--") => {
                eprintln!("{usage}");
                std::process::exit(2);
            }
            _ => path = Some(PathBuf::from(a)),
        }
    }
    let start = path.map(|p| if p.is_absolute() { p } else { std::env::current_dir().unwrap_or_default().join(p) }).unwrap_or_else(home);
    let (view, missing_view) = match views::by_name(&view_name) {
        Some(v) => (v, None),
        None => (views::by_name("list").expect("list view"), Some(view_name)),
    };

    application().run(move |cx: &mut App| {
        prelude_ui::init(cx);
        let c = Some("Filer");
        cx.bind_keys([
            KeyBinding::new("up", Up, c),
            KeyBinding::new("down", Down, c),
            KeyBinding::new("pageup", PageUp, c),
            KeyBinding::new("pagedown", PageDown, c),
            KeyBinding::new("home", First, c),
            KeyBinding::new("end", Last, c),
            KeyBinding::new("enter", Open, c),
            KeyBinding::new("backspace", Parent, c),
            KeyBinding::new("alt-up", Parent, c),
            KeyBinding::new("alt-left", Back, c),
            KeyBinding::new("alt-right", Forward, c),
            KeyBinding::new("ctrl-h", ToggleHidden, c),
            KeyBinding::new("f5", Reload, c),
            KeyBinding::new("ctrl-q", Quit, c),
        ]);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(720.), px(520.)), cx))),
                app_id: Some("prelude-filer".into()),
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    let mut f = Filer {
                        dir: DirState::default(),
                        view,
                        missing_view,
                        pane: None, // 最初のフォルダは知らせない(コアが渡したもの)
                        back: vec![],
                        forward: vec![],
                        focus: cx.focus_handle(),
                    };
                    f.go(start, false, window);
                    f.pane = pane;
                    window.focus(&f.focus, cx);
                    f
                })
            },
        );
        let window = match opened {
            Ok(w) => w,
            Err(e) => {
                eprintln!("prelude-filer: cannot open the window: {e:#}");
                cx.quit();
                return;
            }
        };
        // フォルダの中が変わったら読み直す(1 秒ごとに更新時刻を見る)
        let view = window.update(cx, |_, _, cx| cx.entity()).expect("filer view");
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                view.update(cx, |f, cx| {
                    if f.dir.changed() {
                        f.dir.reload();
                        cx.notify();
                    }
                });
            }
        })
        .detach();
        cx.on_window_closed(|cx, _| cx.quit()).detach();
    });
}

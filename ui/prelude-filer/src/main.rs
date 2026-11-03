//! PRELUDE のファイラの枠(docs/redesign-rust.md の判断 10、移行の段階 5)。コアの `files`・`field` のペインのレシピの既定で、ペインごとに 1 つの窓として起動する。
//!
//! - 見せ方は `--view` で選ぶ部品([`FilerView`])。今あるのは `list`(`field` はまだ無いので `list` で出す)
//! - キー: ↑↓ で選ぶ、Enter で開く(フォルダなら入る、ファイルはアプリで開く)、Backspace・Alt+↑ で上のフォルダ、Alt+←/→ で戻る・進む、
//!   Ctrl+H で隠しファイル、F5 で読み直す
//! - 操作: Ctrl+クリックで足す・外す、Shift+クリック・Shift+↑↓で範囲、Ctrl+A で全部。Ctrl+C / X / V でコピー・切り取り・貼り付け、
//!   Delete でゴミ箱、Shift+Delete で完全に消す(確かめてから)、F2 で名前の変更、Ctrl+Shift+N で新しいフォルダ、Esc で選びを解く
//! - 移ったフォルダはコアに知らせる(`pane.update` の `path`)。タブを開き直すとそのフォルダで開く
//! - フォルダの中が変わったら(ほかのアプリでファイルを足したなど)、1 秒以内に読み直す
//!
//! 使い方: `prelude-filer [--pane <id>] [--view list|field] [フォルダ]`(フォルダを省くとホーム)
use gpui::{
    App, Bounds, ClipboardItem, Context, Entity, FocusHandle, Focusable, KeyBinding, Window, WindowBounds, WindowOptions, actions, div,
    prelude::*, px, size,
};
use gpui_platform::application;
use prelude_filer::ops::{self, Mode};
use prelude_filer::{DirState, FilerCommand, FilerView, views};
use prelude_proto::Client;
use prelude_ui::{TextInput, badge, color};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

actions!(
    filer,
    [
        Up,
        Down,
        ShiftUp,
        ShiftDown,
        PageUp,
        PageDown,
        First,
        Last,
        Open,
        Parent,
        Back,
        Forward,
        ToggleHidden,
        Reload,
        Quit,
        SelectAll,
        Unselect,
        Copy,
        Cut,
        Paste,
        Trash,
        DeleteForever,
        Rename,
        NewFolder,
        PromptConfirm,
        PromptCancel
    ]
);

/// 下の行で聞いていること
enum Prompt {
    /// 名前の変更(元のパス)
    Rename(PathBuf),
    NewFolder,
    /// 完全に消してよいか
    DeleteForever(Vec<PathBuf>),
}

struct Filer {
    dir: DirState,
    view: Box<dyn FilerView>,
    /// 頼まれた見せ方が無くて、代わりに一覧で出しているとき、その名前
    missing_view: Option<String>,
    pane: Option<u64>,
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
    focus: FocusHandle,
    prompt: Option<Prompt>,
    /// 下の行の入力欄(名前の変更・新しいフォルダ)
    input: Option<Entity<TextInput>>,
    prompt_focus: FocusHandle,
    /// 下の行の知らせ(失敗なら true)
    status: Option<(String, bool)>,
    /// コピー・移動・ゴミ箱の途中
    busy: bool,
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
            FilerCommand::Select(i) => self.dir.select_one(i),
            FilerCommand::Toggle(i) => self.dir.toggle_mark(i),
            FilerCommand::Range(i) => self.dir.mark_range(i),
            FilerCommand::Open(i) => self.open(i, window),
            FilerCommand::Go(p) => self.go(p, true, window),
        }
        cx.notify();
    }

    fn act(&mut self, a: &dyn gpui::Action, window: &mut Window, cx: &mut Context<Self>) {
        let any = a.as_any();
        if any.is::<Up>() || any.is::<Down>() {
            self.dir.move_by(if any.is::<Up>() { -1 } else { 1 });
            if let Some(i) = self.dir.selected {
                self.dir.select_one(i);
            }
        } else if any.is::<ShiftUp>() || any.is::<ShiftDown>() {
            let anchor = self.dir.anchor.or(self.dir.selected);
            self.dir.move_by(if any.is::<ShiftUp>() { -1 } else { 1 });
            self.dir.anchor = anchor;
            if let Some(i) = self.dir.selected {
                self.dir.mark_range(i);
            }
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
        } else if any.is::<SelectAll>() {
            self.dir.mark_all();
        } else if any.is::<Unselect>() {
            self.dir.marked.clear();
            self.status = None;
        } else if any.is::<Copy>() || any.is::<Cut>() {
            let paths = self.dir.targets();
            if !paths.is_empty() {
                let cut = any.is::<Cut>();
                cx.write_to_clipboard(ClipboardItem::new_string(ops::clip_text(&paths)));
                ops::clip_note(&ops::clip_note_path(), &paths, cut);
                self.status = Some((format!("{} {} item(s)", if cut { "Cut" } else { "Copied" }, paths.len()), false));
            }
        } else if any.is::<Paste>() {
            self.paste(cx);
        } else if any.is::<Trash>() {
            let paths = self.dir.targets();
            if !paths.is_empty() {
                self.run(format!("Moving {} item(s) to the trash...", paths.len()), cx, move || (vec![], ops::trash(&paths)));
            }
        } else if any.is::<DeleteForever>() {
            let paths = self.dir.targets();
            if !paths.is_empty() {
                self.ask(Prompt::DeleteForever(paths), None, window, cx);
            }
        } else if any.is::<Rename>() {
            if let Some(e) = self.dir.selected_entry().cloned() {
                let name = e.name.clone();
                // 拡張子の前までを選ぶ(フォルダは全部)
                let stem = if e.is_dir { name.len() } else { name.rfind('.').filter(|i| *i > 0).unwrap_or(name.len()) };
                let input = cx.new(|cx| {
                    let mut t = TextInput::new(cx, "name", name.clone());
                    t.select(0..stem, cx);
                    t
                });
                self.ask(Prompt::Rename(self.dir.path.join(&name)), Some(input), window, cx);
            }
        } else if any.is::<NewFolder>() {
            let input = cx.new(|cx| {
                let mut t = TextInput::new(cx, "name", "New folder");
                t.select_everything(cx);
                t
            });
            self.ask(Prompt::NewFolder, Some(input), window, cx);
        }
        cx.notify();
    }

    /// 下の行で聞く。入力欄があればそこへ、無ければ聞いている行へキーを向ける
    fn ask(&mut self, p: Prompt, input: Option<Entity<TextInput>>, window: &mut Window, cx: &mut Context<Self>) {
        match &input {
            Some(i) => window.focus(&i.focus_handle(cx), cx),
            None => window.focus(&self.prompt_focus, cx),
        }
        self.prompt = Some(p);
        self.input = input;
        self.status = None;
    }

    fn end_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt = None;
        self.input = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input.as_ref().map(|i| i.read(cx).text().to_string()).unwrap_or_default();
        let p = self.prompt.take();
        let r = match &p {
            Some(Prompt::Rename(full)) => ops::rename(full, &text).map(Some),
            Some(Prompt::NewFolder) => ops::mkdir(&self.dir.path, &text).map(Some),
            Some(Prompt::DeleteForever(paths)) => {
                let paths = paths.clone();
                self.run(format!("Deleting {} item(s)...", paths.len()), cx, move || (vec![], ops::remove(&paths)));
                Ok(None)
            }
            None => Ok(None),
        };
        match r {
            Ok(name) => {
                self.dir.reload();
                if let Some(n) = name {
                    self.dir.select_name(&n);
                    if let Some(i) = self.dir.selected {
                        self.dir.select_one(i);
                    }
                }
                self.end_prompt(window, cx);
            }
            Err(e) => {
                // 打ち直せるように、聞いたままにする
                self.status = Some((e, true));
                self.prompt = p;
                cx.notify();
            }
        }
    }

    /// クリップボードのファイルを、今のフォルダへコピーか移動(切り取りなら)
    fn paste(&mut self, cx: &mut Context<Self>) {
        let text = cx.read_from_clipboard().and_then(|c| c.text()).unwrap_or_default();
        let paths = ops::clip_paths(&text);
        if paths.is_empty() {
            self.status = Some(("Nothing to paste".into(), true));
            return;
        }
        let note = ops::clip_note_path();
        let mode = if ops::clip_is_cut(&note, &paths) { Mode::Move } else { Mode::Copy };
        let dest = self.dir.path.clone();
        let what = if mode == Mode::Move { "Moving" } else { "Copying" };
        self.run(format!("{what} {} item(s)...", paths.len()), cx, move || {
            let r = ops::transfer(mode, &paths, &dest);
            if mode == Mode::Move && r.1.is_empty() {
                ops::clip_consumed(&note);
            }
            r
        });
    }

    /// 時間のかかる操作を裏で動かす。終わったら読み直し、できたものを選び、失敗を知らせる
    fn run(&mut self, msg: String, cx: &mut Context<Self>, job: impl FnOnce() -> (Vec<String>, Vec<String>) + Send + 'static) {
        if self.busy {
            self.status = Some(("Wait for the current operation".into(), true));
            return;
        }
        self.busy = true;
        self.status = Some((msg, false));
        let task = cx.background_executor().spawn(async move { job() });
        cx.spawn(async move |this, cx| {
            let (names, errors) = task.await;
            let _ = this.update(cx, |f, cx| {
                f.busy = false;
                f.dir.reload();
                if !names.is_empty() {
                    f.dir.marked = names.iter().cloned().collect();
                    f.dir.select_name(&names[0]);
                }
                f.status = if errors.is_empty() { None } else { Some((errors.join(" / "), true)) };
                cx.notify();
            });
        })
        .detach();
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
            .on_action(cx.listener(|f, a: &ShiftUp, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &ShiftDown, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &SelectAll, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Unselect, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Copy, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Cut, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Paste, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Trash, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &DeleteForever, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Rename, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &NewFolder, w, cx| f.act(a, w, cx)))
            .child(head)
            .child(div().flex().flex_col().flex_1().min_h(px(0.)).child(self.view.render(&self.dir, emit)));
        // 下の行: 聞いていること・知らせ・開けなかったファイルなど(フォルダ自体が読めないときは、見せ方の部品が出す)
        if let Some(p) = &self.prompt {
            let q = match p {
                Prompt::Rename(_) => "Rename:".to_string(),
                Prompt::NewFolder => "New folder:".to_string(),
                Prompt::DeleteForever(v) => format!("Delete {} item(s) permanently? Enter = delete, Esc = cancel", v.len()),
            };
            let mut bar = div()
                .id("prompt")
                .key_context("FilerPrompt")
                .track_focus(&self.prompt_focus)
                .on_action(cx.listener(|f, _: &PromptConfirm, w, cx| f.confirm(w, cx)))
                .on_action(cx.listener(|f, _: &PromptCancel, w, cx| f.end_prompt(w, cx)))
                .flex()
                .items_center()
                .gap(px(8.))
                .flex_none()
                .px(px(6.))
                .min_h(px(30.))
                .border_t_1()
                .border_color(color::line())
                .child(div().flex_none().child(q));
            if let Some(i) = &self.input {
                bar = bar.child(div().flex_1().child(i.clone()));
            }
            root = root.child(bar);
        }
        let note = self.status.clone().or_else(|| self.dir.error.clone().filter(|_| !self.dir.entries.is_empty()).map(|e| (e, true)));
        if let Some((msg, bad)) = note {
            root = root.child(
                div().flex_none().px(px(6.)).text_size(px(11.)).text_color(if bad { color::danger() } else { color::dim() }).child(msg),
            );
        } else if !self.dir.marked.is_empty() {
            root = root.child(
                div()
                    .flex_none()
                    .px(px(6.))
                    .text_size(px(11.))
                    .text_color(color::dim())
                    .child(format!("{} selected", self.dir.marked.len())),
            );
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
            KeyBinding::new("shift-up", ShiftUp, c),
            KeyBinding::new("shift-down", ShiftDown, c),
            KeyBinding::new("ctrl-a", SelectAll, c),
            KeyBinding::new("escape", Unselect, c),
            KeyBinding::new("ctrl-c", Copy, c),
            KeyBinding::new("ctrl-x", Cut, c),
            KeyBinding::new("ctrl-v", Paste, c),
            KeyBinding::new("delete", Trash, c),
            KeyBinding::new("shift-delete", DeleteForever, c),
            KeyBinding::new("f2", Rename, c),
            KeyBinding::new("ctrl-shift-n", NewFolder, c),
            // 下の行で聞いている間(入力欄の中でも効く)
            KeyBinding::new("enter", PromptConfirm, Some("FilerPrompt")),
            KeyBinding::new("escape", PromptCancel, Some("FilerPrompt")),
        ]);
        prelude_ui::input::bind_keys(cx);
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
                        prompt: None,
                        input: None,
                        prompt_focus: cx.focus_handle(),
                        status: None,
                        busy: false,
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

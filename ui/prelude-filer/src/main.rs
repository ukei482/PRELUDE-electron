//! PRELUDE のファイラの枠(docs/redesign-rust.md の「ファイラ」、移行の段階 5。外のファイラに任せるので消す予定)。コアの `files`・`field` のペインのレシピの既定で、ペインごとに 1 つの窓として起動する。
//! Electron 版のフォルダのペイン(`src/files.js`)と同じ操作にそろえる。F1 でキーの一覧。
//!
//! - 見せ方は `--view` で選ぶ部品([`FilerView`])。今あるのは `list`(`field` はまだ無いので `list` で出す)
//! - キー: ↑↓ で選ぶ、Enter で開く(フォルダなら入る、ファイルはアプリで開く)、Ctrl+Enter で右に分割して開く、Backspace・Alt+↑ で上のフォルダ、
//!   Alt+←/→ で戻る・進む、Alt+Home でホーム、Ctrl+L でパスを打つ、Ctrl+H で隠しファイル、F5・Ctrl+R で読み直す、名前の先頭を打つとそこへ飛ぶ
//! - 操作: Ctrl+クリック・Space・Insert で足す・外す、Shift+クリック・Shift+↑↓で範囲、Ctrl+↑↓ で選びを変えずに動く、Ctrl+A で全部。
//!   Ctrl+C / X / V でコピー・切り取り・貼り付け、Delete でゴミ箱、Shift+Delete で完全に消す(確かめてから)、F2 で名前の変更、
//!   Ctrl+Shift+N で新しいフォルダ、Ctrl+Shift+C でパスのコピー、Shift+F4 で端末、Alt+Enter でプロパティ、Esc で選びを解く
//! - 右クリック・メニューキー・Shift+F10 でメニュー。見出しを押すと並べ替え。マウスの戻る・進むボタン
//! - 移ったフォルダはコアに知らせる(`pane.update` の `path`)。タブを開き直すとそのフォルダで開く
//! - フォルダの中が変わったら(ほかのアプリでファイルを足したなど)、1 秒以内に読み直す
//!
//! - 選ぶ画面(`--choose`): アプリの「開く」「保存」のダイアログとして動く(「アプリのダイアログ」)。Enter・ボタンで、選んだパスを `--output` のファイルに書いて終わる。
//!   Esc・Cancel・窓を閉じると何も書かずに終わる(取り消し)。消す・移すなどの操作はできない。
//!   `open`(ファイル 1 つ)・`open-multiple`(Ctrl+クリックなどで複数。Ctrl+Enter で確定)・`directory`(今のフォルダ)・`save`(下の欄に名前。あるファイルは確かめる。Ctrl+S でも確定)
//!
//! 使い方: `prelude-filer [--pane <id>] [--view list|field] [--choose <種類> --output <ファイル> [--name <名前>] [--title <題名>] [--filter <種類>]] [フォルダ]`(フォルダを省くとホーム)
use gpui::{
    App, Bounds, ClipboardItem, Context, Entity, FocusHandle, Focusable, KeyBinding, KeyDownEvent, Pixels, Point, Window, WindowBounds,
    WindowOptions, actions, anchored, deferred, div, point, prelude::*, px, size,
};
use gpui_platform::application;
use prelude_filer::info::{self, DirSize, PropItem, TypeAhead};
use prelude_filer::ops::{self, Mode};
use prelude_filer::{DirState, FilerCommand, FilerView, human_size, parse_filters, views};
use prelude_proto::Client;
use prelude_ui::{TextInput, badge, color};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

actions!(
    filer,
    [
        Up,
        Down,
        ShiftUp,
        ShiftDown,
        CtrlUp,
        CtrlDown,
        PageUp,
        PageDown,
        First,
        Last,
        Open,
        OpenSplit,
        Parent,
        Back,
        Forward,
        GoHome,
        EditPath,
        ToggleHidden,
        Reload,
        Quit,
        SelectAll,
        Unselect,
        InsertMark,
        Copy,
        Cut,
        Paste,
        Trash,
        DeleteForever,
        Rename,
        NewFolder,
        CopyPath,
        Terminal,
        Properties,
        Help,
        MenuKey,
        PromptConfirm,
        PromptCancel,
        ChooseConfirm,
        PathGo,
        PathCancel,
        MenuUp,
        MenuDown,
        MenuConfirm,
        MenuClose,
        DialogClose
    ]
);

/// 下の行で聞いていること
enum Prompt {
    /// 名前の変更(元のパス)
    Rename(PathBuf),
    NewFolder,
    /// 完全に消してよいか
    DeleteForever(Vec<PathBuf>),
    /// たくさんのファイルを開いてよいか(11 個から)
    OpenMany(Vec<PathBuf>),
    /// 保存の名前(選ぶ画面)
    SaveName,
}

/// 選ぶ画面(アプリのダイアログ)の種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChooseMode {
    Open,
    OpenMultiple,
    Directory,
    Save,
}

struct Choose {
    mode: ChooseMode,
    output: PathBuf,
    /// 窓の題名(アプリが決めたもの)
    title: String,
    /// 確かめた上書き先(もう一度確定すると置き換える)
    overwrite: Option<PathBuf>,
    /// ファイルの種類(名前と glob)。押すと次の種類に替わる
    filters: Vec<(String, Vec<String>)>,
    filter: usize,
}

/// メニューの項目で行うこと
#[derive(Debug, Clone, PartialEq)]
enum MenuAct {
    Open,
    OpenSplit,
    Cut,
    Copy,
    Paste,
    Rename,
    Trash,
    DeleteForever,
    CopyPath,
    Terminal,
    Properties,
    NewFolder,
    ToggleHidden,
    Reload,
    /// フォルダへ移る(ドライブと場所)
    Go(PathBuf),
}

struct MenuItem {
    label: String,
    key: &'static str,
    act: MenuAct,
    enabled: bool,
    checked: bool,
    danger: bool,
}

/// 右クリックのメニュー(Electron 版の popup)。None の項目は区切り
struct Menu {
    items: Vec<Option<MenuItem>>,
    sel: Option<usize>,
    pos: Point<Pixels>,
}

impl Menu {
    /// 次(前)の選べる項目へ
    fn step(&mut self, d: isize) {
        let n = self.items.len() as isize;
        let mut i = self.sel.map(|i| i as isize).unwrap_or(if d > 0 { -1 } else { n });
        for _ in 0..n {
            i = (i + d).rem_euclid(n);
            if self.items[i as usize].as_ref().is_some_and(|m| m.enabled) {
                self.sel = Some(i as usize);
                return;
            }
        }
    }
}

/// 真ん中に出す小さな窓(Electron 版の fileDialog)
enum Dialog {
    Props { items: Vec<PropItem>, where_: PathBuf, size: Option<DirSize> },
    Help,
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
    /// 上の行のパスの欄(Ctrl+L・パスを押したとき)
    path_input: Option<Entity<TextInput>>,
    menu: Option<Menu>,
    menu_focus: FocusHandle,
    dialog: Option<Dialog>,
    dialog_focus: FocusHandle,
    /// 名前の先頭で飛ぶ
    type_ahead: TypeAhead,
    /// 囲んで選び始める前の選び(Ctrl を押して始めたとき、これに足す)
    band_base: Option<std::collections::BTreeSet<String>>,
    /// 下の行の知らせ(失敗なら true)
    status: Option<(String, bool)>,
    /// コピー・移動・ゴミ箱の途中
    busy: bool,
    /// 選ぶ画面として動いているとき
    choose: Option<Choose>,
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

/// コアに頼む(すぐ返るので待つ)
fn core(cmd: &str, args: Value) -> Result<Value, String> {
    Client::connect("prelude-filer", false).and_then(|mut c| c.call(cmd, args)).map_err(|e| format!("{cmd}: {e}"))
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
        self.refresh_cut();
        self.title(window);
        self.report();
    }

    fn title(&self, window: &mut Window) {
        if let Some(c) = self.choose.as_ref().filter(|c| !c.title.is_empty()) {
            window.set_window_title(&c.title);
            return;
        }
        let name = self.dir.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| short(&self.dir.path));
        window.set_window_title(&format!("{name} - PRELUDE files"));
    }

    /// 今のフォルダをコアに知らせる(ペインの path。開き直すとそのフォルダ)
    fn report(&self) {
        let Some(pane) = self.pane else { return };
        let path = self.dir.path.to_string_lossy().into_owned();
        std::thread::spawn(move || {
            if let Err(e) = core("pane.update", json!({ "pane": pane, "fields": { "path": path } })) {
                log::warn!("cannot tell prelude-core the folder: {e}");
            }
        });
    }

    /// 切り取ったもの(クリップボードの控え)のうち、このフォルダのものを薄く出す
    fn refresh_cut(&mut self) {
        let cut: std::collections::BTreeSet<String> = ops::cut_paths(&ops::clip_note_path())
            .into_iter()
            .filter(|p| p.parent() == Some(self.dir.path.as_path()))
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect();
        self.dir.cut = cut;
    }

    fn open(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(e) = self.dir.entries.get(i).cloned() else { return };
        self.dir.selected = Some(i);
        let p = self.dir.path.join(&e.name);
        if e.is_dir {
            self.go(p, true, window);
        } else if self.choose.is_some() {
            self.choose_confirm(window, cx);
        } else {
            self.open_files(vec![p], false, window, cx);
        }
    }

    /// Enter: 複数選んでいて、選んでいる位置がその中ならそれを全部、そうでなければ選んでいる 1 つを開く(Electron 版の openEntries)
    fn open_targets(&mut self, split: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(i) = self.dir.selected else { return };
        if self.choose.is_some() {
            if split {
                self.choose_confirm(window, cx);
            } else {
                self.open(i, window, cx);
            }
            return;
        }
        let list = if !split && self.dir.marked.len() > 1 && self.dir.is_marked(i) {
            self.dir.targets()
        } else {
            self.dir.entries.get(i).map(|e| vec![self.dir.path.join(&e.name)]).unwrap_or_default()
        };
        if let Some(d) = list.iter().find(|p| p.is_dir()).cloned() {
            if split {
                self.open_split(d);
            } else if list.len() == 1 {
                self.go(d, true, window);
            } else {
                self.open_files(list, false, window, cx);
            }
            return;
        }
        self.open_files(list, false, window, cx);
    }

    /// ファイルをアプリで開く(xdg-open)。11 個からは確かめる。開いた窓はコアが今のタブのペインにする(「PRELUDE を通さずに開いた窓」)
    fn open_files(&mut self, list: Vec<PathBuf>, confirmed: bool, window: &mut Window, cx: &mut Context<Self>) {
        let files: Vec<PathBuf> = list.into_iter().filter(|p| !p.is_dir()).collect();
        if files.len() > 10 && !confirmed {
            self.ask(Prompt::OpenMany(files), None, window, cx);
            return;
        }
        for p in files {
            if let Err(err) = std::process::Command::new("xdg-open").arg(&p).spawn() {
                self.status = Some((format!("cannot open {} ({err})", p.display()), true));
            }
        }
    }

    /// フォルダを右に分割したファイラで開く(Ctrl+Enter)。コアのペインでなければ、別の窓で開く
    fn open_split(&mut self, dir: PathBuf) {
        let path = dir.to_string_lossy().into_owned();
        let r = match self.pane {
            Some(pane) => core("pane.split", json!({ "pane": pane, "dir": "row", "new": { "type": "files", "path": path } })).map(|_| ()),
            None => std::env::current_exe()
                .and_then(|exe| std::process::Command::new(exe).arg(&dir).spawn())
                .map(|_| ())
                .map_err(|e| e.to_string()),
        };
        if let Err(e) = r {
            self.status = Some((e, true));
        }
    }

    /// 端末を開く(Shift+F4)。選んでいる 1 つがフォルダならそこ、そうでなければ今のフォルダ。コアのペインなら下に分割する
    fn open_terminal(&mut self) {
        let dir = match (self.dir.marked.len() <= 1, self.dir.selected_entry()) {
            (true, Some(e)) if e.is_dir && (self.dir.marked.is_empty() || self.dir.marked.contains(&e.name)) => self.dir.path.join(&e.name),
            _ => self.dir.path.clone(),
        };
        let cwd = dir.to_string_lossy().into_owned();
        let new = json!({ "type": "console", "cwd": cwd });
        let r = match self.pane {
            Some(pane) => core("pane.split", json!({ "pane": pane, "dir": "col", "new": new })),
            None => core("tab.open", json!({ "pane": new })),
        };
        if let Err(e) = r {
            self.status = Some((format!("Failed to launch terminal ({e})"), true));
        }
    }

    fn command(&mut self, c: FilerCommand, window: &mut Window, cx: &mut Context<Self>) {
        match c {
            FilerCommand::Select(i) => {
                self.dir.select_one(i);
                self.save_name_from_selection(window, cx);
            }
            FilerCommand::Toggle(i) | FilerCommand::Range(i) if !self.can_mark() => self.dir.select_one(i),
            FilerCommand::Toggle(i) => self.dir.toggle_mark(i),
            FilerCommand::Range(i) => self.dir.mark_range(i),
            FilerCommand::Open(i) => self.open(i, window, cx),
            FilerCommand::Go(p) => self.go(p, true, window),
            FilerCommand::Sort(k) => {
                let s = self.dir.sort.clicked(k);
                self.dir.set_sort(s);
            }
            FilerCommand::Band(Some((lo, hi, cur, add))) if self.can_mark() => {
                let base = self.band_base.get_or_insert_with(|| if add { self.dir.marked.clone() } else { Default::default() }).clone();
                self.dir.mark_band(&base, lo, hi, cur);
            }
            FilerCommand::Band(_) => self.band_base = None,
            FilerCommand::History(fwd) => self.act(if fwd { &Forward as &dyn gpui::Action } else { &Back }, window, cx),
            FilerCommand::Menu(i, pos) => {
                if let Some(i) = i
                    && !self.dir.is_marked(i)
                    && self.dir.selected != Some(i)
                {
                    self.dir.select_one(i);
                }
                if self.choose.is_none() {
                    self.open_menu(i.is_some(), pos, window, cx);
                }
            }
        }
        cx.notify();
    }

    // ------------------------------------------------------------------ メニュー

    /// メニューを出す。on_item: ファイルの上(選んでいるものの操作)か、空いた所(フォルダの操作)か
    fn open_menu(&mut self, on_item: bool, pos: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let it = |label: &str, key: &'static str, act: MenuAct| Some(MenuItem { label: label.into(), key, act, enabled: true, checked: false, danger: false });
        let targets = if on_item { self.dir.targets() } else { vec![] };
        let one = (targets.len() == 1).then(|| targets[0].clone());
        let items: Vec<Option<MenuItem>> = if !targets.is_empty() {
            let mut v = vec![it("Open", "Enter", MenuAct::Open)];
            if one.as_ref().is_some_and(|p| p.is_dir()) {
                v.push(it("Open in right pane", "Ctrl+Enter", MenuAct::OpenSplit));
            }
            v.extend([None, it("Cut", "Ctrl+X", MenuAct::Cut), it("Copy", "Ctrl+C", MenuAct::Copy), it("Paste", "Ctrl+V", MenuAct::Paste)]);
            v.push(Some(MenuItem { enabled: one.is_some(), ..it("Rename", "F2", MenuAct::Rename).unwrap() }));
            v.extend([
                None,
                it("Move to trash", "Delete", MenuAct::Trash),
                Some(MenuItem { danger: true, ..it("Delete permanently", "Shift+Del", MenuAct::DeleteForever).unwrap() }),
                None,
                it("Copy path", "Ctrl+Shift+C", MenuAct::CopyPath),
                it("Open in terminal", "Shift+F4", MenuAct::Terminal),
                it("Properties", "Alt+Enter", MenuAct::Properties),
            ]);
            v
        } else {
            // 空いた所: 選びを解いて、フォルダそのものの操作
            self.dir.marked.clear();
            vec![
                it("New folder", "Ctrl+Shift+N", MenuAct::NewFolder),
                it("Paste", "Ctrl+V", MenuAct::Paste),
                None,
                Some(MenuItem { checked: self.dir.show_hidden, ..it("Show hidden files", "Ctrl+H", MenuAct::ToggleHidden).unwrap() }),
                it("Reload", "F5", MenuAct::Reload),
                None,
                it("Copy path", "Ctrl+Shift+C", MenuAct::CopyPath),
                it("Open in terminal", "Shift+F4", MenuAct::Terminal),
                it("Properties", "Alt+Enter", MenuAct::Properties),
            ]
        };
        self.show_menu(items, pos, false, window, cx);
    }

    /// メニューを出す。keyboard: キーで開いたら最初の項目を選んでおく
    fn show_menu(&mut self, items: Vec<Option<MenuItem>>, pos: Point<Pixels>, keyboard: bool, window: &mut Window, cx: &mut Context<Self>) {
        let mut m = Menu { items, sel: None, pos };
        if keyboard {
            m.step(1);
        }
        self.menu = Some(m);
        window.focus(&self.menu_focus, cx);
    }

    fn close_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menu = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn menu_run(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(act) = self.menu.as_ref().and_then(|m| m.items.get(i)).and_then(|m| m.as_ref()).filter(|m| m.enabled).map(|m| m.act.clone())
        else {
            return;
        };
        self.close_menu(window, cx);
        match act {
            MenuAct::Open => self.act(&Open, window, cx),
            MenuAct::OpenSplit => self.act(&OpenSplit, window, cx),
            MenuAct::Cut => self.act(&Cut, window, cx),
            MenuAct::Copy => self.act(&Copy, window, cx),
            MenuAct::Paste => self.act(&Paste, window, cx),
            MenuAct::Rename => self.act(&Rename, window, cx),
            MenuAct::Trash => self.act(&Trash, window, cx),
            MenuAct::DeleteForever => self.act(&DeleteForever, window, cx),
            MenuAct::CopyPath => self.act(&CopyPath, window, cx),
            MenuAct::Terminal => self.act(&Terminal, window, cx),
            MenuAct::Properties => self.act(&Properties, window, cx),
            MenuAct::NewFolder => self.act(&NewFolder, window, cx),
            MenuAct::ToggleHidden => self.act(&ToggleHidden, window, cx),
            MenuAct::Reload => self.act(&Reload, window, cx),
            MenuAct::Go(p) => self.go(p, true, window),
        }
    }

    /// ドライブと場所のメニュー(上の行のボタン)
    fn drives_menu(&mut self, pos: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let user = std::env::var("USER").unwrap_or_default();
        let items = info::drives(&home(), &user, Path::new("/"))
            .into_iter()
            .map(|(label, p)| Some(MenuItem { label, key: "", act: MenuAct::Go(p), enabled: true, checked: false, danger: false }))
            .collect();
        self.show_menu(items, pos, false, window, cx);
    }

    // ------------------------------------------------------------------ 小さな窓

    fn show_dialog(&mut self, d: Dialog, window: &mut Window, cx: &mut Context<Self>) {
        self.dialog = Some(d);
        window.focus(&self.dialog_focus, cx);
    }

    /// プロパティ(Alt+Enter)。フォルダが入っていれば、中身の合計を裏で数える(4 秒まで)
    fn properties(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let t = self.dir.targets();
        let paths = if t.is_empty() { vec![self.dir.path.clone()] } else { t };
        let items = info::props(&paths);
        let need = items.iter().any(|i| i.is_dir);
        let size = (!need).then(|| DirSize { size: items.iter().map(|i| i.size).sum(), files: items.len() as u64, ..Default::default() });
        self.show_dialog(Dialog::Props { items, where_: self.dir.path.clone(), size }, window, cx);
        if need {
            let task = cx.background_executor().spawn(async move { info::dir_size(&paths, Duration::from_secs(4)) });
            cx.spawn(async move |this, cx| {
                let r = task.await;
                let _ = this.update(cx, |f, cx| {
                    if let Some(Dialog::Props { size, .. }) = &mut f.dialog {
                        *size = Some(r);
                        cx.notify();
                    }
                });
            })
            .detach();
        }
    }

    fn close_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dialog = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    // ------------------------------------------------------------------ パスの欄

    fn edit_path(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.dir.path.to_string_lossy().into_owned();
        let input = cx.new(|cx| {
            let mut t = TextInput::new(cx, "Path", text);
            t.select_everything(cx);
            t
        });
        window.focus(&input.focus_handle(cx), cx);
        self.path_input = Some(input);
    }

    fn path_go(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.path_input.as_ref().map(|i| i.read(cx).text().to_string()).unwrap_or_default();
        match info::resolve_input(&text, &self.dir.path, &home()) {
            Some(p) if p.is_dir() => {
                self.path_input = None;
                window.focus(&self.focus, cx);
                self.go(p, true, window);
            }
            Some(p) if p.is_file() => {
                // ファイルなら、そのフォルダへ移ってファイルを選ぶ
                self.path_input = None;
                window.focus(&self.focus, cx);
                if let (Some(d), Some(n)) = (p.parent(), p.file_name()) {
                    self.go(d.to_path_buf(), true, window);
                    self.dir.select_name(&n.to_string_lossy());
                    if let Some(i) = self.dir.selected {
                        self.dir.select_one(i);
                    }
                }
            }
            _ => self.status = Some((format!("Not found: {text}"), true)),
        }
        cx.notify();
    }

    fn path_cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.path_input = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    // ------------------------------------------------------------------ 打った文字

    /// 名前の先頭を打つとそこへ飛ぶ。空白は、打っている途中でなければ選びの切り替え(Electron 版と同じ)
    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.is_focused(window) {
            return;
        }
        let m = &ev.keystroke.modifiers;
        if m.control || m.alt || m.platform || m.function {
            return;
        }
        let Some(ch) = ev.keystroke.key_char.clone().filter(|c| !c.is_empty() && !c.chars().any(char::is_control)) else { return };
        let now = Instant::now();
        if ch == " " && !self.type_ahead.active(now) {
            if self.can_mark()
                && let Some(i) = self.dir.selected
            {
                self.dir.toggle_mark(i);
            }
        } else {
            let names: Vec<&str> = self.dir.entries.iter().map(|e| e.name.as_str()).collect();
            if let Some(i) = self.type_ahead.push(&names, self.dir.selected, &ch, now) {
                self.dir.select_one(i);
                self.save_name_from_selection(window, cx);
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// 複数を選べるか(選ぶ画面では open-multiple だけ)
    fn can_mark(&self) -> bool {
        self.choose.as_ref().is_none_or(|c| c.mode == ChooseMode::OpenMultiple)
    }

    /// 保存の画面でファイルを選んだら、その名前を欄に入れて、打てるようにする
    fn save_name_from_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.prompt, Some(Prompt::SaveName)) {
            return;
        }
        let Some(input) = self.input.clone() else { return };
        if let Some(e) = self.dir.selected_entry().filter(|e| !e.is_dir).cloned() {
            input.update(cx, |i, cx| i.set_text(e.name, cx));
        }
        window.focus(&input.focus_handle(cx), cx);
    }

    /// 選ぶ画面: 次のファイルの種類にする
    fn next_filter(&mut self) {
        let Some(c) = &mut self.choose else { return };
        if c.filters.is_empty() {
            return;
        }
        c.filter = (c.filter + 1) % c.filters.len();
        self.dir.filter = c.filters[c.filter].1.clone();
        let path = self.dir.path.clone();
        self.dir.load(&path);
    }

    /// 選ぶ画面: 今の選びで確定する。選んだパスを書いて終わる
    fn choose_confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mode) = self.choose.as_ref().map(|c| c.mode) else { return };
        let paths: Vec<PathBuf> = match mode {
            ChooseMode::Directory => vec![self.dir.path.clone()],
            ChooseMode::Open => self.dir.targets().into_iter().take(1).collect(),
            ChooseMode::OpenMultiple => self.dir.targets(),
            ChooseMode::Save => {
                let name = self.input.as_ref().map(|i| i.read(cx).text().to_string()).unwrap_or_default();
                let Some(p) = ops::save_target(&self.dir.path, &name) else {
                    self.status = Some(("Invalid name".into(), true));
                    return;
                };
                if p.is_dir() {
                    self.go(p, true, window);
                    return;
                }
                // あるファイルは、もう一度確定したら置き換える
                let Some(c) = &mut self.choose else { return };
                if p.exists() && c.overwrite.as_ref() != Some(&p) {
                    self.status = Some((format!("\"{name}\" already exists. Press Enter again to replace it"), true));
                    c.overwrite = Some(p);
                    return;
                }
                vec![p]
            }
        };
        let Some(c) = &self.choose else { return };
        if paths.is_empty() {
            return;
        }
        match ops::write_choice(&c.output, &paths) {
            Ok(()) => cx.quit(),
            Err(e) => self.status = Some((e, true)),
        }
    }

    fn act(&mut self, a: &dyn gpui::Action, window: &mut Window, cx: &mut Context<Self>) {
        // 小さな窓・メニューを出している間は、一覧の操作をしない(Electron 版のダイアログと同じ)
        if self.dialog.is_some() || self.menu.is_some() {
            return;
        }
        let any = a.as_any();
        // 選ぶ画面では、ファイルを変える操作はしない。複数選びは open-multiple だけ
        if self.choose.is_some()
            && (any.is::<Cut>()
                || any.is::<Paste>()
                || any.is::<Trash>()
                || any.is::<DeleteForever>()
                || any.is::<Rename>()
                || any.is::<Terminal>()
                || any.is::<MenuKey>()
                || (!self.can_mark()
                    && (any.is::<SelectAll>() || any.is::<ShiftUp>() || any.is::<ShiftDown>() || any.is::<InsertMark>() || any.is::<CtrlUp>() || any.is::<CtrlDown>())))
        {
            return;
        }
        let page = 20;
        if any.is::<Up>() || any.is::<Down>() || any.is::<PageUp>() || any.is::<PageDown>() || any.is::<First>() || any.is::<Last>() {
            let n = self.dir.entries.len() as isize;
            let d = if any.is::<Up>() {
                -1
            } else if any.is::<Down>() {
                1
            } else if any.is::<PageUp>() {
                -page
            } else if any.is::<PageDown>() {
                page
            } else if any.is::<First>() {
                -n
            } else {
                n
            };
            self.dir.move_by(d);
            if let Some(i) = self.dir.selected {
                self.dir.select_one(i);
            }
            self.save_name_from_selection(window, cx);
        } else if any.is::<ShiftUp>() || any.is::<ShiftDown>() {
            let anchor = self.dir.anchor.or(self.dir.selected);
            self.dir.move_by(if any.is::<ShiftUp>() { -1 } else { 1 });
            self.dir.anchor = anchor;
            if let Some(i) = self.dir.selected {
                self.dir.mark_range(i);
            }
        } else if any.is::<CtrlUp>() || any.is::<CtrlDown>() {
            self.dir.move_cursor_only(if any.is::<CtrlUp>() { -1 } else { 1 });
        } else if any.is::<InsertMark>() {
            if let Some(i) = self.dir.selected {
                self.dir.toggle_mark(i);
                self.dir.move_by(1);
            }
        } else if any.is::<Open>() {
            self.open_targets(false, window, cx);
        } else if any.is::<OpenSplit>() {
            self.open_targets(true, window, cx);
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
        } else if any.is::<GoHome>() {
            self.go(home(), true, window);
        } else if any.is::<EditPath>() {
            self.edit_path(window, cx);
        } else if any.is::<ToggleHidden>() {
            self.dir.show_hidden = !self.dir.show_hidden;
            self.dir.reload();
            self.status = Some((if self.dir.show_hidden { "Showing hidden files" } else { "Hiding hidden files" }.into(), false));
        } else if any.is::<Reload>() {
            self.dir.reload();
            self.refresh_cut();
        } else if any.is::<Quit>() {
            cx.quit();
        } else if any.is::<SelectAll>() {
            self.dir.mark_all();
        } else if any.is::<Unselect>() {
            if self.choose.is_some() && self.dir.marked.is_empty() {
                cx.quit(); // 選ぶ画面の Esc は取り消し
            }
            self.dir.marked.clear();
            self.status = None;
        } else if any.is::<ChooseConfirm>() {
            self.choose_confirm(window, cx);
        } else if any.is::<Copy>() || any.is::<Cut>() {
            let paths = self.dir.targets();
            if !paths.is_empty() {
                let cut = any.is::<Cut>();
                cx.write_to_clipboard(ClipboardItem::new_string(ops::clip_text(&paths)));
                ops::clip_note(&ops::clip_note_path(), &paths, cut);
                self.refresh_cut();
                self.status = Some((format!("{} {} items", if cut { "Cut" } else { "Copied" }, paths.len()), false));
            }
        } else if any.is::<CopyPath>() {
            let t = self.dir.targets();
            let text = if t.is_empty() { self.dir.path.to_string_lossy().into_owned() } else { ops::clip_text(&t) };
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            self.status = Some(("Path copied".into(), false));
        } else if any.is::<Paste>() {
            self.paste(cx);
        } else if any.is::<Trash>() {
            let paths = self.dir.targets();
            if !paths.is_empty() {
                self.run(format!("Moving {} items to the trash...", paths.len()), cx, move || (vec![], ops::trash(&paths)));
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
        } else if any.is::<Terminal>() {
            self.open_terminal();
        } else if any.is::<Properties>() {
            self.properties(window, cx);
        } else if any.is::<Help>() {
            self.show_dialog(Dialog::Help, window, cx);
        } else if any.is::<MenuKey>() {
            // キーで開いたメニューは、窓の左上の近く(一覧の上)に出す
            let on_item = !self.dir.targets().is_empty();
            self.open_menu(on_item, point(px(40.), px(60.)), window, cx);
            if let Some(m) = &mut self.menu {
                m.step(1);
            }
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
        if matches!(self.prompt, Some(Prompt::SaveName)) {
            self.choose_confirm(window, cx);
            cx.notify();
            return;
        }
        let text = self.input.as_ref().map(|i| i.read(cx).text().to_string()).unwrap_or_default();
        let p = self.prompt.take();
        let r = match &p {
            Some(Prompt::Rename(full)) => ops::rename(full, &text).map(Some),
            Some(Prompt::NewFolder) => ops::mkdir(&self.dir.path, &text).map(Some),
            Some(Prompt::DeleteForever(paths)) => {
                let paths = paths.clone();
                self.run(format!("Deleting {} items...", paths.len()), cx, move || (vec![], ops::remove(&paths)));
                Ok(None)
            }
            Some(Prompt::OpenMany(files)) => {
                let files = files.clone();
                self.end_prompt(window, cx);
                self.open_files(files, true, window, cx);
                return;
            }
            Some(Prompt::SaveName) | None => Ok(None),
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
        self.run(format!("{what} {} items...", paths.len()), cx, move || {
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
                f.refresh_cut();
                if !names.is_empty() {
                    f.dir.marked = names.iter().cloned().collect();
                    f.dir.select_name(&names[0]);
                }
                f.status = if errors.is_empty() {
                    None
                } else {
                    Some((format!("{} failed: {}", errors.len(), errors.join(" / ")), true))
                };
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

/// 押せる小さな文字(上の行のボタン・選ぶ画面のボタン)
fn button(id: &'static str, t: impl Into<gpui::SharedString>) -> gpui::Stateful<gpui::Div> {
    div().id(id).flex_none().px(px(6.)).cursor_pointer().hover(|s| s.bg(color::panel2())).child(t.into())
}

impl Filer {
    fn render_head(&mut self, cx: &mut Context<Self>) -> gpui::Div {
        let nav = |id: &'static str, t: &'static str, enabled: bool| button(id, t).when(!enabled, |b| b.text_color(color::dim()));
        let head = div().flex().items_center().gap(px(2.)).h(px(30.)).flex_none().px(px(4.)).border_b_1().border_color(color::panel2());
        let head = head
            .child(nav("back", "←", !self.back.is_empty()).on_click(cx.listener(|f, _, w, cx| f.act(&Back, w, cx))))
            .child(nav("fwd", "→", !self.forward.is_empty()).on_click(cx.listener(|f, _, w, cx| f.act(&Forward, w, cx))))
            .child(nav("up", "↑", self.dir.path.parent().is_some()).on_click(cx.listener(|f, _, w, cx| f.act(&Parent, w, cx))))
            .child(button("home", "⌂").on_click(cx.listener(|f, _, w, cx| f.act(&GoHome, w, cx))))
            .child(button("drives", "⛁").on_click(cx.listener(|f, ev: &gpui::ClickEvent, w, cx| {
                let pos = ev.position();
                f.drives_menu(point(pos.x, pos.y + px(10.)), w, cx);
                cx.notify();
            })));
        let path: gpui::AnyElement = match &self.path_input {
            Some(i) => div()
                .flex_1()
                .key_context("FilerPath")
                .on_action(cx.listener(|f, _: &PathGo, w, cx| f.path_go(w, cx)))
                .on_action(cx.listener(|f, _: &PathCancel, w, cx| f.path_cancel(w, cx)))
                .child(i.clone())
                .into_any_element(),
            None => div()
                .id("path")
                .flex_1()
                .min_w(px(0.))
                .px(px(4.))
                .cursor_text()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(short(&self.dir.path))
                .on_click(cx.listener(|f, _, w, cx| {
                    f.edit_path(w, cx);
                    cx.notify();
                }))
                .into_any_element(),
        };
        let mut info = String::new();
        if self.dir.show_hidden {
            info.push_str("hidden");
        }
        if let Some(v) = &self.missing_view {
            info.push_str(&format!("  ({v}: not yet, list)"));
        }
        head.child(path).child(badge(info))
    }

    fn render_menu(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let m = self.menu.as_ref()?;
        let mut list = div()
            .id("menu")
            .key_context("FilerMenu")
            .track_focus(&self.menu_focus)
            .on_action(cx.listener(|f, _: &MenuUp, _, cx| {
                if let Some(m) = &mut f.menu {
                    m.step(-1);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|f, _: &MenuDown, _, cx| {
                if let Some(m) = &mut f.menu {
                    m.step(1);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|f, _: &MenuConfirm, w, cx| {
                if let Some(i) = f.menu.as_ref().and_then(|m| m.sel) {
                    f.menu_run(i, w, cx);
                }
            }))
            .on_action(cx.listener(|f, _: &MenuClose, w, cx| f.close_menu(w, cx)))
            .on_mouse_down_out(cx.listener(|f, _, w, cx| f.close_menu(w, cx)))
            .flex()
            .flex_col()
            .min_w(px(220.))
            .py(px(4.))
            .bg(color::bg())
            .border_1()
            .border_color(color::line())
            .shadow_lg();
        for (i, item) in m.items.iter().enumerate() {
            let Some(item) = item else {
                list = list.child(prelude_ui::rule());
                continue;
            };
            let sel = m.sel == Some(i);
            let mut r = div()
                .id(("menu-item", i))
                .flex()
                .items_center()
                .gap(px(12.))
                .h(px(26.))
                .px(px(10.))
                .child(div().flex_none().w(px(12.)).child(if item.checked { "✓" } else { "" }))
                .child(div().flex_1().child(item.label.clone()))
                .child(div().flex_none().text_size(px(10.)).text_color(if sel { color::bg() } else { color::dim() }).child(item.key));
            if !item.enabled {
                r = r.text_color(color::dim());
            } else {
                r = r
                    .cursor_pointer()
                    .when(item.danger && !sel, |r| r.text_color(color::danger()))
                    .when(sel, |r| r.bg(color::accent()).text_color(color::bg()))
                    .on_mouse_move(cx.listener(move |f, _, _, cx| {
                        if let Some(m) = &mut f.menu
                            && m.sel != Some(i)
                        {
                            m.sel = Some(i);
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |f, _, w, cx| f.menu_run(i, w, cx)));
            }
            list = list.child(r);
        }
        Some(deferred(anchored().position(m.pos).snap_to_window_with_margin(px(4.)).child(list)).with_priority(1).into_any_element())
    }

    fn render_dialog(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let d = self.dialog.as_ref()?;
        let kv = |k: &str, v: String| {
            div().flex().gap(px(12.)).child(div().flex_none().w(px(110.)).text_color(color::dim()).child(k.to_string())).child(div().flex_1().child(v))
        };
        let (title, body): (&str, gpui::Div) = match d {
            Dialog::Help => ("Folder keys", info::KEYS.iter().fold(div().flex().flex_col().gap(px(2.)), |b, (k, v)| {
                b.child(div().flex().gap(px(12.)).child(div().flex_none().w(px(240.)).text_color(color::dim()).child(*k)).child(div().flex_1().child(*v)))
            })),
            Dialog::Props { items, where_, size } => {
                let size_text = |s: &Option<DirSize>, single_file: Option<u64>| match (s, single_file) {
                    (_, Some(n)) => format!("{} ({} bytes)", human_size(n), info::grouped(n)),
                    (None, _) => "Calculating…".into(),
                    (Some(s), _) => {
                        format!("{} ({} files){}", human_size(s.size), info::grouped(s.files), if s.partial { " *partial (timed out)" } else { "" })
                    }
                };
                let mut b = div().flex().flex_col().gap(px(2.));
                if let [it] = items.as_slice() {
                    b = b.child(kv("Name", it.name.clone())).child(kv("Location", it.full.display().to_string()));
                    if let Some(e) = &it.error {
                        b = b.child(kv("Error", e.clone()));
                    }
                    b = b.child(kv("Type", if it.is_dir { format!("Folder ({} items)", it.count.unwrap_or(0)) } else { "File".into() }));
                    if let Some(l) = &it.link {
                        b = b.child(kv("Link target", l.clone()));
                    }
                    b = b.child(kv("Size", size_text(size, (!it.is_dir && it.error.is_none()).then_some(it.size))));
                    if let Some(t) = it.modified {
                        b = b.child(kv("Modified", info::full_time(t)));
                    }
                    if let Some(t) = it.created {
                        b = b.child(kv("Created", info::full_time(t)));
                    }
                    if it.error.is_none() {
                        b = b.child(kv("Mode", it.mode.clone())).child(kv("Owner (uid)", it.uid.to_string()));
                    }
                } else {
                    let dirs = items.iter().filter(|i| i.is_dir).count();
                    b = b
                        .child(kv("Location", where_.display().to_string()))
                        .child(kv("Items", format!("{} ({dirs} folders, {} files)", items.len(), items.len() - dirs)))
                        .child(kv("Total size", match size {
                            Some(s) if !items.iter().any(|i| i.is_dir) => human_size(s.size),
                            s => size_text(s, None),
                        }));
                }
                ("Properties", b)
            }
        };
        let close = button("dialog-close", "Close").border_1().border_color(color::line()).py(px(2.)).on_click(cx.listener(|f, _, w, cx| f.close_dialog(w, cx)));
        let boxed = div()
            .id("dialog")
            .key_context("FilerDialog")
            .track_focus(&self.dialog_focus)
            .on_action(cx.listener(|f, _: &DialogClose, w, cx| f.close_dialog(w, cx)))
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(12.))
            .max_w(px(640.))
            .bg(color::bg())
            .border_1()
            .border_color(color::line())
            .shadow_lg()
            .child(div().text_color(color::accent()).child(title))
            .child(body)
            .child(div().flex().justify_end().child(close));
        // 後ろを暗くして、押すと閉じる
        Some(
            deferred(
                div()
                    .id("dialog-bg")
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(gpui::black().opacity(0.4))
                    .occlude()
                    .on_mouse_down(gpui::MouseButton::Left, cx.listener(|f, _, w, cx| f.close_dialog(w, cx)))
                    .child(boxed.on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

impl Render for Filer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let emit: prelude_filer::Emit = Rc::new(move |c, w, cx| {
            let _ = weak.update(cx, |f, cx| f.command(c, w, cx));
        });
        let head = self.render_head(cx);
        let mut root = prelude_ui::root()
            .relative()
            .key_context("Filer")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|f, ev: &KeyDownEvent, w, cx| f.on_key(ev, w, cx)))
            .on_action(cx.listener(|f, a: &Up, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Down, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &CtrlUp, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &CtrlDown, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &PageUp, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &PageDown, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &First, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Last, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Open, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &OpenSplit, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Parent, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Back, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Forward, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &GoHome, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &EditPath, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &ToggleHidden, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Reload, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Quit, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &ShiftUp, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &ShiftDown, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &SelectAll, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Unselect, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &InsertMark, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Copy, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Cut, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Paste, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Trash, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &DeleteForever, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Rename, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &NewFolder, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &CopyPath, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Terminal, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Properties, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &Help, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &MenuKey, w, cx| f.act(a, w, cx)))
            .on_action(cx.listener(|f, a: &ChooseConfirm, w, cx| f.act(a, w, cx)))
            .child(head)
            .child(div().flex().flex_col().flex_1().min_h(px(0.)).child(self.view.render(&self.dir, emit)));
        // 下の行: 聞いていること(フォルダ自体が読めないときは、見せ方の部品が出す)
        if let Some(p) = &self.prompt {
            let q = match p {
                Prompt::Rename(_) => "Rename:".to_string(),
                Prompt::NewFolder => "New folder:".to_string(),
                Prompt::DeleteForever(v) => {
                    let names: Vec<String> = v.iter().take(5).filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).collect();
                    let more = if v.len() > 5 { format!(" and {} more", v.len() - 5) } else { String::new() };
                    format!("Delete permanently (cannot be undone): {}{more}? Enter = delete, Esc = cancel", names.join(", "))
                }
                Prompt::OpenMany(v) => format!("Open {} files? Enter = open, Esc = cancel", v.len()),
                Prompt::SaveName => "Save as:".to_string(),
            };
            let mut bar = div()
                .id("prompt")
                .key_context("FilerPrompt")
                .track_focus(&self.prompt_focus)
                .on_action(cx.listener(|f, _: &PromptConfirm, w, cx| f.confirm(w, cx)))
                .on_action(cx.listener(|f, _: &PromptCancel, w, cx| {
                    if matches!(f.prompt, Some(Prompt::SaveName)) {
                        cx.quit(); // 保存の欄の Esc は取り消し
                    } else {
                        f.end_prompt(w, cx);
                    }
                }))
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
        // 選ぶ画面: 確定と取り消しのボタン
        if let Some(c) = &self.choose {
            let text: gpui::SharedString = match c.mode {
                ChooseMode::Directory => "Select this folder".into(),
                ChooseMode::Save => "Save".into(),
                _ => "Select".into(),
            };
            let bordered = |id: &'static str, t: gpui::SharedString| button(id, t).px(px(10.)).py(px(3.)).border_1().border_color(color::line());
            let kind = c.filters.get(c.filter).map(|(name, _)| gpui::SharedString::from(format!("Type: {name}")));
            root = root.child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .flex_none()
                    .p(px(6.))
                    .when_some(kind, |d, k| {
                        d.child(bordered("filter", k).on_click(cx.listener(|f, _, _, cx| {
                            f.next_filter();
                            cx.notify();
                        })))
                        .child(div().flex_1())
                    })
                    .child(bordered("cancel", "Cancel".into()).on_click(cx.listener(|_, _, _, cx| cx.quit())))
                    .child(bordered("confirm", text).on_click(cx.listener(|f, _, w, cx| {
                        f.choose_confirm(w, cx);
                        cx.notify();
                    }))),
            );
        }
        // いちばん下: 件数か知らせ(左)、操作の途中か F1(右)。Electron 版の updateStatus
        let note = self.status.clone().or_else(|| self.dir.error.clone().filter(|_| !self.dir.entries.is_empty()).map(|e| (e, true)));
        let (left, bad) = match note {
            Some((m, bad)) => (m, bad),
            None => (self.dir.status_text(), false),
        };
        let right = if self.busy { "Working…" } else { "F1: Keys" };
        root = root.child(
            div()
                .flex()
                .gap(px(8.))
                .flex_none()
                .px(px(6.))
                .h(px(18.))
                .text_size(px(11.))
                .child(div().flex_1().min_w(px(0.)).overflow_hidden().whitespace_nowrap().text_color(if bad { color::danger() } else { color::dim() }).child(left))
                .child(div().flex_none().text_color(if self.busy { color::accent() } else { color::dim() }).child(right)),
        );
        if let Some(m) = self.render_menu(cx) {
            root = root.child(m);
        }
        if let Some(d) = self.render_dialog(cx) {
            root = root.child(d);
        }
        root
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let usage = "usage: prelude-filer [--pane ID] [--view list|field] [--choose open|open-multiple|directory|save --output FILE [--name NAME] [--title TITLE]] [FOLDER]";
    let (mut choose_mode, mut output, mut save_name, mut title) = (None::<ChooseMode>, None::<PathBuf>, String::new(), String::new());
    let (mut pane, mut view_name, mut path) = (None::<u64>, "list".to_string(), None::<PathBuf>);
    let mut filters: Vec<(String, Vec<String>)> = vec![];
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--pane" => pane = args.next().and_then(|v| v.parse().ok()),
            "--view" => view_name = args.next().unwrap_or_default(),
            "--choose" => {
                choose_mode = match args.next().as_deref() {
                    Some("open") => Some(ChooseMode::Open),
                    Some("open-multiple") => Some(ChooseMode::OpenMultiple),
                    Some("directory") => Some(ChooseMode::Directory),
                    Some("save") => Some(ChooseMode::Save),
                    _ => {
                        eprintln!("{usage}");
                        std::process::exit(2);
                    }
                }
            }
            "--output" => output = args.next().map(PathBuf::from),
            "--name" => save_name = args.next().unwrap_or_default(),
            "--title" => title = args.next().unwrap_or_default(),
            "--filter" => filters = parse_filters(&args.next().unwrap_or_default()),
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
    let first_filter: Vec<String> = filters.first().map(|(_, g)| g.clone()).unwrap_or_default();
    let choose = match (choose_mode, output) {
        (Some(mode), Some(output)) => Some(Choose { mode, output, title, overwrite: None, filters: filters.clone(), filter: 0 }),
        (None, _) => None,
        (Some(_), None) => {
            eprintln!("{usage}");
            std::process::exit(2);
        }
    };
    // 選ぶ画面は、paths のフォルダがまだ無いときは開ける一番近いフォルダで開く
    let start = if choose.is_some() { ops::nearest_dir(&start) } else { start };
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
            KeyBinding::new("ctrl-up", CtrlUp, c),
            KeyBinding::new("ctrl-down", CtrlDown, c),
            KeyBinding::new("pageup", PageUp, c),
            KeyBinding::new("pagedown", PageDown, c),
            KeyBinding::new("home", First, c),
            KeyBinding::new("end", Last, c),
            KeyBinding::new("enter", Open, c),
            KeyBinding::new("ctrl-enter", OpenSplit, c),
            KeyBinding::new("backspace", Parent, c),
            KeyBinding::new("alt-up", Parent, c),
            KeyBinding::new("alt-left", Back, c),
            KeyBinding::new("alt-right", Forward, c),
            KeyBinding::new("alt-home", GoHome, c),
            KeyBinding::new("ctrl-l", EditPath, c),
            KeyBinding::new("ctrl-h", ToggleHidden, c),
            KeyBinding::new("f5", Reload, c),
            KeyBinding::new("ctrl-r", Reload, c),
            KeyBinding::new("ctrl-q", Quit, c),
            KeyBinding::new("shift-up", ShiftUp, c),
            KeyBinding::new("shift-down", ShiftDown, c),
            KeyBinding::new("ctrl-a", SelectAll, c),
            KeyBinding::new("escape", Unselect, c),
            KeyBinding::new("insert", InsertMark, c),
            KeyBinding::new("ctrl-c", Copy, c),
            KeyBinding::new("ctrl-x", Cut, c),
            KeyBinding::new("ctrl-v", Paste, c),
            KeyBinding::new("delete", Trash, c),
            KeyBinding::new("shift-delete", DeleteForever, c),
            KeyBinding::new("f2", Rename, c),
            KeyBinding::new("ctrl-shift-n", NewFolder, c),
            KeyBinding::new("ctrl-shift-c", CopyPath, c),
            KeyBinding::new("shift-f4", Terminal, c),
            KeyBinding::new("alt-enter", Properties, c),
            KeyBinding::new("f1", Help, c),
            KeyBinding::new("shift-f10", MenuKey, c),
            KeyBinding::new("menu", MenuKey, c),
            KeyBinding::new("ctrl-s", ChooseConfirm, c),
            // 下の行で聞いている間(入力欄の中でも効く)
            KeyBinding::new("enter", PromptConfirm, Some("FilerPrompt")),
            KeyBinding::new("escape", PromptCancel, Some("FilerPrompt")),
            // パスの欄
            KeyBinding::new("enter", PathGo, Some("FilerPath")),
            KeyBinding::new("escape", PathCancel, Some("FilerPath")),
            // メニュー
            KeyBinding::new("up", MenuUp, Some("FilerMenu")),
            KeyBinding::new("down", MenuDown, Some("FilerMenu")),
            KeyBinding::new("enter", MenuConfirm, Some("FilerMenu")),
            KeyBinding::new("escape", MenuClose, Some("FilerMenu")),
            // 小さな窓
            KeyBinding::new("enter", DialogClose, Some("FilerDialog")),
            KeyBinding::new("escape", DialogClose, Some("FilerDialog")),
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
                        dir: DirState {
                            show_hidden: prelude_ui::SETTINGS.show_hidden,
                            filter: first_filter.clone(),
                            ..DirState::default()
                        },
                        view,
                        missing_view,
                        pane: None, // 最初のフォルダは知らせない(コアが渡したもの)
                        back: vec![],
                        forward: vec![],
                        focus: cx.focus_handle(),
                        prompt: None,
                        input: None,
                        prompt_focus: cx.focus_handle(),
                        path_input: None,
                        menu: None,
                        menu_focus: cx.focus_handle(),
                        dialog: None,
                        dialog_focus: cx.focus_handle(),
                        type_ahead: TypeAhead::default(),
                        band_base: None,
                        status: None,
                        busy: false,
                        choose,
                    };
                    f.go(start, false, window);
                    f.pane = pane;
                    window.focus(&f.focus, cx);
                    if f.choose.as_ref().is_some_and(|c| c.mode == ChooseMode::Save) {
                        let input = cx.new(|cx| {
                            let stem = save_name.rfind('.').filter(|i| *i > 0).unwrap_or(save_name.len());
                            let mut t = TextInput::new(cx, "name", save_name.clone());
                            t.select(0..stem, cx);
                            t
                        });
                        window.focus(&input.focus_handle(cx), cx);
                        f.prompt = Some(Prompt::SaveName);
                        f.input = Some(input);
                    }
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
        // フォルダの中が変わったら読み直す(1 秒ごとに更新時刻を見る)。切り取りの印もほかのファイラと合わせる
        let view = window.update(cx, |_, _, cx| cx.entity()).expect("filer view");
        cx.spawn(async move |cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                view.update(cx, |f, cx| {
                    let cut = f.dir.cut.clone();
                    f.refresh_cut();
                    if f.dir.changed() {
                        f.dir.reload();
                        cx.notify();
                    } else if f.dir.cut != cut {
                        cx.notify();
                    }
                });
            }
        })
        .detach();
        cx.on_window_closed(|cx, _| cx.quit()).detach();
    });
}

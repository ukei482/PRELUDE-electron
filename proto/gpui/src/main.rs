//! GPUI の試作(docs/redesign-rust.md の段階 0)。サイドバー(段階 4)を GPUI で作れるかを確かめる。
//!
//! 確かめること
//! - layer-shell: 画面の左端に場所を予約して貼り付く(ほかの窓はその右に寄る)
//! - 日本語入力(fcitx5)とフォント: 入力欄に変換しながら打てるか。漢字・かな・記号が正しく出るか
//! - 1 万件の一覧: `uniform_list` で見える行だけを描く。読み込みの時間とスクロールの滑らかさ
//! - GPU が不調なとき: Vulkan が無いときに GL(ソフトウェア描画を含む)へ切り替わるか。使った GPU はログに出る
//!
//! 使い方: `prelude-gpui-proto [sidebar|window] [フォルダ]`(既定は sidebar。フォルダを渡すと中のファイル名、無ければ作った 1 万件)
//! 終わるのは Esc か Ctrl+Q
mod input;

use gpui::{
    div, layer_shell::*, prelude::*, px, rgb, size, uniform_list, App, Bounds, Context, Entity, Focusable, KeyBinding, SharedString,
    Window, WindowBounds, WindowKind, WindowOptions,
};
use gpui_platform::application;
use input::*;
use std::time::{Duration, Instant};

gpui::actions!(proto, [Quit]);

const WIDTH: f32 = 300.;
const SAMPLE: &str = "日本語: 漢字・ひらがな・カタカナ・ｶﾀｶﾅ・①②・〜・「」 / English 0123 / 😀";

struct Proto {
    input: Entity<TextInput>,
    rows: Vec<SharedString>,
    took: Duration,
    source: SharedString,
    mode: &'static str,
}

impl Render for Proto {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text = self.input.read(cx).content.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0xffffff))
            .text_color(rgb(0x000000))
            .text_size(px(14.))
            .border_r_1()
            .border_color(rgb(0x1a3cc8))
            .child(
                div()
                    .p_2()
                    .bg(rgb(0x1a3cc8))
                    .text_color(rgb(0xffffff))
                    .child(format!("PRELUDE GPUI proto ({})", self.mode)),
            )
            .child(div().p_2().child(SAMPLE))
            .child(div().px_2().child("入力(日本語入力を試す):"))
            .child(div().px_2().child(self.input.clone()))
            .child(div().px_2().pb_2().text_color(rgb(0x555555)).child(format!("入力した文字: {text}")))
            .child(div().px_2().pb_1().text_color(rgb(0x555555)).child(format!(
                "{} 件({}、{:.1} ms)",
                self.rows.len(),
                self.source,
                self.took.as_secs_f64() * 1000.
            )))
            .child(
                uniform_list(
                    "rows",
                    self.rows.len(),
                    cx.processor(|this, range: std::ops::Range<usize>, _window, _cx| {
                        range
                            .map(|ix| {
                                div()
                                    .id(ix)
                                    .px_2()
                                    .h(px(22.))
                                    .flex()
                                    .items_center()
                                    .hover(|s| s.bg(rgb(0xe8ecfb)))
                                    .child(this.rows[ix].clone())
                            })
                            .collect()
                    }),
                )
                .flex_1(),
            )
    }
}

/// 一覧の中身。フォルダが読めればその中の名前(名前順)、無ければ作った 1 万件
fn rows(dir: Option<&str>) -> (Vec<SharedString>, Duration, SharedString) {
    let t0 = Instant::now();
    if let Some(d) = dir {
        if let Ok(rd) = std::fs::read_dir(d) {
            let mut v: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
            v.sort();
            return (v.into_iter().map(SharedString::from).collect(), t0.elapsed(), d.to_string().into());
        }
        eprintln!("cannot read {d}; using generated rows");
    }
    let v = (1..=10_000).map(|i| SharedString::from(format!("{i:05}  資料_{}.txt", ["議事録", "論文", "写真", "見積"][i % 4]))).collect();
    (v, t0.elapsed(), "generated".into())
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,zbus=warn,tracing=warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode: &'static str = if args.first().map(String::as_str) == Some("window") { "window" } else { "sidebar" };
    let dir = args.iter().map(String::as_str).find(|a| *a != "window" && *a != "sidebar").map(str::to_string);

    application().run(move |cx: &mut App| {
        cx.bind_keys([
            KeyBinding::new("backspace", Backspace, None),
            KeyBinding::new("delete", Delete, None),
            KeyBinding::new("left", Left, None),
            KeyBinding::new("right", Right, None),
            KeyBinding::new("shift-left", SelectLeft, None),
            KeyBinding::new("shift-right", SelectRight, None),
            KeyBinding::new("ctrl-a", SelectAll, None),
            KeyBinding::new("ctrl-v", Paste, None),
            KeyBinding::new("ctrl-c", Copy, None),
            KeyBinding::new("ctrl-x", Cut, None),
            KeyBinding::new("home", Home, None),
            KeyBinding::new("end", End, None),
            KeyBinding::new("escape", Quit, None),
            KeyBinding::new("ctrl-q", Quit, None),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());

        let kind = if mode == "sidebar" {
            WindowKind::LayerShell(LayerShellOptions {
                namespace: "prelude-sidebar".into(),
                layer: Layer::Top,
                anchor: Anchor::LEFT | Anchor::TOP | Anchor::BOTTOM,
                exclusive_zone: Some(px(WIDTH)),
                keyboard_interactivity: KeyboardInteractivity::OnDemand,
                ..Default::default()
            })
        } else {
            WindowKind::Normal
        };
        // layer-shell で上下に貼り付けるときは、高さ 0 で「画面の高さいっぱい」をコンポジタに任せる
        let height = if mode == "sidebar" { 0. } else { 700. };
        let bounds = Bounds::centered(None, size(px(WIDTH), px(height)), cx);
        let (rows, took, source) = rows(dir.as_deref());
        let opened = cx.open_window(
            WindowOptions {
                titlebar: None,
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                app_id: Some("prelude-gpui-proto".into()),
                kind,
                ..Default::default()
            },
            move |_, cx| {
                let input = cx.new(|cx| TextInput {
                    focus_handle: cx.focus_handle(),
                    content: "".into(),
                    placeholder: "ここに入力".into(),
                    selected_range: 0..0,
                    selection_reversed: false,
                    marked_range: None,
                    last_layout: None,
                    last_bounds: None,
                    is_selecting: false,
                });
                cx.new(|_| Proto { input, rows, took, source, mode })
            },
        );
        let window = match opened {
            Ok(w) => w,
            Err(e) => {
                eprintln!("cannot open the {mode} window: {e:#}");
                cx.quit();
                return;
            }
        };
        let input = window.update(cx, |p, _, _| p.input.clone()).unwrap();
        // 入力欄が変わったら一覧の上の「入力した文字」を描き直す
        window
            .update(cx, |_, window, cx| {
                cx.observe(&input, |_, _, cx| cx.notify()).detach();
                window.focus(&input.focus_handle(cx), cx);
                cx.activate(true);
            })
            .unwrap();
    });
}

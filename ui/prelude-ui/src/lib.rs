//! PRELUDE の UI の共通の見た目と部品(docs/redesign-rust.md の判断 11)。サイドバー・ランチャー・ファイラが同じ色・書体・行の形になるように、ここにまとめる。
//! 見た目は Electron 版(src/style.css)に合わせる: 青地に白い文字、角丸なし、英数字は VCR OSD Mono・日本語は DotGothic16。
use gpui::{App, Div, ElementId, FontFallbacks, Hsla, Pixels, SharedString, Stateful, div, font, prelude::*, px, rgb};
use std::borrow::Cow;

/// 色(Electron 版の --bg・--panel などと同じ)
pub mod color {
    use gpui::{Hsla, rgb};
    pub fn bg() -> Hsla {
        rgb(0x1926a3).into()
    }
    pub fn panel() -> Hsla {
        rgb(0x2b3dc8).into()
    }
    pub fn panel2() -> Hsla {
        rgb(0x3a50e0).into()
    }
    pub fn line() -> Hsla {
        rgb(0xffffff).into()
    }
    pub fn text() -> Hsla {
        rgb(0xf0f3ff).into()
    }
    pub fn dim() -> Hsla {
        rgb(0xaab4ee).into()
    }
    pub fn accent() -> Hsla {
        rgb(0xffffff).into()
    }
    pub fn danger() -> Hsla {
        rgb(0xff8080).into()
    }
}

/// 文字の大きさ(Electron 版の --fs)
pub const FONT_SIZE: Pixels = px(13.);
/// 行の高さ(Electron 版の .item)
pub const ROW_HEIGHT: Pixels = px(30.);

const MONO: &str = "VCR OSD Mono";
const JA: &str = "DotGothic16";

/// 書体を読み込む。アプリを始めたら最初に 1 回呼ぶ。読めなくても動く(システムの書体になる)
pub fn init(cx: &mut App) {
    let fonts: Vec<Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!("../../../src/fonts/VCR_OSD_MONO_1.001.ttf")),
        Cow::Borrowed(include_bytes!("../../../src/fonts/DotGothic16-Regular.ttf")),
    ];
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        log::warn!("cannot load PRELUDE fonts: {e:#}");
    }
}

/// 一番外の箱: 青地・白い文字・PRELUDE の書体(英数字は VCR OSD Mono、無い文字は DotGothic16、それも無ければシステム)
pub fn root() -> Div {
    let mut f = font(MONO);
    f.fallbacks = Some(FontFallbacks::from_fonts(vec![JA.into(), "Noto Sans CJK JP".into()]));
    div().size_full().flex().flex_col().bg(color::bg()).text_color(color::text()).text_size(FONT_SIZE).font(f)
}

/// 欄の見出し(「TABS」など。薄い色・少し小さく)
pub fn section_title(text: impl Into<SharedString>) -> Div {
    div().flex().items_center().px(px(6.)).pt(px(6.)).pb(px(2.)).text_color(color::dim()).text_size(px(11.)).child(text.into())
}

/// 一覧の 1 行。active なら反転(白地に青い文字)、そうでなければ乗せると明るくなる
pub fn row(id: impl Into<ElementId>, active: bool) -> Stateful<Div> {
    let d = div().id(id.into()).flex().items_center().gap(px(8.)).h(ROW_HEIGHT).px(px(6.)).cursor_pointer().overflow_hidden();
    if active { d.bg(color::accent()).text_color(color::bg()) } else { d.hover(|s| s.bg(color::panel2())) }
}

/// 行の中の番号(名札。ランチャーで 3 / b3 と呼ぶもの)
pub fn num(text: impl Into<SharedString>, active: bool) -> Div {
    let c: Hsla = if active { color::bg() } else { color::dim() };
    div().flex_none().min_w(px(14.)).text_size(px(10.)).text_color(c).child(text.into())
}

/// 行の中の名前(はみ出したら … で切る)
pub fn label(text: impl Into<SharedString>) -> Div {
    div().flex_1().min_w(px(0.)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(text.into())
}

/// 小さな印(● など)
pub fn badge(text: impl Into<SharedString>) -> Div {
    div().flex_none().text_size(px(10.)).text_color(color::dim()).child(text.into())
}

/// 細い横線
pub fn rule() -> Div {
    div().h(px(1.)).mx(px(6.)).my(px(4.)).bg(rgb(0x3a50e0))
}

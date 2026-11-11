//! PRELUDE の UI の共通の見た目と部品(docs/redesign-rust.md の「UI」)。サイドバー・ランチャー・ファイラが同じ色・書体・行の形になるように、ここにまとめる。
//! 見た目は Electron 版(src/style.css)に合わせる: 青地に白い文字、角丸なし、英数字は VCR OSD Mono・日本語は DotGothic16。
use gpui::{App, Div, ElementId, FontFallbacks, Hsla, Pixels, SharedString, Stateful, div, font, prelude::*, px, rgb};
use prelude_format::config::{Settings, Theme, parse_color};
use std::borrow::Cow;
use std::sync::LazyLock;

pub mod input;
pub use input::TextInput;

/// 色と大きさ(`~/.config/prelude/theme.toml`。起動のときに 1 回読む)
pub static THEME: LazyLock<Theme> = LazyLock::new(Theme::load);
/// 動きの設定(`~/.config/prelude/settings.toml`)
pub static SETTINGS: LazyLock<Settings> = LazyLock::new(Settings::load);

/// 色(Electron 版の --bg・--panel などと同じ)
pub mod color {
    use super::{THEME, parse_color};
    use gpui::{Hsla, rgb};
    fn of(s: &str) -> Hsla {
        rgb(parse_color(s).unwrap_or(0xffffff)).into()
    }
    pub fn bg() -> Hsla {
        of(&THEME.bg)
    }
    pub fn panel() -> Hsla {
        of(&THEME.panel)
    }
    pub fn panel2() -> Hsla {
        of(&THEME.panel2)
    }
    pub fn line() -> Hsla {
        of(&THEME.line)
    }
    pub fn text() -> Hsla {
        of(&THEME.text)
    }
    pub fn dim() -> Hsla {
        of(&THEME.dim)
    }
    pub fn accent() -> Hsla {
        of(&THEME.accent)
    }
    pub fn danger() -> Hsla {
        of(&THEME.danger)
    }
}

/// 文字の大きさ(Electron 版の --fs)
pub fn font_size() -> Pixels {
    px(THEME.font_size)
}
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
    div().size_full().flex().flex_col().bg(color::bg()).text_color(color::text()).text_size(font_size()).font(f)
}

/// 欄の見出し(「TABS」など。薄い色・少し小さく)
pub fn section_title(text: impl Into<SharedString>) -> Div {
    div().flex().items_center().px(px(6.)).pt(px(6.)).pb(px(2.)).text_color(color::dim()).text_size(px(11.)).child(text.into())
}

/// 一覧の 1 行。active なら反転(白地に青い文字)、そうでなければ乗せると明るくなる
pub fn row(id: impl Into<ElementId>, active: bool) -> Stateful<Div> {
    let d = div().id(id.into()).flex().items_center().gap(px(8.)).h(ROW_HEIGHT).flex_none().px(px(6.)).cursor_pointer().overflow_hidden();
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

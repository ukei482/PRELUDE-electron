//! PRELUDE の設定の画面(GPUI)。`~/.config/prelude/theme.toml`・`settings.toml` を画面から書く。
//! ペインの種類 `settings` のレシピで開く。ランチャーの「Settings」から出る。
//! 色と大きさはプログラムの起動のときに読むので、保存したらサイドバーだけ起動し直す(ほかは次に開いたときから)。
use gpui::{App, Bounds, Context, Entity, SharedString, Window, WindowBounds, WindowOptions, div, prelude::*, px, size};
use gpui_platform::application;
use prelude_format::config::{Settings, Theme, parse_color};
use prelude_ui::{TextInput, color, section_title};

/// 打った文字を設定に入れる。だめならその理由
fn apply(theme: &mut Theme, settings: &mut Settings, key: &str, v: &str) -> Result<(), String> {
    let v = v.trim();
    let num = |lo: f32, hi: f32| v.parse::<f32>().ok().filter(|n| (lo..=hi).contains(n)).ok_or(format!("{key}: {lo} to {hi}"));
    let color = |into: &mut String| {
        parse_color(v).ok_or(format!("{key}: use #rrggbb"))?;
        *into = v.to_string();
        Ok::<(), String>(())
    };
    match key {
        "bg" => color(&mut theme.bg)?,
        "panel" => color(&mut theme.panel)?,
        "panel2" => color(&mut theme.panel2)?,
        "line" => color(&mut theme.line)?,
        "text" => color(&mut theme.text)?,
        "dim" => color(&mut theme.dim)?,
        "accent" => color(&mut theme.accent)?,
        "danger" => color(&mut theme.danger)?,
        "font_size" => theme.font_size = num(8., 40.)?,
        "sidebar_width" => theme.sidebar_width = num(100., 800.)?,
        "search_engine" => {
            if !v.contains("%s") {
                return Err(format!("{key}: must contain %s"));
            }
            settings.search_engine = v.to_string();
        }
        "space_dir" => settings.space_dir = v.to_string(),
        "download_dir" => settings.download_dir = v.to_string(),
        _ => return Err(format!("{key}: unknown")),
    }
    Ok(())
}

fn fields(t: &Theme, s: &Settings) -> Vec<(&'static str, &'static str, String)> {
    vec![
        ("bg", "Background", t.bg.clone()),
        ("panel", "Selection", t.panel.clone()),
        ("panel2", "Hover", t.panel2.clone()),
        ("accent", "Accent", t.accent.clone()),
        ("line", "Line", t.line.clone()),
        ("text", "Text", t.text.clone()),
        ("dim", "Dim text", t.dim.clone()),
        ("danger", "Warning", t.danger.clone()),
        ("font_size", "Font size", t.font_size.to_string()),
        ("sidebar_width", "Sidebar width", t.sidebar_width.to_string()),
        ("search_engine", "Search engine (%s)", s.search_engine.clone()),
        ("space_dir", "Space folder (empty = ~/スペース)", s.space_dir.clone()),
        ("download_dir", "Download folder", s.download_dir.clone()),
    ]
}

struct Page {
    inputs: Vec<(&'static str, &'static str, Entity<TextInput>)>,
    theme: Theme,
    settings: Settings,
    status: Option<(String, bool)>,
}

impl Page {
    fn save(&mut self, cx: &mut Context<Self>) {
        let (mut t, mut s) = (self.theme.clone(), self.settings.clone());
        for (key, _, input) in &self.inputs {
            let v = input.read(cx).text().to_string();
            if let Err(e) = apply(&mut t, &mut s, key, &v) {
                self.status = Some((e, true));
                cx.notify();
                return;
            }
        }
        let r = std::fs::create_dir_all(prelude_format::config::dir())
            .and_then(|_| std::fs::write(Theme::path(), t.to_toml()))
            .and_then(|_| std::fs::write(Settings::path(), s.to_toml()));
        self.status = Some(match r {
            Ok(()) => {
                self.theme = t;
                self.settings = s;
                let _ = std::process::Command::new("prelude").args(["sidebar", "restart"]).spawn();
                ("Saved. The sidebar restarted; other windows change when reopened".into(), false)
            }
            Err(e) => (format!("Cannot save ({e})"), true),
        });
        cx.notify();
    }
}

impl Render for Page {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div().id("list").flex().flex_col().flex_1().min_h(px(0.)).overflow_y_scroll().px(px(26.)).child(section_title("SETTINGS"));
        for (i, (_, label, input)) in self.inputs.iter().enumerate() {
            list = list.child(
                div()
                    .id(("field", i))
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .h(px(36.))
                    .flex_none()
                    .child(div().w(px(280.)).flex_none().child(*label))
                    .child(div().flex_1().min_w(px(0.)).border_b_1().border_color(color::line()).child(input.clone())),
            );
        }
        let hidden = self.settings.show_hidden;
        list = list.child(
            div()
                .id("hidden")
                .flex()
                .items_center()
                .h(px(36.))
                .flex_none()
                .cursor_pointer()
                .hover(|s| s.bg(color::panel2()))
                .child(SharedString::from(format!("[{}] Show hidden files", if hidden { "x" } else { " " })))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.settings.show_hidden = !this.settings.show_hidden;
                    cx.notify();
                })),
        );
        let mut foot = div().flex().items_center().gap(px(14.)).h(px(44.)).flex_none().px(px(26.)).child(
            div()
                .id("save")
                .flex_none()
                .px(px(12.))
                .py(px(4.))
                .cursor_pointer()
                .bg(color::accent())
                .text_color(color::bg())
                .child("Save")
                .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
        );
        if let Some((m, err)) = &self.status {
            foot = foot.child(div().when(*err, |d| d.text_color(color::danger())).child(SharedString::from(m.clone())));
        }
        prelude_ui::root().child(list).child(foot)
    }
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    application().run(|cx: &mut App| {
        prelude_ui::init(cx);
        prelude_ui::input::bind_keys(cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(760.), px(640.)), cx))),
                app_id: Some("prelude-settings".into()),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|cx| {
                    let (theme, settings) = (Theme::load(), Settings::load());
                    let inputs = fields(&theme, &settings)
                        .into_iter()
                        .map(|(key, label, v)| (key, label, cx.new(|cx| TextInput::new(cx, "", v))))
                        .collect();
                    Page { inputs, theme, settings, status: None }
                })
            },
        );
        if let Err(e) = opened {
            eprintln!("prelude-settings: cannot open the window: {e:#}");
            cx.quit();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 打った文字を確かめる() {
        let (mut t, mut s) = (Theme::default(), Settings::default());
        assert!(apply(&mut t, &mut s, "bg", " #112233 ").is_ok());
        assert_eq!(t.bg, "#112233");
        assert!(apply(&mut t, &mut s, "bg", "red").is_err());
        assert_eq!(t.bg, "#112233");
        assert!(apply(&mut t, &mut s, "font_size", "15").is_ok());
        assert!(apply(&mut t, &mut s, "font_size", "99").is_err());
        assert!(apply(&mut t, &mut s, "search_engine", "https://x").is_err());
        assert!(apply(&mut t, &mut s, "search_engine", "https://x/?q=%s").is_ok());
        assert!(apply(&mut t, &mut s, "space_dir", "").is_ok());
    }

    #[test]
    fn すべての項目を入れられる() {
        let (t, s) = (Theme::default(), Settings::default());
        let (mut a, mut b) = (t.clone(), s.clone());
        for (key, _, v) in fields(&t, &s) {
            apply(&mut a, &mut b, key, &v).unwrap();
        }
        assert_eq!((a, b), (t, s));
    }
}

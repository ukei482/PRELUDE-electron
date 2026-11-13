//! 使用者の設定(`~/.config/prelude/theme.toml`・`settings.toml`)。コアと UI が同じものを読む。
//! 読めないファイル・知らない項目・欠けた項目があっても動く(その項目だけ既定になる)。
use std::path::{Path, PathBuf};
use toml::{Table, Value};

/// 設定のフォルダ。`$XDG_CONFIG_HOME/prelude`(無ければ `~/.config/prelude`)
pub fn dir() -> PathBuf {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config"))).unwrap_or_else(|| "/".into()).join("prelude")
}

fn read(path: &Path) -> Table {
    let Ok(text) = std::fs::read_to_string(path) else { return Table::new() };
    text.parse().unwrap_or_else(|e| {
        eprintln!("prelude: ignoring {} ({e})", path.display());
        Table::new()
    })
}

fn string(t: &Table, key: &str, into: &mut String) {
    if let Some(s) = t.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()) {
        *into = s.to_string();
    }
}

fn quote(s: &str) -> String {
    Value::String(s.to_string()).to_string()
}

/// 色(`#rrggbb`)と大きさ
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub bg: String,
    pub panel: String,
    pub panel2: String,
    pub line: String,
    pub text: String,
    pub dim: String,
    pub accent: String,
    pub danger: String,
    pub font_size: f32,
    pub sidebar_width: f32,
}

impl Default for Theme {
    fn default() -> Theme {
        let s = |x: &str| x.to_string();
        Theme {
            bg: s("#1926a3"),
            panel: s("#2b3dc8"),
            panel2: s("#3a50e0"),
            line: s("#ffffff"),
            text: s("#f0f3ff"),
            dim: s("#aab4ee"),
            accent: s("#ffffff"),
            danger: s("#ff8080"),
            font_size: 13.,
            sidebar_width: 220.,
        }
    }
}

impl Theme {
    pub fn path() -> PathBuf {
        dir().join("theme.toml")
    }
    pub fn load() -> Theme {
        Theme::from_table(&read(&Theme::path()))
    }
    pub fn parse(text: &str) -> Theme {
        Theme::from_table(&text.parse().unwrap_or_default())
    }
    fn from_table(t: &Table) -> Theme {
        let mut out = Theme::default();
        for (k, v) in [
            ("bg", &mut out.bg),
            ("panel", &mut out.panel),
            ("panel2", &mut out.panel2),
            ("line", &mut out.line),
            ("text", &mut out.text),
            ("dim", &mut out.dim),
            ("accent", &mut out.accent),
            ("danger", &mut out.danger),
        ] {
            let mut s = String::new();
            string(t, k, &mut s);
            if parse_color(&s).is_some() {
                *v = s;
            }
        }
        let num = |k: &str| t.get(k).and_then(|v| v.as_float().or(v.as_integer().map(|i| i as f64))).map(|f| f as f32);
        out.font_size = num("font_size").filter(|n| (8.0..=40.0).contains(n)).unwrap_or(out.font_size);
        out.sidebar_width = num("sidebar_width").filter(|n| (100.0..=800.0).contains(n)).unwrap_or(out.sidebar_width);
        out
    }
    pub fn to_toml(&self) -> String {
        let mut s = String::from("# PRELUDE の色と大きさ\n");
        for (k, v) in [
            ("bg", &self.bg),
            ("panel", &self.panel),
            ("panel2", &self.panel2),
            ("line", &self.line),
            ("text", &self.text),
            ("dim", &self.dim),
            ("accent", &self.accent),
            ("danger", &self.danger),
        ] {
            s.push_str(&format!("{k} = {}\n", quote(v)));
        }
        s.push_str(&format!("font_size = {}\nsidebar_width = {}\n", self.font_size, self.sidebar_width));
        s
    }
}

/// `#rrggbb` → 数
pub fn parse_color(s: &str) -> Option<u32> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    u32::from_str_radix(h, 16).ok()
}

/// 動きの設定
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// 検索の URL(`%s` が検索の言葉)
    pub search_engine: String,
    /// ファイラで隠しファイルを最初から出す
    pub show_hidden: bool,
    /// スペースのフォルダ。空なら `~/スペース`
    pub space_dir: String,
    /// ダウンロードの保存先の既定(paths の download が無いとき)。空なら Chromium の既定
    pub download_dir: String,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            search_engine: "https://www.google.com/search?q=%s".into(),
            show_hidden: false,
            space_dir: String::new(),
            download_dir: String::new(),
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        dir().join("settings.toml")
    }
    pub fn load() -> Settings {
        Settings::from_config(&crate::settings::Config::load())
    }
    pub fn parse(text: &str) -> Settings {
        Settings::from_table(&text.parse().unwrap_or_default())
    }
    fn from_table(t: &Table) -> Settings {
        // 台帳(settings.rs)で確かめて読む。組み合わせ(profile)は環境を見ずに決める(ここで読む 4 つの項目は組み合わせで変えない)
        Settings::from_config(&crate::settings::Config::from_text(&t.to_string(), &|_| None))
    }

    /// 層を重ねた設定から(コアは環境も見た Config を持つ)
    pub fn from_config(c: &crate::settings::Config) -> Settings {
        Settings {
            search_engine: c.str("search_engine").to_string(),
            show_hidden: c.bool("show_hidden"),
            space_dir: c.str("space_dir").to_string(),
            download_dir: c.str("download_dir").to_string(),
        }
    }
    pub fn to_toml(&self) -> String {
        format!(
            "# PRELUDE の動き\nsearch_engine = {}\nshow_hidden = {}\nspace_dir = {}\ndownload_dir = {}\n",
            quote(&self.search_engine),
            self.show_hidden,
            quote(&self.space_dir),
            quote(&self.download_dir)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 書いて読むと同じ() {
        let t = Theme { bg: "#524832".into(), font_size: 15., sidebar_width: 300., ..Theme::default() };
        assert_eq!(Theme::parse(&t.to_toml()), t);
        let s = Settings { show_hidden: true, space_dir: "~/s".into(), ..Settings::default() };
        assert_eq!(Settings::parse(&s.to_toml()), s);
    }

    #[test]
    fn 読めない項目は既定になる() {
        let t = Theme::parse("bg = \"red\"\npanel = \"#112233\"\nfont_size = 900\nnew = 1\n");
        assert_eq!(t.bg, Theme::default().bg);
        assert_eq!(t.panel, "#112233");
        assert_eq!(t.font_size, 13.);
        assert_eq!(Theme::parse("これは = TOML ではない ["), Theme::default());
        assert_eq!(Settings::parse("search_engine = \"https://x\"\n").search_engine, Settings::default().search_engine);
    }

    #[test]
    fn 色の文字() {
        assert_eq!(parse_color("#524832"), Some(0x524832));
        assert_eq!(parse_color("524832"), None);
        assert_eq!(parse_color("#fff"), None);
        assert_eq!(parse_color("#gggggg"), None);
    }
}

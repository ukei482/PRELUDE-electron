//! Electron 版からの引き継ぎ(docs/redesign-rust.md の移行の「引き継ぎ」。段階 8)。最初の 1 回だけ動く。
//!
//! - `config.json` の色・大きさを `theme.toml` に、動きを `settings.toml` に書き出す。既にあるファイルは上書きしない
//! - `~/ブックマーク/*.prelude` を今の形で `<スペース>/Main` にコピーする。同じ名前があれば触らない
//! - Rust 版で無くなる設定は `migrated.txt` に一覧にする。このファイルが「済んだ」印を兼ねる
//! - Electron 版のファイルは変えない(いつでも戻れる)
use prelude_format::config::{Settings, Theme};
use prelude_format::Bookmark;
use serde_json::Value;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Rust 版で無くなる設定と、代わりになるもの
const GONE: [(&str, &str); 14] = [
    ("appearance.wallpaper", "壁紙は rice(swaybg など)で決める"),
    ("behavior.startKind", "新しいタブはランチャーで種類を選ぶ"),
    ("behavior.appOpen", "アプリはいつもペインで開く"),
    ("behavior.fileOpen", "ファイルはアプリで開く"),
    ("behavior.viewInside", "Web のペインの中で開く拡張子は無くなった"),
    ("behavior.homepage", "新しい Web のペインはランチャーで URL を入れる"),
    ("behavior.chromiumSites", "Web のペインはいつも Chromium"),
    ("behavior.startPath", "最初のフォルダは paths の files"),
    ("behavior.terminal", "console のレシピ(~/.config/prelude/panes)で替える"),
    ("behavior.sidebarMode", "サイドバーは別のプログラム。出さないなら services.prelude.ui.sidebar"),
    ("behavior.startFullscreen", "全画面はコンポジタで決める"),
    ("behavior.subDisplays", "画面はコンポジタで決める"),
    ("tabs", "リンクの開き方(新しいタブで開く)に固定"),
    ("shortcuts", "キーはコンポジタの設定で決める(`prelude` のコマンドを呼ぶ)"),
];

fn str_of<'a>(v: &'a Value, path: &str) -> Option<&'a str> {
    v.pointer(path)?.as_str().filter(|s| !s.is_empty())
}

/// `config.json` から色と大きさを読む。無い項目は既定のまま
pub fn theme_of(cfg: &Value) -> Theme {
    let mut t = Theme::default();
    for (k, v) in [
        ("bg", &mut t.bg),
        ("panel", &mut t.panel),
        ("panel2", &mut t.panel2),
        ("line", &mut t.line),
        ("text", &mut t.text),
        ("dim", &mut t.dim),
        ("accent", &mut t.accent),
        ("danger", &mut t.danger),
    ] {
        if let Some(s) = str_of(cfg, &format!("/appearance/{k}")).filter(|s| prelude_format::config::parse_color(s).is_some()) {
            *v = s.to_string();
        }
    }
    let num = |k: &str| cfg.pointer(&format!("/appearance/{k}")).and_then(Value::as_f64).map(|f| f as f32);
    t.font_size = num("fontSize").unwrap_or(t.font_size);
    t.sidebar_width = num("sidebarWidth").unwrap_or(t.sidebar_width);
    t
}

/// `config.json` から動きを読む
pub fn settings_of(cfg: &Value) -> Settings {
    let mut s = Settings::default();
    if let Some(e) = str_of(cfg, "/behavior/searchEngine").filter(|e| e.contains("%s")) {
        s.search_engine = e.to_string();
    }
    s.show_hidden = cfg.pointer("/behavior/showHidden").and_then(Value::as_bool).unwrap_or(false);
    s.space_dir = str_of(cfg, "/behavior/spaceDir").unwrap_or("").to_string();
    s.download_dir = str_of(cfg, "/behavior/downloadDir").unwrap_or("").to_string();
    s
}

/// 引き継ぐ場所
pub struct Places {
    /// `~/.config/prelude`
    pub config_dir: PathBuf,
    /// Electron 版の `config.json`
    pub electron: PathBuf,
    /// `~/ブックマーク`
    pub bookmarks: PathBuf,
    /// 既定のスペース(`~/スペース/Main`)。`config.json` の spaceDir があれば、そちらの Main を使う
    pub main_space: PathBuf,
    pub home: String,
}

impl Places {
    pub fn of(home: &str) -> Places {
        let h = Path::new(home);
        let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        Places {
            config_dir: prelude_format::config::dir(),
            electron: env("XDG_CONFIG_HOME").unwrap_or_else(|| h.join(".config")).join("PRELUDE-electron/config.json"),
            bookmarks: h.join("ブックマーク"),
            main_space: h.join("スペース").join(crate::state::SPACE_DEFAULT),
            home: home.to_string(),
        }
    }
}

fn write_new(path: &Path, text: &str, log: &mut String) -> std::io::Result<()> {
    if path.exists() {
        let _ = writeln!(log, "- {} は既にあるので触っていません", path.display());
        return Ok(());
    }
    std::fs::write(path, text)?;
    let _ = writeln!(log, "- {} を作りました", path.display());
    Ok(())
}

/// 引き継ぐ。済んでいれば、引き継ぐものが無ければ何もしない。動いたら報告の文を返す
pub fn run(p: &Places) -> std::io::Result<Option<String>> {
    let marker = p.config_dir.join("migrated.txt");
    if marker.exists() {
        return Ok(None);
    }
    let Ok(text) = std::fs::read_to_string(&p.electron) else { return Ok(None) };
    let Ok(cfg) = serde_json::from_str::<Value>(&text) else {
        return Err(std::io::Error::other(format!("{} is not JSON", p.electron.display())));
    };
    std::fs::create_dir_all(&p.config_dir)?;
    let mut log =
        String::from("# Electron 版からの引き継ぎ\n\n元のファイルは変えていません。Electron 版にはいつでも戻れます。\n\n## 設定\n");
    write_new(&p.config_dir.join("theme.toml"), &theme_of(&cfg).to_toml(), &mut log)?;
    write_new(&p.config_dir.join("settings.toml"), &settings_of(&cfg).to_toml(), &mut log)?;

    log.push_str("\n## ブックマーク\n");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&p.bookmarks)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|f| f.extension().is_some_and(|x| x == "prelude")).collect())
        .unwrap_or_default();
    files.sort();
    let dir = settings_of(&cfg).space_dir;
    let main_space = if dir.is_empty() {
        p.main_space.clone()
    } else {
        PathBuf::from(prelude_format::abs_path(&dir, &p.home, &p.home)).join(crate::state::SPACE_DEFAULT)
    };
    for f in &files {
        let name = f.file_name().unwrap_or_default();
        let dest = main_space.join(name);
        let parsed = std::fs::read_to_string(f).ok().and_then(|t| Bookmark::parse(&t, &p.home).ok());
        match parsed {
            _ if dest.exists() => {
                let _ = writeln!(log, "- {}: 同じ名前があるので触っていません", name.to_string_lossy());
            }
            Some(b) => {
                std::fs::create_dir_all(&main_space)?;
                std::fs::write(&dest, b.to_toml(&p.home))?;
                let _ = writeln!(log, "- {} をコピーしました", dest.display());
            }
            None => {
                let _ = writeln!(log, "- {}: 読めなかったのでコピーしていません", f.display());
            }
        }
    }
    if files.is_empty() {
        log.push_str("- コピーするものはありません\n");
    }
    if cfg.get("bookmarks").and_then(Value::as_array).is_some_and(|b| !b.is_empty()) {
        log.push_str("- `config.json` の bookmarks はまだ移っていません。Electron 版を起動すると `.prelude` になります\n");
    }

    log.push_str("\n## Rust 版で無くなる設定\n");
    for (k, why) in GONE {
        let _ = writeln!(log, "- `{k}`: {why}");
    }
    log.push_str("\n## 引き継がないもの\n- 前回のタブ(スペースのブックマークから開き直せます)\n- Electron 版の中でログインしていたサイト(一度ログインし直してください)\n");
    std::fs::write(&marker, &log)?;
    Ok(Some(log))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn places(name: &str) -> Places {
        let d = std::env::temp_dir().join(format!("prelude-migrate-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("e")).unwrap();
        Places {
            config_dir: d.join("c"),
            electron: d.join("e/config.json"),
            bookmarks: d.join("b"),
            main_space: d.join("s/Main"),
            home: d.to_string_lossy().into_owned(),
        }
    }
    fn config() -> Value {
        json!({
            "appearance": { "bg": "#524832", "panel": "#2b3dc8", "wallpaper": "/w.png", "fontSize": 15, "sidebarWidth": 260, "accent": "red" },
            "behavior": { "searchEngine": "https://duck.example/?q=%s", "showHidden": true, "spaceDir": "", "downloadDir": "~/DL" },
            "bookmarks": [],
        })
    }

    #[test]
    fn 色と動きを読む() {
        let t = theme_of(&config());
        assert_eq!((t.bg.as_str(), t.font_size, t.sidebar_width), ("#524832", 15., 260.));
        assert_eq!(t.accent, Theme::default().accent); // 色でないものは既定のまま
        let s = settings_of(&config());
        assert_eq!(s.search_engine, "https://duck.example/?q=%s");
        assert!(s.show_hidden);
        assert_eq!(s.download_dir, "~/DL");
        assert_eq!(theme_of(&json!({})), Theme::default());
        assert_eq!(settings_of(&json!({})), Settings::default());
    }

    #[test]
    fn 一度だけ動き_元は変えない() {
        let p = places("once");
        // Electron 版が無ければ何もしない
        assert_eq!(run(&p).unwrap(), None);
        assert!(!p.config_dir.exists());
        std::fs::write(&p.electron, config().to_string()).unwrap();
        let before = std::fs::read(&p.electron).unwrap();
        std::fs::create_dir_all(&p.bookmarks).unwrap();
        std::fs::write(
            p.bookmarks.join("old.prelude"),
            r#"{"prelude":"bookmark","version":1,"title":"古い","root":{"k":"leaf","type":"field","path":"~"}}"#,
        )
        .unwrap();
        std::fs::write(p.bookmarks.join("bad.prelude"), "壊れている").unwrap();
        let log = run(&p).unwrap().unwrap();
        assert!(log.contains("chromiumSites"), "{log}");
        assert!(log.contains("bad.prelude"), "{log}");
        assert!(std::fs::read_to_string(p.config_dir.join("theme.toml")).unwrap().contains("#524832"));
        assert!(Settings::parse(&std::fs::read_to_string(p.config_dir.join("settings.toml")).unwrap()).show_hidden);
        let b = Bookmark::parse(&std::fs::read_to_string(p.main_space.join("old.prelude")).unwrap(), &p.home).unwrap();
        assert_eq!(b.title, "古い");
        assert_eq!(std::fs::read(&p.electron).unwrap(), before);
        assert!(p.bookmarks.join("old.prelude").exists());
        // 2 回目は何もしない
        std::fs::remove_file(p.config_dir.join("theme.toml")).unwrap();
        assert_eq!(run(&p).unwrap(), None);
        assert!(!p.config_dir.join("theme.toml").exists());
    }

    #[test]
    fn 既にあるものは上書きしない() {
        let p = places("keep");
        std::fs::write(&p.electron, config().to_string()).unwrap();
        std::fs::create_dir_all(&p.config_dir).unwrap();
        std::fs::write(p.config_dir.join("theme.toml"), "bg = \"#000000\"\n").unwrap();
        std::fs::create_dir_all(&p.bookmarks).unwrap();
        std::fs::create_dir_all(&p.main_space).unwrap();
        std::fs::write(
            p.bookmarks.join("a.prelude"),
            r#"{"prelude":"bookmark","version":1,"title":"古","root":{"k":"leaf","type":"field","path":"~"}}"#,
        )
        .unwrap();
        std::fs::write(p.main_space.join("a.prelude"), "自分の").unwrap();
        run(&p).unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(p.config_dir.join("theme.toml")).unwrap(), "bg = \"#000000\"\n");
        assert_eq!(std::fs::read_to_string(p.main_space.join("a.prelude")).unwrap(), "自分の");
    }

    #[test]
    fn 壊れた_config_json_は失敗にして印を残さない() {
        let p = places("broken");
        std::fs::write(&p.electron, "{").unwrap();
        assert!(run(&p).is_err());
        assert!(!p.config_dir.join("migrated.txt").exists());
    }
}

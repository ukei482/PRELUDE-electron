//! `prelude browser [--mark <印>] <URL>`: 設定のブラウザ(`browser`)で URL を新しい窓に開く。ブラウザごとの渡し方は
//! ブラウザの表(`prelude_format::browser`)だけが知る。Web のペインの既定のレシピ `web` はこれで開く(docs/redesign-rust.md の「Web のペイン」)。
//!
//! `--mark` を付けると、題名に印を出すだけのページ(`prelude-pane.html`)で開き、ページが自分で本当の URL へ移る。
//! 窓のつなぎは、その印で窓をペインと見分ける。このプロセスはブラウザに置き換わる
use prelude_format::browser::{self, Chosen};
use prelude_format::settings::Config;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;

/// 引数を読む → (印, URL)
pub fn parse(args: &[&str]) -> Result<(Option<String>, String), String> {
    let usage = "usage: prelude browser [--mark <mark>] <url>";
    match args {
        ["--mark", mark, url] => Ok((Some(mark.to_string()), url.to_string())),
        [url] if !url.starts_with("--") => Ok((None, url.to_string())),
        _ => Err(usage.into()),
    }
}

/// 印のページを置く場所。snap 版のブラウザは隠しフォルダを読めないので `~/snap/<名前>/common`、ほかは `$XDG_CACHE_HOME/prelude`
fn page_dir(b: &Chosen, home: &str, cache: Option<String>) -> PathBuf {
    if b.is_snap() {
        let name = std::path::Path::new(&b.program).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        return PathBuf::from(home).join("snap").join(name).join("common");
    }
    let cache = cache.filter(|s| !s.is_empty()).unwrap_or_else(|| format!("{home}/.cache"));
    PathBuf::from(cache).join("prelude")
}

pub fn run(args: &[&str], home: &str) -> Result<(), String> {
    let (mark, url) = parse(args)?;
    let setting = Config::load().str("browser").to_string();
    let Some(b) = browser::choose(&setting, &std::env::var("PATH").unwrap_or_default()) else {
        return Err("No browser found. Install Chromium or Firefox, or `prelude config set browser <name>`".into());
    };
    let open = match mark {
        Some(key) => {
            let dir = page_dir(&b, home, std::env::var("XDG_CACHE_HOME").ok());
            let page = dir.join("prelude-pane.html");
            std::fs::create_dir_all(&dir)
                .and_then(|_| std::fs::write(&page, browser::PANE_PAGE))
                .map_err(|e| format!("Cannot write {} ({e})", page.display()))?;
            browser::pane_url(&page, &key, &url)
        }
        None => url,
    };
    let argv = b.argv(&open);
    let e = std::process::Command::new(&argv[0]).args(&argv[1..]).exec();
    Err(format!("{}: {e}", argv[0]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 引数() {
        assert_eq!(parse(&["https://a/"]), Ok((None, "https://a/".into())));
        assert_eq!(parse(&["--mark", ":5", "https://a/"]), Ok((Some(":5".into()), "https://a/".into())));
        assert_eq!(parse(&["--mark", ":5", ""]), Ok((Some(":5".into()), String::new())));
        assert!(parse(&[]).is_err());
        assert!(parse(&["--mark", ":5"]).is_err());
    }

    #[test]
    fn 印のページの場所() {
        let c = Chosen { program: "chromium".into(), browser: None };
        assert_eq!(page_dir(&c, "/home/u", Some("/c".into())), PathBuf::from("/c/prelude"));
        assert_eq!(page_dir(&c, "/home/u", None), PathBuf::from("/home/u/.cache/prelude"));
        let s = Chosen { program: "/snap/bin/firefox".into(), browser: None };
        assert_eq!(page_dir(&s, "/home/u", None), PathBuf::from("/home/u/snap/firefox/common"));
    }
}

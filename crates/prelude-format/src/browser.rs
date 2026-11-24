//! ブラウザの表(docs/redesign-rust.md の「Web のペイン」)。ブラウザごとに違う、URL を新しい窓で開く渡し方と窓の名前(app_id)を、
//! PRELUDE はここだけで知る。`prelude browser` が起動し、コアはレシピの窓の名前 `@browser` をここから決める。
//!
//! Web のペインの窓を見分けるため、ペインは題名に印を出すだけのページ([`PANE_PAGE`])で開き、ページが自分で本当の URL へ移る。
//! ブラウザは窓が増えても同じプロセスのままなので、起動したプロセスでは見分けられないため(拡張機能は使わない)
use std::path::Path;

/// ブラウザ 1 つ
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Browser {
    /// コマンドの名前
    pub name: &'static str,
    /// 窓の名前(Wayland の app_id・X11 のクラス)。アイコンを探すのに使う
    pub class: &'static str,
    /// URL を新しい窓で開く語(`{}` が URL)
    open: &'static [&'static str],
    /// 履歴のファイルの置き場所(PRELUDE を通さずに開いた窓の URL を、ブックマークに保存するときに引く)
    pub history: History,
}

/// ブラウザの履歴のファイルの置き場所
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum History {
    /// Chromium 系: 設定のフォルダ(`$XDG_CONFIG_HOME` から)。プロファイルごとの `History`(表 `urls`)
    Chromium(&'static str),
    /// Firefox 系: プロファイルを置くフォルダ(ホームから)。プロファイルごとの `places.sqlite`(表 `moz_places`)
    Firefox(&'static [&'static str]),
}

/// Chromium 系: アドレスバーとタブの無い窓(PRELUDE のタブと二重にならない)
const APP: &[&str] = &["--app={}"];
/// Firefox 系: 新しい窓
const NEW_WINDOW: &[&str] = &["--new-window", "{}"];

/// 知っているブラウザ(auto で探す順)。ペインに合う Chromium 系を先に置く
pub const BROWSERS: [Browser; 10] = [
    Browser { name: "chromium", class: "chromium", open: APP, history: CHROMIUM },
    Browser { name: "chromium-browser", class: "chromium-browser", open: APP, history: CHROMIUM },
    Browser { name: "google-chrome-stable", class: "google-chrome", open: APP, history: CHROME },
    Browser { name: "google-chrome", class: "google-chrome", open: APP, history: CHROME },
    Browser { name: "brave", class: "brave-browser", open: APP, history: BRAVE },
    Browser { name: "brave-browser", class: "brave-browser", open: APP, history: BRAVE },
    Browser { name: "vivaldi", class: "vivaldi-stable", open: APP, history: History::Chromium("vivaldi") },
    Browser { name: "microsoft-edge", class: "microsoft-edge", open: APP, history: History::Chromium("microsoft-edge") },
    Browser {
        name: "firefox",
        class: "firefox",
        open: NEW_WINDOW,
        history: History::Firefox(&[".mozilla/firefox", ".config/mozilla/firefox"]),
    },
    Browser {
        name: "librewolf",
        class: "librewolf",
        open: NEW_WINDOW,
        history: History::Firefox(&[".librewolf", ".config/librewolf/librewolf"]),
    },
];
const CHROMIUM: History = History::Chromium("chromium");
const CHROME: History = History::Chromium("google-chrome");
const BRAVE: History = History::Chromium("BraveSoftware/Brave-Browser");

/// 窓の名前(class・app_id)のブラウザ
pub fn by_class(class: &str) -> Option<Browser> {
    BROWSERS.iter().find(|b| b.class.eq_ignore_ascii_case(class) || b.name.eq_ignore_ascii_case(class)).copied()
}

impl Browser {
    /// 履歴のファイル(プロファイルごと)。home はホーム、config は `$XDG_CONFIG_HOME`
    pub fn history_files(&self, home: &Path, config: &Path) -> Vec<std::path::PathBuf> {
        let (roots, file) = match self.history {
            History::Chromium(dir) => (vec![config.join(dir)], "History"),
            History::Firefox(dirs) => (dirs.iter().map(|d| home.join(d)).collect(), "places.sqlite"),
        };
        let mut out: Vec<_> = roots
            .iter()
            .filter_map(|r| std::fs::read_dir(r).ok())
            .flatten()
            .flatten()
            .map(|e| e.path().join(file))
            .filter(|p| p.is_file())
            .collect();
        out.sort();
        out
    }
}

/// Chromium 系で `--app=<URL>` として開いた窓(Omarchy の Web アプリなど)の名前は `chrome-<URL の名前>-<プロファイル>`
/// (例 `chrome-hey.com__-Default`)。その `<URL の名前>`。ほかの窓なら None
pub fn app_window_name(class: &str) -> Option<&str> {
    let (prefix, rest) = class.split_once('-')?;
    if !["chrome", "chromium", "brave", "msedge", "vivaldi"].iter().any(|p| p.eq_ignore_ascii_case(prefix)) {
        return None;
    }
    let (name, profile) = rest.rsplit_once('-')?;
    (!name.is_empty() && !profile.is_empty()).then_some(name)
}

/// Chromium が URL から `--app` の窓の名前を作る形(ホスト + `_` + パス。`/` などのファイル名に使えない文字は `_`)
fn app_name_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let rest = &rest[..rest.find(['?', '#']).unwrap_or(rest.len())];
    let (host, path) = rest.find('/').map_or((rest, "/"), |i| (&rest[..i], &rest[i..]));
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host).to_lowercase();
    let bad = |c: char| matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control();
    Some(format!("{host}_{path}").chars().map(|c| if bad(c) { '_' } else { c }).collect())
}

/// URL が `--app` の窓の名前([`app_window_name`])に合うか。両端の `_` は比べない(ブラウザの版で付き方が違う)
pub fn app_url_matches(url: &str, name: &str) -> bool {
    app_name_of(url).is_some_and(|n| n.trim_matches('_') == name.trim_matches('_'))
}

/// 窓の名前だけから URL を戻す(開いた .desktop が見つからないとき)。パスの `_` は `/` にする(元の `_` は戻せない)
pub fn app_url_guess(name: &str) -> Option<String> {
    let (host, path) = name.split_once('_').unwrap_or((name, ""));
    if !host.contains('.') || !host.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') {
        return None;
    }
    let path = path.replace('_', "/");
    let path = if path.starts_with('/') { path } else { format!("/{path}") };
    Some(format!("https://{host}{path}"))
}

/// 起動コマンドの中の http(s) の URL(`--app=<URL>`・`omarchy-launch-webapp <URL>` など)
pub fn url_in_command(cmd: &str) -> Option<&str> {
    cmd.split_whitespace().map(|w| w.trim_matches(['"', '\'']).trim_start_matches("--app=").trim_matches(['"', '\''])).find(|w| {
        let l = w.to_ascii_lowercase();
        l.starts_with("https://") || l.starts_with("http://")
    })
}

/// 窓の題名から、ページの題名の候補(長い順)。窓の題名は「ページの題名 - ブラウザの名前」のことが多いので、後ろの「 - …」を 2 つまで外す
pub fn page_titles(window: &str) -> Vec<String> {
    let mut t = window.trim();
    let mut out = vec![];
    for _ in 0..3 {
        if t.is_empty() {
            break;
        }
        out.push(t.to_string());
        let Some(i) = [" - ", " — ", " – "].iter().filter_map(|s| t.rfind(s)).max() else { break };
        t = t[..i].trim_end();
    }
    out
}

/// 使うブラウザ: 設定の値(auto か、名前・パス)と PATH から。知らないブラウザなら URL だけを渡す
#[derive(Debug, Clone, PartialEq)]
pub struct Chosen {
    /// 起動するもの(名前かパス)
    pub program: String,
    pub browser: Option<Browser>,
}

fn in_path(name: &str, path: &str) -> Option<std::path::PathBuf> {
    path.split(':').filter(|d| !d.is_empty()).map(|d| Path::new(d).join(name)).find(|p| p.is_file())
}

/// 設定の browser と PATH から決める。auto で見つからなければ None。
/// /snap/bin は systemd のユーザーのサービスの PATH に無いことがあるので、auto のときは足して探す
pub fn choose(setting: &str, path: &str) -> Option<Chosen> {
    let setting = setting.trim();
    if setting.is_empty() || setting == "auto" {
        let path = format!("{path}:/snap/bin");
        return BROWSERS.iter().find_map(|b| {
            let p = in_path(b.name, &path)?;
            Some(Chosen { program: p.to_string_lossy().into_owned(), browser: Some(*b) })
        });
    }
    let base = Path::new(setting).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Some(Chosen { program: setting.to_string(), browser: BROWSERS.iter().find(|b| b.name == base).copied() })
}

impl Chosen {
    /// 窓の名前(知らないブラウザなら名前そのもの)
    pub fn class(&self) -> String {
        match self.browser {
            Some(b) => b.class.to_string(),
            None => Path::new(&self.program).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        }
    }

    /// URL を新しい窓で開く語の並び
    pub fn argv(&self, url: &str) -> Vec<String> {
        let open: &[&str] = self.browser.map_or(&["{}"], |b| b.open);
        std::iter::once(self.program.clone()).chain(open.iter().map(|a| a.replace("{}", url))).collect()
    }

    /// snap 版か(起動するものの先が /snap/ の中)。snap 版は隠しフォルダのファイルを読めない
    pub fn is_snap(&self) -> bool {
        let p = Path::new(&self.program);
        p.starts_with("/snap/") || std::fs::canonicalize(p).is_ok_and(|p| p.starts_with("/snap/"))
    }
}

/// 窓の題名に出す印。key はコアの印とペインの id(`<印>:<ペイン>`)
pub fn marker(key: &str) -> String {
    format!("PRELUDE-PANE:{key}")
}

/// 印のページから本当の URL へ移るまでの時間(ミリ秒)。その間に窓のつなぎが題名の印で窓を見分ける
pub const PANE_DELAY_MS: u32 = 1000;

/// 題名に印を出し、少し後に本当の URL へ移るだけのページ。場所は `#<key>|<URL を encodeURIComponent したもの>`。
/// 移るのは http(s) だけ(人からもらったブックマークの `javascript:` などを、ローカルのページの中で動かさない)
pub const PANE_PAGE: &str = "<!doctype html><meta charset=\"utf-8\"><title>PRELUDE-PANE</title>\
<script>\
var h = location.hash.slice(1), i = h.indexOf('|'), key = i < 0 ? h : h.slice(0, i), url = i < 0 ? '' : decodeURIComponent(h.slice(i + 1));\
document.title = 'PRELUDE-PANE:' + decodeURIComponent(key);\
if (/^https?:\\/\\//i.test(url)) setTimeout(function () { location.replace(url); }, 1000);\
</script><body style=\"background:#fff\">\n";

/// 印のページの場所(ファイルの URL)。page は印のページのファイル
pub fn pane_url(page: &Path, key: &str, url: &str) -> String {
    format!("file://{}#{}|{}", page.display(), encode(key), encode(url))
}

/// encodeURIComponent と同じ(英数字と `-_.!~*'()` 以外を %XX にする)
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn ブラウザごとの開き方() {
        let argv = |name: &str| choose(name, "").unwrap().argv("https://a/");
        assert_eq!(argv("chromium"), s(&["chromium", "--app=https://a/"]));
        assert_eq!(argv("/usr/bin/google-chrome-stable"), s(&["/usr/bin/google-chrome-stable", "--app=https://a/"]));
        assert_eq!(argv("firefox"), s(&["firefox", "--new-window", "https://a/"]));
        // 知らないブラウザは URL だけ
        let x = choose("qutebrowser", "").unwrap();
        assert_eq!((x.argv("https://a/"), x.class()), (s(&["qutebrowser", "https://a/"]), "qutebrowser".to_string()));
        assert_eq!(choose("google-chrome-stable", "").unwrap().class(), "google-chrome");
    }

    #[test]
    fn auto_は表の順に_path_から探す() {
        let d = std::env::temp_dir().join(format!("prelude-browser-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("firefox"), "").unwrap();
        let path = d.to_string_lossy().into_owned();
        assert_eq!(choose("auto", &path).unwrap().browser.unwrap().name, "firefox");
        std::fs::write(d.join("chromium"), "").unwrap();
        let c = choose("", &path).unwrap();
        assert_eq!((c.program, c.browser.unwrap().name), (d.join("chromium").to_string_lossy().into_owned(), "chromium"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn web_アプリの窓の名前と_url() {
        assert_eq!(app_window_name("chrome-hey.com__-Default"), Some("hey.com__"));
        assert_eq!(app_window_name("brave-mail.google.com__mail_u_0_-Profile 1"), Some("mail.google.com__mail_u_0_"));
        assert_eq!(app_window_name("chromium"), None);
        assert_eq!(app_window_name("org.kde.okular"), None);
        assert!(app_url_matches("https://hey.com", "hey.com__"));
        assert!(app_url_matches("https://HEY.com/", "hey.com"));
        assert!(app_url_matches("https://mail.google.com/mail/u/0/?tab=x", "mail.google.com__mail_u_0_"));
        assert!(!app_url_matches("https://hey.com/x", "hey.com__"));
        assert_eq!(app_url_guess("hey.com__").as_deref(), Some("https://hey.com/"));
        assert_eq!(app_url_guess("web.whatsapp.com").as_deref(), Some("https://web.whatsapp.com/"));
        assert_eq!(app_url_guess("mail.google.com__mail_u_0_").as_deref(), Some("https://mail.google.com/mail/u/0/"));
        // PRELUDE の印のページ(file://)などは戻さない
        assert_eq!(app_url_guess("__home_u_.cache_prelude_prelude-pane.html"), None);
        assert_eq!(url_in_command("omarchy-launch-webapp \"https://hey.com\""), Some("https://hey.com"));
        assert_eq!(url_in_command("chromium --app=https://x.com/ --foo"), Some("https://x.com/"));
        assert_eq!(url_in_command("chromium %U"), None);
    }

    #[test]
    fn 窓の題名から_ページの題名() {
        assert_eq!(page_titles("GitHub - Chromium"), s(&["GitHub - Chromium", "GitHub"]));
        assert_eq!(page_titles("a - b — Mozilla Firefox"), s(&["a - b — Mozilla Firefox", "a - b", "a"]));
        assert_eq!(page_titles(" 題名 "), s(&["題名"]));
        assert!(page_titles("").is_empty());
        assert_eq!(by_class("Chromium").map(|b| b.name), Some("chromium"));
        assert_eq!(by_class("brave-browser").map(|b| b.history), Some(BRAVE));
        assert_eq!(by_class("foot"), None);
    }

    #[test]
    fn 印のページの場所() {
        assert_eq!(marker(".d1:5"), "PRELUDE-PANE:.d1:5");
        assert_eq!(
            pane_url(Path::new("/c/prelude-pane.html"), ":5", "https://a.jp/?q=日本&x=1#y"),
            "file:///c/prelude-pane.html#%3A5|https%3A%2F%2Fa.jp%2F%3Fq%3D%E6%97%A5%E6%9C%AC%26x%3D1%23y"
        );
        assert!(PANE_PAGE.contains(&PANE_DELAY_MS.to_string()));
    }
}

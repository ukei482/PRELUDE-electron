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
}

/// Chromium 系: アドレスバーとタブの無い窓(PRELUDE のタブと二重にならない)
const APP: &[&str] = &["--app={}"];
/// Firefox 系: 新しい窓
const NEW_WINDOW: &[&str] = &["--new-window", "{}"];

/// 知っているブラウザ(auto で探す順)。ペインに合う Chromium 系を先に置く
pub const BROWSERS: [Browser; 10] = [
    Browser { name: "chromium", class: "chromium", open: APP },
    Browser { name: "chromium-browser", class: "chromium-browser", open: APP },
    Browser { name: "google-chrome-stable", class: "google-chrome", open: APP },
    Browser { name: "google-chrome", class: "google-chrome", open: APP },
    Browser { name: "brave", class: "brave-browser", open: APP },
    Browser { name: "brave-browser", class: "brave-browser", open: APP },
    Browser { name: "vivaldi", class: "vivaldi-stable", open: APP },
    Browser { name: "microsoft-edge", class: "microsoft-edge", open: APP },
    Browser { name: "firefox", class: "firefox", open: NEW_WINDOW },
    Browser { name: "librewolf", class: "librewolf", open: NEW_WINDOW },
];

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
    fn 印のページの場所() {
        assert_eq!(marker(".d1:5"), "PRELUDE-PANE:.d1:5");
        assert_eq!(
            pane_url(Path::new("/c/prelude-pane.html"), ":5", "https://a.jp/?q=日本&x=1#y"),
            "file:///c/prelude-pane.html#%3A5|https%3A%2F%2Fa.jp%2F%3Fq%3D%E6%97%A5%E6%9C%AC%26x%3D1%23y"
        );
        assert!(PANE_PAGE.contains(&PANE_DELAY_MS.to_string()));
    }
}

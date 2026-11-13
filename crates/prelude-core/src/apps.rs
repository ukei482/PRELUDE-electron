//! インストール済みのアプリ。出どころは [`AppSource`] のトレイトで差し替えられる(docs/redesign-rust.md の「ほかの OS」)。
//! 今の実装は XDG の .desktop([`Xdg`])。Electron 版の sys/apps.js と同じ読み方(起動コマンドの形も同じにして、確かめる結果をそろえる)。
//! - 人からもらったブックマークの起動コマンドが、インストール済みのアプリと一致するか確かめる
//! - 起動したアプリの窓を見分ける名前(StartupWMClass・.desktop の id)を引く
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// インストール済みのアプリの出どころ。コアはこれを通してだけアプリの一覧を見る(ほかの OS では別の実装にする)
pub trait AppSource: Send + Sync {
    /// 出どころの名前(ログ用)
    fn name(&self) -> &'static str;
    /// アプリの一覧(名前順)。何度呼んでもよい(実装が使い回す)
    fn list(&self) -> Arc<Vec<App>>;
    /// アプリのアイコンのファイル
    fn icon(&self, app: &App) -> Option<PathBuf>;
    /// 起動コマンドがインストール済みのアプリのものか(人からもらったブックマークの起動コマンドを確かめる)
    fn is_installed(&self, cmd: &str) -> bool {
        let cmd = cmd.trim();
        self.list().iter().any(|a| a.exec == cmd)
    }
}

/// XDG の .desktop(`XDG_DATA_DIRS` と flatpak)。一覧は 1 分だけ使い回す。アイコンはアイコンのテーマから探す
pub struct Xdg;

impl AppSource for Xdg {
    fn name(&self) -> &'static str {
        "xdg"
    }
    fn list(&self) -> Arc<Vec<App>> {
        cached_arc()
    }
    fn icon(&self, app: &App) -> Option<PathBuf> {
        crate::icons::find(&app.icon)
    }
}

/// 決まった一覧(テスト用)。アイコンは、App の icon がファイルのパスで、あればそれ
pub struct Fixed(pub Vec<App>);

impl AppSource for Fixed {
    fn name(&self) -> &'static str {
        "fixed"
    }
    fn list(&self) -> Arc<Vec<App>> {
        Arc::new(self.0.clone())
    }
    fn icon(&self, app: &App) -> Option<PathBuf> {
        Some(PathBuf::from(&app.icon)).filter(|p| p.is_file())
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct App {
    pub name: String,
    /// 起動コマンド(フィールドコードを除いたもの)
    pub exec: String,
    /// StartupWMClass(小文字)
    pub cls: String,
    /// .desktop のファイル名から .desktop を除いたもの。Wayland の app_id と同じことが多い
    pub id: String,
    /// Icon(アイコンの名前か、ファイルのパス。[`crate::icons::find`] で探す)
    pub icon: String,
}

/// .desktop を探すフォルダ。後のフォルダほど優先(同じ名前の .desktop は後のもので上書き)。
/// XDG_DATA_DIRS は先が優先なので逆にする
fn dirs() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut out: Vec<PathBuf> = crate::xdg::data_dirs().into_iter().rev().collect();
    out.extend([
        crate::xdg::data_home(),
        "/var/lib/flatpak/exports/share".into(),
        Path::new(&home).join(".local/share/flatpak/exports/share"),
    ]);
    let mut seen = Vec::new();
    for d in out {
        if !seen.contains(&d) {
            seen.push(d);
        }
    }
    seen.into_iter().map(|d| d.join("applications")).collect()
}

/// 端末で動くアプリ(htop など)を包むコマンド。PATH から探し、見つからなければ x-terminal-emulator に任せる
fn terminal_cmd(exec: &str) -> String {
    let term = crate::xdg::which(&["konsole", "x-terminal-emulator"], &[])
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "x-terminal-emulator".into());
    format!("{term} -e {exec}")
}

/// Exec のフィールドコード(%f %u %i など)を除く。`--icon %i` は --icon ごと
pub fn clean_exec(exec: &str) -> String {
    let s = exec.replace("--icon %i", "");
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' && chars.peek().is_some_and(|n| n.is_ascii_alphabetic()) {
            chars.next();
            continue;
        }
        out.push(c);
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// .desktop の最初の節([Desktop Entry])の キー=値
fn entry(text: &str) -> BTreeMap<String, String> {
    let first = text.split("\n[").next().unwrap_or("");
    let mut m = BTreeMap::new();
    for l in first.lines() {
        if let Some((k, v)) = l.split_once('=') {
            m.entry(k.trim().to_string()).or_insert_with(|| v.trim().to_string());
        }
    }
    m
}

/// 何もしない項目(snap の補助エントリなど)。`true`・`false` か、どこかのフォルダのそれ
fn is_noop(exec: &str) -> bool {
    !exec.contains(char::is_whitespace) && matches!(exec.rsplit('/').next(), Some("true" | "false"))
}

pub fn list() -> Vec<App> {
    let mut map: BTreeMap<String, App> = BTreeMap::new();
    for dir in dirs() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut files: Vec<_> = rd.flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|f| f.ends_with(".desktop")).collect();
        files.sort();
        for f in files {
            let Ok(text) = std::fs::read_to_string(dir.join(&f)) else { continue };
            let e = entry(&text);
            let get = |k: &str| e.get(k).map(String::as_str).unwrap_or("");
            // 後の(優先度の高い)フォルダで隠されたものは、前のフォルダの同名ファイルも消す
            if get("Type") != "Application" || get("NoDisplay") == "true" || get("Hidden") == "true" {
                map.remove(&f);
                continue;
            }
            let mut exec = clean_exec(get("Exec"));
            if exec.is_empty() || is_noop(&exec) {
                continue;
            }
            if get("Terminal") == "true" {
                exec = terminal_cmd(&exec);
            }
            let name = if get("Name").is_empty() { f.clone() } else { get("Name").to_string() };
            let app = App {
                name,
                exec,
                cls: get("StartupWMClass").to_lowercase(),
                id: f.trim_end_matches(".desktop").to_string(),
                icon: get("Icon").to_string(),
            };
            map.insert(f, app);
        }
    }
    let mut v: Vec<App> = map.into_values().collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

/// list() を 1 分だけ使い回す(写さずに分け合う)
fn cached_arc() -> Arc<Vec<App>> {
    static CACHE: Mutex<Option<(Instant, Arc<Vec<App>>)>> = Mutex::new(None);
    let mut c = CACHE.lock().unwrap();
    match &*c {
        Some((t, v)) if t.elapsed() < Duration::from_secs(60) => v.clone(),
        _ => {
            let v = Arc::new(list());
            *c = Some((Instant::now(), v.clone()));
            v
        }
    }
}

/// list() を 1 分だけ使い回す(窓のつなぎが、起動した窓を見分けるのに使う)
pub fn cached() -> Vec<App> {
    cached_arc().as_ref().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_entries_in_any_folder() {
        assert!(is_noop("true"));
        assert!(is_noop("/usr/bin/false"));
        assert!(is_noop("/run/current-system/sw/bin/true"));
        assert!(!is_noop("true --x"));
        assert!(!is_noop("/usr/bin/trueish"));
    }

    #[test]
    fn フィールドコードを除く() {
        assert_eq!(clean_exec("kate -b %U"), "kate -b");
        assert_eq!(clean_exec("app --icon %i  --x %f"), "app --x");
        assert_eq!(clean_exec("echo 100%"), "echo 100%");
    }

    #[test]
    fn 最初の節だけ読む() {
        let e = entry("[Desktop Entry]\nName=Kate\nExec=kate %U\n[Desktop Action new]\nName=New\nExec=kate -n\n");
        assert_eq!((e["Name"].as_str(), e["Exec"].as_str()), ("Kate", "kate %U"));
    }
}

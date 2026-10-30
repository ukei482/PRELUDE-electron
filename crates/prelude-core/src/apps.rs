//! インストール済みのアプリ(.desktop)。Electron 版の sys/apps.js と同じ読み方(起動コマンドの形も同じにして、確かめる結果をそろえる)。
//! - 人からもらったブックマークの起動コマンドが、インストール済みのアプリと一致するか確かめる
//! - 起動したアプリの窓を見分ける名前(StartupWMClass・.desktop の id)を引く
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub struct App {
    pub name: String,
    /// 起動コマンド(フィールドコードを除いたもの)
    pub exec: String,
    /// StartupWMClass(小文字)
    pub cls: String,
    /// .desktop のファイル名から .desktop を除いたもの。Wayland の app_id と同じことが多い
    pub id: String,
}

fn dirs() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let data = std::env::var("XDG_DATA_DIRS").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let data_home = std::env::var("XDG_DATA_HOME").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| format!("{home}/.local/share"));
    let mut out: Vec<String> = data.split(':').filter(|s| !s.is_empty()).map(String::from).collect();
    out.extend([data_home, "/var/lib/flatpak/exports/share".into(), format!("{home}/.local/share/flatpak/exports/share")]);
    let mut seen = Vec::new();
    for d in out {
        if !seen.contains(&d) {
            seen.push(d);
        }
    }
    seen.into_iter().map(|d| Path::new(&d).join("applications")).collect()
}

/// 端末で動くアプリ(htop など)を包むコマンド
fn terminal_cmd(exec: &str) -> String {
    let term =
        ["/usr/bin/konsole", "/usr/bin/x-terminal-emulator"].into_iter().find(|p| Path::new(p).exists()).unwrap_or("x-terminal-emulator");
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
            if exec.is_empty() || matches!(exec.as_str(), "false" | "true" | "/usr/bin/false" | "/usr/bin/true") {
                continue;
            }
            if get("Terminal") == "true" {
                exec = terminal_cmd(&exec);
            }
            let name = if get("Name").is_empty() { f.clone() } else { get("Name").to_string() };
            let app = App { name, exec, cls: get("StartupWMClass").to_lowercase(), id: f.trim_end_matches(".desktop").to_string() };
            map.insert(f, app);
        }
    }
    let mut v: Vec<App> = map.into_values().collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

/// list() を 1 分だけ使い回す
pub fn cached() -> Vec<App> {
    static CACHE: Mutex<Option<(Instant, Vec<App>)>> = Mutex::new(None);
    let mut c = CACHE.lock().unwrap();
    match &*c {
        Some((t, v)) if t.elapsed() < Duration::from_secs(60) => v.clone(),
        _ => {
            let v = list();
            *c = Some((Instant::now(), v.clone()));
            v
        }
    }
}

/// 起動コマンドがインストール済みのアプリのものか
pub fn is_installed(cmd: &str) -> bool {
    let cmd = cmd.trim();
    cached().iter().any(|a| a.exec == cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

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

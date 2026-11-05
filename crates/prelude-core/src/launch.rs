//! アプリのペインの起動と、起動した窓の見分け方。KWin・sway の裏側で同じものを使う
use crate::{apps, procs};

/// 起動を任せるだけのラッパー(窓の名前にならない)
const WRAPPERS: [&str; 18] = [
    "sh",
    "bash",
    "env",
    "nohup",
    "setsid",
    "pkexec",
    "sudo",
    "lxqt-sudo",
    "xdg-open",
    "gtk-launch",
    "kioclient",
    "kioclient5",
    "flatpak",
    "snap",
    "dbus-launch",
    "x-terminal-emulator",
    "exec",
    "",
];

/// 起動したアプリの窓を見分ける名前(小文字)。.desktop の id・StartupWMClass・実行ファイル名
pub fn keys_of(cmd: &str, cls: &str) -> Vec<String> {
    if cls.starts_with("chrome-") {
        return vec![cls.to_lowercase()]; // 実ブラウザのペインは、そのペイン専用のクラスだけで照合する
    }
    let mut keys: Vec<String> = Vec::new();
    let mut add = |k: String| {
        if !k.is_empty() && !keys.contains(&k) {
            keys.push(k);
        }
    };
    add(cls.to_lowercase());
    if let Some(a) = apps::cached().into_iter().find(|a| a.exec == cmd.trim()) {
        add(a.cls);
        add(a.id.to_lowercase());
    }
    let base = procs::cmd_base(cmd);
    if !WRAPPERS.contains(&base.as_str()) {
        add(base);
    }
    keys
}

/// シェルのコマンドを起動する。作業フォルダが無ければホーム。PRELUDE の端末のシグナル(Ctrl+C など)は受けない
pub fn spawn(cmd: &str, cwd: &str) -> std::io::Result<tokio::process::Child> {
    let cwd = if std::path::Path::new(cwd).is_dir() { cwd.to_string() } else { std::env::var("HOME").unwrap_or_else(|_| "/".into()) };
    let mut c = tokio::process::Command::new("/bin/sh");
    c.arg("-c")
        .arg(cmd)
        .current_dir(&cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    c.process_group(0);
    c.spawn()
}

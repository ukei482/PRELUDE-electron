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

/// 起動したペインのプロセスに渡す、ペインの id の環境変数。ファイラなどは `prelude pane path` がこれを読んで、自分のペインに場所を伝える
pub const PANE_ENV: &str = "PRELUDE_PANE";

/// ペインのシェルのコマンドを起動する。作業フォルダが無ければホーム。PRELUDE の端末のシグナル(Ctrl+C など)は受けない。
/// 環境変数 `PRELUDE_PANE` にペインの id を入れる
pub fn spawn(cmd: &str, cwd: &str, pane: u64) -> std::io::Result<tokio::process::Child> {
    let cwd = if std::path::Path::new(cwd).is_dir() { cwd.to_string() } else { std::env::var("HOME").unwrap_or_else(|_| "/".into()) };
    let mut c = tokio::process::Command::new("/bin/sh");
    c.arg("-c")
        .arg(cmd)
        .current_dir(&cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .env(PANE_ENV, pane.to_string());
    c.process_group(0);
    c.spawn()
}

/// 起動して、プロセスは追わない(終わったら片付けるだけ)。窓を題名の印で見分けるペイン(Web のペイン)に使う:
/// ブラウザは窓が増えても同じプロセスのままなので、プロセスでは見分けられず、ペインを閉じてもプロセスを終わらせてはいけない
pub fn spawn_detached(cmd: &str, cwd: &str, pane: u64) -> std::io::Result<()> {
    let mut child = spawn(cmd, cwd, pane)?;
    if let Ok(rt) = tokio::runtime::Handle::try_current() {
        rt.spawn(async move {
            let _ = child.wait().await;
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ペインの_id_を環境変数で渡す() {
        let d = std::env::temp_dir().join(format!("prelude-launch-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let out = d.join("pane");
        let mut child = spawn(&format!("printf %s \"$PRELUDE_PANE\" > '{}'", out.display()), &d.to_string_lossy(), 42).unwrap();
        child.wait().await.unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "42");
        let _ = std::fs::remove_dir_all(&d);
    }
}

//! `prelude term [--cwd <フォルダ>] [-- <コマンド>...]`: 設定の端末(`terminal`)を、端末の表(`prelude_format::term`)のとおりに起動する。
//! 既定のレシピ(files・console・cli・agent)とダイアログの選ぶ画面(`prelude choose`)は、これで端末を開く。
//! このプロセスを端末に置き換える(端末が閉じるまで終わらない。選ぶ画面はそれを待つ)
use prelude_format::settings::Config;
use std::os::unix::process::CommandExt;

/// 引数を読む → (作業フォルダ, コマンド)
pub fn parse(args: &[&str]) -> Result<(String, Vec<String>), String> {
    let usage = "usage: prelude term [--cwd <folder>] [-- <command>...]";
    let mut cwd = String::new();
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--cwd" => {
                cwd = args.get(i + 1).ok_or(usage)?.to_string();
                i += 2;
            }
            "--" => return Ok((cwd, args[i + 1..].iter().map(|s| s.to_string()).collect())),
            _ => return Err(usage.into()),
        }
    }
    Ok((cwd, vec![]))
}

pub fn run(args: &[&str]) -> Result<(), String> {
    let (cwd, cmd) = parse(args)?;
    let setting = Config::load().str("terminal").to_string();
    let Some(t) = prelude_format::term::choose(&setting, &std::env::var("PATH").unwrap_or_default()) else {
        return Err(
            "No terminal found. Install foot, kitty, ghostty, alacritty, wezterm or konsole, or `prelude config set terminal <name>`"
                .into(),
        );
    };
    let argv = t.argv(&cwd, &cmd);
    let mut c = std::process::Command::new(&argv[0]);
    c.args(&argv[1..]);
    // 作業フォルダを渡せない端末のためにも、起動するフォルダにする
    if !cwd.is_empty() && std::path::Path::new(&cwd).is_dir() {
        c.current_dir(&cwd);
    }
    let e = c.exec();
    Err(format!("{}: {e}", argv[0]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 引数() {
        assert_eq!(parse(&[]), Ok((String::new(), vec![])));
        assert_eq!(parse(&["--cwd", "/w", "--", "yazi", "--x"]), Ok(("/w".into(), vec!["yazi".into(), "--x".into()])));
        assert_eq!(parse(&["--", "a"]), Ok((String::new(), vec!["a".into()])));
        assert!(parse(&["yazi"]).is_err());
        assert!(parse(&["--cwd"]).is_err());
    }
}

//! prelude-core: PRELUDE のコア(常駐)。
//!
//! 使い方: `prelude-core [--socket <パス>] [--state <パス>] [--spaces <フォルダ>] [--recipes <フォルダ>] [--no-embed]`
//! - ソケット: 既定は `$XDG_RUNTIME_DIR/prelude/core.sock`(環境変数 `PRELUDE_CORE_SOCK` でも変えられる)
//! - 残す状態: 既定は `$XDG_STATE_HOME/prelude/core.json`(`PRELUDE_CORE_STATE`)
//! - スペースのフォルダ: 既定は `~/スペース`(`PRELUDE_SPACES`)。Electron 版と同じフォルダを使う(判断 8)
//! - ペインのレシピ: 既定は `$XDG_CONFIG_HOME/prelude/panes`
//! - `--no-embed`: アプリを起動せず、窓も取り込まない(状態だけを試す)
use prelude_core::recipes;
use prelude_core::server::{run, Options, VERSION};
use std::path::PathBuf;

const USAGE: &str = "usage: prelude-core [--socket PATH] [--state PATH] [--spaces DIR] [--recipes DIR] [--no-embed]";

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn main() {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    let state_home = env_path("XDG_STATE_HOME").unwrap_or_else(|| PathBuf::from(&home).join(".local/state"));
    let mut opt = Options {
        socket: prelude_proto::socket_path(),
        state_file: env_path("PRELUDE_CORE_STATE").unwrap_or_else(|| state_home.join("prelude/core.json")),
        spaces_root: env_path("PRELUDE_SPACES").unwrap_or_else(|| PathBuf::from(&home).join("スペース")),
        recipes_dir: recipes::dir(),
        embed: true,
        home,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut val = || args.next().map(PathBuf::from).unwrap_or_else(|| exit_usage());
        match a.as_str() {
            "--socket" => opt.socket = val(),
            "--state" => opt.state_file = val(),
            "--spaces" => opt.spaces_root = val(),
            "--recipes" => opt.recipes_dir = val(),
            "--no-embed" => opt.embed = false,
            "--version" => {
                println!("prelude-core {VERSION} (protocol {})", prelude_proto::PROTOCOL);
                return;
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            _ => exit_usage(),
        }
    }

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("tokio runtime");
    let result = rt.block_on(run(opt, async {
        // Ctrl+C か SIGTERM(systemctl stop など)で止まる
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }));
    if let Err(e) = result {
        eprintln!("prelude-core: {e}");
        std::process::exit(1);
    }
}

fn exit_usage() -> ! {
    eprintln!("{USAGE}");
    std::process::exit(2);
}

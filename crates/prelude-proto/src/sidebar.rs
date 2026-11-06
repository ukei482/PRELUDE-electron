//! サイドバーの幅(広い / 細い列)。サイドバーと `prelude sidebar` の約束。
//!
//! - 見た目だけの状態なので、コアには持たせない。`$XDG_STATE_HOME/prelude/sidebar` に `wide` か `compact` を書く
//! - サイドバーは起動のときにこれを読み、変わったら幅を変える。つまみを押したときも、`prelude sidebar toggle` もここを書き換える
//! - 完全に隠すのは `prelude sidebar hide`(プロセスを止める)だけ
use std::path::PathBuf;

/// サイドバーの幅の状態を書くファイル
pub fn mode_file() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
    state.join("prelude/sidebar")
}

/// 細い列か(ファイルが無い・読めないときは広い)
pub fn is_compact() -> bool {
    std::fs::read_to_string(mode_file()).is_ok_and(|t| t.trim() == "compact")
}

/// 広い / 細い列を書く
pub fn set_compact(compact: bool) -> Result<(), String> {
    let f = mode_file();
    if let Some(dir) = f.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&f, if compact { "compact\n" } else { "wide\n" }).map_err(|e| format!("{}: {e}", f.display()))
}

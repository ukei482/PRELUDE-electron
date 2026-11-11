//! `prelude agent [<頼みごと>...]`: 既定の AI(Claude Code・Codex など)を、今のフォルダで起動する(docs/redesign-rust.md の「AI 連携」)。
//! レシピ `agent`(AI のペイン)は、端末を今のブックマークの作業フォルダで開いて、これを動かす。
//! どの AI か: 環境変数 `PRELUDE_AGENT`(コマンド)、無ければ PATH から [`AGENTS`] の順に探す
use std::os::unix::process::CommandExt;
use std::path::Path;

/// 探す AI(好きな順)
pub const AGENTS: [&str; 4] = ["claude", "codex", "gemini", "opencode"];

/// AI に最初の頼みごとを渡す引数。渡し方が分からない AI は None
pub fn prompt_args(agent: &str, prompt: &str) -> Option<Vec<String>> {
    let base = Path::new(agent.split_whitespace().next().unwrap_or("")).file_name()?.to_string_lossy().into_owned();
    match base.as_str() {
        "claude" | "codex" => Some(vec![prompt.to_string()]),
        "gemini" => Some(vec!["-i".into(), prompt.to_string()]),
        _ => None,
    }
}

/// path(PATH の値)の中で最初に見つかる AI
pub fn find_in(path: &str) -> Option<String> {
    AGENTS.iter().find_map(|a| {
        path.split(':')
            .filter(|d| !d.is_empty())
            .map(|d| Path::new(d).join(a))
            .find(|p| p.is_file())
            .map(|p| p.to_string_lossy().into_owned())
    })
}

/// 使う AI: (AI の名前, 起動する語)。`PRELUDE_AGENT` はシェルのコマンドとして読む(引用符も使える)ので `sh -c` で包み、
/// 後から足す引数(最初の頼みごと)は `"$@"` で渡す
pub fn choose(env_agent: Option<String>, path: &str) -> Option<(String, Vec<String>)> {
    match env_agent.filter(|s| !s.trim().is_empty()) {
        Some(cmd) => {
            let name = cmd.split_whitespace().next().unwrap_or("").to_string();
            Some((name, vec!["/bin/sh".into(), "-c".into(), format!("{cmd} \"$@\""), "prelude-agent".into()]))
        }
        None => find_in(path).map(|p| (p.clone(), vec![p])),
    }
}

pub fn run(prompt: &[&str]) -> Result<(), String> {
    let Some((name, mut cmd)) = choose(std::env::var("PRELUDE_AGENT").ok(), &std::env::var("PATH").unwrap_or_default()) else {
        return Err(format!("No AI agent found. Install one of {} or set PRELUDE_AGENT", AGENTS.join(", ")));
    };
    if !prompt.is_empty() {
        let prompt = prompt.join(" ");
        match prompt_args(&name, &prompt) {
            Some(a) => cmd.extend(a),
            None => eprintln!("prelude: cannot hand a first prompt to {name}; starting it without one"),
        }
    }
    // このプロセスを AI に置き換える(端末は AI を直接持つ)
    let e = std::process::Command::new(&cmd[0]).args(&cmd[1..]).exec();
    Err(format!("{}: {e}", cmd[0]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 使う_ai_を決める() {
        let d = std::env::temp_dir().join(format!("prelude-agent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("a")).unwrap();
        std::fs::create_dir_all(d.join("b")).unwrap();
        std::fs::write(d.join("b/codex"), "").unwrap();
        std::fs::write(d.join("a/opencode"), "").unwrap();
        let path = format!("{}:{}", d.join("a").display(), d.join("b").display());
        // 好きな順(codex が opencode より先)。PATH の順ではない
        let codex = d.join("b/codex").to_string_lossy().into_owned();
        assert_eq!(choose(None, &path), Some((codex.clone(), vec![codex])));
        // PRELUDE_AGENT があればそれ(シェルのコマンドとして読む。引用符も使える)
        let (name, cmd) = choose(Some("claude --model 'opus x'".into()), &path).unwrap();
        assert_eq!(name, "claude");
        assert_eq!(cmd, vec!["/bin/sh", "-c", "claude --model 'opus x' \"$@\"", "prelude-agent"]);
        assert_eq!(choose(Some("  ".into()), ""), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 最初の頼みごとの渡し方() {
        assert_eq!(prompt_args("/usr/bin/claude", "直して"), Some(vec!["直して".into()]));
        assert_eq!(prompt_args("gemini", "直して"), Some(vec!["-i".into(), "直して".into()]));
        assert_eq!(prompt_args("aider", "直して"), None);
    }
}

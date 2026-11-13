//! `prelude skill`: AI(Claude Code・Codex など)のための PRELUDE のスキル(`skills/prelude/`)を出す・書き出す。
//! 中身はこのコマンドに埋め込むので、入っている PRELUDE の版とスキルの中身が必ず合う
use std::path::{Path, PathBuf};

/// スキルのファイル(名前, 中身)
pub const FILES: [(&str, &str); 2] =
    [("SKILL.md", include_str!("../../../skills/prelude/SKILL.md")), ("format.md", include_str!("../../../skills/prelude/format.md"))];

/// 書き出す先の既定: `~/.agents/skills`(Omarchy・Codex などが共有する場所)と `~/.claude/skills`(Claude Code)
pub fn default_dirs(home: &str) -> Vec<PathBuf> {
    vec![Path::new(home).join(".agents/skills"), Path::new(home).join(".claude/skills")]
}

/// dir/prelude/ に書き出す → 書いたフォルダ。同じ名前のフォルダに、PRELUDE のものでないスキルがあれば断る
pub fn install_into(dir: &Path) -> Result<PathBuf, String> {
    let to = dir.join("prelude");
    if let Ok(old) = std::fs::read_to_string(to.join("SKILL.md")) {
        if !old.lines().any(|l| l.trim() == "name: prelude") {
            return Err(format!("{} holds another skill; not overwriting it", to.display()));
        }
    }
    std::fs::create_dir_all(&to).map_err(|e| format!("{}: {e}", to.display()))?;
    for (name, text) in FILES {
        std::fs::write(to.join(name), text).map_err(|e| format!("{}: {e}", to.join(name).display()))?;
    }
    Ok(to)
}

pub fn run(args: &[&str], home: &str) -> Result<(), String> {
    match args {
        [] | ["show"] => print!("{}", FILES[0].1),
        ["show", name] => {
            let (_, text) = FILES.iter().find(|(n, _)| n == name).ok_or_else(|| format!("No such file in the skill: {name}"))?;
            print!("{text}");
        }
        ["install", dirs @ ..] => {
            let dirs: Vec<PathBuf> = if dirs.is_empty() { default_dirs(home) } else { dirs.iter().map(PathBuf::from).collect() };
            let mut failed = false;
            for d in dirs {
                match install_into(&d) {
                    Ok(to) => println!("{}", to.display()),
                    Err(e) => {
                        eprintln!("prelude: {e}");
                        failed = true;
                    }
                }
            }
            if failed {
                return Err("Some skill folders were not written".into());
            }
        }
        _ => return Err("usage: prelude skill [show [<file>] | install [<dir>...]]".into()),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn スキルの形() {
        let skill = FILES[0].1;
        assert!(skill.starts_with("---\nname: prelude\ndescription:"));
        assert!(skill.contains("[`format.md`](format.md)"));
    }

    #[test]
    fn 書き出す_何度でも_ほかのスキルは上書きしない() {
        let d = std::env::temp_dir().join(format!("prelude-skill-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let to = install_into(&d).unwrap();
        assert_eq!(to, d.join("prelude"));
        assert_eq!(std::fs::read_to_string(to.join("format.md")).unwrap(), FILES[1].1);
        install_into(&d).unwrap(); // 2 回目も通る(版を上げたときの書き直し)
        let other = d.join("other");
        std::fs::create_dir_all(other.join("prelude")).unwrap();
        std::fs::write(other.join("prelude/SKILL.md"), "---\nname: someone-else\n---\n").unwrap();
        assert!(install_into(&other).is_err());
        assert_eq!(std::fs::read_to_string(other.join("prelude/SKILL.md")).unwrap(), "---\nname: someone-else\n---\n");
        let _ = std::fs::remove_dir_all(&d);
    }
}

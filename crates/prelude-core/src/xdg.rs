//! どこにアプリ・コマンドがあるかを、決め打ちのパス(/usr/bin・/usr/share)ではなく環境(PATH・XDG_DATA_DIRS)から探す。
//! Kubuntu でも NixOS(/run/current-system/sw・~/.nix-profile)でも同じに動く。Electron 版の sys/xdg.js と同じ読み方。
use std::path::{Path, PathBuf};

fn var(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|s| !s.is_empty())
}

/// $XDG_DATA_HOME(無ければ ~/.local/share)
pub fn data_home() -> PathBuf {
    var("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| Path::new(&var("HOME").unwrap_or_default()).join(".local/share"))
}

/// $XDG_DATA_DIRS(無ければ決まりどおり /usr/local/share:/usr/share)。優先度の高いものが先
pub fn data_dirs() -> Vec<PathBuf> {
    split_dirs(var("XDG_DATA_DIRS").as_deref().unwrap_or("/usr/local/share:/usr/share"))
}

fn split_dirs(s: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for d in s.split(':').filter(|d| !d.is_empty()) {
        let d = PathBuf::from(d);
        if !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// names のうち、PATH(と extra のフォルダ)で最初に見つかったコマンドの絶対パス。
/// names は好きな順。1 つ目の名前を PATH 全体で探してから、次の名前へ進む
pub fn which(names: &[&str], extra: &[&str]) -> Option<PathBuf> {
    which_in(names, &var("PATH").unwrap_or_default(), extra)
}

fn which_in(names: &[&str], path: &str, extra: &[&str]) -> Option<PathBuf> {
    let mut dirs = split_dirs(path);
    for e in extra {
        let e = PathBuf::from(e);
        if !dirs.contains(&e) {
            dirs.push(e);
        }
    }
    for n in names {
        if Path::new(n).is_absolute() {
            if is_exec(Path::new(n)) {
                return Some(PathBuf::from(n));
            }
            continue;
        }
        if let Some(f) = dirs.iter().map(|d| d.join(n)).find(|f| is_exec(f)) {
            return Some(f);
        }
    }
    None
}

fn is_exec(f: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(f).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn data_dirs_keep_order_and_drop_duplicates() {
        assert_eq!(split_dirs("/a::/b:/a"), vec![PathBuf::from("/a"), PathBuf::from("/b")]);
    }

    #[test]
    fn which_prefers_name_order_then_path_order() {
        let t = std::env::temp_dir().join(format!("prelude-xdg-{}", std::process::id()));
        let (a, b) = (t.join("a"), t.join("b"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        let mk = |f: &Path, mode| {
            std::fs::write(f, "").unwrap();
            std::fs::set_permissions(f, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        mk(&a.join("second"), 0o755);
        mk(&b.join("first"), 0o755);
        mk(&a.join("noexec"), 0o644);
        let path = format!("{}:{}", a.display(), b.display());
        assert_eq!(which_in(&["first", "second"], &path, &[]), Some(b.join("first")));
        assert_eq!(which_in(&["noexec", "second"], &path, &[]), Some(a.join("second")));
        assert_eq!(which_in(&["first"], "", &[b.to_str().unwrap()]), Some(b.join("first")));
        assert_eq!(which_in(&["missing"], &path, &[]), None);
        std::fs::remove_dir_all(&t).unwrap();
    }
}

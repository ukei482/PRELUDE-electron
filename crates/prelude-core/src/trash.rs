//! ゴミ箱(freedesktop.org の Trash の決まり)。`$XDG_DATA_HOME/Trash/files` に移し、`info` に元の場所と日時を書く。
//! 同じファイルシステムでなければ、コピーしてから消す(ブックマークは小さなファイルなので)
use std::io;
use std::path::{Path, PathBuf};

/// ゴミ箱のフォルダ(`$XDG_DATA_HOME/Trash`、無ければ `~/.local/share/Trash`)
pub fn dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    let data = std::env::var("XDG_DATA_HOME").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| format!("{home}/.local/share"));
    Path::new(&data).join("Trash")
}

/// URL のパスの形にする(`/` と英数字・`-._~` 以外は %XX)
fn encode(p: &str) -> String {
    let mut s = String::new();
    for b in p.bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

/// 今の地方時(`2026-11-11T12:34:56`)
fn local_now() -> String {
    // SAFETY: localtime_r は渡した tm に書くだけ
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
    }
}

/// ファイル 1 つをゴミ箱(trash のフォルダ)へ移す → ゴミ箱の中のパス
pub fn trash_in(path: &Path, trash: &Path) -> io::Result<PathBuf> {
    let files = trash.join("files");
    let info = trash.join("info");
    std::fs::create_dir_all(&files)?;
    std::fs::create_dir_all(&info)?;
    let name = path.file_name().ok_or_else(|| io::Error::other("no file name"))?.to_string_lossy().into_owned();
    // 同じ名前があれば「名前.2」…(info を先に作って、ほかと取り合わないようにする)
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.clone(), String::new()),
    };
    for n in 1.. {
        let candidate = if n == 1 { name.clone() } else { format!("{stem}.{n}{ext}") };
        let info_file = info.join(format!("{candidate}.trashinfo"));
        let f = std::fs::OpenOptions::new().write(true).create_new(true).open(&info_file);
        let mut f = match f {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        };
        use std::io::Write;
        let abs = std::fs::canonicalize(path)?;
        write!(f, "[Trash Info]\nPath={}\nDeletionDate={}\n", encode(&abs.to_string_lossy()), local_now())?;
        let dest = files.join(&candidate);
        if std::fs::rename(path, &dest).is_err() {
            // 別のファイルシステム: コピーしてから消す
            if let Err(e) = std::fs::copy(path, &dest).and_then(|_| std::fs::remove_file(path)) {
                let _ = std::fs::remove_file(&info_file);
                let _ = std::fs::remove_file(&dest);
                return Err(e);
            }
        }
        return Ok(dest);
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ゴミ箱へ移す_同じ名前は番号を付ける() {
        let d = std::env::temp_dir().join(format!("prelude-trash-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("src")).unwrap();
        let trash = d.join("Trash");
        for _ in 0..2 {
            std::fs::write(d.join("src/本 文.prelude"), "x").unwrap();
            trash_in(&d.join("src/本 文.prelude"), &trash).unwrap();
        }
        assert!(!d.join("src/本 文.prelude").exists());
        assert!(trash.join("files/本 文.prelude").exists() && trash.join("files/本 文.2.prelude").exists());
        let info = std::fs::read_to_string(trash.join("info/本 文.prelude.trashinfo")).unwrap();
        assert!(info.starts_with("[Trash Info]\nPath=/"), "{info}");
        assert!(info.contains("/src/%E6%9C%AC%20%E6%96%87.prelude\n"), "{info}");
        assert!(info.contains("DeletionDate=20"));
        let _ = std::fs::remove_dir_all(&d);
    }
}

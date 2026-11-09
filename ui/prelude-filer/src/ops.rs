//! ファイルの操作(Electron 版の fileops.js の移植)。GPUI に依らないのでテストできる。
//!
//! - 上書きはしない。同じ名前があれば「名前 (1).拡張子」…にする(フォルダは拡張子を分けない)
//! - フォルダを自分の中へコピー・移動することは断る
//! - 消すのは既定でゴミ箱(`gio trash`。別のドライブのものも、そのドライブのゴミ箱へ)。完全に消すのは別の関数
//! - クリップボード: 文字としてパスを 1 行ずつ書く(Dolphin などとも行き来できる)。切り取りかどうかは、
//!   ペインごとに別のプロセスのファイラの間で分け合うため、`$XDG_RUNTIME_DIR/prelude/filer-clip.json` に控える
use std::path::{Path, PathBuf};

/// 使えない名前なら None。前後の空白は除く
pub fn valid_name(name: &str) -> Option<String> {
    let n = name.trim();
    if n.is_empty() || n == "." || n == ".." || n.contains('/') || n.contains('\0') {
        return None;
    }
    Some(n.to_string())
}

/// dir の中で name が空いていればそのまま、あれば「name (1).ext」…
pub fn free_path(dir: &Path, name: &str, is_dir: bool) -> PathBuf {
    let (base, ext) = match name.rfind('.') {
        Some(i) if !is_dir && i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let mut p = dir.join(name);
    let mut i = 1;
    while p.symlink_metadata().is_ok() {
        p = dir.join(format!("{base} ({i}){ext}"));
        i += 1;
    }
    p
}

/// 新しいフォルダ。同じ名前があれば番号を付ける。作ったフォルダの名前
pub fn mkdir(dir: &Path, name: &str) -> Result<String, String> {
    let n = valid_name(name).ok_or("Invalid name")?;
    let p = free_path(dir, &n, true);
    std::fs::create_dir(&p).map_err(|e| e.to_string())?;
    Ok(file_name(&p))
}

/// 名前を変える。同じ名前のものがあれば断る
pub fn rename(full: &Path, name: &str) -> Result<String, String> {
    let n = valid_name(name).ok_or("Invalid name")?;
    let dest = full.with_file_name(&n);
    if dest == full {
        return Ok(n);
    }
    if dest.symlink_metadata().is_ok() {
        return Err(format!("\"{n}\" already exists"));
    }
    std::fs::rename(full, &dest).map_err(|e| e.to_string())?;
    Ok(n)
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// a の中(または a 自身)に b があるか
fn inside(a: &Path, b: &Path) -> bool {
    b.starts_with(a)
}

/// フォルダごとコピーする(シンボリックリンクはリンクのまま、更新時刻は残す)
fn copy_all(src: &Path, dest: &Path) -> std::io::Result<()> {
    let meta = src.symlink_metadata()?;
    if meta.file_type().is_symlink() {
        std::os::unix::fs::symlink(std::fs::read_link(src)?, dest)?;
    } else if meta.is_dir() {
        std::fs::create_dir(dest)?;
        for e in std::fs::read_dir(src)? {
            let e = e?;
            copy_all(&e.path(), &dest.join(e.file_name()))?;
        }
        std::fs::set_permissions(dest, meta.permissions())?;
    } else {
        std::fs::copy(src, dest)?;
    }
    if !meta.file_type().is_symlink()
        && let Ok(t) = meta.modified()
    {
        let _ = std::fs::File::options().write(meta.is_file()).open(dest).and_then(|f| f.set_modified(t));
    }
    Ok(())
}

/// コピーか移動
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Copy,
    Move,
}

/// paths を dest へコピー・移動する。上書きはせず、同じ名前があれば番号を付ける。返すのは (できた名前, 失敗の理由)
pub fn transfer(mode: Mode, paths: &[PathBuf], dest: &Path) -> (Vec<String>, Vec<String>) {
    let (mut names, mut errors) = (vec![], vec![]);
    for src in paths {
        let name = file_name(src);
        let r = (|| -> Result<String, String> {
            let meta = src.symlink_metadata().map_err(|e| e.to_string())?;
            let is_dir = meta.is_dir();
            if is_dir && inside(src, dest) {
                return Err(format!("Cannot {} a folder into itself", if mode == Mode::Move { "move" } else { "copy" }));
            }
            if mode == Mode::Move && src.parent() == Some(dest) {
                return Ok(name.clone()); // 同じ場所への移動は何もしない
            }
            let to = free_path(dest, &name, is_dir);
            match mode {
                Mode::Copy => copy_all(src, &to).map_err(|e| e.to_string())?,
                Mode::Move => {
                    if let Err(e) = std::fs::rename(src, &to) {
                        if e.raw_os_error() != Some(libc::EXDEV) {
                            return Err(e.to_string());
                        }
                        // 別のドライブへの移動は、コピーしてから消す
                        copy_all(src, &to).map_err(|e| e.to_string())?;
                        remove_all(src).map_err(|e| e.to_string())?;
                    }
                }
            }
            Ok(file_name(&to))
        })();
        match r {
            Ok(n) => names.push(n),
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }
    (names, errors)
}

fn remove_all(p: &Path) -> std::io::Result<()> {
    if p.symlink_metadata()?.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) }
}

/// ゴミ箱へ(`gio trash`)。失敗の理由
pub fn trash(paths: &[PathBuf]) -> Vec<String> {
    let mut errors = vec![];
    for p in paths {
        match std::process::Command::new("gio").arg("trash").arg("--").arg(p).output() {
            Ok(o) if o.status.success() => {}
            Ok(o) => errors.push(format!("{}: {}", file_name(p), String::from_utf8_lossy(&o.stderr).trim())),
            Err(e) => errors.push(format!("{}: cannot run gio ({e})", file_name(p))),
        }
    }
    errors
}

/// 完全に消す(ゴミ箱を通さない)。失敗の理由
pub fn remove(paths: &[PathBuf]) -> Vec<String> {
    paths.iter().filter_map(|p| remove_all(p).err().map(|e| format!("{}: {e}", file_name(p)))).collect()
}

// ------------------------------------------------------------------ 選ぶ画面(アプリのダイアログ)

/// 選んだパスを 1 行ずつ書く(コアの portal.rs が読む)
pub fn write_choice(output: &Path, paths: &[PathBuf]) -> Result<(), String> {
    std::fs::write(output, clip_text(paths) + "\n").map_err(|e| e.to_string())
}

/// 保存先のパス。名前が使えなければ None
pub fn save_target(dir: &Path, name: &str) -> Option<PathBuf> {
    valid_name(name).map(|n| dir.join(n))
}

/// 開ける一番近いフォルダ(paths のフォルダがまだ無いときなど)
pub fn nearest_dir(p: &Path) -> PathBuf {
    p.ancestors().find(|d| d.is_dir()).unwrap_or(Path::new("/")).to_path_buf()
}

// ------------------------------------------------------------------ クリップボード

/// クリップボードに書く文字(パスを 1 行ずつ)
pub fn clip_text(paths: &[PathBuf]) -> String {
    paths.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>().join("\n")
}

/// クリップボードの文字からパスを読む。パスか file:// の URL の行だけ。今あるものだけ
pub fn clip_paths(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| match l.strip_prefix("file://") {
            Some(rest) => Some(PathBuf::from(percent_decode(rest))),
            None if l.starts_with('/') => Some(PathBuf::from(l)),
            None => None,
        })
        .filter(|p| p.symlink_metadata().is_ok())
        .collect()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%'
            && i + 3 <= b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 切り取りの控えの場所
pub fn clip_note_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join("prelude/filer-clip.json")
}

/// 切り取った(cut)・コピーしたパスを控える
pub fn clip_note(note: &Path, paths: &[PathBuf], cut: bool) {
    if let Some(d) = note.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let v = serde_json::json!({ "paths": paths, "cut": cut });
    let _ = std::fs::write(note, v.to_string());
}

/// 貼り付けるものが切り取りか。控えのパスとクリップボードのパスが同じで、控えが切り取りのとき
pub fn clip_is_cut(note: &Path, paths: &[PathBuf]) -> bool {
    let Ok(t) = std::fs::read_to_string(note) else { return false };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) else { return false };
    let noted: Vec<PathBuf> = v["paths"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(PathBuf::from)).collect();
    v["cut"] == true && noted.len() == paths.len() && noted.iter().all(|p| paths.contains(p))
}

/// 切り取ったパス(控えが切り取りのときだけ。一覧で薄く出す)
pub fn cut_paths(note: &Path) -> Vec<PathBuf> {
    let Ok(t) = std::fs::read_to_string(note) else { return vec![] };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) else { return vec![] };
    if v["cut"] != true {
        return vec![];
    }
    v["paths"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(PathBuf::from)).collect()
}

/// 切り取りを貼り付け終えたら、控えを消す(同じものをもう一度移さない)
pub fn clip_consumed(note: &Path) {
    let _ = std::fs::remove_file(note);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("prelude-ops-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn 名前と番号() {
        assert_eq!(valid_name("  a.txt "), Some("a.txt".into()));
        for bad in ["", " ", ".", "..", "a/b", "a\0"] {
            assert_eq!(valid_name(bad), None, "{bad:?}");
        }
        let d = tmp("free");
        std::fs::write(d.join("a.txt"), "").unwrap();
        std::fs::write(d.join("a (1).txt"), "").unwrap();
        std::fs::create_dir(d.join("v1.0")).unwrap();
        assert_eq!(file_name(&free_path(&d, "a.txt", false)), "a (2).txt");
        assert_eq!(file_name(&free_path(&d, "v1.0", true)), "v1.0 (1)"); // フォルダは拡張子を分けない
        assert_eq!(file_name(&free_path(&d, ".bashrc", false)), ".bashrc");
        assert_eq!(mkdir(&d, "v1.0").unwrap(), "v1.0 (1)");
        assert_eq!(rename(&d.join("a.txt"), "b.txt").unwrap(), "b.txt");
        assert!(rename(&d.join("b.txt"), "a (1).txt").is_err()); // あれば断る
        assert!(rename(&d.join("b.txt"), "../x").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn コピー_移動() {
        let d = tmp("transfer");
        let (src, dst) = (d.join("src"), d.join("dst"));
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("sub/f.txt"), "hello").unwrap();
        std::os::unix::fs::symlink("sub/f.txt", src.join("link")).unwrap();
        std::fs::create_dir(&dst).unwrap();
        // フォルダごとコピー。リンクはリンクのまま。2 回目は番号付き
        let (n, e) = transfer(Mode::Copy, std::slice::from_ref(&src), &dst);
        assert_eq!((n, e.len()), (vec!["src".to_string()], 0));
        assert_eq!(std::fs::read_to_string(dst.join("src/sub/f.txt")).unwrap(), "hello");
        assert!(dst.join("src/link").symlink_metadata().unwrap().file_type().is_symlink());
        assert_eq!(transfer(Mode::Copy, std::slice::from_ref(&src), &dst).0, ["src (1)"]);
        // 自分の中へは断る
        let (_, e) = transfer(Mode::Copy, std::slice::from_ref(&src), &src.join("sub"));
        assert!(e[0].contains("into itself"));
        // 移動。同じ場所への移動は何もしない
        let f = src.join("sub/f.txt");
        assert_eq!(transfer(Mode::Move, std::slice::from_ref(&f), &src.join("sub")).0, ["f.txt"]);
        assert_eq!(transfer(Mode::Move, std::slice::from_ref(&f), &dst).0, ["f.txt"]);
        assert!(!f.exists() && dst.join("f.txt").exists());
        // 完全に消す
        assert!(remove(&[dst.join("src (1)")]).is_empty());
        assert!(!dst.join("src (1)").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 選ぶ画面() {
        let d = tmp("choice");
        let out = d.join("out");
        write_choice(&out, &[d.join("a b"), d.join("議事録.txt")]).unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), format!("{}/a b\n{}/議事録.txt\n", d.display(), d.display()));
        assert_eq!(save_target(&d, " x.txt "), Some(d.join("x.txt")));
        assert_eq!(save_target(&d, "a/b"), None);
        assert_eq!(nearest_dir(&d.join("no/such/dir")), d);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn クリップボード() {
        let d = tmp("clip");
        let a = d.join("a b.txt");
        std::fs::write(&a, "").unwrap();
        let text = clip_text(std::slice::from_ref(&a));
        assert_eq!(clip_paths(&text), vec![a.clone()]);
        // Dolphin などの file:// の URL も読む。無いもの・パスでない行は除く
        let url = format!("file://{}\n# comment\nhello\n/none/such", a.to_string_lossy().replace(' ', "%20"));
        assert_eq!(clip_paths(&url), vec![a.clone()]);
        // 切り取りの控え
        let note = d.join("clip.json");
        clip_note(&note, std::slice::from_ref(&a), true);
        assert!(clip_is_cut(&note, std::slice::from_ref(&a)));
        assert_eq!(cut_paths(&note), vec![a.clone()]);
        assert!(!clip_is_cut(&note, &[]));
        clip_consumed(&note);
        assert!(!clip_is_cut(&note, std::slice::from_ref(&a)));
        clip_note(&note, std::slice::from_ref(&a), false);
        assert!(!clip_is_cut(&note, std::slice::from_ref(&a)));
        assert!(cut_paths(&note).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}

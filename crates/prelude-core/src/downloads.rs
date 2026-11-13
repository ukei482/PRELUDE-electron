//! ダウンロードの見張り(docs/redesign-rust.md の「Web のペイン」の「ダウンロード」)。ブラウザの中とは話さないので、
//! ブラウザの保存先のフォルダ(設定の `download_dir`)を定期的に見て、現れたファイルを始めたタブのブックマークの paths へ移す。
//!
//! - ファイル(書きかけの `.crdownload`・`.part` を含む)が最初に現れたときに、移す先([`crate::state::Core::download_target`])を覚える。
//!   書きかけから本当の名前になったファイルは、書きかけが現れたときの移す先を使う(Chromium は `Unconfirmed 123.crdownload` →
//!   `名前.crdownload` → `名前`、Firefox は空の `名前` と `名前.part` → `名前`)
//! - 書き終わる(同じ名前の書きかけが無く、大きさが [`STABLE_TICKS`] 回続けて変わらない)と移す。同じ名前があれば `名前 (1).拡張子`
//! - 見張り始めたときからあるファイル・隠しファイル・移す先の無いファイルは動かさない
//! - OS の監視の仕組み(inotify など)には頼らない(ほかの OS にも載るように)
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 見る間隔
pub const POLL: std::time::Duration = std::time::Duration::from_secs(1);
/// 大きさが何回続けて変わらなければ書き終わったとみなすか
pub const STABLE_TICKS: u32 = 2;
/// 消えた書きかけを、本当の名前のファイルと結び付けるために覚えておく回数
const GONE_TICKS: u64 = 10;

/// 書きかけのファイルの印
const PARTIAL: [&str; 3] = [".crdownload", ".part", ".download"];

fn partial_stem(name: &str) -> Option<&str> {
    PARTIAL.iter().find_map(|x| name.strip_suffix(x))
}

/// 見つけたファイル 1 つ
#[derive(Debug, Clone)]
struct Seen {
    /// 最初に見た回(何回目の見張りか)
    first: u64,
    /// 最初に見たときの移す先
    target: Option<String>,
    size: u64,
    /// 大きさが変わらなかった回数
    stable: u32,
}

#[derive(Debug, Default)]
pub struct Watch {
    dir: PathBuf,
    tick: u64,
    /// 見張り始めたときからあるファイル(動かさない)
    old: Option<std::collections::HashSet<String>>,
    seen: HashMap<String, Seen>,
    /// 消えた書きかけ(名前 → 見つけたもの、消えた回)
    gone: HashMap<String, (Seen, u64)>,
}

impl Watch {
    pub fn new(dir: PathBuf) -> Watch {
        Watch { dir, ..Default::default() }
    }

    /// 見るフォルダ。変わったら見張り直す(そのときからあるファイルは動かさない)
    pub fn set_dir(&mut self, dir: PathBuf) {
        if dir != self.dir {
            *self = Watch::new(dir);
        }
    }

    /// 1 回見る。target は今の移す先(現れたファイルに覚える)。移したもの (元, 先) を返す
    pub fn tick(&mut self, target: &dyn Fn() -> Option<String>) -> Vec<(PathBuf, PathBuf)> {
        self.tick += 1;
        let now = list(&self.dir);
        let Some(old) = &self.old else {
            self.old = Some(now.keys().cloned().collect());
            return vec![];
        };
        let old = old.clone();
        // 消えたもの: 書きかけは覚えておく
        for name in self.seen.keys().filter(|n| !now.contains_key(*n)).cloned().collect::<Vec<_>>() {
            let s = self.seen.remove(&name).unwrap();
            if partial_stem(&name).is_some() {
                self.gone.insert(name, (s, self.tick));
            }
        }
        let tick = self.tick;
        self.gone.retain(|_, (_, t)| tick - *t <= GONE_TICKS);
        let mut cur: Option<Option<String>> = None;
        for (name, size) in &now {
            if old.contains(name) {
                continue;
            }
            match self.seen.get_mut(name) {
                Some(s) => {
                    s.stable = if s.size == *size { s.stable + 1 } else { 0 };
                    s.size = *size;
                }
                None => {
                    let t = cur.get_or_insert_with(target).clone();
                    self.seen.insert(name.clone(), Seen { first: tick, target: t, size: *size, stable: 0 });
                }
            }
        }
        // 書き終わったもの
        let mut out = vec![];
        let done: Vec<String> = self
            .seen
            .iter()
            .filter(|(n, s)| {
                partial_stem(n).is_none() && s.stable >= STABLE_TICKS && !PARTIAL.iter().any(|x| now.contains_key(&format!("{n}{x}")))
            })
            .map(|(n, _)| n.clone())
            .collect();
        for name in done {
            let target = self.origin(&name);
            self.seen.remove(&name);
            // 動かさないと決めたものも、二度と見ない
            if let Some(o) = self.old.as_mut() {
                o.insert(name.clone());
            }
            let from = self.dir.join(&name);
            let Some(to) = target.as_deref().and_then(|d| destination(&from, d)) else { continue };
            if let Some(d) = to.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let moved = std::fs::rename(&from, &to).or_else(|_| std::fs::copy(&from, &to).and_then(|_| std::fs::remove_file(&from)));
            match moved {
                Ok(()) => out.push((from, to)),
                Err(e) => eprintln!("prelude-core: cannot move {} to {} ({e})", from.display(), to.display()),
            }
        }
        out
    }

    /// 書き終わったファイルの移す先: そのファイルと、同じ名前の書きかけ・名前の決まる前の書きかけ(`Unconfirmed …`)のうち、
    /// 一番早く現れたものが覚えた移す先
    fn origin(&mut self, name: &str) -> Option<String> {
        let mut best: Option<(u64, Option<String>)> = self.seen.get(name).map(|s| (s.first, s.target.clone()));
        let mut used = vec![];
        for (n, (s, _)) in &self.gone {
            let mine = partial_stem(n) == Some(name) || n.starts_with("Unconfirmed ") || n.starts_with(".com.google.Chrome.");
            if mine && best.as_ref().is_none_or(|(f, _)| s.first < *f) {
                best = Some((s.first, s.target.clone()));
                used.push(n.clone());
            }
        }
        for n in used {
            self.gone.remove(&n);
        }
        best.and_then(|(_, t)| t)
    }
}

/// フォルダの中のファイル(名前 → 大きさ)。隠しファイルとフォルダは見ない
fn list(dir: &Path) -> HashMap<String, u64> {
    let Ok(rd) = std::fs::read_dir(dir) else { return HashMap::new() };
    rd.flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let m = e.metadata().ok().filter(|m| m.is_file())?;
            (!name.starts_with('.')).then_some((name, m.len()))
        })
        .collect()
}

/// 移す先のパス。同じ名前があれば「名前 (1)」にする。移さなくてよければ None
pub fn destination(from: &Path, dir: &str) -> Option<PathBuf> {
    if dir.is_empty() || from.parent() == Some(Path::new(dir)) {
        return None;
    }
    let name = from.file_name()?;
    let first = Path::new(dir).join(name);
    if !first.exists() {
        return Some(first);
    }
    let (stem, ext) =
        (from.file_stem()?.to_string_lossy(), from.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default());
    (1..1000).map(|i| Path::new(dir).join(format!("{stem} ({i}){ext}"))).find(|p| !p.exists())
}

/// 見るフォルダ: 設定の download_dir(~ はホーム)、空なら XDG のダウンロードのフォルダ(`user-dirs.dirs` の `XDG_DOWNLOAD_DIR`)、無ければ ~/Downloads
pub fn dir_of(setting: &str, home: &str) -> PathBuf {
    if !setting.trim().is_empty() {
        return PathBuf::from(prelude_format::abs_path(setting.trim(), home, home));
    }
    let conf = std::env::var("XDG_CONFIG_HOME").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| format!("{home}/.config"));
    let text = std::fs::read_to_string(Path::new(&conf).join("user-dirs.dirs")).unwrap_or_default();
    user_dirs_download(&text, home).unwrap_or_else(|| Path::new(home).join("Downloads"))
}

/// `user-dirs.dirs` の `XDG_DOWNLOAD_DIR="$HOME/ダウンロード"` を読む
fn user_dirs_download(text: &str, home: &str) -> Option<PathBuf> {
    let v = text.lines().find_map(|l| l.trim().strip_prefix("XDG_DOWNLOAD_DIR="))?.trim().trim_matches('"');
    let v = v.replace("$HOME", home);
    (!v.is_empty()).then(|| PathBuf::from(v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct T {
        root: PathBuf,
        dl: PathBuf,
    }
    impl T {
        fn new(name: &str) -> T {
            let root = std::env::temp_dir().join(format!("prelude-dl-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let dl = root.join("Downloads");
            std::fs::create_dir_all(&dl).unwrap();
            T { root, dl }
        }
        fn put(&self, name: &str, body: &str) {
            std::fs::write(self.dl.join(name), body).unwrap();
        }
        fn mv(&self, a: &str, b: &str) {
            std::fs::rename(self.dl.join(a), self.dl.join(b)).unwrap();
        }
        fn dest(&self, d: &str) -> String {
            self.root.join(d).to_string_lossy().into_owned()
        }
    }
    impl Drop for T {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn names(v: &[(PathBuf, PathBuf)]) -> Vec<String> {
        v.iter().map(|(_, to)| to.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn 書き終わったら始めたときのタブのフォルダへ移す() {
        let t = T::new("a");
        t.put("前からある.pdf", "x");
        let target = RefCell::new(Some(t.dest("研究")));
        let mut w = Watch::new(t.dl.clone());
        let tick = |w: &mut Watch| w.tick(&|| target.borrow().clone());
        // 見張り始め
        assert!(tick(&mut w).is_empty());
        // Chromium: 名前の決まる前の書きかけ → 名前の付いた書きかけ → 本当の名前
        t.put("Unconfirmed 123.crdownload", "ab");
        assert!(tick(&mut w).is_empty());
        *target.borrow_mut() = Some(t.dest("ほかのタブ")); // 現れた後に別のタブへ移っても、始めたときのタブ
        t.mv("Unconfirmed 123.crdownload", "論文.pdf.crdownload");
        assert!(tick(&mut w).is_empty());
        t.mv("論文.pdf.crdownload", "論文.pdf");
        assert!(tick(&mut w).is_empty());
        assert!(tick(&mut w).is_empty());
        let moved = tick(&mut w);
        assert_eq!(names(&moved), [t.root.join("研究/論文.pdf").to_string_lossy().into_owned()]);
        assert!(!t.dl.join("論文.pdf").exists());
        assert!(t.dl.join("前からある.pdf").exists());
        assert!(tick(&mut w).is_empty());
    }

    #[test]
    fn firefox_の書きかけと_名前が重なるとき() {
        let t = T::new("b");
        std::fs::create_dir_all(t.root.join("研究")).unwrap();
        std::fs::write(t.root.join("研究/a.txt"), "old").unwrap();
        let target = Some(t.dest("研究"));
        let mut w = Watch::new(t.dl.clone());
        let tick = |w: &mut Watch| w.tick(&|| target.clone());
        tick(&mut w);
        // 空の本当の名前と .part。.part がある間は移さない
        t.put("a.txt", "");
        t.put("a.txt.part", "abc");
        for _ in 0..4 {
            assert!(tick(&mut w).is_empty());
        }
        std::fs::remove_file(t.dl.join("a.txt")).unwrap();
        t.mv("a.txt.part", "a.txt");
        let mut moved = vec![];
        for _ in 0..4 {
            moved.extend(tick(&mut w));
        }
        assert_eq!(names(&moved), [t.root.join("研究/a (1).txt").to_string_lossy().into_owned()]);
        assert_eq!(std::fs::read_to_string(t.root.join("研究/a (1).txt")).unwrap(), "abc");
    }

    #[test]
    fn 移す先が無ければ動かさない() {
        let t = T::new("c");
        let mut w = Watch::new(t.dl.clone());
        w.tick(&|| None);
        t.put("x.zip", "1");
        for _ in 0..5 {
            assert!(w.tick(&|| None).is_empty());
        }
        assert!(t.dl.join("x.zip").exists());
        // 隠しファイルも見ない
        t.put(".x", "1");
        for _ in 0..5 {
            assert!(w.tick(&|| Some("/nowhere".into())).is_empty());
        }
    }

    #[test]
    fn 移す先のパスと見るフォルダ() {
        assert_eq!(destination(Path::new("/d/a.pdf"), ""), None);
        assert_eq!(destination(Path::new("/d/a.pdf"), "/d"), None);
        assert_eq!(dir_of("~/DL", "/home/u"), PathBuf::from("/home/u/DL"));
        let text = "# c\nXDG_DESKTOP_DIR=\"$HOME/デスクトップ\"\nXDG_DOWNLOAD_DIR=\"$HOME/ダウンロード\"\n";
        assert_eq!(user_dirs_download(text, "/home/u"), Some(PathBuf::from("/home/u/ダウンロード")));
        assert_eq!(user_dirs_download("", "/home/u"), None);
    }
}

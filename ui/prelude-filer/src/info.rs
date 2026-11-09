//! 一覧の見せ方に要る計算(GPUI に依らないのでテストできる)。Electron 版の `src/files.js`・`fileops.js` の移植。
//! 並べ替え・種類(色)・名前の先頭で飛ぶ・時刻の見せ方・プロパティ・ドライブの一覧
use crate::{Entry, natural_cmp};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

// ------------------------------------------------------------------ 並べ替え

/// 並べ替えの列(Electron 版の見出し Name・Type・Size・Modified)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    #[default]
    Name,
    Type,
    Size,
    Date,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sort {
    pub key: SortKey,
    pub desc: bool,
}

impl Sort {
    /// 見出しを押したとき: 同じ列なら向きを替え、別の列なら大きさ・時刻は大きい順、ほかは小さい順から
    pub fn clicked(self, key: SortKey) -> Sort {
        if self.key == key {
            Sort { key, desc: !self.desc }
        } else {
            Sort { key, desc: matches!(key, SortKey::Size | SortKey::Date) }
        }
    }

    /// 見出しの印(並べている列だけ ▲ / ▼)
    pub fn mark(self, key: SortKey) -> &'static str {
        match (self.key == key, self.desc) {
            (false, _) => "",
            (true, false) => "▲",
            (true, true) => "▼",
        }
    }
}

/// 拡張子(小文字。フォルダ・. で始まるだけの名前は空)
pub fn ext(e: &Entry) -> String {
    if e.is_dir {
        return String::new();
    }
    match e.name.rfind('.') {
        Some(i) if i > 0 => e.name[i + 1..].to_lowercase(),
        _ => String::new(),
    }
}

/// 並べる。フォルダはいつも先で、フォルダどうしは大きさが無いので、大きさの列でも名前順
pub fn sort_entries(v: &mut [Entry], s: Sort) {
    use std::cmp::Ordering;
    let by_name = |a: &Entry, b: &Entry| natural_cmp(&a.name, &b.name);
    let flip = |o: Ordering| if s.desc { o.reverse() } else { o };
    v.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| match s.key {
            SortKey::Size if a.is_dir => by_name(a, b),
            SortKey::Size => flip(a.size.cmp(&b.size)).then_with(|| by_name(a, b)),
            SortKey::Date => flip(a.modified.cmp(&b.modified)).then_with(|| by_name(a, b)),
            SortKey::Type => flip(natural_cmp(&ext(a), &ext(b))).then_with(|| by_name(a, b)),
            SortKey::Name => flip(by_name(a, b)),
        })
    });
}

// ------------------------------------------------------------------ 種類

/// ファイルの種類(Electron 版の KINDS。アイコンの色を決める)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Dir,
    Image,
    Video,
    Audio,
    Archive,
    Code,
    Text,
    Doc,
    /// ブックマークのファイル(.prelude)
    Bookmark,
    File,
}

pub fn kind_of(e: &Entry) -> Kind {
    if e.is_dir {
        return Kind::Dir;
    }
    let x = ext(e);
    let has = |list: &str| list.split(' ').any(|w| w == x);
    if x.is_empty() {
        Kind::File
    } else if has("png jpg jpeg gif webp svg bmp ico avif heic tif tiff") {
        Kind::Image
    } else if has("mp4 mkv webm mov avi m4v wmv flv") {
        Kind::Video
    } else if has("mp3 flac wav ogg opus m4a aac wma") {
        Kind::Audio
    } else if has("zip 7z rar tar gz tgz xz bz2 zst deb rpm iso appimage") {
        Kind::Archive
    } else if has("js mjs cjs ts jsx tsx rs py c cc cpp h hpp java kt go rb php lua html css scss json toml yaml yml xml sh bash zsh bat ps1 sql vue svelte nix") {
        Kind::Code
    } else if has("txt md log csv tsv ini conf cfg") {
        Kind::Text
    } else if has("pdf doc docx xls xlsx ppt pptx odt ods odp epub") {
        Kind::Doc
    } else if x == "prelude" {
        Kind::Bookmark
    } else {
        Kind::File
    }
}

impl Kind {
    /// アイコンの色(Electron 版の style.css の .ico.k-* と field.js の FIELD_COLORS)。None は文字の色
    pub fn color(self) -> Option<u32> {
        match self {
            Kind::Bookmark => Some(0xffd75f),
            Kind::Image => Some(0x7ddc8a),
            Kind::Video => Some(0xe08ad0),
            Kind::Audio => Some(0x9ff0ff),
            Kind::Archive => Some(0xf0b35a),
            Kind::Code => Some(0x5fe0d0),
            Kind::Doc => Some(0xff8a75),
            Kind::Dir | Kind::Text | Kind::File => None,
        }
    }

    /// アイコンの代わりの 1 文字
    pub fn glyph(self) -> &'static str {
        match self {
            Kind::Dir => "▸",
            Kind::Bookmark => "★",
            Kind::Image => "▣",
            Kind::Video => "▶",
            Kind::Audio => "♪",
            Kind::Archive => "▤",
            Kind::Code => "‹›",
            Kind::Text | Kind::Doc => "≡",
            Kind::File => "·",
        }
    }
}

// ------------------------------------------------------------------ 名前の先頭で飛ぶ

/// 打った文字(1 秒あけると打ち直し)
#[derive(Debug, Clone, Default)]
pub struct TypeAhead {
    typed: String,
    at: Option<Instant>,
}

impl TypeAhead {
    /// まだ打っている途中か(1 秒以内。この間の空白は選びの切り替えでなく名前の一部)
    pub fn active(&self, now: Instant) -> bool {
        self.at.is_some_and(|t| now.duration_since(t) < Duration::from_secs(1))
    }

    /// ch を足して、飛ぶ先を探す。cur: 今の位置。同じ文字の連打は次の候補へ
    pub fn push(&mut self, names: &[&str], cur: Option<usize>, ch: &str, now: Instant) -> Option<usize> {
        if !self.active(now) {
            self.typed.clear();
        }
        self.typed.push_str(&ch.to_lowercase());
        self.at = Some(now);
        let n = names.len();
        if n == 0 {
            return None;
        }
        let first = self.typed.chars().next()?;
        let repeat = self.typed.chars().count() > 1 && self.typed.chars().all(|c| c == first);
        let q = if repeat { first.to_string() } else { self.typed.clone() };
        let cur = cur.map(|c| c as isize).unwrap_or(-1);
        let start = if q.chars().count() == 1 { cur + 1 } else { cur.max(0) } as usize;
        (0..n).map(|k| (start + k) % n).find(|&i| names[i].to_lowercase().starts_with(&q))
    }
}

// ------------------------------------------------------------------ 時刻

/// 時刻を分けたもの(この PC の時間帯)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Local {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub min: u32,
}

pub fn local(t: SystemTime) -> Option<Local> {
    let secs = t.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs() as libc::time_t;
    // SAFETY: localtime_r は渡した tm に書くだけ
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&secs, &mut tm) }.is_null() {
        return None;
    }
    Some(Local { year: tm.tm_year + 1900, month: tm.tm_mon as u32 + 1, day: tm.tm_mday as u32, hour: tm.tm_hour as u32, min: tm.tm_min as u32 })
}

/// 一覧の時刻(Electron 版の fmtDate): 今日・昨日は「Today 12:34」、それより前は「2026/11/04 12:34」
pub fn fmt_date(t: Local, today: Local, yesterday: Local) -> String {
    let same = |a: Local, b: Local| (a.year, a.month, a.day) == (b.year, b.month, b.day);
    if same(t, today) {
        format!("Today {:02}:{:02}", t.hour, t.min)
    } else if same(t, yesterday) {
        format!("Yesterday {:02}:{:02}", t.hour, t.min)
    } else {
        format!("{:04}/{:02}/{:02} {:02}:{:02}", t.year, t.month, t.day, t.hour, t.min)
    }
}

/// 今の時刻で [`fmt_date`]
pub fn fmt_time(t: SystemTime) -> String {
    let now = SystemTime::now();
    match (local(t), local(now), local(now - Duration::from_secs(86_400))) {
        (Some(t), Some(a), Some(b)) => fmt_date(t, a, b),
        _ => String::new(),
    }
}

/// プロパティの時刻(「2026/11/04 12:34」)
pub fn full_time(t: SystemTime) -> String {
    local(t).map(|t| format!("{:04}/{:02}/{:02} {:02}:{:02}", t.year, t.month, t.day, t.hour, t.min)).unwrap_or_default()
}

// ------------------------------------------------------------------ プロパティ

/// プロパティの 1 つ(Electron 版の fs:props)
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PropItem {
    pub full: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
    /// rwxr-xr-x
    pub mode: String,
    pub uid: u32,
    /// シンボリックリンクの先
    pub link: Option<String>,
    /// フォルダの中の数
    pub count: Option<usize>,
    pub error: Option<String>,
}

/// rwxr-xr-x の形
pub fn mode_str(m: u32) -> String {
    [0o400, 0o200, 0o100, 0o40, 0o20, 0o10, 0o4, 0o2, 0o1].iter().enumerate().map(|(i, b)| if m & b != 0 { ['r', 'w', 'x'][i % 3] } else { '-' }).collect()
}

pub fn props(paths: &[PathBuf]) -> Vec<PropItem> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    paths
        .iter()
        .map(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
            let l = match std::fs::symlink_metadata(p) {
                Ok(l) => l,
                Err(e) => return PropItem { full: p.clone(), name, error: Some(e.to_string()), ..Default::default() },
            };
            let link = l.file_type().is_symlink().then(|| std::fs::read_link(p).map(|t| t.display().to_string()).unwrap_or_default());
            let st = if link.is_some() { std::fs::metadata(p).unwrap_or(l) } else { l };
            let is_dir = st.is_dir();
            PropItem {
                full: p.clone(),
                name,
                is_dir,
                size: st.len(),
                modified: st.modified().ok(),
                created: st.created().ok(),
                mode: mode_str(st.permissions().mode()),
                uid: st.uid(),
                link,
                count: is_dir.then(|| std::fs::read_dir(p).map(|r| r.count()).unwrap_or(0)),
                error: None,
            }
        })
        .collect()
}

/// 中身の合計(Electron 版の fs:dirsize)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DirSize {
    pub size: u64,
    pub files: u64,
    pub dirs: u64,
    /// 時間切れで途中まで
    pub partial: bool,
}

/// paths の中身の合計の大きさ。limit を過ぎたら途中までの値を返す。シンボリックリンクはたどらない
pub fn dir_size(paths: &[PathBuf], limit: Duration) -> DirSize {
    let until = Instant::now() + limit;
    let mut r = DirSize::default();
    fn walk(p: &Path, until: Instant, r: &mut DirSize) {
        if Instant::now() > until {
            r.partial = true;
            return;
        }
        let Ok(st) = std::fs::symlink_metadata(p) else { return };
        if !st.is_dir() {
            r.size += st.len();
            r.files += 1;
            return;
        }
        r.dirs += 1;
        let Ok(rd) = std::fs::read_dir(p) else { return };
        for e in rd.flatten() {
            walk(&e.path(), until, r);
        }
    }
    for p in paths {
        walk(p, until, &mut r);
    }
    r
}

/// 3 桁ごとに , を入れる(1,234,567)
pub fn grouped(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

// ------------------------------------------------------------------ ドライブ・場所

/// ドライブと場所(Electron 版の fs:drives): ホーム・/・/mnt・/media・/run/media/<使用者> の下のフォルダ
pub fn drives(home: &Path, user: &str, root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = vec![("Home".to_string(), home.to_path_buf()), ("/".to_string(), root.to_path_buf())];
    let bases = [root.join("mnt"), root.join("media"), root.join("run/media").join(user), root.join("media").join(user)];
    for base in bases {
        let Ok(rd) = std::fs::read_dir(&base) else { continue };
        let mut v: Vec<PathBuf> = rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.path()).collect();
        v.sort();
        for p in v {
            if !out.iter().any(|(_, q)| *q == p) {
                out.push((p.display().to_string(), p));
            }
        }
    }
    out
}

/// パスの欄に打ったもの: ~ はホーム、相対のパスは今のフォルダから
pub fn resolve_input(text: &str, cwd: &Path, home: &Path) -> Option<PathBuf> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let p = if t == "~" {
        home.to_path_buf()
    } else if let Some(r) = t.strip_prefix("~/") {
        home.join(r)
    } else if Path::new(t).is_absolute() {
        PathBuf::from(t)
    } else {
        cwd.join(t)
    };
    // . と .. を畳む(フォルダ名の表示と、上へ戻るため)
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            c => out.push(c),
        }
    }
    Some(out)
}

/// キーの一覧(F1。Electron 版の FILE_KEYS から、GPUI 版にあるものだけ)
pub const KEYS: &[(&str, &str)] = &[
    ("↑ ↓ / PageUp PageDown / Home End", "Move"),
    ("Shift + move", "Select range"),
    ("Ctrl + move", "Move cursor without changing selection"),
    ("Space / Insert", "Toggle selection at cursor (Insert also moves down)"),
    ("Ctrl+A / Esc", "Select all / Clear selection"),
    ("Type letters", "Jump to that name"),
    ("Enter", "Open (enter folder)"),
    ("Ctrl+Enter", "Open in a split on the right"),
    ("Backspace / Alt+↑", "Parent folder"),
    ("Alt+← / Alt+→", "Back / Forward"),
    ("Alt+Home", "Home folder"),
    ("Ctrl+L", "Type a path (Esc returns to the list)"),
    ("F5 / Ctrl+R", "Reload"),
    ("Ctrl+C / Ctrl+X / Ctrl+V", "Copy / Cut / Paste"),
    ("F2", "Rename"),
    ("Delete / Shift+Delete", "Trash / Delete permanently"),
    ("Ctrl+Shift+N", "New folder"),
    ("Ctrl+Shift+C", "Copy path"),
    ("Ctrl+H", "Toggle hidden files"),
    ("Shift+F4", "Open terminal here"),
    ("Alt+Enter", "Properties"),
    ("Menu / Shift+F10", "Context menu (choose with ↑↓ and Enter)"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, is_dir: bool, size: u64, secs: u64) -> Entry {
        Entry { name: name.into(), is_dir, size, modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs)), hidden: name.starts_with('.') }
    }

    fn names(v: &[Entry]) -> Vec<&str> {
        v.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn 並べ替え_フォルダが先() {
        let mut v = vec![e("b.txt", false, 30, 1), e("a.rs", false, 10, 3), e("z", true, 0, 9), e("c.md", false, 20, 2), e("y", true, 0, 1)];
        sort_entries(&mut v, Sort::default());
        assert_eq!(names(&v), ["y", "z", "a.rs", "b.txt", "c.md"]);
        sort_entries(&mut v, Sort { key: SortKey::Name, desc: true });
        assert_eq!(names(&v), ["z", "y", "c.md", "b.txt", "a.rs"]);
        // 大きさの列でも、フォルダは名前順
        sort_entries(&mut v, Sort { key: SortKey::Size, desc: true });
        assert_eq!(names(&v), ["y", "z", "b.txt", "c.md", "a.rs"]);
        sort_entries(&mut v, Sort { key: SortKey::Date, desc: false });
        assert_eq!(names(&v), ["y", "z", "b.txt", "c.md", "a.rs"]);
        sort_entries(&mut v, Sort { key: SortKey::Type, desc: false });
        assert_eq!(names(&v), ["y", "z", "c.md", "a.rs", "b.txt"]);
    }

    #[test]
    fn 見出しを押す() {
        let s = Sort::default();
        assert_eq!(s.clicked(SortKey::Name), Sort { key: SortKey::Name, desc: true });
        assert_eq!(s.clicked(SortKey::Size), Sort { key: SortKey::Size, desc: true });
        assert_eq!(s.clicked(SortKey::Type), Sort { key: SortKey::Type, desc: false });
        assert_eq!(s.mark(SortKey::Name), "▲");
        assert_eq!(s.mark(SortKey::Date), "");
    }

    #[test]
    fn 種類() {
        assert_eq!(kind_of(&e("a.PNG", false, 0, 0)), Kind::Image);
        assert_eq!(kind_of(&e("x.prelude", false, 0, 0)), Kind::Bookmark);
        assert_eq!(kind_of(&e("x.tar.gz", false, 0, 0)), Kind::Archive);
        assert_eq!(kind_of(&e(".bashrc", false, 0, 0)), Kind::File);
        assert_eq!(kind_of(&e("pics.png", true, 0, 0)), Kind::Dir);
        assert_eq!(ext(&e("a.Tar.GZ", false, 0, 0)), "gz");
    }

    #[test]
    fn 名前の先頭で飛ぶ() {
        let list = ["apple", "banana", "blueberry", "cherry", "Bean"];
        let t0 = Instant::now();
        let mut ta = TypeAhead::default();
        assert_eq!(ta.push(&list, Some(0), "b", t0), Some(1));
        // 続けて打つと、今の位置から先頭が合うもの
        assert_eq!(ta.push(&list, Some(1), "l", t0), Some(2));
        // 1 秒あけると打ち直し
        let later = t0 + Duration::from_millis(1500);
        assert!(!ta.active(later));
        assert_eq!(ta.push(&list, Some(2), "b", later), Some(4));
        // 同じ文字の連打は次の候補へ
        assert_eq!(ta.push(&list, Some(4), "b", later), Some(1));
        assert_eq!(ta.push(&list, Some(1), "b", later), Some(2));
        assert_eq!(TypeAhead::default().push(&list, None, "z", t0), None);
    }

    #[test]
    fn 時刻の見せ方() {
        let l = |d: u32, h: u32, m: u32| Local { year: 2026, month: 11, day: d, hour: h, min: m };
        assert_eq!(fmt_date(l(9, 8, 5), l(9, 20, 0), l(8, 20, 0)), "Today 08:05");
        assert_eq!(fmt_date(l(8, 23, 59), l(9, 20, 0), l(8, 20, 0)), "Yesterday 23:59");
        assert_eq!(fmt_date(l(1, 0, 0), l(9, 20, 0), l(8, 20, 0)), "2026/11/01 00:00");
    }

    #[test]
    fn 権限と桁() {
        assert_eq!(mode_str(0o755), "rwxr-xr-x");
        assert_eq!(mode_str(0o640), "rw-r-----");
        assert_eq!(grouped(1234567), "1,234,567");
        assert_eq!(grouped(999), "999");
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("prelude-info-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn プロパティと合計() {
        let d = tmp("props");
        std::fs::create_dir(d.join("sub")).unwrap();
        std::fs::write(d.join("sub/a"), "12345").unwrap();
        std::fs::write(d.join("b"), "123").unwrap();
        std::os::unix::fs::symlink(d.join("b"), d.join("l")).unwrap();
        let v = props(&[d.join("sub"), d.join("l"), d.join("none")]);
        assert_eq!((v[0].is_dir, v[0].count), (true, Some(1)));
        assert_eq!((v[1].size, v[1].link.as_deref()), (3, Some(d.join("b").to_str().unwrap())));
        assert!(v[2].error.is_some());
        let s = dir_size(std::slice::from_ref(&d), Duration::from_secs(5));
        // sub/a と b と l(リンクそのものの大きさ)
        assert_eq!((s.files, s.dirs, s.partial), (3, 2, false));
        assert!(s.size >= 8);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn ドライブ() {
        let r = tmp("drives");
        std::fs::create_dir_all(r.join("mnt/usb")).unwrap();
        std::fs::create_dir_all(r.join("run/media/me/DISK")).unwrap();
        std::fs::write(r.join("mnt/file"), "").unwrap();
        let v = drives(Path::new("/home/me"), "me", &r);
        let labels: Vec<&str> = v.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(labels[..2], ["Home", "/"]);
        assert!(v.iter().any(|(_, p)| *p == r.join("mnt/usb")));
        assert!(v.iter().any(|(_, p)| *p == r.join("run/media/me/DISK")));
        assert_eq!(v.len(), 4);
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn パスの欄() {
        let (cwd, home) = (Path::new("/a/b"), Path::new("/home/me"));
        assert_eq!(resolve_input("~", cwd, home), Some(PathBuf::from("/home/me")));
        assert_eq!(resolve_input("~/x", cwd, home), Some(PathBuf::from("/home/me/x")));
        assert_eq!(resolve_input("../c/./d", cwd, home), Some(PathBuf::from("/a/c/d")));
        assert_eq!(resolve_input(" /etc ", cwd, home), Some(PathBuf::from("/etc")));
        assert_eq!(resolve_input("  ", cwd, home), None);
    }
}

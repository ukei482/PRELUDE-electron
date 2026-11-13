//! PRELUDE のファイラ(docs/redesign-rust.md の「ファイラ」、移行の段階 5。外のファイラに任せるので消す予定)。
//!
//! - フォルダの中身は [`DirState`](読み方・並べ方はここ。GPUI に依らないのでテストできる)
//! - 見せ方は [`FilerView`] のトレイトを満たす部品。一覧([`views::ListView`])のほか、フィールド表示・写真を大きく並べるものなどを足せる
//! - 枠([`Filer`]、main.rs)はキー・移動・開く・コアへの知らせを受け持ち、見せ方を差し替えても同じに動く
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub mod info;
pub mod ops;
pub mod views;

/// フォルダの中の 1 つ
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    /// ファイルの大きさ(フォルダは 0)
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// 名前が . で始まる
    pub hidden: bool,
}

/// `*` と `?` の glob。大文字と小文字は区別しない
pub fn glob_match(glob: &str, name: &str) -> bool {
    fn go(g: &[char], n: &[char]) -> bool {
        match g.split_first() {
            None => n.is_empty(),
            Some(('*', rest)) => (0..=n.len()).any(|i| go(rest, &n[i..])),
            Some(('?', rest)) => !n.is_empty() && go(rest, &n[1..]),
            Some((c, rest)) => n.first().is_some_and(|x| x == c) && go(rest, &n[1..]),
        }
    }
    let low = |s: &str| s.to_lowercase().chars().collect::<Vec<char>>();
    go(&low(glob), &low(name))
}

/// 選ぶ画面の `--filter`(`名前:*.png;*.jpg|名前:*.pdf`)
pub fn parse_filters(s: &str) -> Vec<(String, Vec<String>)> {
    s.split('|')
        .filter_map(|f| {
            let (name, globs) = f.split_once(':')?;
            let g: Vec<String> = globs.split(';').filter(|g| !g.is_empty()).map(str::to_string).collect();
            (!g.is_empty()).then(|| (name.to_string(), g))
        })
        .collect()
}

/// 開いているフォルダと、選んでいるもの
#[derive(Debug, Clone, Default)]
pub struct DirState {
    pub path: PathBuf,
    /// 見せるもの(隠しファイルを出さないときは除いたもの)。フォルダが先、名前順
    pub entries: Vec<Entry>,
    /// 選んでいる位置(entries の中)
    pub selected: Option<usize>,
    pub show_hidden: bool,
    /// 選ぶ画面のファイルの種類(glob)。空なら全部。フォルダはいつも出す
    pub filter: Vec<String>,
    /// 読めなかったときの理由
    pub error: Option<String>,
    /// 読んだときのフォルダの更新時刻(変わったら読み直す)
    pub stamp: Option<SystemTime>,
    /// 複数選んでいるもの(名前。読み直しても残る)。空なら選んでいる 1 つ([`DirState::selected`])が操作の対象
    pub marked: BTreeSet<String>,
    /// Shift で範囲を選ぶときの起点
    pub anchor: Option<usize>,
    /// 並べ方(見出しを押して替える)
    pub sort: info::Sort,
    /// 切り取ったもの(名前。薄く出す)。枠がクリップボードの控えから決める
    pub cut: BTreeSet<String>,
}

impl DirState {
    /// path を読む。選んでいたものは名前で選び直す(無ければ先頭)
    pub fn load(&mut self, path: &Path) {
        let same = self.path == path;
        let keep = self.selected_entry().map(|e| e.name.clone()).filter(|_| same);
        if !same {
            self.marked.clear();
            self.anchor = None;
        }
        self.path = path.to_path_buf();
        self.stamp = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        match read_dir(path) {
            Ok(v) => {
                let shown = |e: &Entry| e.is_dir || self.filter.is_empty() || self.filter.iter().any(|g| glob_match(g, &e.name));
                self.entries = v.into_iter().filter(|e| (self.show_hidden || !e.hidden) && shown(e)).collect();
                info::sort_entries(&mut self.entries, self.sort);
                self.error = None;
            }
            Err(e) => {
                self.entries.clear();
                self.error = Some(e);
            }
        }
        self.selected = keep.and_then(|n| self.entries.iter().position(|e| e.name == n)).or((!self.entries.is_empty()).then_some(0));
        // 無くなったものは選びから外す
        let names: BTreeSet<&str> = self.entries.iter().map(|e| e.name.as_str()).collect();
        self.marked.retain(|n| names.contains(n.as_str()));
    }

    /// i の選びを入れたり外したりする(Ctrl+クリック)。最初の 1 回は、それまで選んでいた 1 つも入れる
    pub fn toggle_mark(&mut self, i: usize) {
        let Some(name) = self.entries.get(i).map(|e| e.name.clone()) else { return };
        if self.marked.is_empty()
            && let Some(cur) = self.selected_entry().map(|e| e.name.clone())
            && cur != name
        {
            self.marked.insert(cur);
        }
        if !self.marked.remove(&name) {
            self.marked.insert(name);
        }
        self.selected = Some(i);
        self.anchor = Some(i);
    }

    /// 起点から i までを選ぶ(Shift+クリック)
    pub fn mark_range(&mut self, i: usize) {
        if i >= self.entries.len() {
            return;
        }
        let a = self.anchor.or(self.selected).unwrap_or(i).min(self.entries.len() - 1);
        let (lo, hi) = if a <= i { (a, i) } else { (i, a) };
        self.marked = self.entries[lo..=hi].iter().map(|e| e.name.clone()).collect();
        self.selected = Some(i);
        self.anchor = Some(a);
    }

    /// 1 つだけを選ぶ(普通のクリック・矢印)
    pub fn select_one(&mut self, i: usize) {
        if i < self.entries.len() {
            self.selected = Some(i);
            self.anchor = Some(i);
            self.marked.clear();
        }
    }

    /// 囲んで選ぶ(Electron 版の bandSelect): base(Ctrl なら始める前の選び)に lo..=hi を足す。選んでいる位置は cur
    pub fn mark_band(&mut self, base: &BTreeSet<String>, lo: usize, hi: usize, cur: usize) {
        let mut m = base.clone();
        m.extend(self.entries.iter().skip(lo).take(hi.saturating_sub(lo) + 1).map(|e| e.name.clone()));
        self.marked = m;
        if cur < self.entries.len() {
            self.selected = Some(cur);
        }
    }

    pub fn mark_all(&mut self) {
        self.marked = self.entries.iter().map(|e| e.name.clone()).collect();
    }

    /// 操作の対象: 複数選んでいればそれ、無ければ選んでいる 1 つ
    pub fn targets(&self) -> Vec<PathBuf> {
        if self.marked.is_empty() {
            self.selected_entry().map(|e| vec![self.path.join(&e.name)]).unwrap_or_default()
        } else {
            self.entries.iter().filter(|e| self.marked.contains(&e.name)).map(|e| self.path.join(&e.name)).collect()
        }
    }

    pub fn is_marked(&self, i: usize) -> bool {
        self.entries.get(i).is_some_and(|e| self.marked.contains(&e.name))
    }

    /// 並べ方を変える。選んでいるもの・複数選びは同じファイルに残す
    pub fn set_sort(&mut self, sort: info::Sort) {
        let cur = self.selected_entry().map(|e| e.name.clone());
        self.sort = sort;
        info::sort_entries(&mut self.entries, sort);
        if let Some(n) = cur {
            self.select_name(&n);
        }
        self.anchor = self.selected;
    }

    /// 選びを変えずに、選んでいる位置だけを d 動かす(Ctrl+↑↓)。それまで 1 つだけ選んでいたら、それを複数選びに入れて残す
    pub fn move_cursor_only(&mut self, d: isize) {
        if self.marked.is_empty()
            && let Some(n) = self.selected_entry().map(|e| e.name.clone())
        {
            self.marked.insert(n);
        }
        self.move_by(d);
    }

    /// 下の行の件数(Electron 版の updateStatus): 「12 items (3 folders)」、選んでいれば「2 selected (1.5 KB) / 12 items」
    pub fn status_text(&self) -> String {
        let n = self.entries.len();
        if self.marked.is_empty() {
            let dirs = self.entries.iter().filter(|e| e.is_dir).count();
            return if dirs > 0 { format!("{n} items ({dirs} folders)") } else { format!("{n} items") };
        }
        let size: u64 = self.entries.iter().filter(|e| !e.is_dir && self.marked.contains(&e.name)).map(|e| e.size).sum();
        let size = if size > 0 { format!(" ({})", human_size(size)) } else { String::new() };
        format!("{} selected{size} / {n} items", self.marked.len())
    }

    /// 読み直す(隠しファイルの切り替え・フォルダが変わったとき)
    pub fn reload(&mut self) {
        let p = self.path.clone();
        self.load(&p);
    }

    /// フォルダが前に読んだときから変わったか
    pub fn changed(&self) -> bool {
        std::fs::metadata(&self.path).and_then(|m| m.modified()).ok() != self.stamp
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        self.selected.and_then(|i| self.entries.get(i))
    }

    /// 名前で選ぶ(上のフォルダへ戻ったとき、出てきたフォルダを選ぶ)
    pub fn select_name(&mut self, name: &str) {
        if let Some(i) = self.entries.iter().position(|e| e.name == name) {
            self.selected = Some(i);
        }
    }

    /// 選びを d だけ動かす(端で止まる)
    pub fn move_by(&mut self, d: isize) {
        if self.entries.is_empty() {
            return;
        }
        let last = self.entries.len() as isize - 1;
        let cur = self.selected.map(|i| i as isize).unwrap_or(if d > 0 { -1 } else { last + 1 });
        self.selected = Some((cur + d).clamp(0, last) as usize);
    }
}

/// フォルダを読む。フォルダが先、名前順(大文字・小文字を区別しない。数字は数の大きさで比べる)
pub fn read_dir(path: &Path) -> Result<Vec<Entry>, String> {
    let rd = std::fs::read_dir(path).map_err(|e| e.to_string())?;
    let mut v: Vec<Entry> = rd
        .flatten()
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            // シンボリックリンクは先のものを見る(フォルダへのリンクはフォルダとして入れる)
            let meta = std::fs::metadata(e.path()).or_else(|_| e.metadata()).ok();
            Entry {
                hidden: name.starts_with('.'),
                is_dir: meta.as_ref().is_some_and(|m| m.is_dir()),
                size: meta.as_ref().filter(|m| m.is_file()).map(|m| m.len()).unwrap_or(0),
                modified: meta.and_then(|m| m.modified().ok()),
                name,
            }
        })
        .collect();
    v.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| natural_cmp(&a.name, &b.name)));
    Ok(v)
}

/// 名前の比べ方: 大文字・小文字を区別せず、数字の並びは数として比べる(file2 < file10)
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(c), Some(d)) if c.is_ascii_digit() && d.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        s.push(c);
                        it.next();
                    }
                    s
                };
                let (n, m) = (take(&mut x), take(&mut y));
                let (n0, m0) = (n.trim_start_matches('0'), m.trim_start_matches('0'));
                let o = n0.len().cmp(&m0.len()).then_with(|| n0.cmp(m0));
                if o != Ordering::Equal {
                    return o;
                }
            }
            (Some(c), Some(d)) => {
                let o = c.to_lowercase().cmp(d.to_lowercase());
                if o != Ordering::Equal {
                    return o;
                }
                x.next();
                y.next();
            }
        }
    }
}

/// 大きさを短く(1.2 KB など)
pub fn human_size(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024. && u < UNITS.len() - 1 {
        v /= 1024.;
        u += 1;
    }
    if u == 0 {
        format!("{n} B")
    } else if v < 10. {
        format!("{v:.1} {}", UNITS[u])
    } else {
        format!("{v:.0} {}", UNITS[u])
    }
}

/// 更新時刻を「2026-11-04 12:34」に(この PC の時間帯)
pub fn local_time(t: SystemTime) -> String {
    let secs = match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as libc::time_t,
        Err(_) => return String::new(),
    };
    // SAFETY: localtime_r は渡した tm に書くだけ
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&secs, &mut tm) }.is_null() {
        return String::new();
    }
    format!("{:04}-{:02}-{:02} {:02}:{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min)
}

/// 見せ方から枠へ頼みごとを渡す口(行を押したときなどに呼ぶ)
pub type Emit = std::rc::Rc<dyn Fn(FilerCommand, &mut gpui::Window, &mut gpui::App)>;

/// 見せ方の部品(`FilerView`)。枠が持つ [`DirState`] を描くだけで、操作は [`FilerCommand`] にして枠に渡す。
/// キー・移動・開く・コアへの知らせは枠が受け持つので、部品を差し替えても同じに動く。部品は枠と一緒にビルドするので、版は付けない(「トレイトの版」)
pub trait FilerView: 'static {
    /// 名前(`--view` で選ぶ)
    fn name(&self) -> &'static str;
    /// 描く。選んでいる位置が見えるようにするのも部品の仕事
    fn render(&mut self, dir: &DirState, emit: Emit) -> gpui::AnyElement;
}

/// 見せ方から枠への頼みごと
#[derive(Debug, Clone, PartialEq)]
pub enum FilerCommand {
    /// その位置だけを選ぶ
    Select(usize),
    /// その位置の選びを入れたり外したりする(Ctrl+クリック)
    Toggle(usize),
    /// 起点からその位置までを選ぶ(Shift+クリック)
    Range(usize),
    /// その位置を開く(フォルダなら入る、ファイルならアプリで開く)
    Open(usize),
    /// その位置で右クリック(選んでいなければ 1 つだけ選び、メニューを出す。None は空いた所)
    Menu(Option<usize>, gpui::Point<gpui::Pixels>),
    /// 並べ方を変える(見出しを押した)
    Sort(info::SortKey),
    /// 戻る・進む(マウスの横のボタン)
    History(bool),
    /// 囲んで選ぶ途中: かかった行の範囲・選んでいる位置・Ctrl を押していたか。None で終わり
    Band(Option<(usize, usize, usize, bool)>),
    /// フォルダへ移る
    Go(PathBuf),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 名前の_glob() {
        assert!(glob_match("*.png", "a.PNG"));
        assert!(glob_match("*", "x"));
        assert!(glob_match("a?c.*", "abc.txt"));
        assert!(!glob_match("*.png", "a.jpg"));
        assert!(!glob_match("?", ""));
        assert!(glob_match("*.tar.*", "x.tar.gz"));
    }

    #[test]
    fn ファイルの種類の文字() {
        assert_eq!(parse_filters("画像:*.png;*.jpg|全部:*"), vec![("画像".to_string(), vec!["*.png".to_string(), "*.jpg".to_string()]), ("全部".to_string(), vec!["*".to_string()])]);
        assert!(parse_filters("").is_empty());
        assert!(parse_filters("名前だけ:").is_empty());
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("prelude-filer-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn 名前の比べ方() {
        let mut v = vec!["file10", "File2", "file1", "b", "A", "file02x", "資料"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["A", "b", "file1", "File2", "file02x", "file10", "資料"]);
    }

    #[test]
    fn 読む_フォルダが先_隠しファイル_選び直し() {
        let d = tmp("read");
        for f in ["b.txt", "a10.txt", "a2.txt", ".hidden"] {
            std::fs::write(d.join(f), "xx").unwrap();
        }
        std::fs::create_dir(d.join("z")).unwrap();
        std::os::unix::fs::symlink(d.join("z"), d.join("link-to-z")).unwrap();
        let mut s = DirState::default();
        s.load(&d);
        let names: Vec<&str> = s.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["link-to-z", "z", "a2.txt", "a10.txt", "b.txt"]);
        assert_eq!((s.selected, s.entries[2].size), (Some(0), 2));
        s.select_name("b.txt");
        s.show_hidden = true;
        s.reload();
        assert_eq!(s.entries.len(), 6);
        assert_eq!(s.selected_entry().unwrap().name, "b.txt"); // 読み直しても同じものを選ぶ
        s.move_by(10);
        assert_eq!(s.selected, Some(5));
        s.move_by(-100);
        assert_eq!(s.selected, Some(0));
        // 複数選び: Ctrl で足す・外す、Shift で範囲。操作の対象
        s.select_one(1);
        s.toggle_mark(3);
        assert_eq!(s.marked.len(), 2);
        assert_eq!(s.targets().len(), 2);
        s.toggle_mark(3);
        assert_eq!(s.targets(), [d.join(&s.entries[1].name)]);
        s.select_one(1);
        s.mark_range(5); // z・.hidden・a2・a10・b(隠しファイルも出している)
        assert_eq!(s.marked.len(), 5);
        assert!(s.is_marked(2) && !s.is_marked(0));
        // 消えたものは選びから外れる
        std::fs::remove_file(d.join("b.txt")).unwrap();
        s.reload();
        assert_eq!(s.marked.len(), 4);
        s.select_one(0);
        assert!(s.marked.is_empty());
        // 読めないフォルダ
        s.load(&d.join("none"));
        assert!(s.error.is_some() && s.entries.is_empty() && s.selected.is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 並べ方を変えても同じものを選ぶ_件数() {
        let d = tmp("sort");
        std::fs::write(d.join("a.txt"), "1").unwrap();
        std::fs::write(d.join("b.txt"), "12345").unwrap();
        std::fs::create_dir(d.join("sub")).unwrap();
        let mut s = DirState::default();
        s.load(&d);
        assert_eq!(s.status_text(), "3 items (1 folders)");
        s.select_name("a.txt");
        s.set_sort(info::Sort { key: info::SortKey::Size, desc: true });
        let names: Vec<&str> = s.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["sub", "b.txt", "a.txt"]);
        assert_eq!(s.selected_entry().unwrap().name, "a.txt");
        // 読み直しても並べ方は残る
        s.reload();
        assert_eq!(s.entries[1].name, "b.txt");
        // Ctrl+↑: 選びを変えずに位置だけ動かす
        s.move_cursor_only(-1);
        assert_eq!(s.selected, Some(1));
        assert_eq!(s.targets(), [d.join("a.txt")]);
        s.toggle_mark(1);
        assert_eq!(s.status_text(), "2 selected (6 B) / 3 items");
        // 囲んで選ぶ: Ctrl なしは囲んだものだけ、Ctrl ありは前の選びに足す
        s.mark_band(&BTreeSet::new(), 0, 1, 1);
        assert_eq!(s.marked.iter().map(String::as_str).collect::<Vec<_>>(), ["b.txt", "sub"]);
        s.mark_band(&["a.txt".to_string()].into(), 0, 0, 0);
        assert_eq!(s.marked.len(), 2);
        assert_eq!(s.selected, Some(0));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 大きさ() {
        assert_eq!(human_size(12), "12 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(50 * 1024 * 1024), "50 MB");
    }
}

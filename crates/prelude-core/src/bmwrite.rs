//! ブックマークのファイルを書く命令(docs/redesign-rust.md の「サイドバー」の「コアに足すもの」)。
//! 使用者がサイドバーなどで頼んだことを、コアが代わりに書く。書くのは今のスペースの中(直下と、その 1 段下のフォルダ)だけ。
//! 書いたら `bookmarks_rev` を増やし、開いているタブ・閉じたタブ・ランチャーの点数が指すパスも合わせる
use super::{Core, Fail, Reply, fail, not_found, stem, valid_name};
use prelude_format::{Bookmark, EXT, file_name};
use prelude_proto::{ErrorCode, TabRef};
use serde_json::json;
use std::path::{Path, PathBuf};

fn io(e: std::io::Error) -> Fail {
    fail(ErrorCode::Io, e.to_string())
}

/// dir の中で使われていない名前。あれば「名前 (2)」「名前 (3)」…
fn unique(dir: &Path, file: &str) -> PathBuf {
    let first = dir.join(file);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match file.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (file.to_string(), String::new()),
    };
    (2..).map(|n| dir.join(format!("{stem} ({n}){ext}"))).find(|p| !p.exists()).unwrap()
}

/// 書きかけのファイルが残らないよう、隣に書いてから置き換える
fn write_bookmark(path: &Path, bm: &Bookmark, home: &str) -> Result<(), Fail> {
    let tmp = path.with_extension(format!("{EXT}.tmp"));
    std::fs::write(&tmp, bm.to_toml(home)).map_err(io)?;
    std::fs::rename(&tmp, path).map_err(io)
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

impl Core {
    /// 今のスペースの中のブックマークのファイル(直下か、1 段下のフォルダ)か確かめる
    fn space_bookmark(&self, path: &str) -> Result<PathBuf, Fail> {
        let p = Path::new(path);
        if !p.is_absolute() || p.extension().and_then(|e| e.to_str()) != Some(EXT) {
            return Err(fail(ErrorCode::BadRequest, "Not a bookmark file"));
        }
        if !p.is_file() {
            return Err(not_found("bookmark"));
        }
        let (Ok(real), Ok(root)) = (std::fs::canonicalize(p), std::fs::canonicalize(self.space_dir())) else {
            return Err(not_found("bookmark"));
        };
        let dir = real.parent().unwrap_or(&real);
        if dir != root && dir.parent() != Some(root.as_path()) {
            return Err(fail(ErrorCode::BadRequest, "The bookmark is not in this space"));
        }
        Ok(p.to_path_buf())
    }

    /// 今のスペースのブックマークのフォルダ。空ならスペースの直下。empty_ok でなければ、空は断る
    fn space_folder(&self, path: &str, empty_ok: bool) -> Result<PathBuf, Fail> {
        if path.is_empty() {
            return if empty_ok { Ok(self.space_dir()) } else { Err(fail(ErrorCode::BadRequest, "No folder")) };
        }
        let p = Path::new(path);
        let hidden = p.file_name().and_then(|n| n.to_str()).is_none_or(|n| n.starts_with('.'));
        let (Ok(real), Ok(root)) = (std::fs::canonicalize(p), std::fs::canonicalize(self.space_dir())) else {
            return Err(not_found("folder"));
        };
        if hidden || !real.is_dir() || real.parent() != Some(root.as_path()) {
            return Err(fail(ErrorCode::BadRequest, "Not a bookmark folder of this space"));
        }
        Ok(self.space_dir().join(p.file_name().unwrap()))
    }

    /// ブックマークのパスが変わった(new が None なら消えた)ことを、タブ・閉じたタブ・点数に伝える。
    /// prefix なら、old の下のパスを全部 new の下にする(フォルダの名前を変えたとき)
    fn relink(&mut self, old: &str, new: Option<(&str, Option<&str>)>, prefix: bool) {
        let map = |p: &str| -> Option<Option<String>> {
            if p == old {
                return Some(new.map(|(n, _)| n.to_string()));
            }
            let rest = p.strip_prefix(old).filter(|r| prefix && r.starts_with('/'))?;
            Some(new.map(|(n, _)| format!("{n}{rest}")))
        };
        let fix = |b: &mut Option<(String, String)>| {
            let Some((p, t)) = b.clone() else { return };
            match map(&p) {
                None => {}
                Some(None) => *b = None,
                Some(Some(np)) => *b = Some((np, new.and_then(|(_, title)| title).map(str::to_string).unwrap_or(t))),
            }
        };
        for t in self.tabs.tabs.iter_mut().chain(self.others.values_mut().flat_map(|x| x.tabs.iter_mut())) {
            fix(&mut t.bookmark);
        }
        for c in self.closed.values_mut().flatten() {
            fix(&mut c.bookmark);
        }
        let keys: Vec<String> = self.history.keys().filter(|k| k.strip_prefix("bookmark:").is_some_and(|p| map(p).is_some())).cloned().collect();
        for k in keys {
            let v = self.history.remove(&k).unwrap();
            if let Some(Some(np)) = map(&k["bookmark:".len()..]) {
                self.history.insert(format!("bookmark:{np}"), v);
            }
        }
        self.bookmarks_rev += 1;
    }

    pub(super) fn bookmark_save(&mut self, tab: Option<TabRef>, title: &str, folder: &str) -> Reply {
        let i = self.tab_index(tab)?;
        let title = title.trim();
        if title.is_empty() {
            return Err(fail(ErrorCode::BadRequest, "No title"));
        }
        let layout = self.reopenable(&self.tabs.tabs[i].layout).ok_or_else(|| fail(ErrorCode::BadRequest, "Nothing in this tab can be reopened"))?;
        let dir = self.space_folder(folder, true)?;
        std::fs::create_dir_all(&dir).map_err(io)?;
        let path = unique(&dir, &file_name(title));
        write_bookmark(&path, &Bookmark::new(title, layout.to_node()), &self.home)?;
        self.tabs.tabs[i].bookmark = Some((s(&path), title.to_string()));
        self.bookmarks_rev += 1;
        Ok(json!(s(&path)))
    }

    pub(super) fn bookmark_overwrite(&mut self, tab: Option<TabRef>) -> Reply {
        let i = self.tab_index(tab)?;
        let (path, _) = self.tabs.tabs[i].bookmark.clone().ok_or_else(|| fail(ErrorCode::BadRequest, "This tab was not opened from a bookmark"))?;
        let layout = self.reopenable(&self.tabs.tabs[i].layout).ok_or_else(|| fail(ErrorCode::BadRequest, "Nothing in this tab can be reopened"))?;
        // 題名・paths・知らない項目は残し、配置だけを替える
        let mut bm = self.read_bookmark(&path)?;
        bm.layout = layout.to_node();
        write_bookmark(Path::new(&path), &bm, &self.home)?;
        self.bookmarks_rev += 1;
        Ok(json!(path))
    }

    pub(super) fn bookmark_rename(&mut self, path: &str, title: &str) -> Reply {
        let p = self.space_bookmark(path)?;
        let title = title.trim();
        if title.is_empty() {
            return Err(fail(ErrorCode::BadRequest, "No title"));
        }
        let new = p.parent().unwrap().join(file_name(title));
        if new != p && new.exists() {
            return Err(fail(ErrorCode::Conflict, "A bookmark with this name already exists"));
        }
        let mut bm = self.read_bookmark(path)?;
        bm.title = title.to_string();
        write_bookmark(&new, &bm, &self.home)?;
        if new != p {
            std::fs::remove_file(&p).map_err(io)?;
        }
        self.relink(path, Some((&s(&new), Some(title))), false);
        Ok(json!(s(&new)))
    }

    pub(super) fn bookmark_move(&mut self, path: &str, folder: &str) -> Reply {
        let p = self.space_bookmark(path)?;
        let dir = self.space_folder(folder, true)?;
        let new = dir.join(p.file_name().unwrap());
        if std::fs::canonicalize(&dir).ok() == p.parent().and_then(|d| std::fs::canonicalize(d).ok()) {
            return Ok(json!(path)); // 同じフォルダ
        }
        if new.exists() {
            return Err(fail(ErrorCode::Conflict, "A bookmark with this name already exists there"));
        }
        std::fs::rename(&p, &new).map_err(io)?;
        self.relink(path, Some((&s(&new), None)), false);
        Ok(json!(s(&new)))
    }

    pub(super) fn bookmark_trash(&mut self, path: &str) -> Reply {
        let p = self.space_bookmark(path)?;
        crate::trash::trash_in(&p, &self.trash_dir).map_err(io)?;
        self.relink(path, None, false);
        Ok(serde_json::Value::Null)
    }

    pub(super) fn folder_create(&mut self, name: &str) -> Reply {
        let name = valid_name(name)?;
        let d = self.space_dir().join(name);
        if d.exists() {
            return Err(fail(ErrorCode::Conflict, "A folder with this name already exists"));
        }
        std::fs::create_dir_all(&d).map_err(io)?;
        self.bookmarks_rev += 1;
        Ok(json!(s(&d)))
    }

    pub(super) fn folder_rename(&mut self, path: &str, name: &str) -> Reply {
        let d = self.space_folder(path, false)?;
        let name = valid_name(name)?;
        let new = self.space_dir().join(name);
        if new == d {
            return Ok(json!(path));
        }
        if new.exists() {
            return Err(fail(ErrorCode::Conflict, "A folder with this name already exists"));
        }
        std::fs::rename(&d, &new).map_err(io)?;
        self.relink(&s(&d), Some((&s(&new), None)), true);
        Ok(json!(s(&new)))
    }

    pub(super) fn folder_remove(&mut self, path: &str) -> Reply {
        let d = self.space_folder(path, false)?;
        let root = self.space_dir();
        let entries: Vec<PathBuf> = std::fs::read_dir(&d).map_err(io)?.flatten().map(|e| e.path()).collect();
        for e in entries {
            let name = e.file_name().unwrap().to_string_lossy().into_owned();
            let to = unique(&root, &name);
            std::fs::rename(&e, &to).map_err(io)?;
            if e.extension().and_then(|x| x.to_str()) == Some(EXT) {
                // 名前が変わったら、題名も合わせる(ファイル名が題名)
                let title = (to.file_name() != e.file_name()).then(|| stem(&s(&to)));
                self.relink(&s(&e), Some((&s(&to), title.as_deref())), false);
            }
        }
        std::fs::remove_dir(&d).map_err(io)?;
        self.bookmarks_rev += 1;
        Ok(serde_json::Value::Null)
    }
}

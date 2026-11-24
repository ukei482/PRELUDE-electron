//! PRELUDE を通さずに開いたブラウザの窓の URL(docs/redesign-rust.md の「Web のペイン」の「ブラウザの窓の URL」)。
//!
//! - Web アプリの窓(Chromium 系の `--app=<URL>`。Omarchy の Web アプリ)は、窓の名前と .desktop の起動コマンドから URL が分かる([`app_window_url`])
//! - 普通のブラウザの窓は、ブックマークに保存するときだけ、ブラウザの履歴のファイルを窓の題名で引く([`from_history`])。
//!   ブラウザの中とは話さない。履歴は使っている間も書かれるので、写しを読む
use crate::apps::App;
use prelude_format::browser::{self, History};
use std::path::{Path, PathBuf};

/// Web アプリの窓の URL と、それを開いた .desktop(あれば)。Web アプリの窓でなければ None
pub fn app_window_url<'a>(class: &str, apps: &'a [App]) -> Option<(String, Option<&'a App>)> {
    let name = browser::app_window_name(class)?;
    let found = apps.iter().find_map(|a| {
        let url = browser::url_in_command(&a.exec)?;
        browser::app_url_matches(url, name).then(|| (url.to_string(), Some(a)))
    });
    found.or_else(|| browser::app_url_guess(name).map(|u| (u, None)))
}

/// 窓の名前 class のブラウザの履歴から、題名 title のページの URL。いちばん最近に開いたもの
pub fn from_history(class: &str, title: &str) -> Option<String> {
    let b = browser::by_class(class)?;
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home.join(".config"));
    let files = b.history_files(&home, &config);
    if files.is_empty() {
        return None;
    }
    let titles = browser::page_titles(title);
    if titles.is_empty() {
        return None;
    }
    let tmp = Tmp::new()?;
    let dbs: Vec<rusqlite::Connection> = files.iter().enumerate().filter_map(|(i, f)| tmp.open_copy(f, i)).collect();
    lookup(&dbs, b.history, &titles)
}

/// 題名の候補を長い順に引き、最初に見つかった候補のうち、いちばん最近のもの
fn lookup(dbs: &[rusqlite::Connection], kind: History, titles: &[String]) -> Option<String> {
    let sql = match kind {
        History::Chromium(_) => {
            "SELECT url, last_visit_time FROM urls WHERE title = ?1 AND hidden = 0 ORDER BY last_visit_time DESC LIMIT 5"
        }
        History::Firefox(_) => {
            "SELECT url, COALESCE(last_visit_date, 0) FROM moz_places WHERE title = ?1 ORDER BY last_visit_date DESC LIMIT 5"
        }
    };
    titles.iter().find_map(|t| {
        dbs.iter()
            .filter_map(|db| {
                let mut st = db.prepare(sql).ok()?;
                let rows: Vec<(String, i64)> = st.query_map([t], |r| Ok((r.get(0)?, r.get(1)?))).ok()?.flatten().collect();
                // 開き直せる http(s) だけ
                rows.into_iter().find(|(u, _)| u.starts_with("https://") || u.starts_with("http://")).map(|(u, at)| (at, u))
            })
            .max()
            .map(|(_, u)| u)
    })
}

/// 履歴の写しを置く、自分だけが読めるフォルダ。終わったら消す
struct Tmp(PathBuf);

impl Tmp {
    fn new() -> Option<Tmp> {
        use std::os::unix::fs::DirBuilderExt;
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let base = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|p| p.is_dir()).unwrap_or_else(std::env::temp_dir);
        let dir = base.join(format!("prelude-history-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::DirBuilder::new().mode(0o700).create(&dir).ok()?;
        Some(Tmp(dir))
    }

    /// file(と、あれば書きかけの -wal)を写して開く。ブラウザが使っている間も読めるように、元のファイルは開かない
    fn open_copy(&self, file: &Path, i: usize) -> Option<rusqlite::Connection> {
        let to = self.0.join(format!("{i}.sqlite"));
        std::fs::copy(file, &to).ok()?;
        let mut wal = file.as_os_str().to_owned();
        wal.push("-wal");
        let _ = std::fs::copy(PathBuf::from(wal), self.0.join(format!("{i}.sqlite-wal")));
        rusqlite::Connection::open_with_flags(&to, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX)
            .ok()
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, exec: &str) -> App {
        App { name: id.into(), exec: exec.into(), cls: String::new(), id: id.into(), icon: String::new() }
    }

    #[test]
    fn web_アプリの窓は_desktop_の_url() {
        let apps = [app("HEY", "omarchy-launch-webapp https://app.hey.com"), app("chromium", "chromium")];
        let (url, a) = app_window_url("chrome-app.hey.com__-Default", &apps).unwrap();
        assert_eq!((url.as_str(), a.map(|a| a.id.as_str())), ("https://app.hey.com", Some("HEY")));
        // .desktop が無ければ窓の名前から
        assert_eq!(app_window_url("chrome-x.com__home-Default", &apps), Some(("https://x.com/home".into(), None)));
        assert_eq!(app_window_url("chromium", &apps), None);
    }

    #[test]
    fn 履歴を題名で引く() {
        let tmp = Tmp::new().unwrap();
        let chromium = rusqlite::Connection::open(tmp.0.join("c.sqlite")).unwrap();
        chromium
            .execute_batch(
                "CREATE TABLE urls (url TEXT, title TEXT, last_visit_time INTEGER, hidden INTEGER DEFAULT 0);
                 INSERT INTO urls (url, title, last_visit_time) VALUES
                   ('https://github.com/', 'GitHub', 10), ('https://github.com/login', 'GitHub', 20),
                   ('chrome://newtab/', '新しいタブ', 30), ('https://a.jp/', 'A - B', 5);",
            )
            .unwrap();
        let other = rusqlite::Connection::open(tmp.0.join("d.sqlite")).unwrap();
        other
            .execute_batch(
                "CREATE TABLE urls (url TEXT, title TEXT, last_visit_time INTEGER, hidden INTEGER DEFAULT 0);
             INSERT INTO urls (url, title, last_visit_time) VALUES ('https://github.com/new', 'GitHub', 15);",
            )
            .unwrap();
        let dbs = [chromium, other];
        let k = History::Chromium("chromium");
        let get = |t: &str| lookup(&dbs, k, &browser::page_titles(t));
        assert_eq!(get("GitHub - Chromium").as_deref(), Some("https://github.com/login"));
        // ページの題名に「 - 」があっても、長い候補が先
        assert_eq!(get("A - B - Chromium").as_deref(), Some("https://a.jp/"));
        assert_eq!(get("新しいタブ - Chromium"), None);
        assert_eq!(get("知らない - Chromium"), None);

        let ff = rusqlite::Connection::open(tmp.0.join("f.sqlite")).unwrap();
        ff.execute_batch(
            "CREATE TABLE moz_places (url TEXT, title TEXT, last_visit_date INTEGER);
             INSERT INTO moz_places VALUES ('https://mozilla.org/', 'Mozilla', NULL);",
        )
        .unwrap();
        assert_eq!(
            lookup(&[ff], History::Firefox(&[]), &browser::page_titles("Mozilla — Mozilla Firefox")).as_deref(),
            Some("https://mozilla.org/")
        );
    }

    #[test]
    fn 履歴のファイルを写して読む() {
        let tmp = Tmp::new().unwrap();
        let home = tmp.0.join("home");
        let prof = home.join(".config/chromium/Profile 1");
        std::fs::create_dir_all(&prof).unwrap();
        let db = rusqlite::Connection::open(prof.join("History")).unwrap();
        db.execute_batch(
            "CREATE TABLE urls (url TEXT, title TEXT, last_visit_time INTEGER, hidden INTEGER DEFAULT 0);
             INSERT INTO urls (url, title, last_visit_time) VALUES ('https://example.com/', 'Example', 1);",
        )
        .unwrap();
        let b = browser::by_class("chromium").unwrap();
        let files = b.history_files(&home, &home.join(".config"));
        assert_eq!(files, vec![prof.join("History")]);
        let t2 = Tmp::new().unwrap();
        let copy = t2.open_copy(&files[0], 0).unwrap();
        assert_eq!(lookup(&[copy], b.history, &browser::page_titles("Example - Chromium")).as_deref(), Some("https://example.com/"));
        // 元のファイルは開いたまま(ブラウザが使っている間)でも読める
        drop(db);
    }
}

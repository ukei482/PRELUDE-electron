//! xdg-desktop-portal の FileChooser のバックエンド(docs/redesign-rust.md の「アプリのダイアログ」、移行の段階 7)。
//!
//! - アプリの「開く」「保存」のダイアログを受ける。コアが決めるのは最初のフォルダ(今のタブの paths の `open` / `save`)だけ
//! - 選ぶ画面は別のプログラム。`~/.config/prelude/chooser.toml` の `run` で替えられる(既定は `prelude choose …`。端末の中の yazi)。
//!   置き換えられるのは `{mode}`(`open`・`open-multiple`・`directory`・`save`)、`{path}`(最初のフォルダ)、`{name}`(保存の名前)、
//!   `{title}`、`{output}`(選んだパスを 1 行ずつ書くファイル。yazi の `--chooser-file` と同じ形)。何も書かずに終わると、取り消し
//! - 「名前を付けて保存」で今のファイル(`current_file`)を渡されたときは、そのフォルダを優先する
//! - D-Bus の名前は `org.freedesktop.impl.portal.desktop.prelude`(別のソケットのコアは印つき)。切り替えは `scripts/portal.sh`
use crate::recipes::{self, Recipe};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;
use zbus::object_server::ObjectServer;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

const SERVICE: &str = "org.freedesktop.impl.portal.desktop.prelude";
const OBJECT: &str = "/org/freedesktop/portal/desktop";
/// ポータルの返事の番号
const OK: u32 = 0;
const CANCELLED: u32 = 1;
const OTHER: u32 = 2;

/// 今のタブの paths の open(save が true なら save)を返す。ブックマークにもスペースにも書いていなければ None
pub type FolderOf = Arc<dyn Fn(bool) -> Option<String> + Send + Sync>;

/// 最初のフォルダ。保存し直すファイルのフォルダ → 今のブックマークの paths → アプリが頼んだフォルダ → ホーム の順。
/// アプリが前回のフォルダを渡してきても、ブックマークの保存先が効くように、paths をアプリのフォルダより先にする
pub fn first_folder(a: &Ask, paths: Option<String>, home: &str) -> String {
    a.file_folder.clone().or(paths).or_else(|| a.app_folder.clone()).unwrap_or_else(|| home.to_string())
}

/// 既定の選ぶ画面
fn builtin() -> Recipe {
    Recipe {
        kind: "chooser".into(),
        name: "Chooser".into(),
        run: "prelude choose {mode} --output {output} --name {name} {path}".into(),
        class: String::new(),
    }
}

/// 選ぶ画面のレシピ。`file` が読めなければ既定
pub fn load(file: &Path) -> Recipe {
    let Ok(text) = std::fs::read_to_string(file) else { return builtin() };
    match toml::from_str::<Recipe>(&text) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("prelude-core: cannot read {} ({e})", file.display());
            builtin()
        }
    }
}

/// アプリの頼み
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Ask {
    pub save: bool,
    pub multiple: bool,
    pub directory: bool,
    pub name: String,
    /// 今のファイル(保存し直すとき。`current_file`)のフォルダ
    pub file_folder: Option<String>,
    /// アプリが頼んだ最初のフォルダ(`current_folder`。前回使ったフォルダのことが多い)
    pub app_folder: Option<String>,
    /// ファイルの種類。`名前:*.png;*.jpg|名前:*.pdf` の形(選ぶ画面の `{filter}`)。MIME の型だけの種類は入れない
    pub filter: String,
    /// `SaveFiles`: 保存する名前(選ぶのはフォルダ)
    pub files: Vec<String>,
}

fn bytes_of(v: &Value) -> Option<String> {
    let Value::Array(a) = v else { return None };
    let b: Vec<u8> = a.iter().filter_map(|x| if let Value::U8(b) = x { Some(*b) } else { None }).collect();
    Some(String::from_utf8_lossy(&b).trim_end_matches('\0').to_string())
}

/// `filters`(`a(sa(us))`)を `{filter}` の文字にする。種類 0 が glob、1 が MIME の型
fn filter_text(v: &Value) -> String {
    let Value::Array(list) = v else { return String::new() };
    let mut out = vec![];
    for f in list.iter() {
        let Value::Structure(s) = f else { continue };
        let [Value::Str(name), Value::Array(rules)] = s.fields() else { continue };
        let globs: Vec<String> = rules
            .iter()
            .filter_map(|r| match r {
                Value::Structure(r) => match r.fields() {
                    [Value::U32(0), Value::Str(g)] => Some(g.to_string()),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        if !globs.is_empty() {
            out.push(format!("{}:{}", name.replace(['|', ':'], " "), globs.join(";")));
        }
    }
    out.join("|")
}

fn bytes_path(v: &OwnedValue) -> Option<String> {
    // ポータルのパスは ay(末尾に 0 の付いたバイト列)
    let Value::Array(a) = &**v else { return None };
    let b: Vec<u8> = a.iter().filter_map(|x| if let Value::U8(b) = x { Some(*b) } else { None }).collect();
    let s = String::from_utf8_lossy(&b).trim_end_matches('\0').to_string();
    // 途中に NUL のあるものはパスではない。無かったことにして、最初のフォルダを paths で決める
    (s.starts_with('/') && !s.contains('\0')).then_some(s)
}

impl Ask {
    pub fn parse(save: bool, o: &HashMap<String, OwnedValue>) -> Ask {
        let flag = |k: &str| matches!(o.get(k).map(|v| &**v), Some(Value::Bool(true)));
        let mut a = Ask { save, multiple: !save && flag("multiple"), directory: !save && flag("directory"), ..Ask::default() };
        if let Some(Value::Str(s)) = o.get("current_name").map(|v| &**v) {
            a.name = s.to_string();
        }
        if let Some(f) = o.get("current_file").and_then(bytes_path) {
            let p = Path::new(&f);
            if a.name.is_empty() {
                a.name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            }
            a.file_folder = p.parent().map(|d| d.to_string_lossy().into_owned());
        }
        a.app_folder = o.get("current_folder").and_then(bytes_path);
        if let Some(f) = o.get("filters") {
            a.filter = filter_text(f);
        }
        a
    }

    /// `SaveFiles` の頼み: 選ぶのはフォルダで、名前はアプリが決めている
    pub fn parse_files(o: &HashMap<String, OwnedValue>) -> Ask {
        let mut a = Ask::parse(false, o);
        a.directory = true;
        a.multiple = false;
        if let Some(Value::Array(l)) = o.get("files").map(|v| &**v) {
            a.files = l.iter().filter_map(bytes_of).filter(|n| !n.is_empty()).collect();
        }
        a
    }

    /// 選ばれたパスから、アプリへ返すパス
    fn results(&self, picked: Vec<String>) -> Vec<String> {
        match picked.first() {
            Some(dir) if !self.files.is_empty() => self.files.iter().map(|n| format!("{}/{n}", dir.trim_end_matches('/'))).collect(),
            _ => picked,
        }
    }

    fn mode(&self) -> &'static str {
        match (self.save, self.directory, self.multiple) {
            (true, ..) => "save",
            (_, true, _) => "directory",
            (_, _, true) => "open-multiple",
            _ => "open",
        }
    }
}

/// レシピの run を置き換えたコマンド
pub fn command(r: &Recipe, a: &Ask, folder: &str, title: &str, output: &Path) -> String {
    let vars = |n: &str| match n {
        "mode" => Some(a.mode().to_string()),
        "path" => Some(folder.to_string()),
        "name" => Some(a.name.clone()),
        "title" => Some(title.to_string()),
        "output" => Some(output.to_string_lossy().into_owned()),
        "filter" => Some(a.filter.clone()),
        _ => None,
    };
    recipes::expand(&r.run, &vars)
}

/// パスを file:// の URL に
pub fn to_uri(path: &str) -> String {
    let mut s = String::from("file://");
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

/// 選ぶ画面が書いたファイルを読む。パスの行だけ
pub fn read_output(text: &str) -> Vec<String> {
    text.lines().map(str::trim).filter(|l| l.starts_with('/')).map(str::to_string).collect()
}

/// 選ぶ画面の環境変数の印。コンポジタの裏側が、親のある窓として浮かせるのに使う
pub const DIALOG_ENV: &str = "PRELUDE_DIALOG=1";

/// 選ぶ画面を動かして、選んだパスを返す。取り消しなら空。cancel が来たら止める
pub async fn choose(cmd: &str, output: &Path, cancel: oneshot::Receiver<()>) -> std::io::Result<Vec<String>> {
    let _ = std::fs::remove_file(output);
    let mut c = tokio::process::Command::new("/bin/sh");
    c.arg("-c")
        .arg(cmd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .env("PRELUDE_DIALOG", "1")
        .kill_on_drop(true);
    c.process_group(0);
    let mut child = c.spawn()?;
    tokio::select! {
        r = child.wait() => { r?; }
        _ = cancel => {
            // sh の子(選ぶ画面)まで止める。起動のときに自分のプロセスグループにしてある
            if let Some(pid) = child.id() {
                crate::procs::signal_group(pid, libc::SIGTERM);
            }
            let _ = child.start_kill();
            let _ = std::fs::remove_file(output);
            return Ok(vec![]);
        }
    }
    let got = std::fs::read_to_string(output).map(|t| read_output(&t)).unwrap_or_default();
    let _ = std::fs::remove_file(output);
    Ok(got)
}

/// ポータルが頼みごとの取り消しに使うもの
struct Request {
    cancel: Mutex<Option<oneshot::Sender<()>>>,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Request")]
impl Request {
    fn close(&self) {
        if let Some(t) = self.cancel.lock().unwrap().take() {
            let _ = t.send(());
        }
    }
}

struct FileChooser {
    recipe: Recipe,
    folder_of: FolderOf,
    runtime: PathBuf,
    next: std::sync::atomic::AtomicU64,
}

impl FileChooser {
    async fn ask(&self, server: &ObjectServer, handle: ObjectPath<'_>, title: &str, a: Ask) -> (u32, HashMap<String, OwnedValue>) {
        let folder = first_folder(&a, (self.folder_of)(a.save), &std::env::var("HOME").unwrap_or_else(|_| "/".into()));
        let n = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let output = self.runtime.join(format!("chooser-{}-{n}", std::process::id()));
        let cmd = command(&self.recipe, &a, &folder, title, &output);
        let (tx, rx) = oneshot::channel();
        let registered = server.at(&handle, Request { cancel: Mutex::new(Some(tx)) }).await.unwrap_or(false);
        let r = choose(&cmd, &output, rx).await;
        if registered {
            let _ = server.remove::<Request, _>(&handle).await;
        }
        let paths = match r {
            Ok(p) => p,
            Err(e) => {
                eprintln!("prelude-core: cannot run the file chooser ({e})");
                return (OTHER, HashMap::new());
            }
        };
        if paths.is_empty() {
            return (CANCELLED, HashMap::new());
        }
        let uris: Vec<String> = a.results(paths).iter().map(|p| to_uri(p)).collect();
        let mut res = HashMap::new();
        match OwnedValue::try_from(Value::from(uris)) {
            Ok(v) => {
                res.insert("uris".to_string(), v);
                (OK, res)
            }
            Err(_) => (OTHER, HashMap::new()),
        }
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.FileChooser")]
impl FileChooser {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        4
    }

    async fn open_file(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        handle: ObjectPath<'_>,
        _app_id: &str,
        _parent_window: &str,
        title: &str,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        self.ask(server, handle, title, Ask::parse(false, &options)).await
    }

    async fn save_file(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        handle: ObjectPath<'_>,
        _app_id: &str,
        _parent_window: &str,
        title: &str,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        self.ask(server, handle, title, Ask::parse(true, &options)).await
    }

    async fn save_files(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        handle: ObjectPath<'_>,
        _app_id: &str,
        _parent_window: &str,
        title: &str,
        options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        self.ask(server, handle, title, Ask::parse_files(&options)).await
    }
}

/// ポータルのバックエンドを始める。D-Bus に繋がらない・名前が取れなければ None(動かない理由は出す)
pub async fn start(tag: &str, recipe: Recipe, folder_of: FolderOf) -> Option<zbus::Connection> {
    let runtime = std::env::var("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir()).join("prelude");
    let _ = std::fs::create_dir_all(&runtime);
    let service = format!("{SERVICE}{tag}");
    let iface = FileChooser { recipe, folder_of, runtime, next: Default::default() };
    let conn = async { zbus::connection::Builder::session()?.name(service.as_str())?.serve_at(OBJECT, iface)?.build().await }.await;
    match conn {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("prelude-core: file chooser portal unavailable ({e})");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn val(v: Value<'_>) -> OwnedValue {
        v.try_into_owned().unwrap()
    }

    #[test]
    fn 頼みを読む() {
        let mut o = HashMap::new();
        o.insert("multiple".to_string(), val(Value::from(true)));
        let a = Ask::parse(false, &o);
        assert_eq!((a.mode(), a.multiple), ("open-multiple", true));
        o.insert("directory".to_string(), val(Value::from(true)));
        assert_eq!(Ask::parse(false, &o).mode(), "directory");
        assert_eq!(Ask::parse(true, &o).mode(), "save"); // 保存に multiple・directory は無い
                                                         // 保存: 名前と、今のファイルのフォルダ
        let mut o = HashMap::new();
        o.insert("current_name".to_string(), val(Value::from("新規.txt")));
        assert_eq!(Ask::parse(true, &o).name, "新規.txt");
        assert_eq!(Ask::parse(true, &o).file_folder, None);
        o.clear();
        o.insert("current_file".to_string(), val(Value::from(b"/home/u/doc/a b.txt\0".to_vec())));
        let a = Ask::parse(true, &o);
        assert_eq!((a.name.as_str(), a.file_folder.as_deref()), ("a b.txt", Some("/home/u/doc")));
        // パスでないものは無視する
        o.insert("current_file".to_string(), val(Value::from(vec![240u8, 194, 0, 0, 32, 103, 0])));
        assert_eq!(Ask::parse(true, &o), Ask { save: true, ..Ask::default() });
    }

    #[test]
    fn ファイルの種類とフォルダ() {
        let val = |v: Value| OwnedValue::try_from(v).unwrap();
        let filters: Vec<(&str, Vec<(u32, &str)>)> = vec![
            ("画像", vec![(0, "*.png"), (0, "*.JPG"), (1, "image/gif")]),
            ("型だけ", vec![(1, "text/plain")]),
            ("a:b|c", vec![(0, "*")]),
        ];
        let mut o = HashMap::new();
        o.insert("filters".to_string(), val(Value::from(filters)));
        o.insert("current_folder".to_string(), val(Value::from(b"/w/x\0".to_vec())));
        let a = Ask::parse(false, &o);
        assert_eq!(a.filter, "画像:*.png;*.JPG|a b c:*");
        assert_eq!(a.app_folder.as_deref(), Some("/w/x"));
    }

    #[test]
    fn 最初のフォルダの順() {
        let ask = |file: Option<&str>, app: Option<&str>| Ask {
            file_folder: file.map(str::to_string),
            app_folder: app.map(str::to_string),
            ..Ask::default()
        };
        let paths = || Some("/研究/原稿".to_string());
        // 保存し直すファイルのフォルダが一番先
        assert_eq!(first_folder(&ask(Some("/doc"), Some("/前回")), paths(), "/h"), "/doc");
        // アプリが前回のフォルダを渡しても、ブックマークの paths が先
        assert_eq!(first_folder(&ask(None, Some("/前回")), paths(), "/h"), "/研究/原稿");
        // paths が無ければアプリのフォルダ、それも無ければホーム
        assert_eq!(first_folder(&ask(None, Some("/前回")), None, "/h"), "/前回");
        assert_eq!(first_folder(&ask(None, None), None, "/h"), "/h");
    }

    #[test]
    fn 複数のファイルを保存する() {
        let val = |v: Value| OwnedValue::try_from(v).unwrap();
        let mut o = HashMap::new();
        o.insert("files".to_string(), val(Value::from(vec![b"a.png\0".to_vec(), b"b c.png\0".to_vec()])));
        let a = Ask::parse_files(&o);
        assert_eq!((a.directory, a.mode()), (true, "directory"));
        assert_eq!(a.results(vec!["/w/d".into()]), ["/w/d/a.png", "/w/d/b c.png"]);
        assert_eq!(a.results(vec![]), Vec::<String>::new());
        assert_eq!(Ask::default().results(vec!["/x".into()]), ["/x"]);
    }

    #[test]
    fn コマンドと結果() {
        let a = Ask { save: true, name: "it's.txt".into(), ..Ask::default() };
        let c = command(&builtin(), &a, "/w/保存", "Save", Path::new("/run/o"));
        assert_eq!(c, r"prelude choose 'save' --output '/run/o' --name 'it'\''s.txt' '/w/保存'");
        assert_eq!(to_uri("/a b/議.txt"), "file:///a%20b/%E8%AD%B0.txt");
        assert_eq!(read_output("/a\n\nnot a path\n/b c\n"), ["/a", "/b c"]);
    }

    #[test]
    fn レシピを読む() {
        let d = std::env::temp_dir().join(format!("prelude-portal-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("chooser.toml");
        assert_eq!(load(&f), builtin()); // 無ければ既定
        std::fs::write(&f, "run = \"konsole -e yazi --chooser-file {output} {path}\"\n").unwrap();
        assert!(load(&f).run.contains("yazi"));
        std::fs::write(&f, "run = = 1").unwrap();
        assert_eq!(load(&f), builtin()); // 読めなければ既定
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn 選ぶ画面を動かす() {
        let d = std::env::temp_dir().join(format!("prelude-portal-run-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let out = d.join("o");
        let q = recipes::quote(&out.to_string_lossy());
        // 選んだ
        let (_t, rx) = oneshot::channel();
        assert_eq!(choose(&format!("printf '/a\\n/b\\n' > {q}"), &out, rx).await.unwrap(), ["/a", "/b"]);
        assert!(!out.exists());
        // 何も書かずに終わると取り消し
        let (_t, rx) = oneshot::channel();
        assert!(choose("true", &out, rx).await.unwrap().is_empty());
        // ポータルが閉じたら止める
        let (t, rx) = oneshot::channel();
        // sh の子まで止まる(残ると、選ぶ画面の窓が残る)。子の pid をファイルに書かせて確かめる
        let pidfile = d.join("pid");
        let script = format!("sh -c 'echo $$ > {}; exec sleep 30'; true", pidfile.display());
        let waiter = tokio::spawn({
            let out = out.clone();
            async move { choose(&script, &out, rx).await.unwrap() }
        });
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let pid: u32 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
        assert!(crate::procs::alive(pid));
        t.send(()).unwrap();
        assert!(waiter.await.unwrap().is_empty());
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(!crate::procs::alive(pid));
        let _ = std::fs::remove_dir_all(&d);
    }
}

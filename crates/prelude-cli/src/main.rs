//! prelude コマンド。コアのソケット(prelude-proto)につないで、状態を見る・操作する。
//! 画面の文は Electron 版に合わせて英語。外のスクリプトからは `prelude raw` と `prelude watch` で JSON をそのまま使える。
use prelude_proto::{BookmarkInfo, Incoming, State, PROTOCOL};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

const HELP: &str = "\
usage: prelude <command> [args]

  status                 Current space and tabs (default)
  tabs                   List tabs (* = current)
  tab <n>                Switch to tab n
  close [n]              Close tab n (default: current)
  open <target>          Open a URL, a folder, a .prelude file, or a bookmark of this space by name
  spaces                 List spaces (* = current)
  space <name>           Switch to a space
  space new <name>       Create a space
  bookmarks              List bookmarks of this space
  paths [n]              Folders used by tab n (files, open, save, download, terminal)
  watch                  Print state events as JSON lines
  raw <cmd> [args-json]  Send a command and print the result as JSON

The core socket is $PRELUDE_CORE_SOCK or $XDG_RUNTIME_DIR/prelude/core.sock.";

struct Conn {
    w: UnixStream,
    r: BufReader<UnixStream>,
    next: u64,
}

impl Conn {
    fn open(subscribe: bool) -> Result<Conn, String> {
        let path = prelude_proto::socket_path();
        let w = UnixStream::connect(&path).map_err(|e| format!("Cannot connect to prelude-core at {} ({e})", path.display()))?;
        let r = BufReader::new(w.try_clone().map_err(|e| e.to_string())?);
        let mut c = Conn { w, r, next: 0 };
        c.call("hello", json!({ "protocol": PROTOCOL, "name": "prelude", "subscribe": subscribe }))?;
        Ok(c)
    }

    fn read(&mut self) -> Result<Incoming, String> {
        let mut l = String::new();
        if self.r.read_line(&mut l).map_err(|e| e.to_string())? == 0 {
            return Err("prelude-core closed the connection".into());
        }
        serde_json::from_str(&l).map_err(|e| format!("Bad answer from prelude-core ({e})"))
    }

    /// コマンドを送り、返事の result を返す(途中に来たイベントは捨てる)
    fn call(&mut self, cmd: &str, args: Value) -> Result<Value, String> {
        self.next += 1;
        let id = self.next;
        let req = json!({ "id": id, "cmd": cmd, "args": args });
        writeln!(self.w, "{req}").map_err(|e| e.to_string())?;
        loop {
            match self.read()? {
                Incoming::Response(r) if r.id == id || r.id == 0 => {
                    return match r.error {
                        Some(e) => Err(e.message),
                        None => Ok(r.result.unwrap_or(Value::Null)),
                    }
                }
                _ => continue,
            }
        }
    }

    fn state(&mut self) -> Result<State, String> {
        serde_json::from_value(self.call("state.get", json!({}))?).map_err(|e| e.to_string())
    }
}

fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

/// ホームの下を ~ にして見せる
fn short(p: &str) -> String {
    let h = home();
    match p.strip_prefix(&h) {
        Some(r) if !h.is_empty() && (r.is_empty() || r.starts_with('/')) => format!("~{r}"),
        _ => p.to_string(),
    }
}

fn num_arg(s: &str) -> Result<Value, String> {
    let n: u32 = s.trim_start_matches('b').parse().map_err(|_| format!("Not a tab number: {s}"))?;
    Ok(json!({ "num": n }))
}

fn print_tabs(s: &State) {
    if s.tabs.is_empty() {
        println!("  (no tabs)");
    }
    for t in &s.tabs {
        let mark = if Some(t.id) == s.active { '*' } else { ' ' };
        let bm = t.bookmark.as_deref().map(|p| format!("  {}", short(p))).unwrap_or_default();
        println!("{mark} {:>2}  {}{bm}", t.num, t.title);
    }
}

fn abs(p: &str) -> PathBuf {
    let p = Path::new(p);
    std::fs::canonicalize(p).unwrap_or_else(|_| std::env::current_dir().unwrap_or_default().join(p))
}

/// open の対象を決める: .prelude のファイル → ブックマーク、フォルダ → フォルダのペイン、URL → Web、
/// それ以外は今のスペースのブックマークの名前(同じ名前、なければ名前の始まり)
fn open(c: &mut Conn, target: &str) -> Result<(), String> {
    let p = Path::new(target);
    if target.ends_with(".prelude") && p.is_file() {
        c.call("bookmark.open", json!({ "path": abs(target) }))?;
        return Ok(());
    }
    if p.is_dir() {
        c.call("tab.open", json!({ "pane": { "type": "files", "path": abs(target) } }))?;
        return Ok(());
    }
    let url_like = target.contains("://") || (!target.contains(' ') && !target.starts_with('/') && target.contains('.') && !p.exists());
    let list: Vec<BookmarkInfo> = serde_json::from_value(c.call("bookmark.list", json!({}))?).map_err(|e| e.to_string())?;
    let lower = target.to_lowercase();
    let hit =
        list.iter().find(|b| b.title.to_lowercase() == lower).or_else(|| list.iter().find(|b| b.title.to_lowercase().starts_with(&lower)));
    match hit {
        Some(b) if !url_like || b.title.to_lowercase() == lower => {
            c.call("bookmark.open", json!({ "path": b.path }))?;
        }
        _ if url_like => {
            let url = if target.contains("://") { target.to_string() } else { format!("https://{target}") };
            c.call("tab.open", json!({ "pane": { "type": "web", "url": url } }))?;
        }
        _ => return Err(format!("Nothing to open: {target}")),
    }
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        [] | ["status"] => {
            let s = Conn::open(false)?.state()?;
            println!("Space: {}  ({})", s.space, short(&s.space_dir));
            print_tabs(&s);
        }
        ["help" | "-h" | "--help"] => println!("{HELP}"),
        ["tabs"] => print_tabs(&Conn::open(false)?.state()?),
        ["tab", n] => {
            Conn::open(false)?.call("tab.activate", json!({ "tab": num_arg(n)? }))?;
        }
        ["close"] => {
            Conn::open(false)?.call("tab.close", json!({}))?;
        }
        ["close", n] => {
            Conn::open(false)?.call("tab.close", json!({ "tab": num_arg(n)? }))?;
        }
        ["open", rest @ ..] if !rest.is_empty() => open(&mut Conn::open(false)?, &rest.join(" "))?,
        ["spaces"] => {
            let mut c = Conn::open(false)?;
            let cur = c.state()?.space;
            let names: Vec<String> = serde_json::from_value(c.call("space.list", json!({}))?).map_err(|e| e.to_string())?;
            for n in names {
                println!("{} {n}", if n == cur { '*' } else { ' ' });
            }
        }
        ["space", "new", name @ ..] if !name.is_empty() => {
            let r = Conn::open(false)?.call("space.create", json!({ "name": name.join(" ") }))?;
            println!("Created space: {}", r.as_str().unwrap_or_default());
        }
        ["space", name @ ..] if !name.is_empty() => {
            Conn::open(false)?.call("space.switch", json!({ "name": name.join(" ") }))?;
        }
        ["bookmarks"] => {
            let list: Vec<BookmarkInfo> =
                serde_json::from_value(Conn::open(false)?.call("bookmark.list", json!({}))?).map_err(|e| e.to_string())?;
            for b in list {
                let name = if b.folder.is_empty() { b.title } else { format!("{}/{}", b.folder, b.title) };
                println!("  {name}");
            }
        }
        ["paths"] | ["paths", _] => {
            let args = match a.get(1) {
                Some(n) => json!({ "tab": num_arg(n)? }),
                None => json!({}),
            };
            let r = Conn::open(false)?.call("paths.get", args)?;
            for (k, v) in r.as_object().into_iter().flatten() {
                println!("{k:<9} {}", short(v.as_str().unwrap_or_default()));
            }
        }
        ["watch"] => {
            let mut c = Conn::open(true)?;
            loop {
                if let Incoming::Event(e) = c.read()? {
                    println!("{}", serde_json::to_string(&e).unwrap_or_default());
                }
            }
        }
        ["raw", cmd, rest @ ..] => {
            let args: Value = match rest {
                [] => json!({}),
                _ => serde_json::from_str(&rest.join(" ")).map_err(|e| format!("Args are not JSON ({e})"))?,
            };
            let r = Conn::open(false)?.call(cmd, args)?;
            println!("{}", serde_json::to_string_pretty(&r).unwrap_or_default());
        }
        _ => return Err(format!("Unknown command. See: prelude help\n\n{HELP}")),
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("prelude: {e}");
        std::process::exit(1);
    }
}

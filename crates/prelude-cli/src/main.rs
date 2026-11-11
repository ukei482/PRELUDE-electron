//! prelude コマンド。コアのソケット(prelude-proto)につないで、状態を見る・操作する。
//! 画面の文は Electron 版に合わせて英語。外のスクリプトからは `prelude raw` と `prelude watch` で JSON をそのまま使える。
use prelude_proto::{BookmarkInfo, BookmarkOpened, Client, FolderInfo, Incoming, Item, RecipeInfo, State};
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;

mod choose;
mod mcp;
mod skill;
mod target;

use target::{abs, bookmark_path, folder_path, Target};

const HELP: &str = "\
usage: prelude <command> [args]

  status                 Current space and tabs (default)
  tabs                   List tabs (* = current)
  tab <n>                Switch to tab n
  close [n]              Close tab n (default: current)
  reopen                 Reopen the last closed tab of this space
  tab dup [n]            Duplicate tab n (default: current) below it
  tab renumber <n> <new> Give tab n the number <new> (swaps with the tab that has it)
  pane close [id]        Close a pane (default: the current pane of the current tab)
  pane path [--pane <id>] <folder>
                         Tell the core where a pane is now (e.g. the folder a file manager moved to), so the tab reopens there.
                         The pane defaults to $PRELUDE_PANE (set for programs PRELUDE starts in a pane), then the current pane
  open [--run] <target>  Open a URL, a folder, a .prelude file, or a bookmark of this space by name or number (b3)
                         (--run: also run commands that do not match an installed app)
  open [--run]           Read one line from stdin: a line of `prelude items`, or a target as above
  items [--json]         Launcher items: tabs, then bookmarks, apps and spaces by score
                         e.g.  prelude items | fuzzel --dmenu | prelude open
  new <type> [key=value...]  Open a pane in a new tab (e.g. new console, new app cmd=kate)
  panes                  List panes of all tabs (id, type, place)
  place <pane> <x> <y> <w> <h>  Tell where a pane is on screen (what the UI does; for testing)
  recipes                Pane types made from recipes (~/.config/prelude/panes)
  url <pane> <url>       Open a URL in a web pane
  go <pane> back|forward|reload  Move in a web pane's history
  mute <pane> [on|off]   Mute (default) or unmute a web pane
  spaces                 List spaces (* = current)
  space <name>           Switch to a space
  space new <name>       Create a space
  bookmarks              List bookmarks of this space
  bookmark save <title> [<folder>]   Save the current tab as a bookmark (in <folder> of this space)
  bookmark overwrite     Overwrite the current tab's bookmark with its current layout
  bookmark rename <bookmark> <title>
  bookmark move <bookmark> [<folder>]  Move a bookmark into <folder> (default: the top of the space)
  bookmark trash <bookmark>          Move a bookmark to the trash
  bookmark reorder <bookmark> [<before>]  Put a bookmark before <before> (same folder; default: last)
                         <bookmark> is a .prelude path or a name as `prelude bookmarks` shows it (folder/title)
  folders                List bookmark folders of this space (empty ones too)
  folder new <name>      Create a bookmark folder in this space
  folder rename <folder> <name>
  folder remove <folder> Remove a bookmark folder; its contents move to the top of the space
  paths [n]              Folders used by tab n (files, open, save, download, terminal)
  watch                  Print state events as JSON lines
  watch --bar            One JSON line per change for a bar (waybar's custom module, return-type json).
                         Keeps running and reconnects while prelude-core is not running
  sidebar [show|hide|toggle|wide|compact|restart]
                         Toggle the sidebar between wide and compact (default: toggle). hide stops it
  shell                  Type these commands one after another (the PRELUDE CLI)
  choose <open|open-multiple|directory|save> --output <file> [--name <name>] [--terminal <cmd>] <folder>
                         The file chooser of app dialogs: yazi in a terminal. Writes the chosen paths to <file>
                         (what prelude-core's dialog portal runs by default)
  mcp                    Run an MCP server on stdin/stdout: PRELUDE's commands as tools for AI agents
                         (e.g. claude mcp add prelude -- prelude mcp)
  skill [show [<file>]]  Print the skill that teaches AI agents how to use PRELUDE (SKILL.md, format.md)
  skill install [<dir>...]
                         Write the skill into <dir>/prelude/ (default: ~/.agents/skills and ~/.claude/skills)
  raw <cmd> [args-json]  Send a command and print the result as JSON

The core socket is $PRELUDE_CORE_SOCK or $XDG_RUNTIME_DIR/prelude/core.sock.";

/// コアとのつながり(prelude-proto のクライアント)
struct Conn;

impl Conn {
    fn open(subscribe: bool) -> Result<Client, String> {
        Client::connect("prelude", subscribe)
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

/// ペインの今の場所(path)をコアに伝える。ペインは --pane、無ければ $PRELUDE_PANE、それも無ければ今のペイン
fn pane_path(id: Option<&str>, folder: &str) -> Result<(), String> {
    let id = id.map(str::to_string).or_else(|| std::env::var("PRELUDE_PANE").ok().filter(|s| !s.is_empty()));
    let mut c = Conn::open(false)?;
    let pane: u64 = match id {
        Some(id) => id.parse().map_err(|_| format!("Not a pane id: {id}"))?,
        None => {
            let s = c.state()?;
            s.tabs.iter().find(|t| Some(t.id) == s.active).map(|t| t.active_pane).ok_or("No tab")?
        }
    };
    let path = abs(folder);
    c.call("pane.update", json!({ "pane": pane, "fields": { "path": path.to_string_lossy() } }))?;
    Ok(())
}

fn bm_save(title: &str, folder: &str) -> Result<(), String> {
    let mut c = Conn::open(false)?;
    let folder = if folder.is_empty() { String::new() } else { folder_path(&mut c, folder)? };
    println!("{}", c.call("bookmark.save", json!({ "title": title, "folder": folder }))?.as_str().unwrap_or(""));
    Ok(())
}

/// ブックマークを開く。インストール済みのアプリと一致しない起動コマンドは、run でなければ外して開き、知らせる
fn open_bookmark(c: &mut Client, path: &str, run: bool) -> Result<(), String> {
    let mut r: BookmarkOpened = serde_json::from_value(c.call("bookmark.open", json!({ "path": path }))?).map_err(|e| e.to_string())?;
    if run && !r.held.is_empty() {
        // 外したコマンドも実行する: 開いたタブを閉じて、許して開き直す
        c.call("tab.close", json!({ "tab": { "id": r.tab } }))?;
        r = serde_json::from_value(c.call("bookmark.open", json!({ "path": path, "allow": r.held }))?).map_err(|e| e.to_string())?;
    }
    if !r.held.is_empty() {
        eprintln!("Not run (does not match any installed app). Unless you made this file yourself, keep it so:");
        for h in &r.held {
            eprintln!("  {h}");
        }
        eprintln!("To run them: prelude open --run {path}");
    }
    Ok(())
}

/// 開く(対象の決め方は [`target::resolve`])
fn open(c: &mut Client, target: &str, run: bool) -> Result<(), String> {
    match target::resolve(c, target)? {
        Target::Bookmark(path) => open_bookmark(c, &path, run)?,
        Target::Folder(path) => {
            c.call("tab.open", json!({ "pane": { "type": "files", "path": path } }))?;
        }
        Target::Url(url) => {
            c.call("tab.open", json!({ "pane": { "type": "web", "url": url } }))?;
        }
    }
    Ok(())
}

/// 候補の 1 行(ランチャーに見せる形。`prelude open` はこの行を受け取って開く)
fn item_line(i: &Item) -> String {
    let kind = match i.kind.as_str() {
        "tab" => format!("Tab {}", i.detail),
        "bookmark" if !i.detail.is_empty() => format!("Bookmark ({})", i.detail),
        "bookmark" => "Bookmark".into(),
        "app" => "App".into(),
        "space" => "Space".into(),
        k => k.to_string(),
    };
    format!("{}  ·  {kind}", i.label.replace(['\n', '\t'], " "))
}

fn items(c: &mut Client) -> Result<Vec<Item>, String> {
    serde_json::from_value(c.call("item.list", json!({}))?).map_err(|e| e.to_string())
}

/// 標準入力の 1 行を開く。`prelude items` の行ならその候補、違えば open の対象(ランチャーで打った URL など)。空なら何もしない
fn open_stdin(run: bool) -> Result<(), String> {
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).map_err(|e| e.to_string())?;
    let line = line.trim_end_matches(['\n', '\r']);
    if line.trim().is_empty() {
        return Ok(());
    }
    let mut c = Conn::open(false)?;
    match items(&mut c)?.into_iter().find(|i| item_line(i) == line) {
        Some(i) => match i.key.strip_prefix("bookmark:") {
            Some(path) => open_bookmark(&mut c, path, run), // 外した起動コマンドの知らせは open と同じ
            None => c.call("item.open", json!({ "key": i.key })).map(|_| ()),
        },
        None => open(&mut c, line.trim(), run),
    }
}

/// ペインを 1 行ずつ: タブの番号・ペインの id・種類・中身
fn print_panes(num: u32, l: &Value) {
    if l["k"] == "split" {
        print_panes(num, &l["a"]);
        print_panes(num, &l["b"]);
        return;
    }
    let what = ["url", "path", "cmd"].iter().find_map(|k| l[*k].as_str()).unwrap_or("");
    let state = if l["stopped"] == true { "  (stopped)" } else { "" };
    let title = l["window_title"].as_str().map(|t| format!("  \"{t}\"")).unwrap_or_default();
    println!("{num:>3}  {:>4}  {:<8} {}{title}{state}", l["id"], l["type"].as_str().unwrap_or(""), short(what));
}

/// ペインの窓の題名(配置の木から id で探す)
fn window_title(l: &Value, pane: u64) -> Option<&str> {
    if l["k"] == "split" {
        return window_title(&l["a"], pane).or_else(|| window_title(&l["b"], pane));
    }
    (l["id"].as_u64() == Some(pane)).then(|| l["window_title"].as_str()).flatten()
}

/// pango の文字(waybar は text・tooltip を pango のマークアップとして読む)
fn pango(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// バーに出す 1 行。text は「スペース  今のタブの番号 題名」、tooltip はタブの一覧、class はつながっているか。
/// waybar の custom モジュール(`"exec": "prelude watch --bar", "return-type": "json"`)がそのまま読める形
fn bar_line(s: Option<&State>) -> Value {
    let Some(s) = s else {
        return json!({ "text": "", "tooltip": "prelude-core is not running", "class": "offline", "alt": "offline" });
    };
    let title = |t: &prelude_proto::TabInfo| {
        let w = window_title(&t.layout, t.active_pane).filter(|w| !w.is_empty()).unwrap_or(&t.title);
        let w: String = w.chars().take(60).collect();
        pango(&w)
    };
    let active = s.tabs.iter().find(|t| Some(t.id) == s.active);
    let text = match active {
        Some(t) => format!("{}  {} {}", pango(&s.space), t.num, title(t)),
        None => pango(&s.space),
    };
    let tooltip: Vec<String> =
        s.tabs.iter().map(|t| format!("{} {:>2}  {}", if Some(t.id) == s.active { "*" } else { " " }, t.num, title(t))).collect();
    json!({ "text": text, "tooltip": tooltip.join("\n"), "class": "online", "alt": s.space })
}

/// `watch --bar`: コアが動いていなければ空の行を出し、2 秒ごとにつなぎ直す。同じ行は続けて出さない
fn watch_bar() -> Result<(), String> {
    let mut last = String::new();
    let mut emit = |v: Value| {
        let l = v.to_string();
        if l != last {
            println!("{l}");
            let _ = std::io::stdout().flush();
            last = l;
        }
    };
    loop {
        if let Ok(mut c) = Conn::open(true) {
            while let Ok(m) = c.read() {
                if let Incoming::Event(e) = m {
                    if e.event == "state" {
                        if let Ok(s) = serde_json::from_value::<State>(e.data) {
                            emit(bar_line(Some(&s)));
                        }
                    }
                }
            }
        }
        emit(bar_line(None));
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

/// `place` で知らせた場所の控え(コアは場所を送り返さないので、続けて使えるように手元に置く)
fn places_file() -> PathBuf {
    let mut p = prelude_proto::socket_path();
    p.set_extension("places.json");
    p
}
fn places() -> Vec<Value> {
    std::fs::read_to_string(places_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}
fn save_places(v: &[Value]) {
    let _ = std::fs::write(places_file(), serde_json::to_string(v).unwrap_or_default());
}

/// 動いているサイドバーのプロセス
fn sidebar_pids() -> Vec<i32> {
    let Ok(rd) = std::fs::read_dir("/proc") else { return vec![] };
    rd.flatten()
        .filter_map(|e| {
            let pid = e.file_name().to_str()?.parse::<i32>().ok()?;
            let comm = std::fs::read_to_string(e.path().join("comm")).ok()?;
            (comm.trim() == "prelude-sidebar").then_some(pid)
        })
        .collect()
}

fn sidebar_stop(pids: &[i32]) {
    for p in pids {
        let _ = std::process::Command::new("kill").arg(p.to_string()).status();
    }
}

/// サイドバーを起動する。この `prelude` が終わっても動き続ける
fn sidebar_start() -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    Command::new("prelude-sidebar")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot start prelude-sidebar ({e})"))
}

fn sidebar(what: &str) -> Result<(), String> {
    let pids = sidebar_pids();
    match what {
        "show" if pids.is_empty() => sidebar_start(),
        "show" => Ok(()),
        "hide" => {
            sidebar_stop(&pids);
            Ok(())
        }
        // 広い ⇄ 細い列。動いていなければ起動する(完全に隠すのは hide だけ)
        "toggle" if pids.is_empty() => sidebar_start(),
        "toggle" => prelude_proto::sidebar::set_compact(!prelude_proto::sidebar::is_compact()),
        "wide" | "compact" => {
            prelude_proto::sidebar::set_compact(what == "compact")?;
            if pids.is_empty() {
                sidebar_start()
            } else {
                Ok(())
            }
        }
        "restart" => {
            sidebar_stop(&pids);
            std::thread::sleep(std::time::Duration::from_millis(300));
            sidebar_start()
        }
        _ => Err("Usage: prelude sidebar [show|hide|toggle|wide|compact|restart]".into()),
    }
}

/// 1 行を言葉に分ける。"…" と '…' の中は 1 つの言葉。閉じていなければ None
fn split_words(line: &str) -> Option<Vec<String>> {
    let (mut out, mut cur, mut quote, mut any) = (vec![], String::new(), None, false);
    for c in line.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                any = true;
            }
            (None, c) if c.is_whitespace() => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            (None, c) => cur.push(c),
        }
    }
    if quote.is_some() {
        return None;
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    Some(out)
}

/// PRELUDE CLI。Electron 版の `PRELUDE` と同じように、`prelude` を省いて命令を打つ。exit か Ctrl+D で閉じる
fn shell() -> Result<(), String> {
    println!("PRELUDE  ({} / help / exit)", env!("CARGO_PKG_VERSION"));
    let _ = run(&[]).map_err(|e| eprintln!("{e}"));
    let stdin = std::io::stdin();
    loop {
        print!("PRELUDE> ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if stdin.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            println!();
            return Ok(());
        }
        let Some(words) = split_words(&line) else {
            eprintln!("Unclosed quote");
            continue;
        };
        match words.first().map(String::as_str) {
            None => {}
            Some("exit" | "quit") => return Ok(()),
            Some("shell") => eprintln!("Already in the PRELUDE CLI"),
            _ => {
                if let Err(e) = run(&words) {
                    eprintln!("{e}");
                }
            }
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    if a.first() == Some(&"choose") {
        return choose::run(&a[1..]);
    }
    if a.as_slice() == ["mcp"] {
        return mcp::run();
    }
    if a.first() == Some(&"skill") {
        return skill::run(&a[1..], &home());
    }
    match a.as_slice() {
        ["shell"] => shell()?,
        ["sidebar"] => sidebar("toggle")?,
        ["sidebar", what] => sidebar(what)?,
        [] | ["status"] => {
            let s = Conn::open(false)?.state()?;
            println!("Space: {}  ({})", s.space, short(&s.space_dir));
            print_tabs(&s);
        }
        ["help" | "-h" | "--help"] => println!("{HELP}"),
        ["tabs"] => print_tabs(&Conn::open(false)?.state()?),
        ["mute", pane] | ["mute", pane, "on"] | ["mute", pane, "off"] => {
            let pane: u64 = pane.parse().map_err(|_| format!("Not a pane id: {pane}"))?;
            Conn::open(false)?.call("web.mute", json!({ "pane": pane, "on": a.get(2) != Some(&"off") }))?;
        }
        ["reopen"] => {
            Conn::open(false)?.call("tab.reopen", json!({}))?;
        }
        ["tab", "dup"] => {
            Conn::open(false)?.call("tab.duplicate", json!({}))?;
        }
        ["tab", "dup", n] => {
            Conn::open(false)?.call("tab.duplicate", json!({ "tab": num_arg(n)? }))?;
        }
        ["tab", "renumber", n, new] => {
            let num: u32 = new.parse().map_err(|_| format!("Not a number: {new}"))?;
            Conn::open(false)?.call("tab.renumber", json!({ "tab": num_arg(n)?, "num": num }))?;
        }
        ["tab", n] => {
            Conn::open(false)?.call("tab.activate", json!({ "tab": num_arg(n)? }))?;
        }
        ["close"] => {
            Conn::open(false)?.call("tab.close", json!({}))?;
        }
        ["close", n] => {
            Conn::open(false)?.call("tab.close", json!({ "tab": num_arg(n)? }))?;
        }
        ["pane", "close"] => {
            let mut c = Conn::open(false)?;
            let s = c.state()?;
            let pane = s.tabs.iter().find(|t| Some(t.id) == s.active).map(|t| t.active_pane).ok_or("No tab")?;
            c.call("pane.close", json!({ "pane": pane }))?;
        }
        ["pane", "path", folder] => pane_path(None, folder)?,
        ["pane", "path", "--pane", id, folder] => pane_path(Some(id), folder)?,
        ["pane", "close", id] => {
            let pane: u64 = id.parse().map_err(|_| format!("Not a pane id: {id}"))?;
            Conn::open(false)?.call("pane.close", json!({ "pane": pane }))?;
        }
        ["open"] => open_stdin(false)?,
        ["open", "--run"] => open_stdin(true)?,
        ["items"] | ["items", "--json"] => {
            let json = a.len() == 2;
            let mut out = std::io::stdout().lock();
            for i in items(&mut Conn::open(false)?)? {
                let line = if json { serde_json::to_string(&i).unwrap_or_default() } else { item_line(&i) };
                if writeln!(out, "{line}").is_err() {
                    break; // 読む側が先に終わった(head・ランチャーを閉じたなど)
                }
            }
        }
        ["open", "--run", rest @ ..] if !rest.is_empty() => open(&mut Conn::open(false)?, &rest.join(" "), true)?,
        ["open", rest @ ..] if !rest.is_empty() => open(&mut Conn::open(false)?, &rest.join(" "), false)?,
        ["new", kind, rest @ ..] => {
            let mut pane = serde_json::Map::new();
            pane.insert("type".into(), json!(kind));
            for kv in rest {
                let (k, v) = kv.split_once('=').ok_or_else(|| format!("Not key=value: {kv}"))?;
                // 引用符で囲まれてシェルが ~ を直さなかったときも、ホームにする
                let v = match v.strip_prefix('~') {
                    Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("{}{rest}", home()),
                    _ => v.to_string(),
                };
                pane.insert(k.into(), json!(v));
            }
            Conn::open(false)?.call("tab.open", json!({ "pane": pane }))?;
        }
        ["panes"] => {
            let s = Conn::open(false)?.state()?;
            for t in &s.tabs {
                print_panes(t.num, &t.layout);
            }
        }
        ["place", pane, x, y, w, h] => {
            let n = |v: &str| v.parse::<i64>().map_err(|_| format!("Not a number: {v}"));
            let mut c = Conn::open(false)?;
            // 今の場所の一覧に足す(UI は見えているペインを全部送るので、ここでも今の分に重ねる)
            let rect = json!({ "pane": n(pane)?, "x": n(x)?, "y": n(y)?, "width": n(w)?, "height": n(h)? });
            let mut rects: Vec<Value> = places().into_iter().filter(|r| r["pane"] != rect["pane"]).collect();
            if rect["width"].as_i64() > Some(0) {
                rects.push(rect);
            }
            c.call("pane.place", json!({ "rects": rects }))?;
            save_places(&rects);
        }
        ["url", pane, url] => {
            let pane: u64 = pane.parse().map_err(|_| format!("Not a pane id: {pane}"))?;
            let url = if url.contains("://") { url.to_string() } else { format!("https://{url}") };
            Conn::open(false)?.call("pane.update", json!({ "pane": pane, "fields": { "url": url } }))?;
        }
        ["go", pane, action @ ("back" | "forward" | "reload")] => {
            let pane: u64 = pane.parse().map_err(|_| format!("Not a pane id: {pane}"))?;
            Conn::open(false)?.call("web.go", json!({ "pane": pane, "action": action }))?;
        }
        ["recipes"] => {
            let list: Vec<RecipeInfo> =
                serde_json::from_value(Conn::open(false)?.call("recipe.list", json!({}))?).map_err(|e| e.to_string())?;
            for r in list {
                println!("  {:<12} {}", r.kind, r.name);
            }
        }
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
        ["bookmark", "save", title] => bm_save(title, "")?,
        ["bookmark", "save", title, folder] => bm_save(title, folder)?,
        ["bookmark", "overwrite"] => {
            println!("{}", Conn::open(false)?.call("bookmark.overwrite", json!({}))?.as_str().unwrap_or(""));
        }
        ["bookmark", "rename", b, title] => {
            let mut c = Conn::open(false)?;
            let path = bookmark_path(&mut c, b)?;
            println!("{}", c.call("bookmark.rename", json!({ "path": path, "title": title }))?.as_str().unwrap_or(""));
        }
        ["bookmark", "move", b] | ["bookmark", "move", b, _] => {
            let mut c = Conn::open(false)?;
            let path = bookmark_path(&mut c, b)?;
            let folder = match a.get(3) {
                Some(f) => folder_path(&mut c, f)?,
                None => String::new(),
            };
            println!("{}", c.call("bookmark.move", json!({ "path": path, "folder": folder }))?.as_str().unwrap_or(""));
        }
        ["bookmark", "trash", b] => {
            let mut c = Conn::open(false)?;
            let path = bookmark_path(&mut c, b)?;
            c.call("bookmark.trash", json!({ "path": path }))?;
        }
        ["bookmark", "reorder", b] | ["bookmark", "reorder", b, _] => {
            let mut c = Conn::open(false)?;
            let path = bookmark_path(&mut c, b)?;
            let before = match a.get(3) {
                Some(x) => bookmark_path(&mut c, x)?,
                None => String::new(),
            };
            c.call("bookmark.reorder", json!({ "path": path, "before": before }))?;
        }
        ["folders"] => {
            let list: Vec<FolderInfo> =
                serde_json::from_value(Conn::open(false)?.call("folder.list", json!({}))?).map_err(|e| e.to_string())?;
            for f in list {
                println!("  {}", f.title);
            }
        }
        ["folder", "new", name] => {
            println!("{}", Conn::open(false)?.call("folder.create", json!({ "name": name }))?.as_str().unwrap_or(""));
        }
        ["folder", "rename", f, name] => {
            let mut c = Conn::open(false)?;
            let path = folder_path(&mut c, f)?;
            println!("{}", c.call("folder.rename", json!({ "path": path, "name": name }))?.as_str().unwrap_or(""));
        }
        ["folder", "remove", f] => {
            let mut c = Conn::open(false)?;
            let path = folder_path(&mut c, f)?;
            c.call("folder.remove", json!({ "path": path }))?;
        }
        ["bookmarks"] => {
            let list: Vec<BookmarkInfo> =
                serde_json::from_value(Conn::open(false)?.call("bookmark.list", json!({}))?).map_err(|e| e.to_string())?;
            for b in list {
                let name = if b.folder.is_empty() { b.title } else { format!("{}/{}", b.folder, b.title) };
                println!("  {:>4}  {name}", format!("b{}", b.num));
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
        ["watch", "--bar"] => watch_bar()?,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 行を言葉に分ける() {
        let w = |s: &str| split_words(s).map(|v| v.join("|"));
        assert_eq!(w("tab 2"), Some("tab|2".into()));
        assert_eq!(w("  new  app cmd=\"foot -a x\" "), Some("new|app|cmd=foot -a x".into()));
        assert_eq!(w("open '~/a b'\n"), Some("open|~/a b".into()));
        assert_eq!(w("\"\""), Some("".into()));
        assert_eq!(split_words("").unwrap().len(), 0);
        assert_eq!(w("open \"a"), None);
    }

    #[test]
    fn バーの行() {
        let s: State = serde_json::from_value(json!({
            "space": "仕事 & 家",
            "space_dir": "/x",
            "active": 8,
            "tabs": [
                { "id": 7, "num": 1, "title": "web", "bookmark": null, "active_pane": 1, "layout": { "k": "leaf", "id": 1, "type": "web" } },
                { "id": 8, "num": 2, "title": "app", "bookmark": null, "active_pane": 3, "layout": {
                    "k": "split", "dir": "row", "ratio": 0.5,
                    "a": { "k": "leaf", "id": 2, "type": "app", "window_title": "a" },
                    "b": { "k": "leaf", "id": 3, "type": "app", "window_title": "<b>" } } }
            ]
        }))
        .unwrap();
        let v = bar_line(Some(&s));
        assert_eq!(v["text"], "仕事 &amp; 家  2 &lt;b&gt;");
        assert_eq!(v["tooltip"], "   1  web\n*  2  &lt;b&gt;");
        assert_eq!(v["class"], "online");
        assert_eq!(bar_line(None)["class"], "offline");
    }
}

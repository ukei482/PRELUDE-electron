//! `prelude mcp`: MCP(Model Context Protocol)のサーバー。標準入出力で 1 行に 1 つの JSON-RPC 2.0 を読み書きし、
//! コアの命令を AI のツールとして見せる(docs/redesign-rust.md の「AI 連携」)。
//!
//! - 読むだけのツールには readOnlyHint、消す・閉じるツールには destructiveHint を付ける
//! - 人からもらったブックマークの起動コマンドは、AI からは実行させない(CLI の `--run` にあたるものは出さない。外したコマンドは返事で知らせる)
//! - ブックマーク・フォルダの指し方は `prelude` のコマンドと同じ([`crate::target`])
//! - コアには命令ごとにつなぐ(コアを起動し直しても、そのまま使える)
use crate::target::{self, Call, Target};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

/// 話せる MCP の版(新しい順)。頼まれた版が無ければ一番新しいもので答える
pub const PROTOCOLS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "PRELUDE manages what the user is working on: spaces (folders of work), bookmarks (.prelude files with a layout and paths), tabs and panes. Call `state` first to see the current space, tabs and pane ids. Bookmarks can be given by name, folder/name, number (b3) or path. PRELUDE does not place windows; the compositor does. Commands in bookmarks from other people that do not match an installed app are not run; tell the user instead of trying to run them.";

/// 引数の形(JSON Schema の properties の 1 つ)
fn prop(kind: &str, desc: &str) -> Value {
    json!({ "type": kind, "description": desc })
}

/// ツールの定義: 名前・説明・properties・必須の引数・読むだけか・壊す操作か
type Def = (&'static str, &'static str, Value, Vec<&'static str>, bool, bool);

fn defs() -> Vec<Def> {
    let tab = || prop("integer", "Tab number as shown by `state` (num). Omit for the current tab");
    let pane = || prop("integer", "Pane id (from `state`)");
    let bm = || prop("string", "Bookmark: name, folder/name, number like b3, or a .prelude path");
    let folder = || prop("string", "Bookmark folder: its name in this space, or its path. Empty or omitted: the top of the space");
    let spec = || json!({ "type": "object", "description": "Pane: {\"type\": \"web\", \"url\": …}, {\"type\": \"app\", \"cmd\": …}, {\"type\": \"files\", \"path\": …}, {\"type\": \"console\"}, or a recipe type from `recipes_list`", "properties": { "type": { "type": "string" } }, "required": ["type"], "additionalProperties": true });
    vec![
        (
            "state",
            "Current space, tabs (number, title, bookmark, layout with pane ids, types and places) and the window backend",
            json!({}),
            vec![],
            true,
            false,
        ),
        (
            "bookmarks_list",
            "Bookmarks of the current space, in order, with number (b1…), title, folder, kind and path",
            json!({}),
            vec![],
            true,
            false,
        ),
        ("folders_list", "Bookmark folders of the current space (empty ones too)", json!({}), vec![], true, false),
        ("spaces_list", "Names of all spaces", json!({}), vec![], true, false),
        ("recipes_list", "Pane types made from recipes (~/.config/prelude/panes)", json!({}), vec![], true, false),
        (
            "paths_get",
            "Folders a tab uses: files, open, save, download, terminal (from its bookmark, then the space)",
            json!({ "tab": tab() }),
            vec![],
            true,
            false,
        ),
        ("items_list", "Launcher items: tabs, then bookmarks, apps and spaces by score", json!({}), vec![], true, false),
        (
            "open",
            "Open a bookmark (name, number, path), a folder, or a URL — like the launcher",
            json!({ "target": prop("string", "What to open") }),
            vec!["target"],
            false,
            false,
        ),
        ("tab_open", "Open a pane in a new tab", json!({ "pane": spec() }), vec!["pane"], false, false),
        ("tab_activate", "Switch to a tab", json!({ "tab": prop("integer", "Tab number") }), vec!["tab"], false, false),
        ("tab_close", "Close a tab", json!({ "tab": tab() }), vec![], false, true),
        ("tab_duplicate", "Duplicate a tab below it", json!({ "tab": tab() }), vec![], false, false),
        (
            "tab_renumber",
            "Give a tab another number (swaps with the tab that has it)",
            json!({ "tab": prop("integer", "Tab number"), "num": prop("integer", "New number (1-99)") }),
            vec!["tab", "num"],
            false,
            false,
        ),
        ("tab_reopen", "Reopen the last closed tab of this space", json!({}), vec![], false, false),
        (
            "pane_split",
            "Split a pane and open another pane right (row) or below (col)",
            json!({ "pane": pane(), "dir": { "type": "string", "enum": ["row", "col"] }, "new": spec() }),
            vec!["pane", "dir", "new"],
            false,
            false,
        ),
        ("pane_close", "Close a pane (closes the tab if it is the last pane)", json!({ "pane": pane() }), vec!["pane"], false, true),
        ("pane_focus", "Make a pane the current pane", json!({ "pane": pane() }), vec!["pane"], false, false),
        (
            "space_switch",
            "Switch to a space (its tabs replace the current ones; the current ones are kept for later)",
            json!({ "name": prop("string", "Space name") }),
            vec!["name"],
            false,
            false,
        ),
        (
            "space_create",
            "Create a space (does not switch to it)",
            json!({ "name": prop("string", "Space name") }),
            vec!["name"],
            false,
            false,
        ),
        (
            "bookmark_save",
            "Save a tab as a new bookmark (the whole layout). The tab becomes opened from it",
            json!({ "title": prop("string", "Title (also the file name)"), "folder": folder(), "tab": tab() }),
            vec!["title"],
            false,
            false,
        ),
        (
            "bookmark_overwrite",
            "Overwrite the tab's bookmark with the tab's current layout (keeps title and paths)",
            json!({ "tab": tab() }),
            vec![],
            false,
            true,
        ),
        (
            "bookmark_rename",
            "Rename a bookmark (title and file name)",
            json!({ "bookmark": bm(), "title": prop("string", "New title") }),
            vec!["bookmark", "title"],
            false,
            false,
        ),
        ("bookmark_move", "Move a bookmark into a folder", json!({ "bookmark": bm(), "folder": folder() }), vec!["bookmark"], false, false),
        (
            "bookmark_reorder",
            "Put a bookmark before another one in the same folder",
            json!({ "bookmark": bm(), "before": prop("string", "Bookmark to put it before. Omit to put it last") }),
            vec!["bookmark"],
            false,
            false,
        ),
        ("bookmark_trash", "Move a bookmark to the trash", json!({ "bookmark": bm() }), vec!["bookmark"], false, true),
        (
            "folder_create",
            "Create a bookmark folder in this space",
            json!({ "name": prop("string", "Folder name") }),
            vec!["name"],
            false,
            false,
        ),
        (
            "folder_rename",
            "Rename a bookmark folder",
            json!({ "folder": folder(), "name": prop("string", "New name") }),
            vec!["folder", "name"],
            false,
            false,
        ),
        (
            "folder_remove",
            "Remove a bookmark folder; its contents move to the top of the space",
            json!({ "folder": folder() }),
            vec!["folder"],
            false,
            true,
        ),
        (
            "config_get",
            "PRELUDE settings: every item (or one, with key) with its value, where it comes from (default, profile, settings.toml), what it does and when it applies",
            json!({ "key": prop("string", "Setting like windows.level. Omit for all") }),
            vec![],
            true,
            false,
        ),
        (
            "config_set",
            "Change one PRELUDE setting in ~/.config/prelude/settings.toml (checked against the catalog; comments are kept). Only when the user asks",
            json!({ "key": prop("string", "Setting like windows.level"), "value": prop("string", "New value, e.g. arrange, true, 30, or a TOML value") }),
            vec!["key", "value"],
            false,
            false,
        ),
    ]
}

/// tools/list の中身
pub fn tools() -> Vec<Value> {
    defs()
        .into_iter()
        .map(|(name, desc, props, required, read_only, destructive)| {
            json!({
                "name": name,
                "description": desc,
                "inputSchema": { "type": "object", "properties": props, "required": required },
                "annotations": { "readOnlyHint": read_only, "destructiveHint": destructive },
            })
        })
        .collect()
}

fn s<'a>(a: &'a Value, k: &str) -> Result<&'a str, String> {
    a[k].as_str().ok_or_else(|| format!("`{k}` (string) is required"))
}

fn n(a: &Value, k: &str) -> Result<u64, String> {
    a[k].as_u64().ok_or_else(|| format!("`{k}` (integer) is required"))
}

/// tab(番号)があれば `{ "tab": { "num": … } }`、無ければ `{}`(今のタブ)
fn tab_arg(a: &Value) -> Value {
    match a["tab"].as_u64() {
        Some(num) => json!({ "tab": { "num": num } }),
        None => json!({}),
    }
}

/// ツールを動かす(コアの命令にする)→ 結果
pub fn call_tool(c: &mut impl Call, name: &str, a: &Value) -> Result<Value, String> {
    match name {
        "state" => c.call("state.get", json!({})),
        "bookmarks_list" => c.call("bookmark.list", json!({})),
        "folders_list" => c.call("folder.list", json!({})),
        "spaces_list" => c.call("space.list", json!({})),
        "recipes_list" => c.call("recipe.list", json!({})),
        "paths_get" => c.call("paths.get", tab_arg(a)),
        "items_list" => c.call("item.list", json!({})),
        "open" => match target::resolve(c, s(a, "target")?)? {
            Target::Bookmark(path) => {
                let r = c.call("bookmark.open", json!({ "path": path }))?;
                let note = if r["held"].as_array().is_some_and(|h| !h.is_empty()) {
                    "Some commands were not run because they do not match an installed app (listed in held). Ask the user before running them; they can run `prelude open --run <bookmark>` themselves."
                } else {
                    ""
                };
                Ok(json!({ "opened": "bookmark", "path": path, "tab": r["tab"], "held": r["held"], "note": note }))
            }
            Target::Folder(path) => {
                Ok(json!({ "opened": "folder", "tab": c.call("tab.open", json!({ "pane": { "type": "files", "path": path } }))? }))
            }
            Target::Url(url) => {
                Ok(json!({ "opened": "web", "tab": c.call("tab.open", json!({ "pane": { "type": "web", "url": url } }))? }))
            }
        },
        "tab_open" => c.call("tab.open", json!({ "pane": a["pane"] })),
        "tab_activate" => c.call("tab.activate", json!({ "tab": { "num": n(a, "tab")? } })),
        "tab_close" => c.call("tab.close", tab_arg(a)),
        "tab_duplicate" => c.call("tab.duplicate", tab_arg(a)),
        "tab_renumber" => c.call("tab.renumber", json!({ "tab": { "num": n(a, "tab")? }, "num": n(a, "num")? })),
        "tab_reopen" => c.call("tab.reopen", json!({})),
        "pane_split" => c.call("pane.split", json!({ "pane": n(a, "pane")?, "dir": s(a, "dir")?, "new": a["new"] })),
        "pane_close" => c.call("pane.close", json!({ "pane": n(a, "pane")? })),
        "pane_focus" => c.call("pane.focus", json!({ "pane": n(a, "pane")? })),
        "space_switch" => c.call("space.switch", json!({ "name": s(a, "name")? })),
        "space_create" => c.call("space.create", json!({ "name": s(a, "name")? })),
        "bookmark_save" => {
            let folder = target::folder_path(c, a["folder"].as_str().unwrap_or(""))?;
            let mut args = tab_arg(a);
            args["title"] = json!(s(a, "title")?);
            args["folder"] = json!(folder);
            c.call("bookmark.save", args)
        }
        "bookmark_overwrite" => c.call("bookmark.overwrite", tab_arg(a)),
        "bookmark_rename" => {
            let path = target::bookmark_path(c, s(a, "bookmark")?)?;
            c.call("bookmark.rename", json!({ "path": path, "title": s(a, "title")? }))
        }
        "bookmark_move" => {
            let path = target::bookmark_path(c, s(a, "bookmark")?)?;
            let folder = target::folder_path(c, a["folder"].as_str().unwrap_or(""))?;
            c.call("bookmark.move", json!({ "path": path, "folder": folder }))
        }
        "bookmark_reorder" => {
            let path = target::bookmark_path(c, s(a, "bookmark")?)?;
            let before = match a["before"].as_str().filter(|b| !b.is_empty()) {
                Some(b) => target::bookmark_path(c, b)?,
                None => String::new(),
            };
            c.call("bookmark.reorder", json!({ "path": path, "before": before }))
        }
        "bookmark_trash" => {
            let path = target::bookmark_path(c, s(a, "bookmark")?)?;
            c.call("bookmark.trash", json!({ "path": path }))
        }
        "folder_create" => c.call("folder.create", json!({ "name": s(a, "name")? })),
        "folder_rename" => {
            let path = target::folder_path(c, s(a, "folder")?)?;
            c.call("folder.rename", json!({ "path": path, "name": s(a, "name")? }))
        }
        "folder_remove" => {
            let path = target::folder_path(c, s(a, "folder")?)?;
            c.call("folder.remove", json!({ "path": path }))
        }
        "config_get" => {
            let c = prelude_format::settings::Config::load();
            match a["key"].as_str().filter(|k| !k.is_empty()) {
                Some(k) => {
                    let i = prelude_format::settings::find(k).ok_or_else(|| format!("Unknown setting: {k}"))?;
                    Ok(crate::config::item_json(&c, i))
                }
                None => Ok(crate::config::all_json(&c)),
            }
        }
        "config_set" => {
            let key = s(a, "key")?;
            let raw = match &a["value"] {
                Value::String(v) => v.clone(),
                Value::Null => return Err("`value` is required".into()),
                other => other.to_string(),
            };
            let (v, note) = crate::config::set(key, &raw)?;
            Ok(json!({ "key": key, "value": v.to_string(), "note": note }))
        }
        _ => Err(format!("Unknown tool: {name}")),
    }
}

fn reply(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// 届いたもの 1 つに答える。通知(id の無いもの)と返事には答えない
pub fn handle(c: &mut impl Call, msg: &Value) -> Option<Value> {
    let method = msg["method"].as_str()?;
    let id = msg.get("id")?.clone();
    let p = &msg["params"];
    Some(match method {
        "initialize" => {
            let asked = p["protocolVersion"].as_str().unwrap_or("");
            let version = PROTOCOLS.iter().find(|v| **v == asked).unwrap_or(&PROTOCOLS[0]);
            reply(
                &id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "prelude", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": INSTRUCTIONS,
                }),
            )
        }
        "ping" => reply(&id, json!({})),
        "tools/list" => reply(&id, json!({ "tools": tools() })),
        "tools/call" => {
            let name = p["name"].as_str().unwrap_or("");
            if !defs().iter().any(|d| d.0 == name) {
                return Some(error(&id, -32602, &format!("Unknown tool: {name}")));
            }
            let args = if p["arguments"].is_object() { p["arguments"].clone() } else { json!({}) };
            let (text, is_error) = match call_tool(c, name, &args) {
                Ok(v) => (serde_json::to_string_pretty(&v).unwrap_or_default(), false),
                Err(e) => (e, true),
            };
            reply(&id, json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
        }
        _ => error(&id, -32601, &format!("Method not found: {method}")),
    })
}

/// 命令ごとにコアへつなぐ(コアが起動し直しても続けられる)
struct Core;

impl Call for Core {
    fn call(&mut self, cmd: &str, args: Value) -> Result<Value, String> {
        prelude_proto::Client::connect("prelude-mcp", false)?.call(cmd, args)
    }
}

/// 標準入力から読み、標準出力へ答える(標準入力が閉じるまで)
pub fn run() -> Result<(), String> {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let answer = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(&mut Core, &msg),
            Err(e) => Some(error(&Value::Null, -32700, &format!("Parse error: {e}"))),
        };
        if let Some(a) = answer {
            writeln!(out, "{a}").and_then(|_| out.flush()).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 頼まれた命令を覚え、決まった返事をする偽のコア
    #[derive(Default)]
    struct Fake {
        calls: Vec<(String, Value)>,
    }

    impl Call for Fake {
        fn call(&mut self, cmd: &str, args: Value) -> Result<Value, String> {
            self.calls.push((cmd.to_string(), args.clone()));
            Ok(match cmd {
                "bookmark.list" => json!([
                    { "path": "/s/Main/本文.prelude", "title": "本文", "folder": "", "num": 1, "kind": "web" },
                    { "path": "/s/Main/執筆/図.prelude", "title": "図", "folder": "執筆", "num": 2, "kind": "app" },
                ]),
                "state.get" => json!({ "space": "Main", "space_dir": "/s/Main", "tabs": [] }),
                "bookmark.open" => json!({ "tab": 5, "held": ["sh -c evil"] }),
                "tab.close" if args["tab"]["num"] == 9 => return Err("No such tab".into()),
                _ => Value::Null,
            })
        }
    }

    fn call(f: &mut Fake, name: &str, args: Value) -> Value {
        handle(f, &json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": name, "arguments": args } })).unwrap()
    }

    #[test]
    fn はじめのあいさつと版() {
        let mut f = Fake::default();
        let r =
            handle(&mut f, &json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } }))
                .unwrap();
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(r["result"]["serverInfo"]["name"], "prelude");
        let r =
            handle(&mut f, &json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize", "params": { "protocolVersion": "1999-01-01" } }))
                .unwrap();
        assert_eq!(r["result"]["protocolVersion"], PROTOCOLS[0]);
        // 通知には答えない
        assert!(handle(&mut f, &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).is_none());
        assert_eq!(handle(&mut f, &json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" })).unwrap()["result"], json!({}));
        assert_eq!(handle(&mut f, &json!({ "jsonrpc": "2.0", "id": 3, "method": "nope" })).unwrap()["error"]["code"], -32601);
    }

    #[test]
    fn ツールの一覧_印と引数の形() {
        let t = tools();
        let get = |n: &str| t.iter().find(|x| x["name"] == n).unwrap().clone();
        assert_eq!(get("state")["annotations"]["readOnlyHint"], true);
        for d in ["bookmark_trash", "folder_remove", "tab_close", "pane_close", "bookmark_overwrite"] {
            assert_eq!(get(d)["annotations"]["destructiveHint"], true, "{d}");
        }
        assert_eq!(get("bookmark_save")["inputSchema"]["required"], json!(["title"]));
        // 名前が重ならず、どれも動かせる(知らない名前にならない)
        let mut names: Vec<&str> = t.iter().map(|x| x["name"].as_str().unwrap()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), t.len());
        let mut f = Fake::default();
        for n in names {
            assert!(!matches!(call_tool(&mut f, n, &json!({})), Err(e) if e.starts_with("Unknown tool")), "{n}");
        }
    }

    #[test]
    fn ツールをコアの命令にする() {
        let mut f = Fake::default();
        // 番号・名前・フォルダの名前は、コアが受け取る形にして渡す
        call(&mut f, "bookmark_move", json!({ "bookmark": "b2", "folder": "資料" }));
        assert_eq!(
            f.calls.last().unwrap(),
            &("bookmark.move".to_string(), json!({ "path": "/s/Main/執筆/図.prelude", "folder": "/s/Main/資料" }))
        );
        call(&mut f, "bookmark_save", json!({ "title": "新", "tab": 3 }));
        assert_eq!(f.calls.last().unwrap(), &("bookmark.save".to_string(), json!({ "tab": { "num": 3 }, "title": "新", "folder": "" })));
        call(&mut f, "tab_close", json!({}));
        assert_eq!(f.calls.last().unwrap(), &("tab.close".to_string(), json!({})));
        // ブックマークを開く: 外したコマンドは実行せず、返事で知らせる(allow は渡さない)
        let r = call(&mut f, "open", json!({ "target": "本文" }));
        assert_eq!(f.calls.last().unwrap(), &("bookmark.open".to_string(), json!({ "path": "/s/Main/本文.prelude" })));
        let text: Value = serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(text["held"], json!(["sh -c evil"]));
        assert!(text["note"].as_str().unwrap().contains("not run"));
    }

    #[test]
    fn 失敗はツールの失敗として返す_知らないツールは約束の失敗() {
        let mut f = Fake::default();
        let r = call(&mut f, "tab_close", json!({ "tab": 9 }));
        assert_eq!((r["result"]["isError"].clone(), r["result"]["content"][0]["text"].clone()), (json!(true), json!("No such tab")));
        let r = call(&mut f, "tab_activate", json!({}));
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("`tab`"));
        assert_eq!(call(&mut f, "rm_rf", json!({}))["error"]["code"], -32602);
    }
}

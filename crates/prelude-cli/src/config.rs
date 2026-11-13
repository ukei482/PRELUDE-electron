//! `prelude config`: 設定(`~/.config/prelude/settings.toml`)を読む・書く(docs/redesign-rust.md の「設定」)。
//! 項目は台帳(`prelude_format::settings::ITEMS`)が正。書くときは台帳で確かめ、コメントと知らない項目を残す。
//! コアは settings.toml を見張っていて、すぐ効く項目は 2 秒以内に反映する
use prelude_format::settings::{self, Apply, Config, Item, Layer};
use serde_json::{json, Value};

/// 項目の値と出どころ(MCP と `list --json`)
pub fn item_json(c: &Config, i: &Item) -> Value {
    json!({
        "key": i.key,
        "value": toml_to_json(c.get(i.key)),
        "layer": c.origin(i.key).name(),
        "default": toml_to_json(&i.default_value()),
        "doc": i.doc,
        "applies": match (i.planned, i.apply) { (true, _) => "not yet used", (_, Apply::Now) => "now", (_, Apply::Restart) => "after restart" },
    })
}

/// 全部の項目(MCP と `list --json`)
pub fn all_json(c: &Config) -> Value {
    json!({
        "file": Config::path(),
        "profile": c.profile,
        "warnings": c.warnings,
        "items": settings::ITEMS.iter().map(|i| item_json(c, i)).collect::<Vec<_>>(),
    })
}

fn toml_to_json(v: &toml::Value) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

fn item(key: &str) -> Result<&'static Item, String> {
    settings::find(key).ok_or_else(|| format!("Unknown setting: {key} (see `prelude config list`)"))
}

/// 1 項目を書く(台帳で確かめる)→ 書いた値と、効き方の知らせ
pub fn set(key: &str, raw: &str) -> Result<(toml::Value, &'static str), String> {
    let i = item(key)?;
    let v = i.parse_input(raw)?;
    settings::write_user(&Config::path(), key, Some(&v))?;
    Ok((v, note(i)))
}

/// 使用者の値を消す(組み合わせ・既定に戻る)
pub fn unset(key: &str) -> Result<(), String> {
    item(key)?;
    settings::write_user(&Config::path(), key, None)
}

fn note(i: &Item) -> &'static str {
    match (i.planned, i.apply) {
        (true, _) => "(not used by PRELUDE yet)",
        (_, Apply::Restart) => "(applies after PRELUDE restarts)",
        _ => "",
    }
}

pub fn run(args: &[&str]) -> Result<(), String> {
    let usage = "usage: prelude config [list [--json] | get <key> | set <key> <value> | unset <key> | defaults | path]";
    match args {
        [] | ["list"] => {
            let c = Config::load();
            let profile = if c.profile == "none" { "none".to_string() } else { c.profile.clone() };
            println!("# {} (profile: {profile})", Config::path().display());
            for w in &c.warnings {
                println!("# warning: {w}");
            }
            for i in settings::ITEMS {
                let layer = match c.origin(i.key) {
                    Layer::Default => String::new(),
                    Layer::Profile => format!("  [profile: {profile}]"),
                    Layer::User => "  [settings.toml]".to_string(),
                };
                println!("{} = {}{layer}  {}", i.key, c.get(i.key), note(i));
                println!("    {}", i.doc);
            }
        }
        ["list", "--json"] => println!("{}", serde_json::to_string_pretty(&all_json(&Config::load())).unwrap_or_default()),
        ["get", key] => {
            item(key)?;
            println!("{}", Config::load().get(key));
        }
        ["set", key, value] => {
            let (v, note) = set(key, value)?;
            println!("{key} = {v}  {note}");
        }
        ["unset", key] => unset(key)?,
        ["defaults"] => print!("{}", settings::defaults_toml()),
        ["path"] => println!("{}", Config::path().display()),
        _ => return Err(usage.into()),
    }
    Ok(())
}

//! 設定の台帳と層(docs/redesign-rust.md の「設定」)。
//!
//! - 項目は [`ITEMS`] の 1 か所に置く(名前・型・既定・説明・反映のしかた)。読む・`prelude config`・設定の画面・MCP はここから作る
//! - 層: 既定(台帳)→ おすすめの組み合わせ(`profiles/<名前>.toml`)→ 使用者(`~/.config/prelude/settings.toml`)。後ほど優先
//! - 使用者のファイルを書くときは、コメントと知らない項目を残す(toml_edit)
//! - 読めない値はその項目だけ既定にし、理由を [`Config::warnings`] に残す
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use toml::{Table, Value};

/// 設定のファイルの形の版。形を変えたら上げて、古い版の設定を移す
pub const VERSION: i64 = 1;

/// 値の型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Str,
    Bool,
    Int {
        min: i64,
        max: i64,
    },
    /// 決まった言葉のどれか
    Choice(&'static [&'static str]),
    /// 文字の並び
    List,
}

/// 変えたときに効くまで
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Apply {
    /// すぐ効く(コアが読み直す・使う側が読み直す)
    Now,
    /// PRELUDE を起動し直すと効く
    Restart,
}

/// 設定の項目 1 つ
#[derive(Debug, Clone, Copy)]
pub struct Item {
    /// `windows.level` のように、節.名前
    pub key: &'static str,
    pub kind: Kind,
    /// 既定(TOML の値の書き方)
    pub default: &'static str,
    pub doc: &'static str,
    pub apply: Apply,
    /// まだ PRELUDE のどこも使っていない(設計で決めた項目。書いても効かない)
    pub planned: bool,
}

const fn item(key: &'static str, kind: Kind, default: &'static str, doc: &'static str, apply: Apply) -> Item {
    Item { key, kind, default, doc, apply, planned: false }
}

const fn planned(key: &'static str, kind: Kind, default: &'static str, doc: &'static str, apply: Apply) -> Item {
    Item { key, kind, default, doc, apply, planned: true }
}

/// おすすめの組み合わせの名前(auto は環境から決める、none は使わない)
pub const PROFILES: [&str; 5] = ["auto", "none", "sway", "kde", "omarchy"];

/// 設定の台帳
pub const ITEMS: &[Item] = &[
    item(
        "profile",
        Kind::Choice(&PROFILES),
        "\"auto\"",
        "おすすめの組み合わせ。auto は環境から決める(sway・KDE・Omarchy)。none は使わない",
        Apply::Now,
    ),
    item("search_engine", Kind::Str, "\"https://www.google.com/search?q=%s\"", "検索の URL。%s が検索の言葉になる", Apply::Now),
    item("show_hidden", Kind::Bool, "false", "ファイラで隠しファイルを最初から出す", Apply::Now),
    item("space_dir", Kind::Str, "\"\"", "スペースのフォルダ。空なら ~/スペース", Apply::Restart),
    item("download_dir", Kind::Str, "\"\"", "ダウンロードの保存先の既定(paths の download が無いとき)。空なら Chromium の既定", Apply::Now),
    item(
        "terminal",
        Kind::Str,
        "\"auto\"",
        "端末(名前かパス)。auto は PATH から foot・kitty・ghostty・alacritty・wezterm・konsole の順に探す。知らない端末は -e でコマンドを渡す",
        Apply::Now,
    ),
    item(
        "agent",
        Kind::Str,
        "\"auto\"",
        "既定の AI(シェルのコマンド)。auto は PRELUDE_AGENT、無ければ claude・codex・gemini・opencode の順に探す",
        Apply::Now,
    ),
    planned(
        "windows.backend",
        Kind::Choice(&["auto", "sway", "kwin", "hyprland", "none"]),
        "\"auto\"",
        "窓のつなぎ。auto は環境から決める。none は起動するだけ",
        Apply::Restart,
    ),
    planned(
        "windows.level",
        Kind::Choice(&["auto", "launch", "workspace", "arrange", "read", "follow"]),
        "\"auto\"",
        "PRELUDE に任せる段の上限。auto はつなぎの最大。launch は起動だけ、workspace は作業領域だけ指定",
        Apply::Now,
    ),
    planned(
        "windows.adopt",
        Kind::Choice(&["tabs", "none"]),
        "\"tabs\"",
        "PRELUDE を通さずに開いた窓。tabs はタブの作業領域のものを引き取る、none は引き取らない",
        Apply::Now,
    ),
    planned("windows.workspace_prefix", Kind::Str, "\"\"", "タブの作業領域の名前の頭(環境の作業領域と分けたいとき)", Apply::Restart),
    planned("windows.floating", Kind::List, "[]", "いつも浮かせる窓(app_id・クラス)", Apply::Now),
    planned("windows.ignore", Kind::List, "[]", "PRELUDE が触らない窓(app_id・クラス)", Apply::Now),
    planned("sidebar.side", Kind::Choice(&["left", "right"]), "\"left\"", "サイドバーを出す辺", Apply::Now),
    planned(
        "sidebar.surface",
        Kind::Choice(&["auto", "layer", "window"]),
        "\"auto\"",
        "サイドバーの出し方。auto は layer-shell があればそれ、無ければ普通のウィンドウ",
        Apply::Restart,
    ),
    planned(
        "tabs.links",
        Kind::Choice(&["tab", "split"]),
        "\"tab\"",
        "Web のペインから新しく開くリンク。tab は新しいタブ、split は右に分割",
        Apply::Now,
    ),
    item("tabs.closed_max", Kind::Int { min: 0, max: 200 }, "20", "覚えておく閉じたタブの数(スペースごと)", Apply::Now),
];

/// 台帳の項目
pub fn find(key: &str) -> Option<&'static Item> {
    ITEMS.iter().find(|i| i.key == key)
}

/// TOML の値の書き方を読む
fn literal(s: &str) -> Option<Value> {
    format!("v = {s}").parse::<Table>().ok()?.remove("v")
}

impl Item {
    pub fn default_value(&self) -> Value {
        literal(self.default).expect("台帳の既定は TOML の値")
    }

    /// 値が型に合うか確かめる。合えばそのまま(リストは文字だけ)
    pub fn check(&self, v: &Value) -> Result<Value, String> {
        let bad = |want: &str| Err(format!("{}: expected {want}, got {v}", self.key));
        match (self.kind, v) {
            (Kind::Str, Value::String(s)) => {
                if self.key == "search_engine" && !s.contains("%s") {
                    return Err(format!("{}: must contain %s", self.key));
                }
                Ok(v.clone())
            }
            (Kind::Bool, Value::Boolean(_)) => Ok(v.clone()),
            (Kind::Int { min, max }, Value::Integer(n)) if (min..=max).contains(n) => Ok(v.clone()),
            (Kind::Int { min, max }, _) => bad(&format!("an integer from {min} to {max}")),
            (Kind::Choice(c), Value::String(s)) if c.contains(&s.as_str()) => Ok(v.clone()),
            (Kind::Choice(c), _) => bad(&format!("one of {}", c.join(", "))),
            (Kind::List, Value::Array(a)) if a.iter().all(Value::is_str) => Ok(v.clone()),
            (Kind::List, _) => bad("a list of strings"),
            (Kind::Str, _) => bad("a string"),
            (Kind::Bool, _) => bad("true or false"),
        }
    }

    /// コマンドで打たれた値を読む。TOML の値として読めなければ、文字(言葉)とみなす。リストは `a,b` でもよい
    pub fn parse_input(&self, raw: &str) -> Result<Value, String> {
        let v = match (literal(raw), self.kind) {
            (Some(v), _) => v,
            (None, Kind::List) => {
                Value::Array(raw.split(',').map(|s| Value::String(s.trim().to_string())).filter(|v| v.as_str() != Some("")).collect())
            }
            (None, _) => Value::String(raw.to_string()),
        };
        self.check(&v)
    }
}

/// どの層の値か
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Default,
    Profile,
    User,
}

impl Layer {
    pub fn name(self) -> &'static str {
        match self {
            Layer::Default => "default",
            Layer::Profile => "profile",
            Layer::User => "user",
        }
    }
}

/// 節.名前 で表を引く
fn lookup<'a>(t: &'a Table, key: &str) -> Option<&'a Value> {
    let mut cur = t;
    let mut parts = key.split('.').peekable();
    while let Some(p) = parts.next() {
        let v = cur.get(p)?;
        if parts.peek().is_none() {
            return Some(v);
        }
        cur = v.as_table()?;
    }
    None
}

/// おすすめの組み合わせの中身
pub fn profile_text(name: &str) -> Option<&'static str> {
    match name {
        "sway" => Some(include_str!("../profiles/sway.toml")),
        "kde" => Some(include_str!("../profiles/kde.toml")),
        "omarchy" => Some(include_str!("../profiles/omarchy.toml")),
        _ => None,
    }
}

/// auto のときの組み合わせを環境から決める: Omarchy(Hyprland で omarchy コマンドがある)・sway・KDE。どれでもなければ none
pub fn detect_profile(env: &dyn Fn(&str) -> Option<String>) -> &'static str {
    let path = env("PATH").unwrap_or_default();
    let has = |cmd: &str| path.split(':').any(|d| !d.is_empty() && Path::new(d).join(cmd).is_file());
    if env("HYPRLAND_INSTANCE_SIGNATURE").is_some() && has("omarchy") {
        "omarchy"
    } else if env("SWAYSOCK").is_some() {
        "sway"
    } else if env("XDG_CURRENT_DESKTOP").is_some_and(|d| d.to_lowercase().contains("kde")) {
        "kde"
    } else {
        "none"
    }
}

/// 層を重ねた設定
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    values: BTreeMap<&'static str, (Value, Layer)>,
    /// 使った組み合わせ(none なら使っていない)
    pub profile: String,
    /// 読めなかった値の理由
    pub warnings: Vec<String>,
    /// 使用者のファイルがまるごと読めなかった(書きかけなど)。見張る側は、前の設定を使い続ける
    pub unreadable: bool,
}

impl Config {
    /// 使用者の設定のファイル
    pub fn path() -> PathBuf {
        crate::config::dir().join("settings.toml")
    }

    /// `settings.toml` と環境から読む
    pub fn load() -> Config {
        let user = std::fs::read_to_string(Config::path()).unwrap_or_default();
        Config::from_text(&user, &|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
    }

    /// 使用者の設定の中身と環境から作る
    pub fn from_text(user: &str, env: &dyn Fn(&str) -> Option<String>) -> Config {
        let mut warnings = vec![];
        let mut unreadable = false;
        let user: Table = user.parse().unwrap_or_else(|e| {
            warnings.push(format!("settings.toml cannot be read ({e}); using the defaults"));
            unreadable = true;
            Table::new()
        });
        let mut values: BTreeMap<&'static str, (Value, Layer)> =
            ITEMS.iter().map(|i| (i.key, (i.default_value(), Layer::Default))).collect();
        let mut apply = |t: &Table, layer: Layer, warnings: &mut Vec<String>| {
            for i in ITEMS {
                if let Some(v) = lookup(t, i.key) {
                    match i.check(v) {
                        Ok(v) => {
                            values.insert(i.key, (v, layer));
                        }
                        Err(e) => warnings.push(e),
                    }
                }
            }
        };
        // 組み合わせは、使用者が選んだもの(読めなければ auto)
        let chosen = lookup(&user, "profile").and_then(Value::as_str).filter(|p| PROFILES.contains(p)).unwrap_or("auto");
        let profile = if chosen == "auto" { detect_profile(env) } else { chosen };
        if let Some(text) = profile_text(profile) {
            apply(&text.parse().expect("組み合わせは TOML"), Layer::Profile, &mut warnings);
        }
        apply(&user, Layer::User, &mut warnings);
        Config { values, profile: profile.to_string(), warnings, unreadable }
    }

    pub fn get(&self, key: &str) -> &Value {
        &self.values.get(key).unwrap_or_else(|| panic!("台帳に無い設定: {key}")).0
    }

    pub fn origin(&self, key: &str) -> Layer {
        self.values.get(key).map_or(Layer::Default, |v| v.1)
    }

    pub fn str(&self, key: &str) -> &str {
        self.get(key).as_str().unwrap_or("")
    }

    pub fn bool(&self, key: &str) -> bool {
        self.get(key).as_bool().unwrap_or(false)
    }

    pub fn int(&self, key: &str) -> i64 {
        self.get(key).as_integer().unwrap_or(0)
    }

    pub fn list(&self, key: &str) -> Vec<String> {
        self.get(key).as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default()
    }
}

/// 既定の設定ファイル(説明のコメントつき)。`prelude config defaults`
pub fn defaults_toml() -> String {
    let mut out = format!("# PRELUDE の設定の既定(prelude config defaults)。~/.config/prelude/settings.toml には、変えたい項目だけを書けばよい\nversion = {VERSION}\n");
    let mut section = "";
    for i in ITEMS {
        let (sec, name) = i.key.rsplit_once('.').unwrap_or(("", i.key));
        if sec != section {
            out.push_str(&format!("\n[{sec}]\n"));
            section = sec;
        } else {
            out.push('\n');
        }
        let note = match (i.apply, i.planned) {
            (_, true) => " (まだ効かない)",
            (Apply::Restart, _) => " (起動し直すと効く)",
            _ => "",
        };
        out.push_str(&format!("# {}{note}\n{name} = {}\n", i.doc, i.default));
    }
    out
}

/// 使用者の設定のファイルに 1 項目を書く(コメントと知らない項目は残す)。value が None なら消す
pub fn write_user(path: &Path, key: &str, value: Option<&Value>) -> Result<(), String> {
    let item = find(key).ok_or_else(|| format!("Unknown setting: {key} (see `prelude config list`)"))?;
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| format!("{} cannot be read ({e}); fix it first", path.display()))?;
    if !doc.contains_key("version") {
        doc.insert("version", toml_edit::value(VERSION));
    }
    let (sec, name) = item.key.rsplit_once('.').unwrap_or(("", item.key));
    let table = if sec.is_empty() {
        doc.as_table_mut()
    } else {
        let entry = doc.entry(sec).or_insert(toml_edit::table());
        entry.as_table_mut().ok_or_else(|| format!("{sec} in {} is not a table", path.display()))?
    };
    match value {
        Some(v) => {
            let mut lit: toml_edit::Value = v.to_string().parse().map_err(|e| format!("{e}"))?;
            // 既にある項目は値だけを置き換え、項目の前のコメントと値の周りの書き方を残す
            match table.get_mut(name).and_then(toml_edit::Item::as_value_mut) {
                Some(old) => {
                    *lit.decor_mut() = old.decor().clone();
                    *old = lit;
                }
                None => {
                    table.insert(name, toml_edit::Item::Value(lit));
                }
            }
        }
        None => {
            table.remove(name);
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, doc.to_string()).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn 台帳の既定はどれも型に合う() {
        for i in ITEMS {
            assert_eq!(i.check(&i.default_value()), Ok(i.default_value()), "{}", i.key);
        }
        // 既定の設定ファイルを読むと、全部が既定(使用者の層)になる
        let c = Config::from_text(&defaults_toml(), &none);
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        for i in ITEMS {
            assert_eq!(c.get(i.key), &i.default_value(), "{}", i.key);
        }
    }

    #[test]
    fn 層を重ねる_読めない値は既定() {
        let env = |k: &str| (k == "SWAYSOCK").then(|| "/run/sway".to_string());
        let user = "version = 1\nshow_hidden = true\n[tabs]\nclosed_max = 999\n[windows]\nbackend = \"none\"\n";
        let c = Config::from_text(user, &env);
        assert_eq!(c.profile, "sway");
        assert_eq!((c.bool("show_hidden"), c.origin("show_hidden")), (true, Layer::User));
        assert_eq!((c.str("windows.level"), c.origin("windows.level")), ("follow", Layer::Profile)); // sway の組み合わせ
        assert_eq!((c.str("windows.backend"), c.origin("windows.backend")), ("none", Layer::User));
        assert_eq!((c.int("tabs.closed_max"), c.origin("tabs.closed_max")), (20, Layer::Default)); // 範囲の外は既定
        assert_eq!(c.warnings.len(), 1);
        // 組み合わせを選ぶ・使わない
        assert_eq!(Config::from_text("profile = \"kde\"\n", &env).str("terminal"), "konsole");
        let c = Config::from_text("profile = \"none\"\n", &env);
        assert_eq!((c.profile.as_str(), c.origin("windows.level")), ("none", Layer::Default));
        // 読めないファイル
        let c = Config::from_text("これは [ TOML ではない", &none);
        assert_eq!((c.profile.as_str(), c.warnings.len(), c.unreadable), ("none", 1, true));
        assert!(!Config::from_text("show_hidden = 3\n", &none).unreadable); // 項目が読めないだけなら、ほかは使う
    }

    #[test]
    fn 打った値を読む() {
        let level = find("windows.level").unwrap();
        assert_eq!(level.parse_input("arrange"), Ok(Value::String("arrange".into())));
        assert!(level.parse_input("everything").is_err());
        assert_eq!(find("tabs.closed_max").unwrap().parse_input("30"), Ok(Value::Integer(30)));
        assert!(find("tabs.closed_max").unwrap().parse_input("many").is_err());
        assert_eq!(find("show_hidden").unwrap().parse_input("true"), Ok(Value::Boolean(true)));
        let list = find("windows.ignore").unwrap();
        assert_eq!(list.parse_input("steam, gimp").unwrap().as_array().unwrap().len(), 2);
        assert_eq!(list.parse_input("[\"a\"]").unwrap().as_array().unwrap().len(), 1);
        assert!(find("search_engine").unwrap().parse_input("https://x/").is_err());
    }

    #[test]
    fn 書くときにコメントと知らない項目を残す() {
        let d = std::env::temp_dir().join(format!("prelude-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let p = d.join("settings.toml");
        write_user(&p, "show_hidden", Some(&Value::Boolean(true))).unwrap();
        let t = std::fs::read_to_string(&p).unwrap();
        assert!(t.contains("version = 1") && t.contains("show_hidden = true"), "{t}");
        std::fs::write(&p, "# 自分のメモ\nversion = 1\nmine = \"残す\"\n\n[windows]\n# 窓の決まり\nlevel = \"follow\"\n").unwrap();
        write_user(&p, "windows.level", Some(&Value::String("arrange".into()))).unwrap();
        write_user(&p, "tabs.closed_max", Some(&Value::Integer(5))).unwrap();
        let t = std::fs::read_to_string(&p).unwrap();
        assert!(t.contains("# 自分のメモ") && t.contains("mine = \"残す\"") && t.contains("# 窓の決まり"), "{t}");
        let c = Config::from_text(&t, &none);
        assert_eq!((c.str("windows.level"), c.int("tabs.closed_max")), ("arrange", 5));
        write_user(&p, "windows.level", None).unwrap();
        assert_eq!(Config::from_text(&std::fs::read_to_string(&p).unwrap(), &none).origin("windows.level"), Layer::Default);
        assert!(write_user(&p, "nope", None).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 組み合わせを環境から決める() {
        let d = std::env::temp_dir().join(format!("prelude-profile-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("omarchy"), "").unwrap();
        let path = d.to_string_lossy().into_owned();
        let env = |pairs: Vec<(&'static str, String)>| move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone());
        assert_eq!(detect_profile(&env(vec![("HYPRLAND_INSTANCE_SIGNATURE", "x".into()), ("PATH", path.clone())])), "omarchy");
        assert_eq!(detect_profile(&env(vec![("HYPRLAND_INSTANCE_SIGNATURE", "x".into())])), "none");
        assert_eq!(detect_profile(&env(vec![("XDG_CURRENT_DESKTOP", "KDE".into())])), "kde");
        let _ = std::fs::remove_dir_all(&d);
    }
}

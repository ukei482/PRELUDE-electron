//! ペインのレシピ(docs/redesign-rust.md の「レシピ」)。窓を取り込むペインの種類を、設定ファイルだけで増やす。
//!
//! ```toml
//! # ~/.config/prelude/panes/console.toml
//! name = "コンソール"
//! run  = "konsole --workdir {paths.terminal}"   # {paths.…} は今のブックマークの paths に置き換わる(引用符つき)
//! class = "org.kde.konsole"                     # 省略できる。窓を見分ける名前(起動したプロセスで見つからないとき)
//! ```
//! - `.prelude` には `type = "console"` のように種類の名前だけを書く。コマンドは自分の設定にしか無いので、人からもらったファイルが勝手なコマンドを実行できない
//! - 既定で `console`・`cli`・`files`・`settings` と、AI のペイン `agent`(端末の中で `prelude agent`)を持つ。同じ名前のファイルを置くと置き換わる。ペインに `cwd` があれば `{paths.terminal}` の代わりにそれを使う(ファイラの「ここで端末を開く」)
//! - 端末を開くレシピは `prelude term`(設定の端末を、端末の表のとおりに起動する)を使い、窓の名前を `@terminal` にする
//! - **ファイラ**(docs/redesign-rust.md の「ファイラ」): `files` のペインもただのレシピ。既定は端末の中の yazi で、
//!   `files.toml` を置けば Dolphin などに替えられる。`{path}` はペインの `path`(無ければ paths の `files`)、`{pane}` はペインの id。
//!   `field`(やめたフィールド表示)は `files` の別名として読む
//!
//!   ```toml
//!   # ~/.config/prelude/panes/files.toml
//!   run   = "dolphin --new-window {path}"
//!   class = "org.kde.dolphin"
//!   ```
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Recipe {
    #[serde(skip)]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    pub run: String,
    #[serde(default)]
    pub class: String,
}

/// レシピの窓の名前に書くと、設定の端末(terminal)の窓の名前になる(端末の表 `prelude_format::term`)
pub const TERMINAL_CLASS: &str = "@terminal";

/// コアが特別に扱う種類(レシピの名前にできない)
const RESERVED: [&str; 4] = ["web", "app", "home", "split"];

pub fn dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    let conf = std::env::var("XDG_CONFIG_HOME").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| format!("{home}/.config"));
    Path::new(&conf).join("prelude/panes")
}

fn builtin() -> Vec<Recipe> {
    let r = |kind: &str, name: &str, run: &str, class: &str| Recipe {
        kind: kind.into(),
        name: name.into(),
        run: run.into(),
        class: class.into(),
    };
    vec![
        r("agent", "AI", "prelude term --cwd {paths.terminal} -- prelude agent", TERMINAL_CLASS),
        r("cli", "CLI", "prelude term --cwd {paths.terminal} -- prelude shell", TERMINAL_CLASS),
        r("console", "Console", "prelude term --cwd {paths.terminal}", TERMINAL_CLASS),
        r("settings", "Settings", "prelude-settings", "prelude-settings"),
        r("files", "Files", "prelude term --cwd {path} -- yazi {path}", TERMINAL_CLASS),
    ]
}

/// 古い種類の名前を今の名前にする(`field` は `files` の別名)
pub fn alias(kind: &str) -> &str {
    if kind == "field" {
        "files"
    } else {
        kind
    }
}

/// 種類のレシピ(別名も見る)
pub fn find<'a>(list: &'a [Recipe], kind: &str) -> Option<&'a Recipe> {
    let kind = alias(kind);
    list.iter().find(|r| r.kind == kind)
}

/// run がペインの `path` を使うか(`{path}` を含むか)。使うペインには、ブックマークを開くときに paths の `files` を入れておく
pub fn uses_path(r: &Recipe) -> bool {
    r.run.contains("{path}")
}

/// レシピの一覧(名前順)。読めないファイルは飛ばす
pub fn load(dir: &Path) -> Vec<Recipe> {
    let mut m: BTreeMap<String, Recipe> = builtin().into_iter().map(|r| (r.kind.clone(), r)).collect();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            let Some(kind) = p.file_stem().and_then(|s| s.to_str()).filter(|_| p.extension().is_some_and(|x| x == "toml")) else {
                continue;
            };
            if RESERVED.contains(&kind) || kind.starts_with('.') {
                continue;
            }
            let Some(mut r) = std::fs::read_to_string(&p).ok().and_then(|t| toml::from_str::<Recipe>(&t).ok()) else {
                eprintln!("prelude-core: cannot read recipe {}", p.display());
                continue;
            };
            r.kind = kind.to_string();
            if r.name.is_empty() {
                r.name = kind.to_string();
            }
            m.insert(r.kind.clone(), r);
        }
    }
    m.into_values().collect()
}

/// シェルの引用符でくくる
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// run の {paths.…} を置き換えたコマンド。paths は項目 → 使うフォルダ
pub fn command(r: &Recipe, paths: &dyn Fn(&str) -> String) -> String {
    expand(&r.run, &|name| name.strip_prefix("paths.").filter(|k| !k.is_empty()).map(paths))
}

/// run の `{名前}` を置き換える(引用符つき)。vars が None を返す名前・形の合わないものはそのまま残す。
/// 名前: `paths.<項目>`(使うフォルダ)、`path`(ペインのフォルダ)、`pane`(ペインの id)
pub fn expand(run: &str, vars: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = run;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let value = after
            .find('}')
            .filter(|&j| j > 0 && after[..j].chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.'))
            .and_then(|j| vars(&after[..j]).map(|v| (j, v)));
        match value {
            Some((j, v)) => {
                out.push_str(&quote(&v));
                rest = &after[j + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_を置き換える() {
        let r = Recipe {
            kind: "x".into(),
            name: String::new(),
            run: "konsole --workdir {paths.terminal} {paths.} {x}".into(),
            class: String::new(),
        };
        assert_eq!(command(&r, &|k| format!("/w/{k}'s")), r"konsole --workdir '/w/terminal'\''s' {paths.} {x}");
    }

    #[test]
    fn path_と_pane_を置き換える() {
        let vars = |n: &str| match n {
            "path" => Some("/a b".to_string()),
            "pane" => Some("7".to_string()),
            _ => None,
        };
        assert_eq!(expand("prelude-filer --pane {pane} {path} {nope} {", &vars), "prelude-filer --pane '7' '/a b' {nope} {");
    }

    #[test]
    fn ファイルで足す_置き換える() {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp-recipes");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("console.toml"), "run = \"xterm\"\n").unwrap();
        std::fs::write(d.join("notes.toml"), "name = \"メモ\"\nrun = \"kate\"\nclass = \"kate\"\n").unwrap();
        std::fs::write(d.join("web.toml"), "run = \"evil\"\n").unwrap(); // 決まった種類は置き換えられない
        std::fs::write(d.join("bad.toml"), "run = = 1").unwrap();
        let list = load(&d);
        let v: Vec<(&str, &str, &str)> = list.iter().map(|r| (r.kind.as_str(), r.name.as_str(), r.run.as_str())).collect();
        // AI のペイン: 端末を今のブックマークの作業フォルダで開いて、既定の AI を起動する
        assert_eq!((v[0].0, v[0].1), ("agent", "AI"));
        assert!(v[0].2.contains("{paths.terminal}") && v[0].2.ends_with("prelude agent"));
        assert_eq!(v[1].0, "cli");
        assert_eq!(v[2], ("console", "console", "xterm"));
        assert_eq!(v[4], ("notes", "メモ", "kate"));
        assert_eq!(v[5].0, "settings");
        // ファイラは既定(yazi)を持ち、置き換えられる。field は files の別名
        assert_eq!(v[3].0, "files");
        assert!(v[3].2.contains("yazi {path}"));
        assert!(uses_path(&list[3]) && !uses_path(&list[2]));
        assert_eq!(find(&list, "field").map(|r| r.kind.as_str()), Some("files"));
        assert!(find(&list, "nope").is_none());
        std::fs::write(d.join("files.toml"), "run = \"dolphin {path}\"\n").unwrap();
        assert!(load(&d).iter().any(|r| r.kind == "files" && r.run == "dolphin {path}"));
    }
}

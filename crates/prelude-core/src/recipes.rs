//! ペインのレシピ(判断 7 の段階 1)。窓を取り込むペインの種類を、設定ファイルだけで増やす。
//!
//! ```toml
//! # ~/.config/prelude/panes/console.toml
//! name = "コンソール"
//! run  = "konsole --workdir {paths.terminal}"   # {paths.…} は今のブックマークの paths に置き換わる(引用符つき)
//! class = "org.kde.konsole"                     # 省略できる。窓を見分ける名前(起動したプロセスで見つからないとき)
//! ```
//! - `.prelude` には `type = "console"` のように種類の名前だけを書く。コマンドは自分の設定にしか無いので、人からもらったファイルが勝手なコマンドを実行できない
//! - 既定で `console` を持つ。同じ名前のファイルを置くと置き換わる
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

/// UI が描く・コアが特別に扱う種類(レシピの名前にできない)
const RESERVED: [&str; 7] = ["web", "files", "field", "app", "home", "settings", "split"];

pub fn dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    let conf = std::env::var("XDG_CONFIG_HOME").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| format!("{home}/.config"));
    Path::new(&conf).join("prelude/panes")
}

fn builtin() -> Vec<Recipe> {
    vec![Recipe {
        kind: "console".into(),
        name: "Console".into(),
        run: "konsole --workdir {paths.terminal}".into(),
        class: "org.kde.konsole".into(),
    }]
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
    let mut out = String::new();
    let mut rest = r.run.as_str();
    while let Some(i) = rest.find("{paths.") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 7..];
        match after.find('}') {
            Some(j) if j > 0 && after[..j].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {
                out.push_str(&quote(&paths(&after[..j])));
                rest = &after[j + 1..];
            }
            _ => {
                out.push_str("{paths.");
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
        assert_eq!(v, vec![("console", "console", "xterm"), ("notes", "メモ", "kate")]);
    }
}

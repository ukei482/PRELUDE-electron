//! サイドバーの決まり(ui/prelude.slint の `global Prelude`)を、実行時に確かめるための一覧。
//! 組み込む UI はビルドのときに slint のコンパイラが確かめるので、これを使うのは読み込む UI(--load)とテストだけ

/// 決まりの global の名前
pub const GLOBAL: &str = "Prelude";

/// 持っていなければならないプロパティ(名前, 型の名前)。型の名前は slint-interpreter の `ValueType` を小文字にしたもの
pub const PROPERTIES: [(&str, &str); 7] = [
    ("online", "bool"),
    ("status", "string"),
    ("space", "string"),
    ("spaces", "model"),
    ("bookmarks", "model"),
    ("tabs", "model"),
    ("compact", "bool"),
];

/// 持っていなければならないコールバック
pub const CALLBACKS: [&str; 6] = ["switch-space", "open-bookmark", "activate-tab", "close-tab", "open-launcher", "toggle-compact"];

/// slint は `_` と `-` を同じに扱うので、そろえて比べる
pub fn norm(s: &str) -> String {
    s.replace('_', "-")
}

/// 足りないもの・型の合わないものの一覧(空なら決まりを満たす)。
/// `props` は global のプロパティ(名前, 型の名前)、`callbacks` は global のコールバックの名前
pub fn missing(props: &[(String, String)], callbacks: &[String]) -> Vec<String> {
    let mut out = vec![];
    for (name, ty) in PROPERTIES {
        match props.iter().find(|(n, _)| norm(n) == name) {
            None => out.push(format!("property `{name}` が無い")),
            Some((_, t)) if t != ty => out.push(format!("property `{name}` の型が {t}({ty} のはず)")),
            _ => {}
        }
    }
    for name in CALLBACKS {
        if !callbacks.iter().any(|c| norm(c) == name) {
            out.push(format!("callback `{name}` が無い"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 足りないものを挙げる() {
        let props = vec![("online".into(), "bool".into()), ("tabs".into(), "string".into())];
        let m = missing(&props, &["activate_tab".into()]);
        assert!(m.iter().any(|s| s.contains("`status` が無い")));
        assert!(m.iter().any(|s| s.contains("`tabs` の型が string")));
        assert!(!m.iter().any(|s| s.contains("activate-tab")));
        assert!(m.iter().any(|s| s.contains("`close-tab` が無い")));
    }
}

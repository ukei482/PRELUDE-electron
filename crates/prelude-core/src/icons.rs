//! アプリのアイコン(.desktop の Icon)のファイルを探す。アイコンのテーマ(hicolor・breeze・Adwaita)と pixmaps から、
//! 大きいもの・SVG を先に探す。見つけた結果は覚えておく
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const THEMES: [&str; 3] = ["hicolor", "breeze", "Adwaita"];
const SIZES: [&str; 7] = ["scalable", "256x256", "128x128", "96x96", "64x64", "48x48", "32x32"];
const EXTS: [&str; 2] = ["svg", "png"];

/// アイコンを探すフォルダ(XDG のデータのフォルダと flatpak)
fn bases() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut v = vec![crate::xdg::data_home()];
    v.extend(crate::xdg::data_dirs());
    v.push("/var/lib/flatpak/exports/share".into());
    v.push(Path::new(&home).join(".local/share/flatpak/exports/share"));
    v
}

/// name のアイコンのファイル。name が絶対パスならそれ(あれば)
pub fn find_in(name: &str, bases: &[PathBuf]) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    let p = Path::new(name);
    if p.is_absolute() {
        return p.is_file().then(|| p.to_path_buf());
    }
    for base in bases {
        for theme in THEMES {
            for size in SIZES {
                for ext in EXTS {
                    let f = base.join("icons").join(theme).join(size).join("apps").join(format!("{name}.{ext}"));
                    if f.is_file() {
                        return Some(f);
                    }
                }
            }
        }
    }
    for base in bases {
        for ext in ["svg", "png", "xpm"] {
            let f = base.join("pixmaps").join(format!("{name}.{ext}"));
            if f.is_file() {
                return Some(f);
            }
        }
    }
    None
}

/// name のアイコンのファイル(覚えておく)
pub fn find(name: &str) -> Option<PathBuf> {
    static CACHE: Mutex<Option<HashMap<String, Option<PathBuf>>>> = Mutex::new(None);
    let mut c = CACHE.lock().unwrap();
    let m = c.get_or_insert_with(HashMap::new);
    if let Some(v) = m.get(name) {
        return v.clone();
    }
    let v = find_in(name, &bases());
    m.insert(name.to_string(), v.clone());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn テーマの大きいものから_pixmaps_も() {
        let d = std::env::temp_dir().join(format!("prelude-icons-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let put = |rel: &str| {
            let f = d.join(rel);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(&f, "x").unwrap();
            f
        };
        put("icons/hicolor/48x48/apps/kate.png");
        let big = put("icons/hicolor/scalable/apps/kate.svg");
        let pix = put("pixmaps/old.xpm");
        let bases = vec![d.clone()];
        assert_eq!(find_in("kate", &bases), Some(big.clone()));
        assert_eq!(find_in("old", &bases), Some(pix));
        assert_eq!(find_in(&big.to_string_lossy(), &bases), Some(big));
        assert_eq!(find_in("none", &bases), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}

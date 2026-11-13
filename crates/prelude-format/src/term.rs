//! 端末の表(docs/redesign-rust.md の「設定」)。端末ごとに違う、作業フォルダとコマンドの渡し方・新しいプロセスで開く引数・
//! 窓の名前(app_id)を、PRELUDE はここだけで知る。`prelude term` が起動し、コアはレシピの窓の名前 `@terminal` をここから決める。
//!
//! 新しいプロセスで開く引数はいつも付ける。動いている本体に頼んですぐ終わる端末だと、ダイアログの選ぶ画面は選び終わるのを
//! 待てず、窓のつなぎは起動したプロセスから窓を見分けられないため
use std::path::Path;

/// 端末 1 つ
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Terminal {
    /// コマンドの名前
    pub name: &'static str,
    /// 窓の名前(Wayland の app_id・X11 のクラス)
    pub class: &'static str,
    /// 新しいプロセスで開く引数
    new_process: &'static [&'static str],
    /// 作業フォルダの渡し方(`{}` がフォルダ。1 語に入れるもの・別の語にするもの)
    cwd: &'static [&'static str],
    /// 動かすコマンドの前に置く語(無ければそのまま並べる)
    exec: &'static [&'static str],
}

/// 知っている端末(auto で探す順)
pub const TERMINALS: [Terminal; 6] = [
    Terminal { name: "foot", class: "foot", new_process: &[], cwd: &["--working-directory={}"], exec: &[] },
    Terminal { name: "kitty", class: "kitty", new_process: &[], cwd: &["--directory", "{}"], exec: &[] },
    Terminal {
        name: "ghostty",
        class: "com.mitchellh.ghostty",
        new_process: &["--gtk-single-instance=false"],
        cwd: &["--working-directory={}"],
        exec: &["-e"],
    },
    Terminal { name: "alacritty", class: "Alacritty", new_process: &[], cwd: &["--working-directory", "{}"], exec: &["-e"] },
    Terminal {
        name: "wezterm",
        class: "org.wezfurlong.wezterm",
        new_process: &["start", "--always-new-process"],
        cwd: &["--cwd", "{}"],
        exec: &["--"],
    },
    Terminal { name: "konsole", class: "org.kde.konsole", new_process: &["--separate"], cwd: &["--workdir", "{}"], exec: &["-e"] },
];

/// 使う端末: 設定の値(auto か、名前・パス)と PATH から。知らない端末の名前なら、`-e` で渡す端末とみなす
#[derive(Debug, Clone, PartialEq)]
pub struct Chosen {
    /// 起動するもの(名前かパス)
    pub program: String,
    pub terminal: Option<Terminal>,
}

fn in_path(name: &str, path: &str) -> bool {
    path.split(':').any(|d| !d.is_empty() && Path::new(d).join(name).is_file())
}

/// 設定の terminal と PATH から決める。auto で見つからなければ None
pub fn choose(setting: &str, path: &str) -> Option<Chosen> {
    let setting = setting.trim();
    if setting.is_empty() || setting == "auto" {
        return TERMINALS.iter().find(|t| in_path(t.name, path)).map(|t| Chosen { program: t.name.to_string(), terminal: Some(*t) });
    }
    let base = Path::new(setting).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Some(Chosen { program: setting.to_string(), terminal: TERMINALS.iter().find(|t| t.name == base).copied() })
}

impl Chosen {
    /// 窓の名前(知らない端末なら名前そのもの)
    pub fn class(&self) -> String {
        match self.terminal {
            Some(t) => t.class.to_string(),
            None => Path::new(&self.program).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        }
    }

    /// 起動する語の並び。cwd が空なら作業フォルダは渡さない(起動したフォルダのまま)。cmd が空ならシェル
    pub fn argv(&self, cwd: &str, cmd: &[String]) -> Vec<String> {
        let mut out = vec![self.program.clone()];
        let (new_process, cwd_args, exec): (&[&str], &[&str], &[&str]) = match &self.terminal {
            Some(t) => (t.new_process, t.cwd, t.exec),
            None => (&[], &[], &["-e"]),
        };
        out.extend(new_process.iter().map(|s| s.to_string()));
        if !cwd.is_empty() {
            out.extend(cwd_args.iter().map(|a| a.replace("{}", cwd)));
        }
        if !cmd.is_empty() {
            out.extend(exec.iter().map(|s| s.to_string()));
            out.extend(cmd.iter().cloned());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn 端末ごとの渡し方() {
        let cmd = s(&["yazi", "/w"]);
        let argv = |name: &str| choose(name, "").unwrap().argv("/w", &cmd);
        assert_eq!(argv("foot"), s(&["foot", "--working-directory=/w", "yazi", "/w"]));
        assert_eq!(argv("kitty"), s(&["kitty", "--directory", "/w", "yazi", "/w"]));
        assert_eq!(argv("ghostty"), s(&["ghostty", "--gtk-single-instance=false", "--working-directory=/w", "-e", "yazi", "/w"]));
        assert_eq!(argv("alacritty"), s(&["alacritty", "--working-directory", "/w", "-e", "yazi", "/w"]));
        assert_eq!(argv("wezterm"), s(&["wezterm", "start", "--always-new-process", "--cwd", "/w", "--", "yazi", "/w"]));
        assert_eq!(argv("/usr/bin/konsole"), s(&["/usr/bin/konsole", "--separate", "--workdir", "/w", "-e", "yazi", "/w"]));
        // シェルだけ・作業フォルダ無し
        assert_eq!(choose("foot", "").unwrap().argv("", &[]), s(&["foot"]));
        // 知らない端末は -e で渡す
        let x = choose("xterm", "").unwrap();
        assert_eq!((x.argv("/w", &cmd), x.class()), (s(&["xterm", "-e", "yazi", "/w"]), "xterm".to_string()));
        assert_eq!(choose("konsole", "").unwrap().class(), "org.kde.konsole");
    }

    #[test]
    fn auto_は表の順に_path_から探す() {
        let d = std::env::temp_dir().join(format!("prelude-term-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("konsole"), "").unwrap();
        std::fs::write(d.join("kitty"), "").unwrap();
        let path = d.to_string_lossy().into_owned();
        assert_eq!(choose("auto", &path).unwrap().program, "kitty");
        assert_eq!(choose("auto", ""), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}

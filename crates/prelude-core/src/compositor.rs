//! 窓を並べる相手(コンポジタ)の差し替え口(docs/redesign-rust.md の「窓のつなぎ」)。
//!
//! コアは「どのペインに何の窓を出すか」と「タブごとの配置の木」([`Scene`])を渡し、窓が無くなった・題名が変わったなどを [`Note`] で受けるだけ。
//! 裏側が KWin(窓をペインの場所に重ねる。今の [`crate::embed`])でも、sway・Hyprland(コンポジタに並べさせる)でも、コアは変わらない。
//! どの裏側を使うかは [`detect`] が環境から決める。
use prelude_format::Dir as FDir;
use prelude_proto::PaneRect;
use std::future::Future;
use std::pin::Pin;
use tokio::sync::mpsc;

/// 窓を出すペイン(コアの状態から作る)
#[derive(Debug, Clone, PartialEq)]
pub struct Want {
    pub pane: u64,
    /// シェルのコマンド
    pub cmd: String,
    /// 窓を見分ける名前(省略できる)
    pub cls: String,
    /// 作業フォルダ
    pub cwd: String,
    /// 見えていれば場所(UI が知らせたもの。窓を重ねる裏側が使う)
    pub rect: Option<PaneRect>,
    /// 窓の題名の印(Web のペイン)。あれば、起動したプロセスでなく、この題名の窓を取り込む。プロセスは追わない
    /// (ブラウザは窓が増えても同じプロセスのまま。docs/redesign-rust.md の「Web のペイン」)
    pub marker: String,
    /// 既にある窓(PRELUDE を通さずに開いた窓。[`Note::Foreign`] の con)。あれば起動はせず、この窓をペインにする
    pub window: Option<i64>,
}

/// タブの配置の木(ペインの id だけ)。コンポジタに並べさせる裏側が使う
#[derive(Debug, Clone, PartialEq)]
pub enum Tree {
    Split { dir: FDir, ratio: f64, a: Box<Tree>, b: Box<Tree> },
    Pane(u64),
}

/// タブ 1 つ(= コンポジタの作業領域 1 つ。「タブ」)
#[derive(Debug, Clone, PartialEq)]
pub struct TabScene {
    pub id: u64,
    pub num: u32,
    pub tree: Tree,
    pub active_pane: u64,
}

/// コンポジタに渡す、今のスペースの全部
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    /// 窓を出すペイン(止まったもの・コマンドの無いものは入らない)
    pub panes: Vec<Want>,
    pub tabs: Vec<TabScene>,
    /// 今のタブの id
    pub active: Option<u64>,
}

/// コンポジタからコアへの知らせ
#[derive(Debug, Clone, PartialEq)]
pub enum Note {
    /// ペインの窓が無くなった(had_window: 一度は窓があった)
    Exited { pane: u64, had_window: bool },
    /// ペインの本体の窓の題名
    Title { pane: u64, title: String },
    /// ペインの窓を取り込んだ
    Claimed { pane: u64 },
    /// 使用者がコンポジタでタブ(作業領域)・ペイン(窓)を選んだ。pane が無ければタブだけ
    Focus { tab: u64, pane: Option<u64> },
    /// PRELUDE を通さずに開いた普通の窓がタブに出てきた。tree はそのタブの今の並びで、この窓はペイン 0(「PRELUDE を通さずに開いた窓」)
    Foreign { tab: u64, con: i64, app: String, title: String, focused: bool, tree: Tree },
    /// 使用者がコンポジタで窓を動かした。タブの番号と、そのタブの今の並び(窓のあるペインだけ)。無ければ窓が無くなった。
    /// 知らない番号はコンポジタで作った作業領域なので、新しいタブにする
    Layout(Vec<(u32, Option<Tree>)>),
}

/// 窓のつなぎができること(docs/redesign-rust.md の「窓のつなぎ」の段)。上の段は下の段のこともできる
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// 0. アプリを起動するだけ。置き場所は環境に任せる
    Launch = 0,
    /// 1. 窓をタブの作業領域へ移すだけ。並べ方・大きさは環境に任せる(「作業領域だけ指定する形」)
    Workspace = 1,
    /// 2. ブックマークを開くときに `[layout]` のとおりに並べる。分割で開けるのはここから
    Arrange = 2,
    /// 3. 今の並びを読む(使用者が動かした・選んだものをコアに合わせる)
    Read = 3,
    /// 4. PRELUDE を通さずに開いた窓も追いかけて、タブのペインにする
    Follow = 4,
}

impl Level {
    /// 設定の windows.level の言葉。auto は None(つなぎの最大)
    pub fn parse(s: &str) -> Option<Level> {
        match s {
            "launch" => Some(Level::Launch),
            "workspace" => Some(Level::Workspace),
            "arrange" => Some(Level::Arrange),
            "read" => Some(Level::Read),
            "follow" => Some(Level::Follow),
            _ => None,
        }
    }
}

/// 設定の [windows](docs/redesign-rust.md の「設定」)。起動のときに読む
#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    /// auto・sway・kwin・hyprland・none
    pub backend: String,
    /// 任せる段の上限(None はつなぎの最大)
    pub cap: Option<Level>,
    /// PRELUDE を通さずに開いた窓を引き取る
    pub adopt: bool,
    /// いつも浮かせる窓・触らない窓(app_id・クラス。大文字小文字は見ない)
    pub floating: Vec<String>,
    pub ignore: Vec<String>,
    /// タブの作業領域の名前の頭
    pub workspace_prefix: String,
    /// 窓と窓の間・窓と画面の端の間(px。None は環境のまま)。サイドバーも同じ決まりで並べる
    pub gaps_inner: Option<u32>,
    pub gaps_outer: Option<u32>,
    /// 窓の角丸(px。None は環境のまま)。サイドバーも同じにする
    pub corner_radius: Option<u32>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            backend: "auto".into(),
            cap: None,
            adopt: true,
            floating: vec![],
            ignore: vec![],
            workspace_prefix: String::new(),
            gaps_inner: None,
            gaps_outer: None,
            corner_radius: None,
        }
    }
}

impl Options {
    pub fn from_config(c: &prelude_format::settings::Config) -> Options {
        Options {
            backend: c.str("windows.backend").to_string(),
            cap: Level::parse(c.str("windows.level")),
            adopt: c.str("windows.adopt") != "none",
            floating: c.list("windows.floating"),
            ignore: c.list("windows.ignore"),
            workspace_prefix: c.str("windows.workspace_prefix").to_string(),
            gaps_inner: c.get("windows.gaps_inner").as_integer().map(|n| n as u32),
            gaps_outer: c.get("windows.gaps_outer").as_integer().map(|n| n as u32),
            corner_radius: c.get("windows.corner_radius").as_integer().map(|n| n as u32),
        }
    }

    /// つなぎの最大の段と上限から、使う段
    pub fn level(&self, max: Level) -> Level {
        self.cap.map_or(max, |c| c.min(max))
    }

    pub fn floats(&self, app: &str) -> bool {
        self.floating.iter().any(|a| a.eq_ignore_ascii_case(app))
    }

    pub fn ignores(&self, app: &str) -> bool {
        self.ignore.iter().any(|a| a.eq_ignore_ascii_case(app))
    }

    /// 使うつなぎ。環境変数 PRELUDE_COMPOSITOR(試すため)→ 設定の backend → 動いているコンポジタ。
    /// 上限が launch なら、どのつなぎでもつなぎ無し
    pub fn choose(&self, get: &dyn Fn(&str) -> Option<String>) -> Option<Kind> {
        if self.cap == Some(Level::Launch) {
            return None;
        }
        if get("PRELUDE_COMPOSITOR").is_some() {
            return detect_from(get);
        }
        match self.backend.as_str() {
            "none" => None,
            "sway" => Some(Kind::Sway),
            "kwin" => Some(Kind::Kwin),
            "hyprland" => Some(Kind::Hyprland),
            _ => detect_from(get),
        }
    }
}

/// PRELUDE の UI の窓(Wayland の app_id・X11 のクラス)。ペインではないので、窓のつなぎは引き取らない
pub const UI_APPS: [&str; 3] = ["prelude-sidebar", "prelude-launcher", "prelude-slint-proto"];

/// PRELUDE の UI の窓か
pub fn is_prelude_ui(app: &str) -> bool {
    UI_APPS.iter().any(|a| app.eq_ignore_ascii_case(a))
}

/// コンポジタの裏側(窓のつなぎ)。部品のトレイトなので版は付けない(「決まりの版」)
pub trait Compositor: Send + Sync {
    /// 裏側の名前(ログ用。状態の `windows` にも出す)
    fn name(&self) -> &'static str;
    /// できること(段)
    fn level(&self) -> Level;
    /// 今の状態に合わせる(起動する・閉じる・並べ直す)。何度呼んでもよい
    fn arrange(&self, scene: Scene);
    /// 起動したアプリを閉じて後片付けをする。終わるまで待つ
    fn shutdown(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

/// 裏側の種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// KDE Plasma 6 の Wayland。KWin スクリプトで窓をペインの場所に重ねる(KDE で使う人のため。「窓のつなぎ」)
    Kwin,
    /// sway・SwayFX(i3 の IPC)
    Sway,
    /// Hyprland(+ hy3)
    Hyprland,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Kwin => "kwin",
            Kind::Sway => "sway",
            Kind::Hyprland => "hyprland",
        }
    }
}

/// 環境から裏側を決める。`PRELUDE_COMPOSITOR`(kwin・sway・hyprland)があればそれ、無ければ動いているコンポジタ。分からなければ None
pub fn detect() -> Option<Kind> {
    detect_from(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
}

pub fn detect_from(get: impl Fn(&str) -> Option<String>) -> Option<Kind> {
    if let Some(v) = get("PRELUDE_COMPOSITOR") {
        return match v.to_lowercase().as_str() {
            "kwin" | "kde" => Some(Kind::Kwin),
            "sway" | "swayfx" => Some(Kind::Sway),
            "hyprland" => Some(Kind::Hyprland),
            _ => None,
        };
    }
    get("WAYLAND_DISPLAY")?;
    if get("SWAYSOCK").is_some() {
        return Some(Kind::Sway);
    }
    if get("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        return Some(Kind::Hyprland);
    }
    if get("XDG_CURRENT_DESKTOP").is_some_and(|d| d.to_lowercase().contains("kde")) {
        return Some(Kind::Kwin);
    }
    None
}

/// 裏側を始める。tag はこのコアの印(別のソケットで動かすコアと取り合わないように)。opts は設定の [windows]。
/// 対応するコンポジタが無い・始められない・上限が launch なら、つなぎ無し(段 0。[`crate::plain`])にする: アプリは起動し、窓の置き場所は環境に任せる
pub async fn start(tag: &str, notes: mpsc::UnboundedSender<Note>, opts: &Options) -> Box<dyn Compositor> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    let found: Option<Box<dyn Compositor>> = match opts.choose(&env) {
        Some(Kind::Kwin) if opts.level(Level::Arrange) < Level::Arrange => {
            // KWin のつなぎは窓をペインの場所に重ねる形で、作業領域だけを指定することはできない
            eprintln!("prelude-core: the kwin backend cannot keep to windows.level = workspace; apps are only started");
            None
        }
        Some(Kind::Kwin) => crate::embed::start(tag, notes.clone()).await.map(|h| Box::new(h) as Box<dyn Compositor>),
        Some(Kind::Sway) => crate::sway::start(tag, notes.clone(), opts.clone()).await.map(|h| Box::new(h) as Box<dyn Compositor>),
        Some(Kind::Hyprland) => {
            eprintln!("prelude-core: the hyprland backend is not written yet");
            None
        }
        None => None,
    };
    found.unwrap_or_else(|| {
        if opts.cap == Some(Level::Launch) {
            eprintln!("prelude-core: windows.level = launch. Apps are started and the desktop places their windows");
        } else {
            eprintln!("prelude-core: no window backend for this desktop. Apps are started and the desktop places their windows");
        }
        Box::new(crate::plain::start(notes))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn 設定から_使うつなぎと段を決める() {
        let o = |backend: &str, level: &str| Options { backend: backend.into(), cap: Level::parse(level), ..Options::default() };
        let sway = env(&[("WAYLAND_DISPLAY", "w"), ("SWAYSOCK", "/s")]);
        assert_eq!(o("auto", "auto").choose(&sway), Some(Kind::Sway));
        assert_eq!(o("none", "auto").choose(&sway), None);
        assert_eq!(o("auto", "launch").choose(&sway), None); // 上限が launch ならつなぎ無し
        assert_eq!(o("kwin", "auto").choose(&env(&[])), Some(Kind::Kwin));
        // 環境変数は設定より先(試すため)
        assert_eq!(o("none", "auto").choose(&env(&[("PRELUDE_COMPOSITOR", "sway")])), Some(Kind::Sway));
        // 段は上限とつなぎの最大の小さいほう
        assert_eq!(o("auto", "workspace").level(Level::Follow), Level::Workspace);
        assert_eq!(o("auto", "follow").level(Level::Arrange), Level::Arrange);
        assert_eq!(o("auto", "auto").level(Level::Follow), Level::Follow);
        let f = Options { floating: vec!["Gimp".into()], ignore: vec!["steam".into()], ..Options::default() };
        assert!(f.floats("gimp") && !f.floats("kate") && f.ignores("Steam"));
    }

    #[test]
    fn 段と_ui_の窓() {
        assert!(
            Level::Follow > Level::Read
                && Level::Read > Level::Arrange
                && Level::Arrange > Level::Workspace
                && Level::Workspace > Level::Launch
        );
        assert!(is_prelude_ui("prelude-sidebar") && is_prelude_ui("Prelude-Launcher"));
        assert!(!is_prelude_ui("org.kde.konsole") && !is_prelude_ui("prelude-settings"));
    }

    #[test]
    fn 動いているコンポジタから決める() {
        let w = ("WAYLAND_DISPLAY", "wayland-0");
        assert_eq!(detect_from(env(&[w, ("XDG_CURRENT_DESKTOP", "KDE")])), Some(Kind::Kwin));
        assert_eq!(detect_from(env(&[w, ("SWAYSOCK", "/run/user/1000/sway.sock")])), Some(Kind::Sway));
        assert_eq!(detect_from(env(&[w, ("HYPRLAND_INSTANCE_SIGNATURE", "abc")])), Some(Kind::Hyprland));
        // X11 の KDE・何もわからないときは無し
        assert_eq!(detect_from(env(&[("XDG_CURRENT_DESKTOP", "KDE")])), None);
        assert_eq!(detect_from(env(&[w])), None);
    }

    #[test]
    fn 環境変数で決められる() {
        assert_eq!(detect_from(env(&[("PRELUDE_COMPOSITOR", "SwayFX")])), Some(Kind::Sway));
        assert_eq!(detect_from(env(&[("PRELUDE_COMPOSITOR", "kwin"), ("SWAYSOCK", "x")])), Some(Kind::Kwin));
        assert_eq!(detect_from(env(&[("PRELUDE_COMPOSITOR", "niri"), ("XDG_CURRENT_DESKTOP", "KDE")])), None);
    }
}

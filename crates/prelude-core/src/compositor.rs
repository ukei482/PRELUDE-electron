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
    /// 窓の題名の印(Web のペイン)。あれば起動はせず、この題名の窓を取り込む(窓は web.rs が作る)
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

/// コンポジタからコアへの知らせ(Web のペインの知らせも同じ通り道を使う)
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
    /// Web のペインの今の URL と題名(web.rs から)
    Web { pane: u64, url: String, title: String },
    /// Web のペインから新しいタブで開くリンク(web.rs から)
    OpenTab { url: String, from: u64 },
    /// Web のペインのファビコンを保存した(web.rs から)
    Icon { pane: u64, path: String },
    /// ダウンロードの始まり・進み具合(CDP の Browser.downloadWillBegin / downloadProgress の params)
    Download(serde_json::Value),
}

/// 窓のつなぎができること(docs/redesign-rust.md の「窓のつなぎ」の段)。上の段は下の段のこともできる
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// 0. アプリを起動するだけ。置き場所は環境に任せる
    Launch = 0,
    /// 1. ブックマークを開くときに `[layout]` のとおりに並べる
    Arrange = 1,
    /// 2. 今の並びを読む
    Read = 2,
    /// 3. 窓の動き・PRELUDE を通さずに開いた窓を追いかけて、コアの状態に合わせる
    Follow = 3,
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

/// 裏側を始める。tag はこのコアの印(別のソケットで動かすコアと取り合わないように)。
/// 対応するコンポジタが無い・始められなければ、つなぎ無し(段 0。[`crate::plain`])にする: アプリは起動し、窓の置き場所は環境に任せる
pub async fn start(tag: &str, notes: mpsc::UnboundedSender<Note>) -> Box<dyn Compositor> {
    let found: Option<Box<dyn Compositor>> = match detect() {
        Some(Kind::Kwin) => crate::embed::start(tag, notes.clone()).await.map(|h| Box::new(h) as Box<dyn Compositor>),
        Some(Kind::Sway) => crate::sway::start(tag, notes.clone()).await.map(|h| Box::new(h) as Box<dyn Compositor>),
        Some(Kind::Hyprland) => {
            eprintln!("prelude-core: the hyprland backend is not written yet");
            None
        }
        None => None,
    };
    found.unwrap_or_else(|| {
        eprintln!("prelude-core: no window backend for this desktop. Apps are started and the desktop places their windows");
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
    fn 段と_ui_の窓() {
        assert!(Level::Follow > Level::Arrange && Level::Arrange > Level::Launch);
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

//! Slint の試作。PRELUDE の UI(サイドバー・ランチャー・設定)を GPUI の代わりに Slint で作れるかを確かめる。
//!
//! 確かめること
//! - サイドバーの決まり(`ui/prelude.slint` の `global Prelude`): 既定のサイドバーはこれだけを使ってコアとやり取りする。
//!   決まりを破った .slint はビルドで止まる(`cargo test` の tests/contract.rs)。実行時に読み込む .slint は、読み込むときに確かめる(`--load`)
//! - 普通のウィンドウ: サイドバーは layer-shell ではなく普通のウィンドウ(app_id は prelude-slint-proto)。つまみで幅を変えると、ウィンドウの大きさを頼む
//! - 日本語入力(fcitx5)とフォント: Slint の入力欄(LineEdit)で変換しながら打てるか。漢字・かな・記号が出るか
//! - 1 万件の一覧: ListView で見える行だけを描く。読み込みの時間とスクロールの滑らかさ
//! - 描き方: `SLINT_BACKEND=winit-femtovg`(OpenGL。既定)と `winit-software`(CPU)で同じに描けるか
//! - コアとのやり取り: コアが動いていれば、スペース・ブックマーク・タブを出し、押すと切り替わる(別のスレッドから UI を書き換える流れ)
//!
//! 使い方: `prelude-slint-proto [フォルダ]`(フォルダを渡すと一覧は中のファイル名、無ければ作った 1 万件)
//!         `prelude-slint-proto --load <ファイル.slint>`(`--features interpreter` でビルドしたとき。使用者が置いた .slint を読み込む)
#[cfg_attr(not(feature = "interpreter"), allow(dead_code))]
mod contract;
mod core;
#[cfg(feature = "interpreter")]
mod loaded;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::time::{Duration, Instant};

slint::include_modules!();

/// 広い・細い列の幅
const WIDE: f32 = 300.;
const COMPACT: f32 = 44.;

/// 一覧の中身。フォルダが読めればその中の名前(名前順)、無ければ作った 1 万件
fn rows(dir: Option<&str>) -> (Vec<SharedString>, String) {
    let t0 = Instant::now();
    if let Some(d) = dir {
        if let Ok(rd) = std::fs::read_dir(d) {
            let mut v: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
            v.sort();
            let n = v.len();
            return (v.into_iter().map(SharedString::from).collect(), format!("{n} 件({d}、{:.1} ms)", ms(t0)));
        }
        eprintln!("cannot read {d}; using generated rows");
    }
    let v: Vec<SharedString> =
        (1..=10_000).map(|i| SharedString::from(format!("{i:05}  資料_{}.txt", ["議事録", "論文", "写真", "見積"][i % 4]))).collect();
    let info = format!("{} 件(作ったもの、{:.1} ms)", v.len(), ms(t0));
    (v, info)
}

fn ms(t0: Instant) -> f64 {
    t0.elapsed().as_secs_f64() * 1000.
}

/// 使っているメモリ(/proc/self/status の VmRSS)
fn rss() -> String {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with("VmRSS:")).map(|l| l["VmRSS:".len()..].trim().to_string()))
        .unwrap_or_else(|| "?".into())
}

/// コアから受け取ったものを決まりの global に入れる
fn apply(ui: &Sidebar, s: core::Snap) {
    let g = ui.global::<Prelude>();
    g.set_online(s.online);
    g.set_status(s.status.into());
    g.set_space(s.space.into());
    let spaces: Vec<SharedString> = s.spaces.into_iter().map(Into::into).collect();
    g.set_spaces(ModelRc::new(VecModel::from(spaces)));
    let bookmarks: Vec<Bookmark> = s
        .bookmarks
        .into_iter()
        .map(|(path, title, folder)| Bookmark { path: path.into(), title: title.into(), folder: folder.into() })
        .collect();
    g.set_bookmarks(ModelRc::new(VecModel::from(bookmarks)));
    let tabs: Vec<Tab> =
        s.tabs.into_iter().map(|t| Tab { id: t.id, num: t.num, title: t.title.into(), kind: t.kind.into(), active: t.active }).collect();
    g.set_tabs(ModelRc::new(VecModel::from(tabs)));
}

fn main() -> Result<(), slint::PlatformError> {
    let t0 = Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();

    if let Some(i) = args.iter().position(|a| a == "--load") {
        let Some(path) = args.get(i + 1) else {
            eprintln!("usage: prelude-slint-proto --load <file.slint>");
            std::process::exit(2);
        };
        #[cfg(feature = "interpreter")]
        return loaded::run(path);
        #[cfg(not(feature = "interpreter"))]
        {
            eprintln!("--load {path}: build with `--features interpreter`");
            std::process::exit(2);
        }
    }

    slint::set_xdg_app_id("prelude-slint-proto")?;
    let ui = Sidebar::new()?;
    let (rows, info) = rows(args.first().map(String::as_str));
    ui.set_rows(ModelRc::new(VecModel::from(rows)));
    ui.set_rows_info(info.into());

    let g = ui.global::<Prelude>();
    g.set_status("コアにつないでいる…".into());
    g.on_switch_space(|s| core::switch_space(s.into()));
    g.on_open_bookmark(|p| core::open_bookmark(p.into()));
    g.on_activate_tab(core::activate_tab);
    g.on_close_tab(core::close_tab);
    g.on_open_launcher(core::open_launcher);
    // 幅の切り替え: 見た目だけの状態なのでサイドバーが持つ。ウィンドウの大きさを頼む(タイル型の環境では環境が決めた幅が勝つ)
    let weak = ui.as_weak();
    g.on_toggle_compact(move || {
        let Some(ui) = weak.upgrade() else { return };
        let g = ui.global::<Prelude>();
        let compact = !g.get_compact();
        g.set_compact(compact);
        let h = ui.window().size().to_logical(ui.window().scale_factor()).height;
        ui.window().set_size(slint::LogicalSize::new(if compact { COMPACT } else { WIDE }, h));
    });

    let weak = ui.as_weak();
    core::watch(move |s| {
        if let Some(ui) = weak.upgrade() {
            apply(&ui, s);
        }
    });

    // 起動の速さとメモリ(最初に描いた後)
    slint::Timer::single_shot(Duration::from_millis(500), move || {
        eprintln!(
            "started in {:.0} ms (incl. 500 ms wait), VmRSS {}, SLINT_BACKEND={}",
            ms(t0),
            rss(),
            std::env::var("SLINT_BACKEND").unwrap_or_else(|_| "(default)".into())
        );
    });
    ui.run()
}

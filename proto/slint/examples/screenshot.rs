//! 画面の無い所で、ソフトウェア描画で試作のサイドバーを描いて PNG に書き出す(見た目と日本語の字の確かめ)。
//! コアにはつながず、作った中身を入れる。使い方: `cargo run --example screenshot -- <出力フォルダ>`
use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, ModelRc, PhysicalSize, SharedString, VecModel};
use std::rc::Rc;

slint::include_modules!();

struct Headless(Rc<MinimalSoftwareWindow>);

impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

fn save(window: &MinimalSoftwareWindow, w: u32, h: u32, path: &str) {
    slint::platform::update_timers_and_animations();
    let mut buf = vec![PremultipliedRgbaColor::default(); (w * h) as usize];
    window.request_redraw();
    window.draw_if_needed(|r| {
        r.render(&mut buf, w as usize);
    });
    let mut out = Vec::with_capacity(buf.len() * 4);
    for p in buf {
        out.extend_from_slice(&[p.red, p.green, p.blue, 255]);
    }
    let f = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&out).unwrap();
    println!("{path}");
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Headless(window.clone()))).unwrap();
    let ui = Sidebar::new().unwrap();

    let g = ui.global::<Prelude>();
    g.set_online(true);
    g.set_space("論文執筆".into());
    let spaces: Vec<SharedString> = vec!["Main".into(), "論文執筆".into()];
    g.set_spaces(ModelRc::new(VecModel::from(spaces)));
    let bm = |t: &str, f: &str| Bookmark { path: format!("/s/{t}.prelude").into(), title: t.into(), folder: f.into() };
    g.set_bookmarks(ModelRc::new(VecModel::from(vec![bm("本文", ""), bm("文献を探す", ""), bm("図", "執筆")])));
    let tab = |id: i32, title: &str, kind: &str, active: bool| Tab { id, num: id, title: title.into(), kind: kind.into(), active };
    g.set_tabs(ModelRc::new(VecModel::from(vec![tab(1, "本文 - Kate", "app", true), tab(2, "scholar.google.com", "web", false), tab(3, "~/研究/論文", "files", false)])));
    let rows: Vec<SharedString> = (1..=10_000).map(|i| format!("{i:05}  資料_{}.txt", ["議事録", "論文", "写真", "見積"][i % 4]).into()).collect();
    ui.set_rows(ModelRc::new(VecModel::from(rows)));
    ui.set_rows_info("10000 件(作ったもの)".into());

    let (w, h) = (300, 760);
    window.set_size(PhysicalSize::new(w, h));
    ui.show().unwrap();
    save(&window, w, h, &format!("{dir}/wide.png"));

    g.set_compact(true);
    window.set_size(PhysicalSize::new(44, h));
    save(&window, 44, h, &format!("{dir}/compact.png"));
}

//! 見せ方の部品([`FilerView`] の実装)
use crate::info::{self, Kind, SortKey};
use crate::{DirState, Emit, FilerCommand, FilerView, human_size};
use gpui::{
    AnyElement, MouseButton, NavigationDirection, Pixels, Point, ScrollStrategy, UniformListScrollHandle, div, point, prelude::*, px, rgb, uniform_list,
};
use prelude_ui::{ROW_HEIGHT, color, label, row};
use std::cell::RefCell;
use std::rc::Rc;

/// 名前で見せ方を作る(知らない名前は None)
pub fn by_name(name: &str) -> Option<Box<dyn FilerView>> {
    match name {
        "list" => Some(Box::new(ListView::default())),
        _ => None,
    }
}

/// 列の幅(Electron 版の COLS。名前の列は残りの幅)
const TYPE_W: f32 = 72.;
const SIZE_W: f32 = 84.;
const DATE_W: f32 = 132.;

/// 一覧(1 行に 1 つ。種類の印・名前・種類・大きさ・更新時刻。Electron 版のフォルダのペイン)。見える行だけを描くので、1 万件でも重くならない
#[derive(Default)]
pub struct ListView {
    scroll: UniformListScrollHandle,
    /// 前に描いたときの選び(変わったときだけ、見える所へ動かす)
    last: Option<usize>,
    /// 引っぱって囲んで選ぶ途中(Electron 版の bandSelect)
    band: Rc<RefCell<Option<Band>>>,
}

/// 囲んで選ぶ枠。y は一覧の中身の上からの位置(スクロールしても変わらない)
#[derive(Debug, Clone, Copy)]
struct Band {
    start: Point<Pixels>,
    cur: Point<Pixels>,
    /// 4px 以上動かして、枠になった
    active: bool,
    /// Ctrl を押していた(それまでの選びに足す)
    add: bool,
}

/// 枠の上下(中身の位置)から、かかる行の範囲と、選んでいる位置(今の端)
pub fn band_rows(y0: f32, y1: f32, row: f32, n: usize) -> Option<(usize, usize, usize)> {
    if n == 0 {
        return None;
    }
    let (top, bot) = (y0.min(y1).max(0.), y0.max(y1));
    let lo = (top / row).floor() as usize;
    let hi = ((bot / row).floor() as usize).min(n - 1);
    if lo > hi {
        return None;
    }
    let cur = ((y1.max(0.) / row).floor() as usize).min(n - 1);
    Some((lo, hi, cur))
}

/// 種類の印(色つき)
pub fn kind_badge(k: Kind, inverted: bool) -> gpui::Div {
    let d = div().flex_none().w(px(18.)).text_size(px(11.)).child(k.glyph());
    match k.color() {
        Some(c) if !inverted => d.text_color(rgb(c)),
        _ => d,
    }
}

/// 種類の列の文字(DIR・拡張子の大文字・FILE)
pub fn type_text(e: &crate::Entry) -> String {
    if e.is_dir {
        return "DIR".into();
    }
    match info::ext(e) {
        x if x.is_empty() => "FILE".into(),
        x => x.to_uppercase(),
    }
}

impl ListView {
    fn header(dir: &DirState, emit: &Emit) -> AnyElement {
        let col = |id: &'static str, text: &'static str, key: SortKey, w: Option<f32>| {
            let emit = emit.clone();
            let mark = dir.sort.mark(key);
            let d = div()
                .id(id)
                .flex()
                .gap(px(4.))
                .cursor_pointer()
                .hover(|s| s.text_color(color::text()))
                .when(dir.sort.key == key, |d| d.text_color(color::text()))
                .child(text)
                .child(mark)
                .on_click(move |_, w, cx| emit(FilerCommand::Sort(key), w, cx));
            match w {
                Some(w) => d.flex_none().w(px(w)).justify_end(),
                None => d.flex_1().min_w(px(0.)),
            }
        };
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(22.))
            .flex_none()
            .px(px(6.))
            .text_size(px(11.))
            .text_color(color::dim())
            .border_b_1()
            .border_color(color::panel2())
            .child(div().flex_none().w(px(18.)))
            .child(col("h-name", "Name", SortKey::Name, None))
            .child(col("h-type", "Type", SortKey::Type, Some(TYPE_W)))
            .child(col("h-size", "Size", SortKey::Size, Some(SIZE_W)))
            .child(col("h-date", "Modified", SortKey::Date, Some(DATE_W)))
            .into_any_element()
    }
}

impl FilerView for ListView {
    fn name(&self) -> &'static str {
        "list"
    }

    fn render(&mut self, dir: &DirState, emit: Emit) -> AnyElement {
        if dir.selected != self.last {
            if let Some(i) = dir.selected {
                self.scroll.scroll_to_item(i, ScrollStrategy::Nearest);
            }
            self.last = dir.selected;
        }
        // 空いた所の右クリック・マウスの戻る/進むボタンは、一覧の全体で受ける
        let frame = |body: AnyElement, emit: &Emit| {
            let (m, b, f) = (emit.clone(), emit.clone(), emit.clone());
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h(px(0.))
                .on_mouse_down(MouseButton::Right, move |ev, w, cx| m(FilerCommand::Menu(None, ev.position), w, cx))
                .on_mouse_down(MouseButton::Navigate(NavigationDirection::Back), move |_, w, cx| b(FilerCommand::History(false), w, cx))
                .on_mouse_down(MouseButton::Navigate(NavigationDirection::Forward), move |_, w, cx| f(FilerCommand::History(true), w, cx))
                .child(Self::header(dir, emit))
                .child(body)
                .into_any_element()
        };
        if let Some(e) = &dir.error {
            return frame(div().p(px(8.)).text_color(color::danger()).child(e.clone()).into_any_element(), &emit);
        }
        if dir.entries.is_empty() {
            return frame(div().flex_1().p(px(8.)).text_color(color::dim()).child("(empty)").into_any_element(), &emit);
        }
        let entries = std::rc::Rc::new(dir.entries.clone());
        let entries_len = entries.len();
        let selected = dir.selected;
        let marked: std::collections::HashSet<usize> = (0..dir.entries.len()).filter(|i| dir.is_marked(*i)).collect();
        let cut = std::rc::Rc::new(dir.cut.clone());
        let emit2 = emit.clone();
        let list = uniform_list("entries", entries.len(), move |range, _, _| {
            range
                .map(|i| {
                    let e = &entries[i];
                    let sel = selected == Some(i);
                    let (emit, menu) = (emit2.clone(), emit2.clone());
                    // 名前は残りの幅いっぱい、種類・大きさ・時刻は右にそろえた列
                    let dim = |b: gpui::Div| if sel { b.text_color(color::bg()) } else { b.text_color(color::dim()) };
                    let small = |t: String, w: f32| div().flex_none().w(px(w)).text_right().text_size(px(11.)).overflow_hidden().whitespace_nowrap().child(t);
                    let size = if e.is_dir { String::new() } else { human_size(e.size) };
                    let time = e.modified.map(info::fmt_time).unwrap_or_default();
                    let r = row(("entry", i), sel)
                        .w_full()
                        .child(kind_badge(info::kind_of(e), sel))
                        .child(label(e.name.clone()))
                        .child(dim(small(type_text(e), TYPE_W)))
                        .child(dim(small(size, SIZE_W)))
                        .child(dim(small(time, DATE_W)));
                    let faded = (e.hidden || cut.contains(&e.name)) && !sel;
                    r.when(faded, |r| r.text_color(color::dim()))
                        .when(marked.contains(&i) && !sel, |r| r.bg(color::panel()))
                        .on_mouse_down(MouseButton::Right, move |ev, w, cx| {
                            cx.stop_propagation();
                            menu(FilerCommand::Menu(Some(i), ev.position), w, cx)
                        })
                        .on_click(move |ev, w, cx| {
                            // 1 回で選び、2 回で開く。Ctrl で足す・外す、Shift で範囲
                            let m = ev.modifiers();
                            let c = if m.control {
                                FilerCommand::Toggle(i)
                            } else if m.shift {
                                FilerCommand::Range(i)
                            } else if ev.click_count() >= 2 {
                                FilerCommand::Open(i)
                            } else {
                                FilerCommand::Select(i)
                            };
                            emit(c, w, cx);
                        })
                })
                .collect::<Vec<_>>()
        })
        .track_scroll(&self.scroll)
        .flex_1()
        .into_any_element();
        // 引っぱって囲んで選ぶ。押した所から 4px 動いたら枠にする。上下の端の外へ出たらスクロールする
        let n = entries_len;
        let row_h = f32::from(ROW_HEIGHT);
        let content = {
            let scroll = self.scroll.clone();
            move |p: Point<Pixels>| {
                let st = scroll.0.borrow();
                let (b, off) = (st.base_handle.bounds(), st.base_handle.offset());
                point(p.x - b.left(), p.y - b.top() - off.y)
            }
        };
        let (band_down, band_move, band_up, band_up_out) = (self.band.clone(), self.band.clone(), self.band.clone(), self.band.clone());
        let (content_down, content_move) = (content.clone(), content);
        let scroll = self.scroll.clone();
        let emit_move = emit.clone();
        let emit_up = emit.clone();
        let emit_up_out = emit.clone();
        let finish = move |band: &Rc<RefCell<Option<Band>>>, emit: &Emit, w: &mut gpui::Window, cx: &mut gpui::App| {
            if band.borrow_mut().take().is_some_and(|b| b.active) {
                emit(FilerCommand::Band(None), w, cx);
            }
        };
        let finish2 = finish;
        let drawn = self.band.borrow().filter(|b| b.active).map(|b| {
            let st = self.scroll.0.borrow();
            let (bounds, off) = (st.base_handle.bounds(), st.base_handle.offset());
            let h = f32::from(bounds.size.height);
            let y = |v: Pixels| (f32::from(v + off.y)).clamp(0., h);
            let (x0, x1) = (b.start.x.min(b.cur.x), b.start.x.max(b.cur.x));
            let (y0, y1) = (y(b.start.y.min(b.cur.y)), y(b.start.y.max(b.cur.y)));
            // 見出しの下から(見出しの高さ 22px)
            div().absolute().left(x0).top(px(22. + y0)).w(x1 - x0).h(px(y1 - y0)).border_1().border_color(color::accent()).bg(color::accent().opacity(0.15))
        });
        let list = div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.))
            .child(list)
            .on_mouse_down(MouseButton::Left, move |ev, _, _| {
                *band_down.borrow_mut() = Some(Band { start: content_down(ev.position), cur: content_down(ev.position), active: false, add: ev.modifiers.control });
            })
            .on_mouse_move(move |ev, w, cx| {
                if ev.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                let mut g = band_move.borrow_mut();
                let Some(b) = g.as_mut() else { return };
                // 端の外へ出たら 1 行ずつスクロール
                {
                    let st = scroll.0.borrow();
                    let bounds = st.base_handle.bounds();
                    let off = st.base_handle.offset();
                    let d = if ev.position.y < bounds.top() + px(8.) {
                        ROW_HEIGHT
                    } else if ev.position.y > bounds.bottom() - px(8.) {
                        -ROW_HEIGHT
                    } else {
                        px(0.)
                    };
                    if b.active && d != px(0.) {
                        let max = -(ROW_HEIGHT * n as f32 - bounds.size.height).max(px(0.));
                        st.base_handle.set_offset(point(off.x, (off.y + d).clamp(max, px(0.))));
                    }
                }
                b.cur = content_move(ev.position);
                if !b.active && (b.cur.x - b.start.x).abs() < px(4.) && (b.cur.y - b.start.y).abs() < px(4.) {
                    return;
                }
                b.active = true;
                let (b, add) = (*b, b.add);
                drop(g);
                if let Some((lo, hi, cur)) = band_rows(f32::from(b.start.y), f32::from(b.cur.y), row_h, n) {
                    emit_move(FilerCommand::Band(Some((lo, hi, cur, add))), w, cx);
                } else {
                    w.refresh();
                }
            })
            .on_mouse_up(MouseButton::Left, move |_, w, cx| finish(&band_up, &emit_up, w, cx))
            .on_mouse_up_out(MouseButton::Left, move |_, w, cx| finish2(&band_up_out, &emit_up_out, w, cx))
            .when_some(drawn, |d, r| d.child(r))
            .into_any_element();
        frame(list, &emit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 囲んだ行() {
        // 行の高さ 30。上から下へ・下から上へ・上にはみ出し・下にはみ出し
        assert_eq!(band_rows(5., 65., 30., 10), Some((0, 2, 2)));
        assert_eq!(band_rows(65., 5., 30., 10), Some((0, 2, 0)));
        assert_eq!(band_rows(40., -50., 30., 10), Some((0, 1, 0)));
        assert_eq!(band_rows(40., 1000., 30., 3), Some((1, 2, 2)));
        // 一覧の下の空いた所だけ
        assert_eq!(band_rows(200., 250., 30., 3), None);
        assert_eq!(band_rows(0., 10., 30., 0), None);
    }

    #[test]
    fn 種類の列() {
        let e = |name: &str, is_dir: bool| crate::Entry { name: name.into(), is_dir, size: 0, modified: None, hidden: false };
        assert_eq!(type_text(&e("a.tar.gz", false)), "GZ");
        assert_eq!(type_text(&e("Makefile", false)), "FILE");
        assert_eq!(type_text(&e("x.d", true)), "DIR");
    }
}

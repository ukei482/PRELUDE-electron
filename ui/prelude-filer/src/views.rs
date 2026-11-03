//! 見せ方の部品([`FilerView`] の実装)
use crate::{DirState, Emit, FilerCommand, FilerView, human_size, local_time};
use gpui::{AnyElement, ScrollStrategy, UniformListScrollHandle, div, prelude::*, px, uniform_list};
use prelude_ui::{badge, color, label, row};

/// 名前で見せ方を作る(知らない名前は None)
pub fn by_name(name: &str) -> Option<Box<dyn FilerView>> {
    match name {
        "list" => Some(Box::new(ListView::default())),
        _ => None,
    }
}

/// 一覧(1 行に 1 つ。名前・大きさ・更新時刻)。見える行だけを描くので、1 万件でも重くならない
#[derive(Default)]
pub struct ListView {
    scroll: UniformListScrollHandle,
    /// 前に描いたときの選び(変わったときだけ、見える所へ動かす)
    last: Option<usize>,
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
        if let Some(e) = &dir.error {
            return div().p(px(8.)).text_color(color::danger()).child(e.clone()).into_any_element();
        }
        if dir.entries.is_empty() {
            return div().p(px(8.)).text_color(color::dim()).child("(empty)").into_any_element();
        }
        let entries = std::rc::Rc::new(dir.entries.clone());
        let selected = dir.selected;
        uniform_list("entries", entries.len(), move |range, _, _| {
            range
                .map(|i| {
                    let e = &entries[i];
                    let sel = selected == Some(i);
                    let emit = emit.clone();
                    // 名前は残りの幅いっぱい、大きさ・時刻は右にそろえた列
                    let dim = |b: gpui::Div| b.when(sel, |b| b.text_color(color::bg()));
                    let size = if e.is_dir { String::new() } else { human_size(e.size) };
                    let time = e.modified.map(local_time).unwrap_or_default();
                    let r = row(("entry", i), sel)
                        .w_full()
                        .child(dim(badge(if e.is_dir { "▸" } else { " " })))
                        .child(label(e.name.clone()))
                        .child(dim(badge(size).w(px(72.)).text_right()))
                        .child(dim(badge(time).w(px(116.)).text_right()));
                    r.when(e.hidden && !sel, |r| r.text_color(color::dim())).on_click(move |ev, w, cx| {
                        // 1 回で選び、2 回で開く
                        let c = if ev.click_count() >= 2 { FilerCommand::Open(i) } else { FilerCommand::Select(i) };
                        emit(c, w, cx);
                    })
                })
                .collect::<Vec<_>>()
        })
        .track_scroll(&self.scroll)
        .flex_1()
        .into_any_element()
    }
}

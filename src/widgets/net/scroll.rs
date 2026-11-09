//! A scrollable column that keeps the keyboard cursor in sight: the port of Slint's `ScrollList`
//! (`legacy/ui/common.slint`).
//!
//! The rows are the direct children of one scrolling element, so a row's index is its place in
//! the list. A widget says which row the cursor is on (`follow`) every time it draws, and the
//! list scrolls just enough to show it, in the same frame, whatever changed (the cursor moved, a
//! section appeared above it, the tile was resized). Turning the wheel lets go of the cursor
//! until the next key.
//!
//! The scrollbar has its own gutter in the card's padding, so rows never sit under it.

use crate::kit::Tk;
use gpui::prelude::*;
use gpui::{AnyElement, ElementId, ScrollHandle, div, px};
use std::cell::Cell;
use std::rc::Rc;

/// The scroll state of one list (a page of a widget).
#[derive(Clone, Default)]
pub struct Scroller {
    handle: ScrollHandle,
    /// The wheel was turned since the last key: the list stays where it was put.
    wheeled: Rc<Cell<bool>>,
}

impl Scroller {
    /// A key was pressed: the list follows the cursor again.
    pub fn follow_again(&self) {
        self.wheeled.set(false);
    }

    /// Whether row `ix` is clearly in sight: no more than a quarter of it cut off by the edge of
    /// the list (a few pixels are no matter). `None` while the list hasn't been laid out, or has
    /// no such row.
    pub fn sees(&self, ix: usize) -> Option<bool> {
        let view = self.handle.bounds();
        let row = self.handle.bounds_for_item(ix)?;
        let (view_h, row_h) = (f32::from(view.size.height), f32::from(row.size.height));
        if view_h <= 0.0 || row_h <= 0.0 {
            return None;
        }
        let top = f32::from(row.top() + self.handle.offset().y);
        let (view_top, view_bottom) = (f32::from(view.top()), f32::from(view.bottom()));
        let shown = (top + row_h).min(view_bottom) - top.max(view_top);
        Some(shown >= 0.75 * row_h.min(view_h))
    }

    /// How many rows the list was laid out with.
    pub fn rows(&self) -> usize {
        self.handle.children_count()
    }

    /// The list: `rows` stacked, scrolled to show row `follow` (the cursor's) unless the wheel was
    /// turned since the last key. It fills the room it is given. `gap` px come under each row.
    pub fn list(
        &self,
        id: &'static str,
        rows: Vec<AnyElement>,
        follow: Option<usize>,
        gap: f32,
        tk: &Tk,
    ) -> AnyElement {
        if let Some(ix) = follow
            && !self.wheeled.get()
        {
            self.handle.scroll_to_item(ix);
        }
        let id_name = id;
        let id: ElementId = id.into();
        // Lists sit in a card's padding; the scrollbar goes there too, so rows are as wide as the
        // header above them. Only a tight padding needs a gutter of its own.
        let in_padding = tk.pad() >= 8.0;
        let gutter = if in_padding { 0.0 } else { 10.0 };
        let wheeled = self.wheeled.clone();
        let thumb = self.thumb(in_padding, tk);
        div()
            .debug_selector(move || id_name.into())
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(
                div()
                    .id(id)
                    .absolute()
                    .top_0()
                    .left_0()
                    .bottom_0()
                    .right(px(gutter))
                    .overflow_y_scroll()
                    .track_scroll(&self.handle)
                    .on_scroll(move |_, _, _| wheeled.set(true))
                    .flex()
                    .flex_col()
                    .children(rows.into_iter().map(|row| {
                        div()
                            .flex_none()
                            .w_full()
                            .pb(px(gap))
                            .child(row)
                            .into_any_element()
                    })),
            )
            .children(thumb)
            .into_any_element()
    }

    /// The scrollbar's thumb, as of the last frame; none while everything fits.
    fn thumb(&self, in_padding: bool, tk: &Tk) -> Option<AnyElement> {
        let view_h = f32::from(self.handle.bounds().size.height);
        let max = f32::from(self.handle.max_offset().y).abs();
        if view_h <= 0.0 || max <= 1.0 {
            return None;
        }
        let content = view_h + max;
        let thumb = (view_h * view_h / content).max(24.0).min(view_h);
        let at = (-f32::from(self.handle.offset().y) / max).clamp(0.0, 1.0);
        let right = if in_padding {
            -(tk.pad() + 4.0) / 2.0
        } else {
            1.0
        };
        Some(
            div()
                .absolute()
                .right(px(right))
                .top_0()
                .bottom_0()
                .w(px(4.0))
                .rounded(px(2.0))
                .bg(tk.subtle())
                .child(
                    div()
                        .absolute()
                        .top(px((view_h - thumb) * at))
                        .h(px(thumb))
                        .w_full()
                        .rounded(px(2.0))
                        .bg(tk.faint()),
                )
                .into_any_element(),
        )
    }
}

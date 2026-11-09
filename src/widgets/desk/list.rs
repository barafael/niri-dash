//! A column of rows that scrolls and keeps the keyboard cursor's row in sight: the port of
//! `ScrollList` in `common.slint`.
//!
//! The list remembers what the cursor last asked to see, and when the cursor, the number of rows
//! or the height of the list change (a key moved it, a row appeared above it, the widget was
//! resized) it scrolls just enough to show the row again. The mouse wheel is left alone
//! otherwise.
//!
//! GPUI scrolls a row into view while it lays the list out, with the list's size of the frame
//! before. When the size is what changed (the widget was resized, the text got bigger) that is
//! the old size, so the list looks once more at the next frame (`cx.notify()`); until then the
//! cursor may not be in sight yet, and [`ScrollList::in_sight`] says nothing.

use crate::dash::Dash;
use crate::kit::Tk;
use gpui::prelude::*;
use gpui::{AnyElement, Context, ElementId, ScrollHandle, div, px};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

/// One list's scroll position, and what it last scrolled for.
#[derive(Clone, Default)]
pub struct ScrollList {
    handle: ScrollHandle,
    /// What the last frame was drawn for.
    seen: Rc<Cell<Seen>>,
    /// The last frame scrolled with the size of the one before: look again.
    pending: Rc<Cell<bool>>,
}

/// What a list was last drawn for: when any of it changes the cursor's row is brought into view.
#[derive(Clone, Copy, Default, PartialEq)]
struct Seen {
    cursor: Option<usize>,
    rows: usize,
    /// The list's height, and what sizes its rows (the card, the text).
    height: i32,
    print: u64,
}

/// What sizes the rows of a list in a card of `w` by `h`: the card, and the look's measures.
pub fn fingerprint(w: f32, h: f32, tk: &Tk) -> u64 {
    use std::hash::{Hash, Hasher};
    let s = &tk.look.style;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for v in [
        w,
        h,
        s.padding,
        s.gap,
        s.radius,
        s.radius_small,
        s.font_small,
        s.font_body,
        s.font_title,
    ] {
        v.to_bits().hash(&mut hasher);
    }
    s.font_family.hash(&mut hasher);
    hasher.finish()
}

impl ScrollList {
    /// The list as a flex child that takes the room left: `rows` are its children (each one a
    /// stop or a heading), `cursor` the child the keyboard cursor is on (`None`: not entered,
    /// or on something outside the list).
    pub fn view(
        &self,
        id: impl Into<ElementId>,
        cursor: Option<usize>,
        rows: Vec<AnyElement>,
        print: u64,
        tk: &Tk,
        cx: &mut Context<Dash>,
    ) -> AnyElement {
        let height = f32::from(self.handle.bounds().size.height) as i32;
        let now = Seen {
            cursor,
            rows: rows.len(),
            height,
            print,
        };
        let was = self.seen.replace(now);
        let stale = was != now && (was.height != now.height || was.print != now.print);
        if was != now
            && let Some(ix) = cursor
        {
            self.handle.scroll_to_item(ix);
        }
        self.pending.set(stale && cursor.is_some());
        if self.pending.get() {
            cx.notify();
        }
        let max = self.handle.max_offset();
        let scrolls = f32::from(max.y.abs()) > 1.0;
        let view = f32::from(self.handle.bounds().size.height);
        let (thumb, top) = if scrolls {
            let content = view + f32::from(max.y.abs());
            let thumb = (view * view / content).max(24.0).min(view);
            let done = (-f32::from(self.handle.offset().y) / f32::from(max.y.abs())).clamp(0.0, 1.0);
            (thumb, (view - thumb) * done)
        } else {
            (0.0, 0.0)
        };
        // The scrollbar sits in the card's padding, beside the rows, so they are as wide as the
        // header above them.
        // Slint puts the bar's left edge half the padding less its own width past the rows (a
        // tight padding has no room, and the bar goes inside, in a gutter of 10 px).
        let in_padding = tk.pad() >= 8.0;
        let inset = if in_padding { (tk.pad() + 4.0) / 2.0 } else { -1.0 };
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(
                div()
                    .id(id)
                    .size_full()
                    .when(!in_padding, |l| l.pr(px(10.0)))
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .track_scroll(&self.handle)
                    .children(rows),
            )
            .when(scrolls, |list| {
                list.child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .right(px(-inset))
                        .w(px(4.0))
                        .rounded(px(2.0))
                        .bg(tk.subtle())
                        .child(
                            div()
                                .absolute()
                                .top(px(top))
                                .w_full()
                                .h(px(thumb))
                                .rounded(px(2.0))
                                .bg(tk.faint()),
                        ),
                )
            })
            .into_any_element()
    }

    /// Whether row `ix` is (at least partly) inside the list, as of the last frame: what the
    /// stress run checks of the cursor. `None` until the list has been laid out.
    pub fn in_sight(&self, ix: usize) -> Option<bool> {
        if self.pending.get() {
            return None;
        }
        let view = self.handle.bounds();
        if view.size.height <= px(0.0) {
            return None;
        }
        let row = self.handle.bounds_for_item(ix)?;
        let off = self.handle.offset();
        let (top, bottom) = (row.top() + off.y, row.bottom() + off.y);
        let slack = px(1.5);
        Some(if row.size.height > view.size.height {
            bottom > view.top() && top < view.bottom()
        } else {
            top >= view.top() - slack && bottom <= view.bottom() + slack
        })
    }

    /// Scroll back to the top (the dashboard opened).
    pub fn reset(&self) {
        self.handle.set_offset(gpui::point(px(0.0), px(0.0)));
        self.seen.set(Seen::default());
        self.pending.set(false);
    }
}

/// The lists of every instance of a widget, by tile id.
#[derive(Default)]
pub struct Lists(RefCell<HashMap<u32, ScrollList>>);

impl Lists {
    pub fn of(&self, id: u32) -> ScrollList {
        self.0.borrow_mut().entry(id).or_default().clone()
    }

    pub fn reset(&self) {
        for list in self.0.borrow().values() {
            list.reset();
        }
    }
}

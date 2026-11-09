//! The small pieces the devices group's widgets (and the about card) are made of: buttons that
//! can be disabled or carry the keyboard cursor, rows, a header with things at its right end, a
//! list that scrolls and keeps the cursor in sight, a ring. Ports of the parts of
//! `legacy/ui/common.slint` that `kit` doesn't have (yet).

use crate::kit::{Tk, icon, with_alpha};
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, ClickEvent, Div, ElementId, Hsla, Pixels, Role, ScrollHandle,
    SharedString, Stateful, Window, canvas, div, fill, point, px, size,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

/// How a widget's card was drawn.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Card {
    pub w: f32,
    pub h: f32,
    pub round: bool,
}

impl Card {
    pub fn of(ctx: &Ctx) -> Self {
        Self {
            w: ctx.w,
            h: ctx.h,
            round: ctx.round,
        }
    }
}

/// How big each widget was drawn, by tile id. The keys and the stress run's checks need to know
/// what a widget shows (a short tile has no list, so no stops), and only the drawing knows.
#[derive(Default)]
pub struct Seen(RefCell<HashMap<u32, Card>>);

impl Seen {
    pub fn note(&self, id: u32, card: Card) {
        self.0.borrow_mut().insert(id, card);
    }

    /// The card the last time it was drawn; not yet: none.
    pub fn of(&self, id: u32) -> Card {
        self.0.borrow().get(&id).copied().unwrap_or_default()
    }
}

/// What the keyboard cursor last asked a list to show, so a list scrolls to it when it moves (or
/// the list changed under it), and not every time it is drawn: a wheel turned since stays.
#[derive(Default)]
pub struct Revealed(Cell<Option<(usize, usize)>>);

impl Revealed {
    /// Scroll `list` to child `child` if (cursor, count) is not what was last shown.
    pub fn show(&self, list: &ScrollHandle, key: Option<(usize, usize)>, child: usize) {
        if self.0.get() != key {
            self.0.set(key);
            if key.is_some() {
                list.scroll_to_item(child);
            }
        }
    }
}

/// Text that is cut with an ellipsis rather than wrapped or overflowing.
pub fn elide(d: Div) -> Div {
    d.min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
}

/// Secondary text, one line.
pub fn muted(text: impl Into<SharedString>, tk: &Tk) -> Div {
    elide(div())
        .text_size(px(tk.font_small()))
        .text_color(tk.dim())
        .child(text.into())
}

/// What a widget says when it has nothing to show: centered, wrapped, filling the room.
pub fn empty(text: impl Into<SharedString>, tk: &Tk) -> Div {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .text_size(px(tk.font_body()))
        .text_color(tk.faint())
        .child(
            div()
                .w_full()
                .text_center()
                .debug_selector(|| "empty state".into())
                .child(text.into()),
        )
}

/// A widget's header: icon, title, and what goes at the right end.
pub fn header(
    icon_name: &str,
    title: impl Into<SharedString>,
    tk: &Tk,
    right: Vec<AnyElement>,
) -> Div {
    div()
        .flex_none()
        .h(px(28.0))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(icon(icon_name, 18.0, tk.dim()))
        .child(
            elide(div())
                .flex_1()
                .text_size(px(tk.font_title()))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(tk.text())
                .child(title.into()),
        )
        .children(right)
}

/// A square button with an icon. `label` says what it does (to screen readers and the tests).
pub fn icon_button(
    id: impl Into<ElementId>,
    name: &str,
    label: impl Into<SharedString>,
    size: f32,
    enabled: bool,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let label: SharedString = label.into();
    let name_ = label.to_string();
    let (hover, pressed) = (tk.hover(), tk.pressed());
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .debug_selector(move || name_)
        .flex_none()
        .size(px(size))
        .rounded(px(tk.radius_small().min(size / 2.0)))
        .flex()
        .items_center()
        .justify_center()
        .when(enabled, |b| {
            b.cursor_pointer()
                .hover(move |s| s.bg(hover))
                .active(move |s| s.bg(pressed))
                .on_click(move |e, w, c| {
                    // A button in a row is not a click on the row.
                    c.stop_propagation();
                    on_click(e, w, c)
                })
        })
        .when(!enabled, |b| b.opacity(0.4))
        .child(icon(name, size * 0.55, tk.text()))
}

/// A button with a text (and maybe an icon). `label` is what a screen reader says, when it is
/// more than the text ("Stop sharing with Firefox" on "Stop"). `focus`: the keyboard cursor is on
/// it.
#[allow(clippy::too_many_arguments)]
pub fn text_button(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    label: impl Into<SharedString>,
    icon_name: Option<&str>,
    danger: bool,
    enabled: bool,
    focus: bool,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let label: SharedString = label.into();
    let name_ = label.to_string();
    let (rest, hover, pressed) = if danger {
        (
            with_alpha(tk.bad(), 0.2),
            with_alpha(tk.bad(), 0.33),
            with_alpha(tk.bad(), 0.45),
        )
    } else {
        (tk.card_raised(), tk.hover(), tk.pressed())
    };
    let ink = if danger { tk.bad() } else { tk.text() };
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .aria_selected(focus)
        .debug_selector(move || name_)
        .flex_none()
        .h(px(30.0))
        .px(px(10.0))
        .flex()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .rounded(px(tk.radius_small().min(15.0)))
        .border_2()
        .border_color(if focus {
            tk.accent()
        } else {
            gpui::transparent_black()
        })
        .bg(rest)
        .text_size(px(tk.font_body()))
        .text_color(ink)
        .whitespace_nowrap()
        .when(enabled, |b| {
            b.cursor_pointer()
                .hover(move |s| s.bg(hover))
                .active(move |s| s.bg(pressed))
                .on_click(move |e, w, c| {
                    c.stop_propagation();
                    on_click(e, w, c)
                })
        })
        .when(!enabled, |b| b.opacity(0.45))
        .when_some(icon_name, |b, name| b.child(icon(name, 16.0, ink)))
        .child(text.into())
}

/// What a row does when it is clicked.
pub type OnClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A row of a list: highlights under the pointer and the keyboard cursor (`focus`), and clicks
/// when `enabled`. `value` says how it stands, for screen readers ("mounted").
#[allow(clippy::too_many_arguments)]
pub fn row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    value: Option<SharedString>,
    height: f32,
    enabled: bool,
    focus: bool,
    tk: &Tk,
    on_click: Option<OnClick>,
) -> Stateful<Div> {
    let label: SharedString = label.into();
    let name_ = label.to_string();
    let (hover, pressed) = (tk.hover(), tk.pressed());
    div()
        .id(id)
        .role(Role::ListItem)
        .aria_label(label)
        .when_some(value, |r, v| r.aria_value(v))
        .aria_selected(focus)
        .debug_selector(move || name_)
        .flex_none()
        .w_full()
        .h(px(height))
        .relative()
        .rounded(px(tk.radius_small().min(height / 2.0)))
        // The cursor's ring is drawn over the row, as in Slint, where a border takes no room:
        // what is in the row stays where the row's padding puts it.
        .when(focus, |r| {
            r.child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(tk.radius_small().min(height / 2.0)))
                    .border_2()
                    .border_color(tk.accent()),
            )
        })
        .when(focus, |r| r.bg(hover))
        .when(enabled, |r| {
            r.hover(move |s| s.bg(hover))
                .active(move |s| s.bg(pressed))
                .when_some(on_click, |r, f| {
                    r.cursor_pointer().on_click(move |e, w, c| f(e, w, c))
                })
        })
}

/// A column of rows that scrolls (wheel, and the keyboard cursor through [`Revealed`]), with a
/// thin scroll bar that shows when there is something to scroll. Each row is a direct child, so
/// the list can scroll to one by its index. It fills what room its parent leaves.
pub fn scroll_list(
    id: impl Into<ElementId>,
    handle: &ScrollHandle,
    rows: Vec<AnyElement>,
    tk: &Tk,
) -> Div {
    let max = f32::from(handle.max_offset().y).max(0.0);
    let view = f32::from(handle.bounds().size.height);
    // Lists sit in a card's padding, and so does their scroll bar; a tight padding has no room
    // for it and reserves a gutter.
    let in_padding = tk.pad() >= 8.0;
    let gutter = if in_padding { 0.0 } else { 10.0 };
    let bar = (max > 1.0 && view > 0.0).then(|| {
        let content = view + max;
        let thumb = (view * view / content).max(24.0).min(view);
        let at = (view - thumb) * (-f32::from(handle.offset().y) / max).clamp(0.0, 1.0);
        let right = if in_padding {
            -(tk.pad() + 4.0) / 2.0
        } else {
            1.0
        };
        div()
            .absolute()
            .top_0()
            .right(px(right))
            .w(px(4.0))
            .h(px(view))
            .rounded(px(2.0))
            .bg(tk.subtle())
            .child(
                div()
                    .absolute()
                    .top(px(at))
                    .w_full()
                    .h(px(thumb))
                    .rounded(px(2.0))
                    .bg(tk.faint()),
            )
    });
    div()
        .relative()
        .flex_1()
        .min_h_0()
        .w_full()
        .child(
            div()
                .id(id)
                .size_full()
                .pr(px(gutter))
                .overflow_y_scroll()
                .track_scroll(handle)
                .flex()
                .flex_col()
                .children(rows),
        )
        .children(bar)
}

/// A ring `value` (0 to 1) full, `thickness` thick, filling its parent (a square).
pub fn ring(value: f32, thickness: f32, tint: Hsla, track: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, _, window, _| {
            let side = f32::from(bounds.size.width).min(f32::from(bounds.size.height));
            if side <= 0.0 {
                return;
            }
            let c = point(
                bounds.origin.x + bounds.size.width / 2.0,
                bounds.origin.y + bounds.size.height / 2.0,
            );
            // The Slint ring's stroke sits a whole thickness in from the edge of its box.
            let r = (side / 2.0 - thickness).max(1.0);
            let at = |frac: f32| {
                let a = frac * std::f32::consts::TAU;
                point(c.x + px(r * a.sin()), c.y - px(r * a.cos()))
            };
            let arc = |frac: f32| {
                let mut b = gpui::PathBuilder::stroke(px(thickness));
                b.move_to(at(0.0));
                if frac >= 0.9999 {
                    b.arc_to(point(px(r), px(r)), px(0.0), false, true, at(0.5));
                    b.arc_to(point(px(r), px(r)), px(0.0), false, true, at(0.0));
                } else {
                    b.arc_to(point(px(r), px(r)), px(0.0), frac > 0.5, true, at(frac));
                }
                b.build().ok()
            };
            if let Some(path) = arc(1.0) {
                window.paint_path(path, track);
            }
            let v = value.clamp(0.0, 0.9999);
            if v > 0.002
                && let Some(path) = arc(v)
            {
                window.paint_path(path, tint);
                // Round ends.
                let t = px(thickness);
                for p in [at(0.0), at(v)] {
                    let dot = Bounds::new(point(p.x - t / 2.0, p.y - t / 2.0), size(t, t));
                    window.paint_quad(fill(dot, tint).corner_radii(t / 2.0));
                }
            }
        },
    )
    .size_full()
}

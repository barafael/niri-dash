//! The small pieces the desk's widgets are made of: buttons and rows that show the keyboard cursor,
//! headers with something on the right, the empty line, a progress ring, the round faces. Ports of
//! `common.slint`'s `IconButton`, `TextButton`, `Row`, `Header`, `Ring`, `RoundFace`, with the
//! cursor ring (`kfocus`) they have there. Used by two widgets of the group, so they live here
//! first and move to `kit` if another group wants them.

use crate::kit::{self, Tk, with_alpha};
use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Div, ElementId, Hsla, Role, SharedString, Stateful, canvas, div, point,
    px,
};
use crate::kit::AriaCursor;

/// `c` lighter by `k` (a fraction of its lightness).
pub fn brighter(c: Hsla, k: f32) -> Hsla {
    Hsla {
        l: (c.l * (1.0 + k)).min(1.0),
        ..c
    }
}

/// `c` darker by `k`.
pub fn darker(c: Hsla, k: f32) -> Hsla {
    Hsla {
        l: c.l * (1.0 - k),
        ..c
    }
}

/// The ring that shows where the keyboard cursor is: laid over the element, taking no room.
pub fn cursor_ring(radius: f32, tk: &Tk) -> Div {
    div()
        .absolute()
        .inset_0()
        .rounded(px(radius))
        .border_2()
        .border_color(tk.accent())
}

/// Secondary text, one line.
pub fn muted(text: impl Into<SharedString>, tk: &Tk) -> Div {
    div()
        .text_size(px(tk.font_small()))
        .text_color(tk.dim())
        .truncate()
        .child(text.into())
}

/// A small heading over a group of rows.
pub fn section_label(text: impl Into<SharedString>, tk: &Tk) -> Div {
    div()
        .text_size(px(tk.font_small()))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(tk.faint())
        .truncate()
        .child(text.into())
}

/// What a widget says when it has nothing to show: centered, taking the room left.
pub fn empty_text(text: impl Into<SharedString>, tk: &Tk) -> AnyElement {
    div()
        .flex_1()
        .w_full()
        .flex()
        .items_center()
        .justify_center()
        .text_center()
        .text_size(px(tk.font_body()))
        .text_color(tk.faint())
        .child(text.into())
        .into_any_element()
}

/// A widget's title row: an icon, the title, and `right` (buttons, a count) at the end.
pub fn header(
    icon: &str,
    icon_color: Hsla,
    title: impl Into<SharedString>,
    right: Vec<AnyElement>,
    tk: &Tk,
) -> Div {
    div()
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .gap(px(8.0))
        .h(px(28.0))
        .w_full()
        .child(kit::icon(icon, 18.0, icon_color))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(tk.font_title()))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(tk.text())
                .child(title.into()),
        )
        .children(right)
}

/// How a button looks.
#[derive(Clone, Copy, Default)]
pub struct Look {
    /// The widget's own state, shown as the accent (a pause that is on).
    pub active: bool,
    /// Under the keyboard cursor.
    pub kfocus: bool,
    pub enabled: bool,
    /// The fill at rest (an icon button that is a button, not just an icon).
    pub fill: Option<Hsla>,
    pub primary: bool,
    /// The icon's size in px, if not the usual share of the button.
    pub icon_size: Option<f32>,
}

impl Look {
    pub fn on() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }
}

/// A square button with an icon. Says what it does to screen readers (and the tests, which find
/// it by that name). A click on it is not a click on the row it sits in.
#[allow(clippy::too_many_arguments)]
pub fn icon_button(
    id: impl Into<ElementId>,
    icon: &'static str,
    label: impl Into<SharedString>,
    size: f32,
    tint: Hsla,
    look: Look,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> Stateful<Div> {
    let label: SharedString = label.into();
    let name = label.to_string();
    let radius = tk.radius_small().min(size / 2.0);
    let (hover, pressed) = (tk.hover(), tk.pressed());
    let tint = if look.active { tk.accent() } else { tint };
    let bg = if look.active {
        tk.accent_soft()
    } else if look.kfocus {
        hover
    } else {
        look.fill.unwrap_or(gpui::transparent_black())
    };
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .aria_cursor(look.kfocus)
        .debug_selector(move || name)
        .relative()
        .flex_none()
        .size(px(size))
        .rounded(px(radius))
        .bg(bg)
        .flex()
        .items_center()
        .justify_center()
        .when(look.enabled, |b| {
            b.cursor_pointer()
                .hover(move |s| s.bg(hover))
                .active(move |s| s.bg(pressed))
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |ev, window, cx| {
                    cx.stop_propagation();
                    on_click(ev, window, cx);
                })
        })
        .when(!look.enabled, |b| b.opacity(0.4))
        .child(kit::icon(icon, look.icon_size.unwrap_or(size * 0.55), tint))
        .when(look.kfocus, |b| b.child(cursor_ring(radius, tk)))
}

/// A button with a label (30 px high). `a11y` is what a screen reader says if that is more than
/// the label.
#[allow(clippy::too_many_arguments)]
pub fn text_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    a11y: impl Into<SharedString>,
    look: Look,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> Stateful<Div> {
    let label: SharedString = label.into();
    let a11y: SharedString = a11y.into();
    let name = a11y.to_string();
    let radius = tk.radius_small().min(15.0);
    let (hover, pressed) = (tk.hover(), tk.pressed());
    let accent = tk.accent();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(a11y)
        .aria_cursor(look.kfocus)
        .debug_selector(move || name)
        .relative()
        .flex_none()
        .h(px(30.0))
        .px(px(12.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(radius))
        .text_size(px(tk.font_body()))
        .whitespace_nowrap()
        .when(look.primary, |b| {
            b.bg(accent)
                .text_color(tk.on_accent())
                .hover(move |s| s.bg(brighter(accent, 0.1)))
                .active(move |s| s.bg(darker(accent, 0.2)))
        })
        .when(!look.primary, |b| {
            b.bg(tk.card_raised())
                .text_color(tk.text())
                .hover(move |s| s.bg(hover))
                .active(move |s| s.bg(pressed))
        })
        .cursor_pointer()
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |ev, window, cx| {
            cx.stop_propagation();
            on_click(ev, window, cx);
        })
        .child(label)
        .when(look.kfocus, |b| {
            b.child(cursor_ring(radius, tk).border_color(if look.primary {
                tk.text()
            } else {
                tk.accent()
            }))
        })
}

/// A list row `height` px high: a hover and press look, `selected` in the accent, the cursor ring
/// when `kfocus` or `kpart` (the cursor is on a part of it).
pub fn row(
    id: impl Into<ElementId>,
    height: f32,
    selected: bool,
    kfocus: bool,
    enabled: bool,
    tk: &Tk,
) -> Stateful<Div> {
    let radius = tk.radius_small().min(height / 2.0);
    let (hover, pressed) = (tk.hover(), tk.pressed());
    div()
        .id(id)
        .role(Role::ListItem)
        .aria_cursor(kfocus)
        .relative()
        .flex_none()
        .w_full()
        .h(px(height))
        .rounded(px(radius))
        .bg(if selected {
            tk.accent_soft()
        } else if kfocus {
            hover
        } else {
            gpui::transparent_black()
        })
        .when(enabled, |r| {
            r.cursor_pointer()
                .hover(move |s| s.bg(hover))
                .active(move |s| s.bg(pressed))
        })
        .when(kfocus, |r| r.child(cursor_ring(radius, tk)))
}

/// An arc from 12 o'clock clockwise, `frac` (0 to 1) of the way round, in a square: a track
/// under it. `thickness` is the stroke's width in px.
pub fn ring(
    key: u64,
    ms: f32,
    frac: f32,
    thickness: f32,
    tint: Hsla,
    track: Hsla,
) -> gpui::Canvas<()> {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let side = f32::from(bounds.size.width.min(bounds.size.height));
            // The Slint ring's stroke sits a whole thickness in from the edge of its box.
            let r = side / 2.0 - thickness;
            if r <= 0.0 {
                return;
            }
            // The arc glides to a new value over `ms` (0: it jumps); `key` says which ring.
            let frac = kit::glide(key, frac.clamp(0.0, 1.0), ms, window);
            let c = point(
                f32::from(bounds.origin.x) + f32::from(bounds.size.width) / 2.0,
                f32::from(bounds.origin.y) + f32::from(bounds.size.height) / 2.0,
            );
            let at = |turn: f32| {
                let a = turn * std::f32::consts::TAU;
                point(px(c.x + r * a.sin()), px(c.y - r * a.cos()))
            };
            let mut stroke = |from: f32, to: f32, color: Hsla| {
                let steps = (((to - from) * 96.0).ceil() as usize).max(2);
                let mut path = gpui::PathBuilder::stroke(px(thickness));
                path.move_to(at(from));
                for i in 1..=steps {
                    path.line_to(at(from + (to - from) * i as f32 / steps as f32));
                }
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            };
            stroke(0.0, 1.0, track);
            let frac = frac.clamp(0.0, 0.9999);
            if frac > 0.002 {
                stroke(0.0, frac, tint);
                // Round ends.
                let t = px(thickness);
                for p in [at(0.0), at(frac)] {
                    let dot =
                        gpui::Bounds::new(point(p.x - t / 2.0, p.y - t / 2.0), gpui::size(t, t));
                    window.paint_quad(gpui::fill(dot, tint).corner_radii(t / 2.0));
                }
            }
        },
    )
    .absolute()
    .size_full()
}

/// A round widget's face: a big icon over a caption (`tint` of `None` keeps a picture's own
/// colors).
pub fn round_face(
    icon: RoundIcon,
    tint: Hsla,
    caption: impl Into<SharedString>,
    width: f32,
    tk: &Tk,
) -> AnyElement {
    let caption: SharedString = caption.into();
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(2.0))
        .child(match icon {
            RoundIcon::Named(name) => kit::icon(name, width * 0.3, tint).into_any_element(),
            RoundIcon::Picture(pic) => pic.square(width * 0.3, false),
        })
        .when(!caption.is_empty(), |face| {
            face.child(
                div()
                    .max_w_full()
                    .truncate()
                    .text_size(px((width * 0.075).max(10.0)))
                    .text_color(tk.dim())
                    .child(caption),
            )
        })
        .into_any_element()
}

/// The icon of a round face.
pub enum RoundIcon {
    Named(&'static str),
    Picture(super::pics::Pic),
}

/// A status dot: accent with a halo while working. Deliberately still (see the Slint widget): any
/// animation here means a redraw several times a second.
pub fn status_dot(busy: bool, tk: &Tk) -> Div {
    div()
        .relative()
        .flex_none()
        .size(px(10.0))
        .rounded_full()
        .bg(if busy { tk.accent() } else { tk.good() })
        .when(busy, |dot| {
            dot.child(
                div()
                    .absolute()
                    .left(px(-4.5))
                    .top(px(-4.5))
                    .size(px(19.0))
                    .rounded_full()
                    .bg(with_alpha(tk.accent(), 0.22)),
            )
        })
}

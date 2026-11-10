//! The small pieces of the Wi-Fi and Bluetooth widgets: rows, buttons with a keyboard cursor
//! ring, switches, signal bars, the QR code, headers. Ports of `slint/ui/common.slint`'s
//! `Row`, `IconButton`, `TextButton`, `Switch`, `SignalBars`, `Header`, `RoundFace` and friends.
//!
//! The ones with an `st` argument can show the keyboard cursor (`kfocus`): a 2 px ring in the
//! accent color, drawn over the element so that the cursor moving never moves anything. The
//! element under the cursor says so to screen readers (`aria_cursor`).

use crate::kit::AriaCursor;
use crate::kit::AriaState;
use crate::kit::{self, Tk, with_alpha};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, ClickEvent, Div, ElementId, Hsla, Role, SharedString, Stateful,
    Toggled, Window, canvas, div, fill, point, px, size, transparent_black,
};
use niri_dash_system::IconSrc;

/// A line of text is as high as Slint draws it (the font's ascent and descent, about 1.36 times
/// its size), not GPUI's 1.6.
pub const LINE: f32 = kit::LINE;

/// `inner` with that line height for all the text in it.
pub fn lined(inner: AnyElement) -> AnyElement {
    kit::slint_lines(inner)
}

/// How a button, row or switch stands.
#[derive(Debug, Clone, Copy)]
pub struct St {
    pub enabled: bool,
    /// Drawn as on (a row of the network in use, a pressed toggle).
    pub active: bool,
    /// Under the keyboard cursor.
    pub kfocus: bool,
}

impl St {
    pub const fn new() -> Self {
        Self {
            enabled: true,
            active: false,
            kfocus: false,
        }
    }

    pub const fn cursor(kfocus: bool) -> Self {
        Self {
            kfocus,
            ..Self::new()
        }
    }

    pub const fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub const fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
}

impl Default for St {
    fn default() -> Self {
        Self::new()
    }
}

/// The keyboard cursor: a ring over an element of this corner radius, `outside` px outside it.
fn ring(tk: &Tk, radius: f32, outside: f32) -> Div {
    div()
        .absolute()
        .top(px(-outside))
        .left(px(-outside))
        .right(px(-outside))
        .bottom(px(-outside))
        .rounded(px(radius))
        .border_2()
        .border_color(tk.accent())
}

/// A listed thing: a network, a device, a way into another page. Say what it is with `label` and
/// how it stands with `value` ("connected, signal 78%"); the caller adds what is in it.
pub fn row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    st: St,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (label, value): (SharedString, SharedString) = (label.into(), value.into());
    let name = label.to_string();
    let (hover, pressed) = (tk.hover(), tk.pressed());
    let radius = tk.radius_small().min(18.0);
    div()
        .id(id)
        .role(Role::ListItem)
        .aria_label(label)
        .aria_state(value)
        .aria_cursor(st.kfocus)
        .debug_selector(move || name)
        .relative()
        .flex_none()
        .w_full()
        .h(px(36.0))
        .rounded(px(radius))
        .flex()
        .flex_row()
        .items_center()
        .bg(if st.active {
            tk.accent_soft()
        } else if st.kfocus {
            hover
        } else {
            transparent_black()
        })
        .when(st.enabled, |r| {
            r.cursor_pointer()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .when(!st.active && !st.kfocus, |r| r.hover(move |s| s.bg(hover)))
                // A selected row stays as it is (Slint: `selected` comes before `pressed`).
                .when(!st.active, |r| r.active(move |s| s.bg(pressed)))
                .on_click(on_click)
        })
        .when(!st.enabled, |r| r.opacity(0.4))
        .when(st.kfocus, |r| r.child(ring(tk, radius, 0.0)))
}

/// A square button with an icon.
#[allow(clippy::too_many_arguments)]
pub fn icon_button(
    id: impl Into<ElementId>,
    icon: &str,
    label: impl Into<SharedString>,
    side: f32,
    tint: Hsla,
    st: St,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let label: SharedString = label.into();
    let name = label.to_string();
    let (hover, pressed) = (tk.hover(), tk.pressed());
    let radius = tk.radius_small().min(side / 2.0);
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .aria_cursor(st.kfocus)
        .debug_selector(move || name)
        .relative()
        .flex_none()
        .size(px(side))
        .rounded(px(radius))
        .flex()
        .items_center()
        .justify_center()
        .bg(if st.active {
            tk.accent_soft()
        } else if st.kfocus {
            hover
        } else {
            transparent_black()
        })
        .when(st.enabled, |b| {
            b.cursor_pointer()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .hover(move |s| s.bg(hover))
                .active(move |s| s.bg(pressed))
                .on_click(move |ev, window, cx| {
                    // The row it may sit in must not take this click too.
                    cx.stop_propagation();
                    on_click(ev, window, cx);
                })
        })
        .when(!st.enabled, |b| b.opacity(0.4))
        .child(kit::icon(
            icon,
            side * 0.55,
            if st.active { tk.accent() } else { tint },
        ))
        .when(st.kfocus, |b| b.child(ring(tk, radius, 0.0)))
}

/// What a button is like besides how it stands.
#[derive(Debug, Clone, Copy, Default)]
pub struct Look {
    pub primary: bool,
    pub danger: bool,
    /// Tight padding, for a row of choices that must fit in a narrow widget.
    pub compact: bool,
}

impl Look {
    pub const PRIMARY: Self = Self {
        primary: true,
        danger: false,
        compact: false,
    };
    pub const DANGER: Self = Self {
        primary: false,
        danger: true,
        compact: false,
    };
}

/// A button with a label. `label` is what a screen reader says when it isn't the text itself.
pub fn text_button(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    label: Option<String>,
    look: Look,
    st: St,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let text: SharedString = text.into();
    let name = label.unwrap_or_else(|| text.to_string());
    let selector = name.clone();
    let radius = tk.radius_small().min(15.0);
    let (hover, pressed) = (tk.hover(), tk.pressed());
    let (accent, bad) = (tk.accent(), tk.bad());
    let (bg, fg) = if look.danger {
        (with_alpha(bad, 0.2), bad)
    } else if look.primary {
        (accent, tk.on_accent())
    } else {
        (tk.card_raised(), tk.text())
    };
    let pad = if look.compact { 4.0 } else { 12.0 };
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(name)
        .aria_cursor(st.kfocus)
        .debug_selector(move || selector)
        .relative()
        .h(px(30.0))
        // Slint's stretch: each takes its text's width, and the room left over is shared equally.
        .when(look.compact, |b| b.flex_auto().min_w_0())
        .when(!look.compact, |b| b.flex_none())
        .px(px(pad))
        .rounded(px(radius))
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .bg(bg)
        .text_color(fg)
        .text_size(px(tk.font_body()))
        .when(st.enabled, |b| {
            b.cursor_pointer()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .hover(move |s| {
                    s.bg(if look.danger {
                        with_alpha(bad, 0.33)
                    } else if look.primary {
                        accent.blend(gpui::white().opacity(0.1))
                    } else {
                        hover
                    })
                })
                .active(move |s| {
                    s.bg(if look.danger {
                        bg
                    } else if look.primary {
                        // Slint: `Theme.accent.darker(0.2)`.
                        Hsla {
                            l: accent.l / 1.2,
                            ..accent
                        }
                    } else {
                        pressed
                    })
                })
                .on_click(on_click)
        })
        .when(!st.enabled, |b| b.opacity(0.45))
        .child(div().truncate().child(text))
        .when(st.kfocus, |b| {
            // Over the fill, in the contrasting color for a filled button.
            b.child(ring(tk, radius, 0.0).border_color(if look.primary {
                tk.text()
            } else {
                accent
            }))
        })
}

/// An on/off switch that can show the keyboard cursor.
pub fn switch(
    id: impl Into<ElementId>,
    on: bool,
    label: impl Into<SharedString>,
    st: St,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let label: SharedString = label.into();
    let name = label.to_string();
    div()
        .id(id)
        .role(Role::Switch)
        .aria_label(label)
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .aria_cursor(st.kfocus)
        .debug_selector(move || name)
        .relative()
        .flex_none()
        .w(px(40.0))
        .h(px(22.0))
        .rounded(px(tk.radius_small().min(11.0)))
        .bg(if on { tk.accent() } else { tk.track() })
        .when(st.enabled, |s| {
            s.cursor_pointer()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(on_click)
        })
        .when(!st.enabled, |s| s.opacity(0.4))
        .child(
            div()
                .absolute()
                .top(px(3.0))
                .left(px(if on { 21.0 } else { 3.0 }))
                .size(px(16.0))
                .rounded(px((tk.radius_small() - 3.0).clamp(0.0, 8.0)))
                .bg(if on { tk.on_accent() } else { tk.dim() }),
        )
        .when(st.kfocus, |s| {
            // Outside the track, so it shows on the accent color of a switch that is on.
            s.child(ring(tk, tk.radius_small().min(11.0) + 3.0, 3.0))
        })
}

/// Signal strength, 0 to 100, as four bars.
pub fn signal_bars(signal: i32, tint: Hsla, tk: &Tk) -> Div {
    let track = tk.track();
    div()
        .relative()
        .flex_none()
        .w(px(18.0))
        .h(px(14.0))
        .children((0..4).map(move |i| {
            let h = (i + 1) as f32 * 3.5;
            div()
                .absolute()
                .left(px(i as f32 * 5.0))
                .top(px(14.0 - h))
                .w(px(3.0))
                .h(px(h))
                .rounded(px(1.0))
                .bg(if signal > i * 25 { tint } else { track })
        }))
}

/// Secondary text, one line.
pub fn muted(text: impl Into<SharedString>, tk: &Tk) -> Div {
    div()
        .text_size(px(tk.font_small()))
        .text_color(tk.dim())
        .truncate()
        .child(text.into())
}

/// Secondary text that wraps.
pub fn muted_wrap(text: impl Into<SharedString>, tk: &Tk) -> Div {
    div()
        .text_size(px(tk.font_small()))
        .text_color(tk.dim())
        .child(text.into())
}

/// What a page says when it has nothing to show: fills the room.
pub fn empty_fill(text: impl Into<SharedString>, tk: &Tk) -> AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .items_center()
        .justify_center()
        .text_center()
        .text_size(px(tk.font_body()))
        .text_color(tk.faint())
        .child(text.into())
        .into_any_element()
}

/// The same as a line in a list, `h` px high.
pub fn empty_line(text: impl Into<SharedString>, h: f32, tk: &Tk) -> AnyElement {
    div()
        .flex_none()
        .w_full()
        .h(px(h))
        .flex()
        .items_center()
        .justify_center()
        .text_center()
        .text_size(px(tk.font_body()))
        .text_color(tk.faint())
        .child(text.into())
        .into_any_element()
}

/// A line of secondary text in a list, `h` px high, centered or not.
pub fn muted_line(text: impl Into<SharedString>, h: f32, center: bool, tk: &Tk) -> AnyElement {
    div()
        .flex_none()
        .w_full()
        .h(px(h))
        .flex()
        .items_center()
        .when(center, |d| d.justify_center())
        .child(muted(text, tk))
        .into_any_element()
}

/// A small heading over a group of rows.
pub fn section_label(text: impl Into<SharedString>, h: f32, tk: &Tk) -> AnyElement {
    div()
        .flex_none()
        .w_full()
        .h(px(h))
        .flex()
        .items_end()
        .text_size(px(tk.font_small()))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(tk.faint())
        .child(div().truncate().child(text.into()))
        .into_any_element()
}

/// A widget's header: an icon, the title, and what the caller adds on the right.
pub fn header(icon: &str, icon_color: Hsla, title: &str, tk: &Tk) -> Div {
    div()
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .h(px(28.0))
        .child(kit::icon(icon, 18.0, icon_color))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(tk.font_title()))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(tk.text())
                .child(title.to_string()),
        )
}

/// The header of a page inside the widget: a back button and the title.
pub fn back_header(
    title: &str,
    tk: &Tk,
    on_back: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    div()
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .h(px(28.0))
        .child(icon_button(
            "back",
            "chevron-left",
            "Back",
            28.0,
            tk.text(),
            St::new(),
            tk,
            on_back,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(tk.font_title()))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(tk.text())
                .child(title.to_string()),
        )
}

/// One line of a details page: what it is, and its value on the right.
pub fn fact(label: &str, value: &str, tk: &Tk) -> AnyElement {
    div()
        .flex_none()
        .w_full()
        .h(px(24.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .w(gpui::relative(0.34))
                .flex_none()
                .truncate()
                .text_size(px(tk.font_small()))
                .text_color(tk.dim())
                .child(label.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_right()
                .text_size(px(tk.font_small()))
                .text_color(tk.text())
                .child(value.to_string()),
        )
        .into_any_element()
}

/// A line with a name on the left and a switch on the right ("Connect automatically").
pub fn switch_line(
    id: impl Into<ElementId>,
    text: &str,
    on: bool,
    st: St,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .flex_none()
        .w_full()
        .h(px(34.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(tk.font_body()))
                .text_color(tk.text())
                .child(text.to_string()),
        )
        .child(switch(id, on, text.to_string(), st, tk, on_click))
        .into_any_element()
}

/// A row of choices, one of them picked (security type, EAP method). `kfocus` is the cursor being
/// on the row: the picked one shows it.
pub fn choice(
    id: &str,
    label: &str,
    options: &[&'static str],
    picked: i32,
    kfocus: bool,
    tk: &Tk,
    pick: impl Fn(i32, &mut Window, &mut App) + Clone + 'static,
) -> AnyElement {
    div()
        .flex_none()
        .w_full()
        .flex()
        .flex_row()
        .gap(px(4.0))
        .children(options.iter().enumerate().map(|(i, o)| {
            let pick = pick.clone();
            let here = i as i32 == picked;
            let name = format!("{label}: {o}");
            text_button(
                SharedString::from(format!("{id}-{i}")),
                *o,
                Some(name),
                Look {
                    primary: here,
                    compact: true,
                    ..Look::default()
                },
                St::cursor(kfocus && here),
                tk,
                move |_, window, cx| pick(i as i32, window, cx),
            )
        }))
        .into_any_element()
}

/// A saved network's QR code, drawn as one square per module, `side` px (as Slint's pixelated
/// image is: modules a pixel wider or narrower here and there, edges on whole pixels, so that it
/// stays sharp). Without an image: an empty white square.
pub fn qr(image: &IconSrc, side: f32) -> AnyElement {
    let frame = |side: f32| {
        div()
            .id("qr")
            .role(Role::Image)
            .aria_label("QR code to join the network")
            .debug_selector(|| "QR code to join the network".into())
            .flex_none()
            .size(px(side))
            .bg(gpui::white())
    };
    let side = side.floor();
    let IconSrc::Rgba {
        width,
        height,
        data,
    } = image
    else {
        return frame(side).into_any_element();
    };
    let (w, h) = (*width as usize, *height as usize);
    if w == 0 || h == 0 || data.len() < w * h * 4 {
        return frame(side).into_any_element();
    }
    let cell = side / w.max(h) as f32;
    let data = data.clone();
    frame(side)
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let edge = |i: usize| (i as f32 * cell).round();
                    for y in 0..h {
                        let mut x = 0;
                        while x < w {
                            if data[(y * w + x) * 4] >= 128 {
                                x += 1;
                                continue;
                            }
                            let start = x;
                            while x < w && data[(y * w + x) * 4] < 128 {
                                x += 1;
                            }
                            let run = Bounds::new(
                                bounds.origin + point(px(edge(start)), px(edge(y))),
                                size(px(edge(x) - edge(start)), px(edge(y + 1) - edge(y))),
                            );
                            window.paint_quad(fill(run, gpui::black()));
                        }
                    }
                },
            )
            .size_full(),
        )
        .into_any_element()
}

/// What a round tile shows: a big icon, a title and a caption, on a soft disc while on. The
/// caller puts the click on it.
pub fn round_face(
    icon: &str,
    tint: Hsla,
    title: &str,
    caption: &str,
    on: bool,
    w: f32,
    tk: &Tk,
) -> Div {
    div()
        .size_full()
        .rounded_full()
        .bg(if on {
            tk.accent_soft()
        } else {
            transparent_black()
        })
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(2.0))
        .child(kit::icon(icon, w * 0.3, tint))
        .when(!title.is_empty(), |c| {
            c.child(
                div()
                    .max_w_full()
                    .px(px(4.0))
                    .truncate()
                    .text_center()
                    .text_size(px((w * 0.085).max(11.0)))
                    .text_color(tk.text())
                    .child(title.to_string()),
            )
        })
        .when(!caption.is_empty(), |c| {
            c.child(
                div()
                    .max_w_full()
                    .truncate()
                    .text_center()
                    .text_size(px((w * 0.075).max(10.0)))
                    .text_color(tk.dim())
                    .child(caption.to_string()),
            )
        })
}

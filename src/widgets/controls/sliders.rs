//! The controls' own pieces: the slider (mouse drag, wheel, keyboard through the widget, screen
//! reader steps), the slider row, the ring and the ring knob, a switch that is only drawn (its
//! row is the button), buttons that name what they do, a scrolling list with a scrollbar.
//!
//! Ports of `Slider`, `SliderRow`, `Ring`, `RingKnob`, `Switch`, `IconButton` and `ScrollList`
//! of `slint/ui/common.slint`. They are the group's first, in its own module (PORTING.md); if
//! another group wants one it moves to `kit`.

use crate::kit::AriaCursor;
use crate::kit::{self, Tk};
use gpui::prelude::*;
use gpui::{
    AccessibleAction, App, BorderStyle, Bounds, ClickEvent, DragMoveEvent, ElementId, Empty, Hsla,
    MouseButton, MouseDownEvent, PathBuilder, Pixels, Render, Role, ScrollEvent, ScrollHandle,
    SharedString, Stateful, Toggled, Window, canvas, div, point, px, quad, size,
};
use std::cell::Cell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

/// The knob's diameter, and so how far the slider's ends are from its value.
const KNOB: f32 = 16.0;
/// How far a turn of the wheel moves a slider or knob: this many pixels are all of it.
const WHEEL_SPAN: f32 = 960.0;
/// What one notch of a wheel that scrolls by lines is worth, in pixels.
const NOTCH: f32 = 60.0;
/// How far a key or a screen reader's step moves a slider.
pub const STEP: f32 = 0.05;

/// A value as the percentage the widgets say.
pub fn percent(v: f32) -> String {
    format!("{}%", (v.clamp(0.0, 1.0) * 100.0).round() as i32)
}

/// A lighter tint of `c`.
fn lighter(c: Hsla, by: f32) -> Hsla {
    Hsla {
        l: (c.l + by).min(1.0),
        ..c
    }
}

/// What a drag of a slider carries: which slider it is.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Dragging(u64);

impl Render for Dragging {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        Empty
    }
}

fn key_of(id: (&'static str, usize)) -> u64 {
    let mut h = DefaultHasher::new();
    id.hash(&mut h);
    h.finish()
}

/// The value the wheel adds for one event.
fn wheel_delta(ev: &ScrollEvent) -> f32 {
    let d = ev.delta.pixel_delta(px(NOTCH));
    (f32::from(d.y) + f32::from(d.x)) / WHEEL_SPAN
}

#[allow(clippy::too_many_arguments)]
/// A horizontal slider, 24 px high, as wide as its parent gives it. `on_change` gets the new value
/// (0 to 1) from a press or drag on it, the wheel, and a screen reader's steps. It says nothing
/// to the keyboard: the widget's scope does that, and shows its cursor through `kfocus`.
pub fn slider(
    id: (&'static str, usize),
    label: impl Into<SharedString>,
    value: f32,
    tint: Hsla,
    enabled: bool,
    kfocus: bool,
    tk: &Tk,
    on_change: impl Fn(f32, &mut Window, &mut App) + 'static,
) -> Stateful<gpui::Div> {
    let label: SharedString = label.into();
    let selector = label.to_string();
    let value = value.clamp(0.0, 1.0);
    let bounds: Rc<Cell<Bounds<Pixels>>> = Rc::default();
    let on_change = Rc::new(on_change);
    let key = key_of(id);
    let from_x = |bounds: Bounds<Pixels>, x: Pixels| -> f32 {
        let span = (f32::from(bounds.size.width) - KNOB).max(1.0);
        ((f32::from(x - bounds.left()) - KNOB / 2.0) / span).clamp(0.0, 1.0)
    };
    let (track, accent, knob, ring) = (tk.track(), tint, tk.knob(), tk.accent());
    let ms = 100.0 * tk.motion();
    let paint_bounds = bounds.clone();
    let surface = canvas(
        move |b, _, _| {
            paint_bounds.set(b);
        },
        move |b, _, window, cx| {
            let mid = b.center().y;
            let width = f32::from(b.size.width);
            // It glides to a new value (a key step, a change from elsewhere), but follows the
            // pointer without lag while it drags.
            let shown = kit::glide(
                key,
                value,
                if cx.has_active_drag() { 0.0 } else { ms },
                window,
            );
            window.paint_quad(quad(
                Bounds::new(point(b.left(), mid - px(3.0)), size(b.size.width, px(6.0))),
                px(3.0),
                track,
                0.0,
                gpui::transparent_black(),
                BorderStyle::default(),
            ));
            let x = shown * (width - KNOB);
            window.paint_quad(quad(
                Bounds::new(
                    point(b.left(), mid - px(3.0)),
                    size(px(x + KNOB / 2.0), px(6.0)),
                ),
                px(3.0),
                accent,
                0.0,
                gpui::transparent_black(),
                BorderStyle::default(),
            ));
            let lit = b.contains(&window.mouse_position()) || cx.has_active_drag();
            window.paint_quad(quad(
                Bounds::new(
                    point(b.left() + px(x), mid - px(KNOB / 2.0)),
                    size(px(KNOB), px(KNOB)),
                ),
                px(KNOB / 2.0),
                if lit { lighter(knob, 0.1) } else { knob },
                if kfocus { 3.0 } else { 0.0 },
                ring,
                BorderStyle::default(),
            ));
        },
    )
    .absolute()
    .size_full();
    let mut root = div()
        .id(ElementId::from(id))
        .role(Role::Slider)
        .aria_label(label)
        .aria_value(percent(value))
        .aria_numeric_value(f64::from((value * 100.0).round()))
        .aria_min_numeric_value(0.0)
        .aria_max_numeric_value(100.0)
        .aria_numeric_value_step(5.0)
        .aria_cursor(kfocus)
        .debug_selector(move || selector.clone())
        .relative()
        .flex_none()
        .h(px(24.0))
        .child(surface);
    if !enabled {
        return root.opacity(0.4);
    }
    let on = on_change.clone();
    let b = bounds.clone();
    root = root
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, move |ev: &MouseDownEvent, window, cx| {
            cx.stop_propagation();
            on(from_x(b.get(), ev.position.x), window, cx);
        })
        .on_drag(Dragging(key), |d, _, _, cx| {
            cx.stop_propagation();
            cx.new(|_| *d)
        });
    let on = on_change.clone();
    root = root.on_drag_move(move |ev: &DragMoveEvent<Dragging>, window, cx| {
        if *ev.drag(cx) == Dragging(key) {
            on(from_x(ev.bounds, ev.event.position.x), window, cx);
        }
    });
    let on = on_change.clone();
    root = root.on_scroll(move |ev: &ScrollEvent, window, cx| {
        cx.stop_propagation();
        on((value + wheel_delta(ev)).clamp(0.0, 1.0), window, cx);
    });
    let (up, down) = (on_change.clone(), on_change);
    root.on_a11y_action(AccessibleAction::Increment, move |_, window, cx| {
        up((value + STEP).clamp(0.0, 1.0), window, cx);
    })
    .on_a11y_action(AccessibleAction::Decrement, move |_, window, cx| {
        down((value - STEP).clamp(0.0, 1.0), window, cx);
    })
}

#[allow(clippy::too_many_arguments)]
/// A button with an icon and a name, for what has no label of its own (mute, unmute).
pub fn icon_button(
    id: (&'static str, usize),
    icon: &str,
    label: impl Into<SharedString>,
    size: f32,
    tint: Hsla,
    kfocus: bool,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<gpui::Div> {
    let label: SharedString = label.into();
    let selector = label.to_string();
    let (hover, pressed, ring) = (tk.hover(), tk.pressed(), tk.accent());
    div()
        .id(ElementId::from(id))
        .role(Role::Button)
        .aria_label(label)
        .aria_cursor(kfocus)
        .debug_selector(move || selector.clone())
        .flex_none()
        .size(px(size))
        .rounded(px(tk.radius_small().min(size / 2.0)))
        .border(px(2.0))
        .border_color(if kfocus {
            ring
        } else {
            gpui::transparent_black()
        })
        .when(kfocus, |b| b.bg(hover))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .active(move |s| s.bg(pressed))
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(on_click)
        .child(kit::icon(icon, size * 0.55, tint))
}

/// What a row of slider and icon is made of.
pub struct Row {
    pub id: (&'static str, usize),
    /// What the slider sets ("Volume").
    pub label: SharedString,
    pub icon: &'static str,
    /// What the icon button does ("Mute"); none: the icon is only a picture.
    pub icon_label: Option<SharedString>,
    pub icon_tint: Hsla,
    pub icon_box: f32,
    pub tint: Hsla,
    pub enabled: bool,
    pub value: f32,
    /// The keyboard cursor is on the slider.
    pub kfocus: bool,
    /// ... or on the icon.
    pub kfocus_icon: bool,
}

/// An icon, a slider and the value in percent.
pub fn slider_row(
    row: Row,
    tk: &Tk,
    on_change: impl Fn(f32, &mut Window, &mut App) + 'static,
    on_icon: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::Div {
    let Row {
        id,
        label,
        icon,
        icon_label,
        icon_tint,
        icon_box,
        tint,
        enabled,
        value,
        kfocus,
        kfocus_icon,
    } = row;
    let lead = match icon_label {
        Some(name) => icon_button(
            (id.0, id.1 + 1),
            icon,
            name,
            icon_box,
            icon_tint,
            kfocus_icon,
            tk,
            on_icon,
        )
        .into_any_element(),
        None => div()
            .flex_none()
            .size(px(icon_box))
            .flex()
            .items_center()
            .justify_center()
            .child(kit::icon(icon, icon_box * 0.55, icon_tint))
            .into_any_element(),
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .h(px(icon_box.max(24.0)))
        .w_full()
        .child(lead)
        .child(
            slider(id, label, value, tint, enabled, kfocus, tk, on_change)
                .flex_1()
                .min_w_0(),
        )
        .child(
            div()
                .flex_none()
                .w(px(38.0))
                .text_right()
                .text_size(px(tk.font_small()))
                .text_color(tk.dim())
                .child(percent(value)),
        )
}

/// A ring: an arc from 12 o'clock clockwise, `value` (0 to 1) of the way round, over a track.
pub fn paint_ring(
    window: &mut Window,
    b: Bounds<Pixels>,
    value: f32,
    thickness: f32,
    tint: Hsla,
    track: Hsla,
) {
    let side = f32::from(b.size.width).min(f32::from(b.size.height));
    // The Slint ring's stroke sits a whole thickness in from the edge of its box.
    let r = (side / 2.0 - thickness).max(1.0);
    let c = b.center();
    let top = point(c.x, c.y - px(r));
    let radii = point(px(r), px(r));
    let mut full = PathBuilder::stroke(px(thickness));
    full.move_to(top);
    full.arc_to(radii, px(0.0), false, true, point(c.x, c.y + px(r)));
    full.arc_to(radii, px(0.0), false, true, top);
    if let Ok(path) = full.build() {
        window.paint_path(path, track);
    }
    let v = value.clamp(0.0, 0.9999);
    if v <= 0.002 {
        return;
    }
    let angle = v * std::f32::consts::TAU;
    let end = point(c.x + px(r * angle.sin()), c.y - px(r * angle.cos()));
    let mut arc = PathBuilder::stroke(px(thickness));
    arc.move_to(top);
    arc.arc_to(radii, px(0.0), v > 0.5, true, end);
    if let Ok(path) = arc.build() {
        window.paint_path(path, tint);
    }
    // Round ends.
    for at in [top, end] {
        window.paint_quad(quad(
            Bounds::new(
                point(at.x - px(thickness / 2.0), at.y - px(thickness / 2.0)),
                size(px(thickness), px(thickness)),
            ),
            px(thickness / 2.0),
            tint,
            0.0,
            gpui::transparent_black(),
            BorderStyle::default(),
        ));
    }
}

/// A ring that fills its parent, as a gauge: `label` says what it measures.
pub fn ring(
    id: (&'static str, usize),
    label: impl Into<SharedString>,
    spoken: impl Into<SharedString>,
    value: f32,
    thickness: f32,
    tint: Hsla,
    tk: &Tk,
) -> Stateful<gpui::Div> {
    let track = tk.track();
    let value = value.clamp(0.0, 1.0);
    // Gauges glide to a new value.
    let key = key_of(id) ^ 0x5151;
    let ms = 400.0 * tk.motion();
    let label: SharedString = label.into();
    let selector = label.to_string();
    div()
        .id(ElementId::from(id))
        .role(Role::ProgressIndicator)
        .aria_label(label)
        .debug_selector(move || selector.clone())
        .aria_value(spoken)
        .aria_numeric_value(f64::from((value * 100.0).round()))
        .aria_min_numeric_value(0.0)
        .aria_max_numeric_value(100.0)
        .absolute()
        .size_full()
        .child(
            canvas(
                |_, _, _| {},
                move |b, _, window, _| {
                    let shown = kit::glide(key, value, ms, window);
                    paint_ring(window, b, shown, thickness, tint, track)
                },
            )
            .absolute()
            .size_full(),
        )
}

/// What a round widget's knob shows.
pub struct Knob {
    pub id: (&'static str, usize),
    /// What it sets ("Volume").
    pub name: SharedString,
    pub value: f32,
    pub tint: Hsla,
    pub icon: &'static str,
    /// What it shows under the icon.
    pub label: String,
    pub dimmed: bool,
    pub kfocus: bool,
    /// The widget's side, in pixels.
    pub side: f32,
}

/// A round knob: a ring showing the value, the icon and a label inside. The wheel changes the
/// value, a click calls `on_click`.
pub fn ring_knob(
    knob: Knob,
    tk: &Tk,
    on_change: impl Fn(f32, &mut Window, &mut App) + 'static,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<gpui::Div> {
    let Knob {
        id,
        name,
        value,
        tint,
        icon,
        label,
        dimmed,
        kfocus,
        side,
    } = knob;
    let value = value.clamp(0.0, 1.0);
    let selector = name.to_string();
    let on_change = Rc::new(on_change);
    let (up, down, wheel) = (on_change.clone(), on_change.clone(), on_change);
    let tint = if dimmed { tk.faint() } else { tint };
    div()
        .id(ElementId::from(id))
        .role(Role::Slider)
        .aria_label(name)
        .aria_value(percent(value))
        .aria_numeric_value(f64::from((value * 100.0).round()))
        .aria_min_numeric_value(0.0)
        .aria_max_numeric_value(100.0)
        .aria_numeric_value_step(5.0)
        .aria_cursor(kfocus)
        .debug_selector(move || selector.clone())
        .relative()
        .size_full()
        .rounded_full()
        .when(kfocus, |d| d.bg(tk.hover()))
        .cursor_pointer()
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(on_click)
        .on_scroll(move |ev: &ScrollEvent, window, cx| {
            cx.stop_propagation();
            wheel((value + wheel_delta(ev)).clamp(0.0, 1.0), window, cx);
        })
        .on_a11y_action(AccessibleAction::Increment, move |_, window, cx| {
            up((value + STEP).clamp(0.0, 1.0), window, cx);
        })
        .on_a11y_action(AccessibleAction::Decrement, move |_, window, cx| {
            down((value - STEP).clamp(0.0, 1.0), window, cx);
        })
        .child(
            canvas(|_, _, _| {}, {
                let track = tk.track();
                let thickness = (side * 0.07).max(6.0);
                let key = key_of(id) ^ 0xA0A0;
                let ms = 100.0 * tk.motion();
                move |b, _, window, _| {
                    let shown = kit::glide(key, value, ms, window);
                    paint_ring(window, b, shown, thickness, tint, track)
                }
            })
            .absolute()
            .size_full(),
        )
        .child(
            div()
                .absolute()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(2.0))
                .child(kit::icon(
                    icon,
                    (side * 0.28).min(40.0),
                    if dimmed { tk.faint() } else { tk.text() },
                ))
                .child(
                    div()
                        .text_size(px((side * 0.11).clamp(11.0, 18.0)))
                        .text_color(tk.dim())
                        .child(label),
                ),
        )
}

/// A switch that is only drawn: the row or button around it is what is clicked.
pub fn switch_view(key: u64, on: bool, enabled: bool, kfocus: bool, tk: &Tk) -> gpui::Div {
    let radius = tk.radius_small().min(11.0);
    let thumb_radius = (tk.radius_small() - 3.0).clamp(0.0, 8.0);
    let (accent, track, on_accent, dim) = (tk.accent(), tk.track(), tk.on_accent(), tk.dim());
    let ms = 120.0 * tk.motion();
    // The track changes color and the knob slides over `ms`, as Slint's switch does.
    let surface = canvas(
        |_, _, _| {},
        move |b, _, window, _| {
            let t = kit::glide(key, if on { 1.0 } else { 0.0 }, ms, window);
            let mix = |a: Hsla, z: Hsla| -> Hsla {
                let (a, z) = (gpui::Rgba::from(a), gpui::Rgba::from(z));
                gpui::Rgba {
                    r: a.r + (z.r - a.r) * t,
                    g: a.g + (z.g - a.g) * t,
                    b: a.b + (z.b - a.b) * t,
                    a: a.a + (z.a - a.a) * t,
                }
                .into()
            };
            window.paint_quad(quad(
                b,
                px(radius),
                mix(track, accent),
                0.0,
                gpui::transparent_black(),
                BorderStyle::default(),
            ));
            window.paint_quad(quad(
                Bounds::new(
                    point(b.left() + px(3.0 + 18.0 * t), b.top() + px(3.0)),
                    size(px(16.0), px(16.0)),
                ),
                px(thumb_radius),
                if on { on_accent } else { dim },
                0.0,
                gpui::transparent_black(),
                BorderStyle::default(),
            ));
        },
    )
    .absolute()
    .size_full();
    div()
        .flex_none()
        .relative()
        .w(px(40.0))
        .h(px(22.0))
        .child(surface)
        .when(!enabled, |s| s.opacity(0.4))
        .when(kfocus, |s| {
            // Outside the track, so that it shows on the accent of a switch that is on.
            s.child(
                div()
                    .absolute()
                    .top(px(-3.0))
                    .left(px(-3.0))
                    .w(px(46.0))
                    .h(px(28.0))
                    .rounded(px((radius + 3.0).min(14.0)))
                    .border(px(2.0))
                    .border_color(tk.accent()),
            )
        })
}

/// A switch with a name, which is the whole of the control: it flips on a click anywhere in it.
pub fn switch_button(
    id: (&'static str, usize),
    label: &'static str,
    on: bool,
    enabled: bool,
    kfocus: bool,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<gpui::Div> {
    let b = div()
        .id(ElementId::from(id))
        .role(Role::Switch)
        .aria_label(label)
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .aria_cursor(kfocus)
        .debug_selector(move || label.to_string())
        .flex_none()
        .child(switch_view(key_of(id) ^ 0x5717, on, enabled, kfocus, tk));
    if enabled {
        b.cursor_pointer()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(on_click)
    } else {
        b
    }
}

/// A column that scrolls, with a scrollbar in its own gutter that shows when there is something
/// to scroll to. `handle` is the widget's, to scroll the row under the cursor into view.
pub fn scroll_list(
    id: (&'static str, usize),
    handle: &ScrollHandle,
    rows: Vec<gpui::AnyElement>,
    tk: &Tk,
) -> gpui::Div {
    let (subtle, faint) = (tk.subtle(), tk.faint());
    let for_bar = handle.clone();
    // The bar sits in the card's padding, beside the rows (so they are as wide as the header
    // above them); a tight padding has no room for it and keeps a gutter of 10 px.
    let in_padding = tk.pad() >= 8.0;
    let beside = if in_padding {
        (tk.pad() + 4.0) / 2.0
    } else {
        -1.0
    };
    div()
        .relative()
        .flex_1()
        .min_h_0()
        .w_full()
        .child(
            div()
                .id(ElementId::from(id))
                .size_full()
                .when(!in_padding, |l| l.pr(px(10.0)))
                .overflow_y_scroll()
                .track_scroll(handle)
                .flex()
                .flex_col()
                .children(rows),
        )
        .child(
            canvas(
                |_, _, _| {},
                move |b, _, window, _| {
                    let content_over = f32::from(for_bar.max_offset().y);
                    if content_over <= 1.0 {
                        return;
                    }
                    let h = f32::from(b.size.height);
                    let bar = Bounds::new(
                        point(b.right() - px(4.0), b.top()),
                        size(px(4.0), b.size.height),
                    );
                    window.paint_quad(quad(
                        bar,
                        px(2.0),
                        subtle,
                        0.0,
                        gpui::transparent_black(),
                        BorderStyle::default(),
                    ));
                    let thumb = (h * h / (h + content_over)).max(24.0).min(h);
                    let at = (h - thumb)
                        * (-f32::from(for_bar.offset().y) / content_over).clamp(0.0, 1.0);
                    window.paint_quad(quad(
                        Bounds::new(
                            point(b.right() - px(4.0), b.top() + px(at)),
                            size(px(4.0), px(thumb)),
                        ),
                        px(2.0),
                        faint,
                        0.0,
                        gpui::transparent_black(),
                        BorderStyle::default(),
                    ));
                },
            )
            .absolute()
            .top_0()
            .right(px(-beside))
            .h_full()
            .w(px(4.0)),
        )
}

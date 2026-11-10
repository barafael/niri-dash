//! The small pieces the time widgets share: buttons with a keyboard cursor, switches, bars, dots,
//! the card's size, and the per-widget keyboard scopes.

use crate::dash::Dash;
use crate::kit::{self, Tk, with_alpha};
use crate::scope::{Scope, Stops};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, Div, ElementId, Hsla, Role, SharedString, Stateful, Window, div,
    px,
};
use std::cell::RefCell;
use std::collections::BTreeMap;
use crate::kit::AriaCursor;

/// The card of widget `id` in pixels, as `ui::frame` draws it, and whether it is drawn round. The
/// keys need the size the widget is drawn at (which stops there are depends on it).
pub fn card(d: &Dash, id: u32) -> Option<(f32, f32, bool)> {
    let tile = d.tile(id)?;
    let (area_w, area_h) = d.size;
    if area_w <= 0.0 || area_h <= 0.0 {
        return None;
    }
    let g = d.geometry(area_w, area_h);
    let r = tile.rect();
    let expanded = d.nav.expanded_id == id;
    let (w, h) = if expanded {
        (
            area_w.min((r.w as f32 * g.cell).max(area_w * 0.55).max(8.0 * g.cell)),
            area_h.min((r.h as f32 * g.cell).max(area_h * 0.8).max(6.0 * g.cell)),
        )
    } else {
        (r.w as f32 * g.cell, r.h as f32 * g.cell)
    };
    let gap = d.look.style.gap;
    let (cw, ch) = (w - 2.0 * gap, h - 2.0 * gap);
    let round = tile.round() && !expanded;
    Some(if round {
        let diameter = cw.min(ch);
        (diameter, diameter, true)
    } else {
        (cw, ch, false)
    })
}

/// Stops that are all there is to them: nothing to skip.
pub struct Nothing;
impl Stops for Nothing {}

/// A keyboard scope per widget instance (each has its own cursor; the state they work on is
/// shared by all of a kind). Drawing keeps the cursor on a stop that exists, as the stops come and
/// go, so they are cells.
#[derive(Default)]
pub struct Scopes(RefCell<BTreeMap<u32, Scope>>);

impl Scopes {
    pub fn get(&self, id: u32) -> Scope {
        self.0.borrow().get(&id).copied().unwrap_or_default()
    }

    pub fn set(&self, id: u32, scope: Scope) {
        self.0.borrow_mut().insert(id, scope);
    }

    /// The widget has `count` stops now, and the stop under the cursor `subs(cursor)` parts: the
    /// cursor goes back onto one that exists. Returns the scope.
    pub fn fit(&self, id: u32, count: usize, subs: impl Fn(usize) -> usize) -> Scope {
        let mut scope = self.get(id);
        scope.set_count(count, &Nothing);
        scope.set_subs(subs(scope.cursor));
        self.set(id, scope);
        scope
    }

    /// Keep the scopes of the widgets that are still there.
    pub fn retain(&self, ids: &[u32]) {
        self.0.borrow_mut().retain(|id, _| ids.contains(id));
    }
}

/// How a button looks.
#[derive(Clone, Copy)]
pub struct Face {
    pub fill: Option<Hsla>,
    pub tint: Option<Hsla>,
    pub enabled: bool,
    /// Under the keyboard cursor.
    pub kfocus: bool,
}

impl Default for Face {
    fn default() -> Self {
        Self {
            fill: None,
            tint: None,
            enabled: true,
            kfocus: false,
        }
    }
}

impl Face {
    pub fn fill(mut self, c: Hsla) -> Self {
        self.fill = Some(c);
        self
    }

    pub fn tint(mut self, c: Hsla) -> Self {
        self.tint = Some(c);
        self
    }

    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }

    pub fn kfocus(mut self, on: bool) -> Self {
        self.kfocus = on;
        self
    }
}

/// A square button with an icon. `label` says what it does to screen readers and the tests.
pub fn icon_button(
    id: impl Into<ElementId>,
    icon: &str,
    label: impl Into<SharedString>,
    size: f32,
    face: Face,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let label: SharedString = label.into();
    let name = label.to_string();
    let (hover, pressed, accent) = (tk.hover(), tk.pressed(), tk.accent());
    let tint = face.tint.unwrap_or_else(|| tk.text());
    let button = div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .aria_cursor(face.kfocus)
        .debug_selector(move || name)
        .occlude()
        .flex_none()
        .size(px(size))
        .rounded(px(tk.radius_small().min(size / 2.0)))
        .flex()
        .items_center()
        .justify_center()
        .when_some(face.fill, |b, fill| b.bg(fill))
        .when(face.kfocus, |b| b.bg(hover).border_2().border_color(accent))
        .when(!face.enabled, |b| b.opacity(0.4))
        .when(face.enabled, |b| {
            b.cursor_pointer()
                .hover(move |s| s.bg(hover))
                .active(move |s| s.bg(pressed))
                .on_click(on_click)
        })
        .child(kit::icon(icon, size * 0.55, tint));
    button.into_any_element()
}

/// A button with a text. `name` is what it is called for screen readers, if that isn't the text.
pub fn text_button(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    name: Option<String>,
    primary: bool,
    face: Face,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let text: SharedString = text.into();
    let name = name.unwrap_or_else(|| text.to_string());
    let (hover, pressed, accent) = (tk.hover(), tk.pressed(), tk.accent());
    let button = div()
        .id(id)
        .role(Role::Button)
        .aria_label(name.clone())
        .aria_cursor(face.kfocus)
        .debug_selector(move || name)
        .occlude()
        .flex_none()
        .h(px(30.0))
        .px(px(12.0))
        .rounded(px(tk.radius_small().min(15.0)))
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(tk.font_body()))
        .whitespace_nowrap()
        .bg(if primary { tk.accent() } else { tk.card_raised() })
        .text_color(if primary { tk.on_accent() } else { tk.text() })
        .when(face.kfocus, |b| {
            b.border_2()
                .border_color(if primary { tk.text() } else { accent })
        })
        .when(!face.enabled, |b| b.opacity(0.45))
        .when(face.enabled, |b| {
            b.cursor_pointer()
                .hover(move |s| if primary { s } else { s.bg(hover) })
                .active(move |s| if primary { s } else { s.bg(pressed) })
                .on_click(on_click)
        })
        .child(text);
    button.into_any_element()
}

/// An on/off switch with a keyboard cursor ring around it.
pub fn switch(
    id: impl Into<ElementId>,
    on: bool,
    label: &'static str,
    kfocus: bool,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Switch)
        .aria_label(label)
        .aria_toggled(if on {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .aria_cursor(kfocus)
        .debug_selector(move || label.to_string())
        .occlude()
        .flex_none()
        .relative()
        .w(px(40.0))
        .h(px(22.0))
        .rounded(px((tk.radius_small()).min(11.0)))
        .bg(if on { tk.accent() } else { tk.track() })
        .cursor_pointer()
        .on_click(on_click)
        .when(kfocus, |s| {
            s.child(
                div()
                    .absolute()
                    .left(px(-3.0))
                    .top(px(-3.0))
                    .w(px(46.0))
                    .h(px(28.0))
                    .rounded(px((tk.radius_small() + 3.0).min(14.0)))
                    .border_2()
                    .border_color(tk.accent()),
            )
        })
        .child(
            div()
                .absolute()
                .top(px(3.0))
                .left(px(if on { 21.0 } else { 3.0 }))
                .size(px(16.0))
                .rounded(px((tk.radius_small() - 3.0).clamp(0.0, 8.0)))
                .bg(if on { tk.on_accent() } else { tk.dim() }),
        )
}

/// A thin progress bar `frac` (0 to 1) full, for screen readers a gauge called `label`.
pub fn bar(id: &'static str, label: &'static str, frac: f32, tint: Hsla, tk: &Tk) -> Stateful<Div> {
    let frac = frac.clamp(0.0, 1.0);
    div()
        .id(id)
        .role(Role::ProgressIndicator)
        .aria_label(label)
        .aria_value(format!("{}%", (frac * 100.0).round() as i32))
        .aria_min_numeric_value(0.0)
        .aria_max_numeric_value(100.0)
        .aria_numeric_value((frac * 100.0).round() as f64)
        .flex_none()
        .w_full()
        .h(px(4.0))
        .rounded_full()
        .bg(tk.track())
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(gpui::relative(frac))
                .rounded_full()
                .bg(tint),
        )
}

/// One dot per session of a round: filled for those done, ringed for the one in hand.
pub fn dots(rounds: u32, done: u32, on_break: bool, size: f32, tint: Hsla, tk: &Tk) -> Div {
    let track = tk.track();
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(size * 0.7))
        .children((0..rounds).map(move |i| {
            let filled = i < done;
            div()
                .flex_none()
                .size(px(size))
                .rounded_full()
                .when(filled, |d| d.bg(tint))
                .when(!filled, |d| {
                    d.border(px(1.5)).border_color(if !on_break && i == done {
                        tint
                    } else {
                        track
                    })
                })
        }))
}

/// Text that is cut with an ellipsis when it doesn't fit.
pub fn elided(text: impl Into<SharedString>) -> Div {
    div().min_w_0().truncate().child(text.into())
}

/// Dimmed small text (`Muted`).
pub fn muted(text: impl Into<SharedString>, tk: &Tk) -> Div {
    elided(text)
        .text_size(px(tk.font_small()))
        .text_color(tk.dim())
}

/// The name of a weather icon in `assets/icons`.
pub fn weather_icon(icon: niri_dash_system::weather::Icon) -> &'static str {
    use niri_dash_system::weather::Icon;
    match icon {
        Icon::Sun => "sun",
        Icon::Moon => "moon",
        Icon::CloudSun => "cloud-sun",
        Icon::CloudMoon => "cloud-moon",
        Icon::Cloud => "cloud",
        Icon::Fog => "fog",
        Icon::Rain => "rain",
        Icon::Snow => "snow",
        Icon::Storm => "storm",
    }
}

/// `c` at `alpha` of its own.
pub fn faded(c: Hsla, alpha: f32) -> Hsla {
    with_alpha(c, c.a * alpha)
}

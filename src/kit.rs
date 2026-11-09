//! The small pieces widgets are made of: the look's colors and sizes as GPUI wants them (`Tk`),
//! icons, buttons, switches, bars, headers. The ports of `common.slint`'s components.

use crate::theme::{Color, ColorRole, Look};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, ElementId, Hsla, Pixels, Role, SharedString, Window, div, px, svg,
};

/// The current look, for the views that draw with it.
#[derive(Debug, Clone)]
pub struct Tk {
    pub look: Look,
}

macro_rules! colors {
    ($($name:ident => $role:ident),* $(,)?) => {
        impl Tk {
            $(pub fn $name(&self) -> Hsla { self.c(ColorRole::$role).hsla() })*
        }
    };
}

colors! {
    card => Card,
    card_raised => CardRaised,
    panel => Panel,
    hover => Hover,
    pressed => Pressed,
    border => Border,
    subtle => Subtle,
    shadow => Shadow,
    text => Text,
    dim => Dim,
    faint => Faint,
    accent => Accent,
    accent_soft => AccentSoft,
    on_accent => OnAccent,
    good => Good,
    warn => Warn,
    bad => Bad,
    track => Track,
    knob => Knob,
}

impl Tk {
    pub fn new(look: &Look) -> Self {
        Self { look: look.clone() }
    }

    pub fn c(&self, role: ColorRole) -> Color {
        self.look.color(role)
    }

    pub fn gap(&self) -> f32 {
        self.look.style.gap
    }

    /// The padding inside a widget.
    pub fn pad(&self) -> f32 {
        self.look.style.padding
    }

    /// For widgets whose content wants the room, and for cards inside a widget.
    pub fn pad_tight(&self) -> f32 {
        (self.look.style.padding * 0.6).max(6.0)
    }

    pub fn radius(&self) -> f32 {
        self.look.style.radius
    }

    pub fn radius_small(&self) -> f32 {
        self.look.style.radius_small
    }

    pub fn font_small(&self) -> f32 {
        self.look.style.font_small
    }

    pub fn font_body(&self) -> f32 {
        self.look.style.font_body
    }

    pub fn font_title(&self) -> f32 {
        self.look.style.font_title
    }

    /// The weight of the big numbers (the clock).
    pub fn display_weight(&self) -> gpui::FontWeight {
        gpui::FontWeight(self.look.style.font_weight_display as f32)
    }

    /// Animation speed: 1 is normal, 0 turns animations off.
    pub fn motion(&self) -> f32 {
        self.look.style.motion
    }
}

/// A tint of `c` at `alpha`.
pub fn with_alpha(c: Hsla, alpha: f32) -> Hsla {
    Hsla { a: alpha, ..c }
}

/// An icon (`assets/icons/<name>.svg`), `size` px, drawn in `color`.
pub fn icon(name: &str, size: f32, color: Hsla) -> gpui::Svg {
    svg()
        .path(crate::assets::icon_path(name))
        .size(px(size))
        .flex_none()
        .text_color(color)
}

/// A square button with an icon, which says what it does to screen readers (and the tests, which
/// find it by that name).
pub fn icon_button(
    id: impl Into<ElementId>,
    name: &str,
    label: &'static str,
    size: f32,
    tint: Hsla,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let (hover, pressed) = (tk.hover(), tk.pressed());
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .debug_selector(move || label.to_string())
        .flex_none()
        .size(px(size))
        .rounded(px(tk.radius_small().min(size / 2.0)))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .active(move |s| s.bg(pressed))
        .on_click(on_click)
        .child(icon(name, size * 0.52, tint))
}

/// A button with a label, `height` px high; `primary` fills it with the accent.
pub fn text_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    primary: bool,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let label: SharedString = label.into();
    let name = label.to_string();
    let (hover, pressed) = (tk.hover(), tk.pressed());
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .debug_selector(move || name)
        .h(px(32.0))
        .px(px(14.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(tk.radius_small()))
        .cursor_pointer()
        .when(primary, |b| b.bg(tk.accent()).text_color(tk.on_accent()))
        .when(!primary, |b| {
            b.bg(tk.subtle())
                .text_color(tk.text())
                .hover(move |s| s.bg(hover))
                .active(move |s| s.bg(pressed))
        })
        .on_click(on_click)
        .child(label)
}

/// An on/off switch.
pub fn switch(
    id: impl Into<ElementId>,
    on: bool,
    label: &'static str,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .role(Role::Switch)
        .aria_label(label)
        .debug_selector(move || label.to_string())
        .flex_none()
        .w(px(40.0))
        .h(px(22.0))
        .rounded_full()
        .bg(if on { tk.accent() } else { tk.track() })
        .cursor_pointer()
        .relative()
        .on_click(on_click)
        .child(
            div()
                .absolute()
                .top(px(3.0))
                .left(px(if on { 21.0 } else { 3.0 }))
                .size(px(16.0))
                .rounded_full()
                .bg(if on { tk.on_accent() } else { tk.knob() }),
        )
}

/// A horizontal bar `frac` (0 to 1) full.
pub fn bar(frac: f32, color: Hsla, tk: &Tk) -> impl IntoElement {
    div()
        .w_full()
        .h(px(6.0))
        .rounded_full()
        .bg(tk.track())
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(gpui::relative(frac.clamp(0.0, 1.0)))
                .rounded_full()
                .bg(color),
        )
}

/// A widget's header: an icon and a title.
pub fn header(icon_name: &str, title: impl Into<SharedString>, tk: &Tk) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .h(px(26.0))
        .child(icon(icon_name, 18.0, tk.dim()))
        .child(
            div()
                .text_size(px(tk.font_title()))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(tk.text())
                .child(title.into()),
        )
}

/// Secondary text.
pub fn muted(text: impl Into<SharedString>, tk: &Tk) -> gpui::Div {
    div().text_color(tk.dim()).child(text.into())
}

/// What a widget says when it has nothing to show.
pub fn empty(text: impl Into<SharedString>, tk: &Tk) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(tk.faint())
        .child(text.into())
        .into_any_element()
}

/// A length in pixels.
pub fn p(v: f32) -> Pixels {
    px(v)
}

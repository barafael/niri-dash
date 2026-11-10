//! The small pieces widgets are made of: the look's colors and sizes as GPUI wants them (`Tk`),
//! icons, buttons, switches, bars, headers. The ports of `common.slint`'s components.

use crate::theme::{Color, ColorRole, Look};
use gpui::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;
use gpui::{
    AnyElement, App, ClickEvent, ElementId, Hsla, Role, SharedString, Window, div, px, svg,
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
        // A press on a control is not a press on the card behind it (a double click on a control
        // must not pick the widget up in edit mode).
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
        // A press on a control is not a press on the card behind it (a double click on a control
        // must not pick the widget up in edit mode).
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
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
        // A press on a control is not a press on the card behind it (a double click on a control
        // must not pick the widget up in edit mode).
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
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

/// The family the system's fontconfig gives "sans-serif" (Noto Sans on most desktops), which is
/// what the Slint version drew with. GPUI's own default lands on a variable font whose semibold
/// and bold weights it cannot pick, so titles came out regular. `None` where `fc-match` is not
/// there: the caller keeps GPUI's choice. Asked once.
pub fn system_font() -> Option<SharedString> {
    use std::sync::OnceLock;
    static FAMILY: OnceLock<Option<String>> = OnceLock::new();
    FAMILY
        .get_or_init(|| {
            let out = std::process::Command::new("fc-match")
                .args(["-f", "%{family[0]}", "sans-serif"])
                .output()
                .ok()?;
            let name = String::from_utf8(out.stdout).ok()?;
            let name = name.trim();
            (out.status.success() && !name.is_empty()).then(|| name.to_string())
        })
        .clone()
        .map(SharedString::from)
}

/// The height of a line of text, in em: Noto Sans's ascent plus descent, which is what the Slint
/// version's `Text` was as high as. GPUI's own default (1.618) makes every text row taller, so
/// cards wrapped in [`slint_lines`] lay out as the Slint ones did.
pub const LINE: f32 = 1.362;

/// `content` with the line height of the Slint version, for the whole widget.
pub fn slint_lines(content: AnyElement) -> AnyElement {
    div()
        .size_full()
        .line_height(gpui::relative(LINE))
        .child(content)
        .into_any_element()
}

/// A value on its way to a new one, for what glides (Slint's `animate`).
struct GlideState {
    from: f32,
    to: f32,
    since: Instant,
}

thread_local! {
    /// What each slider, knob and gauge shows right now, by a key of its own.
    static GLIDES: RefCell<HashMap<u64, GlideState>> = RefCell::new(HashMap::new());
}

/// Where a value is `elapsed` ms into gliding from `from` to `to` over `ms` (ease-out), and
/// whether it has arrived.
pub fn eased(from: f32, to: f32, elapsed: f32, ms: f32) -> (f32, bool) {
    let t = (elapsed / ms.max(0.001)).clamp(0.0, 1.0);
    (from + (to - from) * (1.0 - (1.0 - t) * (1.0 - t)), t >= 1.0)
}

/// What `key` shows now when it should be at `target`: it glides there over `ms` (0: at once),
/// easing out, and asks for frames until it has arrived. Call it while painting.
pub fn glide(key: u64, target: f32, ms: f32, window: &mut gpui::Window) -> f32 {
    GLIDES.with(|g| {
        let mut g = g.borrow_mut();
        let now = Instant::now();
        let at = |s: &GlideState| eased(s.from, s.to, now.duration_since(s.since).as_secs_f32() * 1000.0, ms);
        let Some(state) = g.get_mut(&key) else {
            g.insert(key, GlideState { from: target, to: target, since: now });
            return target;
        };
        if ms <= 0.0 {
            *state = GlideState { from: target, to: target, since: now };
            return target;
        }
        if (state.to - target).abs() > f32::EPSILON {
            let (shown, _) = at(state);
            *state = GlideState { from: shown, to: target, since: now };
        }
        let (shown, done) = at(state);
        if !done {
            window.request_animation_frame();
        }
        shown
    })
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glide_eases_out_and_arrives() {
        let (start, done) = eased(0.0, 1.0, 0.0, 100.0);
        assert!(start == 0.0 && !done);
        let (half, done) = eased(0.0, 1.0, 50.0, 100.0);
        // Ease-out: more than half of the way at half of the time.
        assert!(half > 0.5 && half < 1.0 && !done);
        let (end, done) = eased(0.2, 0.8, 100.0, 100.0);
        assert!(end == 0.8 && done);
        // Past the end it stays, and a time of nothing arrives at once.
        assert_eq!(eased(0.0, 1.0, 500.0, 100.0), (1.0, true));
        assert_eq!(eased(0.0, 1.0, 0.0, 0.0), (0.0, false));
    }
}

/// Values that glide (Slint's `animate`), for one widget: what a bar or a ring shows eases over to
/// a new reading. The style's `motion` scales the time and 0 turns it off.
#[derive(Clone, Copy)]
pub struct Glide<'a> {
    anims: &'a crate::edit::anim::Anims,
    id: u32,
    motion: f32,
}

impl<'a> Glide<'a> {
    pub fn new(ctx: &'a crate::widgets::Ctx) -> Self {
        Self {
            anims: &ctx.d.edit.anim,
            id: ctx.tile.id(),
            motion: ctx.tk.motion(),
        }
    }

    /// Where `what` (one of the widget's values, `n` for several of a kind) is, on its way to
    /// `target` over `ms`.
    pub fn to(&self, what: &'static str, n: u32, target: f32, ms: f32) -> f32 {
        self.anims.follow(
            (what, self.id << 8 | n.min(255)),
            target,
            ms,
            self.motion,
            crate::edit::anim::Ease::Out,
        )
    }
}

/// The radius of a ring's middle line in a `size` square, for a stroke `thickness` thick. Slint
/// fits a stroked path's own box inside the item with half the stroke to spare on every side, so
/// the whole path (this radius included) shrinks by `(size - thickness) / size`: the ring does not
/// touch the edge of its card but leaves a gap of about half its thickness.
pub fn ring_radius(size: f32, thickness: f32) -> f32 {
    (size / 2.0 - thickness / 2.0 - size * 0.005) * ((size - thickness) / size).max(0.0)
}

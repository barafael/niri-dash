//! The night-light widget and `niri-dash night-light`: warmer screen colors through the system's
//! gamma control. Only the daemon drives the gamma tables (two clients can't both hold them); a
//! standalone night-light app hands its changes to the daemon over the control socket.
//!
//! On Plasma, KWin has a night light of its own (and no gamma control for us): the widget shows
//! KWin's, and turning it off pauses KWin's schedule for as long as the daemon runs.
//!
//! Port of `NightLightWidget` (`legacy/ui/widgets/system.slint`) and `legacy/src/app/night_light.rs`.

use super::sliders::{self, Knob, slider, switch_button};
use super::{Fit, NoStops, on, setter};
use crate::dash::Dash;
use crate::keys::Key;
use crate::kit::{self, Tk};
use crate::scope::{Scope, Stops};
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, Context, ElementId, Role, div, px};
use niri_dash_system::Tell;
use niri_dash_system::night_light::{self as gamma, Cmd, Command, Event, NightLightState};
use niri_dash_system::notifications;

/// The presets (rectangular, tall enough).
const PRESETS: [u32; 3] = [3400, 4500, 5500];

#[derive(Default)]
pub struct State {
    pub scope: Scope,
    pub on: bool,
    pub kelvin: u32,
    /// Slider position: 0 neutral, 1 warmest.
    pub warmth: f32,
    pub available: bool,
    /// KWin sets the temperature: there is only the switch.
    pub fixed: bool,
    pub status: String,
    pub note: String,
    /// The state file is behind: written when the next tick finds it so, once a drag settles.
    dirty: bool,
}

/// Slider position (0 = neutral, 1 = warmest) for a temperature, and back.
fn warmth_of(kelvin: u32) -> f32 {
    let span = (gamma::NEUTRAL - gamma::WARMEST) as f32;
    ((gamma::NEUTRAL.saturating_sub(kelvin)) as f32 / span).clamp(0.0, 1.0)
}

fn kelvin_of(warmth: f32) -> u32 {
    let span = (gamma::NEUTRAL - gamma::WARMEST) as f32;
    let k = gamma::NEUTRAL as f32 - warmth.clamp(0.0, 1.0) * span;
    // Steps of 100 K: finer isn't visible, and the label stays calm while dragging.
    ((k / 100.0).round() * 100.0) as u32
}

/// On start: bring back what was on before the restart.
pub fn init(d: &mut Dash) {
    if d.caps.night_light_native {
        // KWin keeps its own state; a pause ended with the daemon.
        let n = &mut d.widgets.controls.night;
        n.fixed = true;
        n.kelvin = gamma::NEUTRAL;
        return;
    }
    let nl = d.state.night_light;
    if d.solo.is_none() {
        d.widgets.controls.night.available = d.caps.gamma;
        set_gamma(d, nl.on.then_some(nl.kelvin));
    } else {
        // The daemon has the gamma control; trust that it is running.
        d.widgets.controls.night.available = true;
    }
    push(d);
}

/// Warm the screen to `want` through the gamma tables (None: neutral).
fn set_gamma(d: &Dash, want: Option<u32>) {
    d.tx.night_light.tell(Command::Gamma(want));
}

/// Show what the saved state says.
fn push(d: &mut Dash) {
    let nl = d.state.night_light;
    let n = &mut d.widgets.controls.night;
    n.on = nl.on;
    n.kelvin = nl.kelvin;
    n.warmth = warmth_of(nl.kelvin);
}

/// Apply a request from the widget, a standalone app or the command line; `announce` shows the
/// result as an on-screen message (a keybinding has no other feedback).
pub fn apply(d: &mut Dash, cmd: Cmd, announce: bool) {
    if d.caps.night_light_native {
        if d.solo.is_some() {
            forward(d, cmd);
        } else {
            d.tx.night_light.tell(Command::Kwin { cmd, announce });
        }
        return;
    }
    let nl = d.state.night_light;
    let (on, kelvin) = cmd.apply(nl.on, nl.kelvin);
    d.state.night_light = crate::state::NightLight { on, kelvin };
    push(d);
    if d.solo.is_some() {
        forward(d, cmd);
        return;
    }
    set_gamma(d, on.then_some(kelvin));
    d.widgets.controls.night.dirty = true;
    if announce {
        let text = if on {
            format!("Night light on · {kelvin} K")
        } else {
            "Night light off".to_string()
        };
        d.tx.notifications.tell(notifications::Command::Osd {
            text,
            icon: "weather-clear-night".into(),
        });
    }
}

/// `niri-dash night-light …` arrived on the control socket.
pub fn command(d: &mut Dash, cmd: Cmd, cx: &mut Context<Dash>) {
    apply(d, cmd, true);
    cx.notify();
}

/// A standalone app hands night light to the daemon.
fn forward(d: &mut Dash, cmd: Cmd) {
    use std::io::Write;
    if let Err(e) = std::os::unix::net::UnixStream::connect(crate::socket_path())
        .and_then(|mut s| writeln!(s, "{}", cmd.to_wire()))
    {
        d.toast(&format!("Night light needs the niri-dash daemon running: {e}"));
    }
}

/// A temperature from the widget: KWin sets its own, so only show it again.
fn set_kelvin(d: &mut Dash, kelvin: u32) {
    if d.caps.night_light_native {
        let n = &mut d.widgets.controls.night;
        n.warmth = warmth_of(n.kelvin);
        return;
    }
    apply(d, Cmd::Kelvin(kelvin), false);
}

/// The warmth slider, a knob or a key moved to `v`.
fn set_warmth(d: &mut Dash, v: f32) {
    let w = v.clamp(0.0, 1.0);
    d.widgets.controls.night.warmth = w;
    set_kelvin(d, kelvin_of(w));
}

fn toggle(d: &mut Dash) {
    apply(d, Cmd::Toggle, false);
}

/// The state file, once a drag has settled.
pub fn tick(d: &mut Dash) {
    save_now(d);
}

pub fn save_now(d: &mut Dash) {
    if std::mem::take(&mut d.widgets.controls.night.dirty) {
        crate::state::save(&d.state);
    }
}

/// KWin's night light changed (Plasma).
pub fn event(d: &mut Dash, event: &Event) {
    let Event::Kwin(s) = event;
    let s: NightLightState = *s;
    let n = &mut d.widgets.controls.night;
    n.fixed = true;
    n.available = s.available;
    n.on = s.on();
    n.kelvin = s.temperature;
    n.warmth = warmth_of(s.temperature);
    n.status = if !s.available {
        ""
    } else if !s.enabled {
        "Off"
    } else if s.inhibited {
        "Paused"
    } else if s.temperature >= gamma::NEUTRAL {
        "Day"
    } else {
        ""
    }
    .into();
    n.note = if !s.available {
        "KWin's night light isn't available"
    } else if !s.enabled {
        "Turned off in System Settings"
    } else if s.inhibited && !s.paused_by_us {
        "Paused by another program"
    } else {
        "Schedule and warmth from System Settings"
    }
    .into();
}

impl State {
    fn has_slider(&self) -> bool {
        self.available && !self.fixed
    }

    fn label(&self) -> String {
        if !self.status.is_empty() {
            self.status.clone()
        } else if self.on {
            format!("{} K", self.kelvin)
        } else {
            "Off".into()
        }
    }

    /// The keyboard stops (rectangular): the switch, the warmth slider, the presets.
    fn count(&self, f: &Fit) -> usize {
        if f.round {
            return 0;
        }
        1 + usize::from(self.has_slider()) + usize::from(self.has_presets(f))
    }

    fn has_presets(&self, f: &Fit) -> bool {
        self.has_slider() && f.h >= 170.0
    }

    /// The presets are the parts of the stop 2.
    fn subs(&self, f: &Fit, cursor: usize) -> usize {
        if !f.round && cursor == 2 { PRESETS.len() } else { 1 }
    }
}

struct Keys<'a> {
    d: &'a mut Dash,
    round: bool,
    has_slider: bool,
}

impl Stops for Keys<'_> {
    fn activate(&mut self, cursor: usize, sub: usize) {
        if !self.round && cursor == 2 {
            set_kelvin(self.d, PRESETS[sub.min(PRESETS.len() - 1)]);
        } else {
            toggle(self.d);
        }
    }

    fn adjust(&mut self, cursor: usize, delta: i32) -> bool {
        if !self.has_slider || (!self.round && cursor != 1) {
            return false;
        }
        let w = self.d.widgets.controls.night.warmth;
        set_warmth(self.d, w + delta as f32 * sliders::STEP);
        true
    }

    fn letter(&mut self, key: &Key, _: usize) -> bool {
        if key.is('t') {
            toggle(self.d);
            return true;
        }
        false
    }
}

/// Enter or t turns it on and off; on the slider h/l change the warmth; on the presets h/l pick
/// one and Enter sets it.
pub fn key(d: &mut Dash, fit: &Fit, key: &Key) -> bool {
    let mut scope = d.widgets.controls.night.scope;
    let n = &d.widgets.controls.night;
    scope.set_count(n.count(fit), &NoStops);
    scope.set_subs(n.subs(fit, scope.cursor));
    let has_slider = n.has_slider();
    let taken = scope.press(
        key,
        &mut Keys {
            d,
            round: fit.round,
            has_slider,
        },
    );
    let n = &d.widgets.controls.night;
    scope.set_count(n.count(fit), &NoStops);
    scope.set_subs(n.subs(fit, scope.cursor));
    d.widgets.controls.night.scope = scope;
    taken
}

pub fn refit(d: &mut Dash, fit: &Fit) {
    let n = &mut d.widgets.controls.night;
    let (count, subs) = (n.count(fit), n.subs(fit, n.scope.cursor));
    n.scope.set_count(count, &NoStops);
    n.scope.set_subs(subs);
}

pub fn describe(n: &State, fit: &Fit) -> String {
    format!(
        "stop {} of {}, {}",
        n.scope.cursor,
        n.count(fit),
        n.label()
    )
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let n = &ctx.d.widgets.controls.night;
    let fit = Fit::of_ctx(ctx);
    let tk = ctx.tk;
    let id = ctx.tile.id() as usize;
    let label = n.label();
    if fit.round {
        return div()
            .size_full()
            .child(sliders::ring_knob(
                Knob {
                    id: ("night-light-knob", id),
                    name: format!("Night light, {label}").into(),
                    value: n.warmth,
                    tint: tk.warn(),
                    icon: "moon",
                    label,
                    dimmed: !n.on,
                    kfocus: ctx.entered,
                    side: fit.w,
                },
                tk,
                // With a fixed temperature (KWin), scrolling the ring changes nothing: it is put
                // back.
                setter(cx, |d, v, _| set_warmth(d, v)),
                on(cx, |d, _| toggle(d)),
            ))
            .into_any_element();
    }
    let cursor = n.scope.cursor.min(n.count(&fit).saturating_sub(1));
    let at = |stop: usize| ctx.entered && cursor == stop;
    let has_presets = n.has_presets(&fit);

    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .h(px(28.0))
        .flex_none()
        .child(kit::icon("moon", 18.0, if n.on { tk.warn() } else { tk.dim() }))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(tk.font_title()))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(tk.text())
                .child("Night light"),
        )
        .when(n.available && fit.w >= 260.0, |h| h.child(muted(&label, tk)))
        .child(switch_button(
            ("night-light-switch", id),
            "Night light",
            n.on,
            n.available,
            at(0),
            tk,
            on(cx, |d, _| toggle(d)),
        ));

    let warmth = n.has_slider().then(|| {
        slider(
            ("night-light-warmth", id),
            "Warmth",
            n.warmth,
            if n.on { tk.warn() } else { tk.faint() },
            true,
            at(1),
            tk,
            setter(cx, |d, v, _| set_warmth(d, v)),
        )
        .w_full()
    });

    let note = (!n.note.is_empty()).then(|| muted(&n.note, tk).flex_none().whitespace_normal());
    let nothing = (!n.available && n.note.is_empty())
        .then(|| muted("The compositor offers no gamma control", tk).flex_none());

    let presets = has_presets.then(|| {
        div()
            .flex()
            .flex_row()
            .gap(px(6.0))
            .flex_none()
            .children(PRESETS.iter().enumerate().map(|(i, &k)| {
                let current = n.on && n.kelvin == k;
                let kf = at(2) && n.scope.part() == i;
                let name = format!("{k} K");
                let selector = name.clone();
                let (hover, pressed) = (tk.hover(), tk.pressed());
                div()
                    .id(ElementId::from(("night-light-preset", id * 8 + i)))
                    .role(Role::Button)
                    .aria_label(name.clone())
                    .aria_value(if current { "current" } else { "" })
                    .aria_selected(kf)
                    .debug_selector(move || selector.clone())
                    .flex_1()
                    .min_w_0()
                    .h(px(30.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(tk.radius_small().min(15.0)))
                    .border(px(2.0))
                    .border_color(if !kf {
                        gpui::transparent_black()
                    } else if current {
                        tk.text()
                    } else {
                        tk.accent()
                    })
                    .cursor_pointer()
                    .when(current, |b| b.bg(tk.accent()).text_color(tk.on_accent()))
                    .when(!current, |b| {
                        b.bg(tk.card_raised())
                            .text_color(tk.text())
                            .hover(move |s| s.bg(hover))
                            .active(move |s| s.bg(pressed))
                    })
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(on(cx, move |d, _| set_kelvin(d, k)))
                    .child(div().truncate().child(name))
            }))
    });

    // Centered while it fits, from the top when it doesn't (as Slint's layout does): the
    // auto margins give way to the overflow.
    div()
        .size_full()
        .p(px(fit.pad))
        .flex()
        .flex_col()
        .child(
            div()
                .w_full()
                .my_auto()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .child(header)
                .children(warmth)
                .children(note)
                .children(nothing)
                .children(presets),
        )
        .into_any_element()
}

fn muted(text: &str, tk: &Tk) -> gpui::Div {
    div()
        .flex_none()
        .text_size(px(tk.font_small()))
        .text_color(tk.dim())
        .child(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_mapping() {
        assert_eq!(warmth_of(6500), 0.0);
        assert_eq!(warmth_of(2500), 1.0);
        assert_eq!(kelvin_of(0.0), 6500);
        assert_eq!(kelvin_of(1.0), 2500);
        assert_eq!(kelvin_of(warmth_of(4500)), 4500);
        assert_eq!(kelvin_of(0.51), 4500);
    }
}

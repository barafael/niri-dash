//! The night-light widget and `niri-dash night-light`: warmer screen colors through the system's
//! gamma control. Only the daemon drives the gamma tables (two clients can't both hold them); a
//! standalone night-light app hands its changes to the daemon over the control socket.
//!
//! On Plasma, KWin has night light of its own (and no gamma control for us): the widget shows
//! KWin's, and turning it off pauses KWin's schedule for as long as the daemon runs.

use super::App;
use crate::{Dash, NightLight as UiNightLight};
use niri_dash_system::Tell;
use niri_dash_system::night_light::{self as gamma, Cmd, NightLightState};
use slint::{ComponentHandle, Timer, TimerMode};
use std::rc::Rc;
use std::time::Duration;

#[derive(Default)]
pub(super) struct NightLight {
    /// Writes the state file once a slider drag settles.
    save: Timer,
}

/// Slider position (0 = neutral, 1 = warmest) for a temperature, and back.
fn warmth(kelvin: u32) -> f32 {
    let span = (gamma::NEUTRAL - gamma::WARMEST) as f32;
    ((gamma::NEUTRAL.saturating_sub(kelvin)) as f32 / span).clamp(0.0, 1.0)
}

fn kelvin(warmth: f32) -> u32 {
    let span = (gamma::NEUTRAL - gamma::WARMEST) as f32;
    let k = gamma::NEUTRAL as f32 - warmth.clamp(0.0, 1.0) * span;
    // Steps of 100 K: finer isn't visible, and the label stays calm while dragging.
    ((k / 100.0).round() * 100.0) as u32
}

impl App {
    /// On start: bring back what was on before the restart.
    pub(super) fn restore_night_light(&self) {
        if self.caps.kwin_night_light() {
            // KWin keeps its own state; a pause ended with the daemon.
            let ui = self.ui();
            let g = ui.global::<UiNightLight>();
            g.set_fixed(true);
            g.set_kelvin(gamma::NEUTRAL as i32);
            return;
        }
        let nl = self.state.borrow().night_light;
        if self.solo.is_none() {
            let ui = self.ui();
            ui.global::<UiNightLight>().set_available(self.caps.gamma);
            self.set_gamma(nl.on.then_some(nl.kelvin));
        } else {
            // The daemon has the gamma control; trust that it is running.
            self.ui().global::<UiNightLight>().set_available(true);
        }
        self.push_night_light();
    }

    /// Warm the screen to `want` through the gamma tables (None: neutral).
    fn set_gamma(&self, want: Option<u32>) {
        self.tx.night_light.tell(gamma::Command::Gamma(want));
    }

    /// Apply a request from the widget, a standalone app or the command line; `announce` shows
    /// the result as an on-screen message (a keybinding has no other feedback).
    pub(super) fn night_light(&self, cmd: Cmd, announce: bool) {
        if self.caps.kwin_night_light() {
            if self.solo.is_some() {
                self.forward_night_light(cmd);
            } else {
                self.tx
                    .night_light
                    .tell(gamma::Command::Kwin { cmd, announce });
            }
            return;
        }
        let (on, kelvin) = {
            let nl = self.state.borrow().night_light;
            cmd.apply(nl.on, nl.kelvin)
        };
        self.state.borrow_mut().night_light = crate::state::NightLight { on, kelvin };
        self.push_night_light();
        if self.solo.is_some() {
            self.forward_night_light(cmd);
            return;
        }
        self.set_gamma(on.then_some(kelvin));
        self.night.save.restart();
        if announce {
            let text = if on {
                format!("Night light on · {kelvin} K")
            } else {
                "Night light off".to_string()
            };
            self.osd(&text, "weather-clear-night");
        }
    }

    /// A standalone app hands night light to the daemon.
    #[cfg(target_arch = "wasm32")]
    fn forward_night_light(&self, _: Cmd) {}

    /// A standalone app hands night light to the daemon.
    #[cfg(not(target_arch = "wasm32"))]
    fn forward_night_light(&self, cmd: Cmd) {
        use std::io::Write;
        if let Err(e) = std::os::unix::net::UnixStream::connect(crate::socket_path())
            .and_then(|mut s| writeln!(s, "{}", cmd.to_wire()))
        {
            self.toast(&format!(
                "Night light needs the niri-dash daemon running: {e}"
            ));
        }
    }

    /// KWin's night light changed (Plasma).
    pub(super) fn show_kwin_night_light(&self, s: NightLightState) {
        let ui = self.ui();
        let g = ui.global::<UiNightLight>();
        g.set_fixed(true);
        g.set_available(s.available);
        g.set_on(s.on());
        g.set_kelvin(s.temperature as i32);
        g.set_warmth(warmth(s.temperature));
        g.set_status(
            if !s.available {
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
            .into(),
        );
        g.set_note(
            if !s.available {
                "KWin's night light isn't available"
            } else if !s.enabled {
                "Turned off in System Settings"
            } else if s.inhibited && !s.paused_by_us {
                "Paused by another program"
            } else {
                "Schedule and warmth from System Settings"
            }
            .into(),
        );
    }

    /// A temperature from the widget: KWin sets its own, so only show it again.
    fn night_light_kelvin(&self, kelvin: u32) {
        if self.caps.kwin_night_light() {
            let ui = self.ui();
            let g = ui.global::<UiNightLight>();
            g.set_warmth(warmth(g.get_kelvin().max(0) as u32));
            return;
        }
        self.night_light(Cmd::Kelvin(kelvin), false);
    }

    fn push_night_light(&self) {
        let nl = self.state.borrow().night_light;
        let ui = self.ui();
        let g = ui.global::<UiNightLight>();
        g.set_on(nl.on);
        g.set_kelvin(nl.kelvin as i32);
        g.set_warmth(warmth(nl.kelvin));
    }
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    let g = ui.global::<UiNightLight>();
    let a = app.clone();
    g.on_toggle(move || a.night_light(Cmd::Toggle, false));
    let a = app.clone();
    g.on_set_warmth(move |w| a.night_light_kelvin(kelvin(w)));
    let a = app.clone();
    g.on_set_kelvin(move |k| a.night_light_kelvin(k.max(0) as u32));
    let w = Rc::downgrade(app);
    app.night.save.start(
        TimerMode::SingleShot,
        Duration::from_millis(500),
        move || {
            if let Some(a) = w.upgrade() {
                crate::state::save(&a.state.borrow());
            }
        },
    );
    app.night.save.stop();
    app.restore_night_light();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_mapping() {
        assert_eq!(warmth(6500), 0.0);
        assert_eq!(warmth(2500), 1.0);
        assert_eq!(kelvin(0.0), 6500);
        assert_eq!(kelvin(1.0), 2500);
        assert_eq!(kelvin(warmth(4500)), 4500);
        assert_eq!(kelvin(0.51), 4500);
    }
}

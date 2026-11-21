//! The night-light widget and `niri-dash night-light`. The system warms the screen, or where the
//! desktop has a night light of its own (KWin's), pauses that one: the widget sends the same
//! commands either way, and shows the state the system reports. Only the daemon runs night light;
//! a standalone night-light app hands its changes to the daemon over the control socket.

use super::App;
use crate::{Dash, NightLight as UiNightLight};
use niri_dash_system::Tell;
use niri_dash_system::night_light::{self as nl, Cmd, State};
use slint::{ComponentHandle, Timer, TimerMode};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;
use web_time::Instant;

#[derive(Default)]
pub(super) struct NightLight {
    /// Writes the state file once a slider drag settles.
    save: Timer,
    /// When the slider was last dragged: until it settles, it keeps its value against what the
    /// system reports, which trails it.
    touched: Cell<Option<Instant>>,
}

/// Slider position (0 = neutral, 1 = warmest) for a temperature, and back.
fn warmth(kelvin: u32) -> f32 {
    let span = (nl::NEUTRAL - nl::WARMEST) as f32;
    ((nl::NEUTRAL.saturating_sub(kelvin)) as f32 / span).clamp(0.0, 1.0)
}

fn kelvin(warmth: f32) -> u32 {
    let span = (nl::NEUTRAL - nl::WARMEST) as f32;
    let k = nl::NEUTRAL as f32 - warmth.clamp(0.0, 1.0) * span;
    // Steps of 100 K: finer isn't visible, and the label stays calm while dragging.
    ((k / 100.0).round() * 100.0) as u32
}

/// A word instead of the temperature next to the switch (empty: the temperature, or "Off").
fn status(s: State) -> &'static str {
    match s.schedule {
        _ if !s.available => "",
        None => "",
        Some(sch) if !sch.enabled => "Off",
        Some(sch) if sch.paused => "Paused",
        Some(_) if s.kelvin >= nl::NEUTRAL => "Day",
        Some(_) => "",
    }
}

/// The line under the switch.
fn note(s: State) -> &'static str {
    match s.schedule {
        None => "",
        Some(_) if !s.available => "The desktop's night light isn't available",
        Some(sch) if !sch.enabled => "Turned off in the system settings",
        Some(sch) if sch.paused && !sch.paused_by_us => "Paused by another program",
        Some(_) => "Schedule and warmth from the system settings",
    }
}

impl App {
    /// On start: bring back how night light was before the restart (where it is ours to keep).
    pub(super) fn restore_night_light(&self) {
        let saved = self.state.borrow().night_light;
        self.tx.tell(nl::Command::Restore {
            on: saved.on,
            kelvin: saved.kelvin,
        });
    }

    /// Apply a request from the widget, a standalone app or the command line; `announce` shows
    /// the result as an on-screen message (a keybinding has no other feedback).
    pub(super) fn night_light(&self, cmd: Cmd, announce: bool) {
        if self.solo.is_some() {
            // The daemon runs it; this one's system only shows it.
            self.forward_night_light(cmd);
        }
        self.tx.tell(nl::Command::Apply { cmd, announce });
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

    /// Night light as the system reports it.
    pub(super) fn show_night_light(&self, s: State) {
        let ui = self.ui();
        let g = ui.global::<UiNightLight>();
        g.set_available(s.available);
        g.set_fixed(s.schedule.is_some());
        g.set_on(s.on);
        if !self.night.touched.get().is_some_and(super::held) {
            g.set_kelvin(s.kelvin as i32);
            g.set_warmth(warmth(s.kelvin));
        }
        g.set_status(status(s).into());
        g.set_note(note(s).into());
        // The daemon keeps its own setting across restarts; a desktop's own keeps itself.
        if s.schedule.is_none() && self.solo.is_none() {
            let kept = crate::state::NightLight {
                on: s.on,
                kelvin: s.kelvin,
            };
            if self.state.borrow().night_light != kept {
                self.state.borrow_mut().night_light = kept;
                self.night.save.restart();
            }
        }
    }

    /// A temperature from the widget.
    fn night_light_kelvin(&self, kelvin: u32) {
        let ui = self.ui();
        let g = ui.global::<UiNightLight>();
        if g.get_fixed() {
            // The desktop sets the warmth: put the slider back.
            g.set_warmth(warmth(g.get_kelvin().max(0) as u32));
            return;
        }
        self.night.touched.set(Some(Instant::now()));
        g.set_kelvin(kelvin as i32);
        g.set_warmth(warmth(kelvin));
        self.night_light(Cmd::Kelvin(kelvin), false);
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

    /// Our own night light needs no words; a desktop's own says why it is off or paused.
    #[test]
    fn what_the_switch_says() {
        use nl::Schedule;
        let ours = State {
            available: true,
            on: true,
            kelvin: 4000,
            schedule: None,
        };
        assert_eq!((status(ours), note(ours)), ("", ""));
        let desktop = |schedule, kelvin| State {
            schedule: Some(schedule),
            kelvin,
            ..ours
        };
        let running = Schedule {
            enabled: true,
            ..Default::default()
        };
        let day = desktop(running, nl::NEUTRAL);
        assert_eq!(status(day), "Day");
        assert_eq!(note(day), "Schedule and warmth from the system settings");
        let off = desktop(Schedule::default(), 4000);
        assert_eq!(status(off), "Off");
        assert_eq!(note(off), "Turned off in the system settings");
        let theirs = Schedule {
            paused: true,
            ..running
        };
        assert_eq!(status(desktop(theirs, 4000)), "Paused");
        assert_eq!(note(desktop(theirs, 4000)), "Paused by another program");
        let mine = Schedule {
            paused_by_us: true,
            ..theirs
        };
        assert_eq!(
            note(desktop(mine, 4000)),
            "Schedule and warmth from the system settings"
        );
    }
}

//! The timer and stopwatch widgets.

use super::{App, model};
use crate::Dash;
use crate::timer::{Countdown, Run, Stopwatch};
use niri_dash_system::{Tell, host, notifications};
use slint::{ComponentHandle, Model, Timer, TimerMode};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use web_time::{SystemTime, UNIX_EPOCH};

/// Played when the timer goes off, if it's there.
const ALARM_SOUND: &str = "/usr/share/sounds/freedesktop/stereo/alarm-clock-elapsed.oga";

/// The countdown and the stopwatch, shared by every timer widget.
pub(super) struct Timers {
    countdown: RefCell<Countdown>,
    stopwatch: RefCell<Stopwatch>,
    /// Updates the timer widgets while the dashboard is up and something runs.
    ticker: Timer,
    /// Goes off when the countdown ends, shown or not.
    end: Timer,
    /// Clears the "Done" a while after the countdown went off.
    done: Timer,
}

impl Timers {
    /// The countdown as it was saved: its length, and its end if it's still running.
    pub(super) fn restored(state: &crate::state::State) -> Self {
        let mut countdown = Countdown::default();
        if state.timer_secs > 0 {
            countdown.length = Duration::from_secs(state.timer_secs as u64);
        }
        if let Some(end) = state.timer_end {
            let end = UNIX_EPOCH + Duration::from_millis(end.max(0) as u64);
            if end > SystemTime::now() {
                countdown.state = Run::Running { end };
            }
        }
        Self {
            countdown: RefCell::new(countdown),
            stopwatch: Default::default(),
            ticker: Default::default(),
            end: Default::default(),
            done: Default::default(),
        }
    }
}

impl App {
    fn countdown(&self, f: impl FnOnce(&mut Countdown, SystemTime)) {
        f(&mut self.timers.countdown.borrow_mut(), SystemTime::now());
        self.timers.done.stop();
        self.ui().global::<crate::Countdown>().set_done(false);
        let c = self.timers.countdown.borrow().clone();
        self.update_state(|s| {
            s.timer_secs = c.length.as_secs() as u32;
            s.timer_end = match c.state {
                Run::Running { end } => end
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_millis() as i64),
                _ => None,
            };
        });
        self.arm_timer_end();
        self.timers_awake(self.visible.get());
    }

    fn stopwatch(&self, f: impl FnOnce(&mut Stopwatch, SystemTime)) {
        f(&mut self.timers.stopwatch.borrow_mut(), SystemTime::now());
        self.timers_awake(self.visible.get());
    }

    /// Set the end timer for a running countdown.
    fn arm_timer_end(&self) {
        let c = self.timers.countdown.borrow().clone();
        match c.state {
            Run::Running { .. } => {
                let left = c.left(SystemTime::now());
                self.timers
                    .end
                    .set_interval(left.max(Duration::from_millis(1)));
                self.timers.end.restart();
            }
            _ => self.timers.end.stop(),
        }
    }

    fn timer_went_off(&self) {
        let now = SystemTime::now();
        if !self.timers.countdown.borrow().finished(now) {
            // Woken early (the clock changed): wait for the rest.
            self.arm_timer_end();
            return;
        }
        let length = self.timers.countdown.borrow().length;
        self.countdown(|c, _| c.reset());
        let ui = self.ui();
        ui.global::<crate::Countdown>().set_done(true);
        let text = format!("{} timer is done", crate::timer::format(length, false));
        // On Plasma the notification pops up by itself.
        if !self.caps.notifications_elsewhere() {
            self.osd(&text, "alarm-symbolic");
        }
        self.tx
            .notifications
            .tell(notifications::Command::AddLocal {
                app: "Timer".into(),
                summary: text,
                body: String::new(),
            });
        self.tx
            .host
            .tell(host::Command::PlaySound(ALARM_SOUND.into()));
        let w = self.ui.clone();
        self.timers
            .done
            .start(TimerMode::SingleShot, Duration::from_secs(60), move || {
                if let Some(ui) = w.upgrade() {
                    ui.global::<crate::Countdown>().set_done(false);
                }
            });
    }

    /// Run the ticker only while it shows anything.
    pub(super) fn timers_awake(&self, visible: bool) {
        let running = matches!(self.timers.countdown.borrow().state, Run::Running { .. })
            || self.timers.stopwatch.borrow().running();
        if visible && running {
            if !self.timers.ticker.running() {
                self.timers.ticker.restart();
            }
        } else {
            self.timers.ticker.stop();
        }
        self.push_timers();
    }

    fn push_timers(&self) {
        let now = SystemTime::now();
        let ui = self.ui();
        let t = ui.global::<crate::Countdown>();
        let c = self.timers.countdown.borrow();
        let text: slint::SharedString = crate::timer::format_left(c.left(now)).into();
        if t.get_text() != text {
            t.set_text(text);
        }
        t.set_fraction(c.fraction(now));
        t.set_running(matches!(c.state, Run::Running { .. }));
        t.set_paused(matches!(c.state, Run::Paused { .. }));
        let s = self.timers.stopwatch.borrow();
        let elapsed = s.elapsed(now);
        t.set_sw_text(crate::timer::format(elapsed, true).into());
        t.set_sw_fraction(elapsed.as_secs_f32() % 60.0 / 60.0);
        t.set_sw_running(s.running());
        t.set_sw_started(!elapsed.is_zero());
        if t.get_laps().row_count() != s.laps.len() {
            t.set_laps(model(
                s.laps
                    .iter()
                    .map(|l| crate::timer::format(*l, true).into())
                    .collect(),
            ));
        }
    }
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    let t = ui.global::<crate::Countdown>();
    let a = app.clone();
    t.on_toggle(move || a.countdown(|c, now| c.toggle(now)));
    let a = app.clone();
    t.on_reset(move || a.countdown(|c, _| c.reset()));
    let a = app.clone();
    t.on_adjust(move |m| a.countdown(|c, now| c.adjust(m as i64, now)));
    let a = app.clone();
    t.on_preset(move |m| {
        a.countdown(|c, now| {
            c.length = Duration::from_secs(m.max(1) as u64 * 60);
            c.state = Run::Idle;
            c.toggle(now);
        })
    });
    let a = app.clone();
    t.on_sw_toggle(move || a.stopwatch(|s, now| s.toggle(now)));
    let a = app.clone();
    t.on_sw_lap(move || a.stopwatch(|s, now| s.lap(now)));
    let a = app.clone();
    t.on_sw_reset(move || a.stopwatch(|s, _| s.reset()));
    let w = Rc::downgrade(app);
    app.timers
        .ticker
        .start(TimerMode::Repeated, Duration::from_millis(100), move || {
            if let Some(a) = w.upgrade() {
                a.push_timers();
            }
        });
    app.timers.ticker.stop();
    let w = Rc::downgrade(app);
    app.timers
        .end
        .start(TimerMode::SingleShot, Duration::from_secs(1), move || {
            if let Some(a) = w.upgrade() {
                a.timer_went_off();
            }
        });
    app.timers.end.stop();
    app.arm_timer_end();
    app.push_timers();
}

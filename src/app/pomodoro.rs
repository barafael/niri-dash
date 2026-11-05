//! The pomodoro widget: the parts of `crate::pomodoro`, run on the clock here, with the notice
//! and the sound when one ends. Every pomodoro widget shows the same one.

use super::App;
use super::timer::ALARM_SOUND;
use crate::layout::Kind;
use crate::pomodoro::{Phase, Plan, Pomodoro};
use crate::timer::{Run, format_left};
use crate::widget_settings::{Opt, get_bool, get_int};
use crate::{Dash, Pomodoro as UiPomodoro};
use niri_dash_system::{Tell, host, notifications};
use slint::{ComponentHandle, Timer, TimerMode};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;
use web_time::{SystemTime, UNIX_EPOCH};

/// How often the widgets are updated while one is up and the time runs. They show whole seconds.
const TICK: Duration = Duration::from_millis(250);

pub(super) struct Pomo {
    state: RefCell<Pomodoro>,
    /// Updates the widgets while the dashboard is up and the time runs.
    ticker: Timer,
    /// Goes off when the part ends, shown or not.
    end: Timer,
}

impl Pomo {
    /// The pomodoro as it was saved: where it stood, and its end if it was still running.
    pub(super) fn restored(state: &crate::state::State) -> Self {
        let saved = state.pomodoro;
        let mut p = Pomodoro::default();
        p.phase = Phase::from_index(saved.phase);
        p.done = saved.done.min(p.plan.rounds);
        if let Some(end) = saved.end {
            let end = UNIX_EPOCH + Duration::from_millis(end.max(0) as u64);
            if end > SystemTime::now() {
                p.run = Run::Running { end };
            }
        }
        Self {
            state: RefCell::new(p),
            ticker: Default::default(),
            end: Default::default(),
        }
    }
}

impl App {
    fn pomodoro(&self, f: impl FnOnce(&mut Pomodoro, SystemTime)) {
        f(&mut self.pomo.state.borrow_mut(), SystemTime::now());
        self.pomodoro_settled();
    }

    /// Keep the saved state, the end timer and the ticker in step, and show it.
    fn pomodoro_settled(&self) {
        let p = self.pomo.state.borrow().clone();
        self.update_state(|s| {
            s.pomodoro = crate::state::Pomodoro {
                phase: p.phase.index(),
                done: p.done,
                end: match p.run {
                    Run::Running { end } => end
                        .duration_since(UNIX_EPOCH)
                        .ok()
                        .map(|d| d.as_millis() as i64),
                    _ => None,
                },
            };
        });
        self.arm_pomodoro_end();
        self.pomodoro_awake(self.visible.get());
    }

    /// Set the end timer for a running part.
    fn arm_pomodoro_end(&self) {
        let p = self.pomo.state.borrow();
        match p.run {
            Run::Running { .. } => {
                self.pomo
                    .end
                    .set_interval(p.left(SystemTime::now()).max(Duration::from_millis(1)));
                self.pomo.end.restart();
            }
            _ => self.pomo.end.stop(),
        }
    }

    fn pomodoro_went_off(&self) {
        let now = SystemTime::now();
        if !self.pomo.state.borrow().finished(now) {
            // Woken early (the clock changed): wait for the rest.
            self.arm_pomodoro_end();
            return;
        }
        let (was, now_in, plan) = {
            let mut p = self.pomo.state.borrow_mut();
            let was = p.advance(now);
            (was, p.phase, p.plan)
        };
        self.pomodoro_settled();
        let minutes = plan.length(now_in).as_secs() / 60;
        let text = match (was, now_in) {
            (Phase::Focus, Phase::LongBreak) => {
                format!("Focus session done. Time for a long break ({minutes} min)")
            }
            (Phase::Focus, _) => format!("Focus session done. Take {minutes} minutes"),
            (_, _) => format!("Break is over. Focus for {minutes} minutes"),
        };
        // On Plasma the notification pops up by itself.
        if !self.caps.notifications_elsewhere() {
            self.osd(&text, "appointment-soon");
        }
        self.tx
            .notifications
            .tell(notifications::Command::AddLocal {
                app: "Pomodoro".into(),
                summary: text,
                body: String::new(),
            });
        self.tx
            .host
            .tell(host::Command::PlaySound(ALARM_SOUND.into()));
    }

    /// Run the ticker only while it shows anything.
    pub(super) fn pomodoro_awake(&self, visible: bool) {
        if visible && self.pomo.state.borrow().running() {
            if !self.pomo.ticker.running() {
                self.pomo.ticker.restart();
            }
        } else {
            self.pomo.ticker.stop();
        }
        self.push_pomodoro();
    }

    /// Follow the first pomodoro widget's settings.
    pub(super) fn sync_pomodoro(&self) {
        let Some(widget) = self.placed_of(Kind::Pomodoro).into_iter().next() else {
            return;
        };
        let us = self.locale.us;
        let v = &widget.settings;
        let minutes = |opt| get_int(v, opt, us).max(1) as u32;
        let plan = Plan {
            focus_min: minutes(Opt::FocusMinutes),
            short_min: minutes(Opt::ShortBreak),
            long_min: minutes(Opt::LongBreak),
            rounds: minutes(Opt::Rounds),
            auto_start: get_bool(v, Opt::AutoStart, us),
        };
        if self.pomo.state.borrow().plan == plan {
            return;
        }
        self.pomo.state.borrow_mut().set_plan(plan);
        self.pomodoro_settled();
    }

    fn push_pomodoro(&self) {
        let now = SystemTime::now();
        let ui = self.ui();
        let t = ui.global::<UiPomodoro>();
        let p = self.pomo.state.borrow();
        let text: slint::SharedString = format_left(p.left(now)).into();
        if t.get_text() != text {
            t.set_text(text);
        }
        t.set_fraction(p.fraction(now));
        t.set_running(p.running());
        t.set_paused(matches!(p.run, Run::Paused { .. }));
        t.set_label(p.phase.label().into());
        t.set_on_break(p.phase.is_break());
        t.set_rounds(p.plan.rounds as i32);
        t.set_done(p.done as i32);
        let next = p.next_phase();
        t.set_next(
            format!(
                "Next: {}, {} min",
                next.label().to_lowercase(),
                p.plan.length(next).as_secs() / 60
            )
            .into(),
        );
    }
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    let t = ui.global::<UiPomodoro>();
    let a = app.clone();
    t.on_toggle(move || a.pomodoro(|p, now| p.toggle(now)));
    let a = app.clone();
    t.on_reset(move || a.pomodoro(|p, _| p.reset()));
    let a = app.clone();
    t.on_skip(move || a.pomodoro(|p, now| p.skip(now)));
    let w = Rc::downgrade(app);
    app.pomo.ticker.start(TimerMode::Repeated, TICK, move || {
        if let Some(a) = w.upgrade() {
            a.push_pomodoro();
        }
    });
    app.pomo.ticker.stop();
    let w = Rc::downgrade(app);
    app.pomo
        .end
        .start(TimerMode::SingleShot, Duration::from_secs(1), move || {
            if let Some(a) = w.upgrade() {
                a.pomodoro_went_off();
            }
        });
    app.pomo.end.stop();
    app.arm_pomodoro_end();
    app.push_pomodoro();
}

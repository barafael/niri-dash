//! The sleep timer widget: the countdown of `niri_dash_system::sleep`, ticked here, with the few
//! things that touch the machine (which modes it offers, running one, keeping the screen awake)
//! handed to the system.

use super::{App, set_if_changed};
use crate::layout::Kind;
use crate::widget_settings::{Opt, get_bool, get_str};
use crate::{Dash, Sleep as UiSleep, SleepMode, SleepStatus};
use chrono::Local;
use niri_dash_system::{Tell, notifications, sleep};
use slint::{ComponentHandle, Timer, TimerMode, VecModel};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use web_time::Instant;

/// How often the countdown is counted. It shows whole seconds, so a quarter of one is plenty.
const TICK: Duration = Duration::from_millis(250);

/// What the widget's settings in config.kdl say.
#[derive(Clone, PartialEq)]
struct Settings {
    when: String,
    script: String,
    dry_run: bool,
    keep_awake: bool,
}

/// The one countdown every sleep timer widget shows: a machine only goes to sleep once.
#[derive(Default)]
pub(super) struct Sleeper {
    ctl: RefCell<sleep::Controller>,
    /// Counts while a countdown runs, shown or not.
    ticker: Timer,
    /// When it last counted.
    last: Cell<Option<Instant>>,
    /// The script behind the run-script mode.
    script: RefCell<PathBuf>,
    /// The settings last applied, so that a toggle made in the widget is not undone by every
    /// change to the layout.
    settings: RefCell<Option<Settings>>,
    /// Whether the system was last told to keep the screen awake.
    awake: Cell<bool>,
    modes: Rc<VecModel<SleepMode>>,
}

/// Where the run-script mode's script is: the setting, or `sleep.sh` next to config.kdl.
fn script_path(setting: &str) -> PathBuf {
    let setting = setting.trim();
    if setting.is_empty() {
        return crate::files::config_home().join("niri-dash/sleep.sh");
    }
    match setting.strip_prefix("~/") {
        Some(rest) => dirs::home_dir().map_or_else(|| PathBuf::from(setting), |h| h.join(rest)),
        None => PathBuf::from(setting),
    }
}

impl App {
    /// Do what the user asked and show how it stands.
    fn sleep_act(&self, act: sleep::Act) {
        self.sleeper.ctl.borrow_mut().apply(act, &Local::now());
        self.sleep_settled();
    }

    /// Keep the ticker and the screen inhibitor in step with the countdown, and show it.
    fn sleep_settled(&self) {
        let (armed, awake) = {
            let ctl = self.sleeper.ctl.borrow();
            (
                ctl.timer.status() == sleep::Status::Armed,
                ctl.wants_awake(),
            )
        };
        let s = &self.sleeper;
        if armed {
            if !s.ticker.running() {
                s.last.set(Some(Instant::now()));
                s.ticker.restart();
            }
        } else {
            s.ticker.stop();
            s.last.set(None);
        }
        if s.awake.replace(awake) != awake {
            self.tx.sleep.tell(sleep::Command::KeepAwake(awake));
        }
        self.push_sleep();
    }

    fn sleep_tick(&self) {
        let now = Instant::now();
        let elapsed = self
            .sleeper
            .last
            .replace(Some(now))
            .map_or(Duration::ZERO, |then| now.saturating_duration_since(then));
        let fired = self.sleeper.ctl.borrow_mut().tick(elapsed);
        if let Some(mode) = fired {
            // The screen is on its own from here, whatever happens next.
            self.sleep_settled();
            self.sleep_fire(mode);
        }
        self.sleep_settled();
    }

    /// The countdown ran out: do it, or for a dry run say what would have been done.
    fn sleep_fire(&self, mode: sleep::Mode) {
        let script = self.sleeper.script.borrow().clone();
        if !self.sleeper.ctl.borrow().dry_run {
            self.tx.sleep.tell(sleep::Command::Run { mode, script });
            return;
        }
        self.sleeper.ctl.borrow_mut().pretend(mode, &script);
        let text = format!(
            "Sleep timer: would have run {}",
            sleep::describe(mode, &script)
        );
        self.toast(&text);
        // On Plasma the notification pops up by itself.
        if self.caps.notifications_server {
            self.osd(&text, "weather-clear-night");
        }
        self.tx
            .notifications
            .tell(notifications::Command::AddLocal {
                app: "Sleep timer".into(),
                summary: text,
                body: String::new(),
            });
    }

    pub(super) fn sleep_event(&self, event: sleep::Event) {
        match event {
            sleep::Event::Modes(modes) => self.sleeper.ctl.borrow_mut().set_modes(modes),
            sleep::Event::Done { mode, error } => {
                self.sleeper.ctl.borrow_mut().performed(mode, error);
            }
        }
        self.push_sleep();
    }

    /// Follow the first sleep timer widget's settings.
    pub(super) fn sync_sleep(&self) {
        let Some(widget) = self.placed_of(Kind::SleepTimer).into_iter().next() else {
            return;
        };
        let us = self.locale.us;
        let v = &widget.settings;
        let now = Settings {
            when: get_str(v, Opt::SleepWhen, us),
            script: get_str(v, Opt::SleepScript, us),
            dry_run: get_bool(v, Opt::DryRun, us),
            keep_awake: get_bool(v, Opt::KeepAwake, us),
        };
        let before = self.sleeper.settings.replace(Some(now.clone()));
        if before.as_ref() == Some(&now) {
            return;
        }
        let script = script_path(&now.script);
        *self.sleeper.script.borrow_mut() = script.clone();
        {
            let mut ctl = self.sleeper.ctl.borrow_mut();
            if before.as_ref().is_none_or(|b| b.when != now.when) && !ctl.timer.counting() {
                ctl.input = match now.when.trim() {
                    "" => sleep::DEFAULT_WHEN.into(),
                    w => w.into(),
                };
                self.ui()
                    .global::<UiSleep>()
                    .set_when(ctl.input.as_str().into());
            }
            if before.as_ref().is_none_or(|b| b.dry_run != now.dry_run) {
                ctl.dry_run = now.dry_run;
            }
            if before
                .as_ref()
                .is_none_or(|b| b.keep_awake != now.keep_awake)
            {
                ctl.keep_awake = now.keep_awake;
            }
        }
        if before.as_ref().is_none_or(|b| b.script != now.script) {
            self.tx.sleep.tell(sleep::Command::Detect { script });
        }
        self.sleep_settled();
    }

    fn push_sleep(&self) {
        let ui = self.ui();
        let t = ui.global::<UiSleep>();
        let now = Local::now();
        let ctl = self.sleeper.ctl.borrow();
        set_if_changed(
            &self.sleeper.modes,
            ctl.modes
                .iter()
                .map(|m| SleepMode {
                    label: m.label().into(),
                    help: m.help().into(),
                })
                .collect(),
            |a, b| a.label == b.label,
        );
        t.set_selected(ctl.selected as i32);
        let (echo, problem) = match ctl.when() {
            Ok(when) => (when.echo(), String::new()),
            Err(e) => (String::new(), e.to_string()),
        };
        t.set_echo(echo.into());
        t.set_problem(problem.into());
        t.set_status(match ctl.timer.status() {
            sleep::Status::Idle => SleepStatus::Idle,
            sleep::Status::Armed => SleepStatus::Armed,
            sleep::Status::Paused => SleepStatus::Paused,
            sleep::Status::Fired => SleepStatus::Fired,
        });
        t.set_headline(ctl.headline(&now).into());
        t.set_left(sleep::clock_text(ctl.timer.remaining()).into());
        t.set_fraction(ctl.timer.fraction());
        t.set_outcome(ctl.outcome().unwrap_or_default().into());
        t.set_dry_run(ctl.dry_run);
        t.set_keep_awake(ctl.keep_awake);
        t.set_can_arm(ctl.can_arm());
    }
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    let t = ui.global::<UiSleep>();
    t.set_modes(slint::ModelRc::from(app.sleeper.modes.clone()));
    t.set_when(app.sleeper.ctl.borrow().input.as_str().into());
    let a = app.clone();
    t.on_edited(move |text| {
        a.sleeper.ctl.borrow_mut().input = text.to_string();
        a.push_sleep();
    });
    let a = app.clone();
    t.on_select(move |i| a.sleep_act(sleep::Act::Select(i.max(0) as usize)));
    let a = app.clone();
    t.on_arm(move || a.sleep_act(sleep::Act::Arm));
    let a = app.clone();
    t.on_pause_resume(move || a.sleep_act(sleep::Act::PauseResume));
    let a = app.clone();
    t.on_abort(move || a.sleep_act(sleep::Act::Abort));
    let a = app.clone();
    t.on_longer(move || a.sleep_act(sleep::Act::Longer));
    let a = app.clone();
    t.on_shorter(move || a.sleep_act(sleep::Act::Shorter));
    let a = app.clone();
    t.on_toggle_dry_run(move || a.sleep_act(sleep::Act::ToggleDryRun));
    let a = app.clone();
    t.on_toggle_keep_awake(move || a.sleep_act(sleep::Act::ToggleKeepAwake));
    let w = Rc::downgrade(app);
    app.sleeper
        .ticker
        .start(TimerMode::Repeated, TICK, move || {
            if let Some(a) = w.upgrade() {
                a.sleep_tick();
            }
        });
    app.sleeper.ticker.stop();
    *app.sleeper.script.borrow_mut() = script_path("");
    // Which modes does this machine offer? Asked once the widget's settings are known (they name
    // the script); a dashboard without the widget never asks.
    app.push_sleep();
}

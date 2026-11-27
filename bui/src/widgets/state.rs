//! What the widgets keep of their own, apart from what the system reports: the timers, the
//! pomodoro, the sleep timer, the notes, the mascots, where the calendar is.

use crate::layout::{Kind, Placed};
use crate::mascots::{self, Board, Spec, Todo};
use crate::pomodoro::{Phase, Plan, Pomodoro};
use crate::timer::{Countdown, Run, Stopwatch};
use crate::widget_settings::{Opt, get_bool, get_int, get_str};
use chrono::NaiveDate;
use niri_dash_system::{self as sys, Command, Tell, notifications, sleep};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use web_time::Instant;

pub struct State {
    /// The calendar's cursor; None: today.
    pub cal: Option<NaiveDate>,
    pub countdown: Countdown,
    /// The countdown went off: "Done" shows until then.
    pub done_until: Option<Instant>,
    pub stopwatch: Stopwatch,
    pub pomodoro: Pomodoro,
    pub sleep: sleep::Controller,
    pub sleep_script: PathBuf,
    sleep_settings: Option<(String, String, bool, bool)>,
    sleep_last: Option<Instant>,
    sleep_awake: bool,
    /// Note texts by note name, and whether they have unsaved changes.
    pub notes: HashMap<String, (String, bool)>,
    pub board: Board,
    /// The notification widget's do-not-disturb choices are open.
    pub dnd_menu: bool,
    /// The keyboard's place in an open tray menu.
    pub menu_cursor: usize,
    /// What is typed into the Wi-Fi password and Bluetooth fields.
    pub wifi_secret: String,
    pub bt_text: String,
    /// The session button waiting for a second press to confirm.
    pub confirm: Option<(&'static str, Instant)>,
    /// What the sleep timer's field holds.
    awake: bool,
}

impl State {
    pub fn new(saved: &crate::state::State) -> Self {
        let mut countdown = Countdown::default();
        if saved.timer_secs > 0 {
            countdown.length = Duration::from_secs(saved.timer_secs as u64);
        }
        if let Some(end) = saved.timer_end {
            let end = UNIX_EPOCH + Duration::from_millis(end.max(0) as u64);
            if end > SystemTime::now() {
                countdown.state = Run::Running { end };
            }
        }
        let mut pomodoro = Pomodoro::default();
        pomodoro.phase = Phase::from_index(saved.pomodoro.phase);
        pomodoro.done = saved.pomodoro.done.min(pomodoro.plan.rounds);
        if let Some(end) = saved.pomodoro.end {
            let end = UNIX_EPOCH + Duration::from_millis(end.max(0) as u64);
            if end > SystemTime::now() {
                pomodoro.run = Run::Running { end };
            }
        }
        Self {
            cal: None,
            countdown,
            done_until: None,
            stopwatch: Stopwatch::default(),
            pomodoro,
            sleep: sleep::Controller::default(),
            sleep_script: script_path(""),
            sleep_settings: None,
            sleep_last: None,
            sleep_awake: false,
            notes: HashMap::new(),
            board: Board::default(),
            dnd_menu: false,
            menu_cursor: 0,
            wifi_secret: String::new(),
            bt_text: String::new(),
            confirm: None,
            awake: false,
        }
    }

    /// The dashboard came up or went away.
    pub fn awake(&mut self, on: bool, tx: &sys::Sink<Command>) {
        self.awake = on;
        if on {
            self.cal = None;
            self.dnd_menu = false;
            self.confirm = None;
            let todo = self.board.wake(Instant::now());
            run_todo(todo, tx);
        }
    }

    /// Escape in an entered widget: a widget with something open closes it first.
    pub fn escape(&mut self, _id: u32, _tx: &sys::Sink<Command>) -> bool {
        if self.dnd_menu {
            self.dnd_menu = false;
            return true;
        }
        self.confirm.take().is_some()
    }

    /// Timers, the pomodoro, the sleep timer and the mascots. Returns lines to show.
    pub fn tick(
        &mut self,
        tx: &sys::Sink<Command>,
        layout: &[Placed],
        _visible: bool,
    ) -> Vec<String> {
        let mut out = Vec::new();
        let now = SystemTime::now();
        if self.countdown.finished(now) {
            let length = self.countdown.length;
            self.countdown.reset();
            self.done_until = Some(Instant::now() + Duration::from_secs(60));
            let text = format!("{} timer is done", crate::timer::format(length, false));
            alarm(tx, "Timer", &text);
            out.push(text);
        }
        if self.done_until.is_some_and(|t| Instant::now() >= t) {
            self.done_until = None;
        }
        self.sync_pomodoro(layout);
        if self.pomodoro.finished(now) {
            let was = self.pomodoro.advance(now);
            let now_in = self.pomodoro.phase;
            let minutes = self.pomodoro.plan.length(now_in).as_secs() / 60;
            let text = match (was, now_in) {
                (Phase::Focus, Phase::LongBreak) => {
                    format!("Focus session done. Time for a long break ({minutes} min)")
                }
                (Phase::Focus, _) => format!("Focus session done. Take {minutes} minutes"),
                _ => format!("Break is over. Focus for {minutes} minutes"),
            };
            alarm(tx, "Pomodoro", &text);
            out.push(text);
        }
        self.sync_sleep(layout, tx);
        let t = Instant::now();
        let elapsed = self
            .sleep_last
            .replace(t)
            .map_or(Duration::ZERO, |then| t.saturating_duration_since(then));
        if self.sleep.timer.status() != sleep::Status::Armed {
            self.sleep_last = None;
        }
        if let Some(mode) = self.sleep.tick(elapsed) {
            if self.sleep.dry_run {
                self.sleep.pretend(mode, &self.sleep_script);
                let text = format!(
                    "Sleep timer: would have run {}",
                    sleep::describe(mode, &self.sleep_script)
                );
                tx.tell(notifications::Command::AddLocal {
                    app: "Sleep timer".into(),
                    summary: text.clone(),
                    body: String::new(),
                });
                out.push(text);
            } else {
                tx.tell(sleep::Command::Run {
                    mode,
                    script: self.sleep_script.clone(),
                });
            }
        }
        self.settle_sleep(tx);
        self.sync_mascots(layout, tx);
        if self.awake {
            let todo = self.board.tick(Instant::now());
            run_todo(todo, tx);
        }
        out
    }

    // -- the timer and the pomodoro -----------------------------------------------------------

    pub fn countdown(
        &mut self,
        saved: &mut crate::state::State,
        f: impl FnOnce(&mut Countdown, SystemTime),
    ) {
        f(&mut self.countdown, SystemTime::now());
        self.done_until = None;
        saved.timer_secs = self.countdown.length.as_secs() as u32;
        saved.timer_end = match self.countdown.state {
            Run::Running { end } => end
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|d| d.as_millis() as i64),
            _ => None,
        };
        crate::state::save(saved);
    }

    pub fn pomo(
        &mut self,
        saved: &mut crate::state::State,
        f: impl FnOnce(&mut Pomodoro, SystemTime),
    ) {
        f(&mut self.pomodoro, SystemTime::now());
        let p = &self.pomodoro;
        saved.pomodoro = crate::state::Pomodoro {
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
        crate::state::save(saved);
    }

    /// Follow the first pomodoro widget's settings.
    fn sync_pomodoro(&mut self, layout: &[Placed]) {
        let Some(p) = layout.iter().find(|w| w.kind == Kind::Pomodoro) else {
            return;
        };
        let v = &p.settings;
        let mins = |o| get_int(v, o, false).max(1) as u32;
        let plan = Plan {
            focus_min: mins(Opt::FocusMinutes),
            short_min: mins(Opt::ShortBreak),
            long_min: mins(Opt::LongBreak),
            rounds: get_int(v, Opt::Rounds, false).max(1) as u32,
            auto_start: get_bool(v, Opt::AutoStart, false),
        };
        if plan != self.pomodoro.plan {
            self.pomodoro.set_plan(plan);
        }
    }

    // -- the sleep timer ----------------------------------------------------------------------

    pub fn sleep_event(&mut self, e: sleep::Event) {
        match e {
            sleep::Event::Modes(m) => self.sleep.set_modes(m),
            sleep::Event::Done { mode, error } => self.sleep.performed(mode, error),
        }
    }

    pub fn sleep_act(&mut self, act: sleep::Act, tx: &sys::Sink<Command>) {
        self.sleep.apply(act, &chrono::Local::now());
        self.settle_sleep(tx);
    }

    fn settle_sleep(&mut self, tx: &sys::Sink<Command>) {
        let awake = self.sleep.wants_awake();
        if self.sleep_awake != awake {
            self.sleep_awake = awake;
            tx.tell(sleep::Command::KeepAwake(awake));
        }
    }

    fn sync_sleep(&mut self, layout: &[Placed], tx: &sys::Sink<Command>) {
        let Some(p) = layout.iter().find(|w| w.kind == Kind::SleepTimer) else {
            return;
        };
        let v = &p.settings;
        let now = (
            get_str(v, Opt::SleepWhen, false),
            get_str(v, Opt::SleepScript, false),
            get_bool(v, Opt::DryRun, false),
            get_bool(v, Opt::KeepAwake, false),
        );
        let before = self.sleep_settings.replace(now.clone());
        if before.as_ref() == Some(&now) {
            return;
        }
        if before.as_ref().is_none_or(|b| b.0 != now.0) && !self.sleep.timer.counting() {
            self.sleep.input = match now.0.trim() {
                "" => sleep::DEFAULT_WHEN.into(),
                w => w.into(),
            };
        }
        if before.as_ref().is_none_or(|b| b.2 != now.2) {
            self.sleep.dry_run = now.2;
        }
        if before.as_ref().is_none_or(|b| b.3 != now.3) {
            self.sleep.keep_awake = now.3;
        }
        if before.as_ref().is_none_or(|b| b.1 != now.1) {
            self.sleep_script = script_path(&now.1);
            tx.tell(sleep::Command::Detect {
                script: self.sleep_script.clone(),
            });
        }
    }

    // -- notes --------------------------------------------------------------------------------

    pub fn note_text(&self, name: &str) -> String {
        match self.notes.get(name) {
            Some((t, _)) => t.clone(),
            None => crate::files::read(&note_path(name)).unwrap_or_default(),
        }
    }

    pub fn edit_note(&mut self, name: &str, text: String) {
        self.notes.insert(name.to_string(), (text, true));
    }

    pub fn save_notes(&mut self) {
        for (name, (text, dirty)) in &mut self.notes {
            if *dirty {
                let path = note_path(name);
                if let Err(e) = crate::files::write_atomic(&path, text.as_bytes()) {
                    log::warn!("saving {}: {e}", path.display());
                }
                *dirty = false;
            }
        }
    }

    // -- mascots ------------------------------------------------------------------------------

    fn sync_mascots(&mut self, layout: &[Placed], tx: &sys::Sink<Command>) {
        let specs: Vec<Spec> = layout
            .iter()
            .filter(|w| w.kind == Kind::Mascot)
            .map(|p| {
                let v = &p.settings;
                Spec {
                    id: p.id,
                    character: character(&p.settings),
                    wildness: get_int(v, Opt::Wildness, false) as u8,
                    interval: Duration::from_secs(get_int(v, Opt::Interval, false).max(5) as u64),
                    command: get_str(v, Opt::Command, false),
                }
            })
            .collect();
        let todo = self.board.sync(specs, Instant::now());
        run_todo(todo, tx);
    }

    pub fn mascot_said(&mut self, id: u32, stdout: Option<String>) {
        let out = stdout.and_then(|s| mascots::from_command_output(&s));
        self.board.answer(id, out, Instant::now());
    }

    pub fn say(&mut self, say: &mascots::Say) {
        if let Err(e) = self.board.say(say, Instant::now()) {
            log::info!("mascots: {e}");
        }
    }
}

/// The mascot a widget's settings name.
pub fn character(v: &crate::widget_settings::Values) -> mascots::Character {
    get_str(v, Opt::Character, false)
        .parse()
        .unwrap_or(mascots::Character::Schraubi)
}

fn run_todo(todo: Vec<Todo>, tx: &sys::Sink<Command>) {
    for t in todo {
        if let Todo::Run { id, command, name } = t {
            tx.tell(sys::mascot::Command::Run { id, name, command });
        }
    }
}

/// Something ran out: a notification, and the alarm.
fn alarm(tx: &sys::Sink<Command>, app: &str, text: &str) {
    tx.tell(notifications::Command::AddLocal {
        app: app.into(),
        summary: text.into(),
        body: String::new(),
    });
    tx.tell(sys::host::Command::PlaySound(sys::host::Sound::Alarm));
}

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

/// A note name as a file name: letters, digits, '-' and '_'; empty is "notes".
pub fn note_name(raw: &str) -> String {
    let name: String = raw
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let name = name.trim_matches('-').to_string();
    if name.is_empty() {
        "notes".into()
    } else {
        name
    }
}

fn note_path(name: &str) -> PathBuf {
    crate::files::data_home()
        .join("niri-dash/notes")
        .join(format!("{name}.md"))
}

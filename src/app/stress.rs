//! `niri-dash stress`: the dashboard used hard by keyboard alone, on the mock system's made-up
//! data, checking after every key that the dashboard is still in a state that makes sense. A
//! development tool, and the core of `tests/stress.rs`.
//!
//! Nothing outside the process changes:
//! - The mock system stands in for the real one, so no D-Bus, compositor, clipboard or gamma.
//! - The config, state, notes and themes go to a fresh temporary directory, which goes when the
//!   run ends.
//! - There is no control socket.
//!
//! The keys are random but weighted by what the dashboard shows: with nothing entered they mostly
//! open the hints, with the hints up they mostly type a label, inside a widget they move, adjust,
//! activate and go back. Every so often the driver presses Escape until the dashboard closes, which
//! has to happen within a few presses from anywhere. A closed dashboard is opened again, as the
//! hotkey would.
//!
//! On screen the dashboard runs in a normal window (closing it ends the run). Hiding and showing
//! keep that window and only play the animations.

use super::snapshot::press_key;
use super::{App, Error, Result};
use crate::layout::Loaded;
use crate::{Control, Nav, NavMode};
use niri_dash_system::Tell;
use slint::{ComponentHandle, Model};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(clap::Args, Clone)]
pub struct Args {
    /// Keys per second; 0 for as fast as it goes.
    #[arg(long, default_value_t = 4.0)]
    pub rate: f64,
    /// Stop after this many keys [default: until the window is closed, or 2000 headless].
    #[arg(long)]
    pub ops: Option<u64>,
    /// Seed for the keys, to run the same sequence again [default: from the clock].
    #[arg(long)]
    pub seed: Option<u64>,
    /// Lay out this config.kdl (a copy is used; the file isn't changed) instead of the default.
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Run offscreen, without a window.
    #[arg(long)]
    pub headless: bool,
    /// Don't print every key.
    #[arg(long, short)]
    pub quiet: bool,
    /// Go on after a problem and report each different one at the end.
    #[arg(long)]
    pub keep_going: bool,
}

/// How many presses of Escape may pass before the dashboard has to be closed.
const ESCAPE_BOUND: u32 = 12;
/// About how often (in keys) the driver checks that Escape gets out.
const ESCAPE_EVERY: u64 = 60;
/// How many of the last keys a failure report shows.
const RECENT: usize = 30;

/// Something the dashboard got wrong, with what led there.
#[derive(Debug)]
pub struct Failure {
    pub seed: u64,
    /// How many keys had been pressed.
    pub op: u64,
    pub problem: String,
    pub recent: Vec<String>,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "after {} keys (seed {}): {}",
            self.op, self.seed, self.problem
        )?;
        writeln!(f, "the last keys:")?;
        for line in &self.recent {
            writeln!(f, "  {line}")?;
        }
        write!(f, "again: niri-dash stress --seed {}", self.seed)
    }
}

/// splitmix64: small, good enough, and the same everywhere.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }

    /// One of `table`'s keys, as likely as its weight.
    fn weighted<'a>(&mut self, table: &[(u32, &'a str)]) -> &'a str {
        let total: u64 = table.iter().map(|(w, _)| u64::from(*w)).sum();
        let mut at = self.below(total);
        for (w, key) in table {
            if at < u64::from(*w) {
                return key;
            }
            at -= u64::from(*w);
        }
        table.last().map_or("esc", |(_, k)| k)
    }
}

/// Nothing entered: mostly open the hints.
const OFF: &[(u32, &str)] = &[
    (70, "f"),
    (6, "ctrl+e"),
    (4, "esc"),
    (4, "j"),
    (4, "enter"),
    (4, "space"),
    (4, "tab"),
    (4, "x"),
];

/// Inside a widget: move, adjust, activate, go back, type, or go to the next tile.
const WIDGET: &[(u32, &str)] = &[
    (15, "j"),
    (9, "k"),
    (8, "h"),
    (8, "l"),
    (3, "down"),
    (2, "up"),
    (2, "left"),
    (2, "right"),
    (10, "enter"),
    (6, "space"),
    (3, "g"),
    (3, "G"),
    (5, "backspace"),
    (6, "esc"),
    (3, "tab"),
    (2, "backtab"),
    (2, "shift+H"),
    (2, "shift+J"),
    (2, "shift+K"),
    (2, "shift+L"),
    (2, "ctrl+h"),
    (2, "ctrl+j"),
    (2, "ctrl+k"),
    (2, "ctrl+l"),
    (2, "ctrl+right"),
    // Moving the widget, and sizing mode with its keys.
    (2, "ctrl+shift+H"),
    (2, "ctrl+shift+J"),
    (1, "ctrl+shift+K"),
    (1, "ctrl+shift+L"),
    (2, "ctrl+r"),
    (2, "shift+J"),
    (1, "shift+L"),
    (2, "f"),
    (1, "ctrl+e"),
    // Typed into whatever text field is open, or a widget's own letter.
    (2, "a"),
    (2, "e"),
    (2, "n"),
    (2, "s"),
    (2, "1"),
    (2, "0"),
];

/// Edit mode: the keys its controls might take, and the ways out.
const EDIT: &[(u32, &str)] = &[
    (15, "tab"),
    (5, "backtab"),
    (8, "left"),
    (8, "right"),
    (8, "up"),
    (8, "down"),
    (6, "h"),
    (6, "j"),
    (6, "k"),
    (6, "l"),
    (8, "enter"),
    (4, "space"),
    (8, "esc"),
    (3, "ctrl+e"),
    (3, "f"),
    (3, "x"),
];

/// Where the dashboard is, as far as the keys are concerned.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Closed,
    Off,
    Hints,
    Widget,
    Edit,
}

/// Presses the keys and checks what they did.
pub struct Monkey {
    seed: u64,
    rng: Rng,
    done: u64,
    /// Escape presses so far while checking that Escape gets out.
    escaping: Option<u32>,
    recent: VecDeque<String>,
    verbose: bool,
    /// How often each place was seen, and how often Escape got out, for the summary.
    seen: [u64; 5],
    escapes: u64,
    /// Going on after problems: each different one, how often, and the keys that first led there.
    collected: Option<BTreeMap<String, (u64, Vec<String>)>>,
}

impl Monkey {
    pub fn new(seed: u64, verbose: bool) -> Self {
        Self {
            seed,
            rng: Rng(seed),
            done: 0,
            escaping: None,
            recent: VecDeque::new(),
            verbose,
            seen: [0; 5],
            escapes: 0,
            collected: None,
        }
    }

    /// Go on after a problem, and report each different one when the run ends.
    pub fn keep_going(mut self) -> Self {
        self.collected = Some(BTreeMap::new());
        self
    }

    /// A problem: the end of the run, or one more for the report.
    fn report(&mut self, problem: String) -> Result<(), Failure> {
        let context: Vec<String> = self.recent.iter().rev().take(6).rev().cloned().collect();
        match &mut self.collected {
            None => Err(self.fail(problem)),
            Some(all) => {
                all.entry(problem).or_insert((0, context)).0 += 1;
                Ok(())
            }
        }
    }

    /// The end of the run: what was collected, if anything.
    pub fn finish(mut self) -> Result<Self, Failure> {
        let Some(all) = self.collected.take().filter(|a| !a.is_empty()) else {
            return Ok(self);
        };
        let mut text = format!("{} different problems:", all.len());
        for (problem, (count, context)) in &all {
            text.push_str(&format!("\n\n{count} × {problem}"));
            for line in context {
                text.push_str(&format!("\n    {line}"));
            }
        }
        self.recent.clear();
        Err(self.fail(text))
    }

    pub fn done(&self) -> u64 {
        self.done
    }

    fn fail(&self, problem: String) -> Failure {
        Failure {
            seed: self.seed,
            op: self.done,
            problem,
            recent: self.recent.iter().cloned().collect(),
        }
    }

    fn note(&mut self, line: String) {
        if self.verbose {
            println!("{line}");
        }
        if self.recent.len() == RECENT {
            self.recent.pop_front();
        }
        self.recent.push_back(line);
    }

    /// What was done, for the end of a run.
    pub fn summary(&self) -> String {
        let [closed, off, hints, widget, edit] = self.seen;
        format!(
            "{} keys (seed {}): {widget} inside widgets, {hints} with the hints up, {off} with \
             nothing entered, {edit} in edit mode, {closed} reopenings; Escape got out {} times",
            self.done, self.seed, self.escapes
        )
    }

    /// Check the dashboard as the last key left it, then press the next one (or open it again).
    pub(super) fn step(&mut self, app: &App, window: &slint::Window) -> Result<(), Failure> {
        // Slint runs `changed` handlers on its next pass: a list that just grew hasn't scrolled
        // the cursor's row back into sight until then, and the check shouldn't look in between.
        slint::platform::update_timers_and_animations();
        let place = place(app);
        if let Err(p) = check(app) {
            self.report(format!("{}: {p}", kind_name(app)))?;
        }
        if let Some(n) = self.escaping {
            if place == Place::Closed {
                self.escaping = None;
                self.escapes += 1;
                self.note(format!("{:>6}  Escape got out in {n}", self.done));
            } else if n >= ESCAPE_BOUND {
                self.escaping = None;
                self.report(format!(
                    "{}: {n} presses of Escape didn't close the dashboard",
                    kind_name(app)
                ))?;
            }
        }
        self.seen[place as usize] += 1;
        let state = describe(app);
        if place == Place::Closed {
            // As the hotkey would.
            self.note(format!("{:>6}  (open again)", self.done));
            app.control_in_place(Control::Show);
            return Ok(());
        }
        if self.escaping.is_none() && self.rng.below(ESCAPE_EVERY) == 0 {
            self.escaping = Some(0);
        }
        let key: String = match (self.escaping.as_mut(), place) {
            (Some(n), _) => {
                *n += 1;
                "esc".into()
            }
            (None, Place::Hints) => self.hint_key(app),
            (None, Place::Widget) => self.rng.weighted(WIDGET).into(),
            (None, Place::Edit) => self.rng.weighted(EDIT).into(),
            (None, _) => self.rng.weighted(OFF).into(),
        };
        self.done += 1;
        self.note(format!("{:>6}  {key:<10} at {state}", self.done));
        // The keyboard is the dashboard's, also while a watched window lacks the focus.
        window.dispatch_event(slint::platform::WindowEvent::WindowActiveChanged(true));
        press_key(window, &key);
        Ok(())
    }

    /// With the hints up: mostly the next letter of some label, now and then a wrong one or a way
    /// out.
    fn hint_key(&mut self, app: &App) -> String {
        let typed = app.ui().global::<Nav>().get_typed().to_string();
        let roll = self.rng.below(100);
        if roll < 4 {
            return "backspace".into();
        }
        if roll < 7 {
            return "esc".into();
        }
        if roll < 15 {
            return ((b'a' + self.rng.below(26) as u8) as char).to_string();
        }
        let labels: Vec<String> = app
            .widgets
            .iter()
            .map(|w| w.hint.to_string())
            .filter(|h| h.starts_with(&typed) && h.len() > typed.len())
            .collect();
        match labels.get(self.rng.below(labels.len() as u64) as usize) {
            Some(l) => l[typed.len()..]
                .chars()
                .next()
                .map_or("f".into(), String::from),
            None => "f".into(),
        }
    }
}

fn place(app: &App) -> Place {
    let ui = app.ui();
    if !app.visible.get() {
        return Place::Closed;
    }
    if ui.get_editing() {
        return Place::Edit;
    }
    match ui.global::<Nav>().get_mode() {
        NavMode::Off => Place::Off,
        NavMode::Hints => Place::Hints,
        NavMode::Widget => Place::Widget,
    }
}

/// The kind of widget entered, or where the dashboard is otherwise.
fn kind_name(app: &App) -> String {
    let id = app.ui().global::<Nav>().get_active_id();
    match place(app) {
        Place::Widget => app
            .widgets
            .iter()
            .find(|w| w.id == id)
            .map_or(format!("widget {id}"), |w| {
                super::enums::kind_from_ui(w.kind).name().to_string()
            }),
        Place::Edit => "edit mode".into(),
        Place::Hints => "hints".into(),
        Place::Off | Place::Closed => "dashboard".into(),
    }
}

/// Where the dashboard is, for the log.
fn describe(app: &App) -> String {
    let ui = app.ui();
    let n = ui.global::<Nav>();
    match place(app) {
        Place::Closed => "closed".into(),
        Place::Edit => "editing".into(),
        Place::Off => "nothing entered".into(),
        Place::Hints => format!("hints, typed {:?}", n.get_typed().as_str()),
        Place::Widget => {
            let name = kind_name(app);
            match app.spoken() {
                Ok(Some(spoken)) => format!("in {name}: {spoken}"),
                _ => format!("in {name}"),
            }
        }
    }
}

/// What has to hold whatever keys came before.
fn check(app: &App) -> Result<(), String> {
    let ui = app.ui();
    let n = ui.global::<Nav>();
    let widgets: Vec<crate::WidgetInst> = app.widgets.iter().collect();
    let hints: Vec<String> = widgets.iter().map(|w| w.hint.to_string()).collect();
    let mut unique = HashSet::new();
    for h in &hints {
        if h.is_empty() {
            return Err("a widget has no hint label".into());
        }
        if !unique.insert(h) {
            return Err(format!("two widgets have the hint label {h:?}"));
        }
        if let Some(o) = hints.iter().find(|o| *o != h && o.starts_with(h.as_str())) {
            return Err(format!("hint label {h:?} is the start of {o:?}"));
        }
    }
    let typed = n.get_typed().to_string();
    match n.get_mode() {
        NavMode::Widget if !widgets.iter().any(|w| w.id == n.get_active_id()) => {
            return Err(format!(
                "entered widget {} isn't on the dashboard",
                n.get_active_id()
            ));
        }
        NavMode::Hints if !typed.is_empty() && !hints.iter().any(|h| h.starts_with(&typed)) => {
            return Err(format!("typed {typed:?} starts no hint label"));
        }
        NavMode::Off | NavMode::Hints if n.get_sizing() => {
            return Err("sizing with no widget entered".into());
        }
        NavMode::Off if !typed.is_empty() => {
            return Err(format!("typed {typed:?} left over with the hints down"));
        }
        _ => {}
    }
    app.spoken()?;
    if !app.visible.get() {
        if ui.get_editing() {
            return Err("closed while still in edit mode".into());
        }
        if n.get_mode() != NavMode::Off {
            return Err("closed with a widget still entered or the hints up".into());
        }
        if n.get_sizing() || n.get_expanded_id() != 0 {
            return Err("closed while sizing or with a widget expanded".into());
        }
    }
    Ok(())
}

impl App {
    /// As [`App::control`], but keeping the surface: hiding plays the exit and leaves the window
    /// up, empty; showing plays the entrance. For the stress run, whose window stays put.
    pub(super) fn control_in_place(&self, c: Control) {
        let ui = self.ui();
        match c {
            Control::Show | Control::Edit => {
                if !self.visible.get() {
                    self.come_up();
                    ui.set_shown(true);
                    self.set_visible(true);
                    self.set_awake(true);
                }
                if c == Control::Edit {
                    self.set_editing(true);
                }
            }
            Control::Toggle if !self.visible.get() => self.control_in_place(Control::Show),
            Control::Hide | Control::Toggle => {
                if self.visible.get() {
                    self.go_down();
                    ui.set_shown(false);
                    self.set_visible(false);
                    self.set_awake(false);
                    self.pick_if_pending();
                }
            }
            Control::Quit => log::info!("not quitting during a stress run"),
            Control::Inhibit => self
                .tx
                .power
                .tell(niri_dash_system::power::Command::ToggleInhibit),
            Control::Dnd => self.toggle_dnd(),
        }
    }
}

/// The disposable home of a run's files, removed when dropped.
pub struct Sandbox(PathBuf);

impl Sandbox {
    /// Point the dashboard's config, state, data and cache directories at a fresh directory, with
    /// a copy of `config` (or nothing: the default layout) as config.kdl. Call before anything
    /// reads them, and before any other thread runs.
    pub fn enter(config: Option<&std::path::Path>) -> Result<Self> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let dir =
            std::env::temp_dir().join(format!("niri-dash-stress-{}-{nanos:x}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let dir = Self(dir);
        // SAFETY: called first thing, while this is the only thread.
        unsafe {
            for (var, sub) in [
                ("XDG_CONFIG_HOME", "config"),
                ("XDG_STATE_HOME", "state"),
                ("XDG_DATA_HOME", "data"),
                ("XDG_CACHE_HOME", "cache"),
            ] {
                std::env::set_var(var, dir.0.join(sub));
            }
        }
        if let Some(src) = config {
            let text = std::fs::read_to_string(src).map_err(|source| Error::Read {
                path: src.to_path_buf(),
                source,
            })?;
            crate::files::write_atomic(&crate::layout::Layout::path(), text)?;
        }
        Ok(dir)
    }

    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The pace of a run: when the next key is due.
pub(super) struct Pace {
    every: Duration,
    next: Instant,
}

impl Pace {
    pub(super) fn new(rate: f64) -> Self {
        let every = if rate > 0.0 {
            Duration::from_secs_f64(1.0 / rate)
        } else {
            Duration::ZERO
        };
        Self {
            every,
            next: Instant::now(),
        }
    }

    /// How long until the next key (zero if it is due).
    pub(super) fn wait(&self) -> Duration {
        self.next.saturating_duration_since(Instant::now())
    }

    /// The next key is due; the one after comes a beat later.
    pub(super) fn due(&mut self) -> bool {
        let now = Instant::now();
        if now < self.next {
            return false;
        }
        self.next = (self.next + self.every).max(now);
        true
    }
}

/// A stress run in the daemon's loop: the keys, their pace and when to stop.
pub(super) struct Run {
    pub(super) monkey: Monkey,
    pub(super) pace: Pace,
    pub(super) ops: Option<u64>,
}

/// `niri-dash stress`.
pub fn run(args: &Args) -> Result<()> {
    let sandbox = Sandbox::enter(args.config.as_deref())?;
    let seed = args.seed.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64)
    });
    println!(
        "stress run, seed {seed}, {} keys a second; files in {}",
        args.rate,
        sandbox.path().display()
    );
    let monkey = Monkey::new(seed, !args.quiet);
    let monkey = if args.keep_going {
        monkey.keep_going()
    } else {
        monkey
    };
    let monkey = if args.headless {
        let mut h = super::testing::Harness::with_mock(&config_text());
        h.stress(monkey, args.ops.unwrap_or(2000), args.rate)
            .map_err(|f| Error::Stress(f.to_string()))?
    } else {
        super::stress_window(Run {
            monkey,
            pace: Pace::new(args.rate),
            ops: args.ops,
        })?
    };
    println!("{}", monkey.summary());
    monkey
        .finish()
        .map(|_| ())
        .map_err(|f| Error::Stress(f.to_string()))
}

/// The config.kdl text in the sandbox (the default layout if there is none), as loaded.
fn config_text() -> String {
    let Loaded { text, .. } = crate::layout::Layout::load();
    text
}

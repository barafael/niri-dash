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

use super::enums::style_var_to_ui;
use super::snapshot::press_key;
use super::{App, Error, Result};
use crate::layout::Loaded;
use crate::{Control, Nav, NavMode, theme};
use niri_dash_system::Tell;
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, Model};
use std::cell::Cell;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};
use strum::IntoEnumIterator;

#[derive(clap::Args, Clone)]
pub struct Args {
    /// Keys per second; 0 for as fast as it goes.
    #[arg(long, default_value_t = 4.0)]
    pub rate: f64,
    /// Stop after this many keys [default: until the window is closed, or 2000 headless].
    #[arg(long)]
    pub ops: Option<u64>,
    /// On screen: seconds to speed up from a crawl to --rate at the start.
    #[arg(long, default_value_t = 0.0)]
    pub ramp_up: f64,
    /// On screen: seconds to slow down to a crawl before --ops keys are done.
    #[arg(long, default_value_t = 0.0)]
    pub ramp_down: f64,
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
/// About how many steps in a hundred are something other than a key: a new theme, a click, a
/// control. Zero keeps a run to keys alone.
const EXTRA_PERCENT: u64 = 12;
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
    (2, "w"),
    (2, "d"),
    // Moving the widget, and sizing it in edit mode.
    (2, "ctrl+shift+H"),
    (2, "ctrl+shift+J"),
    (1, "ctrl+shift+K"),
    (1, "ctrl+shift+L"),
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

/// The keys that move a list's cursor, and with it bring its row back into view.
const CURSOR_KEYS: [&str; 10] = [
    "j", "k", "g", "G", "up", "down", "home", "end", "tab", "backtab",
];

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
    /// Steps that weren't keys (themes, clicks, controls), for the summary.
    extras: u64,
    /// A new look is shown at once, not blended over a third of a second on the real clock: a
    /// headless run presses keys faster than that, and would check the dashboard half-way
    /// between two looks, at moments that depend on how fast the machine is.
    instant_looks: bool,
    /// A wheel turned since a key last moved a cursor: it may have scrolled the cursor's row out
    /// of a list's view (the sidebar's or a widget's), and that is as the person wanted it.
    wheeled: bool,
    /// What the cursor was on when a key that could move it was pressed after the wheel. If it is
    /// on something else after that, the key moved it, and has to have brought its row into view;
    /// if not (`up` on the first row), there was nothing to bring back.
    wheel_cursor: Option<String>,
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
            extras: 0,
            instant_looks: false,
            wheeled: false,
            wheel_cursor: None,
            collected: None,
        }
    }

    /// For a headless run: show new looks at once (there is no time to blend them in) and leave
    /// the choice of font alone (it depends on the machine).
    pub fn instant_looks(mut self) -> Self {
        self.instant_looks = true;
        self
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
            "{} steps (seed {}), {} of them other than keys: {widget} inside widgets, {hints} \
             with the hints up, {off} with nothing entered, {edit} in edit mode, {closed} \
             reopenings; Escape got out {} times",
            self.done, self.seed, self.extras, self.escapes
        )
    }

    /// Check the dashboard as the last key left it, then press the next one (or open it again).
    /// What it does to the window is marked as the run's own, so that it never presses the run's
    /// pause button or its pace controls.
    pub(super) fn step(&mut self, app: &App, window: &slint::Window) -> Result<(), Failure> {
        app.ui().set_stress_synthetic(true);
        let result = self.step_inner(app, window);
        app.ui().set_stress_synthetic(false);
        result
    }

    fn step_inner(&mut self, app: &App, window: &slint::Window) -> Result<(), Failure> {
        // Slint runs `changed` handlers on its next pass: a list that just grew hasn't scrolled
        // the cursor's row back into sight until then, and the check shouldn't look in between.
        slint::platform::update_timers_and_animations();
        let place = place(app);
        if self.wheeled
            && let Some(before) = &self.wheel_cursor
            && app.cursor_signature() != *before
        {
            self.wheeled = false;
            self.wheel_cursor = None;
        }
        if let Err(p) = check(app)
            && !(self.wheeled && scrolled_away(&p))
        {
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
        // Now and then something other than a key: a new theme, a click, a control.
        if self.escaping.is_none() && self.rng.below(100) < EXTRA_PERCENT {
            window.dispatch_event(WindowEvent::WindowActiveChanged(true));
            let what = self.extra(app, window);
            self.done += 1;
            self.extras += 1;
            self.note(format!("{:>6}  {what} at {state}", self.done));
            return Ok(());
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
        if self.wheeled && CURSOR_KEYS.contains(&key.as_str()) {
            self.wheel_cursor = Some(app.cursor_signature());
        }
        self.note(format!("{:>6}  {key:<10} at {state}", self.done));
        // The keyboard is the dashboard's, also while a watched window lacks the focus.
        window.dispatch_event(slint::platform::WindowEvent::WindowActiveChanged(true));
        press_key(window, &key);
        Ok(())
    }

    /// Something a person does besides pressing a key, at random: change the color scheme or the
    /// style, adjust or reset a style variable, click or scroll somewhere, or ask for a control
    /// the way a hotkey or the control socket would. Says what it did.
    fn extra(&mut self, app: &App, window: &slint::Window) -> String {
        let ui = app.ui();
        match self.rng.below(100) {
            0..=19 => {
                let name = self.pick(theme::list(&theme::colorschemes_dir()));
                ui.invoke_choose_colorscheme(name.as_str().into());
                self.settle_look(app);
                format!("color scheme {name}")
            }
            20..=34 => {
                let name = self.pick(theme::list(&theme::styles_dir()));
                ui.invoke_choose_style(name.as_str().into());
                self.settle_look(app);
                format!("style {name}")
            }
            35..=49 => {
                let vars: Vec<theme::StyleVar> = theme::StyleVar::iter().collect();
                let mut var = vars[self.rng.below(vars.len() as u64) as usize];
                let delta = if self.rng.below(2) == 0 { -1 } else { 1 };
                // Fonts are whatever the machine has: a headless run, which has to come out the
                // same everywhere, adjusts the text size instead.
                if self.instant_looks && var == theme::StyleVar::FontFamily {
                    var = theme::StyleVar::FontBody;
                }
                ui.invoke_style_step(style_var_to_ui(var), delta);
                self.settle_look(app);
                format!("{} {delta:+}", var.as_ref())
            }
            50..=53 => {
                let vars: Vec<theme::StyleVar> = theme::StyleVar::iter().collect();
                let var = vars[self.rng.below(vars.len() as u64) as usize];
                ui.invoke_style_reset(style_var_to_ui(var));
                self.settle_look(app);
                format!("reset {}", var.as_ref())
            }
            54..=55 => {
                ui.invoke_style_reset_all();
                self.settle_look(app);
                "reset all style adjustments".into()
            }
            56..=79 => {
                let at = self.point(window);
                window.dispatch_event(WindowEvent::PointerMoved { position: at });
                window.dispatch_event(WindowEvent::PointerPressed {
                    position: at,
                    button: PointerEventButton::Left,
                });
                window.dispatch_event(WindowEvent::PointerReleased {
                    position: at,
                    button: PointerEventButton::Left,
                });
                format!("click at {:.0},{:.0}", at.x, at.y)
            }
            80..=87 => {
                let at = self.point(window);
                let (dx, dy) = (
                    0.0,
                    [-120.0, -40.0, 40.0, 120.0][self.rng.below(4) as usize],
                );
                self.wheeled = true;
                self.wheel_cursor = None;
                window.dispatch_event(WindowEvent::PointerMoved { position: at });
                window.dispatch_event(WindowEvent::PointerScrolled {
                    position: at,
                    delta_x: dx,
                    delta_y: dy,
                });
                format!("scroll {dy:+} at {:.0},{:.0}", at.x, at.y)
            }
            88..=91 => {
                app.toggle_dnd();
                "Do Not Disturb toggled".into()
            }
            92..=94 => {
                app.control_in_place(Control::Inhibit);
                "idle inhibit toggled".into()
            }
            95..=97 => {
                app.control_in_place(Control::Edit);
                "edit mode asked for".into()
            }
            _ => {
                app.control_in_place(Control::Toggle);
                "hotkey toggle".into()
            }
        }
    }

    /// With [`Monkey::instant_looks`]: the look just chosen, shown in full now.
    fn settle_look(&self, app: &App) {
        if self.instant_looks {
            app.apply_look(false);
        }
    }

    /// One of `names`, at random ("default" if there are none).
    fn pick(&mut self, names: Vec<String>) -> String {
        let at = self.rng.below(names.len() as u64) as usize;
        names
            .into_iter()
            .nth(at)
            .unwrap_or_else(|| "default".into())
    }

    /// A point in the window, a little past its edges now and then.
    fn point(&mut self, window: &slint::Window) -> LogicalPosition {
        let size = window.size().to_logical(window.scale_factor());
        let along = |rng: &mut Rng, extent: f32| (rng.below(1100) as f32 / 1000.0 - 0.05) * extent;
        LogicalPosition::new(
            along(&mut self.rng, size.width),
            along(&mut self.rng, size.height),
        )
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

/// Whether the check's complaint is about a cursor whose row a list has been scrolled away from.
fn scrolled_away(problem: &str) -> bool {
    [
        "scrolled out of view",
        "cursor is on nothing",
        "cursor is out of sight",
    ]
    .iter()
    .any(|m| problem.contains(m))
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
        Place::Off => format!("nothing entered [focus {}]", app.focus_description()),
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
        if n.get_expanded_id() != 0 {
            return Err("closed with a widget expanded".into());
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
            std::env::set_var("NIRI_DASH_HOME", &dir.0);
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

    /// Keys at `rate` a second from now on (0: as fast as it goes).
    pub(super) fn set_rate(&mut self, rate: f64) {
        self.every = if rate > 0.0 {
            Duration::from_secs_f64(1.0 / rate)
        } else {
            Duration::ZERO
        };
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

/// What the pace panel on screen sets: all of it can change while the run goes.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Tuning {
    paused: bool,
    /// Keys a second; 0 for as fast as it goes.
    rate: f64,
    /// Stop after this many, or never.
    total: Option<u64>,
    /// Seconds from a crawl up to `rate` at the start, and back down before the `total` is reached.
    ramp_up: f64,
    ramp_down: f64,
}

/// The values the panel's − and + step through, in order. Infinity stands for "as fast as it goes"
/// and "never stops".
const RATES: &[f64] = &[
    0.5,
    1.0,
    2.0,
    4.0,
    8.0,
    16.0,
    32.0,
    64.0,
    128.0,
    f64::INFINITY,
];
const TOTALS: &[f64] = &[
    100.0,
    250.0,
    500.0,
    1000.0,
    2500.0,
    5000.0,
    10_000.0,
    25_000.0,
    100_000.0,
    f64::INFINITY,
];
const RAMPS: &[f64] = &[0.0, 1.0, 2.0, 5.0, 10.0, 20.0, 30.0, 60.0];

/// The next value of `ladder` above `now` (or below, with a negative `delta`); `now` itself when
/// there is none.
fn stepped(ladder: &[f64], now: f64, delta: i32) -> f64 {
    let found = if delta > 0 {
        ladder.iter().copied().find(|v| *v > now)
    } else {
        ladder.iter().rev().copied().find(|v| *v < now)
    };
    found.unwrap_or(now)
}

impl Tuning {
    /// A panel row's − (negative `delta`) or +. The rows are rate, events, ramp up and ramp down.
    fn step(&mut self, row: i32, delta: i32) {
        let unbounded = |v: f64| if v > 0.0 { v } else { f64::INFINITY };
        match row {
            0 => {
                let v = stepped(RATES, unbounded(self.rate), delta);
                self.rate = if v.is_finite() { v } else { 0.0 };
            }
            1 => {
                let now = self.total.map_or(f64::INFINITY, |n| n as f64);
                let v = stepped(TOTALS, now, delta);
                self.total = v.is_finite().then_some(v as u64);
            }
            2 => self.ramp_up = stepped(RAMPS, self.ramp_up, delta),
            3 => self.ramp_down = stepped(RAMPS, self.ramp_down, delta),
            _ => {}
        }
    }

    /// The panel's values as text, in the order of its rows.
    fn texts(&self) -> Vec<slint::SharedString> {
        let seconds = |s: f64| {
            if s > 0.0 {
                format!("{s}s")
            } else {
                "off".into()
            }
        };
        vec![
            if self.rate > 0.0 {
                format!("{}/s", self.rate)
            } else {
                "max".into()
            }
            .into(),
            self.total.map_or("∞".into(), |n| n.to_string()).into(),
            seconds(self.ramp_up).into(),
            seconds(self.ramp_down).into(),
        ]
    }

    /// Keys a second right now (0: as fast as it goes), `running` seconds into the run with `done`
    /// keys pressed. The rate climbs in a straight line over `ramp_up`; at the end it falls in a
    /// straight line to a crawl, starting when just as many keys are left as that fall will press
    /// (at half the rate on average, over `ramp_down`), so that the last key comes as it ends.
    fn rate_now(&self, running: f64, done: u64) -> f64 {
        if self.rate <= 0.0 {
            return 0.0;
        }
        let crawl = (self.rate * 0.05).max(0.5).min(self.rate);
        let mut share = 1.0_f64;
        if self.ramp_up > 0.0 {
            share = share.min(running / self.ramp_up);
        }
        if let (Some(total), true) = (self.total, self.ramp_down > 0.0) {
            let left = total.saturating_sub(done) as f64;
            share = share.min((left / (self.rate * self.ramp_down / 2.0)).sqrt());
        }
        (self.rate * share).max(crawl)
    }
}

/// A stress run in the daemon's loop: the keys, their pace and when to stop.
pub(super) struct Run {
    pub(super) monkey: Monkey,
    pub(super) pace: Pace,
    /// What the panel on screen sets, shared with its callbacks.
    tuning: Rc<Cell<Tuning>>,
    /// What the window shows of it was last made from this.
    shown: Option<(Tuning, u64, u64)>,
    /// How long the run has gone on, not counting the time it was paused.
    running: Duration,
    last: Option<Instant>,
}

/// What a turn of the loop found.
#[derive(PartialEq, Eq)]
pub(super) enum Drive {
    Going,
    Finished,
}

impl Run {
    pub(super) fn new(monkey: Monkey, args: &Args) -> Self {
        let tuning = Tuning {
            paused: false,
            rate: args.rate.max(0.0),
            total: args.ops,
            ramp_up: args.ramp_up.max(0.0),
            ramp_down: args.ramp_down.max(0.0),
        };
        Self {
            monkey,
            pace: Pace::new(tuning.rate_now(0.0, 0)),
            tuning: Rc::new(Cell::new(tuning)),
            shown: None,
            running: Duration::ZERO,
            last: None,
        }
    }

    /// Put the pause button and the pace panel on the dashboard, and hear from them.
    pub(super) fn attach(&self, app: &App) {
        let ui = app.ui();
        ui.set_stress(true);
        let tuning = self.tuning.clone();
        ui.on_stress_toggle(move || {
            let mut t = tuning.get();
            t.paused = !t.paused;
            tuning.set(t);
        });
        let tuning = self.tuning.clone();
        ui.on_stress_step(move |row, delta| {
            let mut t = tuning.get();
            t.step(row, delta);
            tuning.set(t);
        });
    }

    /// How long the loop may sleep before the next key is due: for as long as it likes when the
    /// run is paused.
    pub(super) fn wait(&self) -> Duration {
        if self.tuning.get().paused {
            Duration::MAX
        } else {
            self.pace.wait()
        }
    }

    /// One turn of the loop: show what the panel shows, and press the next key if one is due.
    pub(super) fn drive(&mut self, app: &App, window: &slint::Window) -> Result<Drive, Failure> {
        let now = Instant::now();
        let tuning = self.tuning.get();
        if let Some(last) = self.last.replace(now)
            && !tuning.paused
        {
            self.running += now.saturating_duration_since(last);
        }
        let done = self.monkey.done();
        let rate = tuning.rate_now(self.running.as_secs_f64(), done);
        self.show(app, tuning, rate);
        if tuning.total.is_some_and(|n| done >= n) {
            return Ok(Drive::Finished);
        }
        if !tuning.paused {
            self.pace.set_rate(rate);
            if self.pace.due() {
                self.monkey.step(app, window)?;
            }
        }
        Ok(Drive::Going)
    }

    /// Update the window's pause button and panel when something they show has changed.
    fn show(&mut self, app: &App, tuning: Tuning, rate: f64) {
        let key = (tuning, self.monkey.done(), (rate * 10.0).round() as u64);
        if self.shown == Some(key) {
            return;
        }
        self.shown = Some(key);
        let ui = app.ui();
        ui.set_stress_paused(tuning.paused);
        ui.set_stress_values(slint::ModelRc::new(slint::VecModel::from(tuning.texts())));
        let pace = if rate > 0.0 {
            format!("{rate:.1}/s")
        } else {
            "max".into()
        };
        let status = match (tuning.paused, tuning.total) {
            (true, Some(n)) => format!("paused · {} of {n}", key.1),
            (true, None) => format!("paused · {}", key.1),
            (false, Some(n)) => format!("{} of {n} · {pace}", key.1),
            (false, None) => format!("{} · {pace}", key.1),
        };
        ui.set_stress_status(status.into());
    }
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
        super::stress_window(Run::new(monkey, args))?
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tuning() -> Tuning {
        Tuning {
            paused: false,
            rate: 10.0,
            total: Some(1000),
            ramp_up: 0.0,
            ramp_down: 0.0,
        }
    }

    #[test]
    fn the_rate_is_steady_without_ramps() {
        assert_eq!(tuning().rate_now(0.0, 0), 10.0);
        assert_eq!(tuning().rate_now(100.0, 999), 10.0);
    }

    #[test]
    fn the_rate_climbs_over_the_ramp_up_and_holds() {
        let t = Tuning {
            ramp_up: 10.0,
            ..tuning()
        };
        assert_eq!(t.rate_now(0.0, 0), 0.5);
        assert!((t.rate_now(5.0, 0) - 5.0).abs() < 1e-9);
        assert_eq!(t.rate_now(10.0, 0), 10.0);
        assert_eq!(t.rate_now(60.0, 0), 10.0);
    }

    #[test]
    fn the_rate_falls_over_the_last_keys_and_never_stops() {
        let t = Tuning {
            ramp_down: 10.0,
            ..tuning()
        };
        // 10 keys a second falling to nothing over 10 s presses 50 keys.
        assert_eq!(t.rate_now(0.0, 900), 10.0);
        assert_eq!(t.rate_now(0.0, 950), 10.0);
        assert!(t.rate_now(0.0, 975) < 10.0);
        // One key left: sqrt(1 / 50) of the rate, a bit over the crawl; none left: the crawl.
        assert!((t.rate_now(0.0, 999) - 10.0 * (1.0_f64 / 50.0).sqrt()).abs() < 1e-9);
        assert_eq!(t.rate_now(0.0, 1000), 0.5);
    }

    #[test]
    fn as_fast_as_it_goes_has_no_ramps() {
        let t = Tuning {
            rate: 0.0,
            ramp_up: 10.0,
            ramp_down: 10.0,
            ..tuning()
        };
        assert_eq!(t.rate_now(0.0, 999), 0.0);
    }

    #[test]
    fn the_panel_steps_through_the_ladders_and_stops_at_their_ends() {
        let mut t = tuning();
        t.step(0, 1);
        assert_eq!(t.rate, 16.0);
        t.rate = 128.0;
        t.step(0, 1);
        assert_eq!(t.rate, 0.0, "past the top is as fast as it goes");
        t.step(0, 1);
        assert_eq!(t.rate, 0.0);
        t.step(0, -1);
        assert_eq!(t.rate, 128.0);
        t.total = Some(1000);
        t.step(1, 1);
        assert_eq!(t.total, Some(2500));
        t.total = Some(100_000);
        t.step(1, 1);
        assert_eq!(t.total, None);
        t.step(1, -1);
        assert_eq!(t.total, Some(100_000));
        t.total = Some(777);
        t.step(1, -1);
        assert_eq!(t.total, Some(500));
        t.step(2, -1);
        assert_eq!(t.ramp_up, 0.0);
        t.step(3, 1);
        t.step(3, 1);
        assert_eq!(t.ramp_down, 2.0);
    }
}

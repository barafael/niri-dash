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
//! On screen (the default) the keys go to a real window at `--rate` a second, with a pause button
//! and a panel for the pace in a window of their own (`live`). `--headless` runs on GPUI's test
//! platform instead, as fast as it goes (`tests/stress.rs` is that).

mod live;

use crate::Control;
use crate::dash::{Dash, NavOp, Place};
use crate::testing::Harness;
use crate::theme;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::path::PathBuf;
use strum::IntoEnumIterator;

#[derive(clap::Args, Clone)]
pub struct Args {
    /// Keys per second on screen; 0 for as fast as it goes.
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
    /// Run on GPUI's test platform, without a window.
    #[arg(long)]
    pub headless: bool,
    /// Seed for the keys, to run the same sequence again [default: from the clock].
    #[arg(long)]
    pub seed: Option<u64>,
    /// Lay out this config.kdl (a copy is used; the file isn't changed) instead of the default.
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Don't print every key.
    #[arg(long, short)]
    pub quiet: bool,
    /// Go on after a problem and report each different one at the end.
    #[arg(long)]
    pub keep_going: bool,
}

/// What the monkey does to the dashboard and reads of it: the headless harness, or a window on
/// screen (`live`).
pub trait Rig {
    /// Read the dashboard.
    fn read<R>(&self, f: impl FnOnce(&Dash) -> R) -> R;
    /// Change the dashboard, as the app itself would.
    fn update<R>(&mut self, f: impl FnOnce(&mut Dash, &mut gpui::Context<Dash>) -> R) -> R;
    /// A control, as the hotkey or the control socket would send it.
    fn control(&mut self, c: Control);
    /// Press a key by name (`"j"`, `"ctrl+h"`, `"esc"`).
    fn key(&mut self, name: &str);
    /// What a screen reader would say about where the keyboard is (`Err`: it makes no sense).
    /// A rig with no accessible tree has nothing to say.
    fn spoken(&mut self) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn click(&mut self, x: f32, y: f32);
    fn click_with(&mut self, x: f32, y: f32, button: gpui::MouseButton);
    fn double_click(&mut self, x: f32, y: f32);
    /// The pointer moves, no button down.
    fn move_to(&mut self, x: f32, y: f32);
    /// Press at `from`, move to `to` in `steps` steps, release.
    fn drag(&mut self, from: (f32, f32), to: (f32, f32), steps: u32);
    /// Turn the wheel at a point.
    fn scroll(&mut self, x: f32, y: f32, dy: f32);
    /// The window's size in pixels.
    fn size(&self) -> (u32, u32);
}

impl Rig for Harness {
    fn read<R>(&self, f: impl FnOnce(&Dash) -> R) -> R {
        Harness::read(self, f)
    }
    fn update<R>(&mut self, f: impl FnOnce(&mut Dash, &mut gpui::Context<Dash>) -> R) -> R {
        Harness::update(self, f)
    }
    fn control(&mut self, c: Control) {
        Harness::control(self, c)
    }
    fn key(&mut self, name: &str) {
        Harness::key(self, name)
    }
    fn spoken(&mut self) -> Result<Option<String>, String> {
        Harness::spoken(self)
    }
    fn click(&mut self, x: f32, y: f32) {
        Harness::click(self, x, y)
    }
    fn click_with(&mut self, x: f32, y: f32, button: gpui::MouseButton) {
        Harness::click_with(self, x, y, button)
    }
    fn double_click(&mut self, x: f32, y: f32) {
        Harness::double_click(self, x, y)
    }
    fn move_to(&mut self, x: f32, y: f32) {
        Harness::move_to(self, x, y)
    }
    fn drag(&mut self, from: (f32, f32), to: (f32, f32), steps: u32) {
        Harness::drag(self, from, to, steps)
    }
    fn scroll(&mut self, x: f32, y: f32, dy: f32) {
        Harness::scroll(self, x, y, dy)
    }
    fn size(&self) -> (u32, u32) {
        Harness::size(self)
    }
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
/// How long a key takes a person, as far as the dashboard's clock is concerned, in seconds: a
/// tick of the dashboard's clock every this many keys.
const KEYS_PER_TICK: u64 = 8;

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
    // The move and resize modes, and what their keys do.
    (3, "ctrl+m"),
    (3, "ctrl+r"),
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

/// Edit mode: the keys its controls might take (the sidebar's rows, the widget being worked on:
/// pick the next, move or resize it, reshape, pin, remove), and the ways out.
const EDIT: &[(u32, &str)] = &[
    (14, "tab"),
    (5, "backtab"),
    (6, "left"),
    (6, "right"),
    (6, "up"),
    (6, "down"),
    (6, "h"),
    (6, "j"),
    (6, "k"),
    (6, "l"),
    (3, "H"),
    (3, "J"),
    (3, "K"),
    (3, "L"),
    (3, "ctrl+m"),
    (3, "ctrl+r"),
    (4, "w"),
    (4, "a"),
    (4, "s"),
    (4, "d"),
    (8, "enter"),
    (4, "space"),
    (4, "backspace"),
    (2, "g"),
    (2, "G"),
    (1, "home"),
    (1, "end"),
    (8, "esc"),
    (3, "ctrl+e"),
    (3, "f"),
    (2, "o"),
    (2, "p"),
    (2, "x"),
];

/// The move or resize mode (Ctrl+M, Ctrl+R) is on: its keys, the ones that switch or end it, and
/// others that must do nothing until it ends.
const OP: &[(u32, &str)] = &[
    (8, "h"),
    (8, "j"),
    (8, "k"),
    (8, "l"),
    (4, "H"),
    (4, "J"),
    (4, "K"),
    (4, "L"),
    (3, "left"),
    (3, "down"),
    (3, "up"),
    (3, "right"),
    (2, "shift+left"),
    (2, "shift+right"),
    (3, "ctrl+m"),
    (3, "ctrl+r"),
    (6, "esc"),
    (1, "ctrl+e"),
    (1, "f"),
    (1, "x"),
    (1, "o"),
    (1, "p"),
    (1, "enter"),
    (1, "tab"),
    (1, "e"),
    (1, "s"),
];

/// A text field of the settings card has the keyboard: what is typed into it, and the ways out.
const FIELD: &[(u32, &str)] = &[
    (30, "a"),
    (10, "b"),
    (10, "1"),
    (6, "/"),
    (6, "space"),
    (6, "backspace"),
    (10, "enter"),
    (8, "esc"),
    (6, "tab"),
    (4, "backtab"),
    (3, "left"),
    (3, "right"),
    (2, "f"),
    (2, "j"),
];

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
    /// A wheel turned since a key last moved a cursor: it may have scrolled the cursor's row out
    /// of a list's view, and that is as the person wanted it.
    wheeled: bool,
    /// Where the cursor was when a key that could move it was pressed after the wheel. If it is
    /// somewhere else after that, the key moved it, and has to have brought its row into view; if
    /// not (`up` on the first row), there was nothing to bring back and the excuse stays.
    wheel_cursor: Option<String>,
    /// Going on after problems: each different one, how often, and the keys that first led there.
    collected: Option<BTreeMap<String, (u64, Vec<String>)>>,
    /// Whether the screen reader's view is checked after every key ([`Monkey::speaking`]).
    speaking: bool,
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
            wheeled: false,
            wheel_cursor: None,
            collected: None,
            speaking: false,
        }
    }

    /// Go on after a problem, and report each different one when the run ends.
    pub fn keep_going(mut self) -> Self {
        self.collected = Some(BTreeMap::new());
        self
    }

    /// Also check, after every key, what a screen reader would say about where the keyboard is
    /// (the accessible tree is built for it each time: slower, so a choice).
    pub fn speaking(mut self) -> Self {
        self.speaking = true;
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
    fn step(&mut self, h: &mut impl Rig) -> Result<(), Failure> {
        let (place, state, kind) = h.read(|d| (d.place(), describe(d), kind_name(d)));
        if self.wheeled
            && let Some(before) = &self.wheel_cursor
            && state != *before
        {
            self.wheeled = false;
            self.wheel_cursor = None;
        }
        if let Err(p) = h.read(check)
            && !(self.wheeled && scrolled_away(&p))
        {
            self.report(format!("{kind}: {p}"))?;
        }
        if self.speaking
            && let Err(p) = h.spoken()
            && !(self.wheeled && scrolled_away(&p))
        {
            self.report(format!("{kind}: screen reader: {p}"))?;
        }
        if let Some(n) = self.escaping {
            if place == Place::Closed {
                self.escaping = None;
                self.escapes += 1;
                self.note(format!("{:>6}  Escape got out in {n}", self.done));
            } else if n >= ESCAPE_BOUND {
                self.escaping = None;
                self.report(format!(
                    "{kind}: {n} presses of Escape didn't close the dashboard"
                ))?;
            }
        }
        self.seen[place as usize] += 1;
        if place == Place::Closed {
            // As the hotkey would.
            self.note(format!("{:>6}  (open again)", self.done));
            h.control(Control::Show);
            return Ok(());
        }
        if self.escaping.is_none() && self.rng.below(ESCAPE_EVERY) == 0 {
            self.escaping = Some(0);
        }
        // Now and then something other than a key: a new theme, a click, a control.
        if self.escaping.is_none() && self.rng.below(100) < EXTRA_PERCENT {
            let what = self.extra(h);
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
            (None, Place::Hints) => self.hint_key(h),
            (None, Place::Widget | Place::Edit) if h.read(|d| d.op() != NavOp::None) => {
                self.rng.weighted(OP).into()
            }
            (None, Place::Widget) => self.rng.weighted(WIDGET).into(),
            (None, Place::Edit) => {
                if h.read(|d| d.edit.field.focused) {
                    self.rng.weighted(FIELD).into()
                } else {
                    self.rng.weighted(EDIT).into()
                }
            }
            (None, _) => self.rng.weighted(OFF).into(),
        };
        self.done += 1;
        if self.wheeled && CURSOR_KEYS.contains(&key.as_str()) {
            self.wheel_cursor = Some(state.clone());
        }
        self.note(format!("{:>6}  {key:<10} at {state}", self.done));
        h.key(&key);
        Ok(())
    }

    /// Something a person does besides pressing a key, at random: change the color scheme or the
    /// style, adjust or reset a style variable, click or scroll somewhere, or ask for a control
    /// the way a hotkey or the control socket would. Says what it did.
    fn extra(&mut self, h: &mut impl Rig) -> String {
        match self.rng.below(100) {
            0..=19 => {
                let name = self.pick(theme::list(&theme::colorschemes_dir()));
                h.update(|d, _| {
                    d.choose_colorscheme(&name);
                    d.apply_look(false);
                });
                format!("color scheme {name}")
            }
            20..=34 => {
                let name = self.pick(theme::list(&theme::styles_dir()));
                h.update(|d, _| {
                    d.choose_style(&name);
                    d.apply_look(false);
                });
                format!("style {name}")
            }
            35..=49 => {
                let vars: Vec<theme::StyleVar> = theme::StyleVar::iter().collect();
                let mut var = vars[self.rng.below(vars.len() as u64) as usize];
                let delta = if self.rng.below(2) == 0 { -1 } else { 1 };
                // Fonts are whatever the machine has: a headless run, which has to come out the
                // same everywhere, adjusts the text size instead.
                if var == theme::StyleVar::FontFamily {
                    var = theme::StyleVar::FontBody;
                }
                h.update(|d, _| {
                    d.style_step(var, delta);
                    d.apply_look(false);
                });
                format!("{} {delta:+}", var.as_ref())
            }
            50..=53 => {
                let vars: Vec<theme::StyleVar> = theme::StyleVar::iter().collect();
                let var = vars[self.rng.below(vars.len() as u64) as usize];
                h.update(|d, _| {
                    d.style_reset(Some(var));
                    d.apply_look(false);
                });
                format!("reset {}", var.as_ref())
            }
            54..=55 => {
                h.update(|d, _| {
                    d.style_reset(None);
                    d.apply_look(false);
                });
                "reset all style adjustments".into()
            }
            56..=62 => self.pointer_edit(h),
            63..=79 => {
                let (x, y) = self.point(h);
                h.click(x, y);
                format!("click at {x:.0},{y:.0}")
            }
            80..=87 => {
                let (x, y) = self.point(h);
                let dy = [-120.0, -40.0, 40.0, 120.0][self.rng.below(4) as usize];
                self.wheeled = true;
                self.wheel_cursor = None;
                h.scroll(x, y, dy);
                format!("scroll {dy:+} at {x:.0},{y:.0}")
            }
            88..=91 => {
                h.control(Control::Dnd);
                "Do Not Disturb toggled".into()
            }
            92..=94 => {
                h.control(Control::Inhibit);
                "idle inhibit toggled".into()
            }
            95..=97 => {
                h.control(Control::Edit);
                "edit mode asked for".into()
            }
            _ => {
                h.control(Control::Toggle);
                "hotkey toggle".into()
            }
        }
    }

    /// The pointer at work on the widgets: drag one (by its card or from the middle of an edge)
    /// somewhere, or double click one to pick it up and put it down.
    fn pointer_edit(&mut self, h: &mut impl Rig) -> String {
        let centres: Vec<(f32, f32, f32)> = h.read(|d| {
            let g = d.geometry(d.size.0, d.size.1);
            d.tiles
                .iter()
                .map(|t| {
                    let r = t.rect();
                    (
                        g.x + (r.col as f32 + r.w as f32 / 2.0) * g.cell,
                        g.y + (r.row as f32 + r.h as f32 / 2.0) * g.cell,
                        g.cell,
                    )
                })
                .collect()
        });
        let from = match centres.get(self.rng.below(centres.len() as u64) as usize) {
            Some(&(x, y, _)) if self.rng.below(5) != 0 => (x, y),
            _ => self.point(h),
        };
        if self.rng.below(4) == 0 {
            h.double_click(from.0, from.1);
            // Carried: put it down somewhere (or cancel).
            if h.read(|d| d.edit.dragging()) {
                let to = self.point(h);
                h.move_to(to.0, to.1);
                if self.rng.below(3) == 0 {
                    h.click_with(to.0, to.1, gpui::MouseButton::Right);
                } else {
                    h.click(to.0, to.1);
                }
            }
            return format!("double click at {:.0},{:.0}", from.0, from.1);
        }
        let to = self.point(h);
        let steps = 1 + self.rng.below(8) as u32;
        h.drag(from, to, steps);
        format!(
            "drag {:.0},{:.0} to {:.0},{:.0} in {steps}",
            from.0, from.1, to.0, to.1
        )
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
    fn point(&mut self, h: &impl Rig) -> (f32, f32) {
        let (w, hgt) = h.size();
        let along = |rng: &mut Rng, extent: f32| (rng.below(1100) as f32 / 1000.0 - 0.05) * extent;
        (
            along(&mut self.rng, w as f32),
            along(&mut self.rng, hgt as f32),
        )
    }

    /// With the hints up: mostly the next letter of some label, now and then a wrong one or a way
    /// out.
    fn hint_key(&mut self, h: &impl Rig) -> String {
        let typed = h.read(|d| d.nav.typed.clone());
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
        let labels: Vec<String> = h.read(|d| {
            d.tiles
                .iter()
                .map(|t| t.hint.clone())
                .filter(|l| l.starts_with(&typed) && l.len() > typed.len())
                .collect()
        });
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

/// The kind of widget entered, or where the dashboard is otherwise.
fn kind_name(d: &Dash) -> String {
    match d.place() {
        Place::Widget => d
            .tile(d.nav.active_id)
            .map_or(format!("widget {}", d.nav.active_id), |t| {
                t.kind().name().to_string()
            }),
        Place::Edit => "edit mode".into(),
        Place::Hints => "hints".into(),
        Place::Off | Place::Closed => "dashboard".into(),
    }
}

/// Where the dashboard is, for the log.
fn describe(d: &Dash) -> String {
    let mode = match d.op() {
        NavOp::None => "",
        NavOp::Move => " (move mode)",
        NavOp::Resize => " (resize mode)",
    };
    match d.place() {
        Place::Closed => "closed".into(),
        Place::Edit => format!("editing{mode}"),
        Place::Off => "nothing entered".into(),
        Place::Hints => format!("hints, typed {:?}", d.nav.typed),
        Place::Widget => {
            let name = kind_name(d);
            match crate::widgets::describe(d) {
                Some(what) => format!("in {name}{mode}: {what}"),
                None => format!("in {name}{mode}"),
            }
        }
    }
}

/// What has to hold whatever keys came before.
fn check(d: &Dash) -> Result<(), String> {
    let hints: Vec<&str> = d.tiles.iter().map(|t| t.hint.as_str()).collect();
    let mut unique = HashSet::new();
    for h in &hints {
        if h.is_empty() && d.solo.is_none() {
            return Err("a widget has no hint label".into());
        }
        if !unique.insert(*h) {
            return Err(format!("two widgets have the hint label {h:?}"));
        }
        if let Some(o) = hints.iter().find(|o| *o != h && o.starts_with(h)) {
            return Err(format!("hint label {h:?} is the start of {o:?}"));
        }
    }
    let typed = &d.nav.typed;
    match d.nav.mode {
        crate::dash::NavMode::Widget if d.tile(d.nav.active_id).is_none() => {
            return Err(format!(
                "entered widget {} isn't on the dashboard",
                d.nav.active_id
            ));
        }
        crate::dash::NavMode::Hints
            if !typed.is_empty() && !hints.iter().any(|h| h.starts_with(typed.as_str())) =>
        {
            return Err(format!("typed {typed:?} starts no hint label"));
        }
        crate::dash::NavMode::Off if !typed.is_empty() => {
            return Err(format!("typed {typed:?} left over with the hints down"));
        }
        _ => {}
    }
    crate::widgets::check(d)?;
    crate::edit::probe::check(d)?;
    if !d.visible {
        if d.editing {
            return Err("closed while still in edit mode".into());
        }
        if d.nav.mode != crate::dash::NavMode::Off && d.solo.is_none() {
            return Err("closed with a widget still entered or the hints up".into());
        }
        if d.nav.expanded_id != 0 {
            return Err("closed with a widget expanded".into());
        }
    }
    Ok(())
}

/// The keys, one after the other, on `h`.
pub fn drive(h: &mut Harness, mut monkey: Monkey, ops: u64) -> Result<Monkey, Failure> {
    while monkey.done() < ops {
        // Time passes by the key, not by the clock: the same keys find the same timers run out.
        if monkey.done().is_multiple_of(KEYS_PER_TICK) {
            h.advance(1);
        }
        // What a hide or show asked for is done in place, as on screen where the window stays.
        for c in h.controls() {
            h.control(c);
        }
        monkey.step(h)?;
        for c in h.controls() {
            h.control(c);
        }
    }
    Ok(monkey)
}

/// `niri-dash stress`.
pub fn run(args: &Args) -> crate::window::Result<()> {
    let sandbox = Sandbox::enter(args.config.as_deref())
        .map_err(|e| crate::window::Error::Ui(e.to_string()))?;
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
        let config = crate::layout::Layout::load().text;
        Harness::with_mock(&config)
            .stress(monkey, args.ops.unwrap_or(2000))
            .map_err(|f| crate::window::Error::Stress(f.to_string()))?
    } else {
        live::run(monkey, args)?
    };
    println!("{}", monkey.summary());
    monkey
        .finish()
        .map(|_| ())
        .map_err(|f| crate::window::Error::Stress(f.to_string()))
}

/// The disposable home of a run's files, removed when dropped.
pub struct Sandbox(PathBuf);

impl Sandbox {
    /// Point the dashboard's config, state, data and cache directories at a fresh directory, with
    /// a copy of `config` (or nothing: the default layout) as config.kdl. Call before anything
    /// reads them, and before any other thread runs.
    pub fn enter(config: Option<&std::path::Path>) -> std::io::Result<Self> {
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
            let text = std::fs::read_to_string(src)?;
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

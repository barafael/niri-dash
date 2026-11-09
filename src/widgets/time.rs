//! Time: clock, calendar, weather, timer, stopwatch, pomodoro, sleep timer.
//!
//! The group's hooks, called by `widgets/mod.rs` for every group in turn; see PORTING.md. Each
//! widget is a submodule (`widgets/time/<kind>.rs`) with its state, its drawing, its keys and its
//! invariants; this file routes to them and keeps what they share: the group's state, the clock
//! they count on, and the waiting (`tasks`).

use super::Ctx;
use crate::Control;
use crate::dash::{Dash, NavMode};
use crate::keys::Key;
use crate::layout::{Kind, Placed};
use gpui::{AnyElement, Context};
use niri_dash_system::Event;
use std::time::Duration;
use std::time::SystemTime;

mod calendar;
mod clock;
mod countdown;
mod draw;
mod parts;
mod pomo;
mod sleep;
mod tasks;
mod weather;

/// Everything the group's widgets keep that isn't the layout or the look. Several instances of a
/// kind share it (two clocks show the same time, two timers the same countdown).
#[derive(Default)]
pub struct State {
    pub clock: clock::State,
    pub calendar: calendar::State,
    pub weather: weather::State,
    /// The countdown and the stopwatch.
    pub timers: countdown::State,
    pub pomo: pomo::State,
    pub sleep: sleep::State,
    tasks: tasks::Tasks,
    /// Time that was skipped, for the tests: what the timers count on is the wall clock plus this.
    skew: Duration,
}

impl State {
    /// What the timers count on.
    pub fn now(&self) -> SystemTime {
        SystemTime::now() + self.skew
    }

    /// Let `by` pass for the timers without waiting for it (the tests).
    pub fn pass(&mut self, by: Duration) {
        self.skew += by;
    }
}

/// The widgets of `kind` on show: the standalone app's own, or the visible ones on the dashboard.
fn placed_of(d: &Dash, kind: Kind) -> Vec<Placed> {
    match d.solo {
        Some(k) if k == kind => vec![d.layout.solo_widget(kind)],
        Some(_) => Vec::new(),
        None => d
            .layout
            .visible()
            .into_iter()
            .filter(|p| p.kind == kind)
            .cloned()
            .collect(),
    }
}

/// Every timer, stopwatch, pomodoro and sleep timer's cursor stays on a stop that exists, as the
/// state or the size of the widget changes under it.
fn sync_scopes(d: &mut Dash) {
    countdown::sync(d);
    pomo::sync(d);
    sleep::sync(d);
}

/// A widget's shape changed under its cursor.
pub fn resized(d: &mut Dash) {
    sync_scopes(d);
}

/// The kinds of widget this group draws.
pub fn handles(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Clock
            | Kind::Calendar
            | Kind::Weather
            | Kind::Timer
            | Kind::Stopwatch
            | Kind::Pomodoro
            | Kind::SleepTimer
    )
}

/// What the widget of `ctx.tile` draws, in a card of `ctx.w` by `ctx.h` pixels.
pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    if ctx.d.widgets.time.sleep.take_refocus() {
        sleep::refocus(cx);
    }
    match ctx.kind() {
        Kind::Clock => clock::render(ctx, cx),
        Kind::Calendar => calendar::render(ctx, cx),
        Kind::Weather => weather::render(ctx, cx),
        Kind::Timer => countdown::render(ctx, cx, false),
        Kind::Stopwatch => countdown::render(ctx, cx, true),
        Kind::Pomodoro => pomo::render(ctx, cx),
        Kind::SleepTimer => sleep::render(ctx, cx),
        kind => super::placeholder(ctx, kind),
    }
}

/// What the system reports. `None`: not this group's; `Some(control)`: taken, and the event asks
/// for a control (a color was picked, and the dashboard may show itself again) or not.
pub fn event(d: &mut Dash, event: &Event) -> Option<Option<Control>> {
    match event {
        Event::Weather(niri_dash_system::weather::Event::State(w)) => {
            d.widgets.time.weather.show(w.clone());
            Some(None)
        }
        Event::Sleep(e) => {
            sleep::event(d, e);
            Some(None)
        }
        _ => None,
    }
}

/// A key for the entered widget `id` of `kind` (one of this group's). True if it took it.
pub fn key(d: &mut Dash, kind: Kind, id: u32, key: &Key, cx: &mut Context<Dash>) -> bool {
    match kind {
        Kind::Calendar => calendar::key(d, id, key),
        Kind::Timer => countdown::key(d, false, id, key, cx),
        Kind::Stopwatch => countdown::key(d, true, id, key, cx),
        Kind::Pomodoro => pomo::key(d, id, key, cx),
        Kind::SleepTimer => sleep::key(d, id, key, cx),
        _ => false,
    }
}

/// Escape, before the dashboard takes it: true if something of the group's closed (the sleep
/// timer's field gives the keyboard back; a countdown is given up on before the widget is left).
pub fn escape(d: &mut Dash, cx: &mut Context<Dash>) -> bool {
    if sleep::escape(d, cx) {
        return true;
    }
    let in_sleep_timer = d.nav.mode == NavMode::Widget
        && d.tile(d.nav.active_id)
            .is_some_and(|t| t.kind() == Kind::SleepTimer);
    in_sleep_timer && sleep::give_up(d, cx)
}

/// A second passed while the dashboard is up.
pub fn tick(d: &mut Dash) {
    clock::tick(d);
    if d.solo.is_some() {
        // A standalone app isn't told when its settings change.
        pomo::sync_plan(d);
        sleep::sync_settings(d);
    }
    sync_scopes(d);
}

/// The dashboard opens: every widget back on its first page.
pub fn come_up(d: &mut Dash) {
    clock::tick(d);
    d.widgets.time.calendar.come_up();
}

/// The dashboard goes away: the sleep timer's field lets go of the keyboard.
pub fn go_down(d: &mut Dash) {
    d.widgets.time.sleep.stop_editing();
    sync_scopes(d);
}

/// The dashboard came up (`true`) or went away: what shows time redraws itself while it is
/// shown (`tasks`); what counts and goes off does so either way.
pub fn awake(_d: &mut Dash, _on: bool) {}

/// Widget `id` of `kind` was entered with the keyboard: the keys are the dashboard's, not the
/// sleep timer's field's (if the keyboard was in it, it is given back at the next frame).
pub fn entered(d: &mut Dash, _kind: Kind, _id: u32) {
    d.widgets.time.sleep.stop_editing();
}

/// The tiles changed (a widget added, removed, moved): the pomodoro and the sleep timer follow
/// the settings of the first widget of their kind, and the zones are those of the clocks there.
pub fn layout_changed(d: &mut Dash) {
    clock::tick(d);
    pomo::sync_plan(d);
    sleep::sync_settings(d);
    sync_scopes(d);
}

/// What the entered widget would say about where its cursor is, for the stress run's log.
pub fn describe(d: &Dash, kind: Kind) -> Option<String> {
    match kind {
        Kind::Calendar => calendar::describe(d),
        Kind::Timer => countdown::describe(d, false),
        Kind::Stopwatch => countdown::describe(d, true),
        Kind::Pomodoro => pomo::describe(d),
        Kind::SleepTimer => sleep::describe(d),
        _ => None,
    }
}

/// What has to hold of the group's state whatever keys came before: the stress run checks it
/// after every key. An error says what doesn't add up.
pub fn check(d: &Dash) -> Result<(), String> {
    calendar::check(d)?;
    countdown::check(d)?;
    pomo::check(d)?;
    sleep::check(d)?;
    weather::check(d)
}

/// The dashboard is built (`d.state` is what was saved): the timers that were running are back,
/// and the pomodoro and the sleep timer have their settings.
pub fn init(d: &mut Dash) {
    let now = SystemTime::now();
    d.widgets.time.timers = countdown::State::restored(&d.state, now);
    d.widgets.time.pomo = pomo::State::restored(&d.state, now);
    clock::tick(d);
    pomo::sync_plan(d);
    sleep::sync_settings(d);
}

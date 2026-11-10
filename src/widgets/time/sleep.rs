//! The sleep timer widget: the countdown of `niri_dash_system::sleep`, counted here, with the few
//! things that touch the machine (which modes it offers, running one, keeping the screen awake)
//! handed to the system. Every sleep timer widget shows the same countdown: a machine only goes
//! to sleep once. It follows the first widget's settings.

use super::parts::{self, Face, Scopes, card};
use super::{draw, placed_of, tasks};
use crate::dash::Dash;
use crate::keys::Key;
use crate::kit::{self, Glide};
use crate::layout::Kind;
use crate::scope::{Scope, Stops};
use crate::widget_settings::{Opt, get_bool, get_str};
use crate::widgets::Ctx;
use chrono::Local;
use gpui::prelude::*;
use gpui::{
    AnyElement, AsyncApp, ClickEvent, Context, Entity, Role, ScrollEvent, Subscription, WeakEntity,
    Window, div, px,
};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use niri_dash_system::{Tell, notifications, sleep};
use std::cell::Cell;
use std::path::PathBuf;
use std::time::SystemTime;
use crate::kit::AriaCursor;
use crate::kit::AriaState;

const PLACEHOLDER: &str = "45s, 30 min, 1h30m, 23:45";

/// What the widget's settings in config.kdl say.
#[derive(Clone, PartialEq)]
struct Settings {
    when: String,
    script: String,
    dry_run: bool,
    keep_awake: bool,
}

/// The text field while it has the keyboard.
struct Field {
    state: Entity<InputState>,
    _events: Subscription,
}

pub struct State {
    pub ctl: sleep::Controller,
    /// When it last counted.
    last: Cell<Option<SystemTime>>,
    /// The script behind the run-script mode.
    script: PathBuf,
    /// The settings last applied, so that a toggle made in the widget is not undone by every
    /// change to the layout.
    settings: Option<Settings>,
    /// Whether the system was last told to keep the screen awake.
    awake: bool,
    pub scopes: Scopes,
    /// The keyboard is in the when field: keys are its own, but Ctrl+D.
    pub editing: bool,
    field: Option<Field>,
    /// The field went away while it had the focus: the dashboard takes it back at the next frame.
    refocus: Cell<bool>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            ctl: sleep::Controller::default(),
            last: Cell::new(None),
            script: script_path(""),
            settings: None,
            awake: false,
            scopes: Scopes::default(),
            editing: false,
            field: None,
            refocus: Cell::new(false),
        }
    }
}

impl State {
    pub fn armed(&self) -> bool {
        self.ctl.timer.status() == sleep::Status::Armed
    }

    /// Counting starts from `now`, unless it already does.
    pub fn start_counting(&self, now: SystemTime) {
        if self.last.get().is_none() {
            self.last.set(Some(now));
        }
    }

    pub fn stop_counting(&self) {
        self.last.set(None);
    }

    fn counting(&self) -> bool {
        self.ctl.timer.counting()
    }

    /// The keyboard leaves the when field (the dashboard went away).
    pub fn stop_editing(&mut self) {
        self.editing = false;
        if self.field.take().is_some() {
            self.refocus.set(true);
        }
    }

    /// Whether the dashboard has to take the keyboard back from a field that is gone (once).
    pub fn take_refocus(&self) -> bool {
        self.refocus.replace(false)
    }
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

// ---- what can be done -----------------------------------------------------------------------

/// Do what the user asked and show how it stands.
pub fn act(d: &mut Dash, cx: &mut Context<Dash>, act: sleep::Act) {
    d.widgets.time.sleep.ctl.apply(act, &Local::now());
    settled(d, cx);
}

/// Keep the screen inhibitor in step with the countdown and the cursor in step with the modes,
/// and show it.
fn settled(d: &mut Dash, cx: &mut Context<Dash>) {
    update_awake(d);
    tasks::ensure(d, cx);
    super::sync_scopes(d);
    cx.notify();
}

fn update_awake(d: &mut Dash) {
    let awake = d.widgets.time.sleep.ctl.wants_awake();
    if std::mem::replace(&mut d.widgets.time.sleep.awake, awake) != awake {
        d.tx.sleep.tell(sleep::Command::KeepAwake(awake));
    }
}

/// The countdown is counted, a quarter of a second at a time.
pub fn tick(d: &mut Dash, cx: &mut Context<Dash>) {
    let now = d.widgets.time.now();
    let s = &d.widgets.time.sleep;
    let elapsed = s
        .last
        .replace(Some(now))
        .map_or(std::time::Duration::ZERO, |then| {
            now.duration_since(then).unwrap_or_default()
        });
    let fired = d.widgets.time.sleep.ctl.tick(elapsed);
    if let Some(mode) = fired {
        // The screen is on its own from here, whatever happens next.
        settled(d, cx);
        fire(d, mode);
    }
    settled(d, cx);
}

/// The countdown ran out: do it, or for a dry run say what would have been done.
fn fire(d: &mut Dash, mode: sleep::Mode) {
    let script = d.widgets.time.sleep.script.clone();
    if !d.widgets.time.sleep.ctl.dry_run {
        d.tx.sleep.tell(sleep::Command::Run { mode, script });
        return;
    }
    d.widgets.time.sleep.ctl.pretend(mode, &script);
    let text = format!("Sleep timer: would have run {}", sleep::describe(mode, &script));
    d.toast(&text);
    // On Plasma the notification pops up by itself.
    if d.caps.notifications_server {
        d.tx.notifications.tell(notifications::Command::Osd {
            text: text.clone(),
            icon: "weather-clear-night".into(),
        });
    }
    d.tx.notifications.tell(notifications::Command::AddLocal {
        app: "Sleep timer".into(),
        summary: text,
        body: String::new(),
    });
}

/// What the actor behind the sleep commands reports.
pub fn event(d: &mut Dash, event: &sleep::Event) {
    match event {
        sleep::Event::Modes(modes) => d.widgets.time.sleep.ctl.set_modes(modes.clone()),
        sleep::Event::Done { mode, error } => {
            d.widgets.time.sleep.ctl.performed(*mode, error.clone());
        }
    }
    super::sync_scopes(d);
}

/// Follow the first sleep timer widget's settings.
pub fn sync_settings(d: &mut Dash) {
    let Some(widget) = placed_of(d, Kind::SleepTimer).into_iter().next() else {
        return;
    };
    let us = d.locale.us;
    let v = &widget.settings;
    let now = Settings {
        when: get_str(v, Opt::SleepWhen, us),
        script: get_str(v, Opt::SleepScript, us),
        dry_run: get_bool(v, Opt::DryRun, us),
        keep_awake: get_bool(v, Opt::KeepAwake, us),
    };
    let s = &mut d.widgets.time.sleep;
    let before = s.settings.replace(now.clone());
    if before.as_ref() == Some(&now) {
        return;
    }
    let script = script_path(&now.script);
    s.script = script.clone();
    if before.as_ref().is_none_or(|b| b.when != now.when) && !s.ctl.timer.counting() {
        s.ctl.input = match now.when.trim() {
            "" => sleep::DEFAULT_WHEN.into(),
            w => w.into(),
        };
    }
    if before.as_ref().is_none_or(|b| b.dry_run != now.dry_run) {
        s.ctl.dry_run = now.dry_run;
    }
    if before.as_ref().is_none_or(|b| b.keep_awake != now.keep_awake) {
        s.ctl.keep_awake = now.keep_awake;
    }
    if before.as_ref().is_none_or(|b| b.script != now.script) {
        d.tx.sleep.tell(sleep::Command::Detect { script });
    }
    update_awake(d);
}

// ---- the when field -------------------------------------------------------------------------

/// Run `f` with the window: what makes an input needs one, and the hooks of the group don't have
/// it. It happens right after the event that asked.
fn with_window(
    cx: &mut Context<Dash>,
    f: impl FnOnce(&mut Dash, &mut Window, &mut Context<Dash>) + 'static,
) {
    cx.spawn(async move |this: WeakEntity<Dash>, cx: &mut AsyncApp| {
        let Some(handle) = cx.update(|cx| cx.windows().into_iter().next()) else {
            return;
        };
        let _ = handle.update(cx, |_, window, cx| {
            let _ = this.update(cx, |d, cx| f(d, window, cx));
        });
    })
    .detach();
}

/// The dashboard takes the keyboard back, after the field it was in went away.
pub fn refocus(cx: &mut Context<Dash>) {
    with_window(cx, |d, window, cx| window.focus(&d.focus, cx));
}

/// The keyboard goes into the when field (`i`, Enter on it, a click).
pub fn begin_edit(d: &mut Dash, cx: &mut Context<Dash>) {
    if d.widgets.time.sleep.editing {
        return;
    }
    d.widgets.time.sleep.editing = true;
    cx.notify();
    with_window(cx, |d, window, cx| {
        if !d.widgets.time.sleep.editing {
            return;
        }
        let text = d.widgets.time.sleep.ctl.input.clone();
        let state = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(PLACEHOLDER)
                .default_value(text)
        });
        let events = cx.subscribe_in(&state, window, |d, input, event: &InputEvent, _, cx| {
            match event {
                InputEvent::Change => {
                    d.widgets.time.sleep.ctl.input = input.read(cx).value().to_string();
                    cx.notify();
                }
                // Enter in the field arms, as in sleepy, and the keys are the dashboard's again.
                InputEvent::PressEnter { .. } => {
                    with_window(cx, |d, window, cx| {
                        if d.widgets.time.sleep.editing {
                            act(d, cx, sleep::Act::Arm);
                            end_edit(d, window, cx);
                        }
                    });
                }
                InputEvent::Blur => {
                    with_window(cx, end_edit);
                }
                InputEvent::Focus => {}
            }
        });
        let handle = gpui::Focusable::focus_handle(&state, cx);
        d.widgets.time.sleep.field = Some(Field {
            state,
            _events: events,
        });
        window.focus(&handle, cx);
        cx.notify();
    });
}

/// The keyboard comes back to the dashboard.
fn end_edit(d: &mut Dash, window: &mut Window, cx: &mut Context<Dash>) {
    let s = &mut d.widgets.time.sleep;
    s.editing = false;
    if s.field.take().is_some() {
        window.focus(&d.focus, cx);
    }
    cx.notify();
}

/// Escape in the field gives the keyboard back to the dashboard: true if it was there.
pub fn escape(d: &mut Dash, cx: &mut Context<Dash>) -> bool {
    if !d.widgets.time.sleep.editing {
        return false;
    }
    d.widgets.time.sleep.editing = false;
    with_window(cx, end_edit);
    cx.notify();
    true
}

// ---- the keyboard ---------------------------------------------------------------------------

/// What a widget of this size has on it, and so which stops the cursor has.
struct Layout {
    round: bool,
    counting: bool,
    modes: usize,
    /// Room for every mode as a row; otherwise one row that steps through them.
    list: bool,
    k_toggles: usize,
    k_actions: usize,
}

impl Layout {
    fn new(d: &Dash, round: bool, h: f32) -> Self {
        let ctl = &d.widgets.time.sleep.ctl;
        let modes = ctl.modes.len();
        let list = h > 400.0 && modes > 1;
        // The field, the modes (one stop for all of them when not listed), the two switches
        // (parts of one stop), the buttons (parts of one stop).
        let mode_stops = if round {
            0
        } else if list {
            modes
        } else {
            1
        };
        let k_toggles = 1 + mode_stops;
        Self {
            round,
            counting: d.widgets.time.sleep.counting(),
            modes,
            list,
            k_toggles,
            k_actions: k_toggles + 1,
        }
    }

    fn count(&self) -> usize {
        if self.round { 0 } else { self.k_actions + 1 }
    }

    fn subs(&self, cursor: usize) -> usize {
        if self.round {
            1
        } else if cursor == self.k_toggles {
            2
        } else if cursor == self.k_actions {
            4
        } else {
            1
        }
    }
}

fn layout_of(d: &Dash, id: u32) -> Option<Layout> {
    let (_, h, round) = card(d, id)?;
    Some(Layout::new(d, round, h))
}

/// Arm, or pause and resume.
fn primary(d: &mut Dash, cx: &mut Context<Dash>) {
    if d.widgets.time.sleep.counting() {
        act(d, cx, sleep::Act::PauseResume);
    } else {
        act(d, cx, sleep::Act::Arm);
    }
}

/// Give up on the countdown (or clear what a finished one left): true if there was one.
pub fn give_up(d: &mut Dash, cx: &mut Context<Dash>) -> bool {
    let s = &d.widgets.time.sleep;
    if s.counting() || s.ctl.timer.status() == sleep::Status::Fired {
        act(d, cx, sleep::Act::Abort);
        true
    } else {
        false
    }
}

struct Keys<'a, 'b> {
    d: &'a mut Dash,
    cx: &'a mut Context<'b, Dash>,
    layout: Layout,
}

impl Stops for Keys<'_, '_> {
    fn activate(&mut self, i: usize, sub: usize) {
        let l = &self.layout;
        let (d, cx) = (&mut *self.d, &mut *self.cx);
        if l.round {
            primary(d, cx);
        } else if i == 0 {
            begin_edit(d, cx);
        } else if i == l.k_toggles {
            act(
                d,
                cx,
                if sub == 0 {
                    sleep::Act::ToggleDryRun
                } else {
                    sleep::Act::ToggleKeepAwake
                },
            );
        } else if i == l.k_actions {
            match sub {
                0 => primary(d, cx),
                1 => act(d, cx, sleep::Act::Abort),
                2 => act(d, cx, sleep::Act::Shorter),
                _ => act(d, cx, sleep::Act::Longer),
            }
        } else {
            // On a mode: arm with it.
            act(d, cx, sleep::Act::Arm);
        }
    }

    fn adjust(&mut self, i: usize, delta: i32) -> bool {
        let l = &self.layout;
        let (d, cx) = (&mut *self.d, &mut *self.cx);
        if !l.round && !l.list && i == 1 && l.modes > 0 {
            let n = l.modes as i32;
            let selected = d.widgets.time.sleep.ctl.selected as i32;
            act(d, cx, sleep::Act::Select((selected + delta).rem_euclid(n) as usize));
            return true;
        }
        if l.round && l.counting {
            act(
                d,
                cx,
                if delta < 0 {
                    sleep::Act::Shorter
                } else {
                    sleep::Act::Longer
                },
            );
            return true;
        }
        false
    }

    fn letter(&mut self, key: &Key, i: usize) -> bool {
        let l = &self.layout;
        let (d, cx) = (&mut *self.d, &mut *self.cx);
        let counting = d.widgets.time.sleep.counting();
        if (key.is(' ') && counting) || key.is('p') {
            act(d, cx, sleep::Act::PauseResume);
        } else if key.is('+') || key.is('=') {
            act(d, cx, sleep::Act::Longer);
        } else if key.is('-') || key.is('_') {
            act(d, cx, sleep::Act::Shorter);
        } else if key.is('d') {
            act(d, cx, sleep::Act::ToggleDryRun);
        } else if key.is('a') {
            act(d, cx, sleep::Act::ToggleKeepAwake);
        } else if key.is('i') && !l.round && i == 0 {
            begin_edit(d, cx);
        } else {
            return false;
        }
        true
    }

    fn back(&mut self) -> bool {
        give_up(self.d, self.cx)
    }
}

/// A key for the entered sleep timer `id`. Ctrl+D flips the dry run wherever the keyboard is in
/// the widget, a text field included. Up and down move over the stops, h and l step the mode or
/// move between a stop's parts, Enter presses, Space pauses, + and - move the deadline, d dry
/// run, a keeps the screen on, i types in the field; Escape gives up on the countdown before it
/// leaves the widget.
pub fn key(d: &mut Dash, id: u32, key: &Key, cx: &mut Context<Dash>) -> bool {
    if key.ctrl && !key.alt && !key.meta && (key.is('d') || key.is('D')) {
        act(d, cx, sleep::Act::ToggleDryRun);
        return true;
    }
    if d.widgets.time.sleep.editing {
        return false;
    }
    let Some(layout) = layout_of(d, id) else {
        return false;
    };
    let mut scope = d.widgets.time.sleep.scopes.get(id);
    let count = layout.count();
    let (round, list, modes) = (layout.round, layout.list, layout.modes);
    let mut keys = Keys { d, cx, layout };
    scope.set_count(count, &keys);
    scope.set_subs(keys.layout.subs(scope.cursor));
    let taken = scope.press(key, &mut keys);
    follow_cursor(keys.d, keys.cx, &mut scope, round, list, modes);
    d.widgets.time.sleep.scopes.set(id, scope);
    super::sync_scopes(d);
    taken
}

/// The mode highlight follows the cursor down the list.
fn follow_cursor(
    d: &mut Dash,
    cx: &mut Context<Dash>,
    scope: &mut Scope,
    round: bool,
    list: bool,
    modes: usize,
) {
    if !round
        && list
        && scope.cursor >= 1
        && scope.cursor <= modes
        && d.widgets.time.sleep.ctl.selected != scope.cursor - 1
    {
        act(d, cx, sleep::Act::Select(scope.cursor - 1));
    }
}

/// Keep every sleep timer's cursor on a stop that exists, and the mode picked where it is.
pub fn sync(d: &mut Dash) {
    let ids: Vec<u32> = d
        .tiles
        .iter()
        .filter(|t| t.kind() == Kind::SleepTimer)
        .map(|t| t.id())
        .collect();
    for id in ids {
        let Some(layout) = layout_of(d, id) else {
            continue;
        };
        d.widgets.time.sleep.scopes.fit(id, layout.count(), |c| layout.subs(c));
        // The picked mode is where the cursor is, when it is on a mode: a list drawn after the
        // widget was blown up may have the cursor on another mode than the picked one, and the
        // cursor goes to the picked one.
        let picked = d.widgets.time.sleep.ctl.selected;
        let mut scope = d.widgets.time.sleep.scopes.get(id);
        if !layout.round
            && layout.list
            && scope.cursor >= 1
            && scope.cursor <= layout.modes
            && picked != scope.cursor - 1
        {
            scope.cursor = picked + 1;
            d.widgets.time.sleep.scopes.set(id, scope);
        }
    }
    let all: Vec<u32> = d.tiles.iter().map(|t| t.id()).collect();
    d.widgets.time.sleep.scopes.retain(&all);
}

pub fn describe(d: &Dash) -> Option<String> {
    let id = d.nav.active_id;
    let layout = layout_of(d, id)?;
    let scope = d.widgets.time.sleep.scopes.get(id);
    Some(if layout.round {
        "the face".to_string()
    } else {
        format!(
            "stop {} of {}, part {}{}",
            scope.cursor + 1,
            layout.count(),
            scope.part() + 1,
            if d.widgets.time.sleep.editing { ", typing" } else { "" }
        )
    })
}

pub fn check(d: &Dash) -> Result<(), String> {
    let s = &d.widgets.time.sleep;
    if s.ctl.selected >= s.ctl.modes.len().max(1) {
        return Err(format!(
            "mode {} picked of {}",
            s.ctl.selected,
            s.ctl.modes.len()
        ));
    }
    if s.armed() != (s.ctl.timer.status() == sleep::Status::Armed) {
        return Err("armed and not armed".into());
    }
    if s.ctl.wants_awake() != s.awake {
        return Err(format!(
            "the screen is {} held awake, but the countdown wants it {}",
            if s.awake { "" } else { "not" },
            if s.ctl.wants_awake() { "held" } else { "let go" }
        ));
    }
    for t in &d.tiles {
        if t.kind() != Kind::SleepTimer {
            continue;
        }
        let Some(layout) = layout_of(d, t.id()) else {
            continue;
        };
        let scope = s.scopes.get(t.id());
        if scope.cursor >= layout.count().max(1) {
            return Err(format!(
                "cursor is on stop {} of {} in the sleep timer",
                scope.cursor,
                layout.count()
            ));
        }
        if !layout.round && layout.list {
            let on_mode = scope.cursor >= 1 && scope.cursor <= layout.modes;
            if on_mode && s.ctl.selected != scope.cursor - 1 {
                return Err(format!(
                    "cursor is on mode {} but mode {} is picked",
                    scope.cursor - 1,
                    s.ctl.selected
                ));
            }
        }
    }
    Ok(())
}

// ---- drawing --------------------------------------------------------------------------------

fn mode_name(s: &State) -> String {
    s.ctl
        .modes
        .get(s.ctl.selected)
        .map_or_else(|| "none offered".to_string(), |m| m.label().to_string())
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    tasks::ensure(ctx.d, cx);
    let s = &ctx.d.widgets.time.sleep;
    let tk = ctx.tk;
    let on = ctx.entered;
    let layout = Layout::new(ctx.d, ctx.round, ctx.h);
    let scope = s.scopes.fit(ctx.tile.id(), layout.count(), |c| layout.subs(c));
    let status = s.ctl.timer.status();
    let counting = layout.counting;
    let paused = status == sleep::Status::Paused;
    let fired = status == sleep::Status::Fired;
    let left = sleep::clock_text(s.ctl.timer.remaining());
    let fraction = s.ctl.timer.fraction();
    let tint = if fired { tk.warn() } else { tk.text() };
    let name = mode_name(s);
    let kf = |stop: usize, part: usize| on && scope.cursor == stop && scope.part() == part;
    if ctx.round {
        let face = ctx.w.min(ctx.h);
        let ring = (face * 0.06).max(5.0);
        let roomy = face >= 140.0;
        let shown = if counting {
            left.clone()
        } else if fired {
            "Now".to_string()
        } else {
            s.ctl.input.clone()
        };
        let size = (face * 0.2)
            .min(face * 1.1 / (shown.chars().count().max(4) as f32))
            .clamp(12.0, 52.0);
        let caption = if counting {
            if paused { "paused".to_string() } else { name }
        } else if fired {
            "time!".to_string()
        } else {
            name
        };
        let label = if counting {
            if paused { "Resume" } else { "Pause" }
        } else {
            "Arm"
        };
        return div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("sleep face")
                    .role(Role::Button)
                    .aria_label(label)
                    .aria_cursor(on)
                    .debug_selector(move || label.to_string())
                    .relative()
                    .flex_none()
                    .size(px(face))
                    .cursor_pointer()
                    .on_click(cx.listener(|d, e: &ClickEvent, _, cx| {
                        if e.click_count() >= 2 {
                            act(d, cx, sleep::Act::Abort);
                        } else {
                            primary(d, cx);
                        }
                    }))
                    .on_scroll(cx.listener(|d, e: &ScrollEvent, _, cx| {
                        if d.widgets.time.sleep.counting() {
                            let dy = f32::from(e.delta.pixel_delta(px(20.0)).y);
                            if dy > 0.0 {
                                act(d, cx, sleep::Act::Longer);
                            } else if dy < 0.0 {
                                act(d, cx, sleep::Act::Shorter);
                            }
                        }
                    }))
                    .child(draw::ring(
                        "time left ring",
                        "Time left",
                        if counting { fraction } else { 0.0 },
                        ring,
                        if paused { tk.faint() } else { tk.accent() },
                        tk,
                    ))
                    .child(
                        div()
                            .absolute()
                            .size_full()
                            .p(px(ring * 2.0))
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap(px(2.0))
                            .when(roomy, |col| {
                                col.child(kit::icon(
                                    "moon",
                                    20.0,
                                    if counting { tk.accent() } else { tk.dim() },
                                ))
                            })
                            .child(
                                parts::elided(shown)
                                    .text_size(px(size))
                                    .font_weight(tk.display_weight())
                                    .text_color(tint),
                            )
                            .child(
                                parts::elided(caption)
                                    .text_size(px(tk.font_small()))
                                    .text_color(tk.faint()),
                            ),
                    ),
            )
            .into_any_element();
    }
    let (echo, problem) = match s.ctl.when() {
        Ok(when) => (when.echo(), String::new()),
        Err(e) => (String::new(), e.to_string()),
    };
    let headline = s.ctl.headline(&Local::now());
    let outcome = s.ctl.outcome().unwrap_or_default();
    let primary_text = match status {
        sleep::Status::Armed => "Pause",
        sleep::Status::Paused => "Resume",
        _ => "Arm",
    };
    let can_arm = s.ctl.can_arm();
    let (dry, keep) = (s.ctl.dry_run, s.ctl.keep_awake);
    let help = s
        .ctl
        .modes
        .get(s.ctl.selected)
        .map(|m| m.help().to_string())
        .unwrap_or_default();
    let selected = s.ctl.selected;
    let modes: Vec<(String, String)> = s
        .ctl
        .modes
        .iter()
        .map(|m| (m.label().to_string(), m.help().to_string()))
        .collect();
    let n_modes = modes.len();
    let next_mode = move |d: &mut Dash, cx: &mut Context<Dash>| {
        let n = d.widgets.time.sleep.ctl.modes.len().max(1);
        let sel = d.widgets.time.sleep.ctl.selected;
        act(d, cx, sleep::Act::Select((sel + 1) % n));
    };
    let field = field(ctx, cx, kf(0, 0));
    let glide = Glide::new(ctx);
    div()
        .size_full()
        .p(px(tk.pad()))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .when(ctx.h > 250.0, |col| {
            col.child(
                div()
                    .flex()
                    .flex_row()
                    .flex_none()
                    .items_center()
                    .gap(px(8.0))
                    .h(px(28.0))
                    .child(kit::icon("moon", 18.0, tk.dim()))
                    .child(
                        parts::elided("Sleep timer")
                            .flex_1()
                            .text_size(px(tk.font_title()))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(tk.text()),
                    )
                    .child(
                        div()
                            .text_size(px(tk.font_title()))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(if paused { tk.faint() } else { tk.accent() })
                            .child(if counting { left.clone() } else { String::new() }),
                    ),
            )
        })
        .child(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .h(px(58.0))
                .gap(px(2.0))
                .child(field)
                .child(
                    parts::elided(if problem.is_empty() { echo } else { problem.clone() })
                        .text_size(px(tk.font_small()))
                        .text_color(if problem.is_empty() { tk.dim() } else { tk.bad() }),
                ),
        )
        .when(!layout.list, |col| {
            let focused = kf(1, 0);
            let (hover, card) = (tk.hover(), tk.card_raised());
            col.child(
                div()
                    .id("mode row")
                    .role(Role::ListItem)
                    .aria_label("Mode")
                    .aria_state(name.clone())
                    .aria_cursor(focused)
                    .debug_selector(|| "Mode".to_string())
                    .flex()
                    .flex_row()
                    .flex_none()
                    .items_center()
                    .h(px(30.0))
                    .px(px(8.0))
                    .gap(px(8.0))
                    .rounded(px(tk.radius_small().min(15.0)))
                    .bg(if focused { hover } else { card })
                    .when(focused, |r| r.border_2().border_color(tk.accent()))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .on_click(cx.listener(move |d, _: &ClickEvent, _, cx| next_mode(d, cx)))
                    .child(kit::icon("chevron-left", 14.0, tk.dim()))
                    .child(
                        parts::elided(name.clone())
                            .flex_1()
                            .flex()
                            .justify_center()
                            .text_size(px(tk.font_body()))
                            .text_color(tk.text()),
                    )
                    .child(kit::icon("chevron-right", 14.0, tk.dim())),
            )
        })
        .when(layout.list, |col| {
            col.child(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .gap(px(2.0))
                    .children(modes.iter().enumerate().map(|(i, (label, help))| {
                        let here = i == selected;
                        let focused = kf(1 + i, 0);
                        let (hover, soft) = (tk.hover(), tk.accent_soft());
                        div()
                            .id(("mode", i))
                            .role(Role::ListItem)
                            .aria_label(label.clone())
                            .aria_state(if here { "selected" } else { "" })
                            .aria_description(help.clone())
                            .aria_cursor(focused)
                            .debug_selector({
                                let label = label.clone();
                                move || label.clone()
                            })
                            .flex_none()
                            .h(px(28.0))
                            .px(px(10.0))
                            .flex()
                            .items_center()
                            .rounded(px(tk.radius_small().min(14.0)))
                            .when(here, |r| r.bg(soft))
                            .when(focused, |r| r.bg(hover).border_2().border_color(tk.accent()))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .on_click(cx.listener(move |d, _: &ClickEvent, _, cx| {
                                act(d, cx, sleep::Act::Select(i));
                            }))
                            .text_size(px(tk.font_body()))
                            .font_weight(if here {
                                gpui::FontWeight::SEMIBOLD
                            } else {
                                gpui::FontWeight::NORMAL
                            })
                            .text_color(tk.text())
                            .child(parts::elided(format!(
                                "{}{label}",
                                if here { "› " } else { "   " }
                            )))
                    }))
                    .child(parts::muted(if n_modes > 0 { help.clone() } else { String::new() }, tk).h(px(20.0))),
            )
        })
        .child(
            div()
                .flex()
                .flex_row()
                .flex_none()
                .h(px(24.0))
                .items_center()
                .gap(px(16.0))
                // Each half is as wide as its content plus an equal share of what is left over,
                // which goes to the label: the switch ends up at the half's right edge.
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_auto()
                        .min_w_0()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            div()
                                .flex_auto()
                                .text_size(px(tk.font_small()))
                                .text_color(if dry { tk.text() } else { tk.dim() })
                                .child("Dry run"),
                        )
                        .child(parts::switch(
                            "dry run",
                            dry,
                            glide.to("switch", 0, if dry { 1.0 } else { 0.0 }, 120.0),
                            "Dry run",
                            kf(layout.k_toggles, 0),
                            tk,
                            cx.listener(|d, _: &ClickEvent, _, cx| {
                                act(d, cx, sleep::Act::ToggleDryRun);
                            }),
                        )),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_auto()
                        .min_w_0()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            parts::elided("Keep screen on")
                                .flex_auto()
                                .text_size(px(tk.font_small()))
                                .text_color(if keep { tk.text() } else { tk.dim() }),
                        )
                        .child(parts::switch(
                            "keep awake",
                            keep,
                            glide.to("switch", 1, if keep { 1.0 } else { 0.0 }, 120.0),
                            "Keep screen on",
                            kf(layout.k_toggles, 1),
                            tk,
                            cx.listener(|d, _: &ClickEvent, _, cx| {
                                act(d, cx, sleep::Act::ToggleKeepAwake);
                            }),
                        )),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .flex_none()
                .h(px(30.0))
                .items_center()
                .gap(px(6.0))
                .child(parts::text_button(
                    "primary",
                    primary_text,
                    None,
                    true,
                    Face::default()
                        .enabled(counting || can_arm)
                        .kfocus(kf(layout.k_actions, 0)),
                    tk,
                    cx.listener(|d, _: &ClickEvent, _, cx| primary(d, cx)),
                ))
                .child(parts::text_button(
                    "abort",
                    "Abort",
                    None,
                    false,
                    Face::default()
                        .enabled(counting || fired)
                        .kfocus(kf(layout.k_actions, 1)),
                    tk,
                    cx.listener(|d, _: &ClickEvent, _, cx| act(d, cx, sleep::Act::Abort)),
                ))
                .child(div().flex_1())
                .child(parts::icon_button(
                    "shorter",
                    "minus",
                    "Take a minute off",
                    30.0,
                    Face::default()
                        .fill(tk.card_raised())
                        .enabled(counting)
                        .kfocus(kf(layout.k_actions, 2)),
                    tk,
                    cx.listener(|d, _: &ClickEvent, _, cx| act(d, cx, sleep::Act::Shorter)),
                ))
                .child(parts::icon_button(
                    "longer",
                    "plus",
                    "Add a minute",
                    30.0,
                    Face::default()
                        .fill(tk.card_raised())
                        .enabled(counting)
                        .kfocus(kf(layout.k_actions, 3)),
                    tk,
                    cx.listener(|d, _: &ClickEvent, _, cx| act(d, cx, sleep::Act::Longer)),
                )),
        )
        .child(div().flex_1())
        .child(
            div()
                .flex()
                .flex_col()
                .flex_none()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(tk.font_body()))
                        .font_weight(if counting {
                            gpui::FontWeight::SEMIBOLD
                        } else {
                            gpui::FontWeight::NORMAL
                        })
                        .text_color(tint)
                        .child(headline),
                )
                .when(counting, |col| {
                    col.child(parts::bar(
                        "time left bar",
                        "Time left",
                        fraction,
                        Glide::new(ctx).to("bar", 0, fraction.clamp(0.0, 1.0), 300.0),
                        if paused { tk.faint() } else { tk.accent() },
                        tk,
                    ))
                })
                .when(!outcome.is_empty(), |col| {
                    col.child(
                        div()
                            .text_size(px(tk.font_small()))
                            .text_color(tk.dim())
                            .child(outcome),
                    )
                }),
        )
        .into_any_element()
}

/// The when field: under the keyboard cursor it is underlined, and Enter on it types in it.
fn field(ctx: &Ctx, cx: &mut Context<Dash>, kfocus: bool) -> impl IntoElement {
    let s = &ctx.d.widgets.time.sleep;
    let tk = ctx.tk;
    let editing = s.field.is_some();
    let text = s.ctl.input.clone();
    div()
        .flex()
        .flex_col()
        .flex_none()
        .h(px(36.0))
        .gap(px(2.0))
        .child(
            div()
                .id("when field")
                .debug_selector(|| "When to sleep".to_string())
                .h(px(32.0))
                .px(px(tk.pad_tight()))
                .flex()
                .items_center()
                .rounded(px(tk.radius_small()))
                .bg(tk.subtle())
                .border_1()
                .border_color(if editing { tk.accent() } else { tk.border() })
                .text_size(px(tk.font_body()))
                .text_color(tk.text())
                .overflow_hidden()
                .when(!editing, |f| {
                    f.cursor_text()
                        .on_click(cx.listener(|d, _: &ClickEvent, _, cx| begin_edit(d, cx)))
                        .child(if text.is_empty() {
                            div().text_color(tk.faint()).child(PLACEHOLDER)
                        } else {
                            div().child(text.clone())
                        })
                })
                .when_some(s.field.as_ref(), |f, field| {
                    f.child(Input::new(&field.state).appearance(false).bordered(false).focus_bordered(false))
                }),
        )
        .child(
            div()
                .id("when underline")
                .role(Role::TextInput)
                .aria_label("When to sleep")
                .aria_value(text)
                .aria_placeholder(PLACEHOLDER)
                .aria_cursor(kfocus)
                .h(px(2.0))
                .rounded(px(1.0))
                .bg(if kfocus { tk.accent() } else { gpui::transparent_black() }),
        )
}

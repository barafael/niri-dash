//! The timer and the stopwatch widgets. One countdown and one stopwatch are shared by every
//! widget of the kind; the logic is `crate::timer`.

use super::parts::{self, Face, Scopes, card};
use super::{draw, tasks};
use crate::dash::Dash;
use crate::keys::Key;
use crate::scope::Stops;
use crate::timer::{Countdown, Run, Stopwatch, format, format_left};
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, ClickEvent, Context, Role, ScrollEvent, div, px};
use niri_dash_system::{Tell, host, notifications};
use std::time::Duration;
use web_time::{SystemTime, UNIX_EPOCH};
use crate::kit::AriaCursor;
use crate::kit::AriaState;

/// How long "Done" stays on the timer after it went off.
const DONE_FOR: Duration = Duration::from_secs(60);

/// The countdown's presets, in minutes.
const PRESETS: [i64; 4] = [1, 5, 10, 25];

#[derive(Default)]
pub struct State {
    pub countdown: Countdown,
    pub stopwatch: Stopwatch,
    /// The timer went off and says "Done" until then.
    pub done_until: Option<SystemTime>,
    pub scopes: Scopes,
}

impl State {
    /// The countdown as it was saved: its length, and its end if it is still running.
    pub fn restored(saved: &crate::state::State, now: SystemTime) -> Self {
        let mut countdown = Countdown::default();
        if saved.timer_secs > 0 {
            countdown.length = Duration::from_secs(saved.timer_secs as u64);
        }
        if let Some(end) = saved.timer_end {
            let end = UNIX_EPOCH + Duration::from_millis(end.max(0) as u64);
            if end > now {
                countdown.state = Run::Running { end };
            }
        }
        Self {
            countdown,
            ..Self::default()
        }
    }
}

/// What the timer and stopwatch widgets show, as of `now`.
pub struct View {
    pub running: bool,
    pub paused: bool,
    pub done: bool,
    pub text: String,
    pub fraction: f32,
    pub sw_running: bool,
    pub sw_started: bool,
    pub sw_text: String,
    pub sw_fraction: f32,
    /// Newest first.
    pub laps: Vec<String>,
}

impl State {
    pub fn view(&self, now: SystemTime) -> View {
        let c = &self.countdown;
        let s = &self.stopwatch;
        let elapsed = s.elapsed(now);
        View {
            running: matches!(c.state, Run::Running { .. }),
            paused: matches!(c.state, Run::Paused { .. }),
            done: self.done_until.is_some_and(|until| now < until),
            text: format_left(c.left(now)),
            fraction: c.fraction(now),
            sw_running: s.running(),
            sw_started: !elapsed.is_zero(),
            sw_text: format(elapsed, true),
            sw_fraction: elapsed.as_secs_f32() % 60.0 / 60.0,
            laps: s.laps.iter().map(|l| format(*l, true)).collect(),
        }
    }
}

/// What a widget of this size has on it, and so which stops the keyboard cursor has.
struct Layout {
    round: bool,
    sw: bool,
    has_lap: bool,
    /// Room for presets, lap buttons and the list of laps.
    extras: bool,
    lap_list: bool,
    buttons: usize,
    presets: usize,
}

impl Layout {
    fn new(sw: bool, round: bool, w: f32, h: f32, pad: f32, v: &View) -> Self {
        let has_lap = sw && v.sw_running;
        let has_reset = if sw {
            v.sw_started && !v.sw_running
        } else {
            v.running || v.paused || v.done
        };
        let extras = h > 150.0;
        let presets = if !sw && extras {
            (((w - 2.0 * pad - 72.0) / 74.0).floor() as i64).clamp(0, 4) as usize
        } else {
            0
        };
        Self {
            round,
            sw,
            has_lap,
            extras,
            lap_list: sw && h > 260.0 && !v.laps.is_empty(),
            buttons: 1 + usize::from(has_lap) + usize::from(has_reset),
            presets,
        }
    }

    fn count(&self) -> usize {
        if self.round { 0 } else { self.buttons + self.presets }
    }

    /// What stop `i` is: 0 play / pause, 1 lap, 2 reset, 3 a preset.
    fn stop(&self, i: usize) -> usize {
        if i == 0 {
            0
        } else if i == 1 && self.has_lap {
            1
        } else if i < self.buttons {
            2
        } else {
            3
        }
    }
}

fn layout_of(d: &Dash, id: u32, sw: bool) -> Option<Layout> {
    let (w, h, round) = card(d, id)?;
    let t = &d.widgets.time;
    let v = t.timers.view(t.now());
    Some(Layout::new(sw, round, w, h, d.look.style.padding, &v))
}

// ---- what can be done -----------------------------------------------------------------------

/// Change the countdown, keep it saved, and see that the end is waited for.
pub fn countdown_act(
    d: &mut Dash,
    cx: &mut Context<Dash>,
    f: impl FnOnce(&mut Countdown, SystemTime),
) {
    let now = d.widgets.time.now();
    let t = &mut d.widgets.time.timers;
    f(&mut t.countdown, now);
    t.done_until = None;
    d.state.timer_secs = t.countdown.length.as_secs() as u32;
    d.state.timer_end = match t.countdown.state {
        Run::Running { end } => end
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|d| d.as_millis() as i64),
        _ => None,
    };
    crate::state::save(&d.state);
    settled(d, cx);
}

fn stopwatch_act(d: &mut Dash, cx: &mut Context<Dash>, f: impl FnOnce(&mut Stopwatch, SystemTime)) {
    let now = d.widgets.time.now();
    f(&mut d.widgets.time.timers.stopwatch, now);
    settled(d, cx);
}

fn settled(d: &mut Dash, cx: &mut Context<Dash>) {
    tasks::ensure(d, cx);
    super::sync_scopes(d);
    cx.notify();
}

fn toggle(d: &mut Dash, cx: &mut Context<Dash>, sw: bool) {
    if sw {
        stopwatch_act(d, cx, |s, now| s.toggle(now));
    } else {
        countdown_act(d, cx, |c, now| c.toggle(now));
    }
}

fn reset(d: &mut Dash, cx: &mut Context<Dash>, sw: bool) {
    if sw {
        stopwatch_act(d, cx, |s, _| s.reset());
    } else {
        countdown_act(d, cx, |c, _| c.reset());
    }
}

fn lap(d: &mut Dash, cx: &mut Context<Dash>) {
    stopwatch_act(d, cx, |s, now| s.lap(now));
}

fn adjust(d: &mut Dash, cx: &mut Context<Dash>, minutes: i64) {
    countdown_act(d, cx, |c, now| c.adjust(minutes, now));
}

fn preset(d: &mut Dash, cx: &mut Context<Dash>, minutes: i64) {
    countdown_act(d, cx, |c, now| {
        c.length = Duration::from_secs(minutes.max(1) as u64 * 60);
        c.state = Run::Idle;
        c.toggle(now);
    });
}

/// The countdown's end came: tell the user, shown or not.
pub fn went_off(d: &mut Dash, cx: &mut Context<Dash>) {
    let now = d.widgets.time.now();
    if !d.widgets.time.timers.countdown.finished(now) {
        // Woken early (the clock changed): wait for the rest.
        d.widgets.time.tasks.timer.forget();
        tasks::ensure(d, cx);
        return;
    }
    let length = d.widgets.time.timers.countdown.length;
    countdown_act(d, cx, |c, _| c.reset());
    d.widgets.time.timers.done_until = Some(now + DONE_FOR);
    let text = format!("{} timer is done", crate::timer::format(length, false));
    // On Plasma the notification pops up by itself.
    if d.caps.notifications_server {
        d.tx.notifications.tell(notifications::Command::Osd {
            text: text.clone(),
            icon: "alarm-symbolic".into(),
        });
    }
    d.tx.notifications.tell(notifications::Command::AddLocal {
        app: "Timer".into(),
        summary: text,
        body: String::new(),
    });
    d.tx.host.tell(host::Command::PlaySound(host::Sound::Alarm));
    cx.notify();
}

// ---- the keyboard ---------------------------------------------------------------------------

struct Keys<'a, 'b> {
    d: &'a mut Dash,
    cx: &'a mut Context<'b, Dash>,
    layout: Layout,
}

impl Stops for Keys<'_, '_> {
    fn activate(&mut self, cursor: usize, _sub: usize) {
        let sw = self.layout.sw;
        if self.layout.round {
            toggle(self.d, self.cx, sw);
            return;
        }
        match self.layout.stop(cursor) {
            0 => toggle(self.d, self.cx, sw),
            1 => lap(self.d, self.cx),
            2 => reset(self.d, self.cx, sw),
            _ => {
                let i = cursor - self.layout.buttons;
                if let Some(m) = PRESETS.get(i) {
                    preset(self.d, self.cx, *m);
                }
            }
        }
    }

    fn adjust(&mut self, _cursor: usize, delta: i32) -> bool {
        if self.layout.sw {
            return false;
        }
        adjust(self.d, self.cx, delta as i64);
        true
    }

    fn letter(&mut self, key: &Key, _cursor: usize) -> bool {
        let sw = self.layout.sw;
        if key.is(' ') {
            toggle(self.d, self.cx, sw);
            true
        } else if key.is('r') {
            reset(self.d, self.cx, sw);
            true
        } else if key.is('a') && sw && self.d.widgets.time.timers.stopwatch.running() {
            lap(self.d, self.cx);
            true
        } else {
            false
        }
    }
}

/// A key for the entered timer or stopwatch `id`. Space starts and stops, r resets, h/l take a
/// minute off or add one (timer); rectangular, j/k go over the buttons and presets and Enter
/// presses one.
pub fn key(d: &mut Dash, sw: bool, id: u32, key: &Key, cx: &mut Context<Dash>) -> bool {
    let Some(layout) = layout_of(d, id, sw) else {
        return false;
    };
    let mut scope = d.widgets.time.timers.scopes.get(id);
    let count = layout.count();
    let mut keys = Keys { d, cx, layout };
    scope.set_count(count, &keys);
    let taken = scope.press(key, &mut keys);
    d.widgets.time.timers.scopes.set(id, scope);
    super::sync_scopes(d);
    taken
}

/// Keep every timer and stopwatch's cursor on a stop that exists.
pub fn sync(d: &mut Dash) {
    let ids: Vec<(u32, bool)> = d
        .tiles
        .iter()
        .filter_map(|t| match t.kind() {
            crate::layout::Kind::Timer => Some((t.id(), false)),
            crate::layout::Kind::Stopwatch => Some((t.id(), true)),
            _ => None,
        })
        .collect();
    for (id, sw) in ids {
        let Some(layout) = layout_of(d, id, sw) else {
            continue;
        };
        d.widgets.time.timers.scopes.fit(id, layout.count(), |_| 1);
    }
    let ids: Vec<u32> = d.tiles.iter().map(|t| t.id()).collect();
    d.widgets.time.timers.scopes.retain(&ids);
}

pub fn describe(d: &Dash, sw: bool) -> Option<String> {
    let id = d.nav.active_id;
    let layout = layout_of(d, id, sw)?;
    let scope = d.widgets.time.timers.scopes.get(id);
    Some(if layout.round {
        "the face".to_string()
    } else {
        format!("stop {} of {}", scope.cursor + 1, layout.count())
    })
}

pub fn check(d: &Dash) -> Result<(), String> {
    for t in &d.tiles {
        let sw = match t.kind() {
            crate::layout::Kind::Timer => false,
            crate::layout::Kind::Stopwatch => true,
            _ => continue,
        };
        let Some(layout) = layout_of(d, t.id(), sw) else {
            continue;
        };
        let scope = d.widgets.time.timers.scopes.get(t.id());
        if scope.cursor >= layout.count().max(1) {
            return Err(format!(
                "cursor is on stop {} of {} in the {}",
                scope.cursor,
                layout.count(),
                t.kind().name()
            ));
        }
        let c = &d.widgets.time.timers.countdown;
        if c.length < Duration::from_secs(1) {
            return Err("the timer has no length".into());
        }
    }
    Ok(())
}

// ---- drawing --------------------------------------------------------------------------------

fn buttons(
    cx: &mut Context<Dash>,
    ctx: &Ctx,
    v: &View,
    sw: bool,
    size: f32,
    primary: bool,
    kfocus: Option<usize>,
) -> AnyElement {
    let tk = ctx.tk;
    let running = if sw { v.sw_running } else { v.running };
    let lap_shown = sw && v.sw_running;
    let reset_shown = if sw {
        v.sw_started && !v.sw_running
    } else {
        v.running || v.paused || v.done
    };
    let kf = |i: usize| kfocus == Some(i);
    div()
        .flex()
        .flex_row()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .when(primary, |row| {
            row.child(parts::icon_button(
                "play",
                if running { "pause" } else { "play" },
                if running { "Pause" } else { "Start" },
                size,
                Face::default()
                    .fill(tk.accent_soft())
                    .tint(tk.accent())
                    .kfocus(kf(0)),
                tk,
                cx.listener(move |d, _: &ClickEvent, _, cx| toggle(d, cx, sw)),
            ))
        })
        .when(lap_shown, |row| {
            row.child(parts::icon_button(
                "lap",
                "flag",
                "Lap",
                size,
                Face::default().fill(tk.card_raised()).kfocus(kf(1)),
                tk,
                cx.listener(|d, _: &ClickEvent, _, cx| lap(d, cx)),
            ))
        })
        .when(reset_shown, |row| {
            row.child(parts::icon_button(
                "reset",
                "reset",
                "Reset",
                size,
                Face::default().fill(tk.card_raised()).kfocus(kf(2)),
                tk,
                cx.listener(move |d, _: &ClickEvent, _, cx| reset(d, cx, sw)),
            ))
        })
        .into_any_element()
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>, sw: bool) -> AnyElement {
    tasks::ensure(ctx.d, cx);
    let t = &ctx.d.widgets.time;
    let v = t.timers.view(t.now());
    let tk = ctx.tk;
    let id = ctx.tile.id();
    let layout = Layout::new(sw, ctx.round, ctx.w, ctx.h, tk.pad(), &v);
    let scope = t.timers.scopes.fit(id, layout.count(), |_| 1);
    let on = ctx.entered;
    let shown = if sw {
        v.sw_text.clone()
    } else if v.done {
        "Done".to_string()
    } else {
        v.text.clone()
    };
    let tint = if !sw && v.done { tk.warn() } else { tk.text() };
    let running = if sw { v.sw_running } else { v.running };
    if ctx.round {
        let face = ctx.w.min(ctx.h);
        let ring = (face * 0.06).max(5.0);
        let room = face >= 140.0;
        let size = (face * 0.21)
            .min(face * 1.1 / (shown.chars().count().max(4) as f32))
            .clamp(12.0, 56.0);
        let status = if sw {
            let base = if v.sw_running {
                "running"
            } else if v.sw_started {
                "paused"
            } else {
                "stopwatch"
            };
            let n = v.laps.len();
            match n {
                0 => base.to_string(),
                1 => format!("{base} · 1 lap"),
                n => format!("{base} · {n} laps"),
            }
        } else if v.running {
            "running".to_string()
        } else if v.paused {
            "paused".to_string()
        } else if room {
            "scroll to set".to_string()
        } else {
            "timer".to_string()
        };
        let (value, ring_tint, label, ring_id) = if sw {
            (
                if v.sw_started { v.sw_fraction } else { 0.0 },
                if v.sw_running { tk.accent() } else { tk.faint() },
                "Current minute",
                "minute ring",
            )
        } else {
            (
                if v.running || v.paused { v.fraction } else { 0.0 },
                if v.paused { tk.faint() } else { tk.accent() },
                "Time left",
                "time left ring",
            )
        };
        let faint = tk.faint();
        return div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id(if sw { "stopwatch face" } else { "timer face" })
                    .role(Role::Button)
                    .aria_label(if running { "Pause" } else { "Start" })
                    .aria_cursor(on)
                    .debug_selector(move || (if running { "Pause" } else { "Start" }).to_string())
                    .relative()
                    .flex_none()
                    .size(px(face))
                    .cursor_pointer()
                    .on_click(cx.listener(move |d, e: &ClickEvent, _, cx| {
                        if e.click_count() >= 2 {
                            reset(d, cx, sw);
                        } else {
                            toggle(d, cx, sw);
                        }
                    }))
                    .on_scroll(cx.listener(move |d, e: &ScrollEvent, _, cx| {
                        if !sw {
                            let dy = f32::from(e.delta.pixel_delta(px(20.0)).y);
                            if dy != 0.0 {
                                adjust(d, cx, if dy > 0.0 { 1 } else { -1 });
                            }
                        }
                    }))
                    .child(draw::ring(ring_id, label, value, ring, ring_tint, tk))
                    .when(sw && room, |face_el| {
                        face_el.child(draw::painter(move |b, window| {
                            let (x, y) = (f32::from(b.origin.x), f32::from(b.origin.y));
                            let c = (x + face / 2.0, y + face / 2.0);
                            let r = face / 2.0 - ring * 2.2;
                            let dot = (face * 0.018).max(2.0);
                            let mut shape = draw::Shape::new();
                            for i in 0..12 {
                                let big = if i % 3 == 0 { dot * 1.6 } else { dot };
                                shape.circle(draw::on_circle(c, r, i as f32 / 12.0), big / 2.0);
                            }
                            shape.paint(faint, window);
                        }))
                    })
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
                            .child(
                                div()
                                    .text_size(px(size))
                                    .font_weight(tk.display_weight())
                                    .text_color(tint)
                                    .child(shown),
                            )
                            .child(
                                parts::elided(status)
                                    .text_size(px(tk.font_small()))
                                    .text_color(tk.faint()),
                            )
                            .when(room, |col| {
                                col.child(
                                    div().h(px(34.0)).flex().items_center().child(buttons(
                                        cx, ctx, &v, sw, 30.0, false, None,
                                    )),
                                )
                            }),
                    ),
            )
            .into_any_element();
    }
    let kfocus = (on && scope.cursor < layout.buttons).then(|| layout.stop(scope.cursor));
    let font = (ctx.h * if layout.extras { 0.24 } else { 0.5 })
        .min(ctx.w * 0.16)
        .clamp(18.0, 72.0);
    let list = layout.lap_list;
    let b = layout.buttons;
    div()
        .size_full()
        .p(px(tk.pad()))
        .flex()
        .flex_col()
        .gap(px(10.0))
        .when(!list, |col| col.justify_center())
        .child(
            div()
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .gap(px(12.0))
                .child(
                    parts::elided(shown)
                        .flex_1()
                        .text_size(px(font))
                        .font_weight(tk.display_weight())
                        .text_color(tint),
                )
                .child(buttons(cx, ctx, &v, sw, 36.0, true, kfocus)),
        )
        .when(!sw && (v.running || v.paused), |col| {
            col.child(parts::bar(
                "time left bar",
                "Time left",
                v.fraction,
                if v.paused { tk.faint() } else { tk.accent() },
                tk,
            ))
        })
        .when(!sw && layout.extras, |col| {
            let (w, pad) = (ctx.w, tk.pad());
            col.child(
                div()
                    .flex()
                    .flex_row()
                    .flex_none()
                    .items_center()
                    .gap(px(6.0))
                    .child(parts::icon_button(
                        "minus",
                        "minus",
                        "Take a minute off",
                        30.0,
                        Face::default().fill(tk.card_raised()),
                        tk,
                        cx.listener(|d, _: &ClickEvent, _, cx| adjust(d, cx, -1)),
                    ))
                    .children(PRESETS.iter().enumerate().filter_map(|(i, m)| {
                        let fits = 72.0 + (i as f32 + 1.0) * 74.0 <= w - 2.0 * pad;
                        let m = *m;
                        fits.then(|| {
                            parts::text_button(
                                ("preset", i),
                                format!("{m} min"),
                                Some(format!("Set timer to {m} min")),
                                false,
                                Face::default().kfocus(on && scope.cursor == b + i),
                                tk,
                                cx.listener(move |d, _: &ClickEvent, _, cx| preset(d, cx, m)),
                            )
                        })
                    }))
                    .child(parts::icon_button(
                        "plus",
                        "plus",
                        "Add a minute",
                        30.0,
                        Face::default().fill(tk.card_raised()),
                        tk,
                        cx.listener(|d, _: &ClickEvent, _, cx| adjust(d, cx, 1)),
                    )),
            )
        })
        .when(list, |col| {
            let n = v.laps.len();
            col.child(
                div()
                    .id("laps")
                    .role(Role::List)
                    .aria_label("Laps")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(v.laps.iter().enumerate().map(|(i, l)| {
                        div()
                            .id(("lap", i))
                            .role(Role::ListItem)
                            .aria_label(format!("Lap {}", n - i))
                            .aria_state(l.clone())
                            .flex()
                            .flex_row()
                            .h(px(24.0))
                            .items_center()
                            .text_size(px(tk.font_body()))
                            .child(
                                div()
                                    .flex_1()
                                    .text_color(tk.dim())
                                    .child(format!("Lap {}", n - i)),
                            )
                            .child(div().text_color(tk.text()).child(l.clone()))
                    })),
            )
        })
        .into_any_element()
}

//! The pomodoro widget: the parts of `crate::pomodoro`, run on the clock here, with the notice
//! and the sound when one ends. Every pomodoro widget shows the same one, which follows the
//! first one's settings.

use super::parts::{self, Face, Scopes, card};
use super::{draw, placed_of, tasks};
use crate::dash::Dash;
use crate::keys::Key;
use crate::layout::Kind;
use crate::pomodoro::{Phase, Plan, Pomodoro};
use crate::scope::Stops;
use crate::timer::{Run, format_left};
use crate::widget_settings::{Opt, get_bool, get_int};
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, ClickEvent, Context, Role, div, px};
use niri_dash_system::{Tell, host, notifications};
use std::time::Duration;
use web_time::{SystemTime, UNIX_EPOCH};

#[derive(Default)]
pub struct State {
    pub pomodoro: Pomodoro,
    pub scopes: Scopes,
}

impl State {
    /// The pomodoro as it was saved: where it stood, and its end if it was still running.
    pub fn restored(saved: &crate::state::State, now: SystemTime) -> Self {
        let saved = saved.pomodoro;
        let mut p = Pomodoro::default();
        p.phase = Phase::from_index(saved.phase);
        p.done = saved.done.min(p.plan.rounds);
        if let Some(end) = saved.end {
            let end = UNIX_EPOCH + Duration::from_millis(end.max(0) as u64);
            if end > now {
                p.run = Run::Running { end };
            }
        }
        Self {
            pomodoro: p,
            scopes: Scopes::default(),
        }
    }
}

/// What the widgets show.
pub struct View {
    pub text: String,
    pub fraction: f32,
    pub running: bool,
    pub paused: bool,
    pub label: &'static str,
    pub on_break: bool,
    pub rounds: u32,
    pub done: u32,
    pub next: String,
}

impl State {
    pub fn view(&self, now: SystemTime) -> View {
        View::of(&self.pomodoro, now)
    }
}

impl View {
    fn of(p: &Pomodoro, now: SystemTime) -> Self {
        let next = p.next_phase();
        Self {
            text: format_left(p.left(now)),
            fraction: p.fraction(now),
            running: p.running(),
            paused: matches!(p.run, Run::Paused { .. }),
            label: p.phase.label(),
            on_break: p.phase.is_break(),
            rounds: p.plan.rounds,
            done: p.done,
            next: format!(
                "Next: {}, {} min",
                next.label().to_lowercase(),
                p.plan.length(next).as_secs() / 60
            ),
        }
    }

    fn has_reset(&self) -> bool {
        self.running || self.paused
    }
}

/// Keyboard stops (rectangular): play / pause, skip, and reset where shown.
fn count(round: bool, v: &View) -> usize {
    if round {
        0
    } else {
        2 + usize::from(v.has_reset())
    }
}

// ---- what can be done -----------------------------------------------------------------------

fn act(d: &mut Dash, cx: &mut Context<Dash>, f: impl FnOnce(&mut Pomodoro, SystemTime)) {
    let now = d.widgets.time.now();
    f(&mut d.widgets.time.pomo.pomodoro, now);
    settled(d, cx);
}

/// Keep the saved state and the waiting in step, and show it.
fn settled(d: &mut Dash, cx: &mut Context<Dash>) {
    persist(d);
    tasks::ensure(d, cx);
    super::sync_scopes(d);
    cx.notify();
}

fn persist(d: &mut Dash) {
    let p = &d.widgets.time.pomo.pomodoro;
    d.state.pomodoro = crate::state::Pomodoro {
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
    crate::state::save(&d.state);
}

fn toggle(d: &mut Dash, cx: &mut Context<Dash>) {
    act(d, cx, |p, now| p.toggle(now));
}

fn skip(d: &mut Dash, cx: &mut Context<Dash>) {
    act(d, cx, |p, now| p.skip(now));
}

fn reset(d: &mut Dash, cx: &mut Context<Dash>) {
    act(d, cx, |p, _| p.reset());
}

/// The part ran out: move on, and say so, shown or not.
pub fn went_off(d: &mut Dash, cx: &mut Context<Dash>) {
    let now = d.widgets.time.now();
    if !d.widgets.time.pomo.pomodoro.finished(now) {
        // Woken early (the clock changed): wait for the rest.
        d.widgets.time.tasks.pomo.forget();
        tasks::ensure(d, cx);
        return;
    }
    let (was, now_in, plan) = {
        let p = &mut d.widgets.time.pomo.pomodoro;
        let was = p.advance(now);
        (was, p.phase, p.plan)
    };
    settled(d, cx);
    let minutes = plan.length(now_in).as_secs() / 60;
    let text = match (was, now_in) {
        (Phase::Focus, Phase::LongBreak) => {
            format!("Focus session done. Time for a long break ({minutes} min)")
        }
        (Phase::Focus, _) => format!("Focus session done. Take {minutes} minutes"),
        (_, _) => format!("Break is over. Focus for {minutes} minutes"),
    };
    // On Plasma the notification pops up by itself.
    if d.caps.notifications_server {
        d.tx.notifications.tell(notifications::Command::Osd {
            text: text.clone(),
            icon: "appointment-soon".into(),
        });
    }
    d.tx.notifications.tell(notifications::Command::AddLocal {
        app: "Pomodoro".into(),
        summary: text,
        body: String::new(),
    });
    d.tx.host.tell(host::Command::PlaySound(host::Sound::Alarm));
}

/// Follow the first pomodoro widget's settings.
pub fn sync_plan(d: &mut Dash) {
    let Some(widget) = placed_of(d, Kind::Pomodoro).into_iter().next() else {
        return;
    };
    let us = d.locale.us;
    let v = &widget.settings;
    let minutes = |opt| get_int(v, opt, us).max(1) as u32;
    let plan = Plan {
        focus_min: minutes(Opt::FocusMinutes),
        short_min: minutes(Opt::ShortBreak),
        long_min: minutes(Opt::LongBreak),
        rounds: minutes(Opt::Rounds),
        auto_start: get_bool(v, Opt::AutoStart, us),
    };
    if d.widgets.time.pomo.pomodoro.plan == plan {
        return;
    }
    d.widgets.time.pomo.pomodoro.set_plan(plan);
    persist(d);
}

// ---- the keyboard ---------------------------------------------------------------------------

struct Keys<'a, 'b> {
    d: &'a mut Dash,
    cx: &'a mut Context<'b, Dash>,
    round: bool,
}

impl Stops for Keys<'_, '_> {
    fn activate(&mut self, cursor: usize, _sub: usize) {
        if self.round || cursor == 0 {
            toggle(self.d, self.cx);
        } else if cursor == 1 {
            skip(self.d, self.cx);
        } else {
            reset(self.d, self.cx);
        }
    }

    fn letter(&mut self, key: &Key, _cursor: usize) -> bool {
        if key.is(' ') {
            toggle(self.d, self.cx);
        } else if key.is('n') {
            skip(self.d, self.cx);
        } else if key.is('r') {
            reset(self.d, self.cx);
        } else {
            return false;
        }
        true
    }
}

fn count_of(d: &Dash, id: u32) -> Option<usize> {
    let (_, _, round) = card(d, id)?;
    let t = &d.widgets.time;
    Some(count(round, &View::of(&t.pomo.pomodoro, t.now())))
}

/// A key for the entered pomodoro `id`. Space starts and pauses, n skips to the next part, r
/// starts this one over. Rectangular: j/k over the buttons, Enter presses one.
pub fn key(d: &mut Dash, id: u32, key: &Key, cx: &mut Context<Dash>) -> bool {
    let Some((_, _, round)) = card(d, id) else {
        return false;
    };
    let n = count_of(d, id).unwrap_or(0);
    let mut scope = d.widgets.time.pomo.scopes.get(id);
    let mut keys = Keys { d, cx, round };
    scope.set_count(n, &keys);
    let taken = scope.press(key, &mut keys);
    d.widgets.time.pomo.scopes.set(id, scope);
    super::sync_scopes(d);
    taken
}

pub fn sync(d: &mut Dash) {
    let ids: Vec<u32> = d
        .tiles
        .iter()
        .filter(|t| t.kind() == Kind::Pomodoro)
        .map(|t| t.id())
        .collect();
    for &id in &ids {
        let Some(n) = count_of(d, id) else { continue };
        d.widgets.time.pomo.scopes.fit(id, n, |_| 1);
    }
    let all: Vec<u32> = d.tiles.iter().map(|t| t.id()).collect();
    d.widgets.time.pomo.scopes.retain(&all);
}

pub fn describe(d: &Dash) -> Option<String> {
    let id = d.nav.active_id;
    let n = count_of(d, id)?;
    let scope = d.widgets.time.pomo.scopes.get(id);
    Some(if n == 0 {
        "the face".to_string()
    } else {
        format!("stop {} of {n}", scope.cursor + 1)
    })
}

pub fn check(d: &Dash) -> Result<(), String> {
    let p = &d.widgets.time.pomo.pomodoro;
    if p.done > p.plan.rounds {
        return Err(format!(
            "{} focus sessions done in a round of {}",
            p.done, p.plan.rounds
        ));
    }
    if p.plan.rounds == 0 {
        return Err("a round of no sessions".into());
    }
    for t in &d.tiles {
        if t.kind() != Kind::Pomodoro {
            continue;
        }
        let n = count_of(d, t.id()).unwrap_or(0);
        let scope = d.widgets.time.pomo.scopes.get(t.id());
        if scope.cursor >= n.max(1) {
            return Err(format!("cursor is on stop {} of {n} in the pomodoro", scope.cursor));
        }
    }
    Ok(())
}

// ---- drawing --------------------------------------------------------------------------------

fn buttons(
    cx: &mut Context<Dash>,
    ctx: &Ctx,
    v: &View,
    size: f32,
    primary: bool,
    kfocus: Option<usize>,
) -> AnyElement {
    let tk = ctx.tk;
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
                if v.running { "pause" } else { "play" },
                if v.running { "Pause" } else { "Start" },
                size,
                Face::default()
                    .fill(tk.accent_soft())
                    .tint(tk.accent())
                    .kfocus(kf(0)),
                tk,
                cx.listener(|d, _: &ClickEvent, _, cx| toggle(d, cx)),
            ))
        })
        .child(parts::icon_button(
            "skip",
            "next",
            "Skip to the next part",
            size,
            Face::default().fill(tk.card_raised()).kfocus(kf(1)),
            tk,
            cx.listener(|d, _: &ClickEvent, _, cx| skip(d, cx)),
        ))
        .when(v.has_reset(), |row| {
            row.child(parts::icon_button(
                "reset",
                "reset",
                "Start this part over",
                size,
                Face::default().fill(tk.card_raised()).kfocus(kf(2)),
                tk,
                cx.listener(|d, _: &ClickEvent, _, cx| reset(d, cx)),
            ))
        })
        .into_any_element()
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    tasks::ensure(ctx.d, cx);
    let t = &ctx.d.widgets.time;
    let v = View::of(&t.pomo.pomodoro, t.now());
    let tk = ctx.tk;
    let tint = if v.on_break { tk.good() } else { tk.accent() };
    let scope = t.pomo.scopes.fit(ctx.tile.id(), count(ctx.round, &v), |_| 1);
    let on = ctx.entered;
    let value = if v.running || v.paused { v.fraction } else { 1.0 };
    let ring_tint = if v.paused { tk.faint() } else { tint };
    if ctx.round {
        let face = ctx.w.min(ctx.h);
        let ring = (face * 0.06).max(5.0);
        let room = face >= 150.0;
        let label = if v.running { "Pause" } else { "Start" };
        return div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("pomodoro face")
                    .role(Role::Button)
                    .aria_label(label)
                    .aria_selected(on)
                    .debug_selector(move || label.to_string())
                    .relative()
                    .flex_none()
                    .size(px(face))
                    .cursor_pointer()
                    .on_click(cx.listener(|d, e: &ClickEvent, _, cx| {
                        if e.click_count() >= 2 {
                            reset(d, cx);
                        } else {
                            toggle(d, cx);
                        }
                    }))
                    .child(draw::ring(
                        "part ring",
                        "Time left in this part",
                        value,
                        ring,
                        ring_tint,
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
                            .child(
                                div()
                                    .text_size(px((face * 0.21).clamp(12.0, 56.0)))
                                    .font_weight(tk.display_weight())
                                    .text_color(tk.text())
                                    .child(v.text.clone()),
                            )
                            .child(
                                parts::elided(v.label)
                                    .text_size(px(tk.font_small()))
                                    .text_color(tint),
                            )
                            .when(face >= 120.0, |col| {
                                col.child(
                                    div()
                                        .h(px(14.0))
                                        .flex()
                                        .items_center()
                                        .child(parts::dots(
                                            v.rounds,
                                            v.done,
                                            v.on_break,
                                            (face * 0.04).clamp(6.0, 12.0),
                                            tint,
                                            tk,
                                        )),
                                )
                            })
                            .when(room, |col| {
                                col.child(
                                    div()
                                        .h(px(34.0))
                                        .flex()
                                        .items_center()
                                        .child(buttons(cx, ctx, &v, 30.0, false, None)),
                                )
                            }),
                    ),
            )
            .into_any_element();
    }
    let extras = ctx.h > 110.0;
    let n = count(false, &v);
    let kfocus = (on && scope.cursor < n).then_some(scope.cursor);
    let status = format!(
        "{} · {}/{}{}",
        v.label,
        v.done,
        v.rounds,
        if v.paused { " · paused" } else { "" }
    );
    let font = (ctx.h * if extras { 0.3 } else { 0.42 })
        .min(ctx.w * 0.15)
        .clamp(18.0, 64.0);
    div()
        .size_full()
        .p(px(tk.pad()))
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .gap(px(12.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .child(
                            parts::elided(v.text.clone())
                                .text_size(px(font))
                                .font_weight(tk.display_weight())
                                .text_color(tk.text()),
                        )
                        .child(
                            parts::elided(status)
                                .text_size(px(tk.font_small()))
                                .text_color(tint),
                        ),
                )
                .child(buttons(cx, ctx, &v, 36.0, true, kfocus)),
        )
        .when(extras, |col| {
            col.child(parts::bar("part bar", "Time left in this part", value, ring_tint, tk))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_none()
                        .h(px(16.0))
                        .items_center()
                        .gap(px(12.0))
                        .child(parts::dots(v.rounds, v.done, v.on_break, 9.0, tint, tk))
                        .child(
                            parts::muted(v.next.clone(), tk)
                                .flex_1()
                                .flex()
                                .justify_end(),
                        ),
                )
        })
        .into_any_element()
}

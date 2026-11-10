//! The notifications widget and do not disturb. Port of `NotificationsWidget` in
//! `slint/ui/widgets/desk.slint` and `slint/src/app/notifications.rs`.

use super::list::Lists;
use super::parts::{self, Look, RoundIcon};
use super::pics::Pic;
use super::shared::{self, clock_text, when_text};
use crate::dash::Dash;
use crate::keys::Key;
use crate::kit::AriaCursor;
use crate::kit::AriaState;
use crate::scope::{Scope, Stops};
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, ClickEvent, Context, div, px};
use niri_dash_system::notifications::{Command, Dnd, State as Sys, next_hour};
use niri_dash_system::{Senders, Tell};
use std::collections::HashMap;

/// "Until tomorrow" ends at this hour.
const MORNING: u32 = 7;

/// A notification as the list shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Notif {
    pub id: u32,
    pub app: String,
    pub summary: String,
    pub body: String,
    pub time: String,
    pub icon: Option<Pic>,
    pub has_default: bool,
    pub urgent: bool,
}

/// What one instance of the widget keeps: its cursor, and whether the pause choices show.
#[derive(Default, Clone, Copy)]
pub struct Inst {
    pub scope: Scope,
    pub choosing: bool,
}

pub struct State {
    pub items: Vec<Notif>,
    /// We are the notification server.
    pub server: bool,
    /// Another server shows the notifications, and do not disturb pauses its pop-ups (Plasma).
    pub can_pause: bool,
    pub dnd: Dnd,
    /// Do not disturb is on at the moment.
    pub dnd_active: bool,
    /// What it says ("Pop-ups paused until 7:00").
    pub dnd_text: String,
    /// How many came in meanwhile.
    pub missed: u32,
    /// The time "until tomorrow" ends, e.g. "7:00".
    pub morning: String,
    pub insts: HashMap<u32, Inst>,
    pub lists: Lists,
}

impl Default for State {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            server: false,
            can_pause: false,
            dnd: Dnd::Off,
            dnd_active: false,
            dnd_text: String::new(),
            missed: 0,
            morning: "7:00".into(),
            insts: HashMap::new(),
            lists: Lists::default(),
        }
    }
}

impl State {
    pub fn inst(&self, id: u32) -> Inst {
        self.insts.get(&id).copied().unwrap_or_default()
    }

    /// The choices that come first among the stops while they show, with `width` px to show them
    /// in: 1 hour, until morning, always (where there is room, or while not paused), off (while
    /// paused).
    fn choices(&self, choosing: bool, width: f32) -> usize {
        if !choosing {
            return 0;
        }
        2 + usize::from(self.third(width)) + usize::from(self.dnd_active)
    }

    fn third(&self, width: f32) -> bool {
        width > 330.0 || !self.dnd_active
    }

    /// What choice `i` of the stops sets: 1 an hour, 2 until morning, 3 until turned off, 0 off.
    fn choice(&self, i: usize, width: f32) -> u8 {
        if i < 2 {
            i as u8 + 1
        } else if i == 2 && self.third(width) {
            3
        } else {
            0
        }
    }

    /// Can the bell pause anything?
    fn pausable(&self) -> bool {
        self.server || self.can_pause
    }
}

/// The notifications as the system reports them.
pub fn show(d: &mut Dash, s: Sys) {
    let us = d.locale.us;
    let desk = &mut d.widgets.desk;
    let n = &mut desk.notifications;
    n.server = s.server;
    n.can_pause = s.can_pause;
    n.items = s
        .items
        .iter()
        .map(|x| Notif {
            id: x.id,
            app: x.app.clone(),
            summary: x.summary.clone(),
            body: x.body.clone(),
            time: when_text(x.time, us),
            icon: desk.pics.get(&x.icon),
            has_default: x.has_default(),
            urgent: x.urgent,
        })
        .collect();
    sync_dnd(d, s.dnd, s.missed);
    sync(d);
}

/// The notification server's do-not-disturb state: shown, and remembered for restarts.
fn sync_dnd(d: &mut Dash, dnd: Dnd, missed: u32) {
    let now = chrono::Local::now();
    let us = d.locale.us;
    let n = &mut d.widgets.desk.notifications;
    n.dnd = dnd;
    n.dnd_active = dnd.active(now);
    n.missed = missed;
    n.dnd_text = match dnd {
        Dnd::Off => String::new(),
        Dnd::Forever => "Pop-ups paused".into(),
        Dnd::Until(t) if t.date_naive() == now.date_naive() => {
            format!("Pop-ups paused until {}", clock_text(t, us))
        }
        Dnd::Until(t) => format!("Pop-ups paused until tomorrow {}", clock_text(t, us)),
    };
    n.morning = clock_text(next_hour(now, MORNING), us);
    let saved = dnd.to_saved();
    if d.state.dnd_until != saved {
        d.state.dnd_until = saved;
        crate::state::save(&d.state);
    }
}

/// Do not disturb off or on (`niri-dash dnd`, the stress run): on when it is off, and an
/// on-screen message says so.
pub fn toggle_dnd(d: &mut Dash) {
    let on = Dnd::from_saved(d.state.dnd_until).active(chrono::Local::now());
    d.tx.notifications
        .tell(Command::SetDnd(if on { Dnd::Off } else { Dnd::Forever }));
    osd(
        d,
        if on {
            "Notifications back on"
        } else {
            "Do not disturb"
        },
        if on {
            "preferences-system-notifications"
        } else {
            "notifications-disabled"
        },
    );
}

fn osd(d: &Dash, text: &str, icon: &str) {
    d.tx.notifications.tell(Command::Osd {
        text: text.into(),
        icon: icon.into(),
    });
}

/// Do not disturb as the widget's choices say: 1 for an hour, 2 until tomorrow morning, 3 until
/// turned off, anything else off.
fn set_dnd(tx: &Senders, choice: u8) {
    let now = chrono::Local::now();
    let dnd = match choice {
        1 => Dnd::Until(now + chrono::Duration::hours(1)),
        2 => Dnd::Until(next_hour(now, MORNING)),
        3 => Dnd::Forever,
        _ => Dnd::Off,
    };
    tx.notifications.tell(Command::SetDnd(dnd));
}

/// The dashboard is built: do not disturb survives a restart.
pub fn init(d: &mut Dash) {
    let us = d.locale.us;
    d.widgets.desk.notifications.morning = clock_text(next_hour(chrono::Local::now(), MORNING), us);
    let saved = Dnd::from_saved(d.state.dnd_until);
    if saved != Dnd::Off {
        d.tx.notifications.tell(Command::SetDnd(saved));
    }
}

/// A second passed: a timed pause may have run out.
pub fn tick(d: &mut Dash) {
    let n = &mut d.widgets.desk.notifications;
    let active = n.dnd.active(chrono::Local::now());
    if active != n.dnd_active {
        n.dnd_active = active;
        sync(d);
    }
}

struct Keys<'a> {
    items: &'a [Notif],
    choices: usize,
    width: f32,
    state: &'a State,
    choosing: &'a mut bool,
    tx: &'a Senders,
    hide: bool,
}

impl Keys<'_> {
    fn item(&self, i: usize) -> Option<&Notif> {
        i.checked_sub(self.choices).and_then(|k| self.items.get(k))
    }
}

impl Stops for Keys<'_> {
    fn activate(&mut self, i: usize, sub: usize) {
        if i < self.choices {
            set_dnd(self.tx, self.state.choice(i, self.width));
            *self.choosing = false;
            return;
        }
        let Some(n) = self.item(i) else { return };
        if sub == 1 {
            self.tx.notifications.tell(Command::Dismiss(n.id));
        } else if n.has_default {
            self.tx.notifications.tell(Command::Invoke(n.id));
            self.hide = true;
        }
    }

    fn letter(&mut self, key: &Key, i: usize) -> bool {
        if key.is('d')
            && let Some(n) = self.item(i)
        {
            self.tx.notifications.tell(Command::Dismiss(n.id));
            return true;
        }
        if key.is('D') && !self.items.is_empty() {
            self.tx.notifications.tell(Command::Clear);
            return true;
        }
        false
    }

    fn back(&mut self) -> bool {
        if *self.choosing {
            *self.choosing = false;
            return true;
        }
        false
    }
}

/// The widget's width, and whether it is drawn round (no stops then).
fn size(d: &Dash, id: u32) -> (f32, bool) {
    shared::card_of(d, id).map_or((0.0, true), |((_, _, w, _), round)| (w, round))
}

/// The stops of `id`: the choices, then the notifications.
fn stops(d: &Dash, id: u32) -> usize {
    let (w, round) = size(d, id);
    if round {
        return 0;
    }
    let n = &d.widgets.desk.notifications;
    n.choices(n.inst(id).choosing, w) + n.items.len()
}

/// The cursor of `id` stays on a stop that exists, with the parts the stop has.
fn settle(d: &mut Dash, id: u32) {
    let count = stops(d, id);
    let (w, _) = size(d, id);
    let n = &mut d.widgets.desk.notifications;
    let mut inst = n.inst(id);
    let choices = n.choices(inst.choosing, w);
    inst.scope.set_count(count, &NoSkip);
    inst.scope
        .set_subs(if inst.scope.cursor >= choices { 2 } else { 1 });
    n.insts.insert(id, inst);
}

struct NoSkip;
impl Stops for NoSkip {}

/// The list changed: every cursor stays on a stop that exists.
pub fn sync(d: &mut Dash) {
    let ids: Vec<u32> = d.widgets.desk.notifications.insts.keys().copied().collect();
    for id in ids {
        settle(d, id);
    }
}

pub fn entered(d: &mut Dash, id: u32) {
    settle(d, id);
}

pub fn key(d: &mut Dash, id: u32, key: &Key, cx: &mut Context<Dash>) -> bool {
    settle(d, id);
    let (width, round) = size(d, id);
    let n = &d.widgets.desk.notifications;
    let mut inst = n.inst(id);
    // z shows the do-not-disturb choices, which then come first.
    if key.is('z') && key.bare() && !round && n.pausable() {
        inst.choosing = !inst.choosing;
        inst.scope.cursor = 0;
        inst.scope.sub = 0;
        d.widgets.desk.notifications.insts.insert(id, inst);
        settle(d, id);
        return true;
    }
    let choices = n.choices(inst.choosing, width);
    let mut choosing = inst.choosing;
    let mut keys = Keys {
        items: &n.items,
        choices,
        width,
        state: n,
        choosing: &mut choosing,
        tx: &d.tx,
        hide: false,
    };
    let taken = inst.scope.press(key, &mut keys);
    let hide = keys.hide;
    inst.choosing = choosing;
    d.widgets.desk.notifications.insts.insert(id, inst);
    settle(d, id);
    if hide {
        shared::hide(d, cx);
    }
    taken
}

pub fn describe(d: &Dash, id: u32) -> String {
    let (w, _) = size(d, id);
    let n = &d.widgets.desk.notifications;
    let inst = n.inst(id);
    let choices = n.choices(inst.choosing, w);
    let c = inst.scope.cursor;
    if c < choices {
        format!("pause choice {}/{choices}", c + 1)
    } else if let Some(x) = n.items.get(c - choices) {
        format!(
            "notification {}/{} {:?}{}",
            c - choices + 1,
            n.items.len(),
            x.summary,
            if inst.scope.sub == 1 {
                ", its dismiss button"
            } else {
                ""
            }
        )
    } else {
        format!(
            "no notifications{}",
            if inst.choosing {
                ", choices showing"
            } else {
                ""
            }
        )
    }
}

pub fn check(d: &Dash, id: u32, entered: bool) -> Result<(), String> {
    let (w, round) = size(d, id);
    let n = &d.widgets.desk.notifications;
    let inst = n.inst(id);
    let count = stops(d, id);
    let choices = n.choices(inst.choosing, w);
    let scope = shared::eff(inst.scope, count);
    let c = scope.cursor;
    let parts = if c >= choices { 2 } else { 1 };
    if scope.sub >= parts {
        return Err(format!(
            "cursor on part {} of a stop with {parts}",
            scope.sub
        ));
    }
    if inst.choosing && !n.pausable() && !round {
        return Err("the pause choices show where nothing can be paused".into());
    }
    if entered
        && !round
        && c >= choices
        && c - choices < n.items.len()
        && n.lists.of(id).in_sight(c - choices) == Some(false)
    {
        return Err(format!(
            "cursor is out of sight: notification {}",
            c - choices
        ));
    }
    Ok(())
}

/// Escape: the pause choices close first (Slint's `back`).
pub fn escape(d: &mut Dash, id: u32) -> bool {
    let n = &mut d.widgets.desk.notifications;
    match n.insts.get_mut(&id) {
        Some(inst) if inst.choosing => {
            inst.choosing = false;
            settle(d, id);
            true
        }
        _ => false,
    }
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let tk = ctx.tk;
    let d = ctx.d;
    let n = &d.widgets.desk.notifications;
    let id = ctx.tile.id();
    let plasma = shared::plasma(d);
    if ctx.round {
        let (icon, tint) = if n.dnd_active {
            ("bell-off", tk.warn())
        } else if !n.items.is_empty() {
            ("bell", tk.accent())
        } else {
            ("bell", tk.faint())
        };
        let caption = if n.dnd_active {
            if n.missed > 0 {
                format!("{} missed", n.missed)
            } else {
                "Paused".into()
            }
        } else if !n.server && plasma {
            "Pop-ups on".into()
        } else if n.items.is_empty() {
            "None".into()
        } else {
            format!("{} new", n.items.len())
        };
        return parts::round_face(RoundIcon::Named(icon), tint, caption, ctx.w, tk);
    }
    let inst = n.inst(id);
    let on = ctx.entered;
    let choosing = inst.choosing;
    let choices = n.choices(choosing, ctx.w);
    let scope = shared::eff(inst.scope, choices + n.items.len());
    let third = n.third(ctx.w);
    let wide = ctx.w > 330.0;
    let bell = if n.dnd_active { "bell-off" } else { "bell" };

    let mut right: Vec<AnyElement> = Vec::new();
    if !n.items.is_empty() && !choosing {
        right.push(
            parts::text_button(
                ("notif-clear", id as usize),
                "Clear",
                "Clear notifications",
                Look::on(),
                tk,
                cx.listener(|d, _: &ClickEvent, _, _| {
                    d.tx.notifications.tell(Command::Clear);
                }),
            )
            .into_any_element(),
        );
    }
    if n.pausable() {
        right.push(
            parts::icon_button(
                ("notif-pause", id as usize),
                bell,
                if choosing {
                    "Hide pause choices"
                } else if n.dnd_active {
                    "Change notification pause"
                } else {
                    "Pause notifications"
                },
                28.0,
                tk.text(),
                Look {
                    active: choosing || n.dnd_active,
                    ..Look::on()
                },
                tk,
                cx.listener(move |d, _: &ClickEvent, _, cx| {
                    let n = &mut d.widgets.desk.notifications;
                    let mut inst = n.inst(id);
                    inst.choosing = !inst.choosing;
                    n.insts.insert(id, inst);
                    settle(d, id);
                    cx.notify();
                }),
            )
            .into_any_element(),
        );
    }

    let choose =
        |cx: &mut Context<Dash>, i: usize, text: String, a11y: String, primary: bool, to: u8| {
            parts::text_button(
                ("notif-choice", id as usize * 10 + i),
                text,
                a11y,
                Look {
                    kfocus: on && scope.cursor == i,
                    primary,
                    ..Look::on()
                },
                tk,
                cx.listener(move |d, _: &ClickEvent, _, cx| {
                    set_dnd(&d.tx, to);
                    let n = &mut d.widgets.desk.notifications;
                    let mut inst = n.inst(id);
                    inst.choosing = false;
                    n.insts.insert(id, inst);
                    settle(d, id);
                    cx.notify();
                }),
            )
        };

    let mut col = div()
        .size_full()
        .p(px(tk.pad()))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(parts::header(
            bell,
            if n.dnd_active { tk.warn() } else { tk.dim() },
            "Notifications",
            right,
            tk,
        ));
    if choosing {
        let mut buttons = div()
            .flex()
            .flex_row()
            .flex_none()
            .gap(px(6.0))
            .justify_start();
        buttons = buttons
            .child(choose(
                cx,
                0,
                "1 hour".into(),
                "Pause notifications for 1 hour".into(),
                false,
                1,
            ))
            .child(choose(
                cx,
                1,
                format!("Until {}", n.morning),
                format!("Pause notifications until {}", n.morning),
                false,
                2,
            ));
        if third {
            buttons = buttons.child(choose(
                cx,
                2,
                if wide { "Until turned off" } else { "Always" }.into(),
                "Pause notifications until turned off".into(),
                false,
                3,
            ));
        }
        if n.dnd_active {
            buttons = buttons.child(choose(
                cx,
                choices - 1,
                "Off".into(),
                "Stop pausing notifications".into(),
                true,
                0,
            ));
        }
        col = col.child(buttons);
    }
    if n.dnd_active && !choosing {
        let mut text = n.dnd_text.clone();
        if n.missed > 0 {
            text.push_str(&format!(" · {} without pop-up", n.missed));
        }
        col = col.child(parts::muted(text, tk).text_color(tk.warn()));
    }
    if !n.server && plasma {
        col = col.child(parts::empty_text(
            if n.can_pause {
                "Plasma shows notifications; the bell pauses its pop-ups"
            } else {
                "Plasma shows notifications"
            },
            tk,
        ));
    }
    if !n.server && !plasma {
        col = col.child(
            div()
                .flex_none()
                .text_size(px(tk.font_small()))
                .text_color(tk.dim())
                .child("Another notification daemon is running; showing nothing."),
        );
    }
    if n.items.is_empty() && (n.server || !plasma) {
        col = col.child(parts::empty_text("No notifications", tk));
    }
    if !n.items.is_empty() {
        let rows: Vec<AnyElement> = n
            .items
            .iter()
            .enumerate()
            .map(|(i, x)| {
                let at = choices + i;
                let here = on && scope.cursor == at;
                let nid = x.id;
                let has_default = x.has_default;
                let label = if x.app.is_empty() {
                    if x.summary.is_empty() {
                        "Notification".to_string()
                    } else {
                        x.summary.clone()
                    }
                } else if x.summary.is_empty() {
                    x.app.clone()
                } else {
                    format!("{}: {}", x.app, x.summary)
                };
                let radius = tk.radius_small();
                let raised = tk.card_raised();
                let card = div()
                    .id(("notification", id as usize * 10_000 + i))
                    .role(gpui::Role::ListItem)
                    .aria_label(label.clone())
                    .aria_state(if x.urgent { "urgent" } else { "" })
                    .aria_cursor(here && scope.sub == 0)
                    .debug_selector(move || label)
                    .relative()
                    .w_full()
                    .flex()
                    .flex_row()
                    .gap(px(10.0))
                    .p(px(tk.pad_tight()))
                    .rounded(px(radius))
                    .bg(if here {
                        parts::brighter(raised, 0.15)
                    } else {
                        raised
                    })
                    .hover(move |s| s.bg(parts::brighter(raised, 0.15)))
                    .when(has_default, |c| c.cursor_pointer())
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |d, _: &ClickEvent, _, cx| {
                        if has_default {
                            d.tx.notifications.tell(Command::Invoke(nid));
                            shared::hide(d, cx);
                        }
                    }))
                    .when_some(x.icon.as_ref(), |c, pic| c.child(pic.square(32.0, false)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap(px(6.0))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .child(parts::muted(x.app.clone(), tk)),
                                    )
                                    .child(parts::muted(x.time.clone(), tk)),
                            )
                            .child(
                                div()
                                    .text_size(px(tk.font_body()))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(tk.text())
                                    .child(x.summary.clone()),
                            )
                            .when(!x.body.is_empty(), |c| {
                                c.child(
                                    div()
                                        .max_h(px(52.0))
                                        .overflow_hidden()
                                        .text_size(px(tk.font_small()))
                                        .text_color(tk.dim())
                                        .child(x.body.clone()),
                                )
                            }),
                    )
                    .child(div().flex_none().child(parts::icon_button(
                        ("notification-dismiss", id as usize * 10_000 + i),
                        "close",
                        format!(
                            "Dismiss {}",
                            if x.summary.is_empty() {
                                "notification"
                            } else {
                                &x.summary
                            }
                        ),
                        24.0,
                        tk.faint(),
                        Look {
                            kfocus: here && scope.sub == 1,
                            ..Look::on()
                        },
                        tk,
                        cx.listener(move |d, _: &ClickEvent, _, _| {
                            d.tx.notifications.tell(Command::Dismiss(nid));
                        }),
                    )))
                    .when(here && scope.sub == 0, |c| {
                        c.child(parts::cursor_ring(radius, tk))
                    })
                    .when(x.urgent && !(here && scope.sub == 0), |c| {
                        c.child(
                            parts::cursor_ring(radius, tk)
                                .border_1()
                                .border_color(tk.bad()),
                        )
                    });
                div()
                    .flex_none()
                    .w_full()
                    .pb(px(6.0))
                    .child(card)
                    .into_any_element()
            })
            .collect();
        let cursor = (on && scope.cursor >= choices).then(|| scope.cursor - choices);
        col = col.child(n.lists.of(id).view(
            ("notif-list", id as usize),
            cursor,
            rows,
            super::list::fingerprint(ctx.w, ctx.h, tk),
            tk,
            cx,
        ));
    }
    col.into_any_element()
}

//! The phone widget: a phone through KDE Connect, with its battery, Ring and Send files. Round:
//! its battery as a ring. One row: name and battery; two rows: Ring and Send; three rows with
//! several paired: all of them. Port of `PhoneWidget` (`legacy/ui/widgets/devices.slint`) and the
//! phones part of `legacy/src/app/devices.rs`.

use super::parts::{self, Card, Revealed, icon_button, muted, row, text_button};
use crate::dash::{Dash, DashEvent};
use crate::keys::Key;
use crate::kit::{Tk, icon};
use crate::scope::{Scope, Stops};
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Hsla, Role, ScrollHandle, SharedString, div, px};
use niri_dash_system::{Tell, phones};

/// A paired device as the widget shows it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct P {
    pub id: String,
    pub name: String,
    pub status: String,
    pub reachable: bool,
    /// Percent; -1 when the phone doesn't say.
    pub charge: i32,
    pub charging: bool,
    pub can_ring: bool,
    pub can_share: bool,
}

impl P {
    fn knows_charge(&self) -> bool {
        self.reachable && self.charge >= 0
    }

    fn low(&self) -> bool {
        self.charge >= 0 && self.charge < 20 && !self.charging
    }
}

#[derive(Default)]
pub struct State {
    pub available: bool,
    pub phones: Vec<P>,
    pub scope: Scope,
    list: ScrollHandle,
    revealed: Revealed,
}

/// What the widget can do.
#[derive(Debug, Clone, PartialEq)]
pub enum Do {
    Ring(String),
    /// Pick files to send (with the desktop's file chooser, so the dashboard goes away).
    Send(String),
}

/// How much of the phones the card shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// The battery as a ring.
    Round,
    /// One row.
    Row,
    /// The first phone, with its buttons.
    Big,
    /// Every phone, in a list.
    All,
}

impl State {
    fn mode(&self, card: Card) -> Mode {
        if card.round {
            Mode::Round
        } else if card.h < 170.0 {
            Mode::Row
        } else if card.h >= 270.0 && self.phones.len() > 1 {
            Mode::All
        } else {
            Mode::Big
        }
    }

    /// The first phone; out of range reads as an empty one.
    fn main(&self) -> P {
        self.phones.first().cloned().unwrap_or_default()
    }

    fn empty_text(&self) -> &'static str {
        if self.available {
            "Pair a phone in KDE Connect"
        } else {
            "KDE Connect isn't running"
        }
    }

    fn keys(&self, card: Card) -> Keys {
        Keys {
            all: self.mode(card) == Mode::All,
            phones: self.phones.clone(),
            out: Vec::new(),
        }
    }

    pub fn sync(&mut self, card: Card) {
        let keys = self.keys(card);
        let n = if keys.all { self.phones.len() } else { 0 };
        let mut scope = self.scope;
        scope.set_count(n, &keys);
        self.scope = scope;
    }
}

/// r rings the phone, u uploads files to it (s is the settings, everywhere); with all of them
/// listed, j/k pick one.
struct Keys {
    all: bool,
    phones: Vec<P>,
    out: Vec<Do>,
}

impl Stops for Keys {
    fn letter(&mut self, key: &Key, cursor: usize) -> bool {
        if self.phones.is_empty() {
            return false;
        }
        let p = if self.all {
            self.phones.get(cursor)
        } else {
            self.phones.first()
        };
        let Some(p) = p else { return false };
        if key.is('r') && p.can_ring {
            self.out.push(Do::Ring(p.id.clone()));
            return true;
        }
        if key.is('u') && p.can_share {
            self.out.push(Do::Send(p.id.clone()));
            return true;
        }
        false
    }
}

pub fn run(d: &mut Dash, what: Do, cx: &mut Context<Dash>) {
    match what {
        Do::Ring(id) => d.tx.phones.tell(phones::Command::Ring(id)),
        Do::Send(id) => {
            // The file chooser would be behind the dashboard.
            cx.emit(DashEvent::Hide);
            d.tx.phones.tell(phones::Command::SendFiles(id));
        }
    }
    cx.notify();
}

pub fn show(d: &mut Dash, s: phones::State) {
    let st = &mut d.widgets.devices.phone;
    st.available = s.available;
    st.phones = s
        .phones
        .iter()
        .map(|ph| P {
            id: ph.id.clone(),
            name: ph.name.clone(),
            status: ph.status(),
            reachable: ph.reachable,
            charge: ph.charge.unwrap_or(-1),
            charging: ph.charging,
            can_ring: ph.can_ring,
            can_share: ph.can_share,
        })
        .collect();
    super::resync(d);
}

/// A key for the entered widget. True if it was taken.
pub fn key(d: &mut Dash, id: u32, key: &Key, cx: &mut Context<Dash>) -> bool {
    let card = d.widgets.devices.seen.of(id);
    let st = &mut d.widgets.devices.phone;
    st.sync(card);
    let mut keys = st.keys(card);
    let mut scope = st.scope;
    let taken = scope.press(key, &mut keys);
    st.scope = scope;
    for what in keys.out {
        run(d, what, cx);
    }
    cx.notify();
    taken
}

pub fn describe(d: &Dash) -> Option<String> {
    let st = &d.widgets.devices.phone;
    match st.phones.get(st.scope.cursor) {
        Some(p) if st.scope.count > 0 => Some(format!(
            "phone {}/{}: {}",
            st.scope.cursor + 1,
            st.scope.count,
            p.name
        )),
        _ => Some("no stops".into()),
    }
}

pub fn check(d: &Dash) -> Result<(), String> {
    let st = &d.widgets.devices.phone;
    if st.scope.cursor >= st.scope.count.max(1) {
        return Err(format!(
            "the cursor is on stop {} of {}",
            st.scope.cursor, st.scope.count
        ));
    }
    if st.scope.count > st.phones.len() {
        return Err(format!(
            "{} stops, but {} phones",
            st.scope.count,
            st.phones.len()
        ));
    }
    Ok(())
}

fn charge_tint(p: &P, tk: &Tk) -> Hsla {
    if p.low() { tk.bad() } else { tk.good() }
}

fn do_on_click(
    what: Do,
    cx: &mut Context<Dash>,
) -> impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static {
    cx.listener(move |d, _, _, cx| run(d, what.clone(), cx))
}

/// The battery as a ring, with what to read out.
fn battery(p: &P, thickness: f32, tk: &Tk) -> impl IntoElement {
    let value = if p.knows_charge() {
        p.charge as f32 / 100.0
    } else {
        0.0
    };
    div()
        .id("phone-battery")
        .role(Role::ProgressIndicator)
        .aria_label("Phone battery")
        .debug_selector(|| "Phone battery".into())
        .aria_value(if p.knows_charge() {
            format!("{}%", p.charge)
        } else {
            "unknown".into()
        })
        .absolute()
        .size_full()
        .child(parts::ring(
            value,
            thickness,
            charge_tint(p, tk),
            tk.track(),
        ))
}

/// One phone: name, battery and its buttons.
fn phone_row(
    i: usize,
    p: &P,
    buttons: bool,
    named: bool,
    focus: bool,
    tk: &Tk,
    cx: &mut Context<Dash>,
) -> gpui::Stateful<gpui::Div> {
    let ring_label: SharedString = if named {
        format!("Ring {}", p.name).into()
    } else {
        "Ring".into()
    };
    let send_label: SharedString = if named {
        format!("Send files to {}", p.name).into()
    } else {
        "Send files".into()
    };
    row(
        ("phone-row", i),
        p.name.clone(),
        Some(p.status.clone().into()),
        44.0,
        true,
        focus,
        tk,
        None,
    )
    .child(
        div()
            .size_full()
            .pl(px(8.0))
            .pr(px(4.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .child(icon(
                "phone",
                18.0,
                if p.reachable { tk.text() } else { tk.faint() },
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .child(
                        parts::elide(div())
                            .text_size(px(tk.font_body()))
                            .text_color(if p.reachable { tk.text() } else { tk.dim() })
                            .child(p.name.clone()),
                    )
                    .child(muted(p.status.clone(), tk).text_color(if p.low() {
                        tk.bad()
                    } else {
                        tk.dim()
                    })),
            )
            .when(buttons && p.can_ring, |r| {
                r.child(icon_button(
                    ("phone-ring", i),
                    "ring",
                    ring_label,
                    32.0,
                    true,
                    tk,
                    do_on_click(Do::Ring(p.id.clone()), cx),
                ))
            })
            .when(buttons && p.can_share, |r| {
                r.child(icon_button(
                    ("phone-send", i),
                    "send",
                    send_label,
                    32.0,
                    true,
                    tk,
                    do_on_click(Do::Send(p.id.clone()), cx),
                ))
            }),
    )
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let (d, tk) = (ctx.d, ctx.tk);
    let st = &d.widgets.devices.phone;
    let main = st.main();
    let count = st.phones.len();
    let pad = tk.pad();
    match st.mode(Card::of(ctx)) {
        Mode::Round => {
            let text: SharedString = if count == 0 {
                "No phone".into()
            } else if main.knows_charge() {
                format!("{}%", main.charge).into()
            } else if main.reachable {
                "connected".into()
            } else {
                "offline".into()
            };
            div()
                .size_full()
                .relative()
                .child(battery(&main, (ctx.w * 0.07).max(6.0), tk))
                .child(
                    div()
                        .absolute()
                        .size_full()
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(2.0))
                        .child(icon(
                            "phone",
                            (ctx.w * 0.28).min(40.0),
                            if main.reachable {
                                tk.text()
                            } else {
                                tk.faint()
                            },
                        ))
                        .child(
                            div()
                                .text_size(px((ctx.w * 0.11).clamp(11.0, 18.0)))
                                .text_color(tk.dim())
                                .child(text),
                        ),
                )
                .into_any_element()
        }
        Mode::Row => div()
            .size_full()
            .p(px(pad))
            .flex()
            .flex_row()
            .items_center()
            .child(if count == 0 {
                div()
                    .flex_1()
                    .text_center()
                    .child(muted(st.empty_text(), tk).text_center())
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .child(phone_row(0, &main, ctx.w >= 260.0, false, false, tk, cx))
                    .into_any_element()
            })
            .into_any_element(),
        Mode::Big => {
            let body = if count == 0 {
                parts::empty(st.empty_text(), tk).into_any_element()
            } else {
                let value: SharedString = if main.knows_charge() {
                    format!("{}%", main.charge).into()
                } else {
                    "–".into()
                };
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(14.0))
                    .child(
                        div()
                            .flex_none()
                            .relative()
                            .size(px(64.0))
                            .child(battery(&main, 6.0, tk))
                            .child(
                                div()
                                    .absolute()
                                    .size_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(tk.font_small()))
                                    .text_color(tk.text())
                                    .child(value),
                            ),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .justify_center()
                            .gap(px(8.0))
                            .child(muted(main.status.clone(), tk))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap(px(6.0))
                                    .child(text_button(
                                        "phone-ring",
                                        "Ring",
                                        "Ring",
                                        Some("ring"),
                                        false,
                                        main.can_ring,
                                        false,
                                        tk,
                                        do_on_click(Do::Ring(main.id.clone()), cx),
                                    ))
                                    .child(if ctx.w >= 300.0 {
                                        text_button(
                                            "phone-send",
                                            "Send files",
                                            "Send files",
                                            Some("send"),
                                            false,
                                            main.can_share,
                                            false,
                                            tk,
                                            do_on_click(Do::Send(main.id.clone()), cx),
                                        )
                                        .into_any_element()
                                    } else {
                                        icon_button(
                                            "phone-send",
                                            "send",
                                            "Send files",
                                            30.0,
                                            main.can_share,
                                            tk,
                                            do_on_click(Do::Send(main.id.clone()), cx),
                                        )
                                        .into_any_element()
                                    }),
                            ),
                    )
                    .into_any_element()
            };
            div()
                .size_full()
                .p(px(pad))
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(parts::header(
                    "phone",
                    if count == 0 {
                        "Phone".to_string()
                    } else {
                        main.name.clone()
                    },
                    tk,
                    Vec::new(),
                ))
                .child(body)
                .into_any_element()
        }
        Mode::All => {
            let cursor = ctx.entered.then_some(st.scope.cursor);
            st.revealed
                .show(&st.list, cursor.map(|c| (c, count)), cursor.unwrap_or(0));
            let rows = st
                .phones
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    phone_row(i, p, true, true, cursor == Some(i), tk, cx).into_any_element()
                })
                .collect();
            div()
                .size_full()
                .p(px(pad))
                .flex()
                .flex_col()
                .gap(px(6.0))
                .child(parts::header("phone", "Phones", tk, Vec::new()))
                .child(
                    parts::scroll_list("phone-list", &st.list, rows, tk)
                        .debug_selector(|| "phone list".into()),
                )
                .into_any_element()
        }
    }
}

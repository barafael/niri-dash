//! The privacy widget: who uses the microphone and camera (PipeWire), who shares the screen
//! (niri), and a way to stop a share. Port of `PrivacyWidget` (`legacy/ui/widgets/devices.slint`)
//! and `legacy/src/app/privacy.rs`.

use super::parts::{self, Card, Revealed, muted, text_button};
use crate::dash::Dash;
use crate::keys::Key;
use crate::kit::{Tk, icon, with_alpha};
use crate::scope::{Scope, Stops};
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Role, ScrollHandle, SharedString, div, px};
use niri_dash_system::privacy::{Command, ScreenCast, Usage};
use niri_dash_system::{Tell, notifications};

/// What uses what.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum What {
    Mic,
    Camera,
    Screen,
}

/// One line of the list: an app, and what it uses.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub what: What,
    pub app: String,
    pub detail: String,
    /// The screencast session, for a share.
    pub session: u64,
}

impl Item {
    fn icon(&self) -> &'static str {
        match self.what {
            What::Mic => "mic",
            What::Camera => "camera",
            What::Screen => "screen-share",
        }
    }

    fn says(&self) -> &'static str {
        match self.what {
            What::Mic => "uses the microphone",
            What::Camera => "uses the camera",
            What::Screen => "shares the screen",
        }
    }
}

#[derive(Default)]
pub struct State {
    /// Microphone and camera use, from PipeWire.
    pub usage: Usage,
    /// Screencasts, from niri.
    pub casts: Vec<ScreenCast>,
    /// The keyboard cursor: over the screen shares, the only thing there is to stop.
    pub scope: Scope,
    list: ScrollHandle,
    revealed: Revealed,
}

/// What the widget can do.
#[derive(Debug, Clone, PartialEq)]
pub enum Do {
    /// Stop screencast session `.0`.
    Stop(u64),
}

impl State {
    /// Everything using something: the microphone's apps, the camera's, then the shares. A share
    /// is named by its process, else by who reads its PipeWire stream.
    pub fn items(&self) -> Vec<Item> {
        let mut items = Vec::new();
        for app in &self.usage.mic {
            items.push(Item {
                what: What::Mic,
                app: app.clone(),
                detail: "microphone".into(),
                session: 0,
            });
        }
        for app in &self.usage.camera {
            items.push(Item {
                what: What::Camera,
                app: app.clone(),
                detail: "camera".into(),
                session: 0,
            });
        }
        for c in &self.casts {
            let app = c
                .process
                .clone()
                .or_else(|| {
                    let node = c.pw_node?;
                    self.usage
                        .video_readers
                        .iter()
                        .find(|(n, _)| *n == node)
                        .map(|(_, app)| app.clone())
                })
                .unwrap_or_else(|| "An app".into());
            items.push(Item {
                what: What::Screen,
                app,
                detail: format!("{}{}", c.target, if c.active { "" } else { " · paused" }),
                session: c.session_id,
            });
        }
        items
    }

    /// Whether the card has room for the list of who uses what.
    pub fn listing(card: Card) -> bool {
        !card.round && card.h > 150.0
    }

    /// How many stops the keyboard cursor has: the shares, when the list is shown.
    pub fn stops(&self, card: Card) -> usize {
        if Self::listing(card) {
            self.casts.len()
        } else {
            0
        }
    }

    /// The number of stops may have changed: the cursor stays on one that exists.
    pub fn sync(&mut self, card: Card) {
        let n = self.stops(card);
        let mut scope = self.scope;
        scope.set_count(n, &Nothing);
        self.scope = scope;
    }
}

struct Nothing;
impl Stops for Nothing {}

/// The keys: j/k over the screen shares, Enter stops one.
struct Keys {
    sessions: Vec<u64>,
    out: Vec<Do>,
}

impl Stops for Keys {
    fn activate(&mut self, cursor: usize, _sub: usize) {
        if let Some(&s) = self.sessions.get(cursor) {
            self.out.push(Do::Stop(s));
        }
    }
}

pub fn run(d: &mut Dash, what: Do) {
    match what {
        Do::Stop(session) => d.tx.privacy.tell(Command::StopCast(session)),
    }
}

/// Who is casting, from niri.
pub fn set_casts(d: &mut Dash, casts: Vec<ScreenCast>) {
    let st = &mut d.widgets.devices.privacy;
    let started = casts
        .iter()
        .any(|c| !st.casts.iter().any(|o| o.session_id == c.session_id));
    st.casts = casts;
    if started {
        d.tx.notifications.tell(notifications::Command::Osd {
            text: "Screen sharing started".into(),
            icon: "video-display".into(),
        });
    }
    super::resync(d);
}

/// Who uses the microphone and camera, from PipeWire.
pub fn set_usage(d: &mut Dash, usage: Usage) {
    d.widgets.devices.privacy.usage = usage;
    super::resync(d);
}

/// A key for the entered widget. True if it was taken.
pub fn key(d: &mut Dash, id: u32, key: &Key, cx: &mut Context<Dash>) -> bool {
    let card = d.widgets.devices.seen.of(id);
    let st = &mut d.widgets.devices.privacy;
    st.sync(card);
    let mut stops = Keys {
        sessions: if State::listing(card) {
            st.casts.iter().map(|c| c.session_id).collect()
        } else {
            Vec::new()
        },
        out: Vec::new(),
    };
    let mut scope = st.scope;
    let taken = scope.press(key, &mut stops);
    st.scope = scope;
    for what in stops.out {
        run(d, what);
    }
    cx.notify();
    taken
}

/// Where the cursor is, for the stress run's log.
pub fn describe(d: &Dash) -> Option<String> {
    let st = &d.widgets.devices.privacy;
    let shares: Vec<Item> = st
        .items()
        .into_iter()
        .filter(|i| i.what == What::Screen)
        .collect();
    match shares.get(st.scope.cursor) {
        Some(i) if st.scope.count > 0 => Some(format!(
            "stop {}/{}: {}",
            st.scope.cursor + 1,
            st.scope.count,
            i.app
        )),
        _ => Some("no stops".into()),
    }
}

/// What has to hold: the cursor is on a share that exists.
pub fn check(d: &Dash) -> Result<(), String> {
    let st = &d.widgets.devices.privacy;
    if st.scope.cursor >= st.scope.count.max(1) {
        return Err(format!(
            "the cursor is on stop {} of {}",
            st.scope.cursor, st.scope.count
        ));
    }
    if st.scope.count > st.casts.len() {
        return Err(format!(
            "{} stops, but {} screen shares",
            st.scope.count,
            st.casts.len()
        ));
    }
    Ok(())
}

/// One of the round dots: what it watches, lit when it is in use.
fn dot(name: &'static str, icon_name: &str, on: bool, size: f32, tk: &Tk) -> AnyElement {
    div()
        .id(name)
        .role(Role::Image)
        .aria_label(name)
        .aria_value(if on { "in use" } else { "not in use" })
        .debug_selector(move || name.to_string())
        .flex_none()
        .size(px(size))
        .rounded_full()
        .bg(if on {
            with_alpha(tk.bad(), 0.2)
        } else {
            tk.card_raised()
        })
        .flex()
        .items_center()
        .justify_center()
        .child(icon(
            icon_name,
            size * 0.5,
            if on { tk.bad() } else { tk.faint() },
        ))
        .into_any_element()
}

fn dots(d: &Dash, st: &State, size: f32, tk: &Tk) -> Vec<AnyElement> {
    let mut dots = vec![
        dot("Microphone", "mic", !st.usage.mic.is_empty(), size, tk),
        dot("Camera", "camera", !st.usage.camera.is_empty(), size, tk),
    ];
    // Screen sharing is only known from niri.
    if d.caps.screencast {
        dots.push(dot(
            "Screen sharing",
            "screen-share",
            !st.casts.is_empty(),
            size,
            tk,
        ));
    }
    dots
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let (d, tk) = (ctx.d, ctx.tk);
    let st = &d.widgets.devices.privacy;
    let card = Card::of(ctx);
    let items = st.items();
    let any = !items.is_empty();
    if !State::listing(card) {
        let size = if ctx.round {
            ctx.w * 0.24
        } else {
            (ctx.h * 0.42).clamp(24.0, 40.0)
        };
        let summary: SharedString = if !any {
            "Nothing in use".into()
        } else if items.len() == 1 {
            items[0].app.clone().into()
        } else {
            format!("{} in use", items.len()).into()
        };
        return div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_center()
                    .gap(px(size * 0.25))
                    .children(dots(d, st, size, tk)),
            )
            .when(ctx.h > 90.0 || ctx.round, |c| {
                c.child(
                    parts::elide(div())
                        .max_w_full()
                        .text_center()
                        .text_size(px(tk.font_small()))
                        .text_color(if any { tk.text() } else { tk.faint() })
                        .child(summary),
                )
            })
            .into_any_element();
    }

    let first_share = items.len() - st.casts.len();
    let cursor = ctx.entered.then_some(st.scope.cursor);
    st.revealed.show(
        &st.list,
        cursor.map(|c| (c, items.len())),
        cursor.map_or(0, |c| first_share + c),
    );
    let body = if !any {
        parts::empty(
            if d.caps.screencast {
                "Nothing uses the microphone, camera or screen"
            } else {
                "Nothing uses the microphone or camera"
            },
            tk,
        )
        .into_any_element()
    } else {
        let rows = items
            .iter()
            .enumerate()
            .map(|(i, it)| {
                let stop = it.what == What::Screen;
                let focused = stop && cursor == Some(i - first_share);
                let (app, session) = (it.app.clone(), it.session);
                div()
                    .id(("privacy-item", i))
                    .role(Role::ListItem)
                    .aria_label(it.app.clone())
                    .aria_value(it.says())
                    .flex_none()
                    .w_full()
                    .h(px(36.0))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.0))
                    .child(icon(it.icon(), 18.0, tk.bad()))
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
                                    .text_color(tk.text())
                                    .child(it.app.clone()),
                            )
                            .when(!it.detail.is_empty(), |c| {
                                c.child(muted(it.detail.clone(), tk))
                            }),
                    )
                    .when(stop, |r| {
                        r.child(text_button(
                            ("privacy-stop", i),
                            "Stop",
                            format!("Stop sharing with {app}"),
                            None,
                            true,
                            true,
                            focused,
                            tk,
                            cx.listener(move |d, _, _, cx| {
                                run(d, Do::Stop(session));
                                cx.notify();
                            }),
                        ))
                    })
                    .into_any_element()
            })
            .collect();
        parts::scroll_list("privacy-list", &st.list, rows, tk)
            .debug_selector(|| "privacy list".into())
            .into_any_element()
    };
    div()
        .size_full()
        .p(px(tk.pad()))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(parts::header(
            "lock-small",
            "Privacy",
            tk,
            dots(d, st, 24.0, tk),
        ))
        .child(body)
        .into_any_element()
}

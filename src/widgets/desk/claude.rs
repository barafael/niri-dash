//! The Claude sessions widget: the Claude Code sessions that run, and a way to their windows.
//! Port of `ClaudeSessionsWidget` in `legacy/ui/widgets/desk.slint` and `show_claude` in
//! `legacy/src/app/feeds.rs`.

use super::list::Lists;
use super::parts;
use super::shared::{self, Scopes};
use crate::dash::Dash;
use crate::keys::Key;
use crate::scope::{Scope, Stops};
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, ClickEvent, Context, div, px};
use niri_dash_system::claude::{State as Sys, Status};
use niri_dash_system::windows::Command as Windows;
use niri_dash_system::{Senders, Tell};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub pid: i32,
    pub title: String,
    pub place: String,
    pub status: String,
    pub busy: bool,
    pub has_window: bool,
}

#[derive(Default)]
pub struct State {
    pub sessions: Vec<Session>,
    pub busy: usize,
    /// The window each session runs in, by pid (those that have one).
    pub windows: HashMap<i32, u64>,
    pub scopes: Scopes,
    pub lists: Lists,
}

/// The sessions as the system reports them.
pub fn show(d: &mut Dash, s: Sys) {
    let c = &mut d.widgets.desk.claude;
    c.windows.clear();
    c.busy = s.busy;
    c.sessions = s
        .sessions
        .iter()
        .map(|x| {
            if let Some(w) = x.window {
                c.windows.insert(x.pid, w);
            }
            Session {
                pid: x.pid,
                title: x.title.clone(),
                place: x.place.clone(),
                status: x.status_text.clone(),
                busy: x.status == Status::Busy,
                has_window: x.window.is_some(),
            }
        })
        .collect();
    sync(d);
}

/// Go to session `pid`'s window: its terminal tab first, where the terminal can be asked.
fn focus(tx: &Senders, windows: &HashMap<i32, u64>, pid: i32) -> bool {
    let Some(win) = windows.get(&pid).copied() else {
        return false;
    };
    tx.windows.tell(Windows::ShowTab(pid));
    tx.windows.tell(Windows::Focus(win));
    true
}

struct Keys<'a> {
    sessions: &'a [Session],
    windows: &'a HashMap<i32, u64>,
    tx: &'a Senders,
    hide: bool,
}

impl Stops for Keys<'_> {
    /// Enter focuses the session under the cursor, if it has a window (one that runs where none
    /// was found, over ssh or in a multiplexer, can be read but not focused).
    fn activate(&mut self, i: usize, _sub: usize) {
        if let Some(s) = self.sessions.get(i)
            && s.has_window
            && focus(self.tx, self.windows, s.pid)
        {
            self.hide = true;
        }
    }
}

fn count(d: &Dash, id: u32) -> usize {
    match shared::card_of(d, id) {
        Some((_, true)) => 0,
        _ => d.widgets.desk.claude.sessions.len(),
    }
}

fn settle(d: &mut Dash, id: u32) {
    let n = count(d, id);
    let c = &mut d.widgets.desk.claude;
    let mut scope = c.scopes.get(id);
    scope.set_count(n, &NoSkip);
    c.scopes.put(id, scope);
}

struct NoSkip;
impl Stops for NoSkip {}

/// The sessions changed: every cursor stays on a stop that exists.
pub fn sync(d: &mut Dash) {
    let ids: Vec<u32> = d.widgets.desk.claude.scopes.0.keys().copied().collect();
    for id in ids {
        settle(d, id);
    }
}

pub fn entered(d: &mut Dash, id: u32) {
    settle(d, id);
}

pub fn key(d: &mut Dash, id: u32, key: &Key, cx: &mut Context<Dash>) -> bool {
    settle(d, id);
    let c = &d.widgets.desk.claude;
    let mut scope = c.scopes.get(id);
    let mut keys = Keys {
        sessions: &c.sessions,
        windows: &c.windows,
        tx: &d.tx,
        hide: false,
    };
    let taken = scope.press(key, &mut keys);
    let hide = keys.hide;
    d.widgets.desk.claude.scopes.put(id, scope);
    if hide {
        shared::hide(d, cx);
    }
    taken
}

pub fn describe(d: &Dash, id: u32) -> String {
    let c = &d.widgets.desk.claude;
    let s: Scope = c.scopes.get(id);
    match c.sessions.get(s.cursor) {
        Some(x) => format!("session {}/{} {:?}", s.cursor + 1, c.sessions.len(), x.title),
        None => "no sessions".into(),
    }
}

pub fn check(d: &Dash, id: u32, entered: bool) -> Result<(), String> {
    let c = &d.widgets.desk.claude;
    let n = count(d, id);
    let s = shared::eff(c.scopes.get(id), n);
    if entered && n > 0 && c.lists.of(id).in_sight(s.cursor) == Some(false) {
        return Err(format!("cursor is out of sight: session {}", s.cursor));
    }
    Ok(())
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let tk = ctx.tk;
    let c = &ctx.d.widgets.desk.claude;
    let id = ctx.tile.id();
    let busy = c.busy;
    if ctx.round {
        let w = ctx.w;
        let frac = if c.sessions.is_empty() {
            0.0
        } else {
            busy as f32 / c.sessions.len() as f32
        };
        return div()
            .size_full()
            .relative()
            .child(
                div()
                    .id(("claude-ring", id as usize))
                    .absolute()
                    .size_full()
                    .role(gpui::Role::ProgressIndicator)
                    .aria_label("Claude sessions working")
                    .child(parts::ring(
                        frac,
                        (w * 0.06).max(6.0),
                        tk.accent(),
                        tk.track(),
                    )),
            )
            .child(
                div()
                    .absolute()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(2.0))
                    .child(
                        div()
                            .text_size(px(w * 0.26))
                            .font_weight(tk.display_weight())
                            .text_color(tk.text())
                            .child(c.sessions.len().to_string()),
                    )
                    .child(
                        div()
                            .text_size(px((w * 0.07).max(10.0)))
                            .text_color(if busy > 0 { tk.accent() } else { tk.dim() })
                            .child(if busy > 0 {
                                format!("{busy} working")
                            } else {
                                "sessions".to_string()
                            }),
                    ),
            )
            .into_any_element();
    }
    let scope = shared::eff(c.scopes.get(id), c.sessions.len());
    let on = ctx.entered;
    let rows: Vec<AnyElement> = c
        .sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let (pid, has_window) = (s.pid, s.has_window);
            let name = if s.title.is_empty() {
                "Claude session".to_string()
            } else {
                s.title.clone()
            };
            let row = parts::row(
                ("claude-session", id as usize * 10_000 + i),
                46.0,
                false,
                on && scope.cursor == i,
                has_window,
                tk,
            )
            .aria_label(name.clone())
            .aria_value(format!("{} · {}", s.place, s.status))
            .debug_selector(move || name)
            .px(px(10.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.0))
            .when(has_window, |r| {
                r.on_click(cx.listener(move |d, _: &ClickEvent, _, cx| {
                    let c = &d.widgets.desk.claude;
                    if focus(&d.tx, &c.windows, pid) {
                        shared::hide(d, cx);
                    }
                }))
            })
            .child(parts::status_dot(s.busy, tk))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(tk.font_body()))
                            .text_color(tk.text())
                            .child(s.title.clone()),
                    )
                    .child(parts::muted(format!("{}  ·  {}", s.place, s.status), tk)),
            );
            div()
                .flex_none()
                .w_full()
                .h(px(48.0))
                .child(row)
                .into_any_element()
        })
        .collect();
    let cursor = (on && !c.sessions.is_empty()).then_some(scope.cursor);
    let count = c.sessions.len();
    div()
        .size_full()
        .p(px(tk.pad()))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(parts::header(
            "sparkle",
            if busy > 0 { tk.accent() } else { tk.dim() },
            "Claude sessions",
            vec![parts::muted(
                if busy > 0 {
                    format!("{busy} working · {count}")
                } else {
                    count.to_string()
                },
                tk,
            )
            .into_any_element()],
            tk,
        ))
        .child(if c.sessions.is_empty() {
            parts::empty_text("No Claude Code sessions running", tk)
        } else {
            c.lists.of(id).view(
                ("claude-list", id as usize),
                cursor,
                rows,
                super::list::fingerprint(ctx.w, ctx.h, tk),
                tk,
                cx,
            )
        })
        .into_any_element()
}

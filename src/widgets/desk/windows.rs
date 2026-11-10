//! The windows widget: every window, by workspace; click one to go to it, middle-click or the
//! close button to close it. Port of `WindowsWidget` in `legacy/ui/widgets/desk.slint` and
//! `show_windows` in `legacy/src/app/feeds.rs`.

use super::list::Lists;
use super::parts::{self, Look, RoundIcon};
use super::pics::{Pic, Pics};
use super::shared::{self, Scopes};
use crate::dash::Dash;
use crate::keys::Key;
use crate::scope::Stops;
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, ClickEvent, Context, MouseButton, MouseUpEvent, div, px};
use niri_dash_system::windows::{Command, State as Sys};
use niri_dash_system::{Senders, Tell};

/// A row of the list: a window, or the heading of a workspace.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: u64,
    pub header: bool,
    pub title: String,
    pub app: String,
    pub icon: Option<Pic>,
    pub focused: bool,
    pub urgent: bool,
}

pub struct State {
    pub rows: Vec<Row>,
    pub count: usize,
    /// The compositor's window list works.
    pub available: bool,
    /// The focused window's icon, for the round widget.
    pub focused_icon: Option<Pic>,
    pub scopes: Scopes,
    pub lists: Lists,
}

impl Default for State {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            count: 0,
            available: true,
            focused_icon: None,
            scopes: Scopes::default(),
            lists: Lists::default(),
        }
    }
}

/// The windows as the system reports them.
pub fn show(d: &mut Dash, s: Sys) {
    let desk = &mut d.widgets.desk;
    desk.windows.take(&mut desk.pics, s);
    sync(d);
}

impl State {
    fn take(&mut self, pics: &mut Pics, s: Sys) {
        self.rows = s
            .rows
            .iter()
            .map(|r| Row {
                id: r.id,
                header: r.header,
                title: r.title.clone(),
                app: r.app.clone(),
                icon: pics.get(&r.icon),
                focused: r.focused,
                urgent: r.urgent,
            })
            .collect();
        self.count = s.count;
        self.available = s.connected;
        self.focused_icon = s.focused_icon.as_ref().and_then(|i| pics.get(i));
    }
}

struct Keys<'a> {
    rows: &'a [Row],
    tx: &'a Senders,
    hide: bool,
}

impl Keys<'_> {
    fn window(&self, i: usize) -> Option<&Row> {
        self.rows.get(i).filter(|r| !r.header)
    }
}

impl Stops for Keys<'_> {
    fn skip(&self, i: usize) -> bool {
        self.rows.get(i).is_some_and(|r| r.header)
    }

    fn activate(&mut self, i: usize, sub: usize) {
        let Some(row) = self.window(i) else { return };
        if sub == 1 {
            self.tx.windows.tell(Command::Close(row.id));
        } else {
            self.tx.windows.tell(Command::Focus(row.id));
            self.hide = true;
        }
    }

    fn letter(&mut self, key: &Key, i: usize) -> bool {
        if (key.is('d') || key.is('x'))
            && let Some(row) = self.window(i)
        {
            self.tx.windows.tell(Command::Close(row.id));
            return true;
        }
        false
    }
}

/// How many stops widget `id` has.
fn count(d: &Dash, id: u32) -> usize {
    match shared::card_of(d, id) {
        Some((_, true)) => 0,
        _ => d.widgets.desk.windows.rows.len(),
    }
}

/// The cursor of `id` follows the list: on a stop that exists, not on a heading.
fn settle(d: &mut Dash, id: u32) {
    let n = count(d, id);
    let w = &mut d.widgets.desk.windows;
    let mut scope = w.scopes.get(id);
    scope.subs = 2;
    let keys = Keys {
        rows: &w.rows,
        tx: &d.tx,
        hide: false,
    };
    scope.set_count(n, &keys);
    scope.settle(&keys);
    w.scopes.put(id, scope);
}

/// The list changed: every cursor stays on a stop that exists.
pub fn sync(d: &mut Dash) {
    let ids: Vec<u32> = d.widgets.desk.windows.scopes.0.keys().copied().collect();
    for id in ids {
        settle(d, id);
    }
}

pub fn entered(d: &mut Dash, id: u32) {
    settle(d, id);
}

pub fn key(d: &mut Dash, id: u32, key: &Key, cx: &mut Context<Dash>) -> bool {
    settle(d, id);
    let w = &d.widgets.desk.windows;
    let mut scope = w.scopes.get(id);
    let mut keys = Keys {
        rows: &w.rows,
        tx: &d.tx,
        hide: false,
    };
    let taken = scope.press(key, &mut keys);
    let hide = keys.hide;
    d.widgets.desk.windows.scopes.put(id, scope);
    if hide {
        shared::hide(d, cx);
    }
    taken
}

pub fn describe(d: &Dash, id: u32) -> String {
    let w = &d.widgets.desk.windows;
    let s = w.scopes.get(id);
    match w.rows.get(s.cursor) {
        Some(r) => format!(
            "window {}/{} {:?}{}",
            s.cursor + 1,
            w.rows.len(),
            r.title,
            if s.sub == 1 { ", its close button" } else { "" }
        ),
        None => format!("no windows ({} rows)", w.rows.len()),
    }
}

pub fn check(d: &Dash, id: u32, entered: bool) -> Result<(), String> {
    let w = &d.widgets.desk.windows;
    // A widget never entered has no cursor to check.
    if !w.scopes.0.contains_key(&id) {
        return Ok(());
    }
    let n = count(d, id);
    let s = shared::eff(w.scopes.get(id), n);
    if n > 0 && w.rows.get(s.cursor).is_some_and(|r| r.header) {
        return Err(format!("cursor on the workspace heading at row {}", s.cursor));
    }
    if s.sub > 1 {
        return Err(format!("cursor on part {} of a window row", s.sub));
    }
    if entered
        && n > 0
        && w.available
        && w.lists.of(id).in_sight(s.cursor) == Some(false)
    {
        return Err(format!("cursor is out of sight: window row {}", s.cursor));
    }
    Ok(())
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let tk = ctx.tk;
    let w = &ctx.d.widgets.desk.windows;
    if ctx.round {
        let (icon, tint) = match &w.focused_icon {
            Some(p) => (RoundIcon::Picture(p.clone()), tk.dim()),
            None => (RoundIcon::Named("window"), tk.dim()),
        };
        return parts::round_face(icon, tint, format!("{} windows", w.count), ctx.w, tk);
    }
    let id = ctx.tile.id();
    let scope = shared::eff(w.scopes.get(id), w.rows.len());
    let on = ctx.entered;
    let narrow = ctx.w - 2.0 * tk.pad() < 240.0;
    let rows: Vec<AnyElement> = w
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            if r.header {
                return div()
                    .flex_none()
                    .w_full()
                    .h(px(26.0))
                    .px(px(6.0))
                    .flex()
                    // Slint's label is asked to sit at the bottom but is drawn centered.
                    .items_center()
                    .child(parts::section_label(r.title.clone(), tk).w_full())
                    .into_any_element();
            }
            let cursor_here = on && scope.cursor == i;
            let name = if !r.title.is_empty() {
                r.title.clone()
            } else if !r.app.is_empty() {
                r.app.clone()
            } else {
                "Window".to_string()
            };
            let (wid, title) = (r.id, r.title.clone());
            let pad_l = if narrow { 4.0 } else { 8.0 };
            let gap = if narrow { 4.0 } else { 8.0 };
            let row = parts::row(
                ("window", id as usize * 10_000 + i),
                36.0,
                r.focused,
                cursor_here && scope.sub == 0,
                true,
                tk,
            )
            .aria_label(name.clone())
            .aria_value(if r.focused {
                "focused"
            } else if r.urgent {
                "urgent"
            } else {
                ""
            })
            .debug_selector({
                let name = name.clone();
                move || name
            })
            .flex()
            .flex_row()
            .items_center()
            .pl(px(pad_l))
            .pr(px(4.0))
            .gap(px(gap))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |d, _: &ClickEvent, _, cx| {
                d.tx.windows.tell(Command::Focus(wid));
                shared::hide(d, cx);
            }))
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(move |d, _: &MouseUpEvent, _, _| {
                    d.tx.windows.tell(Command::Close(wid));
                }),
            )
            .child(match &r.icon {
                Some(pic) => pic.square(22.0, false),
                None => crate::kit::icon("window", 20.0, tk.faint()).into_any_element(),
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(tk.font_body()))
                    .text_color(if r.urgent { tk.warn() } else { tk.text() })
                    .child(r.title.clone()),
            )
            .when(!narrow, |row| {
                row.child(div().max_w(px(120.0)).flex().child(parts::muted(r.app.clone(), tk)))
            })
            .child(parts::icon_button(
                ("window-close", id as usize * 10_000 + i),
                "close",
                format!("Close {}", if title.is_empty() { "window" } else { &title }),
                26.0,
                tk.faint(),
                Look {
                    kfocus: cursor_here && scope.sub == 1,
                    ..Look::on()
                },
                tk,
                cx.listener(move |d, _: &ClickEvent, _, _| {
                    d.tx.windows.tell(Command::Close(wid));
                }),
            ));
            div()
                .flex_none()
                .w_full()
                .h(px(38.0))
                .child(row)
                .into_any_element()
        })
        .collect();
    let cursor = (on && !w.rows.is_empty()).then_some(scope.cursor);
    let unavailable = if shared::niri(ctx.d) {
        "Not connected to niri".to_string()
    } else if shared::plasma(ctx.d) {
        "KWin gives no window list: run niri-dash desktop-entries, then restart".to_string()
    } else {
        format!("Window list {}", ctx.d.caps.unavailable())
    };
    div()
        .size_full()
        .p(px(tk.pad()))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(parts::header(
            "window",
            tk.dim(),
            "Windows",
            vec![parts::muted(w.count.to_string(), tk).into_any_element()],
            tk,
        ))
        .child(if w.available {
            w.lists.of(id).view(
                ("window-list", id as usize),
                cursor,
                rows,
                super::list::fingerprint(ctx.w, ctx.h, tk),
                tk,
                cx,
            )
        } else {
            parts::empty_text(unavailable, tk)
        })
        .into_any_element()
}

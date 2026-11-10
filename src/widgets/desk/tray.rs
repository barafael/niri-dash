//! The tray widget (status notifier items) and the menu of one, drawn over the whole window.
//! Port of `TrayWidget` in `legacy/ui/widgets/desk.slint`, `TrayMenu` in `legacy/ui/app.slint` and
//! the tray parts of `legacy/src/app/feeds.rs`.

use super::list::ScrollList;
use super::parts;
use super::pics::{Pic, Pics};
use super::shared::{self, Scopes};
use crate::dash::{Dash, DashEvent};
use crate::keys::{Key, Named};
use crate::kit::Tk;
use crate::scope::Stops;
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, Context, MouseButton, MouseUpEvent, Role, div, point, px};
use niri_dash_system::tray::{Command, Entry, Event as Sys, Item, Toggle};
use niri_dash_system::{Senders, Tell};
use crate::kit::AriaCursor;

/// A tray icon.
#[derive(Debug, Clone)]
pub struct TrayItem {
    pub item: Item,
    pub icon: Option<Pic>,
}

/// The menu that is open.
#[derive(Debug, Clone)]
pub struct Menu {
    pub address: String,
    pub title: String,
    pub entries: Vec<Entry>,
    /// Where it was asked for, in window pixels.
    pub x: f32,
    pub y: f32,
    /// The entry under the keyboard cursor of a menu opened from the keyboard.
    pub cursor: Option<usize>,
}

#[derive(Default)]
pub struct State {
    pub items: Vec<TrayItem>,
    pub menu: Option<Menu>,
    /// The keyboard opened the menu: j/k and Enter work in it.
    pub kbd_menu: bool,
    pub scopes: Scopes,
    pub menu_list: ScrollList,
}

/// How the icons sit in a card of `w` by `h`: the room around them, the side of a slot, how
/// many fit in a row, how many show at all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub inset: f32,
    pub slot: f32,
    pub per_row: usize,
    pub shown: usize,
}

pub fn grid(w: f32, h: f32, round: bool, pad_tight: f32, n: usize) -> Grid {
    let inset = if round { w * 0.17 } else { pad_tight };
    let slot = if round {
        28.0f32.max((w - 2.0 * inset) / 3.0)
    } else {
        44.0f32.min(h - 2.0 * inset)
    };
    let per_row = (((w - 2.0 * inset) / slot).floor() as i64).max(1) as usize;
    let rows = (((h + 1.0 - inset) / slot).floor() as i64).max(0) as usize;
    Grid {
        inset,
        slot,
        per_row,
        shown: n.min(per_row * rows),
    }
}

impl Grid {
    /// The centre of icon `i`, relative to the card.
    fn centre(&self, i: usize) -> (f32, f32) {
        (
            self.inset + ((i % self.per_row) as f32 + 0.5) * self.slot,
            self.inset + ((i / self.per_row) as f32 + 0.5) * self.slot,
        )
    }
}

/// What the system reports: the icons, or a menu to show.
pub fn event(d: &mut Dash, e: Sys) {
    match e {
        Sys::Items(items) => {
            let desk = &mut d.widgets.desk;
            let pics: &mut Pics = &mut desk.pics;
            desk.tray.items = items
                .into_iter()
                .map(|item| TrayItem {
                    icon: pics.get(&item.icon),
                    item,
                })
                .collect();
            sync(d);
        }
        Sys::Menu(m) => {
            if m.entries.is_empty() {
                d.toast("This tray item has an empty menu");
                return;
            }
            let t = &mut d.widgets.desk.tray;
            t.menu = Some(Menu {
                address: m.address,
                title: m.title,
                entries: m.entries,
                x: m.x,
                y: m.y,
                cursor: None,
            });
            t.menu_list.reset();
        }
    }
}

pub fn close_menu(d: &mut Dash) {
    let t = &mut d.widgets.desk.tray;
    t.menu = None;
    t.kbd_menu = false;
}

/// An entry of a menu: the cursor can stop on it and it can be pressed.
fn entry_ok(m: &Menu, i: Option<usize>) -> bool {
    i.and_then(|i| m.entries.get(i))
        .is_some_and(|e| !e.separator && e.enabled && !e.header)
}

/// The next entry the cursor can stop on in direction `d`, or where it is.
fn entry_step(m: &Menu, d: isize) -> Option<usize> {
    let from = m.cursor.map_or(-1, |c| c as isize);
    let mut i = from + d;
    for _ in 0..4 {
        let at = usize::try_from(i).ok().filter(|i| *i < m.entries.len());
        if at.is_some() && !entry_ok(m, at) {
            i += d;
        }
    }
    let at = usize::try_from(i).ok();
    if entry_ok(m, at) { at } else { m.cursor }
}

/// An icon was clicked at (`x`, `y`): a menu item shows its menu, anything else is activated,
/// and the dashboard gets out of the way.
pub fn activate(d: &mut Dash, address: &str, x: f32, y: f32, cx: &mut Context<Dash>) {
    let Some(item) = d
        .widgets
        .desk
        .tray
        .items
        .iter()
        .find(|i| i.item.address == address)
        .map(|i| i.item.clone())
    else {
        return;
    };
    if item.item_is_menu && item.has_menu {
        d.tx.tray.tell(Command::OpenMenu {
            address: item.address,
            x,
            y,
        });
    } else {
        d.tx.tray.tell(Command::Activate {
            address: item.address,
            x: x as i32,
            y: y as i32,
        });
        cx.emit(DashEvent::Hide);
    }
}

fn open_menu(d: &mut Dash, address: &str, x: f32, y: f32) {
    d.tx.tray.tell(Command::OpenMenu {
        address: address.into(),
        x,
        y,
    });
}

/// Entry `id` of the open menu was chosen: the app is told, the menu closes, the dashboard goes.
pub fn menu_activate(d: &mut Dash, id: i32, cx: &mut Context<Dash>) {
    let Some(address) = d.widgets.desk.tray.menu.as_ref().map(|m| m.address.clone()) else {
        return;
    };
    d.tx.tray.tell(Command::MenuItem { address, id });
    close_menu(d);
    cx.emit(DashEvent::Hide);
}

struct Keys<'a> {
    items: &'a [TrayItem],
    shown: usize,
    grid: Grid,
    /// The card's top left in the window.
    origin: (f32, f32),
    menu: &'a mut Option<Menu>,
    kbd: &'a mut bool,
    tx: &'a Senders,
    /// Set when the cursor should go to this icon (h/l).
    go: Option<usize>,
    hide: bool,
    activated: bool,
}

impl Keys<'_> {
    fn centre(&self, i: usize) -> (f32, f32) {
        let (x, y) = self.grid.centre(i);
        (self.origin.0 + x, self.origin.1 + y)
    }
}

impl Stops for Keys<'_> {
    fn adjust(&mut self, cursor: usize, delta: i32) -> bool {
        let to = (cursor as i32 + delta).clamp(0, self.shown.saturating_sub(1) as i32);
        self.go = Some(to as usize);
        true
    }

    fn activate(&mut self, i: usize, _sub: usize) {
        if i < self.shown {
            let (x, y) = self.centre(i);
            let item = &self.items[i].item;
            if item.item_is_menu && item.has_menu {
                self.tx.tray.tell(Command::OpenMenu {
                    address: item.address.clone(),
                    x,
                    y,
                });
            } else {
                self.tx.tray.tell(Command::Activate {
                    address: item.address.clone(),
                    x: x as i32,
                    y: y as i32,
                });
                self.hide = true;
            }
            self.activated = true;
        }
    }

    fn letter(&mut self, key: &Key, i: usize) -> bool {
        if key.is('m') && i < self.shown && self.items[i].item.has_menu {
            *self.kbd = true;
            if let Some(m) = self.menu.as_mut() {
                m.cursor = None;
            }
            let (x, y) = self.centre(i);
            self.tx.tray.tell(Command::OpenMenu {
                address: self.items[i].item.address.clone(),
                x,
                y,
            });
            return true;
        }
        false
    }

    fn filter(&mut self, key: &Key) -> bool {
        let Some(m) = self.menu.as_mut().filter(|_| *self.kbd) else {
            return false;
        };
        if key.is('j') || key.is_named(Named::Down) {
            m.cursor = entry_step(m, 1);
        } else if key.is('k') || key.is_named(Named::Up) {
            m.cursor = entry_step(m, -1);
        } else if key.is_named(Named::Enter) || key.is(' ') {
            if entry_ok(m, m.cursor)
                && let Some(c) = m.cursor
            {
                let (address, id) = (m.address.clone(), m.entries[c].id);
                self.tx.tray.tell(Command::MenuItem { address, id });
                *self.menu = None;
                *self.kbd = false;
                self.hide = true;
            }
        } else if key.is('q') || key.is_named(Named::Backspace) {
            *self.menu = None;
            *self.kbd = false;
        }
        true
    }
}

/// The icons that show in the card of widget `id`.
fn grid_of(d: &Dash, id: u32) -> Option<(Grid, (f32, f32), bool)> {
    let ((x, y, w, h), round) = shared::card_of(d, id)?;
    let pad = d.look.style.padding;
    let g = grid(w, h, round, (pad * 0.6).max(6.0), d.widgets.desk.tray.items.len());
    Some((g, (x, y), round))
}

fn settle(d: &mut Dash, id: u32) {
    let (shown, per_row) = grid_of(d, id).map_or((0, 1), |(g, _, _)| (g.shown, g.per_row));
    let mut scope = d.widgets.desk.tray.scopes.get(id);
    scope.stride = per_row;
    scope.set_count(shown, &NoSkip);
    d.widgets.desk.tray.scopes.put(id, scope);
}

struct NoSkip;
impl Stops for NoSkip {}

/// The icons changed: every cursor stays on an icon that exists.
pub fn sync(d: &mut Dash) {
    let ids: Vec<u32> = d.widgets.desk.tray.scopes.0.keys().copied().collect();
    for id in ids {
        settle(d, id);
    }
}

pub fn entered(d: &mut Dash, id: u32) {
    settle(d, id);
}

pub fn key(d: &mut Dash, id: u32, key: &Key, cx: &mut Context<Dash>) -> bool {
    settle(d, id);
    let Some((grid, origin, _)) = grid_of(d, id) else {
        return false;
    };
    let t = &mut d.widgets.desk.tray;
    let mut scope = t.scopes.get(id);
    let mut keys = Keys {
        items: &t.items,
        shown: grid.shown,
        grid,
        origin,
        menu: &mut t.menu,
        kbd: &mut t.kbd_menu,
        tx: &d.tx,
        go: None,
        hide: false,
        activated: false,
    };
    let taken = scope.press(key, &mut keys);
    let (go, hide) = (keys.go, keys.hide);
    if let Some(to) = go {
        scope.move_to(to, &NoSkip);
    }
    t.scopes.put(id, scope);
    if hide {
        cx.emit(DashEvent::Hide);
    }
    taken
}

/// Escape closes an open menu.
pub fn escape(d: &mut Dash) -> bool {
    if d.widgets.desk.tray.menu.is_some() {
        close_menu(d);
        true
    } else {
        false
    }
}

pub fn describe(d: &Dash, id: u32) -> String {
    let t = &d.widgets.desk.tray;
    let s = t.scopes.get(id);
    let mut text = match t.items.get(s.cursor) {
        Some(i) => format!("icon {}/{} {:?}", s.cursor + 1, t.items.len(), i.item.title),
        None => "no icons".to_string(),
    };
    if let Some(m) = &t.menu {
        text.push_str(&format!(
            ", menu of {} open{}",
            m.entries.len(),
            m.cursor.map_or(String::new(), |c| format!(" on entry {c}"))
        ));
    }
    text
}

pub fn check(d: &Dash, id: u32) -> Result<(), String> {
    let t = &d.widgets.desk.tray;
    let shown = grid_of(d, id).map_or(0, |(g, _, _)| g.shown);
    let s = shared::eff(t.scopes.get(id), shown);
    if shown > 0 && s.cursor >= t.items.len() {
        return Err(format!("cursor on tray icon {} of {}", s.cursor, t.items.len()));
    }
    Ok(())
}

/// What holds of the menu whatever keys came: its cursor is on an entry that can be pressed.
pub fn check_menu(d: &Dash) -> Result<(), String> {
    let t = &d.widgets.desk.tray;
    match &t.menu {
        Some(m) => {
            if m.entries.is_empty() {
                return Err("a tray menu without entries is open".into());
            }
            if let Some(c) = m.cursor
                && !entry_ok(m, Some(c))
            {
                return Err(format!("tray menu cursor on entry {c}, which can't be pressed"));
            }
            if m.cursor.is_some() && !t.kbd_menu {
                return Err("tray menu cursor in a menu the keyboard didn't open".into());
            }
        }
        None if t.kbd_menu => {
            // The keyboard asked for a menu that hasn't come (yet): fine. Nothing to check.
        }
        None => {}
    }
    Ok(())
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let tk = ctx.tk;
    let t = &ctx.d.widgets.desk.tray;
    let id = ctx.tile.id();
    let g = grid(ctx.w, ctx.h, ctx.round, tk.pad_tight(), t.items.len());
    let scope = shared::eff(t.scopes.get(id), g.shown);
    let on = ctx.entered;
    let mut root = div().id(("tray", id as usize)).size_full().relative().role(Role::Group);
    if t.items.is_empty() {
        root = root.child(
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(tk.font_body()))
                .text_color(tk.faint())
                .child(if ctx.round { "" } else { "No tray icons" }),
        );
    }
    for (i, it) in t.items.iter().enumerate().take(g.shown) {
        let left = g.inset + (i % g.per_row) as f32 * g.slot;
        let top = g.inset + (i / g.per_row) as f32 * g.slot;
        let kfocus = on && scope.cursor == i;
        let radius = tk.radius_small();
        let (hover, pressed) = (tk.hover(), tk.pressed());
        let address = it.item.address.clone();
        let (address_up, has_menu) = (address.clone(), it.item.has_menu);
        let label = if it.item.title.is_empty() {
            "Tray icon".to_string()
        } else {
            it.item.title.clone()
        };
        root = root.child(
            div()
                .id(("tray-item", id as usize * 1000 + i))
                .role(Role::Button)
                .aria_label(label.clone())
                .aria_cursor(kfocus)
                .debug_selector(move || label)
                .absolute()
                .left(px(left))
                .top(px(top))
                .size(px(g.slot))
                .rounded(px(radius))
                .bg(if kfocus { hover } else { gpui::transparent_black() })
                .hover(move |s| s.bg(hover))
                .active(move |s| s.bg(pressed))
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |d, ev: &MouseUpEvent, _, cx| {
                        activate(d, &address, f32::from(ev.position.x), f32::from(ev.position.y), cx);
                    }),
                )
                .on_mouse_up(
                    MouseButton::Right,
                    cx.listener(move |d, ev: &MouseUpEvent, _, _| {
                        if has_menu {
                            d.widgets.desk.tray.kbd_menu = false;
                            open_menu(d, &address_up, f32::from(ev.position.x), f32::from(ev.position.y));
                        }
                    }),
                )
                .child(match &it.icon {
                    Some(pic) => pic.square(g.slot * 0.62, false),
                    None => div()
                        .w(px(g.slot - 4.0))
                        .truncate()
                        .text_center()
                        .text_size(px(10.0))
                        .text_color(tk.dim())
                        .child(if it.item.title.is_empty() {
                            "?".to_string()
                        } else {
                            it.item.title.clone()
                        })
                        .into_any_element(),
                })
                .when(kfocus, |c| c.child(parts::cursor_ring(radius, tk))),
        );
    }
    root.into_any_element()
}

/// The menu, over the whole window, with a catcher behind it: a click outside closes it.
pub fn overlay(d: &Dash, tk: &Tk, cx: &mut Context<Dash>) -> Option<AnyElement> {
    let t = &d.widgets.desk.tray;
    let m = t.menu.as_ref()?;
    let (win_w, win_h) = d.size;
    let pw = 260.0f32;
    let title_h = if m.title.is_empty() { 0.0 } else { 26.0 };
    let content: f32 = m
        .entries
        .iter()
        .map(|e| if e.separator { 9.0 } else { 32.0 })
        .sum();
    let ph = (win_h - 40.0).min(title_h + content + 12.0).max(0.0);
    let x = m.x.clamp(10.0, (win_w - pw - 10.0).max(10.0));
    let y = if m.y + ph + 10.0 > win_h {
        (m.y - ph).max(10.0)
    } else {
        m.y
    };
    let radius = tk.radius_small() + 2.0;
    let rows: Vec<AnyElement> = m
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let live = !e.separator && e.enabled && !e.header;
            let here = m.cursor == Some(i);
            let (hover, eid) = (tk.hover(), e.id);
            let mut row = div()
                .id(("tray-menu-entry", i))
                .role(Role::ListItem)
                .aria_label(e.label.clone())
                .aria_cursor(here)
                .relative()
                .flex_none()
                .w_full()
                .h(px(if e.separator { 9.0 } else { 32.0 }))
                .rounded(px(tk.radius_small()));
            if e.separator {
                return row
                    .flex()
                    .items_center()
                    .px(px(8.0))
                    .child(div().w_full().h(px(1.0)).bg(tk.border()))
                    .into_any_element();
            }
            let label = e.label.clone();
            row = row.debug_selector(move || label);
            if live {
                row = row
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |d, _, _, cx| {
                        cx.stop_propagation();
                        menu_activate(d, eid, cx);
                    }));
                if here {
                    row = row.bg(hover);
                }
            }
            row.flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .pl(px(10.0 + e.depth as f32 * 14.0))
                .pr(px(10.0))
                .child(
                    div()
                        .flex_none()
                        .w(px(16.0))
                        .when(e.toggle != Toggle::None && e.checked, |c| {
                            c.child(crate::kit::icon("check", 14.0, tk.accent()))
                        }),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(tk.font_body()))
                        .font_weight(if e.header {
                            gpui::FontWeight::SEMIBOLD
                        } else {
                            gpui::FontWeight::NORMAL
                        })
                        .text_color(if e.header {
                            tk.dim()
                        } else if e.enabled {
                            tk.text()
                        } else {
                            tk.faint()
                        })
                        .child(e.label.clone()),
                )
                .when(here, |r| r.child(parts::cursor_ring(tk.radius_small(), tk)))
                .into_any_element()
        })
        .collect();
    let cursor = m.cursor;
    let panel = div()
        .id("tray-menu")
        .role(Role::List)
        .aria_label("Tray menu")
        .debug_selector(|| "Tray menu".into())
        .absolute()
        .left(px(x))
        .top(px(y))
        .w(px(pw))
        .h(px(ph))
        .rounded(px(radius))
        .bg(tk.panel())
        .border_1()
        .border_color(tk.border())
        .shadow(vec![gpui::BoxShadow {
            color: tk.shadow(),
            offset: point(px(0.0), px(8.0)),
            blur_radius: px(24.0),
            spread_radius: px(0.0),
            inset: false,
        }])
        .occlude()
        .p(px(6.0))
        .flex()
        .flex_col()
        .when(!m.title.is_empty(), |p| {
            p.child(
                div()
                    .flex_none()
                    .h(px(26.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .child(parts::section_label(m.title.clone(), tk)),
            )
        })
        .child(t.menu_list.view(
            "tray-menu-list",
            cursor,
            rows,
            super::list::fingerprint(win_w, win_h, tk),
            tk,
            cx,
        ));
    Some(
        div()
            .id("tray-menu-catcher")
            .absolute()
            .size_full()
            .occlude()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|d, _, _, cx| {
                close_menu(d);
                cx.notify();
            }))
            .child(panel)
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(label: &str, separator: bool, enabled: bool, header: bool) -> Entry {
        Entry {
            id: 0,
            label: label.into(),
            depth: 0,
            separator,
            enabled,
            toggle: Toggle::None,
            checked: false,
            header,
        }
    }

    fn menu(entries: Vec<Entry>, cursor: Option<usize>) -> Menu {
        Menu {
            address: "a".into(),
            title: String::new(),
            entries,
            x: 0.0,
            y: 0.0,
            cursor,
        }
    }

    #[test]
    fn the_menu_cursor_skips_what_cannot_be_pressed() {
        let m = menu(
            vec![
                entry("Open", false, true, false),
                entry("", true, true, false),
                entry("Greyed", false, false, false),
                entry("Section", false, true, true),
                entry("Quit", false, true, false),
            ],
            None,
        );
        assert_eq!(entry_step(&m, 1), Some(0));
        let m = Menu { cursor: Some(0), ..m };
        assert_eq!(entry_step(&m, 1), Some(4), "past the separator, the greyed entry and the heading");
        let m = Menu { cursor: Some(4), ..m };
        assert_eq!(entry_step(&m, -1), Some(0));
        assert_eq!(entry_step(&m, 1), Some(4), "stays at the end");
        let m = Menu { cursor: Some(0), ..m };
        assert_eq!(entry_step(&m, -1), Some(0), "stays at the start");
    }

    #[test]
    fn icons_fill_rows_of_slots() {
        let g = grid(300.0, 100.0, false, 10.0, 12);
        assert_eq!(g.slot, 44.0);
        assert_eq!(g.per_row, 6);
        assert_eq!(g.shown, 12, "two rows of six");
        let tiny = grid(30.0, 30.0, false, 10.0, 3);
        assert_eq!(tiny.per_row, 1);
        let round = grid(100.0, 100.0, true, 10.0, 9);
        assert!(round.per_row >= 1);
    }
}

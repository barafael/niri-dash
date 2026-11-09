//! Desk: windows, notifications, tray (and its menu), media, Claude sessions.
//!
//! The group's hooks, called by `widgets/mod.rs` for every group in turn; see PORTING.md. Put the
//! widgets in submodules (`widgets/desk/<kind>.rs`), and the group's state in `State`.

use super::Ctx;
use crate::Control;
use crate::dash::{Dash, NavMode};
use crate::keys::Key;
use crate::kit::Tk;
use crate::layout::Kind;
use gpui::{AnyElement, Context};
use niri_dash_system::Event;
use niri_dash_system::{claude as sys_claude, media as sys_media, notifications as sys_notifications};
use niri_dash_system::{tray as sys_tray, windows as sys_windows};

mod claude;
mod list;
mod media;
mod notifications;
mod parts;
mod pics;
mod shared;
mod tray;
mod windows;

/// Everything the group's widgets keep that isn't the layout or the look. Several instances of a
/// kind share it (two clocks show the same time).
#[derive(Default)]
pub struct State {
    /// Icons, cover art and notification pictures, by path.
    pics: pics::Pics,
    pub windows: windows::State,
    pub notifications: notifications::State,
    pub tray: tray::State,
    pub media: media::State,
    pub claude: claude::State,
}

/// The kinds of widget this group draws.
pub fn handles(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Windows | Kind::Notifications | Kind::Tray | Kind::Media | Kind::ClaudeSessions
    )
}

/// What the widget of `ctx.tile` draws, in a card of `ctx.w` by `ctx.h` pixels.
pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    match ctx.kind() {
        Kind::Windows => windows::render(ctx, cx),
        Kind::Notifications => notifications::render(ctx, cx),
        Kind::Tray => tray::render(ctx, cx),
        Kind::Media => media::render(ctx, cx),
        _ => claude::render(ctx, cx),
    }
}

/// What the system reports. `None`: not this group's; `Some(control)`: taken, and the event asks
/// for a control (a color was picked, and the dashboard may show itself again) or not.
pub fn event(d: &mut Dash, event: &Event) -> Option<Option<Control>> {
    match event {
        Event::Windows(sys_windows::Event::State(s)) => windows::show(d, s.clone()),
        Event::Claude(sys_claude::Event::Sessions(s)) => claude::show(d, s.clone()),
        Event::Notifications(sys_notifications::Event::State(s)) => {
            notifications::show(d, s.clone())
        }
        Event::Tray(e) => match e {
            sys_tray::Event::Items(items) => tray::event(d, sys_tray::Event::Items(items.clone())),
            sys_tray::Event::Menu(m) => tray::event(d, sys_tray::Event::Menu(m.clone())),
        },
        Event::Media(sys_media::Event::State(m)) => media::show(d, m.clone()),
        _ => return None,
    }
    Some(None)
}

/// A key for the entered widget `id` of `kind` (one of this group's). True if it took it.
pub fn key(d: &mut Dash, kind: Kind, id: u32, key: &Key, cx: &mut Context<Dash>) -> bool {
    match kind {
        Kind::Windows => windows::key(d, id, key, cx),
        Kind::Notifications => notifications::key(d, id, key, cx),
        Kind::Tray => tray::key(d, id, key, cx),
        Kind::Media => media::key(d, key),
        Kind::ClaudeSessions => claude::key(d, id, key, cx),
        _ => false,
    }
}

/// Escape, before the dashboard takes it: true if something of the group's closed (a menu).
pub fn escape(d: &mut Dash, _cx: &mut Context<Dash>) -> bool {
    // The notifications' pause choices first (they are the entered widget's own), then a menu.
    if d.nav.mode == NavMode::Widget {
        let id = d.nav.active_id;
        if d.tile(id).is_some_and(|t| t.kind() == Kind::Notifications)
            && notifications::escape(d, id)
        {
            return true;
        }
    }
    tray::escape(d)
}

/// A second passed while the dashboard is up.
pub fn tick(d: &mut Dash) {
    notifications::tick(d);
}

/// The dashboard opens: every widget back on its first page.
pub fn come_up(d: &mut Dash) {
    let desk = &mut d.widgets.desk;
    desk.windows.scopes.reset();
    desk.windows.lists.reset();
    desk.claude.scopes.reset();
    desk.claude.lists.reset();
    desk.notifications.insts.clear();
    desk.notifications.lists.reset();
    desk.tray.scopes.reset();
}

/// The dashboard goes away: menus shut.
pub fn go_down(d: &mut Dash) {
    tray::close_menu(d);
}

/// The dashboard came up (`true`) or went away: notes, timers and mascots only run while seen.
pub fn awake(_d: &mut Dash, _on: bool) {}

/// Widget `id` of `kind` was entered with the keyboard.
pub fn entered(d: &mut Dash, kind: Kind, id: u32) {
    match kind {
        Kind::Windows => windows::entered(d, id),
        Kind::Notifications => notifications::entered(d, id),
        Kind::Tray => tray::entered(d, id),
        Kind::ClaudeSessions => claude::entered(d, id),
        _ => {}
    }
}

/// The tiles changed (a widget added, removed, moved): what is kept per instance follows.
pub fn layout_changed(d: &mut Dash) {
    let tiles = d.tiles.clone();
    let desk = &mut d.widgets.desk;
    desk.windows.scopes.keep(&tiles);
    desk.claude.scopes.keep(&tiles);
    desk.tray.scopes.keep(&tiles);
    desk.notifications
        .insts
        .retain(|id, _| tiles.iter().any(|t| t.id() == *id));
    windows::sync(d);
    claude::sync(d);
    tray::sync(d);
    notifications::sync(d);
}

/// What the entered widget would say about where its cursor is, for the stress run's log.
pub fn describe(d: &Dash, kind: Kind) -> Option<String> {
    let id = d.nav.active_id;
    Some(match kind {
        Kind::Windows => windows::describe(d, id),
        Kind::Notifications => notifications::describe(d, id),
        Kind::Tray => tray::describe(d, id),
        Kind::Media => media::describe(d),
        Kind::ClaudeSessions => claude::describe(d, id),
        _ => return None,
    })
}

/// What has to hold of the group's state whatever keys came before: the stress run checks it
/// after every key. An error says what doesn't add up.
pub fn check(d: &Dash) -> Result<(), String> {
    tray::check_menu(d)?;
    for tile in &d.tiles {
        let id = tile.id();
        let entered = d.nav.mode == NavMode::Widget && d.nav.active_id == id;
        match tile.kind() {
            Kind::Windows => windows::check(d, id, entered),
            Kind::Notifications => notifications::check(d, id, entered),
            Kind::Tray => tray::check(d, id),
            Kind::ClaudeSessions => claude::check(d, id, entered),
            _ => Ok(()),
        }
        .map_err(|e| format!("{}: {e}", tile.kind().name()))?;
    }
    Ok(())
}

/// The dashboard is built (`d.state` is what was saved): what the group restores.
pub fn init(d: &mut Dash) {
    notifications::init(d);
}

/// Do not disturb on or off: `niri-dash dnd`, the stress run's toggle.
pub fn toggle_dnd(d: &mut Dash) {
    notifications::toggle_dnd(d);
}

/// What the group draws over the whole window, above the frames: the tray's menu, with the
/// catcher behind it that closes it when something else is clicked.
pub fn overlay(d: &Dash, tk: &Tk, cx: &mut Context<Dash>) -> Vec<AnyElement> {
    tray::overlay(d, tk, cx).into_iter().collect()
}

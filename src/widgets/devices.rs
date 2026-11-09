//! Devices: privacy, removable drives, phone.
//!
//! The group's hooks, called by `widgets/mod.rs` for every group in turn; see PORTING.md. Put the
//! widgets in submodules (`widgets/devices/<kind>.rs`), and the group's state in `State`.

use super::Ctx;
use crate::Control;
use crate::dash::{Dash, NavMode};
use crate::keys::Key;
use crate::layout::Kind;
use gpui::{AnyElement, Context};
use niri_dash_system as sys;
use niri_dash_system::Event;

mod drives;
pub(crate) mod parts;
mod phone;
mod privacy;

/// Everything the group's widgets keep that isn't the layout or the look. Several instances of a
/// kind share it (two privacy widgets show the same apps, and have one keyboard cursor).
#[derive(Default)]
pub struct State {
    pub privacy: privacy::State,
    pub drives: drives::State,
    pub phone: phone::State,
    /// How each widget was last drawn: what its keys and checks need to know.
    seen: parts::Seen,
}

/// The kinds of widget this group draws.
pub fn handles(kind: Kind) -> bool {
    matches!(kind, Kind::Privacy | Kind::RemovableDrives | Kind::Phone)
}

/// What the widget of `ctx.tile` draws, in a card of `ctx.w` by `ctx.h` pixels.
pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    ctx.d
        .widgets
        .devices
        .seen
        .note(ctx.tile.id(), parts::Card::of(ctx));
    let widget = match ctx.kind() {
        Kind::Privacy => privacy::render(ctx, cx),
        Kind::RemovableDrives => drives::render(ctx, cx),
        _ => phone::render(ctx, cx),
    };
    crate::kit::slint_lines(widget)
}

/// The tile of `kind` the keys are on, else the first one there is.
fn tile_of(d: &Dash, kind: Kind) -> Option<u32> {
    let entered = (d.nav.mode == NavMode::Widget)
        .then(|| d.tile(d.nav.active_id))
        .flatten()
        .filter(|t| t.kind() == kind);
    entered
        .or_else(|| d.tiles.iter().find(|t| t.kind() == kind))
        .map(|t| t.id())
}

/// What the widgets show changed (or what is entered did): each cursor is on a stop that exists.
fn resync(d: &mut Dash) {
    let card = |d: &Dash, kind| {
        tile_of(d, kind).map_or_else(Default::default, |id| d.widgets.devices.seen.of(id))
    };
    let c = card(d, Kind::Privacy);
    d.widgets.devices.privacy.sync(c);
    let c = card(d, Kind::RemovableDrives);
    d.widgets.devices.drives.sync(c);
    let c = card(d, Kind::Phone);
    d.widgets.devices.phone.sync(c);
}

/// What the system reports. `None`: not this group's; `Some(control)`: taken, and the event asks
/// for a control (a color was picked, and the dashboard may show itself again) or not.
pub fn event(d: &mut Dash, event: &Event) -> Option<Option<Control>> {
    match event {
        Event::Privacy(sys::privacy::Event::Casts(casts)) => privacy::set_casts(d, casts.clone()),
        Event::Privacy(sys::privacy::Event::Usage(usage)) => privacy::set_usage(d, usage.clone()),
        Event::Drives(sys::drives::Event::State(s)) => drives::show(d, s.clone()),
        Event::Phones(sys::phones::Event::State(s)) => phone::show(d, s.clone()),
        _ => return None,
    }
    Some(None)
}

/// A key for the entered widget `id` of `kind` (one of this group's). True if it took it.
pub fn key(d: &mut Dash, kind: Kind, id: u32, key: &Key, cx: &mut Context<Dash>) -> bool {
    match kind {
        Kind::Privacy => privacy::key(d, id, key, cx),
        Kind::RemovableDrives => drives::key(d, id, key, cx),
        _ => phone::key(d, id, key, cx),
    }
}

/// Escape, before the dashboard takes it: true if something of the group's closed (a menu).
pub fn escape(_d: &mut Dash, _cx: &mut Context<Dash>) -> bool {
    false
}

/// A second passed while the dashboard is up.
pub fn tick(_d: &mut Dash) {}

/// The dashboard opens: every widget back on its first page.
pub fn come_up(_d: &mut Dash) {}

/// The dashboard goes away: menus shut.
pub fn go_down(_d: &mut Dash) {}

/// The dashboard came up (`true`) or went away: notes, timers and mascots only run while seen.
pub fn awake(_d: &mut Dash, _on: bool) {}

/// Widget `id` of `kind` was entered with the keyboard.
pub fn entered(d: &mut Dash, kind: Kind, _id: u32) {
    if handles(kind) {
        resync(d);
    }
}

/// The tiles changed (a widget added, removed, moved): what is kept per instance follows.
pub fn layout_changed(d: &mut Dash) {
    resync(d);
}

/// What the entered widget would say about where its cursor is, for the stress run's log.
pub fn describe(d: &Dash, kind: Kind) -> Option<String> {
    match kind {
        Kind::Privacy => privacy::describe(d),
        Kind::RemovableDrives => drives::describe(d),
        Kind::Phone => phone::describe(d),
        _ => None,
    }
}

/// What has to hold of the group's state whatever keys came before: the stress run checks it
/// after every key. An error says what doesn't add up.
pub fn check(d: &Dash) -> Result<(), String> {
    privacy::check(d)?;
    drives::check(d)?;
    phone::check(d)
}

/// The dashboard is built (`d.state` is what was saved): what the group restores.
pub fn init(_d: &mut Dash) {}

//! Extras: the mascots and the about card.
//!
//! The group's hooks, called by `widgets/mod.rs` for every group in turn; see PORTING.md. Put the
//! widgets in submodules (`widgets/extras/<kind>.rs`), and the group's state in `State`.

use super::{Ctx, placeholder};
use crate::Control;
use crate::dash::Dash;
use crate::keys::Key;
use crate::layout::Kind;
use gpui::{AnyElement, Context};
use niri_dash_system::Event;

/// Everything the group's widgets keep that isn't the layout or the look. Several instances of a
/// kind share it (two clocks show the same time).
#[derive(Default)]
pub struct State {}

/// The kinds of widget this group draws.
pub fn handles(kind: Kind) -> bool {
    matches!(kind, Kind::Mascot | Kind::About)
}

/// What the widget of `ctx.tile` draws, in a card of `ctx.w` by `ctx.h` pixels.
pub fn render(ctx: &Ctx, _cx: &mut Context<Dash>) -> AnyElement {
    placeholder(ctx, ctx.kind())
}

/// What the system reports. `None`: not this group's; `Some(control)`: taken, and the event asks
/// for a control (a color was picked, and the dashboard may show itself again) or not.
pub fn event(_d: &mut Dash, _event: &Event) -> Option<Option<Control>> {
    None
}

/// A key for the entered widget `id` of `kind` (one of this group's). True if it took it.
pub fn key(_d: &mut Dash, _kind: Kind, _id: u32, _key: &Key, _cx: &mut Context<Dash>) -> bool {
    false
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
pub fn entered(_d: &mut Dash, _kind: Kind, _id: u32) {}

/// The tiles changed (a widget added, removed, moved): what is kept per instance follows.
pub fn layout_changed(_d: &mut Dash) {}

/// What the entered widget would say about where its cursor is, for the stress run's log.
pub fn describe(_d: &Dash, _kind: Kind) -> Option<String> {
    None
}

/// What has to hold of the group's state whatever keys came before: the stress run checks it
/// after every key. An error says what doesn't add up.
pub fn check(_d: &Dash) -> Result<(), String> {
    Ok(())
}

/// The dashboard is built (`d.state` is what was saved): what the group restores.
pub fn init(_d: &mut Dash) {}

//! Tools: keyboard layout, color picker, notes, clipboard history.
//!
//! The group's hooks, called by `widgets/mod.rs` for every group in turn; see PORTING.md. Each
//! widget is a submodule (`tools/<kind>.rs`) with its data in a `State` here.

use super::Ctx;
use crate::Control;
use crate::dash::Dash;
use crate::keys::Key;
use crate::layout::Kind;
use crate::scope::{Scope, Stops};
use gpui::{AnyElement, Context};
use niri_dash_system::Event;
use std::cell::RefCell;
use std::collections::HashMap;

mod clipboard;
mod color;
mod keyboard;
mod notes;
mod ui;

/// Put text on the clipboard and say so in a toast: for the other groups' "copy" buttons.
#[allow(unused_imports)]
pub use clipboard::copy_text;

/// Everything the group's widgets keep that isn't the layout or the look. Several instances of a
/// kind share the data; each instance has its own keyboard cursor ([`Insts`]).
#[derive(Default)]
pub struct State {
    keyboard: keyboard::State,
    color: color::State,
    notes: notes::State,
    clipboard: clipboard::State,
    insts: Insts,
}

impl State {
    /// How many entries the clipboard history holds.
    pub fn clipboard_len(&self) -> usize {
        self.clipboard.entries.len()
    }

    /// The history's entries as text (images say "Image"), newest first.
    pub fn clipboard_texts(&self) -> Vec<String> {
        self.clipboard
            .entries
            .iter()
            .map(|e| e.text.clone())
            .collect()
    }

    /// Whether what is copied is being kept (there is a clipboard widget).
    pub fn clipboard_recording(&self) -> bool {
        self.clipboard.recording
    }

    /// The text of notes widget `id`.
    pub fn note_text(&self, id: u32) -> String {
        self.notes.text_of(id)
    }

    /// What has to hold of the group's state (the stress run's check).
    pub fn check(&self, d: &Dash) -> Result<(), String> {
        check(d)
    }

    /// The keyboard cursor of widget `id`: the stop and the part of it.
    pub fn cursor(&self, id: u32) -> (usize, usize) {
        let s = self.insts.get(id).scope;
        (s.cursor, s.part())
    }
}

/// What a widget instance keeps for the keyboard: its cursor, and the shape it was last drawn in
/// (which decides how many stops there are, as the Slint widgets' properties did).
#[derive(Clone, Copy, Default)]
struct Inst {
    scope: Scope,
    w: f32,
    h: f32,
    round: bool,
}

/// [`Inst`] of every widget instance of the group, by widget id. Drawing records the shape
/// through a shared reference, hence the cell.
#[derive(Default)]
struct Insts(RefCell<HashMap<u32, Inst>>);

impl Insts {
    fn get(&self, id: u32) -> Inst {
        self.0.borrow().get(&id).copied().unwrap_or_default()
    }

    fn set(&self, id: u32, inst: Inst) {
        self.0.borrow_mut().insert(id, inst);
    }

    /// The widget was drawn `w` by `h` px, with `count` stops and `subs` parts to a stop.
    fn drawn(&self, id: u32, w: f32, h: f32, round: bool, count: usize, subs: usize) {
        let mut all = self.0.borrow_mut();
        let inst = all.entry(id).or_default();
        inst.w = w;
        inst.h = h;
        inst.round = round;
        inst.scope.set_count(count, &Plain);
        inst.scope.set_subs(subs);
    }

    fn retain(&self, keep: impl Fn(u32) -> bool) {
        self.0.borrow_mut().retain(|id, _| keep(*id));
    }
}

/// Stops that are all alike: nothing is skipped.
struct Plain;

impl Stops for Plain {}

/// The kinds of widget this group draws.
pub fn handles(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::KeyboardLayout | Kind::ColorPicker | Kind::Notes | Kind::Clipboard
    )
}

/// What the widget of `ctx.tile` draws, in a card of `ctx.w` by `ctx.h` pixels.
pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    match ctx.kind() {
        Kind::KeyboardLayout => keyboard::render(ctx, cx),
        Kind::ColorPicker => color::render(ctx, cx),
        Kind::Notes => notes::render(ctx, cx),
        Kind::Clipboard => clipboard::render(ctx, cx),
        kind => super::placeholder(ctx, kind),
    }
}

/// What the system reports. `None`: not this group's; `Some(control)`: taken, and the event asks
/// for a control (a color was picked, and the dashboard may show itself again) or not.
pub fn event(d: &mut Dash, event: &Event) -> Option<Option<Control>> {
    use niri_dash_system::{
        clipboard as sys_clipboard, color as sys_color, keyboard as sys_keyboard,
    };
    match event {
        Event::Keyboard(sys_keyboard::Event::Layouts(l)) => {
            keyboard::layouts(d, l.clone());
            Some(None)
        }
        Event::Clipboard(sys_clipboard::Event::Copied(clip)) => {
            clipboard::add(d, clip.clone());
            Some(None)
        }
        Event::Color(sys_color::Event::Picked(rgb)) => Some(color::picked(d, *rgb)),
        _ => None,
    }
}

/// A key for the entered widget `id` of `kind` (one of this group's). True if it took it.
pub fn key(d: &mut Dash, kind: Kind, id: u32, key: &Key, cx: &mut Context<Dash>) -> bool {
    match kind {
        Kind::KeyboardLayout => keyboard::key(d, id, key),
        Kind::ColorPicker => color::key(d, id, key, cx),
        Kind::Clipboard => clipboard::key(d, id, key),
        // The text area has the keys.
        _ => false,
    }
}

/// Escape, before the dashboard takes it: true if something of the group's closed (a menu).
pub fn escape(_d: &mut Dash, _cx: &mut Context<Dash>) -> bool {
    false
}

/// A second passed while the dashboard is up.
pub fn tick(d: &mut Dash) {
    notes::tick(d);
}

/// The dashboard opens: every widget back on its first page.
pub fn come_up(_d: &mut Dash) {}

/// The dashboard goes away: menus shut.
pub fn go_down(_d: &mut Dash) {}

/// The dashboard came up (`true`) or went away: notes, timers and mascots only run while seen.
pub fn awake(d: &mut Dash, on: bool) {
    notes::awake(d, on);
    color::awake(d, on);
}

/// Widget `id` of `kind` was entered with the keyboard.
pub fn entered(d: &mut Dash, kind: Kind, id: u32) {
    match kind {
        Kind::KeyboardLayout => keyboard::entered(d, id),
        Kind::Notes => notes::entered(d, id),
        _ => {}
    }
}

/// The tiles changed (a widget added, removed, moved): what is kept per instance follows.
pub fn layout_changed(d: &mut Dash) {
    let ids: Vec<u32> = d.tiles.iter().map(|t| t.id()).collect();
    d.widgets.tools.insts.retain(|id| ids.contains(&id));
    notes::sync(d, false);
    clipboard::sync_history(d);
}

/// What the entered widget would say about where its cursor is, for the stress run's log.
pub fn describe(d: &Dash, kind: Kind) -> Option<String> {
    let id = d.nav.active_id;
    match kind {
        Kind::KeyboardLayout => keyboard::describe(d, id),
        Kind::ColorPicker => color::describe(d, id),
        Kind::Clipboard => clipboard::describe(d, id),
        Kind::Notes => Some("typing".into()),
        _ => None,
    }
}

/// What has to hold of the group's state whatever keys came before: the stress run checks it
/// after every key. An error says what doesn't add up.
pub fn check(d: &Dash) -> Result<(), String> {
    keyboard::check(d)?;
    color::check(d)?;
    clipboard::check(d)?;
    notes::check(d)
}

/// The dashboard is built (`d.state` is what was saved): what the group restores.
pub fn init(d: &mut Dash) {
    clipboard::sync_history(d);
    notes::sync(d, false);
}

/// The widgets of `kind` on show: a standalone app's own, or the visible ones on the dashboard.
fn placed_of(d: &Dash, kind: Kind) -> Vec<crate::layout::Placed> {
    match d.solo {
        Some(k) if k == kind => vec![d.layout.solo_widget(kind)],
        Some(_) => Vec::new(),
        None => d
            .tiles
            .iter()
            .filter(|t| t.kind() == kind)
            .map(|t| t.placed.clone())
            .collect(),
    }
}

/// Check that widget `id`'s cursor is on a stop that exists: `count` stops, `subs` parts.
fn cursor_ok(what: &str, d: &Dash, id: u32, count: usize, subs: usize) -> Result<(), String> {
    let s = d.widgets.tools.insts.get(id).scope;
    if s.cursor >= count.max(1) {
        return Err(format!(
            "{what}: the cursor is on stop {} of {count}",
            s.cursor
        ));
    }
    if s.sub >= subs.max(1) && s.subs != subs {
        return Err(format!("{what}: the cursor is on part {} of {subs}", s.sub));
    }
    Ok(())
}

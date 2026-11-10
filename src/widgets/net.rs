//! Network: Wi-Fi and Bluetooth.
//!
//! The group's hooks, called by `widgets/mod.rs` for every group in turn; see PORTING.md. The
//! widgets are in `net/wifi.rs` and `net/bluetooth.rs`; both are made of `net/ui.rs` (rows,
//! buttons, switches), `net/scroll.rs` (a list that follows the keyboard cursor) and
//! `net/field.rs` (text fields: passwords, names, pairing codes).
//!
//! A widget's data is what its system actor last reported (`wifi::State`, `bluetooth::State`),
//! kept whole; what a widget draws and what its keys do are worked out from it when they are
//! needed. Each widget on the dashboard has a `View`: its page, its keyboard cursor, its text
//! fields. The cursor is a `scope::Scope` over the page's stops, which `L` (where the stops are)
//! lists for each page; a prompt has stops and a cursor of its own (`P`).
//!
//! Keyboard and mouse do the same things: both are an `Act`.

use super::{Ctx, placeholder};
use crate::Control;
use crate::dash::{Dash, NavMode, Tile};
use crate::keys::Key;
use crate::layout::Kind;
use gpui::{AnyElement, Context, KeyDownEvent, Window};
use niri_dash_system::Event;

mod bluetooth;
mod field;
mod scroll;
mod ui;
mod wifi;

/// Everything the group's widgets keep that isn't the layout or the look. Several instances of a
/// kind share the data (two Wi-Fi widgets show the same networks) but not the page and cursor.
#[derive(Default)]
pub struct State {
    pub wifi: wifi::Data,
    pub bluetooth: bluetooth::Data,
    /// A text field that may have had the keyboard is gone: the next frame gives it back to the
    /// dashboard.
    refocus: std::cell::Cell<bool>,
}

impl State {
    /// What a screen reader would say about the keyboard cursor of the entered Wi-Fi or
    /// Bluetooth widget (`listitem "Lighthouse" connected, signal 78%, secured`), `None` while
    /// there is none to say.
    pub fn cursor_says(&self, d: &Dash) -> Option<String> {
        if d.nav.mode != NavMode::Widget {
            return None;
        }
        describe(d, d.tile(d.nav.active_id)?.kind())
    }
}

/// A Wi-Fi or Bluetooth widget, by its tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wid {
    Wifi(u32),
    Bt(u32),
}

/// What a screen reader says of one thing: its role, its label, how it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    pub role: &'static str,
    pub label: String,
    pub value: String,
    pub checked: Option<bool>,
}

impl std::fmt::Display for Said {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {:?}", self.role, self.label)?;
        if !self.value.is_empty() {
            write!(f, " {}", self.value)?;
        }
        match self.checked {
            Some(true) => write!(f, " (on)"),
            Some(false) => write!(f, " (off)"),
            None => Ok(()),
        }
    }
}

/// Note which text field has the keyboard.
fn set_typing(d: &mut Dash, w: Wid, name: Option<&'static str>) {
    match w {
        Wid::Wifi(id) => {
            if let Some(v) = wifi::view(d, id) {
                v.typing.set(name);
            }
        }
        Wid::Bt(id) => {
            if let Some(v) = bluetooth::view(d, id) {
                v.typing.set(name);
            }
        }
    }
}

fn pool(d: &Dash, w: Wid) -> Option<field::Pool> {
    match w {
        Wid::Wifi(id) => wifi::view(d, id)?.pool.clone(),
        Wid::Bt(id) => bluetooth::view(d, id)?.pool.clone(),
    }
}

fn id_of(w: Wid) -> u32 {
    match w {
        Wid::Wifi(id) | Wid::Bt(id) => id,
    }
}

/// The cursor goes to the text field's stop.
fn move_cursor_to(d: &mut Dash, w: Wid, name: &str) {
    match w {
        Wid::Wifi(id) => wifi::move_cursor_to(d, id, name),
        Wid::Bt(id) => bluetooth::move_cursor_to(d, id, name),
    }
}

/// A key that reached a text field's frame (see `field.rs` for which do).
fn field_key(
    d: &mut Dash,
    w: Wid,
    name: &'static str,
    ev: &KeyDownEvent,
    window: &mut Window,
    cx: &mut Context<Dash>,
) {
    use field::Pressed;
    let Some(pool) = pool(d, w) else {
        return;
    };
    match Pressed::of(ev) {
        Pressed::Text => {
            if let Some(text) = field::typed(ev) {
                // A passkey is a number.
                let digits = matches!(w, Wid::Bt(_)) && name == "code" && bluetooth::digits_only(d);
                if !digits || text.chars().all(|c| c.is_ascii_digit()) {
                    field::insert(&pool, name, &text, window, cx);
                }
            }
            // Not the dashboard's keys: `f` is a letter here.
            cx.stop_propagation();
        }
        Pressed::Tab { back } => {
            let order = match w {
                Wid::Wifi(id) => wifi::tab_order(d, id, name),
                Wid::Bt(_) => bluetooth::tab_order(name),
            };
            if let Some(at) = order.iter().position(|n| *n == name) {
                let n = order.len() as isize;
                let to = order[(at as isize + if back { -1 } else { 1 }).rem_euclid(n) as usize];
                pool.focus(to, window, cx);
                set_typing(d, w, Some(to));
                move_cursor_to(d, w, to);
            }
            cx.stop_propagation();
        }
        Pressed::Escape => {
            // The keyboard goes back to the cursor; only an entered widget takes the key.
            set_typing(d, w, None);
            field::give_back(d, window, cx);
            let entered = d.nav.mode == NavMode::Widget && d.nav.active_id == id_of(w);
            if entered {
                cx.stop_propagation();
            }
        }
        Pressed::Other => {}
    }
    cx.notify();
}

/// Before a widget draws: if a text field that had the keyboard went away, the dashboard gets it
/// back (the keys go to whoever has it, and to nobody when that is a field that isn't there).
fn refocus(d: &Dash, cx: &mut Context<Dash>) {
    if d.widgets.net.refocus.replace(false) {
        field::later(cx, |d, window, cx| field::give_back(d, window, cx));
    }
}

/// The kinds of widget this group draws.
pub fn handles(kind: Kind) -> bool {
    matches!(kind, Kind::Wifi | Kind::Bluetooth)
}

/// What the widget of `ctx.tile` draws, in a card of `ctx.w` by `ctx.h` pixels.
pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    refocus(ctx.d, cx);
    let inner = match ctx.kind() {
        Kind::Wifi => wifi::render(ctx, cx),
        Kind::Bluetooth => bluetooth::render(ctx, cx),
        kind => return placeholder(ctx, kind),
    };
    ui::lined(inner)
}

/// What the system reports. `None`: not this group's; `Some(control)`: taken, and the event asks
/// for a control (a color was picked, and the dashboard may show itself again) or not.
pub fn event(d: &mut Dash, event: &Event) -> Option<Option<Control>> {
    match event {
        Event::Wifi(niri_dash_system::wifi::Event::State(s)) => {
            wifi::set(d, (**s).clone());
            Some(None)
        }
        Event::Bluetooth(niri_dash_system::bluetooth::Event::State(s)) => {
            bluetooth::set(d, (**s).clone());
            Some(None)
        }
        _ => None,
    }
}

/// A key for the entered widget `id` of `kind` (one of this group's). True if it took it.
pub fn key(d: &mut Dash, kind: Kind, id: u32, key: &Key, cx: &mut Context<Dash>) -> bool {
    match kind {
        Kind::Wifi => wifi::key(d, id, key, cx),
        Kind::Bluetooth => bluetooth::key(d, id, key, cx),
        _ => false,
    }
}

/// Escape, before the dashboard takes it: true if something of the group's closed (a prompt, a
/// page).
pub fn escape(d: &mut Dash, cx: &mut Context<Dash>) -> bool {
    if d.nav.mode != NavMode::Widget {
        return false;
    }
    let id = d.nav.active_id;
    match d.tile(id).map(Tile::kind) {
        Some(Kind::Wifi) => wifi::escape(d, id, cx),
        Some(Kind::Bluetooth) => bluetooth::escape(d, id, cx),
        _ => false,
    }
}

/// A second passed while the dashboard is up.
pub fn tick(_d: &mut Dash) {}

/// Every widget back on its first page, with nothing typed.
fn reset(d: &mut Dash) {
    d.widgets.net.refocus.set(true);
    for v in d.widgets.net.wifi.views.values_mut() {
        *v = wifi::View::default();
    }
    for v in d.widgets.net.bluetooth.views.values_mut() {
        *v = bluetooth::View::default();
    }
    wifi::sync(d);
    bluetooth::sync(d);
}

/// The dashboard opens: every widget back on its first page.
pub fn come_up(d: &mut Dash) {
    reset(d);
}

/// The dashboard goes away: pages shut, and what was typed forgotten (a text field belongs to a
/// window, and the next one is another).
pub fn go_down(d: &mut Dash) {
    reset(d);
}

/// The dashboard came up (`true`) or went away: notes, timers and mascots only run while seen.
pub fn awake(_d: &mut Dash, _on: bool) {}

/// Widget `id` of `kind` was entered with the keyboard.
pub fn entered(d: &mut Dash, kind: Kind, id: u32) {
    if kind == Kind::Bluetooth {
        bluetooth::entered(d, id);
    }
}

/// The tiles changed (a widget added, removed, moved): what is kept per instance follows.
pub fn layout_changed(d: &mut Dash) {
    let ids = |kind| -> Vec<u32> {
        d.tiles
            .iter()
            .filter(|t| t.kind() == kind)
            .map(Tile::id)
            .collect()
    };
    let (wifi_ids, bt_ids) = (ids(Kind::Wifi), ids(Kind::Bluetooth));
    if d.widgets.net.wifi.views.keys().any(|id| !wifi_ids.contains(id))
        || d.widgets.net.bluetooth.views.keys().any(|id| !bt_ids.contains(id))
    {
        d.widgets.net.refocus.set(true);
    }
    d.widgets.net.wifi.views.retain(|id, _| wifi_ids.contains(id));
    d.widgets
        .net
        .bluetooth
        .views
        .retain(|id, _| bt_ids.contains(id));
    for id in wifi_ids {
        d.widgets.net.wifi.views.entry(id).or_default();
    }
    for id in bt_ids {
        d.widgets.net.bluetooth.views.entry(id).or_default();
    }
    wifi::sync(d);
    bluetooth::sync(d);
}

/// What the entered widget would say about where its cursor is, for the stress run's log.
pub fn describe(d: &Dash, kind: Kind) -> Option<String> {
    let id = d.nav.active_id;
    let said = match kind {
        Kind::Wifi => wifi::spoken(d, id),
        Kind::Bluetooth => bluetooth::spoken(d, id),
        _ => None,
    }?;
    Some(said.to_string())
}

/// What has to hold of the group's state whatever keys came before: the stress run checks it
/// after every key. An error says what doesn't add up.
pub fn check(d: &Dash) -> Result<(), String> {
    wifi::check(d)?;
    bluetooth::check(d)
}

/// The dashboard is built (`d.state` is what was saved): what the group restores.
pub fn init(_d: &mut Dash) {}

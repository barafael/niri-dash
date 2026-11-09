//! The Bluetooth widget: paired and nearby devices, a device's details, and the pairing prompt.
//! Port of `BluetoothWidget`, `BtRow` and `BtPairingPrompt` (`legacy/ui/widgets/net.slint`) with
//! what `legacy/src/app/feeds.rs` made of the system's state.
//!
//! Pages: 0 devices, 1 one device's details. Stops are kept the way `wifi` keeps them (see [`L`]);
//! the pairing prompt has its own ([`P`]).

use super::field::{self, Field, OnEvent, Pool, Spec, later};
use super::scroll::Scroller;
use super::ui::{self, Look, St};
use super::{Said, Wid, set_typing};
use crate::dash::{Dash, NavMode};
use crate::keys::Key;
use crate::kit::Tk;
use crate::scope::{Scope, Stops};
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, Context, KeyDownEvent, SharedString, Window, div, px};
use gpui_kit::component::input::InputEvent;
use niri_dash_system::{Tell, bluetooth as bt, power::DeviceKind};
use std::cell::Cell;

// ---- what the system reports, as the widget reads it -----------------------------------------

/// A pairing prompt.
#[derive(Debug, Clone)]
pub struct Prompt {
    pub serial: u64,
    pub name: String,
    pub address: String,
    /// 1: do the codes match, 2: enter the PIN, 3: enter the passkey, 4: type this PIN on the
    /// device, 5: type this passkey on the device, 6: allow pairing.
    pub kind: i32,
    /// The code to compare or type on the device.
    pub code: String,
    /// For 5: how many digits have been typed on the device.
    pub entered: u16,
}

impl Prompt {
    /// Asks for a code.
    pub fn asks_code(&self) -> bool {
        self.kind == 2 || self.kind == 3
    }

    /// Shows a code to type on the device.
    pub fn shows_code(&self) -> bool {
        self.kind == 4 || self.kind == 5
    }
}

/// Everything the widget reads of `bluetooth::State`.
#[derive(Debug, Clone, Default)]
pub struct Model {
    pub available: bool,
    pub powered: bool,
    pub blocked: bool,
    pub discovering: bool,
    /// Paired devices, and connected ones.
    pub devices: Vec<bt::Device>,
    /// Unpaired devices a scan found, strongest first.
    pub nearby: Vec<bt::Device>,
    /// How many devices without a name the scan sees.
    pub unnamed: usize,
    pub connected: usize,
    pub status: String,
    pub failed: bool,
    pub prompt: Option<Prompt>,
    /// The device the details page shows; none while there is none.
    pub detail: Option<bt::Device>,
    pub facts: Vec<(String, String)>,
}

impl Model {
    pub fn of(s: &bt::State) -> Self {
        use bt::PromptKind as K;
        let prompt = s
            .prompt
            .as_ref()
            .map(|p| {
                let (kind, code, entered) = match &p.kind {
                    K::Confirm(k) => (1, format!("{k:06}"), 0),
                    K::EnterPin => (2, String::new(), 0),
                    K::EnterPasskey => (3, String::new(), 0),
                    K::ShowPin(pin) => (4, pin.clone(), 0),
                    K::ShowPasskey(k, n) => (5, format!("{k:06}"), *n),
                    K::Authorize => (6, String::new(), 0),
                };
                Prompt {
                    serial: p.serial,
                    name: p.name.clone(),
                    address: p.address.clone(),
                    kind,
                    code,
                    entered,
                }
            })
            .filter(|p| !p.address.is_empty());
        Self {
            available: s.available,
            powered: s.powered,
            blocked: s.blocked,
            discovering: s.discovering,
            devices: s.devices.clone(),
            nearby: s.nearby.clone(),
            unnamed: s.unnamed,
            connected: s.devices.iter().filter(|d| d.connected).count(),
            status: s.activity.to_string(),
            failed: matches!(s.activity, bt::Activity::Failed(_)),
            prompt,
            detail: s.detail.clone().filter(|d| !d.address.is_empty()),
            facts: s.detail.as_ref().map(bt::facts).unwrap_or_default(),
        }
    }

    fn address(&self) -> &str {
        self.detail.as_ref().map_or("", |d| d.address.as_str())
    }

    fn paired(&self) -> bool {
        self.detail.as_ref().is_some_and(|d| d.paired)
    }

    fn known(&self) -> bool {
        self.detail.is_some()
    }

    fn connected_detail(&self) -> bool {
        self.detail.as_ref().is_some_and(|d| d.connected)
    }

    fn blocked_detail(&self) -> bool {
        self.detail.as_ref().is_some_and(|d| d.blocked)
    }

    fn can_connect(&self) -> bool {
        !self.connected_detail() && self.paired() && !self.blocked_detail()
    }

    fn can_pair(&self) -> bool {
        !self.paired() && self.known() && !self.blocked_detail()
    }

    fn has_act(&self) -> bool {
        self.connected_detail() || self.can_connect() || self.can_pair()
    }
}

// ---- the widget's own state -----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Devices,
    Detail,
}

/// The pairing prompt, but for the code typed (the field holds that itself).
#[derive(Debug, Clone, Copy, Default)]
pub struct PromptForm {
    /// The prompt it is for: a new one starts empty.
    pub serial: Option<u64>,
    pub scope: Scope,
}

/// One Bluetooth widget on the dashboard.
#[derive(Default)]
pub struct View {
    pub page: Page,
    pub scope: Scope,
    pub prompt: PromptForm,
    pub pool: Option<Pool>,
    pub scroll: [Scroller; 2],
    /// The field that has the keyboard, if one does.
    pub typing: Cell<Option<&'static str>>,
    pub shown_prompt: Cell<Option<u64>>,
    pub pool_requested: Cell<bool>,
    /// The row the cursor was on as of the last frame: page, stop, row.
    pub last: Cell<Option<(Page, usize, usize)>>,
}

const FIELDS: [Spec; 2] = [
    Spec {
        name: "code",
        placeholder: "PIN (often 0000)",
        masked: false,
    },
    Spec {
        name: "rename",
        placeholder: "Rename",
        masked: false,
    },
];

// ---- where the stops are ---------------------------------------------------------------------

/// The keyboard stops of the widget's pages.
#[derive(Debug, Clone, Copy)]
pub struct L {
    pub round: bool,
    pub page: Page,
    /// The first nearby device.
    pub k_near: isize,
    /// "Add a device".
    pub k_add: isize,
    pub paired: bool,
    pub known: bool,
    pub has_act: bool,
    pub k_act: isize,
    pub k_forget: isize,
    pub count: usize,
}

impl L {
    pub fn of(round: bool, v: &View, m: &Model) -> Self {
        let k_near = m.devices.len() as isize;
        let k_add = k_near + m.nearby.len() as isize;
        let (paired, known, has_act) = (m.paired(), m.known(), m.has_act());
        let k_act = if paired {
            3
        } else if known {
            1
        } else {
            0
        };
        let k_forget = if paired {
            k_act + isize::from(has_act)
        } else {
            -1
        };
        let count = if round || !m.available {
            0
        } else if v.page == Page::Devices {
            if m.powered { (k_add + 1) as usize } else { 0 }
        } else {
            (k_act + isize::from(has_act) + isize::from(paired)) as usize
        };
        Self {
            round,
            page: v.page,
            k_near,
            k_add,
            paired,
            known,
            has_act,
            k_act,
            k_forget,
            count,
        }
    }

    /// A device row has two parts: the row, and its details.
    pub fn subs(&self, cursor: usize) -> usize {
        if self.page == Page::Devices && (cursor as isize) < self.k_add {
            2
        } else {
            1
        }
    }
}

/// The stops of the pairing prompt.
#[derive(Debug, Clone, Copy)]
pub struct P {
    pub asks_code: bool,
    pub shows_code: bool,
    /// "No" / "Cancel": the stop after the code field.
    pub k_no: usize,
    pub count: usize,
}

impl P {
    pub fn of(p: &Prompt) -> Self {
        let k_no = usize::from(p.asks_code());
        Self {
            asks_code: p.asks_code(),
            shows_code: p.shows_code(),
            k_no,
            count: k_no + if p.shows_code() { 1 } else { 2 },
        }
    }
}

// ---- views, and their state in the dashboard -----------------------------------------------

#[derive(Default)]
pub struct Data {
    pub state: bt::State,
    pub views: std::collections::HashMap<u32, View>,
}

pub fn view(d: &Dash, id: u32) -> Option<&View> {
    d.widgets.net.bluetooth.views.get(&id)
}

pub fn view_mut(d: &mut Dash, id: u32) -> &mut View {
    d.widgets.net.bluetooth.views.entry(id).or_default()
}

fn view_mut_opt(d: &mut Dash, id: u32) -> Option<&mut View> {
    d.widgets.net.bluetooth.views.get_mut(&id)
}

fn model(d: &Dash) -> Model {
    Model::of(&d.widgets.net.bluetooth.state)
}

fn is_round(d: &Dash, id: u32) -> bool {
    d.tile(id).is_some_and(|t| t.round()) && d.nav.expanded_id != id
}

/// The system reported its state.
pub fn set(d: &mut Dash, state: bt::State) {
    d.widgets.net.bluetooth.state = state;
    sync(d);
}

struct Plain;
impl Stops for Plain {}

/// The stops follow the data and the layout.
pub fn sync(d: &mut Dash) {
    let m = model(d);
    let ids: Vec<u32> = d.widgets.net.bluetooth.views.keys().copied().collect();
    for id in ids {
        let round = is_round(d, id);
        let v = view_mut(d, id);
        match &m.prompt {
            Some(p) if v.prompt.serial != Some(p.serial) => {
                v.prompt = PromptForm {
                    serial: Some(p.serial),
                    ..PromptForm::default()
                };
            }
            None => v.prompt.serial = None,
            _ => {}
        }
        let l = L::of(round, v, &m);
        v.scope.set_count(l.count, &Plain);
        v.scope.set_subs(l.subs(v.scope.cursor));
        if let Some(p) = &m.prompt {
            v.prompt.scope.set_count(P::of(p).count, &Plain);
        }
    }
}

/// The widget was entered: a yes or no question has the cursor on the answer.
pub fn entered(d: &mut Dash, id: u32) {
    let m = model(d);
    if let Some(p) = &m.prompt
        && !p.asks_code()
        && !p.shows_code()
        && let Some(v) = view_mut_opt(d, id)
    {
        v.prompt.scope.cursor = 1;
        v.prompt.scope.sub = 0;
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Fx {
    pub page: bool,
    pub prompt_cursor: Option<usize>,
}

impl Fx {
    fn apply(self, v: &mut View) {
        if self.page {
            v.scope.cursor = 0;
            v.scope.sub = 0;
        }
        if let Some(c) = self.prompt_cursor {
            v.prompt.scope.cursor = c;
            v.prompt.scope.sub = 0;
        }
    }
}

// ---- what can be done ----------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Act {
    Toggle,
    Scan,
    /// A device's row was pressed.
    Press(String, bool),
    Open(String),
    Back,
    Trust,
    Block,
    Rename,
    Edit(&'static str),
    /// The main button of the details page: Disconnect, Connect or Pair.
    Main,
    Forget,
    PairNow,
    Yes,
    No,
    /// Enter in the code field.
    Accept,
}

fn tell(d: &Dash, cmd: bt::Command) {
    d.tx.bluetooth.tell(cmd);
}

pub fn act(d: &mut Dash, id: u32, a: Act, cx: &mut Context<Dash>) -> Fx {
    let m = model(d);
    let mut fx = Fx::default();
    match a {
        Act::Toggle => tell(d, bt::Command::Toggle),
        Act::Scan => tell(d, bt::Command::Scan(!m.discovering)),
        Act::Press(address, nearby) => {
            let dev = m
                .devices
                .iter()
                .chain(&m.nearby)
                .find(|x| x.address == address)
                .cloned();
            let Some(dev) = dev else {
                return fx;
            };
            if nearby {
                tell(d, bt::Command::Pair(address));
            } else if dev.blocked {
                return act(d, id, Act::Open(address), cx);
            } else if dev.connected {
                tell(d, bt::Command::Disconnect(address));
            } else {
                tell(d, bt::Command::Connect(address));
            }
        }
        Act::Open(address) => {
            clear_rename(d, id, cx);
            view_mut(d, id).page = Page::Detail;
            fx.page = true;
            tell(d, bt::Command::Select(address));
        }
        Act::Back => {
            tell(d, bt::Command::Select(String::new()));
            view_mut(d, id).page = Page::Devices;
            fx.page = true;
        }
        Act::Trust => {
            if let Some(det) = &m.detail {
                tell(d, bt::Command::SetTrusted(det.address.clone(), !det.trusted));
            }
        }
        Act::Block => {
            if let Some(det) = &m.detail {
                tell(d, bt::Command::SetBlocked(det.address.clone(), !det.blocked));
            }
        }
        Act::Rename => {
            let name = view(d, id)
                .and_then(|v| v.pool.clone())
                .map(|p| p.value("rename", cx))
                .unwrap_or_default();
            if !name.is_empty() {
                tell(d, bt::Command::Rename(m.address().to_string(), name));
                clear_rename(d, id, cx);
            }
        }
        Act::Edit(name) => focus_field(d, id, name, cx),
        Act::Main => {
            if m.connected_detail() {
                tell(d, bt::Command::Disconnect(m.address().to_string()));
            } else if m.can_connect() && m.powered {
                tell(d, bt::Command::Connect(m.address().to_string()));
            } else if m.can_pair() && m.powered {
                tell(d, bt::Command::Pair(m.address().to_string()));
                return act(d, id, Act::Back, cx);
            }
        }
        Act::PairNow => {
            if m.powered {
                tell(d, bt::Command::Pair(m.address().to_string()));
                return act(d, id, Act::Back, cx);
            }
        }
        Act::Forget => {
            tell(d, bt::Command::Forget(m.address().to_string()));
            return act(d, id, Act::Back, cx);
        }
        Act::No => {
            if let Some(p) = &m.prompt {
                if p.shows_code() {
                    tell(d, bt::Command::CancelPairing);
                } else {
                    tell(d, bt::Command::Decline);
                }
            }
        }
        Act::Yes => {
            if let Some(p) = &m.prompt {
                let code = code_typed(d, id, cx);
                if !p.asks_code() || !code.is_empty() {
                    answer(d, p.serial, code);
                }
            }
        }
        Act::Accept => {
            if let Some(p) = &m.prompt {
                let code = code_typed(d, id, cx);
                answer(d, p.serial, code);
            }
        }
    }
    cx.notify();
    fx
}

fn code_typed(d: &Dash, id: u32, cx: &Context<Dash>) -> String {
    view(d, id)
        .and_then(|v| v.pool.as_ref())
        .map(|p| p.value("code", cx))
        .unwrap_or_default()
}

/// The answer to the pairing prompt with this serial: yes, or the code typed.
fn answer(d: &Dash, serial: u64, code: String) {
    let answer = if code.is_empty() {
        bt::Answer::Yes
    } else {
        bt::Answer::Code(code)
    };
    tell(d, bt::Command::Answer { serial, answer });
}

fn clear_rename(d: &mut Dash, id: u32, cx: &mut Context<Dash>) {
    if let Some(pool) = view(d, id).and_then(|v| v.pool.clone()) {
        later(cx, move |_, window, cx| pool.set("rename", "", window, cx));
    }
}

fn focus_field(d: &mut Dash, id: u32, name: &'static str, cx: &mut Context<Dash>) {
    let pool = view(d, id).and_then(|v| v.pool.clone());
    later(cx, move |d, window, cx| {
        let pool = pool.or_else(|| view(d, id).and_then(|v| v.pool.clone()));
        if let Some(pool) = pool {
            pool.focus(name, window, cx);
            set_typing(d, Wid::Bt(id), Some(name));
        }
    });
}

pub fn run(d: &mut Dash, id: u32, a: Act, cx: &mut Context<Dash>) {
    let fx = act(d, id, a, cx);
    fx.apply(view_mut(d, id));
    sync(d);
    cx.notify();
}

// ---- keys ----------------------------------------------------------------------------------

struct Main<'a, 'b> {
    d: &'a mut Dash,
    cx: &'a mut Context<'b, Dash>,
    id: u32,
    fx: Fx,
}

impl Main<'_, '_> {
    fn ctx(&self) -> (Model, L) {
        let m = model(self.d);
        let l = L::of(
            is_round(self.d, self.id),
            view(self.d, self.id).expect("a view"),
            &m,
        );
        (m, l)
    }

    fn do_(&mut self, a: Act) {
        let fx = act(self.d, self.id, a, self.cx);
        self.fx.page |= fx.page;
        self.fx.prompt_cursor = fx.prompt_cursor.or(self.fx.prompt_cursor);
    }
}

impl Stops for Main<'_, '_> {
    fn activate(&mut self, i: usize, sub: usize) {
        let (m, l) = self.ctx();
        let ii = i as isize;
        if l.round {
            self.do_(Act::Toggle);
        } else if l.page == Page::Devices {
            if ii < l.k_add {
                let nearby = ii >= l.k_near;
                let dev = if nearby {
                    m.nearby.get((ii - l.k_near) as usize)
                } else {
                    m.devices.get(i)
                };
                let Some(dev) = dev.cloned() else {
                    return;
                };
                if sub == 1 {
                    self.do_(Act::Open(dev.address));
                } else if !dev.busy {
                    self.do_(Act::Press(dev.address, nearby));
                }
            } else {
                self.do_(Act::Scan);
            }
        } else if l.paired && i == 0 {
            self.do_(Act::Trust);
        } else if l.known && ii == if l.paired { 1 } else { 0 } {
            self.do_(Act::Block);
        } else if l.paired && i == 2 {
            self.do_(Act::Edit("rename"));
        } else if ii == l.k_act && l.has_act {
            self.do_(Act::Main);
        } else if ii == l.k_forget {
            self.do_(Act::Forget);
        }
    }

    fn letter(&mut self, key: &Key, cursor: usize) -> bool {
        let (m, l) = self.ctx();
        if l.round || l.page != Page::Devices {
            if key.is('i') && l.page == Page::Detail && l.paired && cursor == 2 {
                self.do_(Act::Edit("rename"));
                return true;
            }
            return false;
        }
        if key.is('r') && m.powered {
            self.do_(Act::Scan);
            return true;
        }
        if key.is('t') {
            self.do_(Act::Toggle);
            return true;
        }
        false
    }

    fn back(&mut self) -> bool {
        let page = view(self.d, self.id).map(|v| v.page);
        if page.is_some_and(|p| p != Page::Devices) {
            self.do_(Act::Back);
            return true;
        }
        false
    }
}

struct Ask<'a, 'b> {
    d: &'a mut Dash,
    cx: &'a mut Context<'b, Dash>,
    id: u32,
    fx: Fx,
}

impl Ask<'_, '_> {
    fn ctx(&self) -> Option<(Prompt, P)> {
        let p = model(self.d).prompt?;
        let q = P::of(&p);
        Some((p, q))
    }

    fn do_(&mut self, a: Act) {
        let fx = act(self.d, self.id, a, self.cx);
        self.fx.prompt_cursor = fx.prompt_cursor.or(self.fx.prompt_cursor);
    }
}

impl Stops for Ask<'_, '_> {
    fn adjust(&mut self, i: usize, d: i32) -> bool {
        let Some((_, q)) = self.ctx() else {
            return false;
        };
        let to = (i as i32 + d).clamp(q.k_no as i32, q.count as i32 - 1) as usize;
        self.fx.prompt_cursor = Some(to);
        true
    }

    fn activate(&mut self, i: usize, _sub: usize) {
        let Some((_, q)) = self.ctx() else {
            return;
        };
        if i < q.k_no {
            self.do_(Act::Edit("code"));
        } else if i == q.k_no {
            self.do_(Act::No);
        } else {
            self.do_(Act::Yes);
        }
    }

    fn letter(&mut self, key: &Key, i: usize) -> bool {
        let Some((_, q)) = self.ctx() else {
            return false;
        };
        if key.is('i') && i < q.k_no {
            self.do_(Act::Edit("code"));
            return true;
        }
        false
    }

    fn back(&mut self) -> bool {
        self.do_(Act::No);
        true
    }
}

/// A key for the entered widget `id`. True if it was taken.
pub fn key(d: &mut Dash, id: u32, key: &Key, cx: &mut Context<Dash>) -> bool {
    let Some(v) = view(d, id) else {
        return false;
    };
    if v.typing.get().is_some() {
        return false;
    }
    for s in &v.scroll {
        s.follow_again();
    }
    let asking = model(d).prompt.is_some();
    let mut scope = if asking { v.prompt.scope } else { v.scope };
    let (taken, fx) = if asking {
        let mut stops = Ask {
            d,
            cx,
            id,
            fx: Fx::default(),
        };
        (scope.press(key, &mut stops), stops.fx)
    } else {
        let mut stops = Main {
            d,
            cx,
            id,
            fx: Fx::default(),
        };
        (scope.press(key, &mut stops), stops.fx)
    };
    let Some(v) = view_mut_opt(d, id) else {
        return taken;
    };
    if asking {
        v.prompt.scope = scope;
    } else {
        v.scope = scope;
    }
    fx.apply(v);
    sync(d);
    cx.notify();
    taken
}

/// Escape in the entered widget: the prompt is answered no, a page left.
pub fn escape(d: &mut Dash, id: u32, cx: &mut Context<Dash>) -> bool {
    if view(d, id).is_none() {
        return false;
    }
    if view(d, id).is_some_and(|v| v.typing.get().is_some()) {
        set_typing(d, Wid::Bt(id), None);
        later(cx, |d, window, cx| field::give_back(d, window, cx));
        return true;
    }
    let (taken, fx) = if model(d).prompt.is_some() {
        let mut s = Ask {
            d,
            cx,
            id,
            fx: Fx::default(),
        };
        (s.back(), s.fx)
    } else {
        let mut s = Main {
            d,
            cx,
            id,
            fx: Fx::default(),
        };
        (s.back(), s.fx)
    };
    fx.apply(view_mut(d, id));
    sync(d);
    cx.notify();
    taken
}

pub fn field_event(
    d: &mut Dash,
    id: u32,
    name: &'static str,
    event: &InputEvent,
    cx: &mut Context<Dash>,
) {
    match event {
        InputEvent::Focus => {
            let m = model(d);
            let Some(v) = view_mut_opt(d, id) else {
                return;
            };
            v.typing.set(Some(name));
            // The cursor goes where the keyboard is.
            match name {
                "code" if m.prompt.is_some() => v.prompt.scope.move_to(0, &Plain),
                "rename" if v.page == Page::Detail => v.scope.move_to(2, &Plain),
                _ => {}
            }
            cx.notify();
        }
        InputEvent::Blur => {
            if let Some(v) = view_mut_opt(d, id)
                && v.typing.get() == Some(name)
            {
                v.typing.set(None);
                cx.notify();
            }
        }
        InputEvent::PressEnter { .. } => match name {
            "code" => run(d, id, Act::Accept, cx),
            "rename" => run(d, id, Act::Rename, cx),
            _ => {}
        },
        InputEvent::Change => cx.notify(),
    }
}

/// The cursor goes to the stop of the text field `name`.
pub(super) fn move_cursor_to(d: &mut Dash, id: u32, name: &str) {
    let m = model(d);
    let Some(v) = view_mut_opt(d, id) else {
        return;
    };
    match name {
        "code" if m.prompt.is_some() => v.prompt.scope.move_to(0, &Plain),
        "rename" if v.page == Page::Detail => v.scope.move_to(2, &Plain),
        _ => {}
    }
}

/// The pairing code is digits only when it is a passkey.
pub fn digits_only(d: &Dash) -> bool {
    model(d).prompt.is_some_and(|p| p.kind == 3)
}

pub fn tab_order(d: &Dash, id: u32, name: &str) -> Vec<&'static str> {
    // One field at a time on the pairing prompt; the details page has one too.
    let _ = (d, id);
    FIELDS
        .iter()
        .map(|s| s.name)
        .filter(|n| *n == name)
        .collect()
}

pub fn make_pool(d: &mut Dash, id: u32, window: &mut Window, cx: &mut Context<Dash>) {
    let Some(v) = view(d, id) else {
        return;
    };
    if v.pool.is_some() {
        return;
    }
    let on: OnEvent = std::rc::Rc::new(move |d, name, event, _, cx| {
        field_event(d, id, name, event, cx)
    });
    let pool = Pool::new(&FIELDS, window, cx, on);
    view_mut(d, id).pool = Some(pool);
    cx.notify();
}

// ---- what a screen reader says ----------------------------------------------------------------

fn said(role: &'static str, label: impl Into<String>, value: impl Into<String>) -> Said {
    Said {
        role,
        label: label.into(),
        value: value.into(),
        checked: None,
    }
}

fn row_value(d: &bt::Device) -> String {
    if d.busy {
        "busy".into()
    } else if d.blocked {
        "blocked".into()
    } else if d.connected && !d.paired {
        "connected, not paired".into()
    } else if d.connected && d.battery >= 0 {
        format!("connected, battery {}%", d.battery)
    } else if d.connected {
        "connected".into()
    } else if d.battery >= 0 {
        format!("battery {}%", d.battery)
    } else {
        String::new()
    }
}

fn add_label(m: &Model) -> &'static str {
    if m.discovering { "Stop looking" } else { "Add a device" }
}

/// What the stop under the cursor of widget `id` is, as a screen reader would say it.
pub fn spoken(d: &Dash, id: u32) -> Option<Said> {
    let v = view(d, id)?;
    let m = model(d);
    if let Some(p) = &m.prompt {
        let q = P::of(p);
        let c = v.prompt.scope.cursor;
        return Some(if c < q.k_no {
            said("textinput", if p.kind == 2 { "PIN" } else { "Passkey" }, "")
        } else if c == q.k_no {
            said("button", if p.kind == 1 || p.kind == 6 { "No" } else { "Cancel" }, "")
        } else {
            said("button", yes_label(p), "")
        });
    }
    let l = L::of(is_round(d, id), v, &m);
    if l.count == 0 {
        return None;
    }
    let c = v.scope.cursor.min(l.count - 1);
    let ci = c as isize;
    Some(match l.page {
        Page::Devices => {
            if ci < l.k_add {
                let dev = if ci < l.k_near {
                    &m.devices[c]
                } else {
                    &m.nearby[(ci - l.k_near) as usize]
                };
                if v.scope.part() == 1 {
                    said("button", format!("Details of {}", dev.name), "")
                } else {
                    said("listitem", dev.name.clone(), row_value(dev))
                }
            } else {
                said("listitem", add_label(&m), "")
            }
        }
        Page::Detail => {
            let det = m.detail.as_ref();
            if l.paired && c == 0 {
                Said {
                    checked: Some(det.is_some_and(|x| x.trusted)),
                    ..said("switch", "Connect automatically", "")
                }
            } else if l.known && ci == if l.paired { 1 } else { 0 } {
                Said {
                    checked: Some(m.blocked_detail()),
                    ..said("switch", "Block", "")
                }
            } else if l.paired && c == 2 {
                said("textinput", "New name", "")
            } else if ci == l.k_act && l.has_act {
                said(
                    "button",
                    if m.connected_detail() {
                        "Disconnect"
                    } else if m.can_connect() {
                        "Connect"
                    } else {
                        "Pair"
                    },
                    "",
                )
            } else {
                said("button", "Forget device", "")
            }
        }
    })
}

fn yes_label(p: &Prompt) -> &'static str {
    match p.kind {
        1 => "Yes, they match",
        6 => "Allow",
        _ => "Pair",
    }
}

// ---- invariants -----------------------------------------------------------------------------

pub fn check(d: &Dash) -> Result<(), String> {
    let m = model(d);
    for (&id, v) in &d.widgets.net.bluetooth.views {
        let l = L::of(is_round(d, id), v, &m);
        let s = &v.scope;
        let at = format!("bluetooth widget {id}");
        if l.count > 0 && s.cursor >= l.count {
            return Err(format!(
                "{at}: the cursor is on stop {} of {}",
                s.cursor, l.count
            ));
        }
        if s.sub >= s.subs.max(1) || (l.count > 0 && s.subs != l.subs(s.cursor).max(1)) {
            return Err(format!(
                "{at}: the cursor is on part {} of a stop with {} (and {} now)",
                s.sub,
                s.subs,
                l.subs(s.cursor)
            ));
        }
        if let Some(p) = &m.prompt
            && v.prompt.scope.cursor >= P::of(p).count
        {
            return Err(format!(
                "{at}: the cursor is on stop {} of the prompt's {}",
                v.prompt.scope.cursor,
                P::of(p).count
            ));
        }
        let mine = d.nav.mode == NavMode::Widget && d.nav.active_id == id;
        if let Some(name) = v.typing.get()
            && mine
        {
            let (cursor, stop) = match (name, &m.prompt) {
                ("code", Some(_)) => (v.prompt.scope.cursor, 0),
                ("rename", None) => (v.scope.cursor, 2),
                _ => (0, 0),
            };
            if cursor != stop && matches!((name, &m.prompt), ("code", Some(_)) | ("rename", None))
            {
                return Err(format!(
                    "{at}: typing in {name:?}, but the cursor is on stop {cursor}, not {stop}"
                ));
            }
        }
        if let Some((page, stop, row)) = v.last.get()
            && mine
            && m.prompt.is_none()
            && page == v.page
            && stop == v.scope.cursor
            && v.scroll[page as usize].sees(row) == Some(false)
        {
            return Err(format!("{at}: the cursor is out of sight on stop {stop}"));
        }
    }
    Ok(())
}

// ---- drawing -----------------------------------------------------------------------------------

fn icon_of(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Mouse => "mouse",
        DeviceKind::Keyboard => "keyboard",
        DeviceKind::Headphones => "headphones",
        DeviceKind::Gamepad => "gamepad",
        DeviceKind::Phone => "phone",
        DeviceKind::Other => "bluetooth",
    }
}

fn row_width(ctx: &Ctx) -> f32 {
    let gutter = if ctx.tk.pad() >= 8.0 { 0.0 } else { 10.0 };
    ctx.w - 2.0 * ctx.tk.pad() - gutter
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let (d, tk) = (ctx.d, ctx.tk);
    let id = ctx.tile.id();
    let Some(v) = view(d, id) else {
        return div().into_any_element();
    };
    let m = model(d);
    field::sync_theme(tk, cx);
    if ctx.round {
        return round(&m, ctx, id, cx);
    }
    let l = L::of(false, v, &m);
    let on = ctx.entered && m.prompt.is_none();
    let scope = v.scope;
    let kf = move |i: isize| on && scope.cursor as isize == i;
    reconcile(v, &m, id, cx);

    let mut card = div()
        .size_full()
        .p(px(tk.pad()))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .overflow_hidden();
    card = match v.page {
        Page::Devices => card.child(
            ui::header(
                "bluetooth",
                if m.powered { tk.accent() } else { tk.faint() },
                "Bluetooth",
                tk,
            )
            .child(ui::icon_button(
                "add-device",
                "plus",
                add_label(&m),
                28.0,
                tk.text(),
                St::new().enabled(m.powered).active(m.discovering),
                tk,
                cx.listener(move |d, _, _, cx| run(d, id, Act::Scan, cx)),
            ))
            .child(ui::switch(
                "bluetooth-switch",
                m.powered,
                "Bluetooth",
                St::new().enabled(m.available && !m.blocked),
                tk,
                cx.listener(move |d, _, _, cx| run(d, id, Act::Toggle, cx)),
            )),
        ),
        Page::Detail => card.child(ui::back_header(
            m.detail.as_ref().map_or("", |x| x.name.as_str()),
            tk,
            cx.listener(move |d, _, _, cx| run(d, id, Act::Back, cx)),
        )),
    };
    if !m.status.is_empty() {
        card = card.child(
            ui::muted_wrap(m.status.clone(), tk)
                .flex_none()
                .text_color(if m.failed { tk.bad() } else { tk.dim() }),
        );
    }
    if let Some(p) = &m.prompt {
        card = card.child(prompt(ctx, v, p, id, cx));
    }
    if !m.available {
        card = card.child(ui::empty_fill("No Bluetooth adapter", tk));
    }
    if m.available && m.blocked {
        card = card.child(ui::empty_fill(
            "Bluetooth is blocked (airplane mode or a hardware switch)",
            tk,
        ));
    }
    if m.available && !m.blocked && !m.powered {
        card = card.child(ui::empty_fill("Bluetooth is off", tk));
    }
    if m.available && m.powered && v.page == Page::Devices {
        card = card.child(devices(ctx, v, &m, &l, &kf, id, cx));
    }
    if m.available && v.page == Page::Detail {
        card = card.child(detail(ctx, v, &m, &l, &kf, id, cx));
    }
    card.into_any_element()
}

fn round(m: &Model, ctx: &Ctx, id: u32, cx: &mut Context<Dash>) -> AnyElement {
    let tk = ctx.tk;
    let title = if !m.available {
        "No adapter".to_string()
    } else if m.blocked {
        "Blocked".to_string()
    } else if !m.powered {
        "Off".to_string()
    } else if m.connected > 0 {
        format!("{} connected", m.connected)
    } else {
        "On".to_string()
    };
    div()
        .id("bluetooth-round")
        .role(gpui::Role::Switch)
        .aria_label("Bluetooth")
        .aria_toggled(if m.powered {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .debug_selector(|| "Bluetooth".into())
        .size_full()
        .cursor_pointer()
        .on_click(cx.listener(move |d, _, _, cx| run(d, id, Act::Toggle, cx)))
        .child(ui::round_face(
            "bluetooth",
            if m.powered { tk.accent() } else { tk.faint() },
            &title,
            "",
            m.powered,
            ctx.w,
            tk,
        ))
        .into_any_element()
}

/// What appeared and what went since the last frame, as far as the keyboard goes.
fn reconcile(v: &View, m: &Model, id: u32, cx: &mut Context<Dash>) {
    let serial = m
        .prompt
        .as_ref()
        .filter(|p| p.asks_code())
        .map(|p| p.serial);
    let rename_up = m.available && v.page == Page::Detail && m.paired();
    if (serial.is_some() || rename_up) && v.pool.is_none() && !v.pool_requested.get() {
        v.pool_requested.set(true);
        later(cx, move |d, window, cx| {
            make_pool(d, id, window, cx);
            view_mut(d, id).pool_requested.set(false);
        });
    }
    if v.shown_prompt.get() != serial {
        v.shown_prompt.set(serial);
        if serial.is_some() {
            let placeholder = match m.prompt.as_ref().map(|p| p.kind) {
                Some(3) => "6-digit passkey",
                _ => "PIN (often 0000)",
            };
            later(cx, move |d, window, cx| {
                make_pool(d, id, window, cx);
                let Some(pool) = view(d, id).and_then(|v| v.pool.clone()) else {
                    return;
                };
                if let Some(state) = pool.state("code") {
                    state.update(cx, |s, cx| s.set_placeholder(placeholder, window, cx));
                }
                pool.set("code", "", window, cx);
                pool.focus("code", window, cx);
                set_typing(d, Wid::Bt(id), Some("code"));
                if let Some(v) = view_mut_opt(d, id) {
                    v.prompt.scope.move_to(0, &Plain);
                }
            });
        }
    }
    if let Some(name) = v.typing.get() {
        let there = match name {
            "code" => serial.is_some(),
            _ => rename_up,
        };
        if !there {
            v.typing.set(None);
            later(cx, |d, window, cx| field::give_back(d, window, cx));
        }
    }
}

fn field_key(
    d: &mut Dash,
    id: u32,
    name: &'static str,
    ev: &KeyDownEvent,
    window: &mut Window,
    cx: &mut Context<Dash>,
) {
    super::field_key(d, Wid::Bt(id), name, ev, window, cx);
}

#[allow(clippy::too_many_arguments)]
fn field<'a>(
    v: &'a View,
    id: u32,
    name: &'static str,
    label: &'a str,
    placeholder: &'a str,
    secret: bool,
    kfocus: bool,
    tk: &Tk,
    cx: &mut Context<Dash>,
) -> AnyElement {
    Field {
        pool: v.pool.as_ref(),
        name,
        label,
        placeholder,
        secret,
        kfocus,
        typing: v.typing.get() == Some(name),
    }
    .view(tk, cx, move |d, ev, window, cx| {
        field_key(d, id, name, ev, window, cx)
    })
}

/// A pairing prompt: compare a code, type one, or type one shown here on the device.
fn prompt(ctx: &Ctx, v: &View, p: &Prompt, id: u32, cx: &mut Context<Dash>) -> AnyElement {
    let tk = ctx.tk;
    let q = P::of(p);
    let on = ctx.entered;
    let cursor = v.prompt.scope.cursor;
    let title = match p.kind {
        1 => format!("Pair with {}?", p.name),
        2 => format!("PIN for {}", p.name),
        3 => format!("Passkey for {}", p.name),
        4 | 5 => format!("Type this on {}, then Enter", p.name),
        _ => format!("Allow {} to pair?", p.name),
    };
    let mut col = div()
        .flex_none()
        .w_full()
        .p(px(tk.pad_tight()))
        .rounded(px(tk.radius_small()))
        .bg(tk.card_raised())
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .text_size(px(tk.font_small()))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(tk.text())
                .child(title),
        )
        // The address too: the name is whatever the device says it is.
        .child(ui::muted(p.address.clone(), tk));
    if p.kind == 1 || q.shows_code {
        col = col.child(
            div()
                .w_full()
                .text_center()
                .text_size(px(tk.font_title() * 1.6))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(tk.accent())
                .child(p.code.clone()),
        );
    }
    if p.kind == 1 {
        col = col.child(
            ui::muted_wrap("Pair only if the device shows the same number.", tk).text_center(),
        );
    }
    if p.kind == 5 && p.entered > 0 {
        col = col.child(ui::muted(format!("{} of 6 typed", p.entered), tk).text_center());
    }
    if q.asks_code {
        let (label, placeholder) = if p.kind == 2 {
            ("PIN", "PIN (often 0000)")
        } else {
            ("Passkey", "6-digit passkey")
        };
        // What is typed isn't read out.
        col = col.child(field(
            v,
            id,
            "code",
            label,
            placeholder,
            true,
            on && cursor == 0,
            tk,
            cx,
        ));
    }
    let mut buttons = div()
        .flex()
        .flex_row()
        .justify_end()
        .gap(px(6.0))
        .child(ui::text_button(
            "prompt-no",
            if p.kind == 1 || p.kind == 6 { "No" } else { "Cancel" },
            None,
            Look::default(),
            St::cursor(on && cursor == q.k_no),
            tk,
            cx.listener(move |d, _, _, cx| run(d, id, Act::No, cx)),
        ));
    if !q.shows_code {
        let code_empty = v
            .pool
            .as_ref()
            .is_none_or(|pl| pl.value("code", cx).is_empty());
        buttons = buttons.child(ui::text_button(
            "prompt-yes",
            yes_label(p),
            None,
            Look::PRIMARY,
            St::cursor(on && cursor == q.k_no + 1).enabled(!q.asks_code || !code_empty),
            tk,
            cx.listener(move |d, _, _, cx| run(d, id, Act::Yes, cx)),
        ));
    }
    col.child(buttons).into_any_element()
}

/// One device: icon, name, and what's worth knowing at a glance.
#[allow(clippy::too_many_arguments)]
fn device_row(
    ctx: &Ctx,
    dev: &bt::Device,
    key: String,
    nearby: bool,
    here: bool,
    sub: usize,
    id: u32,
    cx: &mut Context<Dash>,
) -> AnyElement {
    let tk = ctx.tk;
    let narrow = row_width(ctx) < 150.0;
    let address = dev.address.clone();
    let tint = if dev.connected { tk.accent() } else { tk.dim() };
    let mut row = ui::row(
        SharedString::from(key.clone()),
        dev.name.clone(),
        row_value(dev),
        St::cursor(here && sub == 0)
            .active(dev.connected)
            .enabled(!dev.busy),
        tk,
        {
            let address = address.clone();
            cx.listener(move |d, _, _, cx| run(d, id, Act::Press(address.clone(), nearby), cx))
        },
    )
    .pl(px(10.0))
    .pr(px(2.0))
    .gap(px(8.0))
    .child(crate::kit::icon(icon_of(dev.kind), 16.0, tint))
    .child(
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(px(tk.font_body()))
            .text_color(if dev.connected { tk.accent() } else { tk.text() })
            .child(dev.name.clone()),
    );
    if !narrow {
        let text = if dev.busy {
            "…".to_string()
        } else if dev.blocked {
            "blocked".into()
        } else if !dev.paired && dev.connected {
            "not paired".into()
        } else if dev.battery >= 0 {
            format!("{}%", dev.battery)
        } else {
            String::new()
        };
        let warn = (!dev.paired && dev.connected) || dev.blocked;
        row = row.child(
            ui::muted(text, tk)
                .flex_none()
                .text_color(if warn { tk.warn() } else { tk.dim() }),
        );
        if dev.bars() >= 0 {
            row = row.child(ui::signal_bars(dev.bars() * 25, tk.text(), tk));
        }
    }
    row.child(ui::icon_button(
        SharedString::from(format!("{key}-details")),
        "chevron-right",
        format!("Details of {}", dev.name),
        26.0,
        tk.dim(),
        St::cursor(here && sub == 1),
        tk,
        cx.listener(move |d, _, _, cx| run(d, id, Act::Open(address.clone()), cx)),
    ))
    .into_any_element()
}

/// Paired devices, then what a scan finds.
fn devices(
    ctx: &Ctx,
    v: &View,
    m: &Model,
    l: &L,
    kf: &impl Fn(isize) -> bool,
    id: u32,
    cx: &mut Context<Dash>,
) -> AnyElement {
    let tk = ctx.tk;
    let sub = v.scope.part();
    let on = ctx.entered && m.prompt.is_none();
    let mut rows: Vec<AnyElement> = Vec::new();
    let mut follow = None;
    let mut cursor_row = None;
    for (i, dev) in m.devices.iter().enumerate() {
        if kf(i as isize) {
            follow = Some(rows.len());
            cursor_row = Some(i);
        }
        rows.push(device_row(
            ctx,
            dev,
            format!("device-{i}"),
            false,
            kf(i as isize),
            sub,
            id,
            cx,
        ));
    }
    let _ = cursor_row;
    if m.devices.is_empty() && !m.discovering {
        rows.push(ui::empty_line("No paired devices", 48.0, tk));
    }
    if m.discovering || !m.nearby.is_empty() {
        rows.push(ui::section_label(
            if m.discovering { "Nearby (looking…)" } else { "Nearby" },
            26.0,
            tk,
        ));
    }
    for (i, dev) in m.nearby.iter().enumerate() {
        let stop = l.k_near + i as isize;
        if kf(stop) {
            follow = Some(rows.len());
        }
        rows.push(device_row(
            ctx,
            dev,
            format!("nearby-{i}"),
            true,
            kf(stop),
            sub,
            id,
            cx,
        ));
    }
    if m.discovering && m.nearby.is_empty() {
        rows.push(ui::muted_line("Put the device into pairing mode.", 30.0, false, tk));
    }
    if m.unnamed > 0 {
        rows.push(ui::muted_line(
            format!(
                "+ {} {} without a name",
                m.unnamed,
                if m.unnamed == 1 { "device" } else { "devices" }
            ),
            24.0,
            false,
            tk,
        ));
    }
    if kf(l.k_add) {
        follow = Some(rows.len());
    }
    rows.push(
        ui::row(
            "add",
            add_label(m),
            "",
            St::cursor(kf(l.k_add)),
            tk,
            cx.listener(move |d, _, _, cx| run(d, id, Act::Scan, cx)),
        )
        .pl(px(10.0))
        .gap(px(8.0))
        .child(crate::kit::icon(
            if m.discovering { "close" } else { "plus" },
            16.0,
            tk.dim(),
        ))
        .child(
            div()
                .flex_1()
                .text_size(px(tk.font_body()))
                .text_color(tk.dim())
                .child(if m.discovering {
                    "Stop looking"
                } else {
                    "Add a device…"
                }),
        )
        .into_any_element(),
    );
    let list = v.scroll[0].list("device-list", rows, follow, 0.0, tk);
    match (on, follow) {
        (true, Some(row)) => v.last.set(Some((Page::Devices, v.scope.cursor, row))),
        _ => v.last.set(None),
    }
    list
}

/// One device: what BlueZ knows, and what can be done with it.
fn detail(
    ctx: &Ctx,
    v: &View,
    m: &Model,
    l: &L,
    kf: &impl Fn(isize) -> bool,
    id: u32,
    cx: &mut Context<Dash>,
) -> AnyElement {
    let tk = ctx.tk;
    let on = ctx.entered && m.prompt.is_none();
    let cursor = v.scope.cursor as isize;
    let mut rows: Vec<AnyElement> = Vec::new();
    let mut follow = None;
    if m.detail.is_none() {
        rows.push(ui::empty_line("This device is gone.", 48.0, tk));
    }
    for (label, value) in &m.facts {
        rows.push(ui::fact(label, value, tk));
    }
    let det = m.detail.as_ref();
    if det.is_some_and(|x| x.paired) {
        if kf(0) {
            follow = Some(rows.len());
        }
        rows.push(ui::switch_line(
            "trust",
            "Connect automatically",
            det.is_some_and(|x| x.trusted),
            St::cursor(kf(0)),
            tk,
            cx.listener(move |d, _, _, cx| run(d, id, Act::Trust, cx)),
        ));
    }
    if det.is_some() {
        let k = if l.paired { 1 } else { 0 };
        if kf(k) {
            follow = Some(rows.len());
        }
        rows.push(ui::switch_line(
            "block",
            "Block",
            m.blocked_detail(),
            St::cursor(kf(k)),
            tk,
            cx.listener(move |d, _, _, cx| run(d, id, Act::Block, cx)),
        ));
    }
    if l.paired {
        if kf(2) {
            follow = Some(rows.len());
        }
        let named = v
            .pool
            .as_ref()
            .is_some_and(|p| !p.value("rename", cx).is_empty());
        rows.push(
            div()
                .flex_none()
                .w_full()
                .flex()
                .flex_row()
                .items_start()
                .gap(px(6.0))
                .child(
                    div().flex_1().min_w_0().child(field(
                        v,
                        id,
                        "rename",
                        "New name",
                        "Rename",
                        false,
                        kf(2),
                        tk,
                        cx,
                    )),
                )
                .child(ui::text_button(
                    "rename-button",
                    "Rename",
                    None,
                    Look::default(),
                    St::new().enabled(named),
                    tk,
                    cx.listener(move |d, _, _, cx| run(d, id, Act::Rename, cx)),
                ))
                .into_any_element(),
        );
    }
    // The keyboard cursor comes to these right to left: the main one first.
    if on && cursor >= l.k_act {
        follow = Some(rows.len());
    }
    let mut buttons = div()
        .flex_none()
        .w_full()
        .pt(px(6.0))
        .flex()
        .flex_row()
        .justify_end()
        .gap(px(6.0));
    if l.paired {
        buttons = buttons.child(ui::text_button(
            "forget",
            "Forget",
            Some("Forget device".into()),
            Look::DANGER,
            St::cursor(kf(l.k_forget)),
            tk,
            cx.listener(move |d, _, _, cx| run(d, id, Act::Forget, cx)),
        ));
    }
    if m.connected_detail() {
        buttons = buttons.child(ui::text_button(
            "disconnect",
            "Disconnect",
            None,
            Look::default(),
            St::cursor(kf(l.k_act)),
            tk,
            cx.listener(move |d, _, _, cx| run(d, id, Act::Main, cx)),
        ));
    }
    if m.can_connect() {
        buttons = buttons.child(ui::text_button(
            "connect",
            "Connect",
            None,
            Look::PRIMARY,
            St::cursor(kf(l.k_act)).enabled(m.powered),
            tk,
            cx.listener(move |d, _, _, cx| run(d, id, Act::Main, cx)),
        ));
    }
    if m.can_pair() {
        buttons = buttons.child(ui::text_button(
            "pair",
            "Pair",
            None,
            Look::PRIMARY,
            St::cursor(kf(l.k_act)).enabled(m.powered),
            tk,
            cx.listener(move |d, _, _, cx| run(d, id, Act::PairNow, cx)),
        ));
    }
    rows.push(buttons.into_any_element());
    let list = v.scroll[1].list("bt-detail-list", rows, follow, 0.0, tk);
    match (on, follow) {
        (true, Some(row)) => v.last.set(Some((Page::Detail, v.scope.cursor, row))),
        _ => v.last.set(None),
    }
    list
}

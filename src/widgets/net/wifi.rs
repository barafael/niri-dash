//! The Wi-Fi widget: networks in reach, a network's details and sharing, saved networks, joining a
//! hidden network, and the password prompt. Port of `WifiWidget` and `WifiPasswordPrompt`
//! (`legacy/ui/widgets/net.slint`) with what `legacy/src/app/feeds.rs` made of the system's state.
//!
//! Pages: 0 networks, 1 one network's details, 2 join a hidden network, 3 saved networks. Each has
//! its stops, which the keyboard cursor visits (see [`L`]); the prompt, which comes over whatever
//! page is up, has stops of its own ([`P`]) and its own cursor.

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
use niri_dash_system::{self as sys, IconSrc, Tell, wifi as w};
use std::cell::Cell;

// ---- what the system reports, as the widget reads it -----------------------------------------

/// A network in reach.
#[derive(Debug, Clone)]
pub struct Net {
    pub key: String,
    pub ssid: String,
    pub signal: i32,
    pub secure: bool,
    pub active: bool,
    pub known: bool,
    /// How it is joined, when another row has the same SSID (an evil twin, say); else "".
    pub tag: String,
}

/// A saved network.
#[derive(Debug, Clone)]
pub struct Saved {
    pub key: String,
    pub ssid: String,
    /// In range, or when it was last used.
    pub detail: String,
}

/// The network the details page shows; nothing while `ssid` is empty.
#[derive(Debug, Clone, Default)]
pub struct Detail {
    pub key: String,
    pub ssid: String,
    pub active: bool,
    pub known: bool,
    pub uuid: String,
    pub autoconnect: bool,
    pub facts: Vec<(String, String)>,
}

/// The password prompt.
#[derive(Debug, Clone)]
pub struct Prompt {
    pub serial: u64,
    pub ssid: String,
    /// 0: a password, 1: a WEP key, 2: a user name and password (Enterprise).
    pub kind: i32,
    pub retry: bool,
    pub new: bool,
    pub identity: String,
    pub message: String,
}

/// Everything the widget reads of `wifi::State`.
#[derive(Debug, Clone)]
pub struct Model {
    pub available: bool,
    pub enabled: bool,
    pub blocked: bool,
    pub ssid: String,
    pub signal: i32,
    /// The key of the network being joined right now.
    pub connecting: String,
    pub nets: Vec<Net>,
    pub saved: Vec<Saved>,
    pub status: String,
    pub failed: bool,
    pub scanning: bool,
    /// The network wants a login in the browser first.
    pub portal: bool,
    pub prompt: Option<Prompt>,
    pub detail: Detail,
    pub revealed: bool,
    pub password: String,
    pub qr: IconSrc,
}

impl Model {
    pub fn of(s: &w::State) -> Self {
        use w::{Activity, PromptKind};
        let status = match &s.activity {
            Activity::Idle if !s.connecting.is_empty() => {
                format!("Connecting to {}…", s.connecting)
            }
            a => a.to_string(),
        };
        let prompt = s
            .prompt
            .as_ref()
            .map(|p| Prompt {
                serial: p.serial,
                ssid: sys::clean_label(&p.ssid),
                kind: match p.kind {
                    PromptKind::Password => 0,
                    PromptKind::Wep => 1,
                    PromptKind::Enterprise => 2,
                },
                retry: p.retry,
                new: p.new,
                identity: p.identity.clone(),
                message: p.message.clone(),
            })
            .filter(|p| !p.ssid.is_empty());
        let detail = match &s.detail {
            Some(d) => Detail {
                key: d.key.clone(),
                ssid: sys::clean_label(&d.ssid),
                active: d.network.as_ref().is_some_and(|n| n.active),
                known: d.profile.is_some(),
                uuid: d
                    .profile
                    .as_ref()
                    .map(|p| p.uuid.clone())
                    .unwrap_or_default(),
                autoconnect: d.profile.as_ref().is_some_and(|p| p.autoconnect),
                facts: w::facts(d, &s.link),
            },
            None => Detail::default(),
        };
        Self {
            available: s.available,
            enabled: s.enabled,
            blocked: s.blocked,
            ssid: sys::clean_label(&s.ssid),
            signal: s.signal,
            connecting: s.connecting_key.clone(),
            nets: s
                .networks
                .iter()
                .map(|n| {
                    let twins = s.networks.iter().filter(|m| m.ssid == n.ssid).count();
                    Net {
                        key: n.key(),
                        ssid: sys::clean_label(&n.ssid),
                        signal: n.signal,
                        secure: n.security.needs_secret(),
                        active: n.active,
                        known: n.known(),
                        tag: if twins > 1 {
                            n.security_label()
                        } else {
                            String::new()
                        },
                    }
                })
                .collect(),
            saved: s
                .saved
                .iter()
                .map(|p| {
                    let key = p.key();
                    let in_range = s.networks.iter().any(|n| n.key() == key);
                    Saved {
                        key,
                        ssid: sys::clean_label(&p.ssid),
                        detail: if in_range {
                            "in range".into()
                        } else if p.timestamp > 0 {
                            w::day(p.timestamp)
                        } else {
                            "never used".into()
                        },
                    }
                })
                .collect(),
            status,
            failed: matches!(s.activity, Activity::Failed(_)),
            scanning: s.scanning,
            portal: s.link.connectivity == w::Connectivity::Portal,
            prompt,
            detail,
            revealed: s.secret.is_some(),
            password: s
                .secret
                .as_ref()
                .map(|x| x.password.clone())
                .unwrap_or_default(),
            qr: s.secret.as_ref().map_or(IconSrc::None, |x| x.qr.clone()),
        }
    }
}

// ---- the widget's own state -----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Networks,
    Detail,
    Hidden,
    Saved,
}

/// The hidden-network form, but for the text in its fields (they hold that themselves).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hidden {
    /// 0 open, 1 WPA, 2 Enterprise.
    pub security: i32,
    /// 0 PEAP, 1 TTLS.
    pub method: i32,
    pub insecure: bool,
}

impl Default for Hidden {
    fn default() -> Self {
        Self {
            security: 1,
            method: 0,
            insecure: false,
        }
    }
}

/// The password prompt, but for the text in its fields.
#[derive(Debug, Clone, Copy, Default)]
pub struct PromptForm {
    /// The prompt it is for: a new one starts empty.
    pub serial: Option<u64>,
    pub method: i32,
    pub insecure: bool,
    pub scope: Scope,
}

/// One Wi-Fi widget on the dashboard.
#[derive(Default)]
pub struct View {
    pub page: Page,
    /// Where the details page goes back to.
    pub back_to: Page,
    pub scope: Scope,
    pub hidden: Hidden,
    pub prompt: PromptForm,
    pub pool: Option<Pool>,
    pub scroll: [Scroller; 4],
    /// The field that has the keyboard, if one does.
    pub typing: Cell<Option<&'static str>>,
    /// What was on screen as of the last frame, to see what appeared and what went.
    pub shown_prompt: Cell<Option<u64>>,
    pub shown_form: Cell<bool>,
    pub pool_requested: Cell<bool>,
    /// The row the cursor was on as of the last frame (page, stop), and whether it was in sight.
    pub last: Cell<Option<(Page, usize, usize)>>,
}

const PROMPT_FIELDS: [Spec; 3] = [
    Spec {
        name: "ident",
        placeholder: "User name",
        masked: false,
    },
    Spec {
        name: "pass",
        placeholder: "Password",
        masked: true,
    },
    Spec {
        name: "domain",
        placeholder: "Server domain, e.g. example.org",
        masked: false,
    },
];

const FORM_FIELDS: [Spec; 4] = [
    Spec {
        name: "h-ssid",
        placeholder: "Network name (SSID)",
        masked: false,
    },
    Spec {
        name: "h-ident",
        placeholder: "User name",
        masked: false,
    },
    Spec {
        name: "h-pass",
        placeholder: "Password",
        masked: true,
    },
    Spec {
        name: "h-domain",
        placeholder: "Server domain, e.g. example.org",
        masked: false,
    },
];

// ---- where the stops are ---------------------------------------------------------------------

/// The keyboard stops of the widget's pages (`k_*` are the stops of the controls on them, -1:
/// not there).
#[derive(Debug, Clone, Copy)]
pub struct L {
    pub round: bool,
    pub page: Page,
    pub k_net: isize,
    pub k_hidden: isize,
    pub has_act: bool,
    pub k_act: isize,
    pub k_share: isize,
    pub enterprise: bool,
    pub k_method: isize,
    pub k_ident: isize,
    pub k_pass: isize,
    pub k_domain: isize,
    pub k_insecure: isize,
    pub k_join: isize,
    pub count: usize,
}

impl L {
    pub fn of(round: bool, v: &View, m: &Model) -> Self {
        let h = v.hidden;
        let on_list = m.available && m.enabled;
        let k_net = isize::from(m.portal);
        let k_hidden = k_net + m.nets.len() as isize;
        let has_act = m.detail.active || !m.detail.ssid.is_empty();
        let k_act = isize::from(m.detail.known);
        let k_share = if m.detail.known {
            k_act + isize::from(has_act)
        } else {
            -1
        };
        let enterprise = h.security == 2;
        let k_method = if enterprise { 2 } else { -1 };
        let k_ident = if enterprise { 3 } else { -1 };
        let k_pass = if h.security == 0 {
            -1
        } else if enterprise {
            4
        } else {
            2
        };
        let k_domain = if enterprise && !h.insecure { 5 } else { -1 };
        let k_insecure = if enterprise {
            if h.insecure { 5 } else { 6 }
        } else {
            -1
        };
        let k_join = match (h.security, h.insecure) {
            (0, _) => 2,
            (1, _) => 3,
            (_, true) => 6,
            (_, false) => 7,
        };
        let count = if round || !m.available {
            0
        } else {
            match v.page {
                Page::Networks => {
                    if on_list {
                        (k_hidden + 1) as usize + usize::from(!m.saved.is_empty())
                    } else {
                        0
                    }
                }
                Page::Detail => {
                    (if m.detail.known { 3 } else { 0 }) + usize::from(has_act)
                }
                Page::Hidden => {
                    if m.enabled {
                        (k_join + 1) as usize
                    } else {
                        0
                    }
                }
                Page::Saved => m.saved.len(),
            }
        };
        Self {
            round,
            page: v.page,
            k_net,
            k_hidden,
            has_act,
            k_act,
            k_share,
            enterprise,
            k_method,
            k_ident,
            k_pass,
            k_domain,
            k_insecure,
            k_join,
            count,
        }
    }

    /// How many parts the stop `cursor` has: a network's row, its Disconnect (when it is the one
    /// in use) and its details.
    pub fn subs(&self, cursor: usize, m: &Model) -> usize {
        let c = cursor as isize;
        if self.page == Page::Networks && c >= self.k_net && c < self.k_hidden {
            if m.nets.get((c - self.k_net) as usize).is_some_and(|n| n.active) {
                3
            } else {
                2
            }
        } else {
            1
        }
    }

    /// The text field the stop is, if it is one (hidden-network form).
    pub fn field_at(&self, i: usize) -> Option<&'static str> {
        let i = i as isize;
        if self.page != Page::Hidden {
            return None;
        }
        if i == 0 {
            Some("h-ssid")
        } else if i == self.k_ident {
            Some("h-ident")
        } else if i == self.k_pass {
            Some("h-pass")
        } else if i == self.k_domain {
            Some("h-domain")
        } else {
            None
        }
    }

    /// The stop of the text field `name` of the form.
    pub fn stop_of(&self, name: &str) -> Option<usize> {
        let k = match name {
            "h-ssid" => 0,
            "h-ident" => self.k_ident,
            "h-pass" => self.k_pass,
            "h-domain" => self.k_domain,
            _ => -1,
        };
        usize::try_from(k).ok()
    }
}

/// The stops of the password prompt.
#[derive(Debug, Clone, Copy)]
pub struct P {
    pub enterprise: bool,
    pub new_enterprise: bool,
    pub k_method: isize,
    pub k_ident: isize,
    pub k_pass: isize,
    pub k_domain: isize,
    pub k_insecure: isize,
    pub k_cancel: isize,
    pub count: usize,
}

impl P {
    pub fn of(f: &PromptForm, p: &Prompt) -> Self {
        let enterprise = p.kind == 2;
        let new_enterprise = enterprise && p.new;
        let n = isize::from(new_enterprise);
        let k_method = if new_enterprise { 0 } else { -1 };
        let k_ident = if enterprise { n } else { -1 };
        let k_pass = n + isize::from(enterprise);
        let k_domain = if new_enterprise && !f.insecure {
            k_pass + 1
        } else {
            -1
        };
        let k_insecure = if new_enterprise {
            k_pass + if f.insecure { 1 } else { 2 }
        } else {
            -1
        };
        let k_cancel = k_pass
            + 1
            + if new_enterprise {
                if f.insecure { 1 } else { 2 }
            } else {
                0
            };
        Self {
            enterprise,
            new_enterprise,
            k_method,
            k_ident,
            k_pass,
            k_domain,
            k_insecure,
            k_cancel,
            count: (k_cancel + 2) as usize,
        }
    }

    pub fn field_at(&self, i: usize) -> Option<&'static str> {
        let i = i as isize;
        if i == self.k_ident {
            Some("ident")
        } else if i == self.k_pass {
            Some("pass")
        } else if i == self.k_domain {
            Some("domain")
        } else {
            None
        }
    }

    pub fn stop_of(&self, name: &str) -> Option<usize> {
        let k = match name {
            "ident" => self.k_ident,
            "pass" => self.k_pass,
            "domain" => self.k_domain,
            _ => -1,
        };
        usize::try_from(k).ok()
    }
}

// ---- views, and their state in the dashboard -----------------------------------------------

/// The state of the Wi-Fi widgets: what the system said, and each widget's page and cursor.
#[derive(Default)]
pub struct Data {
    pub state: w::State,
    pub views: std::collections::HashMap<u32, View>,
}

pub fn view(d: &Dash, id: u32) -> Option<&View> {
    d.widgets.net.wifi.views.get(&id)
}

pub fn view_mut(d: &mut Dash, id: u32) -> &mut View {
    d.widgets.net.wifi.views.entry(id).or_default()
}

fn model(d: &Dash) -> Model {
    Model::of(&d.widgets.net.wifi.state)
}

fn is_round(d: &Dash, id: u32) -> bool {
    d.tile(id).is_some_and(|t| t.round()) && d.nav.expanded_id != id
}

/// The system reported its state.
pub fn set(d: &mut Dash, state: w::State) {
    d.widgets.net.wifi.state = state;
    sync(d);
}

/// The stops follow the data and the layout: the cursor stays on a stop that exists, and on the
/// part of it that still means what it did.
pub fn sync(d: &mut Dash) {
    let m = model(d);
    let ids: Vec<u32> = d.widgets.net.wifi.views.keys().copied().collect();
    for id in ids {
        let round = is_round(d, id);
        let v = view_mut(d, id);
        // A new prompt starts empty.
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
        v.scope.set_subs(l.subs(v.scope.cursor, &m));
        if let Some(p) = &m.prompt {
            let p = P::of(&v.prompt, p);
            v.prompt.scope.set_count(p.count, &Plain);
        }
    }
}

/// Stops that are all stops.
struct Plain;
impl Stops for Plain {}

/// What a key or click did to the cursor beyond the widget's own state.
#[derive(Debug, Default, Clone, Copy)]
pub struct Fx {
    /// The page changed: the cursor starts at its top.
    pub page: bool,
    pub cursor: Option<usize>,
    pub prompt_cursor: Option<usize>,
}

impl Fx {
    fn apply(self, v: &mut View) {
        if self.page {
            v.scope.cursor = 0;
            v.scope.sub = 0;
        }
        if let Some(c) = self.cursor {
            v.scope.cursor = c;
            v.scope.sub = 0;
        }
        if let Some(c) = self.prompt_cursor {
            v.prompt.scope.cursor = c;
            v.prompt.scope.sub = 0;
        }
    }
}

// ---- what can be done ----------------------------------------------------------------------

/// Everything the widget can do, by mouse or keyboard.
#[derive(Debug, Clone)]
pub enum Act {
    Toggle,
    Rescan,
    SignIn,
    Connect(String),
    Disconnect,
    /// A network's row was pressed: the one in use opens its details, another is joined.
    Press(String),
    Open(String, Page),
    Back,
    OpenHidden,
    OpenSaved,
    Autoconnect,
    /// The main button of the details page: Disconnect, or Connect.
    Main,
    Share,
    Forget,
    HiddenSecurity(i32),
    HiddenMethod(i32),
    HiddenInsecure,
    HiddenJoin,
    PromptMethod(i32),
    PromptInsecure,
    PromptCancel,
    PromptSubmit,
    /// Put the keyboard in a text field.
    Edit(&'static str),
}

fn answer(identity: String, password: String, method: i32, domain: String, insecure: bool) -> w::Answer {
    w::Answer {
        identity,
        password,
        eap: w::Eap::from_index(method),
        domain,
        insecure,
    }
}

fn tell(d: &Dash, cmd: w::Command) {
    d.tx.wifi.tell(cmd);
}

/// Do `a` for the widget `id`. Returns what it did to the cursor.
pub fn act(d: &mut Dash, id: u32, a: Act, cx: &mut Context<Dash>) -> Fx {
    let m = model(d);
    let mut fx = Fx::default();
    match a {
        Act::Toggle => tell(d, w::Command::Toggle),
        Act::Rescan => tell(d, w::Command::Rescan),
        Act::SignIn => tell(d, w::Command::SignIn),
        Act::Connect(key) => tell(d, w::Command::Connect { key }),
        Act::Disconnect => tell(d, w::Command::Disconnect),
        Act::Press(key) => {
            if m.nets.iter().any(|n| n.key == key && n.active) {
                return act(d, id, Act::Open(key, Page::Networks), cx);
            }
            tell(d, w::Command::Connect { key });
        }
        Act::Open(key, from) => {
            let v = view_mut(d, id);
            v.back_to = from;
            v.page = Page::Detail;
            fx.page = true;
            tell(d, w::Command::Select { key });
        }
        Act::Back => {
            let v = view_mut(d, id);
            if v.page == Page::Detail {
                let to = v.back_to;
                v.page = to;
                tell(d, w::Command::Select { key: String::new() });
                tell(d, w::Command::HideSecret);
            } else {
                v.page = Page::Networks;
                reset_hidden(d, id, cx);
            }
            fx.page = true;
        }
        Act::OpenHidden => {
            reset_hidden(d, id, cx);
            view_mut(d, id).page = Page::Hidden;
            fx.page = true;
        }
        Act::OpenSaved => {
            view_mut(d, id).page = Page::Saved;
            fx.page = true;
        }
        Act::Autoconnect => tell(
            d,
            w::Command::SetAutoconnect {
                uuid: m.detail.uuid.clone(),
                on: !m.detail.autoconnect,
            },
        ),
        Act::Main => {
            if m.detail.active {
                tell(d, w::Command::Disconnect);
            } else if m.enabled {
                tell(
                    d,
                    w::Command::Connect {
                        key: m.detail.key.clone(),
                    },
                );
            }
        }
        Act::Share => {
            if m.revealed {
                tell(d, w::Command::HideSecret);
            } else {
                tell(
                    d,
                    w::Command::Reveal {
                        uuid: m.detail.uuid.clone(),
                    },
                );
            }
        }
        Act::Forget => {
            tell(
                d,
                w::Command::Forget {
                    uuid: m.detail.uuid.clone(),
                },
            );
            return act(d, id, Act::Back, cx);
        }
        Act::HiddenSecurity(s) => view_mut(d, id).hidden.security = s.clamp(0, 2),
        Act::HiddenMethod(s) => view_mut(d, id).hidden.method = s.clamp(0, 1),
        Act::HiddenInsecure => {
            let v = view_mut(d, id);
            v.hidden.insecure = !v.hidden.insecure;
            // The domain field comes or goes: without this, the cursor lands on Join, and
            // pressing again would join with the check off.
            let l = L::of(is_round(d, id), view(d, id).expect("a view"), &m);
            fx.cursor = Some(l.k_insecure.max(0) as usize);
        }
        Act::HiddenJoin => {
            let v = view(d, id).expect("a view");
            let (h, pool) = (v.hidden, v.pool.clone());
            let text = |n: &str| pool.as_ref().map(|p| p.value(n, cx)).unwrap_or_default();
            let ssid = text("h-ssid");
            if ssid.is_empty() {
                return fx;
            }
            let ans = answer(text("h-ident"), text("h-pass"), h.method, text("h-domain"), h.insecure);
            tell(
                d,
                w::Command::JoinHidden {
                    ssid,
                    security: h.security,
                    answer: ans,
                },
            );
            reset_hidden(d, id, cx);
            view_mut(d, id).page = Page::Networks;
            fx.page = true;
        }
        Act::PromptMethod(i) => view_mut(d, id).prompt.method = i.clamp(0, 1),
        Act::PromptInsecure => {
            let v = view_mut(d, id);
            v.prompt.insecure = !v.prompt.insecure;
            if let Some(p) = &m.prompt {
                fx.prompt_cursor = Some(P::of(&v.prompt, p).k_insecure.max(0) as usize);
            }
        }
        Act::PromptCancel => tell(d, w::Command::CancelPrompt),
        Act::PromptSubmit => {
            let Some(p) = &m.prompt else {
                return fx;
            };
            let v = view(d, id).expect("a view");
            let (f, pool) = (v.prompt, v.pool.clone());
            let text = |n: &str| pool.as_ref().map(|p| p.value(n, cx)).unwrap_or_default();
            let ans = answer(text("ident"), text("pass"), f.method_value(), text("domain"), f.insecure);
            tell(
                d,
                w::Command::Answer {
                    serial: p.serial,
                    answer: ans,
                },
            );
            // What was typed goes nowhere else.
            if let Some(pool) = pool {
                later(cx, move |_, window, cx| pool.set("pass", "", window, cx));
            }
        }
        Act::Edit(name) => focus_field(d, id, name, cx),
    }
    cx.notify();
    fx
}

impl PromptForm {
    fn method_value(&self) -> i32 {
        self.method
    }
}

/// The hidden-network form starts empty, and forgets what was typed once used or left.
fn reset_hidden(d: &mut Dash, id: u32, cx: &mut Context<Dash>) {
    let v = view_mut(d, id);
    v.hidden = Hidden::default();
    if let Some(pool) = v.pool.clone() {
        later(cx, move |_, window, cx| {
            for f in FORM_FIELDS {
                pool.set(f.name, "", window, cx);
            }
        });
    }
}

/// Put the keyboard in the text field `name`, now or when its pool is made.
fn focus_field(d: &mut Dash, id: u32, name: &'static str, cx: &mut Context<Dash>) {
    let pool = view(d, id).and_then(|v| v.pool.clone());
    later(cx, move |d, window, cx| {
        let pool = pool.or_else(|| view(d, id).and_then(|v| v.pool.clone()));
        if let Some(pool) = pool {
            pool.focus(name, window, cx);
            set_typing(d, Wid::Wifi(id), Some(name));
        }
    });
}

/// Act, and settle what it did to the cursor: for a click.
pub fn run(d: &mut Dash, id: u32, a: Act, cx: &mut Context<Dash>) {
    let fx = act(d, id, a, cx);
    fx.apply(view_mut(d, id));
    sync(d);
    cx.notify();
}

// ---- keys ----------------------------------------------------------------------------------

/// The stops of the widget's page.
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
        self.fx.cursor = fx.cursor.or(self.fx.cursor);
        self.fx.prompt_cursor = fx.prompt_cursor.or(self.fx.prompt_cursor);
    }
}

impl Stops for Main<'_, '_> {
    fn adjust(&mut self, cursor: usize, delta: i32) -> bool {
        let (_, l) = self.ctx();
        let v = view(self.d, self.id).expect("a view");
        if l.page == Page::Hidden && cursor == 1 {
            let s = v.hidden.security + delta;
            self.do_(Act::HiddenSecurity(s));
            return true;
        }
        if l.page == Page::Hidden && cursor as isize == l.k_method {
            let s = v.hidden.method + delta;
            self.do_(Act::HiddenMethod(s));
            return true;
        }
        false
    }

    fn activate(&mut self, i: usize, sub: usize) {
        let (m, l) = self.ctx();
        let ii = i as isize;
        let v = view(self.d, self.id).expect("a view");
        match l.page {
            _ if l.round => self.do_(Act::Toggle),
            Page::Networks => {
                if ii < l.k_net {
                    self.do_(Act::SignIn);
                } else if ii < l.k_hidden {
                    let n = m.nets[(ii - l.k_net) as usize].clone();
                    if sub == 0 && !n.active {
                        self.do_(Act::Connect(n.key));
                    } else if sub == 1 && n.active {
                        self.do_(Act::Disconnect);
                    } else {
                        self.do_(Act::Open(n.key, Page::Networks));
                    }
                } else if ii == l.k_hidden {
                    self.do_(Act::OpenHidden);
                } else {
                    self.do_(Act::OpenSaved);
                }
            }
            Page::Detail => {
                if m.detail.known && i == 0 {
                    self.do_(Act::Autoconnect);
                } else if ii == l.k_act && l.has_act {
                    self.do_(Act::Main);
                } else if ii == l.k_share {
                    self.do_(Act::Share);
                } else if ii == l.k_share + 1 {
                    self.do_(Act::Forget);
                }
            }
            Page::Hidden => {
                let h = v.hidden;
                if i == 1 {
                    self.do_(Act::HiddenSecurity((h.security + 1).rem_euclid(3)));
                } else if ii == l.k_method {
                    self.do_(Act::HiddenMethod(1 - h.method));
                } else if ii == l.k_insecure {
                    self.do_(Act::HiddenInsecure);
                } else if ii == l.k_join {
                    self.do_(Act::HiddenJoin);
                } else if let Some(f) = l.field_at(i) {
                    self.do_(Act::Edit(f));
                }
            }
            Page::Saved => {
                if let Some(s) = m.saved.get(i) {
                    self.do_(Act::Open(s.key.clone(), Page::Saved));
                }
            }
        }
    }

    fn letter(&mut self, key: &Key, cursor: usize) -> bool {
        let (m, l) = self.ctx();
        if l.round {
            return false;
        }
        let page0 = l.page == Page::Networks;
        if key.is('r') && page0 {
            self.do_(Act::Rescan);
            return true;
        }
        if key.is('t') && page0 {
            self.do_(Act::Toggle);
            return true;
        }
        if key.is('d') && (m.detail.active || (page0 && !m.ssid.is_empty())) {
            self.do_(Act::Disconnect);
            return true;
        }
        if key.is('i') && l.page == Page::Hidden && l.field_at(cursor).is_some() {
            if let Some(f) = l.field_at(cursor) {
                self.do_(Act::Edit(f));
            }
            return true;
        }
        false
    }

    fn back(&mut self) -> bool {
        let page = view(self.d, self.id).map(|v| v.page);
        if page.is_some_and(|p| p != Page::Networks) {
            self.do_(Act::Back);
            return true;
        }
        false
    }
}

/// The stops of the password prompt.
struct Ask<'a, 'b> {
    d: &'a mut Dash,
    cx: &'a mut Context<'b, Dash>,
    id: u32,
    fx: Fx,
}

impl Ask<'_, '_> {
    fn ctx(&self) -> Option<P> {
        let m = model(self.d);
        let v = view(self.d, self.id)?;
        m.prompt.as_ref().map(|p| P::of(&v.prompt, p))
    }

    fn do_(&mut self, a: Act) {
        let fx = act(self.d, self.id, a, self.cx);
        self.fx.page |= fx.page;
        self.fx.prompt_cursor = fx.prompt_cursor.or(self.fx.prompt_cursor);
    }
}

impl Stops for Ask<'_, '_> {
    fn adjust(&mut self, i: usize, d: i32) -> bool {
        let Some(p) = self.ctx() else {
            return false;
        };
        if i as isize != p.k_method {
            return false;
        }
        let v = view(self.d, self.id).expect("a view");
        let m = v.prompt.method + d;
        self.do_(Act::PromptMethod(m));
        true
    }

    fn activate(&mut self, i: usize, _sub: usize) {
        let Some(p) = self.ctx() else {
            return;
        };
        let ii = i as isize;
        if ii == p.k_method {
            let m = view(self.d, self.id).expect("a view").prompt.method;
            self.do_(Act::PromptMethod(1 - m));
        } else if ii == p.k_insecure {
            self.do_(Act::PromptInsecure);
        } else if ii == p.k_cancel {
            self.do_(Act::PromptCancel);
        } else if ii == p.k_cancel + 1 {
            self.do_(Act::PromptSubmit);
        } else if let Some(f) = p.field_at(i) {
            self.do_(Act::Edit(f));
        }
    }

    fn letter(&mut self, key: &Key, i: usize) -> bool {
        let Some(p) = self.ctx() else {
            return false;
        };
        if key.is('i')
            && let Some(f) = p.field_at(i)
        {
            self.do_(Act::Edit(f));
            return true;
        }
        false
    }

    fn back(&mut self) -> bool {
        self.do_(Act::PromptCancel);
        true
    }
}

/// A key for the entered widget `id`. True if it was taken.
pub fn key(d: &mut Dash, id: u32, key: &Key, cx: &mut Context<Dash>) -> bool {
    let Some(v) = view(d, id) else {
        return false;
    };
    // A text field has the keyboard: what it doesn't take is nobody's.
    if v.typing.get().is_some() {
        return false;
    }
    for s in &v.scroll {
        s.follow_again();
    }
    let m = model(d);
    let (mut scope, asking) = match &m.prompt {
        Some(_) => (v.prompt.scope, true),
        None => (v.scope, false),
    };
    let taken;
    let fx = if asking {
        let mut stops = Ask {
            d,
            cx,
            id,
            fx: Fx::default(),
        };
        taken = scope.press(key, &mut stops);
        stops.fx
    } else {
        let mut stops = Main {
            d,
            cx,
            id,
            fx: Fx::default(),
        };
        taken = scope.press(key, &mut stops);
        stops.fx
    };
    let Some(v) = d.widgets.net.wifi.views.get_mut(&id) else {
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
    true
}

/// Escape (or Backspace) in the entered widget: a prompt is cancelled, a page left.
pub fn escape(d: &mut Dash, id: u32, cx: &mut Context<Dash>) -> bool {
    if view(d, id).is_none() {
        return false;
    }
    // The keyboard was in a field: back to the cursor first.
    if view(d, id).is_some_and(|v| v.typing.get().is_some()) {
        set_typing(d, Wid::Wifi(id), None);
        later(cx, |d, window, cx| field::give_back(d, window, cx));
        return true;
    }
    let m = model(d);
    let (taken, fx) = if m.prompt.is_some() {
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

/// The text field `name` of widget `id` heard something.
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
            let round = is_round(d, id);
            let Some(v) = view_mut_opt(d, id) else {
                return;
            };
            v.typing.set(Some(name));
            // The cursor goes where the keyboard is.
            let l = L::of(round, v, &m);
            if let Some(at) = l.stop_of(name)
                && l.page == Page::Hidden
            {
                v.scope.move_to(at, &Plain);
            }
            if let Some(p) = &m.prompt
                && let Some(at) = P::of(&v.prompt, p).stop_of(name)
            {
                v.prompt.scope.move_to(at, &Plain);
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
        InputEvent::PressEnter { .. } => {
            // Only a password field answers.
            match name {
                "pass" => run(d, id, Act::PromptSubmit, cx),
                "h-pass" => run(d, id, Act::HiddenJoin, cx),
                _ => {}
            }
        }
        InputEvent::Change => cx.notify(),
    }
}

fn view_mut_opt(d: &mut Dash, id: u32) -> Option<&mut View> {
    d.widgets.net.wifi.views.get_mut(&id)
}

/// The fields of the form the field `name` is in, in the order Tab visits them.
pub fn tab_order(d: &Dash, id: u32, name: &str) -> Vec<&'static str> {
    let m = model(d);
    let Some(v) = view(d, id) else {
        return Vec::new();
    };
    if let Some(p) = &m.prompt
        && ["ident", "pass", "domain"].contains(&name)
    {
        let p = P::of(&v.prompt, p);
        return ["ident", "pass", "domain"]
            .into_iter()
            .filter(|n| p.stop_of(n).is_some())
            .collect();
    }
    let l = L::of(is_round(d, id), v, &m);
    ["h-ssid", "h-ident", "h-pass", "h-domain"]
        .into_iter()
        .filter(|n| l.stop_of(n).is_some())
        .collect()
}

/// The fields are made (in the window), or nobody asked.
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
    let specs: Vec<Spec> = PROMPT_FIELDS.iter().chain(&FORM_FIELDS).copied().collect();
    let pool = Pool::new(&specs, window, cx, on);
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

fn said_on(role: &'static str, label: impl Into<String>, on: bool) -> Said {
    Said {
        checked: Some(on),
        ..said(role, label, "")
    }
}

fn choice_name(options: &[&str], picked: i32) -> &'static str {
    // The options are the same few words each time.
    match (options, picked) {
        (["Open", ..], 0) => "Open",
        (["Open", ..], 1) => "WPA",
        (["Open", ..], _) => "802.1X",
        (_, 0) => "PEAP",
        _ => "TTLS",
    }
}

/// What the stop under the cursor of the widget `id` is, as a screen reader would say it.
pub fn spoken(d: &Dash, id: u32) -> Option<Said> {
    let v = view(d, id)?;
    let m = model(d);
    if let Some(p) = &m.prompt {
        let q = P::of(&v.prompt, p);
        let c = v.prompt.scope.cursor as isize;
        return Some(if c == q.k_method {
            said(
                "button",
                format!("Method: {}", choice_name(&["PEAP"], v.prompt.method)),
                "selected",
            )
        } else if let Some(f) = q.field_at(c as usize) {
            said("textinput", prompt_label(f, p), "")
        } else if c == q.k_insecure {
            said_on("switch", INSECURE, v.prompt.insecure)
        } else if c == q.k_cancel {
            said("button", "Cancel", "")
        } else {
            said("button", "Join", "")
        });
    }
    let l = L::of(is_round(d, id), v, &m);
    if l.count == 0 {
        return None;
    }
    let c = v.scope.cursor.min(l.count - 1);
    let ci = c as isize;
    let sub = v.scope.part();
    Some(match l.page {
        Page::Networks => {
            if ci < l.k_net {
                said("listitem", "Sign in to the network", "")
            } else if ci < l.k_hidden {
                let n = &m.nets[(ci - l.k_net) as usize];
                let subs = l.subs(c, &m);
                match sub {
                    0 => said("listitem", n.ssid.clone(), net_value(n, &m)),
                    1 if subs == 3 => said("button", format!("Disconnect from {}", n.ssid), ""),
                    _ => said("button", format!("Details of {}", n.ssid), ""),
                }
            } else if ci == l.k_hidden {
                said("listitem", "Hidden network", "")
            } else {
                said(
                    "listitem",
                    format!("Saved networks ({})", m.saved.len()),
                    "",
                )
            }
        }
        Page::Detail => {
            if m.detail.known && c == 0 {
                said_on("switch", "Connect automatically", m.detail.autoconnect)
            } else if ci == l.k_act && l.has_act {
                said(
                    "button",
                    if m.detail.active { "Disconnect" } else { "Connect" },
                    "",
                )
            } else if ci == l.k_share {
                said(
                    "button",
                    if m.revealed { "Hide password" } else { "Share network" },
                    "",
                )
            } else {
                said("button", "Forget network", "")
            }
        }
        Page::Hidden => {
            if ci == 1 {
                said(
                    "button",
                    format!(
                        "Security: {}",
                        choice_name(&["Open", "WPA", "802.1X"], v.hidden.security)
                    ),
                    "selected",
                )
            } else if ci == l.k_method {
                said(
                    "button",
                    format!("Method: {}", choice_name(&["PEAP"], v.hidden.method)),
                    "selected",
                )
            } else if let Some(f) = l.field_at(c) {
                said("textinput", form_label(f), "")
            } else if ci == l.k_insecure {
                said_on("switch", INSECURE, v.hidden.insecure)
            } else {
                said("button", "Join", "")
            }
        }
        Page::Saved => {
            let s = &m.saved[c];
            said(
                "listitem",
                s.ssid.clone(),
                format!(
                    "{}{}",
                    if s.ssid == m.ssid { "connected, " } else { "" },
                    s.detail
                ),
            )
        }
    })
}

const INSECURE: &str = "Don't check the server (unsafe)";

fn prompt_label(field: &str, p: &Prompt) -> &'static str {
    match field {
        "ident" => "User name",
        "pass" if p.kind == 1 => "WEP key",
        "pass" => "Password",
        _ => "Server domain",
    }
}

fn form_label(field: &str) -> &'static str {
    match field {
        "h-ssid" => "Network name (SSID)",
        "h-ident" => "User name",
        "h-pass" => "Password",
        _ => "Server domain",
    }
}

fn net_value(n: &Net, m: &Model) -> String {
    format!(
        "{}signal {}%, {}",
        if n.active {
            "connected, "
        } else if n.key == m.connecting {
            "joining, "
        } else if n.known {
            "saved, "
        } else {
            ""
        },
        n.signal,
        if !n.tag.is_empty() {
            n.tag.as_str()
        } else if n.secure {
            "secured"
        } else {
            "open"
        }
    )
}

// ---- invariants -----------------------------------------------------------------------------

/// What has to hold of every Wi-Fi widget whatever keys came before.
pub fn check(d: &Dash) -> Result<(), String> {
    let m = model(d);
    for (&id, v) in &d.widgets.net.wifi.views {
        let round = is_round(d, id);
        let l = L::of(round, v, &m);
        let s = &v.scope;
        let at = format!("wi-fi widget {id}");
        if l.count > 0 && s.cursor >= l.count {
            return Err(format!(
                "{at}: the cursor is on stop {} of {}",
                s.cursor, l.count
            ));
        }
        if s.sub >= s.subs.max(1) || s.subs != l.subs(s.cursor, &m).max(1) && l.count > 0 {
            return Err(format!(
                "{at}: the cursor is on part {} of a stop with {} (and {} now)",
                s.sub,
                s.subs,
                l.subs(s.cursor, &m)
            ));
        }
        if let Some(p) = &m.prompt {
            let q = P::of(&v.prompt, p);
            if v.prompt.scope.cursor >= q.count {
                return Err(format!(
                    "{at}: the cursor is on stop {} of the prompt's {}",
                    v.prompt.scope.cursor, q.count
                ));
            }
        }
        // The cursor goes where the keyboard is.
        if let Some(name) = v.typing.get() {
            let (cursor, stop) = match &m.prompt {
                Some(p) if P::of(&v.prompt, p).stop_of(name).is_some() => {
                    (v.prompt.scope.cursor, P::of(&v.prompt, p).stop_of(name))
                }
                _ => (v.scope.cursor, l.stop_of(name)),
            };
            if let Some(stop) = stop
                && stop != cursor
                && d.nav.mode == NavMode::Widget
                && d.nav.active_id == id
            {
                return Err(format!(
                    "{at}: typing in {name:?}, but the cursor is on stop {cursor}, not {stop}"
                ));
            }
        }
        // The cursor is in sight: as of the last frame, which is of this cursor.
        if let Some((page, stop, row)) = v.last.get()
            && d.nav.mode == NavMode::Widget
            && d.nav.active_id == id
            && page == v.page
            && stop == v.scope.cursor
            && m.prompt.is_none()
            && v.scroll[page as usize].sees(row) == Some(false)
        {
            return Err(format!("{at}: the cursor is out of sight on stop {stop}"));
        }
    }
    Ok(())
}

// ---- drawing -----------------------------------------------------------------------------------

/// How wide a row is, for what it leaves out when narrow.
fn row_width(ctx: &Ctx) -> f32 {
    let gutter = if ctx.tk.pad() >= 8.0 { 0.0 } else { 10.0 };
    ctx.w - 2.0 * ctx.tk.pad() - gutter
}

/// The widget of tile `ctx.tile`.
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
    let (scope, page) = (v.scope, v.page);
    let kf = move |i: isize| on && scope.cursor as isize == i;
    // What appeared and what went since the last frame, as far as the keyboard goes.
    let form_up = m.available && m.enabled && page == Page::Hidden;
    reconcile(v, &m, form_up, id, cx);

    let mut card = div()
        .size_full()
        .p(px(tk.pad()))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .overflow_hidden();
    // The header.
    card = if page == Page::Networks {
        card.child(
            ui::header(
                if m.enabled { "wifi" } else { "wifi-off" },
                if m.enabled { tk.accent() } else { tk.faint() },
                "Wi-Fi",
                tk,
            )
            .child(ui::icon_button(
                "rescan",
                "refresh",
                "Rescan",
                28.0,
                tk.text(),
                St::new().enabled(m.enabled && !m.scanning),
                tk,
                cx.listener(move |d, _, _, cx| run(d, id, Act::Rescan, cx)),
            ))
            .child(ui::switch(
                "wifi-switch",
                m.enabled,
                "Wi-Fi",
                St::new().enabled(m.available && !m.blocked),
                tk,
                cx.listener(move |d, _, _, cx| run(d, id, Act::Toggle, cx)),
            )),
        )
    } else {
        let title = match page {
            Page::Detail => m.detail.ssid.as_str(),
            Page::Hidden => "Hidden network",
            _ => "Saved networks",
        };
        card.child(ui::back_header(
            title,
            tk,
            cx.listener(move |d, _, _, cx| run(d, id, Act::Back, cx)),
        ))
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
        card = card.child(ui::empty_fill("No Wi-Fi device (NetworkManager)", tk));
    }
    if m.available && m.blocked {
        card = card.child(ui::empty_fill(
            "Wi-Fi is blocked (airplane mode or a hardware switch)",
            tk,
        ));
    }
    if m.available && !m.blocked && !m.enabled && page == Page::Networks {
        card = card.child(ui::empty_fill("Wi-Fi is off", tk));
    }
    if m.available && m.enabled && page == Page::Networks {
        card = card.child(networks(ctx, v, &m, &l, &kf, id, cx));
    }
    if m.available && page == Page::Detail {
        card = card.child(detail(ctx, v, &m, &l, &kf, id, cx));
    }
    if m.available && m.enabled && page == Page::Hidden {
        card = card.child(hidden(ctx, v, &m, &l, &kf, id, cx));
    }
    if m.available && page == Page::Saved {
        card = card.child(saved(ctx, v, &m, &kf, id, cx));
    }
    card.into_any_element()
}

/// The widget as a round tile: the switch, with the name of the network.
fn round(m: &Model, ctx: &Ctx, id: u32, cx: &mut Context<Dash>) -> AnyElement {
    let tk = ctx.tk;
    let on = m.enabled && !m.ssid.is_empty();
    let title = if !m.available {
        "No Wi-Fi".to_string()
    } else if !m.enabled {
        "Off".to_string()
    } else if !m.ssid.is_empty() {
        m.ssid.clone()
    } else {
        "Not connected".to_string()
    };
    let caption = if on {
        format!("{}%", m.signal)
    } else {
        String::new()
    };
    div()
        .id("wifi-round")
        .role(gpui::Role::Switch)
        .aria_label("Wi-Fi")
        .aria_toggled(if m.enabled {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .debug_selector(|| "Wi-Fi".into())
        .size_full()
        .cursor_pointer()
        .on_click(cx.listener(move |d, _, _, cx| run(d, id, Act::Toggle, cx)))
        .child(ui::round_face(
            if m.enabled { "wifi" } else { "wifi-off" },
            if on { tk.accent() } else { tk.faint() },
            &title,
            &caption,
            on,
            ctx.w,
            tk,
        ))
        .into_any_element()
}

/// What appeared and what went since the last frame: the fields a form needs are made, the one
/// to type in has the keyboard, and the keyboard comes back to the dashboard when its field goes.
fn reconcile(v: &View, m: &Model, form_up: bool, id: u32, cx: &mut Context<Dash>) {
    let serial = m.prompt.as_ref().map(|p| p.serial);
    let wants_pool = serial.is_some() || form_up;
    if wants_pool && v.pool.is_none() && !v.pool_requested.get() {
        v.pool_requested.set(true);
        later(cx, move |d, window, cx| {
            make_pool(d, id, window, cx);
            view_mut(d, id).pool_requested.set(false);
        });
    }
    if v.shown_prompt.get() != serial {
        v.shown_prompt.set(serial);
        if let Some(p) = &m.prompt {
            // A new prompt: its fields start empty (the user name as asked for), and the first
            // takes the keyboard.
            let (identity, enterprise) = (p.identity.clone(), p.kind == 2);
            later(cx, move |d, window, cx| {
                make_pool(d, id, window, cx);
                let Some(pool) = view(d, id).and_then(|v| v.pool.clone()) else {
                    return;
                };
                pool.set("ident", &identity, window, cx);
                pool.set("pass", "", window, cx);
                pool.set("domain", "", window, cx);
                let first = if enterprise { "ident" } else { "pass" };
                pool.focus(first, window, cx);
                set_typing(d, Wid::Wifi(id), Some(first));
                move_cursor_to(d, id, first);
            });
        }
    }
    if v.shown_form.get() != form_up {
        v.shown_form.set(form_up);
        if form_up {
            later(cx, move |d, window, cx| {
                make_pool(d, id, window, cx);
                let Some(pool) = view(d, id).and_then(|v| v.pool.clone()) else {
                    return;
                };
                pool.focus("h-ssid", window, cx);
                set_typing(d, Wid::Wifi(id), Some("h-ssid"));
                move_cursor_to(d, id, "h-ssid");
            });
        }
    }
    // A field with the keyboard that isn't on screen any more: the keyboard comes back.
    let prompt_up = serial.is_some();
    if let Some(name) = v.typing.get() {
        let there = if name.starts_with("h-") { form_up } else { prompt_up };
        if !there {
            v.typing.set(None);
            later(cx, |d, window, cx| field::give_back(d, window, cx));
        }
    }
}

/// The cursor goes to the stop of the text field `name`.
pub(super) fn move_cursor_to(d: &mut Dash, id: u32, name: &str) {
    let m = model(d);
    let round = is_round(d, id);
    let Some(v) = view_mut_opt(d, id) else {
        return;
    };
    let l = L::of(round, v, &m);
    if let Some(at) = l.stop_of(name)
        && l.page == Page::Hidden
    {
        v.scope.move_to(at, &Plain);
    }
    if let Some(p) = &m.prompt
        && let Some(at) = P::of(&v.prompt, p).stop_of(name)
    {
        v.prompt.scope.move_to(at, &Plain);
    }
}

/// A key that reached a text field's frame.
fn field_key(
    d: &mut Dash,
    id: u32,
    name: &'static str,
    ev: &KeyDownEvent,
    window: &mut Window,
    cx: &mut Context<Dash>,
) {
    super::field_key(d, Wid::Wifi(id), name, ev, window, cx);
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

/// The prompt for a network's password: asked by the widget for a new network, or by
/// NetworkManager (through the secret agent) for a saved one.
fn prompt(
    ctx: &Ctx,
    v: &View,
    p: &Prompt,
    id: u32,
    cx: &mut Context<Dash>,
) -> AnyElement {
    let tk = ctx.tk;
    let q = P::of(&v.prompt, p);
    let on = ctx.entered;
    let cursor = v.prompt.scope.cursor as isize;
    let kf = move |i: isize| on && cursor == i;
    let f = v.prompt;
    let title = format!(
        "{}{}",
        if p.retry {
            "Wrong password for "
        } else if q.enterprise {
            "Sign in to "
        } else {
            "Password for "
        },
        p.ssid
    );
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
                .truncate()
                .text_size(px(tk.font_small()))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(if p.retry { tk.warn() } else { tk.text() })
                .child(title),
        );
    if q.new_enterprise {
        col = col.child(ui::choice(
            "prompt-method",
            "Method",
            &["PEAP", "TTLS"],
            f.method,
            kf(q.k_method),
            tk,
            {
                let l = std::rc::Rc::new(
                    cx.listener(move |d, i: &i32, _, cx| run(d, id, Act::PromptMethod(*i), cx)),
                );
                move |i, window, cx| l(&i, window, cx)
            },
        ));
    }
    if q.enterprise {
        col = col.child(field(
            v,
            id,
            "ident",
            "User name",
            "User name",
            false,
            kf(q.k_ident),
            tk,
            cx,
        ));
    }
    let (label, placeholder) = if p.kind == 1 {
        ("WEP key", "WEP key")
    } else {
        ("Password", "Password")
    };
    col = col.child(field(
        v,
        id,
        "pass",
        label,
        placeholder,
        true,
        kf(q.k_pass),
        tk,
        cx,
    ));
    if q.new_enterprise {
        col = col.child(server_check(
            v,
            id,
            "domain",
            f.insecure,
            kf(q.k_domain),
            kf(q.k_insecure),
            Act::PromptInsecure,
            tk,
            cx,
        ));
    }
    if !p.message.is_empty() {
        col = col.child(
            div()
                .text_size(px(tk.font_small()))
                .text_color(tk.bad())
                .child(p.message.clone()),
        );
    }
    col.child(
        div()
            .flex()
            .flex_row()
            .justify_end()
            .gap(px(6.0))
            .child(ui::text_button(
                "prompt-cancel",
                "Cancel",
                None,
                Look::default(),
                St::cursor(kf(q.k_cancel)),
                tk,
                cx.listener(move |d, _, _, cx| run(d, id, Act::PromptCancel, cx)),
            ))
            .child(ui::text_button(
                "prompt-join",
                "Join",
                None,
                Look::PRIMARY,
                St::cursor(kf(q.k_cancel + 1)),
                tk,
                cx.listener(move |d, _, _, cx| run(d, id, Act::PromptSubmit, cx)),
            )),
    )
    .into_any_element()
}

/// For a new Enterprise network: the domain the server's certificate must be issued for, or an
/// explicit "don't check" (an evil twin could then take the password).
#[allow(clippy::too_many_arguments)]
fn server_check(
    v: &View,
    id: u32,
    domain: &'static str,
    insecure: bool,
    kf_domain: bool,
    kf_switch: bool,
    toggle: Act,
    tk: &Tk,
    cx: &mut Context<Dash>,
) -> AnyElement {
    div()
        .flex_none()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .when(!insecure, |c| {
            c.child(field(
                v,
                id,
                domain,
                "Server domain",
                "Server domain, e.g. example.org",
                false,
                kf_domain,
                tk,
                cx,
            ))
        })
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(tk.font_small()))
                        .text_color(if insecure { tk.warn() } else { tk.dim() })
                        .child(INSECURE),
                )
                .child(ui::switch(
                    SharedString::from(format!("insecure-{domain}")),
                    insecure,
                    INSECURE,
                    St::cursor(kf_switch),
                    tk,
                    cx.listener(move |d, _, _, cx| run(d, id, toggle.clone(), cx)),
                )),
        )
        .into_any_element()
}

/// The networks around, then the ways to the other pages.
fn networks(
    ctx: &Ctx,
    v: &View,
    m: &Model,
    l: &L,
    kf: &impl Fn(isize) -> bool,
    id: u32,
    cx: &mut Context<Dash>,
) -> AnyElement {
    let tk = ctx.tk;
    let cursor = v.scope.cursor as isize;
    let sub = v.scope.part();
    let on = ctx.entered && m.prompt.is_none();
    let rw = row_width(ctx);
    let (narrow, tiny) = (rw < 150.0, rw < 135.0);
    let mut rows: Vec<AnyElement> = Vec::new();
    let mut follow = None;
    let mut mark = |rows: &Vec<AnyElement>, here: bool| {
        if here {
            follow = Some(rows.len());
        }
    };
    if m.portal {
        mark(&rows, kf(0));
        let warn = tk.warn();
        rows.push(
            ui::row(
                "sign-in",
                "Sign in to the network",
                "",
                St::cursor(kf(0)),
                tk,
                cx.listener(move |d, _, _, cx| run(d, id, Act::SignIn, cx)),
            )
            .bg(crate::kit::with_alpha(warn, if kf(0) { 0.3 } else { 0.18 }))
            .px(px(10.0))
            .gap(px(8.0))
            .child(crate::kit::icon("globe", 16.0, warn))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(tk.font_body()))
                    .text_color(tk.text())
                    .child("Sign in to the network"),
            )
            .into_any_element(),
        );
    }
    for (i, n) in m.nets.iter().enumerate() {
        let stop = l.k_net + i as isize;
        let here = on && cursor == stop;
        mark(&rows, here);
        let key = n.key.clone();
        let ssid = n.ssid.clone();
        let tint = if n.active { tk.accent() } else { tk.text() };
        let mut row = ui::row(
            SharedString::from(format!("net-{i}")),
            n.ssid.clone(),
            net_value(n, m),
            St::cursor(here && sub == 0).active(n.active),
            tk,
            {
                let key = key.clone();
                cx.listener(move |d, _, _, cx| run(d, id, Act::Press(key.clone()), cx))
            },
        )
        .pl(px(if narrow { 6.0 } else { 10.0 }))
        .pr(px(2.0))
        .gap(px(if narrow { 4.0 } else { 8.0 }));
        if !tiny {
            row = row.child(ui::signal_bars(n.signal, tint, tk));
        }
        row = row.child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(tk.font_body()))
                .font_weight(if n.known {
                    gpui::FontWeight::SEMIBOLD
                } else {
                    gpui::FontWeight::NORMAL
                })
                .text_color(tint)
                .child(n.ssid.clone()),
        );
        if n.key == m.connecting && !narrow {
            row = row.child(ui::muted("joining…", tk).flex_none());
        }
        if !n.tag.is_empty() && !narrow {
            row = row.child(
                ui::muted(n.tag.clone(), tk)
                    .flex_none()
                    .text_color(if n.secure { tk.dim() } else { tk.warn() }),
            );
        }
        if n.secure && !narrow {
            row = row.child(crate::kit::icon("lock-small", 14.0, tk.faint()));
        }
        if n.active {
            row = row.child(ui::icon_button(
                SharedString::from(format!("disconnect-{i}")),
                "close",
                format!("Disconnect from {ssid}"),
                26.0,
                tk.text(),
                St::cursor(here && sub == 1),
                tk,
                cx.listener(move |d, _, _, cx| run(d, id, Act::Disconnect, cx)),
            ));
        }
        row = row.child(ui::icon_button(
            SharedString::from(format!("details-{i}")),
            "chevron-right",
            format!("Details of {}", n.ssid),
            26.0,
            tk.dim(),
            St::cursor(here && sub == if n.active { 2 } else { 1 }),
            tk,
            {
                let key = key.clone();
                cx.listener(move |d, _, _, cx| run(d, id, Act::Open(key.clone(), Page::Networks), cx))
            },
        ));
        rows.push(row.into_any_element());
    }
    if m.nets.is_empty() {
        rows.push(ui::empty_line(
            if m.scanning { "Scanning…" } else { "No networks found" },
            48.0,
            tk,
        ));
    }
    let k_hidden = l.k_hidden;
    mark(&rows, on && cursor == k_hidden);
    rows.push(
        ui::row(
            "hidden",
            "Hidden network",
            "",
            St::cursor(on && cursor == k_hidden),
            tk,
            cx.listener(move |d, _, _, cx| run(d, id, Act::OpenHidden, cx)),
        )
        .pl(px(10.0))
        .gap(px(8.0))
        .child(crate::kit::icon("plus", 16.0, tk.dim()))
        .child(
            div()
                .flex_1()
                .text_size(px(tk.font_body()))
                .text_color(tk.dim())
                .child("Hidden network…"),
        )
        .into_any_element(),
    );
    if !m.saved.is_empty() {
        mark(&rows, on && cursor == k_hidden + 1);
        let label = format!("Saved networks ({})", m.saved.len());
        rows.push(
            ui::row(
                "saved",
                label.clone(),
                "",
                St::cursor(on && cursor == k_hidden + 1),
                tk,
                cx.listener(move |d, _, _, cx| run(d, id, Act::OpenSaved, cx)),
            )
            .pl(px(10.0))
            .pr(px(2.0))
            .gap(px(8.0))
            .child(crate::kit::icon("check", 16.0, tk.dim()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(tk.font_body()))
                    .text_color(tk.dim())
                    .child(label),
            )
            .child(crate::kit::icon("chevron-right", 16.0, tk.dim()))
            .into_any_element(),
        );
    }
    let list = v.scroll[0].list("net-list", rows, follow, 0.0, tk);
    record(v, Page::Networks, on, follow);
    list
}

/// Remember whether the cursor's row is in sight, for the stress run.
fn record(v: &View, page: Page, on: bool, follow: Option<usize>) {
    match (on, follow) {
        (true, Some(row)) => v.last.set(Some((page, v.scope.cursor, row))),
        _ => v.last.set(None),
    }
}

/// One network: what NetworkManager knows about it, and what can be done with it.
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
    let det = &m.detail;
    let on = ctx.entered && m.prompt.is_none();
    let cursor = v.scope.cursor as isize;
    let mut rows: Vec<AnyElement> = Vec::new();
    let mut follow = None;
    if det.ssid.is_empty() {
        rows.push(ui::empty_line("This network is gone.", 48.0, tk));
    }
    for (label, value) in &det.facts {
        rows.push(ui::fact(label, value, tk));
    }
    if det.known {
        if kf(0) {
            follow = Some(rows.len());
        }
        rows.push(ui::switch_line(
            "autoconnect",
            "Connect automatically",
            det.autoconnect,
            St::cursor(kf(0)),
            tk,
            cx.listener(move |d, _, _, cx| run(d, id, Act::Autoconnect, cx)),
        ));
    }
    if det.known && m.revealed {
        let side = 160.0f32.min(ctx.w - 2.0 * tk.pad());
        let mut block = div()
            .flex_none()
            .w_full()
            .pt(px(4.0))
            .flex()
            .flex_col()
            .gap(px(6.0));
        if !m.password.is_empty() {
            block = block.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .w(gpui::relative(0.34))
                            .flex_none()
                            .truncate()
                            .text_size(px(tk.font_small()))
                            .text_color(tk.dim())
                            .child("Password"),
                    )
                    .child(
                        // The password itself is never read out.
                        div()
                            .id("shown-password")
                            .role(gpui::Role::Label)
                            .aria_label("Shown on screen, not read out")
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_right()
                            .text_size(px(tk.font_body()))
                            .text_color(tk.text())
                            .child(m.password.clone()),
                    ),
            );
        }
        block = block
            .child(
                div()
                    .flex()
                    .justify_center()
                    .child(ui::qr(&m.qr, side.max(0.0))),
            )
            .child(ui::muted("Scan with a phone camera to join.", tk).text_center());
        rows.push(block.into_any_element());
    }
    // The keyboard cursor comes to these right to left: the main one first.
    let buttons_here = on && cursor >= l.k_act;
    if buttons_here {
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
    if det.known {
        buttons = buttons.child(ui::text_button(
            "forget",
            "Forget",
            Some("Forget network".into()),
            Look::DANGER,
            St::cursor(kf(l.k_share + 1)),
            tk,
            cx.listener(move |d, _, _, cx| run(d, id, Act::Forget, cx)),
        ));
        buttons = buttons.child(ui::text_button(
            "share",
            if m.revealed { "Hide" } else { "Share" },
            Some(if m.revealed { "Hide password" } else { "Share network" }.into()),
            Look::default(),
            St::cursor(kf(l.k_share)),
            tk,
            cx.listener(move |d, _, _, cx| run(d, id, Act::Share, cx)),
        ));
    }
    if det.active {
        buttons = buttons.child(ui::text_button(
            "disconnect",
            "Disconnect",
            None,
            Look::default(),
            St::cursor(kf(l.k_act)),
            tk,
            cx.listener(move |d, _, _, cx| run(d, id, Act::Disconnect, cx)),
        ));
    } else if !det.ssid.is_empty() {
        let key = det.key.clone();
        buttons = buttons.child(ui::text_button(
            "connect",
            "Connect",
            None,
            Look::PRIMARY,
            St::cursor(kf(l.k_act)).enabled(m.enabled),
            tk,
            cx.listener(move |d, _, _, cx| run(d, id, Act::Connect(key.clone()), cx)),
        ));
    }
    rows.push(buttons.into_any_element());
    let list = v.scroll[1].list("detail-list", rows, follow, 0.0, tk);
    record(v, Page::Detail, on, follow);
    list
}

/// A network that doesn't broadcast its name.
fn hidden(
    ctx: &Ctx,
    v: &View,
    m: &Model,
    l: &L,
    kf: &impl Fn(isize) -> bool,
    id: u32,
    cx: &mut Context<Dash>,
) -> AnyElement {
    let tk = ctx.tk;
    let h = v.hidden;
    let on = ctx.entered && m.prompt.is_none();
    let mut rows: Vec<AnyElement> = Vec::new();
    let mut follow = None;
    let mut mark = |rows: &Vec<AnyElement>, here: bool| {
        if here {
            follow = Some(rows.len());
        }
    };
    mark(&rows, kf(0));
    rows.push(field(
        v,
        id,
        "h-ssid",
        "Network name (SSID)",
        "Network name (SSID)",
        false,
        kf(0),
        tk,
        cx,
    ));
    mark(&rows, kf(1));
    rows.push(ui::choice(
        "security",
        "Security",
        &["Open", "WPA", "802.1X"],
        h.security,
        kf(1),
        tk,
        {
            let ls = std::rc::Rc::new(
                cx.listener(move |d, i: &i32, _, cx| run(d, id, Act::HiddenSecurity(*i), cx)),
            );
            move |i, window, cx| ls(&i, window, cx)
        },
    ));
    if l.enterprise {
        mark(&rows, kf(l.k_method));
        rows.push(ui::choice(
            "method",
            "Method",
            &["PEAP", "TTLS"],
            h.method,
            kf(l.k_method),
            tk,
            {
                let ls = std::rc::Rc::new(
                    cx.listener(move |d, i: &i32, _, cx| run(d, id, Act::HiddenMethod(*i), cx)),
                );
                move |i, window, cx| ls(&i, window, cx)
            },
        ));
        mark(&rows, kf(l.k_ident));
        rows.push(field(
            v,
            id,
            "h-ident",
            "User name",
            "User name",
            false,
            kf(l.k_ident),
            tk,
            cx,
        ));
    }
    if h.security != 0 {
        mark(&rows, kf(l.k_pass));
        rows.push(field(
            v,
            id,
            "h-pass",
            "Password",
            "Password",
            true,
            kf(l.k_pass),
            tk,
            cx,
        ));
    }
    if l.enterprise {
        // The domain field and the switch are two stops.
        if !h.insecure {
            mark(&rows, kf(l.k_domain));
            rows.push(field(
                v,
                id,
                "h-domain",
                "Server domain",
                "Server domain, e.g. example.org",
                false,
                kf(l.k_domain),
                tk,
                cx,
            ));
        }
        mark(&rows, kf(l.k_insecure));
        rows.push(
            div()
                .flex_none()
                .w_full()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(tk.font_small()))
                        .text_color(if h.insecure { tk.warn() } else { tk.dim() })
                        .child(INSECURE),
                )
                .child(ui::switch(
                    "insecure-hidden",
                    h.insecure,
                    INSECURE,
                    St::cursor(kf(l.k_insecure)),
                    tk,
                    cx.listener(move |d, _, _, cx| run(d, id, Act::HiddenInsecure, cx)),
                ))
                .into_any_element(),
        );
    }
    mark(&rows, kf(l.k_join));
    let named = v
        .pool
        .as_ref()
        .is_some_and(|p| !p.value("h-ssid", cx).is_empty());
    rows.push(
        div()
            .flex_none()
            .w_full()
            .flex()
            .flex_row()
            .justify_end()
            .child(ui::text_button(
                "join",
                "Join",
                None,
                Look::PRIMARY,
                St::cursor(kf(l.k_join)).enabled(named),
                tk,
                cx.listener(move |d, _, _, cx| run(d, id, Act::HiddenJoin, cx)),
            ))
            .into_any_element(),
    );
    let list = v.scroll[2].list("hidden-list", rows, follow, 6.0, tk);
    record(v, Page::Hidden, on, follow);
    list
}

/// Every saved network, last used first; in range or not.
fn saved(
    ctx: &Ctx,
    v: &View,
    m: &Model,
    kf: &impl Fn(isize) -> bool,
    id: u32,
    cx: &mut Context<Dash>,
) -> AnyElement {
    let tk = ctx.tk;
    let on = ctx.entered && m.prompt.is_none();
    let mut rows = Vec::new();
    let mut follow = None;
    for (i, s) in m.saved.iter().enumerate() {
        if kf(i as isize) {
            follow = Some(i);
        }
        let key = s.key.clone();
        let here = s.ssid == m.ssid;
        rows.push(
            ui::row(
                SharedString::from(format!("saved-{i}")),
                s.ssid.clone(),
                format!("{}{}", if here { "connected, " } else { "" }, s.detail),
                St::cursor(kf(i as isize)),
                tk,
                cx.listener(move |d, _, _, cx| {
                    run(d, id, Act::Open(key.clone(), Page::Saved), cx)
                }),
            )
            .pl(px(10.0))
            .pr(px(2.0))
            .gap(px(8.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(tk.font_body()))
                    .text_color(if here { tk.accent() } else { tk.text() })
                    .child(s.ssid.clone()),
            )
            .child(ui::muted(s.detail.clone(), tk).flex_none())
            .child(crate::kit::icon("chevron-right", 16.0, tk.dim()))
            .into_any_element(),
        );
    }
    let list = v.scroll[3].list("saved-list", rows, follow, 0.0, tk);
    record(v, Page::Saved, on, follow);
    list
}


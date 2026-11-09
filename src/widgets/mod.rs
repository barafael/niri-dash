//! The widgets, in groups (one module each, owned by whoever ports it), and the registry that
//! routes to them: what each kind draws, what the system's events do to it, and what the keys do
//! inside it.
//!
//! A group module has the same hooks (see any of them; `time.rs` is the model): `State`,
//! `handles`, `render`, `event`, `key`, `escape`, `tick`, `init`, `come_up`, `go_down`, `awake`,
//! `entered`, `layout_changed`, `describe`, `check`. This file calls each group's hook in turn
//! (`groups!`), so a group is added or changed without touching the others.
//!
//! Edit mode is `edit` (frames' handles, the drawer, the settings card, edit keys).

use crate::Control;
use crate::dash::{Dash, NavMode, Tile};
use crate::keys::Key;
use crate::kit::{self, Tk};
use crate::layout::Kind;
use gpui::{AnyElement, Context};
use niri_dash_system::Event;

mod controls;
mod desk;
mod devices;
mod extras;
mod monitors;
mod net;
mod time;
mod tools;

/// What a widget draws with.
pub struct Ctx<'a> {
    pub d: &'a Dash,
    pub tk: &'a Tk,
    pub tile: &'a Tile,
    /// The card's size in pixels.
    pub w: f32,
    pub h: f32,
    /// Drawn as a circle.
    pub round: bool,
    /// Entered with the keyboard: the widget shows its cursor.
    pub entered: bool,
    /// The widget's own settings.
    pub opts: Opts,
}

/// A widget's settings (config.kdl children, defaults filled in), as the views read them.
#[derive(Debug, Clone)]
pub struct Opts {
    // Clock.
    pub hours12: bool,
    pub seconds: bool,
    pub date: bool,
    // Calendar.
    pub sunday_first: bool,
    pub week_numbers: bool,
    // Charts: how many one-second samples to show.
    pub history: i64,
    pub cores: bool,
    pub top_apps: i64,
    pub hide_system: bool,
    // Session buttons.
    pub lock: bool,
    pub suspend: bool,
    pub logout: bool,
    pub restart: bool,
    pub poweroff: bool,
    pub confirm: bool,
    // Mascot.
    pub character: crate::mascots::Character,
    pub cowfile: String,
}

impl Opts {
    pub fn of(p: &crate::layout::Placed, us: bool) -> Self {
        use crate::widget_settings::{Opt, get_bool, get_int, get_str};
        let v = &p.settings;
        let b = |o| get_bool(v, o, us);
        Self {
            hours12: get_int(v, Opt::Hours, us) == 12,
            seconds: b(Opt::Seconds),
            date: b(Opt::Date),
            sunday_first: get_str(v, Opt::FirstDay, us) == "sunday",
            week_numbers: b(Opt::WeekNumbers),
            history: get_int(v, Opt::History, us),
            cores: b(Opt::Cores),
            top_apps: get_int(v, Opt::TopApps, us),
            hide_system: b(Opt::HideSystem),
            lock: b(Opt::Lock),
            suspend: b(Opt::Suspend),
            logout: b(Opt::Logout),
            restart: b(Opt::Restart),
            poweroff: b(Opt::PowerOff),
            confirm: b(Opt::Confirm),
            character: get_str(v, Opt::Character, us)
                .parse()
                .expect("the setting only takes character names"),
            cowfile: get_str(v, Opt::Cowfile, us),
        }
    }
}

impl<'a> Ctx<'a> {
    pub fn new(d: &'a Dash, tk: &'a Tk, tile: &'a Tile, w: f32, h: f32) -> Self {
        Self {
            d,
            tk,
            tile,
            w,
            h,
            round: tile.round() && d.nav.expanded_id != tile.id(),
            entered: d.nav.mode == NavMode::Widget && d.nav.active_id == tile.id(),
            opts: Opts::of(&tile.placed, d.locale.us),
        }
    }

    pub fn kind(&self) -> Kind {
        self.tile.kind()
    }
}

/// The state of every group of widgets.
#[derive(Default)]
pub struct Widgets {
    pub time: time::State,
    pub monitors: monitors::State,
    pub controls: controls::State,
    pub net: net::State,
    pub tools: tools::State,
    pub desk: desk::State,
    pub devices: devices::State,
    pub extras: extras::State,
}

/// Calls `$hook` of every group, in order.
macro_rules! groups {
    ($hook:ident($($arg:expr),*)) => {{
        time::$hook($($arg),*);
        monitors::$hook($($arg),*);
        controls::$hook($($arg),*);
        net::$hook($($arg),*);
        tools::$hook($($arg),*);
        desk::$hook($($arg),*);
        devices::$hook($($arg),*);
        extras::$hook($($arg),*);
    }};
}

/// The first group that has `$hook`'s answer (`Some`).
macro_rules! first {
    ($hook:ident($($arg:expr),*)) => {
        time::$hook($($arg),*)
            .or_else(|| monitors::$hook($($arg),*))
            .or_else(|| controls::$hook($($arg),*))
            .or_else(|| net::$hook($($arg),*))
            .or_else(|| tools::$hook($($arg),*))
            .or_else(|| desk::$hook($($arg),*))
            .or_else(|| devices::$hook($($arg),*))
            .or_else(|| extras::$hook($($arg),*))
    };
}

/// The dashboard is built: what groups restore from the saved state (timers that were running).
pub fn init(d: &mut Dash) {
    groups!(init(d));
}

/// The tiles changed: what a widget keeps per instance (notes, mascots) follows.
pub fn layout_changed(d: &mut Dash) {
    groups!(layout_changed(d));
}

/// The dashboard came up or went away: notes, timers and mascots only run while seen.
pub fn awake(d: &mut Dash, on: bool) {
    groups!(awake(d, on));
}

/// The dashboard opens: every widget back on its first page.
pub fn come_up(d: &mut Dash) {
    groups!(come_up(d));
}

/// The dashboard goes away: menus shut.
pub fn go_down(d: &mut Dash) {
    groups!(go_down(d));
}

/// A second passed while the dashboard is up.
pub fn tick(d: &mut Dash) {
    groups!(tick(d));
}

/// Widget `id` was entered with the keyboard.
pub fn entered(d: &mut Dash, id: u32) {
    if let Some(kind) = d.tile(id).map(Tile::kind) {
        groups!(entered(d, kind, id));
    }
}

/// What the system reports. Returns a control if the event asks for one (a color was picked, and
/// the dashboard may now show itself again).
pub fn event(d: &mut Dash, event: Event) -> Option<Control> {
    match &event {
        Event::Toast(text) => {
            d.toast(text);
            return None;
        }
        _ => {}
    }
    match first!(event(d, &event)) {
        Some(control) => control,
        None => {
            log::trace!("not shown: {event:?}");
            None
        }
    }
}

/// Draw the widget of `ctx.tile`.
pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let kind = ctx.kind();
    if time::handles(kind) {
        time::render(ctx, cx)
    } else if monitors::handles(kind) {
        monitors::render(ctx, cx)
    } else if controls::handles(kind) {
        controls::render(ctx, cx)
    } else if net::handles(kind) {
        net::render(ctx, cx)
    } else if tools::handles(kind) {
        tools::render(ctx, cx)
    } else if desk::handles(kind) {
        desk::render(ctx, cx)
    } else if devices::handles(kind) {
        devices::render(ctx, cx)
    } else if extras::handles(kind) {
        extras::render(ctx, cx)
    } else {
        placeholder(ctx, kind)
    }
}

/// A kind that isn't ported yet.
pub(crate) fn placeholder(ctx: &Ctx, kind: Kind) -> AnyElement {
    kit::empty(format!("{} …", kind.info().title), ctx.tk)
}

/// A key for the entered widget. True if it took it.
pub fn widget_key(d: &mut Dash, key: &Key, cx: &mut Context<Dash>) -> bool {
    let id = d.nav.active_id;
    let Some(kind) = d.tile(id).map(Tile::kind) else {
        return false;
    };
    time::handles(kind) && time::key(d, kind, id, key, cx)
        || monitors::handles(kind) && monitors::key(d, kind, id, key, cx)
        || controls::handles(kind) && controls::key(d, kind, id, key, cx)
        || net::handles(kind) && net::key(d, kind, id, key, cx)
        || tools::handles(kind) && tools::key(d, kind, id, key, cx)
        || desk::handles(kind) && desk::key(d, kind, id, key, cx)
        || devices::handles(kind) && devices::key(d, kind, id, key, cx)
        || extras::handles(kind) && extras::key(d, kind, id, key, cx)
}

/// What the entered widget would say about where its cursor is, for the stress run's log.
pub fn describe(d: &Dash) -> Option<String> {
    let kind = d.tile(d.nav.active_id)?.kind();
    first!(describe(d, kind))
}

/// What has to hold of the widgets whatever keys came before (the stress run checks it after
/// every key): the cursor is on a stop that exists, and so on. An error says what doesn't add up.
pub fn check(d: &Dash) -> Result<(), String> {
    time::check(d)?;
    monitors::check(d)?;
    controls::check(d)?;
    net::check(d)?;
    tools::check(d)?;
    desk::check(d)?;
    devices::check(d)?;
    extras::check(d)
}

/// Escape inside a widget or one of its menus, before the dashboard takes it: true if something
/// closed.
pub fn escape(d: &mut Dash, cx: &mut Context<Dash>) -> bool {
    time::escape(d, cx)
        || monitors::escape(d, cx)
        || controls::escape(d, cx)
        || net::escape(d, cx)
        || tools::escape(d, cx)
        || desk::escape(d, cx)
        || devices::escape(d, cx)
        || extras::escape(d, cx)
}

/// Do not disturb on or off (`niri-dash dnd`, the stress run): the notifications widget's.
pub fn toggle_dnd(d: &mut Dash) {
    desk::toggle_dnd(d);
}

/// `niri-dash night-light …` arrived on the control socket: `controls`' night light's.
pub fn night_light_command(
    _d: &mut Dash,
    _cmd: niri_dash_system::night_light::Cmd,
    _cx: &mut Context<Dash>,
) {
}

/// `niri-dash say …` arrived on the control socket: `extras`' mascots'.
pub fn mascot_say(d: &mut Dash, say: crate::mascots::Say, cx: &mut Context<Dash>) {
    extras::mascot_say(d, say, cx);
}

/// What a group draws over the whole window, above the frames (the tray's menu): in order.
pub fn overlays(_d: &Dash, _tk: &Tk, _cx: &mut Context<Dash>) -> Vec<AnyElement> {
    Vec::new()
}

// ---- edit mode: replaced by the edit mode port ---------------------------------------------

/// A key in edit mode.
pub fn edit_key(_d: &mut Dash, _key: &Key, _cx: &mut Context<Dash>) -> bool {
    false
}

/// Move widget `id` one cell (Ctrl+Shift+h/j/k/l).
pub fn nudge(_d: &mut Dash, _id: u32, _dir: crate::nav::Dir, _cx: &mut Context<Dash>) {}

/// Open widget `id`'s settings (`s`).
pub fn open_settings(_d: &mut Dash, _id: u32, _cx: &mut Context<Dash>) {}

/// Pick widget `id` to work on in edit mode.
pub fn configure(_d: &mut Dash, _id: u32, _cx: &mut Context<Dash>) {}

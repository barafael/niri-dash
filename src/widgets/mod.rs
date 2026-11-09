//! The widgets, one module per kind (or per few related ones), and the registry that routes to
//! them: what each kind draws, what the system's events do to it, and what the keys do inside it.
//!
//! A widget module has:
//! - a `State` (Default) in [`Widgets`]: everything it keeps that isn't the layout or the look.
//!   Several instances of a kind share it (two clocks show the same time);
//! - `fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement`: what it draws, in a card of
//!   `ctx.w` by `ctx.h` pixels (`ctx.round`: as a circle);
//! - `fn event(d: &mut Dash, e: &sys::Event) -> bool` for the system's reports it listens to;
//! - `fn key(d: &mut Dash, tile_id: u32, key: &Key, cx) -> bool` for the keys of an entered widget,
//!   usually by way of a [`crate::scope::Scope`].
//!
//! Adding a kind: add its module, its `State` to [`Widgets`], and a line to each of the `match`es
//! below.

use crate::dash::{Dash, Tile};
use crate::keys::Key;
use crate::kit::{self, Tk};
use crate::layout::Kind;
use gpui::{AnyElement, Context};
use niri_dash_system::{self as sys, Event, Senders};

mod clock;

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
            entered: d.nav.mode == crate::dash::NavMode::Widget && d.nav.active_id == tile.id(),
            opts: Opts::of(&tile.placed, d.locale.us),
        }
    }

    pub fn kind(&self) -> Kind {
        self.tile.kind()
    }

}

/// The state of every kind of widget.
#[derive(Default)]
pub struct Widgets {
    pub clock: clock::State,
}

impl Widgets {
    /// As the daemon left them: timers that were running, and so on.
    pub fn restored(_state: &crate::state::State) -> Self {
        Self::default()
    }

    /// The tiles changed: what a widget keeps per instance (notes, mascots) follows.
    pub fn layout_changed(&mut self, _tiles: &[Tile]) {}

    /// The dashboard came up or went away: notes, timers and mascots only run while seen.
    pub fn awake(&mut self, _on: bool, _tx: &Senders) {}

    /// The dashboard opens: every widget back on its first page.
    pub fn come_up(&mut self) {
        self.clock.come_up();
    }

    /// The dashboard goes away: menus shut.
    pub fn go_down(&mut self) {}

    /// Widget `id` was entered.
    pub fn entered(&mut self, _id: u32, _tiles: &[Tile]) {}

    /// Do not disturb on or off.
    pub fn toggle_dnd(&mut self, _state: &mut crate::state::State, _tx: &Senders) {}

    /// A second passed while the dashboard is up.
    pub fn tick(&mut self) {
        self.clock.tick();
    }
}

/// What the system reports. Returns a control if the event asks for one (a color was picked, and
/// the dashboard may now show itself again).
pub fn event(d: &mut Dash, event: Event) -> Option<crate::Control> {
    match event {
        Event::Toast(text) => d.toast(&text),
        other => log::trace!("not shown yet: {other:?}"),
    }
    None
}

/// Draw the widget of `ctx.tile`.
pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    match ctx.kind() {
        Kind::Clock => clock::render(ctx, cx),
        kind => placeholder(ctx, kind),
    }
}

/// A kind that isn't ported yet.
fn placeholder(ctx: &Ctx, kind: Kind) -> AnyElement {
    kit::empty(format!("{} …", kind.info().title), ctx.tk)
}

/// A key for the entered widget. True if it took it.
pub fn widget_key(d: &mut Dash, key: &Key, _cx: &mut Context<Dash>) -> bool {
    let Some(tile) = d.tile(d.nav.active_id) else {
        return false;
    };
    match tile.kind() {
        _ => false,
    }
}

/// What the entered widget would say about where its cursor is, for the stress run's log.
pub fn describe(_d: &Dash) -> Option<String> {
    None
}

/// What has to hold of the widgets whatever keys came before (the stress run checks it after
/// every key): the cursor is on a stop that exists, and so on. An error says what doesn't add up.
pub fn check(_d: &Dash) -> Result<(), String> {
    Ok(())
}

/// Escape inside a widget or one of its menus, before the dashboard takes it: true if something
/// closed.
pub fn escape(_d: &mut Dash, _cx: &mut Context<Dash>) -> bool {
    false
}

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

#[allow(unused)]
fn _uses(_: sys::Compositor) {}

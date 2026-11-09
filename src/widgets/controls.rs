//! Controls: battery and power, volume, brightness, night light, session buttons.
//!
//! The group's hooks, called by `widgets/mod.rs` for every group in turn; see PORTING.md. Each
//! widget is a submodule (`widgets/controls/<kind>.rs`) with its own `State`; the pieces they
//! share (sliders, rings) are in `sliders`.
//!
//! Time here passes in ticks (the dashboard's second): a slider the user dragged keeps its value
//! against the lagging source for [`HOLD`] ticks, a session button that asks for confirmation
//! stays armed for [`DISARM`], and a night light change is written to the state file once the
//! next tick finds it unsaved. A headless run, which has no clock, comes out the same everywhere.

use super::{Ctx, Opts, placeholder};
use crate::Control;
use crate::dash::{Dash, Geometry};
use crate::keys::Key;
use crate::kit::Tk;
use crate::layout::Kind;
use gpui::{AnyElement, App, Context, Window};
use niri_dash_system::Event;

mod battery;
mod brightness;
mod night_light;
mod session;
mod sliders;
mod volume;

pub use night_light::command as night_light_command;

/// How many ticks a slider the user dragged keeps its value against what the source reports
/// (which lags behind): the Slint widgets held it for 1.5 s, and a tick is a second.
const HOLD: u64 = 2;

/// How many ticks a session button that asks twice stays armed.
const DISARM: u64 = 3;

/// Everything the group's widgets keep that isn't the layout or the look. Several instances of a
/// kind share it (two volume widgets show the same volume).
#[derive(Default)]
pub struct State {
    /// Seconds the dashboard has been up, as ticks counted them.
    pub ticks: u64,
    pub battery: battery::State,
    pub volume: volume::State,
    pub brightness: brightness::State,
    pub night: night_light::State,
    pub session: session::State,
}

impl State {
    /// Whether something touched at tick `since` is still held.
    pub(super) fn held(&self, since: Option<u64>) -> bool {
        since.is_some_and(|t| self.ticks.saturating_sub(t) < HOLD)
    }
}

/// The kinds of widget this group draws.
pub fn handles(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Battery | Kind::Volume | Kind::Brightness | Kind::NightLight | Kind::Session
    )
}

/// What a widget is drawn in: its card's size, which the keys need as well (what fits decides
/// what has a keyboard stop).
#[derive(Debug, Clone)]
pub struct Fit {
    pub w: f32,
    pub h: f32,
    pub round: bool,
    pub pad: f32,
    pub pad_tight: f32,
    pub opts: Opts,
}

impl Fit {
    pub fn of_ctx(ctx: &Ctx) -> Self {
        Self {
            w: ctx.w,
            h: ctx.h,
            round: ctx.round,
            pad: ctx.tk.pad(),
            pad_tight: ctx.tk.pad_tight(),
            opts: ctx.opts.clone(),
        }
    }

    /// The same as `of_ctx` would see, from the dashboard's state: how the frame in `ui` sizes
    /// the card of widget `id`.
    pub fn of_dash(d: &Dash, id: u32) -> Option<Self> {
        let tile = d.tile(id)?;
        let tk = Tk::new(&d.look);
        let g: Geometry = d.geometry(d.size.0, d.size.1);
        let r = tile.rect();
        let gap = tk.gap();
        let expanded = d.nav.expanded_id == id;
        let (w, h) = if expanded {
            (
                g.width.min((r.w as f32 * g.cell).max(g.width * 0.55).max(8.0 * g.cell)),
                g.height.min((r.h as f32 * g.cell).max(g.height * 0.8).max(6.0 * g.cell)),
            )
        } else {
            (r.w as f32 * g.cell, r.h as f32 * g.cell)
        };
        let shows_round = tile.round() && !expanded;
        let (cw, ch) = ((w - 2.0 * gap).max(0.0), (h - 2.0 * gap).max(0.0));
        let (w, h) = if shows_round {
            (cw.min(ch), cw.min(ch))
        } else {
            (cw, ch)
        };
        Some(Self {
            w,
            h,
            round: shows_round,
            pad: tk.pad(),
            pad_tight: tk.pad_tight(),
            opts: Opts::of(&tile.placed, d.locale.us),
        })
    }
}

/// Stops with nothing skipped, for `Scope::set_count`.
pub(super) struct NoStops;
impl crate::scope::Stops for NoStops {}

/// A click handler that changes the dashboard.
pub(super) fn on<E: 'static>(
    cx: &Context<Dash>,
    f: impl Fn(&mut Dash, &mut Context<Dash>) + 'static,
) -> impl Fn(&E, &mut Window, &mut App) + 'static {
    cx.listener(move |d, _: &E, _, cx| {
        f(d, cx);
        cx.notify();
    })
}

/// A handler for a value a slider or knob reports.
pub(super) fn setter(
    cx: &Context<Dash>,
    f: impl Fn(&mut Dash, f32, &mut Context<Dash>) + 'static,
) -> impl Fn(f32, &mut Window, &mut App) + 'static {
    let dash = cx.entity();
    move |v, _, app| {
        dash.update(app, |d, cx| {
            f(d, v, cx);
            cx.notify();
        })
    }
}

/// The digit a key is, as the fraction of a full slider it sets (1 to 9: 10 to 90 %, 0: 100 %).
pub(super) fn digit_level(key: &Key) -> Option<f32> {
    match key.code {
        crate::keys::Code::Char('0') => Some(1.0),
        crate::keys::Code::Char(c @ '1'..='9') => Some((c as u8 - b'0') as f32 / 10.0),
        _ => None,
    }
}

/// What the widget of `ctx.tile` draws, in a card of `ctx.w` by `ctx.h` pixels.
pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    match ctx.kind() {
        Kind::Battery => battery::render(ctx, cx),
        Kind::Volume => volume::render(ctx, cx),
        Kind::Brightness => brightness::render(ctx, cx),
        Kind::NightLight => night_light::render(ctx, cx),
        Kind::Session => session::render(ctx, cx),
        kind => placeholder(ctx, kind),
    }
}

/// What the system reports. `None`: not this group's; `Some(control)`: taken, and the event asks
/// for a control (a color was picked, and the dashboard may show itself again) or not.
pub fn event(d: &mut Dash, event: &Event) -> Option<Option<Control>> {
    match event {
        Event::Power(e) => battery::event(d, e),
        Event::Audio(e) => volume::event(d, e),
        Event::Brightness(e) => brightness::event(d, e),
        Event::NightLight(e) => night_light::event(d, e),
        _ => return None,
    }
    Some(None)
}

/// A key for the entered widget `id` of `kind` (one of this group's). True if it took it.
pub fn key(d: &mut Dash, kind: Kind, id: u32, key: &Key, cx: &mut Context<Dash>) -> bool {
    let Some(fit) = Fit::of_dash(d, id) else {
        return false;
    };
    match kind {
        Kind::Battery => battery::key(d, &fit, key),
        Kind::Volume => volume::key(d, &fit, key),
        Kind::Brightness => brightness::key(d, &fit, key),
        Kind::NightLight => night_light::key(d, &fit, key),
        Kind::Session => session::key(d, &fit, key, cx),
        _ => false,
    }
}

/// Escape, before the dashboard takes it: true if something of the group's closed (a menu).
pub fn escape(d: &mut Dash, _cx: &mut Context<Dash>) -> bool {
    use crate::dash::NavMode;
    if d.nav.mode != NavMode::Widget {
        return false;
    }
    match d.tile(d.nav.active_id).map(|t| t.kind()) {
        Some(Kind::Volume) => volume::back(d),
        _ => false,
    }
}

/// A second passed while the dashboard is up.
pub fn tick(d: &mut Dash) {
    let c = &mut d.widgets.controls;
    c.ticks += 1;
    night_light::tick(d);
    session::tick(d);
}

/// The dashboard opens: every widget back on its first page.
pub fn come_up(d: &mut Dash) {
    let c = &mut d.widgets.controls;
    c.battery.scope = Default::default();
    c.volume.scope = Default::default();
    c.volume.picking = false;
    c.brightness.scope = Default::default();
    c.night.scope = Default::default();
    c.session.scope = Default::default();
    c.session.armed.clear();
}

/// The dashboard goes away: menus shut.
pub fn go_down(d: &mut Dash) {
    let c = &mut d.widgets.controls;
    c.volume.picking = false;
    c.session.armed.clear();
    night_light::save_now(d);
}

/// The dashboard came up (`true`) or went away: notes, timers and mascots only run while seen.
pub fn awake(_d: &mut Dash, _on: bool) {}

/// Widget `id` of `kind` was entered with the keyboard.
pub fn entered(d: &mut Dash, kind: Kind, id: u32) {
    if !handles(kind) {
        return;
    }
    if let Some(fit) = Fit::of_dash(d, id) {
        match kind {
            Kind::Battery => battery::refit(d, &fit),
            Kind::Volume => volume::refit(d, &fit),
            Kind::NightLight => night_light::refit(d, &fit),
            Kind::Session => session::refit(d, &fit),
            _ => {}
        }
    }
}

/// The tiles changed (a widget added, removed, moved): what is kept per instance follows.
pub fn layout_changed(_d: &mut Dash) {}

/// What the entered widget would say about where its cursor is, for the stress run's log.
pub fn describe(d: &Dash, kind: Kind) -> Option<String> {
    let fit = Fit::of_dash(d, d.nav.active_id)?;
    let c = &d.widgets.controls;
    match kind {
        Kind::Battery => Some(battery::describe(&c.battery, &fit)),
        Kind::Volume => Some(volume::describe(&c.volume, &fit)),
        Kind::Brightness => Some(brightness::describe(&c.brightness)),
        Kind::NightLight => Some(night_light::describe(&c.night, &fit)),
        Kind::Session => Some(session::describe(&c.session, &fit)),
        _ => None,
    }
}

/// What has to hold of the group's state whatever keys came before: the stress run checks it
/// after every key. An error says what doesn't add up.
///
/// A cursor may be past the last stop of a widget that was just resized (a style step, the
/// widget blown up with `e`): the next key, or tick, puts it back, and until then it is drawn on
/// the last stop. What can't happen is a cursor on a stop that never existed, or a level out of
/// range.
pub fn check(d: &Dash) -> Result<(), String> {
    let c = &d.widgets.controls;
    let level = |name: &str, v: f32| {
        if v.is_finite() && (0.0..=1.0).contains(&v) {
            Ok(())
        } else {
            Err(format!("{name} is {v}, outside 0 to 1"))
        }
    };
    level("volume", c.volume.volume)?;
    level("microphone volume", c.volume.mic_volume)?;
    for s in &c.volume.streams {
        level(&format!("{} volume", s.app), s.volume)?;
    }
    level("brightness", c.brightness.level)?;
    level("night light warmth", c.night.warmth)?;
    let nl = d.state.night_light;
    if !(1000..=niri_dash_system::night_light::NEUTRAL).contains(&nl.kelvin) {
        return Err(format!("night light at {} K", nl.kelvin));
    }
    for (name, scope) in [
        ("battery", c.battery.scope),
        ("volume", c.volume.scope),
        ("brightness", c.brightness.scope),
        ("night light", c.night.scope),
        ("session", c.session.scope),
    ] {
        if scope.subs == 0 || scope.part() >= scope.subs {
            return Err(format!("{name}: part {} of {} parts", scope.sub, scope.subs));
        }
        if scope.cursor > 64 {
            return Err(format!("{name}: cursor on stop {}", scope.cursor));
        }
    }
    if let Some(n) = c.session.armed.iter().find(|a| c.ticks.saturating_sub(a.since) > DISARM) {
        return Err(format!("session button {:?} armed since tick {}", n.button, n.since));
    }
    Ok(())
}

/// The dashboard is built (`d.state` is what was saved): what the group restores.
pub fn init(d: &mut Dash) {
    night_light::init(d);
}

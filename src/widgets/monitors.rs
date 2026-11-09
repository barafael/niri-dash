//! Monitors: system monitor, memory, disks, network, GPU, Claude usage.
//!
//! None of these takes keys: they show what the system reports (`Event::Perf`, `Event::Claude`)
//! and nothing in them can be pressed, as in the Slint widgets. The charts are in `charts`, the
//! pieces they share in `parts`.

use super::{Ctx, placeholder};
use crate::Control;
use crate::dash::Dash;
use crate::keys::Key;
use crate::layout::Kind;
use gpui::{AnyElement, Context};
use niri_dash_system::{Event, claude as sys, perf};

pub mod charts;
mod claude;
mod disks;
mod gpu;
mod memory;
mod network;
mod parts;
mod system;

/// Everything the group's widgets keep that isn't the layout or the look. Several instances of a
/// kind share it (two memory widgets show the same reading).
#[derive(Default)]
pub struct State {
    /// The last reading of the system monitor, memory, disks, network and GPU widgets (all
    /// empty and zero until the first arrives, as the Slint widgets were).
    pub perf: perf::Snapshot,
    /// The last report of the Claude usage widget.
    pub usage: sys::Usage,
    /// The Claude Code sessions running and how many of them are working (the usage widget
    /// says so in its header); the sessions widget's own list is the desk group's.
    pub sessions: usize,
    pub busy: usize,
}

/// The kinds of widget this group draws.
pub fn handles(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::SystemMonitor
            | Kind::Memory
            | Kind::Disks
            | Kind::Network
            | Kind::Gpu
            | Kind::ClaudeUsage
    )
}

/// What the widget of `ctx.tile` draws, in a card of `ctx.w` by `ctx.h` pixels.
pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    match ctx.kind() {
        Kind::SystemMonitor => system::render(ctx, cx),
        Kind::Memory => memory::render(ctx, cx),
        Kind::Disks => disks::render(ctx, cx),
        Kind::Network => network::render(ctx, cx),
        Kind::Gpu => gpu::render(ctx, cx),
        Kind::ClaudeUsage => claude::render(ctx, cx),
        kind => placeholder(ctx, kind),
    }
}

/// What the system reports. `None`: not this group's; `Some(control)`: taken, and the event asks
/// for a control (a color was picked, and the dashboard may show itself again) or not.
pub fn event(d: &mut Dash, event: &Event) -> Option<Option<Control>> {
    let m = &mut d.widgets.monitors;
    match event {
        Event::Perf(perf::Event::Snapshot(s)) => {
            m.perf = (**s).clone();
            Some(None)
        }
        Event::Claude(sys::Event::Usage(u)) => {
            m.usage = (**u).clone();
            Some(None)
        }
        // The sessions are the sessions widget's; the usage widget only counts them.
        Event::Claude(sys::Event::Sessions(s)) => {
            m.sessions = s.sessions.len();
            m.busy = s.busy;
            None
        }
        _ => None,
    }
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
pub fn check(d: &Dash) -> Result<(), String> {
    check_state(&d.widgets.monitors)
}

/// The readings are numbers a chart can draw: finite, not negative, and what is a share of a
/// whole is at most that (a little over for rounding); histories are as long as the sources
/// make them.
fn check_state(m: &State) -> Result<(), String> {
    fn ok(what: &str, v: f32, max: f32) -> Result<(), String> {
        if v.is_finite() && (0.0..=max).contains(&v) {
            Ok(())
        } else {
            Err(format!("monitors: {what} is {v}, not between 0 and {max}"))
        }
    }
    fn all(what: &str, vs: &[f32], max: f32) -> Result<(), String> {
        vs.iter().try_for_each(|v| ok(what, *v, max))
    }
    let p = &m.perf;
    for (what, v) in [
        ("cpu", p.cpu),
        ("mem", p.mem),
        ("swap", p.swap),
        ("root disk", p.root_used),
        ("net rx", p.net_rx_frac),
        ("net tx", p.net_tx_frac),
    ] {
        ok(what, v, 1.01)?;
    }
    for (what, vs) in [
        ("cpu history", &p.cpu_history),
        ("mem history", &p.mem_history),
        ("rx history", &p.net_rx_history),
        ("tx history", &p.net_tx_history),
        ("disk history", &p.disk_history),
        ("cores", &p.cores),
    ] {
        all(what, vs, 1.01)?;
    }
    for d in &p.disks {
        ok("a disk's use", d.used, 1.01)?;
    }
    if let Some(g) = &p.gpu {
        ok("gpu load", g.busy, 1.01)?;
        ok("vram", g.vram, 1.01)?;
        all("gpu history", &g.history, 1.01)?;
    }
    let u = &m.usage;
    ok("claude window", u.window_progress, 1.01)?;
    all("claude hours", &u.hourly, 1.01)?;
    for (name, share) in &u.models {
        ok(&format!("the share of {name}"), *share, 1.01)?;
    }
    if m.busy > m.sessions {
        return Err(format!(
            "monitors: {} claude sessions busy of {}",
            m.busy, m.sessions
        ));
    }
    Ok(())
}

/// The dashboard is built (`d.state` is what was saved): what the group restores.
pub fn init(_d: &mut Dash) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_read_yet_is_fine() {
        assert_eq!(check_state(&State::default()), Ok(()));
    }

    #[test]
    fn readings_a_chart_cannot_draw_are_caught() {
        let mut m = State::default();
        m.perf.cpu_history = vec![0.1, f32::NAN];
        assert!(check_state(&m).unwrap_err().contains("cpu history"));
        let mut m = State::default();
        m.perf.mem = -0.2;
        assert!(check_state(&m).unwrap_err().contains("mem"));
        let m = State {
            sessions: 1,
            busy: 2,
            ..State::default()
        };
        assert!(check_state(&m).unwrap_err().contains("claude sessions"));
    }
}

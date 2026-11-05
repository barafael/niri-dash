//! What the system does through Wayland protocols rather than through the compositor's IPC: the
//! clipboard (ext-/wlr-data-control) and night light's gamma tables (wlr-gamma-control). Its own
//! event queue on the dashboard's connection (one client, as far as the compositor can tell),
//! dispatched on a thread of its own that owns all of it. Commands arrive over channels into
//! that thread's event loop, which ends when they are all closed.

mod clipboard;
pub mod gamma;

use super::{EventSender, clipboard as vocab};
use smithay_client_toolkit::reexports::calloop::EventLoop;
use smithay_client_toolkit::reexports::calloop::channel::{self, Channel, Sender};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::{
    Connection, QueueHandle, globals::registry_queue_init, protocol::wl_output, protocol::wl_seat,
};
use smithay_client_toolkit::{
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{Capability, SeatHandler, SeatState},
};

/// The Wayland side of the system, as the dashboard and the other actors see it.
pub struct Handles {
    pub clipboard: Sender<vocab::Command>,
    /// The temperature to warm the screen to; None turns night light off.
    pub gamma: Sender<Option<u32>>,
    /// The compositor offers ext- or wlr-data-control.
    pub has_clipboard: bool,
    /// The compositor offers wlr-gamma-control (and we asked for it).
    pub has_gamma: bool,
}

/// The state the thread owns; Wayland events are dispatched to it.
pub struct Services {
    conn: Connection,
    qh: QueueHandle<Services>,
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    clipboard: clipboard::Clipboard,
    gamma: gamma::Gamma,
    events: EventSender,
    /// Command channels that are still open; the loop ends when none are.
    open: u8,
}

/// Start the thread on `conn`, reporting to `events`. `with_gamma`: bind the gamma control (the
/// dashboard does; a standalone app leaves it to the dashboard). `kwin`: night light is KWin's
/// own here (only for what the log says).
pub fn start(
    conn: Connection,
    events: EventSender,
    with_gamma: bool,
    kwin: bool,
) -> std::io::Result<Handles> {
    let (handles_tx, handles_rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("niri-dash-wayland".into())
        .spawn(move || {
            let (mut event_loop, mut state) = match setup(conn, events, with_gamma, kwin) {
                Ok((event_loop, state, handles)) => {
                    if handles_tx.send(Ok(handles)).is_err() {
                        return;
                    }
                    (event_loop, state)
                }
                Err(e) => {
                    let _ = handles_tx.send(Err(e));
                    return;
                }
            };
            while state.open > 0 {
                if let Err(e) = event_loop.dispatch(None, &mut state) {
                    log::warn!("wayland services: {e}");
                    return;
                }
            }
        })?;
    handles_rx
        .recv()
        .unwrap_or_else(|_| Err(std::io::Error::other("the wayland thread died starting up")))
}

fn setup(
    conn: Connection,
    events: EventSender,
    with_gamma: bool,
    kwin: bool,
) -> std::io::Result<(EventLoop<'static, Services>, Services, Handles)> {
    let err = |e: &dyn std::fmt::Display| std::io::Error::other(format!("wayland: {e}"));
    let (globals, queue) = registry_queue_init::<Services>(&conn).map_err(|e| err(&e))?;
    let qh = queue.handle();
    let event_loop = EventLoop::<Services>::try_new().map_err(|e| err(&e))?;
    let gamma = if with_gamma {
        gamma::Gamma::bind(&globals, &qh, kwin)
    } else {
        Default::default()
    };
    let state = Services {
        registry_state: RegistryState::new(&globals),
        seat_state: SeatState::new(&globals, &qh),
        output_state: OutputState::new(&globals, &qh),
        clipboard: clipboard::Clipboard::bind(&globals, &qh),
        gamma,
        conn: conn.clone(),
        qh,
        events,
        open: 2,
    };
    WaylandSource::new(conn, queue)
        .insert(event_loop.handle())
        .map_err(|e| err(&e))?;
    let (clipboard_tx, clipboard_rx): (_, Channel<vocab::Command>) = channel::channel();
    event_loop
        .handle()
        .insert_source(clipboard_rx, |ev, _, me| match ev {
            channel::Event::Msg(vocab::Command::Set(clip)) => me.set_clipboard(clip),
            channel::Event::Msg(vocab::Command::Record(on)) => me.record_clipboard(on),
            channel::Event::Closed => me.open -= 1,
        })
        .map_err(|e| err(&e))?;
    let (gamma_tx, gamma_rx): (_, Channel<Option<u32>>) = channel::channel();
    event_loop
        .handle()
        .insert_source(gamma_rx, |ev, _, me| match ev {
            channel::Event::Msg(want) => {
                let qh = me.qh.clone();
                me.gamma.set(want, &qh);
                let _ = me.conn.flush();
            }
            channel::Event::Closed => me.open -= 1,
        })
        .map_err(|e| err(&e))?;
    let handles = Handles {
        clipboard: clipboard_tx,
        gamma: gamma_tx,
        has_clipboard: state.clipboard.available(),
        has_gamma: state.gamma.available(),
    };
    Ok((event_loop, state, handles))
}

impl SeatHandler for Services {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        _: Capability,
    ) {
        self.clipboard.seat(&seat, qh);
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        _: Capability,
    ) {
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl OutputHandler for Services {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, qh: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.gamma.add_output(output, qh);
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.gamma.remove_output(&output);
    }
}

impl ProvidesRegistryState for Services {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(Services);
smithay_client_toolkit::delegate_dispatch2!(Services);

//! The system niri-dash runs on, split from the dashboard. Each applet has a module here with the
//! `Command`s it can be asked to do and the `Event`s it reports. Behind every module with
//! commands is an actor that owns that part of the system. A whole system is an actor too
//! ([`System`]): the dashboard sends it one [`Command`] enum and gets one [`Event`] enum back, each
//! through a queue of fixed length, and holds nothing else of it.
//!
//! [`live`] runs the real system, under niri, KWin or another compositor (not in a browser).
//! [`simulated`] runs a made-up one that keeps moving, which is all a browser gets. [`mock`] runs
//! one that sits still, for tests and screenshots.

pub mod audio;
pub mod bluetooth;
pub mod brightness;
pub mod claude;
pub mod clipboard;
pub mod color;
#[cfg(not(target_arch = "wasm32"))]
mod compositor;
pub mod drives;
pub mod host;
pub mod keyboard;
#[cfg(not(target_arch = "wasm32"))]
mod live;
pub mod mascot;
pub mod media;
mod mock;
pub mod night_light;
pub mod notifications;
pub mod perf;
pub mod phones;
pub mod power;
pub mod privacy;
pub mod session;
mod sim;
pub mod sleep;
#[cfg(not(target_arch = "wasm32"))]
mod sources;
mod text;
pub mod tray;
#[cfg(not(target_arch = "wasm32"))]
mod wayland;
pub mod weather;
pub mod wifi;
pub mod windows;

#[cfg(not(target_arch = "wasm32"))]
pub use compositor::guess as guess_compositor;
#[cfg(not(target_arch = "wasm32"))]
pub use live::{Options, live};
#[cfg(not(target_arch = "wasm32"))]
pub use mock::mock;
pub use mock::{Demo, MockSystem};
pub use sim::{Sim, simulated};
#[cfg(not(target_arch = "wasm32"))]
pub use sources::which;
use std::path::PathBuf;
use std::sync::Arc;
pub use text::{Locale, clean_label, locale, one_line, strip_spinner, unescape_entities};
pub use tokio::sync::mpsc::{Receiver, Sender, channel};
pub use tokio::sync::oneshot;

/// How many commands may wait for the system, and for each of its actors. Actors handle a command
/// without waiting on anything, so a full queue means the system has stopped reading; the UI
/// doesn't wait for room then, and the command is dropped (and logged).
pub const COMMAND_QUEUE: usize = 256;

/// How many events may wait for the dashboard, which takes all of them each time its loop wakes.
/// A full queue means the UI thread is stuck; what the system finds meanwhile is dropped (and
/// logged), and what is still true is reported again with the next change.
pub const EVENT_QUEUE: usize = 1024;

/// Where a system's events go: a function that hands each one to the dashboard and wakes it. What
/// wakes it is the dashboard's business (a Wayland event loop, a browser promise, a test's
/// receiver), so nothing here depends on it.
pub type EventSender = Sink<Event>;

/// Why a value wasn't sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// There is no room: the other end has stopped reading for a while.
    Full,
    /// The other end is gone; there is nobody left to tell.
    Closed,
}

/// A way to send values to someone who wakes up for them, through a queue of fixed length. It
/// never waits: a value that finds no room is refused.
pub struct Sink<T> {
    /// What the queue holds, for the log ("the dashboard's events").
    name: &'static str,
    send: Arc<dyn Fn(T) -> Result<(), Refused> + Send + Sync>,
}

impl<T> Clone for Sink<T> {
    fn clone(&self) -> Self {
        Self {
            name: self.name,
            send: self.send.clone(),
        }
    }
}

impl<T> Sink<T> {
    /// A sink that calls `send` with each value, from whichever thread sends it. `send` must not
    /// block: it refuses a value it has no room for.
    pub fn new(
        name: &'static str,
        send: impl Fn(T) -> Result<(), Refused> + Send + Sync + 'static,
    ) -> Self {
        Self {
            name,
            send: Arc::new(send),
        }
    }

    /// Send `value` if there is room. A value dropped for want of room is logged here, so callers
    /// only need to look at the result if they would do something else then.
    pub fn send(&self, value: T) -> Result<(), Refused> {
        let sent = (self.send)(value);
        if sent == Err(Refused::Full) {
            log::warn!("dropped one of {}: the queue is full", self.name);
        }
        sent
    }
}

impl<T: Send + 'static> Sink<T> {
    /// A sink into an async queue of `cap`, and the queue's receiving end.
    pub fn queue(name: &'static str, cap: usize) -> (Self, Receiver<T>) {
        let (tx, rx) = channel(cap);
        let sink = Self::new(name, move |v| {
            tx.try_send(v).map_err(|e| match e {
                tokio::sync::mpsc::error::TrySendError::Full(_) => Refused::Full,
                tokio::sync::mpsc::error::TrySendError::Closed(_) => Refused::Closed,
            })
        });
        (sink, rx)
    }

    /// A sink of [`EVENT_QUEUE`] and the receiver that gets what is sent to it, with nothing to
    /// wake: for tests and anything else that reads when it chooses to.
    pub fn channel() -> (Self, std::sync::mpsc::Receiver<T>) {
        let (tx, rx) = std::sync::mpsc::sync_channel(EVENT_QUEUE);
        let sink = Self::new("a test's values", move |v| {
            tx.try_send(v).map_err(|e| match e {
                std::sync::mpsc::TrySendError::Full(_) => Refused::Full,
                std::sync::mpsc::TrySendError::Disconnected(_) => Refused::Closed,
            })
        });
        (sink, rx)
    }
}

/// An image that the UI thread turns into a `slint::Image`.
#[derive(Debug, Clone, PartialEq)]
pub enum IconSrc {
    None,
    Path(PathBuf),
    Rgba {
        width: u32,
        height: u32,
        data: Arc<Vec<u8>>,
    },
}

/// Which compositor we run under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compositor {
    Niri,
    /// KDE Plasma's KWin.
    Kwin,
    /// Anything else: the Wayland protocols work, the compositor-specific features don't.
    Other,
}

impl Compositor {
    /// What the platform is called to the user, to finish "not available on …".
    pub const fn name(self) -> &'static str {
        match self {
            Compositor::Niri => "niri",
            Compositor::Kwin => "KDE Plasma",
            Compositor::Other => "this desktop",
        }
    }
}

impl std::fmt::Display for Compositor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Hand a command to the system, without waiting. A full queue is logged by the sink; a closed
/// one means the system is shutting down, and there is nobody left to tell.
pub trait Tell<T> {
    fn tell(&self, cmd: T);
}

impl<T: Into<Command>> Tell<T> for Sink<Command> {
    fn tell(&self, cmd: T) {
        if self.send(cmd.into()) == Err(Refused::Closed) {
            log::debug!("the system has stopped");
        }
    }
}

/// Every applet that takes commands, as `Variant(module)`, handed to `$then`: the one list the
/// [`Command`] enum and the live system's router are both made from.
macro_rules! applets {
    ($then:ident) => {
        $then! {
            Host(host),
            Windows(windows),
            Keyboard(keyboard),
            Privacy(privacy),
            Color(color),
            Wifi(wifi),
            Bluetooth(bluetooth),
            Audio(audio),
            Brightness(brightness),
            Notifications(notifications),
            Tray(tray),
            Media(media),
            Power(power),
            Session(session),
            Drives(drives),
            Phones(phones),
            Weather(weather),
            NightLight(night_light),
            Mascot(mascot),
            Clipboard(clipboard),
            Sleep(sleep),
        }
    };
}
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use applets;

macro_rules! commands {
    ($($variant:ident($module:ident)),* $(,)?) => {
        /// What the dashboard asks of the system, of whichever applet. Each applet's commands keep
        /// their order, and so do all of them together.
        #[derive(Debug)]
        pub enum Command {
            $($variant($module::Command),)*
        }

        $(impl From<$module::Command> for Command {
            fn from(c: $module::Command) -> Self {
                Command::$variant(c)
            }
        }

        /// The applet's command, or the command back if it is another applet's.
        impl TryFrom<Command> for $module::Command {
            type Error = Command;

            fn try_from(c: Command) -> Result<Self, Command> {
                match c {
                    Command::$variant(c) => Ok(c),
                    other => Err(other),
                }
            }
        })*
    };
}

applets!(commands);

/// A running system: an actor that takes [`Command`]s and reports [`Event`]s, and what it can do
/// here.
pub struct System {
    /// Where to send it commands. It stops when the last clone of this is dropped.
    pub commands: Sink<Command>,
    pub caps: Capabilities,
}

impl System {
    /// A system's inbox: a sink for the dashboard to send to, and where the commands arrive.
    pub fn inbox() -> (Sink<Command>, Receiver<Command>) {
        Sink::queue("the system's commands", COMMAND_QUEUE)
    }
}

/// What the system can do here, decided once when it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Which compositor this is, for wording ("Plasma's night light"); what the system can do is
    /// in the fields below.
    pub compositor: Compositor,
    /// The compositor offers clipboard access (ext- or wlr-data-control).
    pub clipboard: bool,
    /// We hold the gamma control (wlr-gamma-control) for night light.
    pub gamma: bool,
    /// The window list works, and focusing and closing windows. On niri always; on KWin only when
    /// it let us bind its window management (see `niri-dash desktop-entries`).
    pub windows: bool,
    /// The keyboard layouts can be listed and switched.
    pub keyboard_layouts: bool,
    /// A color can be picked from the screen.
    pub color_picker: bool,
    /// Screen sharing is reported (who is casting, and stopping it).
    pub screencast: bool,
    /// We are the notification server, so our own messages land in our history. Where another
    /// one is (Plasma's), they go to it and it pops them up by itself.
    pub notifications_server: bool,
    /// Night light is the compositor's own rather than our gamma control.
    pub night_light_native: bool,
    /// None of it is real (the demo in a browser): opening something by clicking it opens nothing.
    pub simulated: bool,
}

impl Capabilities {
    /// How to say that something this platform can't do isn't there: "not available on KDE
    /// Plasma". The one wording for every widget that lacks what it needs; callers put the
    /// feature in front of it ("Window list not available on …").
    pub fn unavailable(&self) -> String {
        format!("not available on {}", self.compositor)
    }

    /// What `compositor` offers, with clipboard and gamma as found (those are protocols, not
    /// compositors).
    pub const fn for_compositor(compositor: Compositor, clipboard: bool, gamma: bool) -> Self {
        let niri = matches!(compositor, Compositor::Niri);
        let kwin = matches!(compositor, Compositor::Kwin);
        Self {
            compositor,
            clipboard,
            gamma,
            windows: niri,
            keyboard_layouts: niri || kwin,
            color_picker: niri || kwin,
            screencast: niri,
            notifications_server: !kwin,
            night_light_native: kwin,
            simulated: false,
        }
    }
}

/// A part of the system a widget reads from. A standalone app only runs the ones its widget
/// shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feed {
    Perf,
    /// What the compositor reports: windows, keyboard layouts, screencasts.
    Compositor,
    /// KWin's night light (on Plasma; elsewhere the daemon holds the gamma control).
    NightLight,
    Power,
    Claude,
    ClaudeUsage,
    Weather,
    Wifi,
    Bluetooth,
    Audio,
    Brightness,
    Notifications,
    Tray,
    Media,
    Inhibit,
    Privacy,
    Drives,
    Phone,
}

macro_rules! events {
    ($($(#[$doc:meta])* $variant:ident($module:ident)),* $(,)?) => {
        /// What the system reports to the dashboard.
        #[derive(Debug)]
        pub enum Event {
            $($(#[$doc])* $variant($module::Event),)*
            /// Something went wrong that the user should hear about.
            Toast(String),
        }

        $(impl From<$module::Event> for Event {
            fn from(e: $module::Event) -> Self {
                Event::$variant(e)
            }
        })*
    };
}

events! {
    Perf(perf),
    Claude(claude),
    Windows(windows),
    Keyboard(keyboard),
    Privacy(privacy),
    Color(color),
    Wifi(wifi),
    Bluetooth(bluetooth),
    Audio(audio),
    Brightness(brightness),
    Notifications(notifications),
    Tray(tray),
    Media(media),
    Power(power),
    Drives(drives),
    Phones(phones),
    Weather(weather),
    Clipboard(clipboard),
    NightLight(night_light),
    Mascot(mascot),
    Sleep(sleep),
}

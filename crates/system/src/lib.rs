//! The system niri-dash runs on, split from the dashboard. Each applet has a module here with the
//! `Command`s it can be asked to do and the `Event`s it reports. Behind every module with
//! commands is an actor that owns that part of the system. The dashboard holds nothing but the
//! actors' senders ([`Senders`]) and gets their events on its event loop.
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
pub use mock::{Demo, MockSystem, Received};
pub use sim::{Sim, simulated};
#[cfg(not(target_arch = "wasm32"))]
pub use sources::which;
use std::path::PathBuf;
use std::sync::Arc;
pub use text::{Locale, clean_label, locale, one_line, strip_spinner, unescape_entities};
pub use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
pub use tokio::sync::oneshot;

/// Where a system's events go: a function that hands each one to the dashboard and wakes it. What
/// wakes it is the dashboard's business (a Wayland event loop, a browser promise, a test's
/// receiver), so nothing here depends on it.
pub type EventSender = Sink<Event>;

/// The other end is gone; there is nobody left to tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Closed;

/// A way to send values to someone who wakes up for them.
pub struct Sink<T>(Arc<dyn Fn(T) -> Result<(), Closed> + Send + Sync>);

impl<T> Clone for Sink<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Sink<T> {
    /// A sink that calls `send` with each value, from whichever thread sends it.
    pub fn new(send: impl Fn(T) -> Result<(), Closed> + Send + Sync + 'static) -> Self {
        Self(Arc::new(send))
    }

    pub fn send(&self, value: T) -> Result<(), Closed> {
        (self.0)(value)
    }
}

impl<T: Send + 'static> Sink<T> {
    /// A sink and the receiver that gets what is sent to it, with nothing to wake: for tests and
    /// anything else that reads when it chooses to.
    pub fn channel() -> (Self, std::sync::mpsc::Receiver<T>) {
        let (tx, rx) = std::sync::mpsc::channel();
        (Self::new(move |v| tx.send(v).map_err(|_| Closed)), rx)
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

/// Hand a command to an actor. Commands are never refused, so the only failure is that the actor
/// is gone, which means the system is shutting down; there is nobody left to tell.
pub trait Tell<T> {
    fn tell(&self, cmd: T);
}

impl<T> Tell<T> for UnboundedSender<T> {
    fn tell(&self, cmd: T) {
        if self.send(cmd).is_err() {
            log::debug!("a system actor has stopped");
        }
    }
}

/// Where the dashboard sends its commands: one actor per applet. Unbounded, because the UI thread
/// must neither block nor drop a click; actors that get floods (slider drags) only act on the
/// latest value.
#[derive(Clone)]
pub struct Senders {
    pub host: UnboundedSender<host::Command>,
    pub windows: UnboundedSender<windows::Command>,
    pub keyboard: UnboundedSender<keyboard::Command>,
    pub privacy: UnboundedSender<privacy::Command>,
    pub color: UnboundedSender<color::Command>,
    pub wifi: UnboundedSender<wifi::Command>,
    pub bluetooth: UnboundedSender<bluetooth::Command>,
    pub audio: UnboundedSender<audio::Command>,
    pub brightness: UnboundedSender<brightness::Command>,
    pub notifications: UnboundedSender<notifications::Command>,
    pub tray: UnboundedSender<tray::Command>,
    pub media: UnboundedSender<media::Command>,
    pub power: UnboundedSender<power::Command>,
    pub session: UnboundedSender<session::Command>,
    pub drives: UnboundedSender<drives::Command>,
    pub phones: UnboundedSender<phones::Command>,
    pub weather: UnboundedSender<weather::Command>,
    pub night_light: UnboundedSender<night_light::Command>,
    pub mascot: UnboundedSender<mascot::Command>,
    pub clipboard: UnboundedSender<clipboard::Command>,
    pub sleep: UnboundedSender<sleep::Command>,
}

/// The receiving ends of [`Senders`]: what the actors (or a test standing in for them) read.
pub struct Inboxes {
    pub host: UnboundedReceiver<host::Command>,
    pub windows: UnboundedReceiver<windows::Command>,
    pub keyboard: UnboundedReceiver<keyboard::Command>,
    pub privacy: UnboundedReceiver<privacy::Command>,
    pub color: UnboundedReceiver<color::Command>,
    pub wifi: UnboundedReceiver<wifi::Command>,
    pub bluetooth: UnboundedReceiver<bluetooth::Command>,
    pub audio: UnboundedReceiver<audio::Command>,
    pub brightness: UnboundedReceiver<brightness::Command>,
    pub notifications: UnboundedReceiver<notifications::Command>,
    pub tray: UnboundedReceiver<tray::Command>,
    pub media: UnboundedReceiver<media::Command>,
    pub power: UnboundedReceiver<power::Command>,
    pub session: UnboundedReceiver<session::Command>,
    pub drives: UnboundedReceiver<drives::Command>,
    pub phones: UnboundedReceiver<phones::Command>,
    pub weather: UnboundedReceiver<weather::Command>,
    pub night_light: UnboundedReceiver<night_light::Command>,
    pub mascot: UnboundedReceiver<mascot::Command>,
    pub clipboard: UnboundedReceiver<clipboard::Command>,
    pub sleep: UnboundedReceiver<sleep::Command>,
}

/// A sender for every applet, and where each one's commands arrive.
pub fn channels() -> (Senders, Inboxes) {
    macro_rules! pairs {
        ($($name:ident),*) => {{
            $(let $name = unbounded_channel();)*
            (Senders { $($name: $name.0,)* }, Inboxes { $($name: $name.1,)* })
        }};
    }
    pairs!(
        host,
        windows,
        keyboard,
        privacy,
        color,
        wifi,
        bluetooth,
        audio,
        brightness,
        notifications,
        tray,
        media,
        power,
        session,
        drives,
        phones,
        weather,
        night_light,
        mascot,
        clipboard,
        sleep
    )
}

/// A running system: where to send commands, and what it can do here.
pub struct Started {
    pub senders: Senders,
    pub caps: Capabilities,
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

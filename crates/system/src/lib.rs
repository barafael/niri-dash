//! The system niri-dash runs on, split from the dashboard. Each applet has a module here with the
//! `Command`s it can be asked to do and the `Event`s it reports. Behind every module with
//! commands is an actor that owns that part of the system. The dashboard holds nothing but the
//! actors' senders ([`Senders`]) and gets their events on its event loop.
//!
//! [`live`] runs the real system, under niri, KWin or another compositor.

pub mod audio;
pub mod bluetooth;
pub mod brightness;
pub mod claude;
pub mod clipboard;
pub mod color;
mod compositor;
pub mod drives;
pub mod host;
pub mod keyboard;
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
mod sources;
pub mod tray;
mod wayland;
pub mod weather;
pub mod wifi;
pub mod windows;

pub use compositor::{Compositor, guess as guess_compositor};
pub use live::{Options, live};
pub use mock::{Demo, MockSystem, Received, mock};
pub use sources::{IconSrc, Locale, clean_label, locale, which};
use tokio::sync::mpsc::unbounded_channel;
pub use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
pub use tokio::sync::oneshot;

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
        clipboard
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
    pub compositor: Compositor,
    /// The compositor offers clipboard access (ext- or wlr-data-control).
    pub clipboard: bool,
    /// We hold the gamma control (wlr-gamma-control) for night light.
    pub gamma: bool,
}

impl Capabilities {
    pub fn niri(self) -> bool {
        self.compositor == Compositor::Niri
    }

    pub fn kwin(self) -> bool {
        self.compositor == Compositor::Kwin
    }

    /// Whether the window list (and focusing windows) works here.
    pub fn has_windows(self) -> bool {
        self.niri()
    }

    /// Whether our own messages go to another notification server (Plasma's) rather than into
    /// our history, which then pops them up by itself.
    pub fn notifications_elsewhere(self) -> bool {
        self.kwin()
    }

    /// Whether night light is KWin's own rather than our gamma control.
    pub fn kwin_night_light(self) -> bool {
        self.kwin()
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
}

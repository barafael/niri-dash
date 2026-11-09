//! niri-dash: a fullscreen widget dashboard for niri, toggled with a keybind.
//!
//! `niri-dash` (or `niri-dash daemon`) runs the dashboard in the background; `niri-dash toggle`
//! shows or hides it and starts the daemon if needed. `niri-dash app <widget>` runs one widget as
//! a standalone app in its own window.
//!
//! The window is drawn by [GPUI](https://gpui.rs). The binary is `main` here; the library exists
//! so the tests (`tests/`) can drive the dashboard headlessly against channels standing in for
//! the system ([`testing`]).

mod affinity;
mod autolayout;
#[cfg(test)]
mod book;
mod assets;
mod cli;
mod config;
mod cowfile;
mod dash;
mod desktop;
mod edit;
mod files;
mod keys;
mod kit;
mod layout;
mod look;
mod mascots;
mod nav;
mod pomodoro;
mod scope;
mod state;
mod theme;
mod timer;
mod ui;
mod widget_settings;
mod widgets;
mod window;
mod zones;

#[cfg(feature = "testing")]
pub mod stress;
#[doc(hidden)]
#[cfg(feature = "testing")]
pub mod testing;

pub use cli::{main, named_socket_path, socket_path};

/// This process's id.
fn own_pid() -> u32 {
    std::process::id()
}

/// A command on the control socket, and the subcommand that sends it; the lowercase name is the
/// wire format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString, strum::AsRefStr, clap::Subcommand)]
#[strum(serialize_all = "lowercase")]
pub enum Control {
    /// Show or hide the dashboard; starts the daemon if it isn't running.
    Toggle,
    /// Show the dashboard.
    Show,
    /// Hide the dashboard.
    Hide,
    /// Show the dashboard in edit mode.
    Edit,
    /// Stop the daemon.
    Quit,
    /// Turn blocking sleep and screen locking on or off (like Plasma's battery widget switch).
    Inhibit,
    /// Turn do not disturb on or off: notifications are kept without a pop-up.
    Dnd,
}

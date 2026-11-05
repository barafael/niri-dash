//! niri-dash: a fullscreen widget dashboard for niri, toggled with a keybind.
//!
//! `niri-dash` (or `niri-dash daemon`) runs the dashboard in the background; `niri-dash toggle`
//! shows or hides it and starts the daemon if needed. `niri-dash app <widget>` runs one widget as
//! a standalone app in its own window.
//!
//! The binary is `main` here; the library exists so the UI tests (`tests/ui.rs`) can drive the
//! dashboard headlessly against channels standing in for the system ([`testing`]).
//!
//! In a browser the same dashboard runs on a canvas, against a simulated system ([`web`]).

// Much of the dashboard only comes to use on the desktop (standalone apps, the command line).
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

slint::include_modules!();

mod affinity;
mod app;
mod autolayout;
#[cfg(not(target_arch = "wasm32"))]
mod cli;
mod cowfile;
#[cfg(not(target_arch = "wasm32"))]
pub use app::testing;
#[cfg(target_arch = "wasm32")]
pub use app::web;
#[cfg(not(target_arch = "wasm32"))]
pub use cli::{main, named_socket_path, socket_path};
#[cfg(not(target_arch = "wasm32"))]
mod desktop;
mod files;
#[cfg(not(target_arch = "wasm32"))]
mod gpu;
mod layout;
mod mascots;
mod nav;
mod pomodoro;
#[cfg(not(target_arch = "wasm32"))]
mod shell;
mod state;
mod theme;
mod timer;
mod widget_settings;
mod zones;

/// This process's id, which a browser doesn't have.
fn own_pid() -> u32 {
    #[cfg(not(target_arch = "wasm32"))]
    return std::process::id();
    #[cfg(target_arch = "wasm32")]
    return 0;
}

/// A command on the control socket, and the subcommand that sends it; the lowercase name is the
/// wire format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString, strum::AsRefStr)]
#[cfg_attr(not(target_arch = "wasm32"), derive(clap::Subcommand))]
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

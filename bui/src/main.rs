//! niri-dash drawn with BUI (Blender's UI toolkit, the `blenderui` crate): the same system, the
//! same config file and the same keys as the Slint version in the repository's root, with
//! Blender's look. See README.md.

// The parts of the Slint version that don't touch the window, shared with it as they are (these
// files are links to the root crate's): the config and its widgets, the keyboard's arithmetic,
// the timers, the mascots and the saved state.
#[allow(dead_code)]
mod affinity;
#[allow(dead_code)]
mod autolayout;
#[allow(dead_code)]
mod cowfile;
#[allow(dead_code)]
mod files;
#[allow(dead_code)]
mod layout;
#[allow(dead_code)]
mod mascots;
#[allow(dead_code)]
mod nav;
#[allow(dead_code)]
mod pomodoro;
#[allow(dead_code)]
mod state;
#[allow(dead_code)]
mod timer;
#[allow(dead_code)]
mod widget_settings;
#[allow(dead_code)]
mod zones;

// This version's own.
mod a11y;
mod app;
mod cli;
mod dash;
mod data;
mod edit;
mod icons;
mod look;
mod mascot_art;
mod snapshot;
#[cfg(test)]
mod testing;
mod view;
mod widgets;

/// This process's id.
fn own_pid() -> u32 {
    std::process::id()
}

/// A command on the control socket, and the subcommand that sends it; the lowercase name is the
/// wire format (the same as the Slint version's).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, strum::EnumString, strum::AsRefStr, clap::Subcommand,
)]
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
    /// Block sleep and screen locking, or stop blocking them.
    Inhibit,
    /// Do not disturb on or off.
    Dnd,
}

fn main() -> std::process::ExitCode {
    cli::main()
}

//! niri-dash: a fullscreen widget dashboard for niri, toggled with a keybind.
//!
//! `niri-dash` (or `niri-dash daemon`) runs the dashboard in the background; `niri-dash toggle`
//! shows or hides it and starts the daemon if needed. `niri-dash app <widget>` runs one widget as
//! a standalone app in its own window.

slint::include_modules!();

mod app;
mod desktop;
mod gpu;
mod layout;
mod mascots;
mod shell;
mod sources;
mod state;
mod theme;
mod timer;
mod widget_settings;
mod zones;

use clap::{Parser, Subcommand};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

/// What the command line reports when a command fails.
#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("unknown widget {widget:?}; one of: {names}")]
    UnknownWidget { widget: String, names: String },
    #[error("sending command: {0}")]
    Send(#[source] std::io::Error),
    #[error("niri-dash is not running: {0}")]
    NotRunning(#[source] std::io::Error),
    #[error("niri-dash is not running")]
    NobodyListening,
    #[error("reading stdin: {0}")]
    Stdin(#[source] std::io::Error),
    #[error("nothing to say")]
    NothingToSay,
    #[error("{0}")]
    BadTarget(String),
    #[error(transparent)]
    App(#[from] app::Error),
    #[error(transparent)]
    Desktop(#[from] desktop::Error),
}

type Result<T, E = Error> = std::result::Result<T, E>;

fn unknown_widget(widget: &str) -> Error {
    let names: Vec<&str> = <layout::Kind as strum::IntoEnumIterator>::iter()
        .map(|k| k.name())
        .collect();
    Error::UnknownWidget {
        widget: widget.into(),
        names: names.join(", "),
    }
}

/// A command on the control socket; the lowercase name is the wire format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumString, strum::AsRefStr)]
#[strum(serialize_all = "lowercase")]
pub enum Control {
    Show,
    Hide,
    Toggle,
    Edit,
    Quit,
    /// Toggle blocking sleep and screen locking.
    Inhibit,
    /// Toggle do not disturb (until turned off).
    Dnd,
}

#[derive(Parser)]
#[command(version, about = "A fullscreen widget dashboard for niri")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Clone)]
enum Command {
    /// Run the dashboard in the background (the default). Put this in spawn-at-startup.
    Daemon {
        /// Show the dashboard right away.
        #[arg(long)]
        show: bool,
    },
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
    /// Run one widget as a standalone app in its own window, e.g. `niri-dash app clock`. If it is
    /// already running, show and focus it.
    App {
        /// The widget, as named in config.kdl (clock, calendar, wifi, bluetooth, clipboard, …).
        widget: String,
        /// Close it if it is open, show it otherwise (for keybinds).
        #[arg(long, conflicts_with = "quit")]
        toggle: bool,
        /// Stop it, also if it keeps running in the background (clipboard).
        #[arg(long)]
        quit: bool,
    },
    /// Make a mascot widget say something, e.g. `niri-dash say --to zolli "Build finished"`. The
    /// line waits in its speech bubble until clicked away or it expires; meanwhile the mascot's
    /// own remarks pause.
    Say {
        /// What to say; `-` reads it from standard input.
        #[arg(required = true)]
        text: Vec<String>,
        /// schraubi, zolli, verschnitti, all, or any (one of them; the default).
        #[arg(short, long)]
        to: Option<String>,
        /// What it wears: helmet for a warning, vest for something to be careful about, beard
        /// for musing; shades, tophat and party just for fun.
        #[arg(short, long, value_enum)]
        outfit: Option<mascots::Outfit>,
        /// Seconds the line stays up [default: 900]; 0 keeps it until clicked.
        #[arg(long = "for", value_name = "SECS")]
        secs: Option<u64>,
        /// Also open the dashboard.
        #[arg(long)]
        show: bool,
    },
    /// Print niri window rules for the standalone apps (floating, focused, the center third of the
    /// screen), as a file to `include` from niri's config.
    WindowRules {
        /// Write them to this file instead of printing them.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Install desktop entries ("Clock (niri-dash)", …) that launch the standalone apps, into
    /// ~/.local/share/applications.
    DesktopEntries {
        /// Remove them instead.
        #[arg(long)]
        remove: bool,
    },
    /// Render the dashboard offscreen to a PNG (for development).
    #[command(hide = true)]
    Snapshot {
        out: PathBuf,
        #[arg(long, default_value_t = 1706)]
        width: u32,
        #[arg(long, default_value_t = 1066)]
        height: u32,
        #[arg(long, default_value_t = 1.5)]
        scale: f32,
        #[arg(long, default_value_t = 3.0)]
        wait: f32,
        #[arg(long)]
        edit: bool,
        /// Render a standalone app (a widget name) instead of the dashboard.
        #[arg(long)]
        app: Option<String>,
    },
}

pub fn socket_path() -> PathBuf {
    named_socket_path("")
}

/// The control socket of a standalone app (`suffix` = its widget name), or the dashboard's ("").
pub fn named_socket_path(suffix: &str) -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
    let suffix = if suffix.is_empty() {
        String::new()
    } else {
        format!("-{suffix}")
    };
    dir.join(format!("niri-dash-{display}{suffix}.sock"))
}

/// `niri-dash app <widget>`: hand the request to a running instance, or become it.
fn app_client(widget: &str, toggle: bool, quit: bool) -> Result<()> {
    let Some(kind) = layout::Kind::from_str_opt(widget) else {
        return Err(unknown_widget(widget));
    };
    let control = if quit {
        Control::Quit
    } else if toggle {
        Control::Toggle
    } else {
        Control::Show
    };
    match UnixStream::connect(named_socket_path(kind.name())) {
        Ok(mut s) => {
            writeln!(s, "{}", control.as_ref()).map_err(Error::Send)?;
            Ok(())
        }
        Err(_) if quit => Ok(()),
        Err(_) => Ok(app::run_app(kind)?),
    }
}

fn client(cmd: Command) -> Result<()> {
    let control = match cmd {
        Command::Toggle => Control::Toggle,
        Command::Show => Control::Show,
        Command::Hide => Control::Hide,
        Command::Edit => Control::Edit,
        Command::Quit => Control::Quit,
        Command::Inhibit => Control::Inhibit,
        Command::Dnd => Control::Dnd,
        Command::Daemon { .. }
        | Command::Snapshot { .. }
        | Command::App { .. }
        | Command::Say { .. }
        | Command::DesktopEntries { .. }
        | Command::WindowRules { .. } => unreachable!("handled by the caller"),
    };
    match UnixStream::connect(socket_path()) {
        Ok(mut s) => {
            writeln!(s, "{}", control.as_ref()).map_err(Error::Send)?;
            Ok(())
        }
        Err(_) if matches!(control, Control::Toggle | Control::Show | Control::Edit) => {
            // Not running yet: become the daemon, showing right away.
            Ok(app::run(Some(if control == Control::Edit {
                Control::Edit
            } else {
                Control::Show
            }))?)
        }
        Err(_) if matches!(control, Control::Hide | Control::Quit) => Ok(()),
        Err(e) => Err(Error::NotRunning(e)),
    }
}

/// `niri-dash say`: hand a line to the dashboard and to a standalone mascot app, if running.
fn say(
    text: Vec<String>,
    to: Option<String>,
    outfit: Option<mascots::Outfit>,
    secs: Option<u64>,
    show: bool,
) -> Result<()> {
    let text = if text == ["-"] {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut s).map_err(Error::Stdin)?;
        s
    } else {
        text.join(" ")
    };
    let say = mascots::Say {
        text: mascots::clean(&text),
        to,
        outfit,
        secs,
    };
    if say.text.is_empty() {
        return Err(Error::NothingToSay);
    }
    say.target().map_err(Error::BadTarget)?;
    let send = |path: PathBuf, line: &str| -> bool {
        UnixStream::connect(path)
            .and_then(|mut s| writeln!(s, "{line}"))
            .is_ok()
    };
    let wire = say.to_wire();
    let dash = send(socket_path(), &wire);
    let app = send(named_socket_path(layout::Kind::Mascot.name()), &wire);
    if !dash && !app {
        return Err(Error::NobodyListening);
    }
    if show && dash {
        send(socket_path(), Control::Show.as_ref());
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("niri-dash: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,zbus=warn,system_tray=warn"),
    )
    .init();
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Daemon { show: false }) {
        Command::Daemon { show } => Ok(app::run(show.then_some(Control::Show))?),
        Command::App {
            widget,
            toggle,
            quit,
        } => app_client(&widget, toggle, quit),
        Command::Say {
            text,
            to,
            outfit,
            secs,
            show,
        } => say(text, to, outfit, secs, show),
        Command::DesktopEntries { remove } => Ok(desktop::entries(remove)?),
        Command::WindowRules { output } => Ok(desktop::window_rules(output.as_deref())?),
        Command::Snapshot {
            out,
            width,
            height,
            scale,
            wait,
            edit,
            app,
        } => {
            let solo = match app {
                Some(w) => Some(layout::Kind::from_str_opt(&w).ok_or_else(|| unknown_widget(&w))?),
                None => None,
            };
            Ok(app::snapshot(
                &out,
                width,
                height,
                scale,
                std::time::Duration::from_secs_f32(wait),
                edit,
                solo,
            )?)
        }
        cmd => client(cmd),
    }
}

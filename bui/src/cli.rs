//! The command line: the same commands as the Slint version's, handed to a running dashboard over
//! its control socket, or becoming the dashboard if there is none. The socket has a name of its
//! own, so this version and the Slint one can run side by side.

use crate::layout::Kind;
use crate::{Control, app, mascots, snapshot};
use clap::{Parser, Subcommand};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("sending command: {0}")]
    Send(#[source] std::io::Error),
    #[error("niri-dash-bui is not running: {0}")]
    NotRunning(#[source] std::io::Error),
    #[error("niri-dash-bui is not running")]
    NobodyListening,
    #[error("nothing to say")]
    NothingToSay,
    #[error("{0}")]
    BadTarget(String),
    #[error("night-light takes toggle, on, off or a temperature in kelvin, not {0:?}")]
    BadNightLight(String),
    #[error(transparent)]
    App(#[from] app::Error),
    #[error("{0}")]
    Snapshot(String),
}

#[derive(Parser)]
#[command(
    version,
    about = "niri-dash drawn with BUI, Blender's UI toolkit: a fullscreen widget dashboard for niri (and KDE Plasma)"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Clone)]
enum Command {
    /// Run the dashboard in the background (the default).
    Daemon {
        /// Show the dashboard right away.
        #[arg(long)]
        show: bool,
    },
    #[command(flatten)]
    Control(Control),
    /// Run one widget as a standalone app in its own window, e.g. `niri-dash-bui app clock`.
    App {
        #[arg(value_enum)]
        widget: Kind,
        /// Close it if it is open, show it otherwise.
        #[arg(long, conflicts_with = "quit")]
        toggle: bool,
        /// Stop it.
        #[arg(long)]
        quit: bool,
    },
    /// Make a mascot widget say something.
    Say {
        #[arg(required = true)]
        text: Vec<String>,
        #[arg(short, long)]
        to: Option<String>,
        #[arg(short, long, value_enum)]
        outfit: Option<mascots::Outfit>,
        #[arg(long = "for", value_name = "SECS")]
        secs: Option<u64>,
        #[arg(long)]
        show: bool,
    },
    /// Night light: toggle, on, off, or a temperature in kelvin.
    NightLight {
        #[arg(default_value = "toggle")]
        value: String,
    },
    /// Draw the dashboard offscreen into a PNG (for development).
    #[command(hide = true)]
    Snapshot(snapshot::Args),
}

pub fn socket_path() -> PathBuf {
    named_socket_path("")
}

/// The control socket of a standalone app (`suffix` = its widget name), or the dashboard's.
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
    dir.join(format!("niri-dash-bui-{display}{suffix}.sock"))
}

fn send(path: &Path, line: &str) -> Result<(), Error> {
    let mut s = UnixStream::connect(path).map_err(Error::NotRunning)?;
    writeln!(s, "{line}").map_err(Error::Send)
}

fn client(control: Control) -> Result<(), Error> {
    match send(&socket_path(), control.as_ref()) {
        Err(Error::NotRunning(_))
            if matches!(control, Control::Toggle | Control::Show | Control::Edit) =>
        {
            let first = if control == Control::Toggle {
                Control::Show
            } else {
                control
            };
            Ok(app::run(Some(first), None)?)
        }
        Err(Error::NotRunning(_)) if matches!(control, Control::Hide | Control::Quit) => Ok(()),
        sent => sent,
    }
}

fn app_client(kind: Kind, toggle: bool, quit: bool) -> Result<(), Error> {
    let control = if quit {
        Control::Quit
    } else if toggle {
        Control::Toggle
    } else {
        Control::Show
    };
    match send(&named_socket_path(kind.name()), control.as_ref()) {
        Err(Error::NotRunning(_)) if quit => Ok(()),
        Err(Error::NotRunning(_)) => Ok(app::run(None, Some(kind))?),
        sent => sent,
    }
}

pub fn main() -> std::process::ExitCode {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,zbus=warn,system_tray=warn"),
    )
    .init();
    let result = run();
    crate::files::flush();
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("niri-dash-bui: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Error> {
    match Cli::parse()
        .command
        .unwrap_or(Command::Daemon { show: false })
    {
        Command::Daemon { show } => Ok(app::run(show.then_some(Control::Show), None)?),
        Command::Control(c) => client(c),
        Command::App {
            widget,
            toggle,
            quit,
        } => app_client(widget, toggle, quit),
        Command::Say {
            text,
            to,
            outfit,
            secs,
            show,
        } => {
            let say = mascots::Say {
                text: mascots::clean(&text.join(" ")),
                to,
                outfit,
                secs,
            };
            if say.text.is_empty() {
                return Err(Error::NothingToSay);
            }
            say.target().map_err(Error::BadTarget)?;
            let wire = say.to_wire();
            let dash = send(&socket_path(), &wire).is_ok();
            let app = send(&named_socket_path(Kind::Mascot.name()), &wire).is_ok();
            if !dash && !app {
                return Err(Error::NobodyListening);
            }
            if show && dash {
                let _ = send(&socket_path(), Control::Show.as_ref());
            }
            Ok(())
        }
        Command::NightLight { value } => {
            let cmd = niri_dash_system::night_light::Cmd::parse(&value)
                .ok_or(Error::BadNightLight(value))?;
            send(&socket_path(), &cmd.to_wire())
        }
        Command::Snapshot(args) => snapshot::run(&args).map_err(Error::Snapshot),
    }
}

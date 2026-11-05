//! The command line: parsing it, and handing what it asks for to a running dashboard over its
//! control socket, or becoming the dashboard if there is none.

use crate::{Control, app, desktop, layout, mascots};
use clap::{Parser, Subcommand};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

/// What the command line reports when a command fails.
#[derive(Debug, thiserror::Error)]
enum Error {
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
    #[error("night-light takes toggle, on, off or a temperature in kelvin, not {0:?}")]
    BadNightLight(String),
    #[error(transparent)]
    App(#[from] app::Error),
    #[error(transparent)]
    Desktop(#[from] desktop::Error),
}

type Result<T, E = Error> = std::result::Result<T, E>;

/// The part of `niri-dash --help` for Plasma users.
const PLASMA_HELP: &str = "\
On KDE Plasma (6):
  Start it at login: System Settings > Autostart > Add New > Login Script, and pick the niri-dash
  binary (or put a .desktop file with Exec=niri-dash daemon into ~/.config/autostart).
  Bind a key to `niri-dash toggle`: System Settings > Keyboard > Shortcuts > Add New >
  Command or Script.

  Plasma keeps showing the notifications; the notification widget's bell and `niri-dash dnd`
  pause Plasma's pop-ups. Night light is KWin's: the switch and `niri-dash night-light` pause
  or resume its schedule while niri-dash runs; its times and warmth are set in System Settings.
  Keyboard layouts, the color picker and Log out go through Plasma.
  Not available on Plasma: the window list, focusing a Claude session's window, and noticing
  screen sharing. `niri-dash window-rules` writes rules for niri only.";

#[derive(Parser)]
#[command(
    version,
    about = "A fullscreen widget dashboard for niri (and KDE Plasma)",
    after_long_help = PLASMA_HELP
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Clone)]
enum Command {
    /// Run the dashboard in the background (the default). Put this in niri's spawn-at-startup (or
    /// Plasma's Autostart).
    Daemon {
        /// Show the dashboard right away.
        #[arg(long)]
        show: bool,
    },
    #[command(flatten)]
    Control(Control),
    /// Run one widget as a standalone app in its own window, e.g. `niri-dash app clock`. If it is
    /// already running, show and focus it.
    App {
        /// The widget, as named in config.kdl.
        #[arg(value_enum)]
        widget: layout::Kind,
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
        /// schraubi, zolli, verschnitti, cow, all, or any (one of them; the default).
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
    /// Make the screen warmer for the evening: `niri-dash night-light` toggles it, `on`/`off`
    /// switch it, a temperature like `4000` (in kelvin, 1000–6500) turns it on at that warmth.
    NightLight {
        #[arg(default_value = "toggle")]
        value: String,
    },
    /// Print niri window rules for the standalone apps (floating, focused, the center third of the
    /// screen), as a file to `include` from niri's config. For niri only: Plasma has its window
    /// rules in System Settings.
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
    Snapshot(app::snapshot::Args),
    /// Press random keys at the dashboard, on made-up data, and check what they do (for
    /// development). Changes nothing on the system: its files go to a temporary directory.
    #[command(hide = true)]
    Stress(app::stress::Args),
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

/// Write `line` to the control socket at `path`: `Error::NotRunning` if nothing listens there.
fn send(path: &Path, line: &str) -> Result<()> {
    let mut s = UnixStream::connect(path).map_err(Error::NotRunning)?;
    writeln!(s, "{line}").map_err(Error::Send)
}

/// `niri-dash app <widget>`: hand the request to a running instance, or become it.
fn app_client(kind: layout::Kind, toggle: bool, quit: bool) -> Result<()> {
    let control = if quit {
        Control::Quit
    } else if toggle {
        Control::Toggle
    } else {
        Control::Show
    };
    match send(&named_socket_path(kind.name()), control.as_ref()) {
        Err(Error::NotRunning(_)) if quit => Ok(()),
        Err(Error::NotRunning(_)) => Ok(app::run_app(kind)?),
        sent => sent,
    }
}

/// Send `control` to the dashboard.
fn client(control: Control) -> Result<()> {
    match send(&socket_path(), control.as_ref()) {
        Err(Error::NotRunning(_))
            if matches!(control, Control::Toggle | Control::Show | Control::Edit) =>
        {
            // Not running yet: become the daemon, showing (or editing) right away.
            let first = if control == Control::Toggle {
                Control::Show
            } else {
                control
            };
            Ok(app::run(Some(first))?)
        }
        Err(Error::NotRunning(_)) if matches!(control, Control::Hide | Control::Quit) => Ok(()),
        sent => sent,
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
    let wire = say.to_wire();
    let dash = send(&socket_path(), &wire).is_ok();
    let app = send(&named_socket_path(layout::Kind::Mascot.name()), &wire).is_ok();
    if !dash && !app {
        return Err(Error::NobodyListening);
    }
    if show && dash {
        // The line is delivered either way.
        let _ = send(&socket_path(), Control::Show.as_ref());
    }
    Ok(())
}

/// The command line: parse it, run the command, and report how it went.
pub fn main() -> std::process::ExitCode {
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
        } => app_client(widget, toggle, quit),
        Command::Say {
            text,
            to,
            outfit,
            secs,
            show,
        } => say(text, to, outfit, secs, show),
        Command::DesktopEntries { remove } => Ok(desktop::entries(remove)?),
        Command::WindowRules { output } => Ok(desktop::window_rules(output.as_deref())?),
        Command::Snapshot(args) => Ok(app::snapshot::run(&args, args.app)?),
        Command::Stress(args) => Ok(app::stress::run(&args)?),
        Command::NightLight { value } => {
            let cmd = niri_dash_system::night_light::Cmd::parse(&value)
                .ok_or(Error::BadNightLight(value))?;
            send(&socket_path(), &cmd.to_wire())
        }
        Command::Control(control) => client(control),
    }
}

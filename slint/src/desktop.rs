//! Desktop entries for the standalone apps (`niri-dash app <widget>`), so launchers list them as
//! "Clock (niri-dash)" and so on. The window's app id matches the entry's file name, which is how
//! launchers and niri connect a window to its entry (and how window rules can match it). On KDE
//! Plasma the entries also let niri-dash see the window list: KWin gives that only to programs
//! whose desktop file asks for it.

use crate::layout::Kind;
use std::path::{Path, PathBuf};
use strum::IntoEnumIterator;

/// A file operation on a desktop entry (or the window rules) failed.
#[derive(Debug, thiserror::Error)]
#[error("{action} {}: {source}", path.display())]
pub struct Error {
    action: &'static str,
    path: PathBuf,
    #[source]
    source: std::io::Error,
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Wraps an io error with what was being done to which file.
fn io_err(action: &'static str, path: &Path) -> impl FnOnce(std::io::Error) -> Error {
    let path = path.to_path_buf();
    move |source| Error {
        action,
        path,
        source,
    }
}

/// Marks entries we wrote, so removing them never touches anything else.
const MARKER: &str = "X-Niri-Dash-App";

/// What KWin hands only to programs whose desktop file asks for it (matched by the program the
/// entry's Exec runs, so every entry we write covers the dashboard too): the window list.
const KWIN_INTERFACES: &str = "org_kde_plasma_window_management";

pub fn app_id(kind: Kind) -> String {
    format!("niri-dash-{}", kind.name())
}

/// The window title and the launcher name.
pub fn name(kind: Kind) -> String {
    format!("{} (niri-dash)", kind.info().title)
}

/// Whether a kind gets an entry. Notifications don't: while the dashboard runs it owns the
/// notification history, and a separate app would show an empty list.
fn listed(kind: Kind) -> bool {
    kind != Kind::Notifications
}

fn dir() -> PathBuf {
    crate::files::data_home().join("applications")
}

/// How the entries start us: plain `niri-dash` when that is this binary on $PATH, else the full
/// path (e.g. a build in target/).
fn exec() -> String {
    let me = std::env::current_exe().ok();
    let on_path = niri_dash_system::which("niri-dash");
    let same = match (&me, &on_path) {
        (Some(a), Some(b)) => a.canonicalize().ok() == b.canonicalize().ok(),
        _ => false,
    };
    match me {
        Some(p) if !same => {
            let p = p.to_string_lossy().into_owned();
            if p.contains(' ') {
                format!("\"{p}\"")
            } else {
                p
            }
        }
        _ => "niri-dash".into(),
    }
}

fn entry(kind: Kind, exec: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={name}\n\
         GenericName={title}\n\
         Comment={comment}\n\
         Exec={exec} app {widget}\n\
         Icon={icon}\n\
         Terminal=false\n\
         Categories=Utility;\n\
         StartupWMClass={id}\n\
         SingleMainWindow=true\n\
         X-KDE-Wayland-Interfaces={KWIN_INTERFACES}\n\
         {MARKER}=true\n",
        name = name(kind),
        title = kind.info().title,
        comment = kind.info().description,
        widget = kind.name(),
        icon = kind.info().icon,
        id = app_id(kind),
    )
}

/// Remove the entries we wrote earlier; returns how many.
fn remove_ours() -> Result<usize> {
    let Ok(read) = std::fs::read_dir(dir()) else {
        return Ok(0);
    };
    let mut n = 0;
    for e in read.flatten() {
        let path = e.path();
        let ours = path
            .file_name()
            .and_then(|f| f.to_str())
            .is_some_and(|f| f.starts_with("niri-dash-") && f.ends_with(".desktop"))
            && std::fs::read_to_string(&path).is_ok_and(|t| t.contains(MARKER));
        if ours {
            std::fs::remove_file(&path).map_err(io_err("removing", &path))?;
            n += 1;
        }
    }
    Ok(n)
}

/// `niri-dash desktop-entries [--remove]`.
pub fn entries(remove: bool) -> Result<()> {
    let removed = remove_ours()?;
    if remove {
        println!("removed {removed} desktop entries from {}", dir().display());
        return Ok(());
    }
    let dir = dir();
    std::fs::create_dir_all(&dir).map_err(io_err("creating", &dir))?;
    let exec = exec();
    let mut n = 0;
    for kind in Kind::iter().filter(|&k| listed(k)) {
        let path = dir.join(format!("{}.desktop", app_id(kind)));
        std::fs::write(&path, entry(kind, &exec)).map_err(io_err("writing", &path))?;
        n += 1;
    }
    println!(
        "installed {n} desktop entries into {} (Exec={exec})",
        dir.display()
    );
    Ok(())
}

/// The corner radius of the configured theme (style plus config.kdl adjustments), so niri can
/// round the app windows like the dashboard's cards.
fn theme_radius() -> f32 {
    let layout = crate::layout::Layout::read().layout;
    let (look, _) =
        crate::theme::load_look(&layout.colorscheme, &layout.style, &layout.style_overrides);
    look.style.radius
}

/// The recommended niri window rules for the standalone apps, as a file to `include`.
fn window_rules_text() -> String {
    let radius = theme_radius().round() as i32;
    format!(
        r#"// niri window rules for niri-dash's standalone apps (`niri-dash app <widget>`).
// Generated by `niri-dash window-rules`; regenerate it after changing the theme's corner radius.
//
// Use it from ~/.config/niri/config.kdl (niri 25.11 or later):
//
//     include "niri-dash.kdl"
//
// Rules apply in order and later ones win: include this after your own window rules to let these
// take precedence, or before them to override these.

// All apps are quick one-off tools: they float (above the tiled windows), take focus, and open in
// the center third of the screen. Floating windows open centered unless a rule places them.
window-rule {{
    match app-id="^niri-dash-"
    open-floating true
    open-focused true
    default-column-width {{ proportion 0.33333; }}
    default-window-height {{ proportion 0.33333; }}
    geometry-corner-radius {radius}
    clip-to-geometry true
}}

// Clipboard history can hold passwords and other secrets: keep it out of screencasts and
// screenshots (niri's own interactive screenshot UI still shows it).
window-rule {{
    match app-id="^niri-dash-clipboard$"
    block-out-from "screen-capture"
}}

// If you want clipboard history, a key for it, for your binds {{ }} section (niri's default config
// uses Mod+V for toggle-window-floating, so pick one that's free). It starts recording what you
// copy on the first press, and stops with `niri-dash app clipboard --quit`:
//
//     Mod+Ctrl+V {{ spawn "niri-dash" "app" "clipboard" "--toggle"; }}
"#
    )
}

/// `niri-dash window-rules [-o FILE]`.
pub fn window_rules(output: Option<&Path>) -> Result<()> {
    if niri_dash_system::guess_compositor() != niri_dash_system::Compositor::Niri {
        eprintln!(
            "niri-dash: these rules are for niri's config; on KDE Plasma, set up window rules in \
             System Settings (Window Management → Window Rules) matching window class niri-dash-*"
        );
    }
    let text = window_rules_text();
    match output {
        None => print!("{text}"),
        Some(path) => {
            std::fs::write(path, text).map_err(io_err("writing", path))?;
            eprintln!(
                "wrote {}; add `include \"{}\"` to your niri config",
                path.display(),
                path.display()
            );
        }
    }
    Ok(())
}

//! Icon lookup: freedesktop icon themes, and app ids to desktop-file icons.
#![allow(clippy::disallowed_types)] // Still to be moved over to an actor.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

const THEMES: [&str; 3] = ["breeze-dark", "breeze", "Adwaita"];

/// Lookup results, misses included.
type Cache<K> = LazyLock<Mutex<HashMap<K, Option<PathBuf>>>>;

/// Theme lookups by icon name and size.
static THEME_CACHE: Cache<(String, u16)> = LazyLock::new(Default::default);
/// Desktop-file icons by app id.
static APP_CACHE: Cache<String> = LazyLock::new(Default::default);

fn user_theme() -> Option<String> {
    static THEME: LazyLock<Option<String>> = LazyLock::new(|| {
        let out = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "icon-theme"])
            .output()
            .ok()?;
        let s = String::from_utf8_lossy(&out.stdout)
            .trim()
            .trim_matches('\'')
            .to_string();
        (!s.is_empty()).then_some(s)
    });
    THEME.clone()
}

/// Resolve an icon name (or absolute path) to a file.
pub fn lookup(name: &str, size: u16) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    if name.starts_with('/') {
        return Path::new(name).is_file().then(|| PathBuf::from(name));
    }
    let key = (name.to_string(), size);
    if let Some(hit) = THEME_CACHE.lock().ok()?.get(&key) {
        return hit.clone();
    }
    let mut themes: Vec<String> = user_theme().into_iter().collect();
    themes.extend(THEMES.iter().map(|s| s.to_string()));
    let found = themes
        .iter()
        .find_map(|t| {
            freedesktop_icons::lookup(name)
                .with_size(size)
                .with_theme(t)
                .find()
        })
        .or_else(|| freedesktop_icons::lookup(name).with_size(size).find())
        .or_else(|| {
            ["png", "svg", "xpm"]
                .map(|e| Path::new("/usr/share/pixmaps").join(format!("{name}.{e}")))
                .into_iter()
                .find(|p| p.is_file())
        })
        // Slint can't decode xpm.
        .filter(|p| p.extension().is_some_and(|e| e != "xpm"));
    if let Ok(mut c) = THEME_CACHE.lock() {
        c.insert(key, found.clone());
    }
    found
}

/// Look up an icon inside an extra directory first (tray items may ship their own theme path).
pub fn lookup_in(dir: Option<&str>, name: &str, size: u16) -> Option<PathBuf> {
    if let Some(dir) = dir.filter(|d| !d.is_empty()) {
        for sub in [
            "",
            "hicolor/scalable/apps",
            "hicolor/48x48/apps",
            "hicolor/32x32/apps",
            "hicolor/24x24/apps",
            "hicolor/22x22/apps",
        ] {
            for ext in ["png", "svg"] {
                let p = Path::new(dir).join(sub).join(format!("{name}.{ext}"));
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    lookup(name, size)
}

fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![dirs::data_dir().unwrap_or_default()];
    let sys =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(std::env::split_paths(&sys));
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share"));
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/share/flatpak/exports/share"));
    }
    dirs
}

fn desktop_icon(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if in_entry && let Some(v) = line.strip_prefix("Icon=") {
            return Some(v.trim().to_string());
        }
    }
    None
}

fn wm_class_matches(path: &Path, app_id: &str) -> bool {
    std::fs::read_to_string(path)
        .map(|t| {
            t.lines().any(|l| {
                l.trim()
                    .strip_prefix("StartupWMClass=")
                    .is_some_and(|v| v.trim().eq_ignore_ascii_case(app_id))
            })
        })
        .unwrap_or(false)
}

/// The icon file for a Wayland app id (or a desktop file name, as notifications give it), via
/// its desktop file.
pub fn app_icon(app_id: &str) -> Option<PathBuf> {
    if app_id.is_empty() {
        return None;
    }
    if let Some(hit) = APP_CACHE.lock().ok()?.get(app_id) {
        return hit.clone();
    }
    let dirs: Vec<PathBuf> = data_dirs()
        .into_iter()
        .map(|d| d.join("applications"))
        .filter(|d| d.is_dir())
        .collect();
    let lower = app_id.to_lowercase();
    let last = app_id.rsplit('.').next().unwrap_or(app_id).to_lowercase();
    let candidates = [app_id.to_string(), lower.clone(), last.clone()];
    let mut desktop = None;
    'outer: for name in &candidates {
        for d in &dirs {
            let p = d.join(format!("{name}.desktop"));
            if p.is_file() {
                desktop = Some(p);
                break 'outer;
            }
        }
    }
    if desktop.is_none() {
        // Fall back to scanning for StartupWMClass or a case-insensitive file name match.
        'scan: for d in &dirs {
            let Ok(rd) = std::fs::read_dir(d) else {
                continue;
            };
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().is_none_or(|x| x != "desktop") {
                    continue;
                }
                let stem = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if stem == lower
                    || stem.ends_with(&format!(".{last}"))
                    || wm_class_matches(&p, app_id)
                {
                    desktop = Some(p);
                    break 'scan;
                }
            }
        }
    }
    let icon = desktop
        .as_deref()
        .and_then(desktop_icon)
        .and_then(|name| lookup(&name, 48))
        .or_else(|| lookup(app_id, 48))
        .or_else(|| lookup(&lower, 48));
    if let Ok(mut c) = APP_CACHE.lock() {
        c.insert(app_id.to_string(), icon.clone());
    }
    icon
}

//! Icon lookup: freedesktop icon themes, and app ids to desktop-file icons.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use tokio::sync::oneshot;

const THEMES: [&str; 3] = ["breeze-dark", "breeze", "Adwaita"];

/// Icon lookups and what they found, misses included: they read the disk. Whoever looks icons up
/// owns one of these.
#[derive(Default)]
pub struct Icons {
    /// By icon name and size.
    themes: HashMap<(String, u16), Option<PathBuf>>,
    /// Desktop-file icons by app id.
    apps: HashMap<String, Option<PathBuf>>,
}

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

impl Icons {
    /// Resolve an icon name (or absolute path) to a file.
    pub fn lookup(&mut self, name: &str, size: u16) -> Option<PathBuf> {
        if name.is_empty() {
            return None;
        }
        if name.starts_with('/') {
            return Path::new(name).is_file().then(|| PathBuf::from(name));
        }
        let key = (name.to_string(), size);
        if let Some(hit) = self.themes.get(&key) {
            return hit.clone();
        }
        let found = find_in_themes(name, size);
        self.themes.insert(key, found.clone());
        found
    }

    /// Look up an icon inside an extra directory first (tray items may ship their own theme path).
    pub fn lookup_in(&mut self, dir: Option<&str>, name: &str, size: u16) -> Option<PathBuf> {
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
        self.lookup(name, size)
    }

    /// The icon file for a Wayland app id (or a desktop file name, as notifications give it), via
    /// its desktop file.
    pub fn app_icon(&mut self, app_id: &str) -> Option<PathBuf> {
        if app_id.is_empty() {
            return None;
        }
        if let Some(hit) = self.apps.get(app_id) {
            return hit.clone();
        }
        let lower = app_id.to_lowercase();
        let icon = find_desktop_file(app_id)
            .as_deref()
            .and_then(desktop_icon)
            .and_then(|name| self.lookup(&name, 48))
            .or_else(|| self.lookup(app_id, 48))
            .or_else(|| self.lookup(&lower, 48));
        self.apps.insert(app_id.to_string(), icon.clone());
        icon
    }
}

enum Ask {
    Lookup(String, u16, oneshot::Sender<Option<PathBuf>>),
    AppIcon(String, oneshot::Sender<Option<PathBuf>>),
}

/// For who can't own [`Icons`] (a D-Bus server's methods only get `&self`): a thread that does, and
/// answers. It ends with the last clone of this.
#[derive(Clone)]
pub struct IconThread(std::sync::mpsc::Sender<Ask>);

impl IconThread {
    pub fn start() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("niri-dash-icons".into())
            .spawn(move || {
                let mut icons = Icons::default();
                while let Ok(ask) = rx.recv() {
                    match ask {
                        Ask::Lookup(name, size, reply) => {
                            let _ = reply.send(icons.lookup(&name, size));
                        }
                        Ask::AppIcon(id, reply) => {
                            let _ = reply.send(icons.app_icon(&id));
                        }
                    }
                }
            })
            .expect("spawning the icon thread");
        Self(tx)
    }

    /// See [`Icons::lookup`].
    pub async fn lookup(&self, name: &str, size: u16) -> Option<PathBuf> {
        let (reply, answer) = oneshot::channel();
        self.0.send(Ask::Lookup(name.into(), size, reply)).ok()?;
        answer.await.ok()?
    }

    /// See [`Icons::app_icon`].
    pub async fn app_icon(&self, app_id: &str) -> Option<PathBuf> {
        let (reply, answer) = oneshot::channel();
        self.0.send(Ask::AppIcon(app_id.into(), reply)).ok()?;
        answer.await.ok()?
    }
}

fn find_in_themes(name: &str, size: u16) -> Option<PathBuf> {
    let mut themes: Vec<String> = user_theme().into_iter().collect();
    themes.extend(THEMES.iter().map(|s| s.to_string()));
    themes
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
        .filter(|p| p.extension().is_some_and(|e| e != "xpm"))
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

/// The desktop file of a Wayland app id.
fn find_desktop_file(app_id: &str) -> Option<PathBuf> {
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
    desktop
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file that exists, wherever the tests run.
    const THIS_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");

    #[test]
    fn a_path_is_taken_as_it_is() {
        let mut icons = Icons::default();
        assert_eq!(icons.lookup(THIS_FILE, 48), Some(PathBuf::from(THIS_FILE)));
        assert_eq!(icons.lookup("/no/such/file.png", 48), None);
    }

    #[test]
    fn no_name_no_icon() {
        let mut icons = Icons::default();
        assert_eq!(icons.lookup("", 48), None);
        assert_eq!(icons.app_icon(""), None);
    }

    #[test]
    fn a_miss_is_remembered() {
        let mut icons = Icons::default();
        let name = "niri-dash-no-such-icon";
        assert_eq!(icons.lookup(name, 48), None);
        assert_eq!(icons.themes.get(&(name.to_string(), 48)), Some(&None));
        assert_eq!(icons.app_icon(name), None);
        assert_eq!(icons.apps.get(name), Some(&None));
    }

    #[test]
    fn an_extra_directory_comes_first() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/icons");
        let Some(name) = std::fs::read_dir(dir).ok().and_then(|mut d| {
            d.find_map(|e| {
                let p = e.ok()?.path();
                (p.extension()? == "svg")
                    .then(|| p.file_stem().map(|s| s.to_string_lossy().into_owned()))?
            })
        }) else {
            panic!("no icons in {dir}");
        };
        let mut icons = Icons::default();
        let found = icons.lookup_in(Some(dir), &name, 48).expect("found");
        assert!(found.starts_with(dir));
    }

    #[tokio::test]
    async fn the_thread_answers() {
        let icons = IconThread::start();
        assert_eq!(
            icons.lookup(THIS_FILE, 48).await,
            Some(PathBuf::from(THIS_FILE))
        );
        assert_eq!(icons.app_icon("").await, None);
    }
}

//! Where niri-dash keeps its files, and reading and writing them: on disk, with writes that survive
//! a crash halfway.

use std::path::{Path, PathBuf};

/// A base directory from `dirs`, which only lacks one without a home directory; the current
/// directory stands in then.
fn or_here(dir: Option<PathBuf>) -> PathBuf {
    dir.unwrap_or_else(|| PathBuf::from("."))
}

/// `$NIRI_DASH_HOME/<sub>` if that is set: one directory for everything niri-dash keeps, which is
/// how tests and the stress run stay off the real files, whatever the platform's own rules for
/// where they go.
fn under_home(sub: &str) -> Option<PathBuf> {
    std::env::var_os("NIRI_DASH_HOME").map(|home| PathBuf::from(home).join(sub))
}

/// `$XDG_CONFIG_HOME`, usually `~/.config`.
pub fn config_home() -> PathBuf {
    under_home("config").unwrap_or_else(|| or_here(dirs::config_dir()))
}

/// `$XDG_DATA_HOME`, usually `~/.local/share`.
pub fn data_home() -> PathBuf {
    under_home("data").unwrap_or_else(|| or_here(dirs::data_dir()))
}

/// `$XDG_STATE_HOME`, usually `~/.local/state`. Where there is no such directory, the data
/// directory.
pub fn state_home() -> PathBuf {
    under_home("state").unwrap_or_else(|| or_here(dirs::state_dir().or_else(dirs::data_dir)))
}

pub fn read(path: &Path) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}

pub fn exists(path: &Path) -> bool {
    path.exists()
}

/// Write `path` through a temporary file next to it, so a crash mid-write doesn't leave half a
/// file. Creates the directory if needed.
pub fn write_atomic(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}

/// The files in a directory; none if it can't be read.
pub fn list(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

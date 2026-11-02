//! Where niri-dash keeps its files, and writing them safely.

use std::path::{Path, PathBuf};

/// A base directory from `dirs`, which only lacks one without a home directory; the current
/// directory stands in then.
fn or_here(dir: Option<PathBuf>) -> PathBuf {
    dir.unwrap_or_else(|| PathBuf::from("."))
}

/// `$XDG_CONFIG_HOME`, usually `~/.config`.
pub fn config_home() -> PathBuf {
    or_here(dirs::config_dir())
}

/// `$XDG_DATA_HOME`, usually `~/.local/share`.
pub fn data_home() -> PathBuf {
    or_here(dirs::data_dir())
}

/// `$XDG_STATE_HOME`, usually `~/.local/state`.
pub fn state_home() -> PathBuf {
    or_here(dirs::state_dir())
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

//! Where niri-dash keeps its files, and reading and writing them: on disk, with writes that survive
//! a crash halfway; in a browser, in the page's localStorage, where the same paths work as keys.

use std::path::PathBuf;

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

#[cfg(not(target_arch = "wasm32"))]
pub use disk::*;
#[cfg(target_arch = "wasm32")]
pub use page::*;

#[cfg(not(target_arch = "wasm32"))]
mod disk {
    use std::path::{Path, PathBuf};

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
}

#[cfg(target_arch = "wasm32")]
mod page {
    use std::io::{Error, ErrorKind};
    use std::path::{Path, PathBuf};
    use web_sys::Storage;

    const PREFIX: &str = "niri-dash:";

    fn storage() -> std::io::Result<Storage> {
        // Absent or refused in some private windows: then nothing is kept, and that is all.
        web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .ok_or_else(|| Error::new(ErrorKind::Unsupported, "no localStorage"))
    }

    fn key(path: &Path) -> String {
        format!("{PREFIX}{}", path.display())
    }

    pub fn read(path: &Path) -> std::io::Result<String> {
        storage()?
            .get_item(&key(path))
            .ok()
            .flatten()
            .ok_or_else(|| Error::from(ErrorKind::NotFound))
    }

    pub fn exists(path: &Path) -> bool {
        read(path).is_ok()
    }

    /// A write to localStorage is whole or doesn't happen.
    pub fn write_atomic(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
        let text = std::str::from_utf8(contents.as_ref())
            .map_err(|e| Error::new(ErrorKind::InvalidData, e))?;
        storage()?
            .set_item(&key(path), text)
            .map_err(|_| Error::new(ErrorKind::StorageFull, "localStorage is full"))
    }

    /// The files in a directory.
    pub fn list(dir: &Path) -> Vec<PathBuf> {
        let Ok(storage) = storage() else {
            return Vec::new();
        };
        let wanted = format!("{}/", key(dir));
        (0..storage.length().unwrap_or(0))
            .filter_map(|i| storage.key(i).ok().flatten())
            .filter_map(|k| k.strip_prefix(&wanted).map(str::to_string))
            .filter(|name| !name.contains('/'))
            .map(|name| dir.join(name))
            .collect()
    }

    /// Everything the page keeps: for starting over.
    pub fn clear() {
        let Ok(storage) = storage() else {
            return;
        };
        let keys: Vec<String> = (0..storage.length().unwrap_or(0))
            .filter_map(|i| storage.key(i).ok().flatten())
            .filter(|k| k.starts_with(PREFIX))
            .collect();
        for k in keys {
            let _ = storage.remove_item(&k);
        }
    }
}

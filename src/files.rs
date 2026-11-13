//! Where niri-dash keeps its files, and reading and writing them: on disk, with writes that survive
//! a crash halfway; in a browser, in the page's localStorage, where the same paths work as keys.

use std::path::PathBuf;

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

    /// Write `path` as [`write_atomic`] does, on a thread of its own: the UI doesn't wait for the
    /// disk, which now and then takes tens of milliseconds, right as an animation starts (a drop,
    /// a theme picked). Writes happen in the order asked; [`flush`] waits for them. A write that
    /// fails is logged.
    pub fn write_later(path: PathBuf, contents: Vec<u8>) {
        if writer()
            .send(Job::Write(path.clone(), contents.clone()))
            .is_err()
        {
            // The writer is gone (it never is, short of a panic): write here instead.
            if let Err(e) = write_atomic(&path, contents) {
                log::warn!("writing {}: {e}", path.display());
            }
        }
    }

    /// Wait until everything [`write_later`] was given is on disk.
    pub fn flush() {
        let (done, wait) = std::sync::mpsc::channel();
        if writer().send(Job::Flush(done)).is_ok() {
            let _ = wait.recv();
        }
    }

    enum Job {
        Write(PathBuf, Vec<u8>),
        Flush(std::sync::mpsc::Sender<()>),
    }

    /// The writer: a thread that owns the writing, fed through a channel.
    fn writer() -> &'static std::sync::mpsc::Sender<Job> {
        static WRITER: std::sync::OnceLock<std::sync::mpsc::Sender<Job>> =
            std::sync::OnceLock::new();
        WRITER.get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<Job>();
            std::thread::Builder::new()
                .name("niri-dash-files".into())
                .spawn(move || {
                    for job in rx {
                        match job {
                            Job::Write(path, contents) => {
                                if let Err(e) = write_atomic(&path, contents) {
                                    log::warn!("writing {}: {e}", path.display());
                                }
                            }
                            Job::Flush(done) => {
                                let _ = done.send(());
                            }
                        }
                    }
                })
                .expect("starting the file writer");
            tx
        })
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
    /// As on disk; localStorage is fast, so it is written right away.
    pub fn write_later(path: PathBuf, contents: Vec<u8>) {
        if let Err(e) = write_atomic(&path, contents) {
            log::warn!("writing {}: {e}", path.display());
        }
    }

    pub fn flush() {}

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

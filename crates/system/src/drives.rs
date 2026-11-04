//! USB drives and SD cards, from UDisks.

use crate::perf::human_bytes;

/// A filesystem (or a locked encrypted one) on a removable drive.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Volume {
    /// The block device's object path.
    pub path: String,
    pub name: String,
    pub size: u64,
    /// Where it is mounted, if it is.
    pub mount: Option<String>,
    /// Encrypted and not unlocked (this version doesn't unlock).
    pub locked: bool,
    /// For an unlocked encrypted volume: the encrypted block to lock again on eject.
    pub backing: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Drive {
    /// The drive's object path.
    pub path: String,
    pub name: String,
    pub size: u64,
    pub can_power_off: bool,
    pub ejectable: bool,
    pub volumes: Vec<Volume>,
}

impl Drive {
    /// "16 GiB · mounted at /run/media/me/STICK", "not mounted", "locked".
    pub fn summary(&self) -> String {
        let mounted: Vec<&str> = self
            .volumes
            .iter()
            .filter_map(|v| v.mount.as_deref())
            .collect();
        let state = match mounted.as_slice() {
            [] if self.volumes.iter().all(|v| v.locked) => "locked".to_string(),
            [] => "not mounted".to_string(),
            [one] => format!("mounted at {one}"),
            more => format!("{} mounted", more.len()),
        };
        if self.size > 0 {
            format!("{} · {state}", human_bytes(self.size))
        } else {
            state
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub available: bool,
    pub drives: Vec<Drive>,
}

/// Volumes are named by their block device, drives by their UDisks path.
#[derive(Debug)]
pub enum Command {
    Mount(String),
    Unmount(String),
    Eject(String),
    EjectAll,
    /// Open a mount point in the file manager.
    Open(String),
}

#[derive(Debug)]
pub enum Event {
    State(State),
}

//! USB drives and SD cards, from UDisks.

pub use crate::sources::udisks::{Drive, State, Volume};

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

//! Phones paired through KDE Connect.

pub use crate::sources::kdeconnect::{Phone, State};

/// Phones are named by their KDE Connect id.
#[derive(Debug)]
pub enum Command {
    Ring(String),
    /// Pick files with the desktop's file chooser and send them.
    SendFiles(String),
}

#[derive(Debug)]
pub enum Event {
    State(State),
}

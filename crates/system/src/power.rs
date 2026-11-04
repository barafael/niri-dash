//! Battery, power profiles, and blocking sleep.

pub use crate::sources::inhibit::Blocker;
pub use crate::sources::power::{DeviceKind, Profile, State};

#[derive(Debug)]
pub enum Command {
    SetProfile(Profile),
    /// Block sleep and screen locking, or stop blocking them.
    ToggleInhibit,
}

#[derive(Debug)]
pub enum Event {
    State(State),
    /// Whether we block sleep and screen locking right now.
    Inhibited(bool),
    /// What blocks sleep right now (from logind).
    SleepBlockers(Vec<Blocker>),
}

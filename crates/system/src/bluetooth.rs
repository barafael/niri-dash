//! Bluetooth, from BlueZ.

pub use crate::sources::bluetooth::{Activity, Answer, Device, Prompt, PromptKind, State, facts};

/// Devices are named by their address.
#[derive(Debug)]
pub enum Command {
    /// Turn the adapter on or off.
    Toggle,
    Scan(bool),
    Connect(String),
    Disconnect(String),
    SetTrusted(String, bool),
    SetBlocked(String, bool),
    Forget(String),
    Rename(String, String),
    Pair(String),
    CancelPairing,
    /// The answer to the pairing prompt with this serial.
    Answer {
        serial: u64,
        answer: Answer,
    },
    /// Say no to the pairing prompt.
    Decline,
    /// Show this device's details ("" to leave them).
    Select(String),
}

#[derive(Debug)]
pub enum Event {
    State(Box<State>),
}

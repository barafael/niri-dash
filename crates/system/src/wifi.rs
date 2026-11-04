//! Wi-Fi, from NetworkManager.

pub use crate::sources::wifi::{
    Activity, Answer, Connectivity, Eap, Network, Prompt, PromptKind, Security, State, day, facts,
};

#[derive(Debug)]
pub enum Command {
    /// Turn the radio on or off.
    Toggle,
    Rescan,
    /// Connect to the network with this key (saved profile or visible network).
    Connect {
        key: String,
    },
    JoinHidden {
        ssid: String,
        security: i32,
        answer: Answer,
    },
    /// What was typed into the prompt with this serial.
    Answer {
        serial: u64,
        answer: Answer,
    },
    CancelPrompt,
    Disconnect,
    Forget {
        uuid: String,
    },
    SetAutoconnect {
        uuid: String,
        on: bool,
    },
    /// Show the details page of the network with this key ("" to leave it).
    Select {
        key: String,
    },
    /// Read a saved network's password, to show it and its QR code.
    Reveal {
        uuid: String,
    },
    HideSecret,
    /// Open the captive portal's login page.
    SignIn,
}

#[derive(Debug)]
pub enum Event {
    State(Box<State>),
}

//! Bluetooth, from BlueZ.

use crate::power::DeviceKind;

#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub path: String,
    pub address: String,
    /// What it is called here (its alias, cleaned).
    pub name: String,
    /// What it calls itself, when that differs from `name`; "" otherwise.
    pub claimed: String,
    /// It has a name, rather than just an address.
    pub named: bool,
    pub kind: DeviceKind,
    pub paired: bool,
    /// Allowed to connect without asking.
    pub trusted: bool,
    pub blocked: bool,
    pub connected: bool,
    /// Its link uses the old PIN pairing (no protection against eavesdroppers during pairing).
    pub legacy: bool,
    /// Bluetooth Low Energy with a random (private) address.
    pub random_address: bool,
    /// -1 if unknown.
    pub battery: i32,
    /// Signal strength in dBm while it is seen by a scan.
    pub rssi: Option<i16>,
    pub profiles: Vec<&'static str>,
    pub busy: bool,
}

impl Device {
    /// Signal as 0–4 bars (-1: not seen right now).
    pub fn bars(&self) -> i32 {
        match self.rssi {
            None => -1,
            Some(r) if r >= -55 => 4,
            Some(r) if r >= -67 => 3,
            Some(r) if r >= -78 => 2,
            Some(r) if r >= -88 => 1,
            Some(_) => 0,
        }
    }
}

/// What a pairing prompt asks.
#[derive(Debug, Clone, PartialEq)]
pub enum PromptKind {
    /// Numeric comparison: does the device show this passkey too?
    Confirm(u32),
    /// Type the PIN (old devices; often 0000).
    EnterPin,
    /// Type the passkey the device shows.
    EnterPasskey,
    /// Type this PIN on the device.
    ShowPin(String),
    /// Type this passkey on the device (`entered` digits so far).
    ShowPasskey(u32, u16),
    /// Allow pairing (no code to compare).
    Authorize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Prompt {
    /// A new prompt has a new serial: an answer to an earlier one goes nowhere.
    pub serial: u64,
    pub name: String,
    pub address: String,
    pub kind: PromptKind,
}

/// What the prompt was answered with: yes/no, or the code typed.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Yes,
    Code(String),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum Activity {
    #[default]
    Idle,
    Pairing(String),
    Failed(String),
}

impl std::fmt::Display for Activity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Activity::Idle => Ok(()),
            Activity::Pairing(name) => write!(f, "Pairing with {name}…"),
            Activity::Failed(why) => write!(f, "{why}"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub available: bool,
    pub powered: bool,
    /// Off through rfkill (airplane mode or a hardware switch).
    pub blocked: bool,
    pub adapter: String,
    pub adapter_name: String,
    pub discovering: bool,
    /// Paired devices, and connected ones (paired or not).
    pub devices: Vec<Device>,
    /// Devices a scan found that aren't paired: named ones, strongest first.
    pub nearby: Vec<Device>,
    /// How many devices without a name the scan sees.
    pub unnamed: usize,
    pub activity: Activity,
    pub prompt: Option<Prompt>,
    /// The device whose details are shown.
    pub detail: Option<Device>,
}

impl State {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn device(&self, address: &str) -> Option<&Device> {
        self.devices
            .iter()
            .chain(&self.nearby)
            .find(|d| d.address == address)
    }
}

/// The details page's lines (label, value) for a device.
pub fn facts(d: &Device) -> Vec<(String, String)> {
    let mut f: Vec<(String, String)> = Vec::new();
    let mut add = |label: &str, value: String| {
        if !value.is_empty() {
            f.push((label.into(), value));
        }
    };
    add(
        "Status",
        match (d.connected, d.paired) {
            (true, true) => "Connected",
            (true, false) => "Connected, not paired",
            (false, true) => "Paired",
            (false, false) => "Not paired",
        }
        .into(),
    );
    if d.battery >= 0 {
        add("Battery", format!("{} %", d.battery));
    }
    if let Some(rssi) = d.rssi {
        add("Signal", format!("{rssi} dBm"));
    }
    add("Calls itself", d.claimed.clone());
    add(
        "Address",
        if d.random_address {
            format!("{} (private)", d.address)
        } else {
            d.address.clone()
        },
    );
    for (i, p) in d.profiles.iter().enumerate() {
        add(if i == 0 { "Offers" } else { "" }, (*p).into());
    }
    if d.paired && d.legacy {
        add("Pairing", "Old PIN pairing (weaker)".into());
    }
    if d.blocked {
        add("Blocked", "Connections are refused".into());
    }
    f
}

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

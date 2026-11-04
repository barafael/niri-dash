//! Battery, power profiles, and blocking sleep.

/// A power-profiles-daemon profile; the kebab-case name is what powerprofilesctl uses.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::EnumString, strum::AsRefStr,
)]
#[strum(serialize_all = "kebab-case")]
pub enum Profile {
    PowerSaver,
    Balanced,
    Performance,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub has_battery: bool,
    pub percent: i32,
    pub charging: bool,
    pub status_text: String,
    pub power_text: String,
    pub profile: Option<Profile>,
    pub profiles: Vec<Profile>,
    /// Batteries of mice, keyboards, headphones and the like.
    pub peripherals: Vec<Peripheral>,
}

/// What kind of device a peripheral battery belongs to (UPower's device type, simplified).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Mouse,
    Keyboard,
    Headphones,
    Gamepad,
    Phone,
    Other,
}

impl DeviceKind {
    /// From UPower's `Type` property.
    pub fn from_upower(t: u32) -> DeviceKind {
        match t {
            5 | 14 => DeviceKind::Mouse,
            6 => DeviceKind::Keyboard,
            17..=19 | 21 => DeviceKind::Headphones,
            12 => DeviceKind::Gamepad,
            8 => DeviceKind::Phone,
            _ => DeviceKind::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Peripheral {
    pub kind: DeviceKind,
    pub name: String,
    pub percent: i32,
}

/// Something that blocks sleep or idling right now, from logind's list. Apps that only block
/// the screensaver through niri (or with Wayland's idle inhibitor) don't show up here: niri
/// keeps those to itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Blocker {
    pub who: String,
    pub why: String,
}

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

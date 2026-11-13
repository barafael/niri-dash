//! Runtime state the daemon keeps across restarts: do not disturb, the timer, the picked
//! colors, night light and the widget picker's filter. It lives in
//! `~/.local/state/niri-dash/state.json`; config.kdl stays the user's hand-written file and isn't
//! touched for these.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// Do not disturb: off when absent, until this time (Unix seconds), or `i64::MAX` for
    /// "until turned off".
    pub dnd_until: Option<i64>,
    /// A running timer's end (Unix milliseconds) and its full length in seconds.
    pub timer_end: Option<i64>,
    pub timer_secs: u32,
    /// Picked colors as "#rrggbb", newest first.
    pub colors: Vec<String>,
    pub night_light: NightLight,
    pub pomodoro: Pomodoro,
    /// The widget picker's categories that are filtered out, by name ("Media").
    pub palette_hidden: Vec<String>,
}

/// Where the pomodoro stood: its part (0 focus, 1 short break, 2 long break), how many focus
/// sessions of the round are done, and the end of the part if it was running.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pomodoro {
    pub phase: u8,
    pub done: u32,
    /// Unix milliseconds.
    pub end: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NightLight {
    pub on: bool,
    /// The temperature when on, in kelvin.
    pub kelvin: u32,
}

impl Default for NightLight {
    fn default() -> Self {
        Self {
            on: false,
            kelvin: 4500,
        }
    }
}

pub fn path() -> PathBuf {
    if let Some(p) = std::env::var_os("NIRI_DASH_STATE") {
        return PathBuf::from(p);
    }
    crate::files::state_home().join("niri-dash/state.json")
}

pub fn load() -> State {
    let path = path();
    match crate::files::read(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            log::warn!("{}: {e}; starting fresh", path.display());
            State::default()
        }),
        Err(_) => State::default(),
    }
}

pub fn save(state: &State) {
    let path = path();
    let json = serde_json::to_vec_pretty(state).expect("the state serializes");
    crate::files::write_later(path, json);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_missing_fields() {
        let s = State {
            dnd_until: Some(42),
            colors: vec!["#112233".into()],
            ..Default::default()
        };
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<State>(&text).unwrap(), s);
        // Files with fields missing, or ones it doesn't know, still load.
        let s: State = serde_json::from_str(r#"{"timer_secs":60,"other":1}"#).unwrap();
        assert_eq!(s.timer_secs, 60);
        assert_eq!(s.dnd_until, None);
        assert_eq!(s.night_light.kelvin, 4500);
    }
}

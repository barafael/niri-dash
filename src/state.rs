//! Runtime state the daemon keeps across restarts: night light, do not disturb, the timer and the
//! picked colors. It lives in `~/.local/state/niri-dash/state.json`; config.kdl stays the
//! user's hand-written file and isn't touched for these.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NightLight {
    pub on: bool,
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub night_light: NightLight,
    /// Do not disturb: off when absent, until this time (Unix seconds), or `i64::MAX` for
    /// "until turned off".
    pub dnd_until: Option<i64>,
    /// A running timer's end (Unix milliseconds) and its full length in seconds.
    pub timer_end: Option<i64>,
    pub timer_secs: u32,
    /// Picked colors as "#rrggbb", newest first.
    pub colors: Vec<String>,
}

pub fn path() -> PathBuf {
    if let Some(p) = std::env::var_os("NIRI_DASH_STATE") {
        return PathBuf::from(p);
    }
    crate::files::state_home().join("niri-dash/state.json")
}

pub fn load() -> State {
    let path = path();
    match std::fs::read_to_string(&path) {
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
    if let Err(e) = crate::files::write_atomic(&path, json) {
        log::warn!("saving {}: {e}", path.display());
    }
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
        // Older or hand-edited files with fields missing still load.
        let s: State = serde_json::from_str(r#"{"night_light":{"on":true}}"#).unwrap();
        assert!(s.night_light.on);
        assert_eq!(s.night_light.kelvin, 4500);
        assert_eq!(s.dnd_until, None);
    }
}

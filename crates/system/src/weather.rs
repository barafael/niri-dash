//! The weather, from Open-Meteo.

pub use crate::sources::weather::{Day, Icon, State};

/// Where the weather widget looks. Without a location, the city of the system time zone is used.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WeatherConfig {
    pub location: Option<String>,
    pub coords: Option<(f64, f64)>,
    pub fahrenheit: bool,
}

#[derive(Debug)]
pub enum Command {
    /// Where to look; fetches again.
    Configure(WeatherConfig),
}

#[derive(Debug)]
pub enum Event {
    State(State),
}

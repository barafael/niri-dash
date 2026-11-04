//! The weather, from Open-Meteo.

/// Which weather icon to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Icon {
    Sun,
    Moon,
    CloudSun,
    CloudMoon,
    #[default]
    Cloud,
    Fog,
    Rain,
    Snow,
    Storm,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Day {
    pub name: String,
    pub icon: Icon,
    pub max: String,
    pub min: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub place: String,
    pub temp: String,
    pub feels: String,
    pub description: String,
    pub icon: Icon,
    pub details: String,
    pub days: Vec<Day>,
    pub error: String,
}

/// WMO weather code to (icon, description).
pub fn describe(code: i32, day: bool) -> (Icon, &'static str) {
    match code {
        0 => (if day { Icon::Sun } else { Icon::Moon }, "Clear"),
        1 => (if day { Icon::Sun } else { Icon::Moon }, "Mostly clear"),
        2 => (
            if day { Icon::CloudSun } else { Icon::CloudMoon },
            "Partly cloudy",
        ),
        3 => (Icon::Cloud, "Overcast"),
        45 | 48 => (Icon::Fog, "Fog"),
        51 | 53 | 55 => (Icon::Rain, "Drizzle"),
        56 | 57 => (Icon::Rain, "Freezing drizzle"),
        61 | 63 => (Icon::Rain, "Rain"),
        65 => (Icon::Rain, "Heavy rain"),
        66 | 67 => (Icon::Rain, "Freezing rain"),
        71 | 73 | 77 => (Icon::Snow, "Snow"),
        75 => (Icon::Snow, "Heavy snow"),
        80 | 81 => (Icon::Rain, "Showers"),
        82 => (Icon::Rain, "Heavy showers"),
        85 | 86 => (Icon::Snow, "Snow showers"),
        95 => (Icon::Storm, "Thunderstorm"),
        96 | 99 => (Icon::Storm, "Thunderstorm, hail"),
        _ => (Icon::Cloud, "Unknown"),
    }
}

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

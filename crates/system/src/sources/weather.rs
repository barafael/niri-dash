//! Weather from Open-Meteo (no API key). The location comes from config.kdl, or defaults to the
//! city of the system time zone. Fetched at most every 15 minutes, and only while visible.

use super::Ctx;
use crate::weather::WeatherConfig;
pub use crate::weather::{Day, State, describe};
use chrono::NaiveDate;
use serde::Deserialize;
use std::time::{Duration, Instant};
use tokio::sync::watch;

/// "Europe/Berlin" -> "Berlin", from /etc/localtime or $TZ.
pub fn timezone_city() -> Option<String> {
    let tz = std::env::var("TZ")
        .ok()
        .filter(|t| t.contains('/'))
        .or_else(|| {
            std::fs::read_link("/etc/localtime")
                .ok()
                .map(|p| p.to_string_lossy().into_owned())
        })?;
    let city = tz.rsplit('/').next()?.replace('_', " ");
    (!city.is_empty() && city != "UTC").then_some(city)
}

#[derive(Deserialize)]
struct GeoResults {
    results: Option<Vec<Geo>>,
}

#[derive(Deserialize)]
struct Geo {
    name: String,
    latitude: f64,
    longitude: f64,
}

#[derive(Deserialize)]
struct Forecast {
    current: Current,
    daily: Daily,
}

#[derive(Deserialize)]
struct Current {
    temperature_2m: f64,
    apparent_temperature: f64,
    relative_humidity_2m: f64,
    weather_code: i32,
    wind_speed_10m: f64,
    is_day: i32,
}

#[derive(Deserialize)]
struct Daily {
    time: Vec<String>,
    weather_code: Vec<i32>,
    temperature_2m_max: Vec<f64>,
    temperature_2m_min: Vec<f64>,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .user_agent(concat!("niri-dash/", env!("CARGO_PKG_VERSION")))
        .build()
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("no location; set `weather location=\"…\"` in config.kdl")]
    NoLocation,
    #[error("looking up the place failed: {0}")]
    Lookup(Http),
    #[error("unknown place \"{0}\"")]
    UnknownPlace(String),
    #[error("forecast unavailable: {0}")]
    Forecast(Http),
    #[error("reading the reply: {0}")]
    Decode(#[from] std::io::Error),
}

/// A failed request, said briefly: the widget shows it, and ureq's own message carries the whole
/// request URL. The full error goes to the log.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct Http(String);

impl From<ureq::Error> for Http {
    fn from(e: ureq::Error) -> Self {
        log::debug!("weather: {e}");
        Http(match &e {
            ureq::Error::Status(429, _) => "too many requests, try again later".into(),
            ureq::Error::Status(code @ 500.., _) => format!("server error ({code})"),
            ureq::Error::Status(code, _) => format!("request refused ({code})"),
            ureq::Error::Transport(t) => match t.kind() {
                ureq::ErrorKind::Dns => "no network (the server's name doesn't resolve)".into(),
                ureq::ErrorKind::ConnectionFailed => "can't reach the server".into(),
                ureq::ErrorKind::Io => "network error or timeout".into(),
                other => other.to_string(),
            },
        })
    }
}

fn fetch(cfg: &WeatherConfig, locale: chrono::Locale) -> Result<State, Error> {
    let agent = agent();
    let (place, lat, lon) = match (cfg.coords, &cfg.location) {
        (Some((lat, lon)), name) => (
            name.clone()
                .unwrap_or_else(|| format!("{lat:.2}, {lon:.2}")),
            lat,
            lon,
        ),
        (None, name) => {
            let name = name
                .clone()
                .or_else(timezone_city)
                .ok_or(Error::NoLocation)?;
            let lang = locale.to_string();
            let lang = lang.split('_').next().unwrap_or("en");
            let geo: GeoResults = agent
                .get("https://geocoding-api.open-meteo.com/v1/search")
                .query("name", &name)
                .query("count", "1")
                .query("language", lang)
                .call()
                .map_err(|e| Error::Lookup(e.into()))?
                .into_json()?;
            let g = geo
                .results
                .and_then(|r| r.into_iter().next())
                .ok_or_else(|| Error::UnknownPlace(name.clone()))?;
            (g.name, g.latitude, g.longitude)
        }
    };
    let unit = if cfg.fahrenheit {
        "fahrenheit"
    } else {
        "celsius"
    };
    let f: Forecast = agent
        .get("https://api.open-meteo.com/v1/forecast")
        .query("latitude", &lat.to_string())
        .query("longitude", &lon.to_string())
        .query("current", "temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,wind_speed_10m,is_day")
        .query("daily", "weather_code,temperature_2m_max,temperature_2m_min")
        .query("timezone", "auto")
        .query("forecast_days", "6")
        .query("temperature_unit", unit)
        .call()
        .map_err(|e| Error::Forecast(e.into()))?
        .into_json()?;
    let c = &f.current;
    let (icon, description) = describe(c.weather_code, c.is_day != 0);
    let deg = |t: f64| format!("{:.0}°", t);
    let days = f
        .daily
        .time
        .iter()
        .enumerate()
        .skip(1)
        .map(|(i, date)| {
            let name = NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .map(|d| d.format_localized("%a", locale).to_string())
                .unwrap_or_default();
            Day {
                name,
                icon: describe(f.daily.weather_code.get(i).copied().unwrap_or(-1), true).0,
                max: f
                    .daily
                    .temperature_2m_max
                    .get(i)
                    .map(|t| deg(*t))
                    .unwrap_or_default(),
                min: f
                    .daily
                    .temperature_2m_min
                    .get(i)
                    .map(|t| deg(*t))
                    .unwrap_or_default(),
            }
        })
        .collect();
    let today = (
        f.daily.temperature_2m_max.first(),
        f.daily.temperature_2m_min.first(),
    );
    Ok(State {
        place,
        temp: deg(c.temperature_2m),
        feels: format!("feels like {}", deg(c.apparent_temperature)),
        description: description.to_string(),
        icon,
        details: format!(
            "{}{}% humidity · wind {:.0} km/h",
            match today {
                (Some(hi), Some(lo)) => format!("{} / {} · ", deg(*hi), deg(*lo)),
                _ => String::new(),
            },
            c.relative_humidity_2m.round(),
            c.wind_speed_10m
        ),
        days,
        error: String::new(),
    })
}

/// How long weather that was fetched is good for.
const FRESH: Duration = Duration::from_secs(15 * 60);

/// Whether it's time to fetch: weather stays good for a while, unless it's for another place.
/// A fetch that failed is tried again on the next poll.
#[derive(Default)]
struct Gate {
    cfg: WeatherConfig,
    fetched: Option<Instant>,
}

impl Gate {
    fn due(&mut self, cfg: &WeatherConfig, now: Instant) -> bool {
        if self.cfg != *cfg {
            self.cfg = cfg.clone();
            self.fetched = None;
        }
        self.fetched
            .is_none_or(|t| now.saturating_duration_since(t) >= FRESH)
    }
}

pub struct Weather {
    cfg: watch::Sender<WeatherConfig>,
    ctx: Ctx,
}

impl Weather {
    pub fn start(ctx: Ctx, locale: chrono::Locale) -> Self {
        let cfg = watch::Sender::new(WeatherConfig::default());
        let current = cfg.subscribe();
        let mut ticks = ctx.ticks(Default::default());
        let c = ctx.clone();
        // One at a time, here rather than from the actor: a refresh comes with every action
        // anywhere, and the weather service doesn't like being asked in parallel.
        ctx.spawn(async move {
            let mut gate = Gate::default();
            ticks.ready().await;
            loop {
                let asked = current.borrow().clone();
                if gate.due(&asked, Instant::now()) {
                    let cfg = asked.clone();
                    let result = tokio::task::spawn_blocking(move || fetch(&cfg, locale)).await;
                    // The location may have changed while fetching: then this result is stale, and
                    // the fetch for the new one is already queued.
                    if *current.borrow() == asked {
                        let state = match result {
                            Ok(Ok(s)) => {
                                gate.fetched = Some(Instant::now());
                                s
                            }
                            Ok(Err(e)) => State {
                                error: e.to_string(),
                                ..Default::default()
                            },
                            Err(e) => State {
                                error: e.to_string(),
                                ..Default::default()
                            },
                        };
                        c.send(crate::weather::Event::State(state));
                    }
                }
                ticks.next(Duration::from_secs(30)).await;
            }
        });
        Self { cfg, ctx }
    }

    /// Use a new location; refetches on the next poll if it changed.
    pub fn configure(&self, cfg: WeatherConfig) {
        let changed = self.cfg.send_if_modified(|current| {
            let changed = *current != cfg;
            if changed {
                *current = cfg;
            }
            changed
        });
        if changed {
            self.ctx.refresh();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weather::Icon;

    fn place(name: &str) -> WeatherConfig {
        WeatherConfig {
            location: Some(name.into()),
            ..Default::default()
        }
    }

    #[test]
    fn fetched_weather_is_good_for_a_while() {
        let now = Instant::now();
        let mut gate = Gate::default();
        let here = place("Hamburg");
        assert!(gate.due(&here, now), "never fetched");
        gate.fetched = Some(now);
        assert!(!gate.due(&here, now + FRESH - Duration::from_secs(1)));
        assert!(gate.due(&here, now + FRESH));
    }

    #[test]
    fn another_place_is_due_right_away() {
        let now = Instant::now();
        let mut gate = Gate::default();
        assert!(gate.due(&place("Hamburg"), now));
        gate.fetched = Some(now);
        assert!(gate.due(&place("Lisbon"), now));
    }

    /// A failed fetch doesn't set `fetched`, so it stays due.
    #[test]
    fn a_failure_stays_due() {
        let now = Instant::now();
        let mut gate = Gate::default();
        let here = place("Hamburg");
        assert!(gate.due(&here, now));
        assert!(gate.due(&here, now + Duration::from_secs(30)));
    }

    #[tokio::test]
    async fn configuring_the_same_place_refreshes_nothing() {
        let (ctx, _events) = crate::sources::testing::ctx();
        let weather = Weather::start(ctx.clone(), chrono::Locale::POSIX);
        let mut refreshes = ctx.wake.subscribe();
        weather.configure(WeatherConfig::default());
        assert!(!refreshes.has_changed().unwrap());
        weather.configure(place("Hamburg"));
        assert!(refreshes.has_changed().unwrap());
        refreshes.mark_unchanged();
        weather.configure(place("Hamburg"));
        assert!(!refreshes.has_changed().unwrap());
    }

    #[test]
    fn http_errors_are_short() {
        let status = |code| {
            let r = ureq::Response::new(code, "x", "").unwrap();
            Error::Forecast(ureq::Error::Status(code, r).into()).to_string()
        };
        assert_eq!(status(503), "forecast unavailable: server error (503)");
        assert_eq!(status(404), "forecast unavailable: request refused (404)");
        assert_eq!(
            status(429),
            "forecast unavailable: too many requests, try again later"
        );
    }

    #[test]
    fn codes() {
        assert_eq!(describe(0, true).0, Icon::Sun);
        assert_eq!(describe(0, false).0, Icon::Moon);
        assert_eq!(describe(95, true).0, Icon::Storm);
    }
}

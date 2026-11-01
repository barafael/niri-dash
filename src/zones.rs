//! Extra time zones for the clock widget: `zones "Asia/Tokyo, New York"`.

use chrono::{DateTime, Offset, TimeZone, Utc};
use chrono_tz::Tz;

/// One zone as the clock shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ZoneTime {
    pub label: String,
    pub time: String,
    /// The difference to local time, e.g. "+8 h", "−5:30 h", with "tomorrow"/"yesterday" when the
    /// date differs.
    pub delta: String,
}

/// Look up a zone by its IANA name ("America/New_York") or just the city ("new york", "Tokyo").
pub fn resolve(name: &str) -> Option<Tz> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    if let Ok(tz) = name.parse::<Tz>() {
        return Some(tz);
    }
    let want = name.replace(' ', "_").to_lowercase();
    chrono_tz::TZ_VARIANTS.iter().copied().find(|tz| {
        let n = tz.name().to_lowercase();
        n == want || n.rsplit('/').next() == Some(want.as_str())
    })
}

/// The city part of a zone name: "America/New_York" → "New York".
pub fn label(tz: Tz) -> String {
    tz.name()
        .rsplit('/')
        .next()
        .unwrap_or(tz.name())
        .replace('_', " ")
}

/// The zones in a comma-separated list, unknown names dropped.
pub fn parse_list(list: &str) -> (Vec<Tz>, Vec<String>) {
    let mut zones = Vec::new();
    let mut unknown = Vec::new();
    for part in list
        .split([',', ';'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        match resolve(part) {
            Some(tz) => zones.push(tz),
            None => unknown.push(part.to_string()),
        }
    }
    (zones, unknown)
}

/// The rows for `zones` at `now`, compared with the local zone `local`.
pub fn rows<L: TimeZone>(
    zones: &[Tz],
    now: DateTime<Utc>,
    local: &L,
    hours12: bool,
) -> Vec<ZoneTime> {
    let here = now.with_timezone(local);
    let here_off = here.offset().fix().local_minus_utc();
    zones
        .iter()
        .map(|tz| {
            let there = now.with_timezone(tz);
            let diff = there.offset().fix().local_minus_utc() - here_off;
            let mut delta = if diff == 0 {
                "same time".to_string()
            } else {
                let sign = if diff > 0 { "+" } else { "\u{2212}" };
                let (h, m) = (diff.abs() / 3600, diff.abs() % 3600 / 60);
                if m == 0 {
                    format!("{sign}{h} h")
                } else {
                    format!("{sign}{h}:{m:02} h")
                }
            };
            let day = there
                .date_naive()
                .signed_duration_since(here.date_naive())
                .num_days();
            if day > 0 {
                delta.push_str(" · tomorrow");
            } else if day < 0 {
                delta.push_str(" · yesterday");
            }
            ZoneTime {
                label: label(*tz),
                time: if hours12 {
                    there.format("%-I:%M %p").to_string()
                } else {
                    there.format("%H:%M").to_string()
                },
                delta,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn resolves_names_and_cities() {
        assert_eq!(resolve("Asia/Tokyo"), Some(chrono_tz::Asia::Tokyo));
        assert_eq!(resolve("new york"), Some(chrono_tz::America::New_York));
        assert_eq!(resolve("Tokyo"), Some(chrono_tz::Asia::Tokyo));
        assert_eq!(resolve("Atlantis"), None);
        let (z, bad) = parse_list("Tokyo, , Atlantis; UTC");
        assert_eq!(z.len(), 2);
        assert_eq!(bad, vec!["Atlantis"]);
        assert_eq!(label(chrono_tz::America::New_York), "New York");
    }

    #[test]
    fn deltas() {
        // 23:30 in Berlin (UTC+1 in winter) is 07:30 the next day in Tokyo.
        let berlin = chrono_tz::Europe::Berlin;
        let now = berlin
            .with_ymd_and_hms(2026, 11, 1, 23, 30, 0)
            .unwrap()
            .with_timezone(&Utc);
        let r = rows(
            &[chrono_tz::Asia::Tokyo, chrono_tz::Asia::Kolkata, berlin],
            now,
            &berlin,
            false,
        );
        assert_eq!(r[0].time, "07:30");
        assert_eq!(r[0].delta, "+8 h · tomorrow");
        assert_eq!(r[1].delta, "+4:30 h · tomorrow");
        assert_eq!(r[2].delta, "same time");
        let ny = rows(&[chrono_tz::America::New_York], now, &berlin, true);
        assert_eq!(ny[0].time, "5:30 PM");
        assert_eq!(ny[0].delta, "\u{2212}6 h");
    }
}

//! Per-widget settings. They live as children of the widget's node in config.kdl:
//!
//! ```kdl
//! clock col=0 row=0 width=4 height=2 {
//!     hours 12
//!     seconds #false
//! }
//! ```
//!
//! Only values that differ from the default are written. Each kind of widget has a fixed list of
//! settings ([`for_kind`]); the edit sidebar shows them in that order.

use kdl::KdlValue;
use strum::{AsRefStr, EnumString};

use crate::layout::Kind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumString, AsRefStr)]
#[strum(serialize_all = "kebab-case")]
pub enum Opt {
    // Clock.
    Hours,
    Seconds,
    Date,
    Zones,
    // Timer.
    Mode,
    // Notes.
    Note,
    // Calendar.
    FirstDay,
    WeekNumbers,
    // Monitors.
    History,
    Cores,
    TopApps,
    HideSystem,
    // Weather.
    Location,
    Units,
    // Session.
    Lock,
    Suspend,
    Logout,
    Restart,
    PowerOff,
    Confirm,
    // Mascot.
    Character,
    Wildness,
    Interval,
    Command,
}

/// What kind of value a setting takes, and how it is edited.
pub enum Ty {
    Toggle,
    /// Stepped through a fixed list of numbers.
    Steps(&'static [i64]),
    /// Stepped through a fixed list of words.
    Choice(&'static [&'static str]),
    /// Free text; empty means the default.
    Text {
        placeholder: &'static str,
    },
}

const HISTORY: &[i64] = &[30, 60, 90, 120, 180];

impl Opt {
    pub fn label(self) -> &'static str {
        match self {
            Opt::Hours => "Time format",
            Opt::Seconds => "Seconds",
            Opt::Date => "Date",
            Opt::Zones => "Other time zones",
            Opt::Mode => "Shows",
            Opt::Note => "Note",
            Opt::FirstDay => "Week starts on",
            Opt::WeekNumbers => "Week numbers",
            Opt::History => "Chart covers",
            Opt::Cores => "Per-core bars",
            Opt::TopApps => "Top apps",
            Opt::HideSystem => "Hide /boot and /efi",
            Opt::Location => "Location",
            Opt::Units => "Units",
            Opt::Lock => "Lock",
            Opt::Suspend => "Suspend",
            Opt::Logout => "Log out",
            Opt::Restart => "Restart",
            Opt::PowerOff => "Power off",
            Opt::Confirm => "Confirm restart and power off",
            Opt::Character => "Mascot",
            Opt::Wildness => "Wildness",
            Opt::Interval => "New remark every",
            Opt::Command => "Lines from command",
        }
    }

    pub fn ty(self) -> Ty {
        match self {
            Opt::Hours => Ty::Steps(&[12, 24]),
            Opt::FirstDay => Ty::Choice(&["monday", "sunday"]),
            Opt::Mode => Ty::Choice(&["timer", "stopwatch"]),
            Opt::History => Ty::Steps(HISTORY),
            Opt::TopApps => Ty::Steps(&[0, 1, 2, 3, 4, 5, 6, 8, 10]),
            Opt::Units => Ty::Choice(&["celsius", "fahrenheit"]),
            Opt::Note => Ty::Text {
                placeholder: "notes (same name, same note)",
            },
            Opt::Zones => Ty::Text {
                placeholder: "e.g. Asia/Tokyo, New York",
            },
            Opt::Location => Ty::Text {
                placeholder: "City of your time zone",
            },
            Opt::Character => Ty::Choice(&["schraubi", "zolli", "verschnitti"]),
            Opt::Wildness => Ty::Steps(&[0, 1, 2, 3, 4]),
            Opt::Interval => Ty::Steps(&[15, 30, 45, 60, 120, 300, 600]),
            Opt::Command => Ty::Text {
                placeholder: "Built-in remarks",
            },
            _ => Ty::Toggle,
        }
    }

    /// The default value. `us` is whether the locale is American (12-hour clock etc).
    pub fn default(self, us: bool) -> KdlValue {
        match self {
            Opt::Hours => KdlValue::Integer(if us { 12 } else { 24 }),
            Opt::FirstDay => KdlValue::String(if us { "sunday" } else { "monday" }.into()),
            Opt::History => KdlValue::Integer(90),
            Opt::TopApps => KdlValue::Integer(5),
            Opt::Units => KdlValue::String(if us { "fahrenheit" } else { "celsius" }.into()),
            Opt::Location | Opt::Command | Opt::Zones | Opt::Note => {
                KdlValue::String(String::new())
            }
            Opt::Character => KdlValue::String("schraubi".into()),
            Opt::Mode => KdlValue::String("timer".into()),
            Opt::Wildness => KdlValue::Integer(2),
            Opt::Interval => KdlValue::Integer(45),
            Opt::WeekNumbers | Opt::HideSystem => KdlValue::Bool(false),
            _ => KdlValue::Bool(true),
        }
    }

    /// How a value is shown in the sidebar.
    pub fn value_text(self, v: &KdlValue) -> String {
        match (self, v) {
            (Opt::Hours, KdlValue::Integer(h)) => format!("{h}-hour"),
            (Opt::History | Opt::Interval, KdlValue::Integer(s)) if *s % 60 == 0 => {
                format!("{} min", s / 60)
            }
            (Opt::History | Opt::Interval, KdlValue::Integer(s)) if *s > 60 => {
                format!("{:.1} min", *s as f32 / 60.0)
            }
            (Opt::History | Opt::Interval, KdlValue::Integer(s)) => format!("{s} s"),
            (Opt::Wildness, KdlValue::Integer(w)) => {
                ["calm", "dry", "cheeky", "wild", "unhinged"][(*w).clamp(0, 4) as usize].into()
            }
            (Opt::TopApps, KdlValue::Integer(0)) => "none".into(),
            (_, KdlValue::Integer(i)) => i.to_string(),
            (_, KdlValue::String(s)) => {
                let mut c = s.chars();
                c.next()
                    .map(|f| f.to_uppercase().chain(c).collect())
                    .unwrap_or_default()
            }
            (_, KdlValue::Bool(b)) => if *b { "on" } else { "off" }.into(),
            _ => String::new(),
        }
    }

    /// Whether `v` is an acceptable value for this setting.
    pub fn accepts(self, v: &KdlValue) -> bool {
        match (self.ty(), v) {
            (Ty::Toggle, KdlValue::Bool(_)) => true,
            (Ty::Steps(s), KdlValue::Integer(i)) => s.iter().any(|x| *x as i128 == *i),
            (Ty::Choice(c), KdlValue::String(s)) => c.contains(&s.as_str()),
            (Ty::Text { .. }, KdlValue::String(_)) => true,
            _ => false,
        }
    }

    /// The value one step from `cur` (toggles flip; lists clamp at their ends).
    pub fn step(self, cur: &KdlValue, delta: i32) -> KdlValue {
        match (self.ty(), cur) {
            (Ty::Toggle, KdlValue::Bool(b)) => KdlValue::Bool(!b),
            (Ty::Steps(s), KdlValue::Integer(i)) => {
                // The nearest listed value, then move along the list.
                let at = s
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, v)| (**v as i128 - i).abs())
                    .map_or(0, |(k, _)| k) as i32;
                KdlValue::Integer(s[(at + delta).clamp(0, s.len() as i32 - 1) as usize] as i128)
            }
            (Ty::Choice(c), KdlValue::String(v)) => {
                let at = c.iter().position(|x| x == v).unwrap_or(0) as i32;
                KdlValue::String(c[(at + delta).rem_euclid(c.len() as i32) as usize].into())
            }
            _ => cur.clone(),
        }
    }

    /// Whether `step(cur, delta)` would change anything.
    pub fn can_step(self, cur: &KdlValue, delta: i32) -> bool {
        match self.ty() {
            Ty::Steps(_) => self.step(cur, delta) != *cur,
            Ty::Choice(c) => c.len() > 1,
            _ => false,
        }
    }
}

/// The settings each kind of widget has, in display order.
pub fn for_kind(kind: Kind) -> &'static [Opt] {
    use Opt::*;
    match kind {
        Kind::Clock => &[Hours, Seconds, Date, Zones],
        Kind::Timer => &[Mode],
        Kind::Notes => &[Note],
        Kind::Calendar => &[FirstDay, WeekNumbers],
        Kind::Perf => &[History, Cores],
        Kind::Memory => &[History, TopApps],
        Kind::Network | Kind::Gpu => &[History],
        Kind::Disks => &[HideSystem],
        Kind::Weather => &[Location, Units],
        Kind::Session => &[Lock, Suspend, Logout, Restart, PowerOff, Confirm],
        Kind::Mascot => &[Character, Wildness, Interval, Command],
        _ => &[],
    }
}

/// A widget's settings as written in the file: `(name, value)` pairs, unknown ones included so
/// they survive saving.
pub type Values = Vec<(String, KdlValue)>;

/// The effective value of `opt`: the stored one if valid, else the default.
pub fn get(values: &[(String, KdlValue)], opt: Opt, us: bool) -> KdlValue {
    values
        .iter()
        .find(|(k, v)| k == opt.as_ref() && opt.accepts(v))
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| opt.default(us))
}

pub fn get_bool(values: &[(String, KdlValue)], opt: Opt, us: bool) -> bool {
    matches!(get(values, opt, us), KdlValue::Bool(true))
}

pub fn get_int(values: &[(String, KdlValue)], opt: Opt, us: bool) -> i64 {
    match get(values, opt, us) {
        KdlValue::Integer(i) => i as i64,
        _ => 0,
    }
}

pub fn get_str(values: &[(String, KdlValue)], opt: Opt, us: bool) -> String {
    match get(values, opt, us) {
        KdlValue::String(s) => s,
        _ => String::new(),
    }
}

/// Store `value` for `opt`, dropping the entry when it equals the default.
pub fn set(values: &mut Values, opt: Opt, value: KdlValue, us: bool) {
    let is_default = value == opt.default(us);
    match values.iter_mut().find(|(k, _)| k == opt.as_ref()) {
        Some(_) if is_default => values.retain(|(k, _)| k != opt.as_ref()),
        Some(e) => e.1 = value,
        None if is_default => {}
        None => values.push((opt.as_ref().to_string(), value)),
    }
}

/// Warnings for settings that don't exist for `kind` or have a bad value.
pub fn check(kind: Kind, values: &[(String, KdlValue)]) -> Vec<String> {
    let name = kind.name();
    values
        .iter()
        .filter_map(|(k, v)| match k.parse::<Opt>() {
            Ok(opt) if for_kind(kind).contains(&opt) => {
                (!opt.accepts(v)).then(|| format!("{name} {{ {k} }}: unexpected value {v}"))
            }
            _ => Some(format!("{name} has no setting `{k}`")),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_clamp_and_choices_wrap() {
        let h = KdlValue::Integer(90);
        assert_eq!(Opt::History.step(&h, 1), KdlValue::Integer(120));
        assert_eq!(
            Opt::History.step(&KdlValue::Integer(180), 1),
            KdlValue::Integer(180)
        );
        assert!(!Opt::History.can_step(&KdlValue::Integer(30), -1));
        // An unlisted value snaps to the nearest one first.
        assert_eq!(
            Opt::History.step(&KdlValue::Integer(100), 1),
            KdlValue::Integer(120)
        );
        let d = KdlValue::String("monday".into());
        assert_eq!(Opt::FirstDay.step(&d, 1), KdlValue::String("sunday".into()));
        assert_eq!(
            Opt::FirstDay.step(&d, -1),
            KdlValue::String("sunday".into())
        );
    }

    #[test]
    fn defaults_are_not_stored() {
        let mut v = Values::new();
        set(&mut v, Opt::Seconds, KdlValue::Bool(false), false);
        assert_eq!(v.len(), 1);
        assert!(!get_bool(&v, Opt::Seconds, false));
        set(&mut v, Opt::Seconds, KdlValue::Bool(true), false);
        assert!(v.is_empty());
    }

    #[test]
    fn bad_values_fall_back_and_warn() {
        let v: Values = vec![
            ("hours".into(), KdlValue::Integer(7)),
            ("colour".into(), KdlValue::Bool(true)),
        ];
        assert_eq!(get_int(&v, Opt::Hours, false), 24);
        assert_eq!(check(Kind::Clock, &v).len(), 2);
        let v: Values = vec![("hours".into(), KdlValue::String("x".into()))];
        assert_eq!(check(Kind::Clock, &v).len(), 1);
    }
}

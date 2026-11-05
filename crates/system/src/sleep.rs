//! The sleep timer: count down to the moment the machine goes to sleep (or off), with the same
//! grammar and keys as `sleepy`.
//!
//! Everything that decides something is plain data and arithmetic here, so it is tested without a
//! machine to sleep: what the user typed ([`parse_when`]), the countdown ([`Timer`]) and what a
//! command does to both ([`Controller`]). The dashboard owns a controller, ticks it, and hands
//! the few things that touch the machine to the actor behind [`Command`]: asking which modes the
//! machine offers, running one, and keeping the screen awake meanwhile.

use chrono::{DateTime, Duration as Span, NaiveTime, TimeZone, Timelike};
use std::path::{Path, PathBuf};
use std::time::Duration;
use strum::{Display, EnumIter, EnumString, IntoEnumIterator};

/// What the countdown does when it ends, from quickest to come back from to most final, ending
/// with the one that is not the machine's business.
///
/// The words are systemd's, which is what the config and the logind questions use; `Poweroff`
/// also answers to `shutdown`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, EnumIter, EnumString, Display)]
#[strum(serialize_all = "kebab-case")]
pub enum Mode {
    /// To RAM: fast both ways, but the machine must keep its power.
    Suspend,
    /// To disk, then off: slow to come back, survives any power loss.
    Hibernate,
    /// Both at once: quick wake-up while the battery holds, nothing lost if it does not.
    HybridSleep,
    /// Suspend now, hibernate if the nap runs long.
    SuspendThenHibernate,
    /// Off. Waking up means booting.
    #[strum(to_string = "poweroff", serialize = "shutdown")]
    Poweroff,
    /// Run the user's script instead of sleeping.
    RunScript,
}

impl Mode {
    /// The name to show a person.
    pub fn label(self) -> &'static str {
        match self {
            Mode::Suspend => "suspend",
            Mode::Hibernate => "hibernate",
            Mode::HybridSleep => "hybrid sleep",
            Mode::SuspendThenHibernate => "suspend, then hibernate",
            Mode::Poweroff => "power off",
            Mode::RunScript => "run script",
        }
    }

    /// One line on what it does, for choosing between them.
    pub fn help(self) -> &'static str {
        match self {
            Mode::Suspend => "sleeps now, wakes fast; the machine must hold power",
            Mode::Hibernate => "memory to disk, power off; slow to wake, survives anything",
            Mode::HybridSleep => "RAM and disk both; fast wake, nothing lost if the battery dies",
            Mode::SuspendThenHibernate => "suspends now, hibernates if the nap runs long",
            Mode::Poweroff => "powers off; waking up means booting again",
            Mode::RunScript => "runs your script instead of sleeping",
        }
    }

    /// What a finished countdown says it is doing.
    pub fn firing(self) -> String {
        match self {
            Mode::RunScript => "run the script".into(),
            m => format!("{} the machine", m.label()),
        }
    }

    /// The `systemctl` verb, for the modes that are systemd's.
    pub fn verb(self) -> Option<String> {
        (self != Mode::RunScript).then(|| self.to_string())
    }

    /// The logind method that says whether this is possible now.
    pub fn method(self) -> Option<&'static str> {
        match self {
            Mode::Suspend => Some("CanSuspend"),
            Mode::Hibernate => Some("CanHibernate"),
            Mode::HybridSleep => Some("CanHybridSleep"),
            Mode::SuspendThenHibernate => Some("CanSuspendThenHibernate"),
            Mode::Poweroff => Some("CanPowerOff"),
            Mode::RunScript => None,
        }
    }

    /// Every mode, in display order.
    pub fn all() -> impl Iterator<Item = Mode> {
        Mode::iter()
    }
}

/// What running `mode` comes to, in words: the command, or the script.
pub fn describe(mode: Mode, script: &Path) -> String {
    match mode.verb() {
        Some(verb) => format!("systemctl {verb}"),
        None => script.display().to_string(),
    }
}

/// Whether logind's answer to a `Can…` question means yes. A `challenge` counts: polkit asks for
/// a password when the time comes, not when the countdown starts.
pub fn possible(answer: &str) -> Option<bool> {
    match answer.to_ascii_lowercase().as_str() {
        "yes" | "challenge" | "platform" => Some(true),
        "no" | "na" | "disabled" | "off" => Some(false),
        _ => None,
    }
}

/// What the dashboard asks of the system.
#[derive(Debug)]
pub enum Command {
    /// Which modes does this machine offer? `script` is the file that makes
    /// [`Mode::RunScript`] one of them.
    Detect { script: PathBuf },
    /// Do it, now. Answered with [`Event::Done`].
    Run { mode: Mode, script: PathBuf },
    /// Hold the screen awake (or let it go).
    KeepAwake(bool),
}

#[derive(Debug)]
pub enum Event {
    Modes(Vec<Mode>),
    /// A mode ran; `error` is why the machine is still awake, if it is.
    Done {
        mode: Mode,
        error: Option<String>,
    },
}

/// When the machine should go: after a while, or when the wall clock says so.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    After(Duration),
    /// The next time the clock reads this.
    At(NaiveTime),
}

impl When {
    /// How long from `now`. A clock time means the next one to come: `23:45` typed at ten is
    /// tonight, typed at a minute to midnight it is tomorrow's.
    pub fn duration_from<Tz: TimeZone>(self, now: &DateTime<Tz>) -> Duration {
        match self {
            When::After(d) => d,
            When::At(time) => {
                let zone = now.timezone();
                // Tomorrow always has the time; the day after is for a night that skipped it.
                (0..=2)
                    .filter_map(|days| {
                        let day = now.date_naive().checked_add_days(chrono::Days::new(days))?;
                        let local = day.and_time(time);
                        // A time that happens twice (the clocks go back) is slept at its first
                        // passing, or the second if the first has gone by.
                        let candidates = match zone.from_local_datetime(&local) {
                            chrono::LocalResult::Single(t) => vec![t],
                            chrono::LocalResult::Ambiguous(a, b) => vec![a, b],
                            chrono::LocalResult::None => vec![],
                        };
                        candidates.into_iter().find(|t| t > now)
                    })
                    .next()
                    .and_then(|at| (at - now.clone()).to_std().ok())
                    // Whole seconds, rounded up: sleeping at 23:44:59.5 is before the time.
                    .map_or(Duration::ZERO, |d| {
                        Duration::from_secs(d.as_secs() + u64::from(d.subsec_nanos() > 0))
                    })
            }
        }
    }

    /// What the field echoes beside itself: the promise being made.
    pub fn echo(&self) -> String {
        match self {
            When::After(d) => format!("→ in {}", spell(*d)),
            When::At(t) => format!("→ at {:02}:{:02}", t.hour(), t.minute()),
        }
    }
}

/// Why the field is not a when yet; the message is shown under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("type a time, e.g. 30 min or 23:45")]
    Empty,
    #[error("not a duration; try 45s, 30 min, 1h30m")]
    Invalid,
    #[error("that would sleep immediately; type more than 0")]
    Zero,
    #[error("not a clock time; try 23:45 or 6:30")]
    Clock,
}

/// A duration (`45s`, `30 min`, `1h30m`) or a clock time (`23:45`).
pub fn parse_when(input: &str) -> Result<When, ParseError> {
    let text = input.trim();
    if text.contains(':') {
        return parse_clock(text).map(When::At).ok_or(ParseError::Clock);
    }
    parse_duration(text).map(When::After)
}

/// A run of `<number><unit>` pieces, with or without spaces between: `1h30m`, `1 h 30 min`.
pub fn parse_duration(input: &str) -> Result<Duration, ParseError> {
    let text = input.trim();
    if text.is_empty() {
        return Err(ParseError::Empty);
    }
    let mut total = Duration::ZERO;
    let mut rest = text;
    while !rest.is_empty() {
        let digits = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        if digits == 0 {
            return Err(ParseError::Invalid);
        }
        let number: u64 = rest[..digits].parse().map_err(|_| ParseError::Invalid)?;
        rest = rest[digits..].trim_start();
        let letters = rest
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(rest.len());
        let seconds = match rest[..letters].to_ascii_lowercase().as_str() {
            "s" | "sec" | "secs" | "second" | "seconds" => 1,
            "m" | "min" | "mins" | "minute" | "minutes" => 60,
            "h" | "hr" | "hrs" | "hour" | "hours" => 3600,
            "d" | "day" | "days" => 86_400,
            "w" | "week" | "weeks" => 604_800,
            _ => return Err(ParseError::Invalid),
        };
        rest = rest[letters..].trim_start();
        let piece = number
            .checked_mul(seconds)
            .ok_or(ParseError::Invalid)
            .map(Duration::from_secs)?;
        total = total.checked_add(piece).ok_or(ParseError::Invalid)?;
    }
    if total.is_zero() {
        Err(ParseError::Zero)
    } else {
        Ok(total)
    }
}

/// `HH:MM` or `HH:MM:SS`, one or two digits a part.
fn parse_clock(text: &str) -> Option<NaiveTime> {
    let parts: Vec<&str> = text.split(':').map(str::trim).collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let number = |part: &str| -> Option<u32> {
        (!part.is_empty() && part.len() <= 2 && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse().ok())
            .flatten()
    };
    let second = match parts.get(2) {
        Some(p) => number(p)?,
        None => 0,
    };
    NaiveTime::from_hms_opt(number(parts[0])?, number(parts[1])?, second)
}

/// A duration in words, whole seconds: `1h 30m`, `45s`, `2d 1h`.
pub fn spell(d: Duration) -> String {
    let secs = d.as_secs();
    let parts = [
        (secs / 86_400, "d"),
        (secs % 86_400 / 3600, "h"),
        (secs % 3600 / 60, "m"),
        (secs % 60, "s"),
    ];
    let words: Vec<String> = parts
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, unit)| format!("{n}{unit}"))
        .collect();
    if words.is_empty() {
        "0s".into()
    } else {
        words.join(" ")
    }
}

/// A countdown as the clock shows it: `1:30:05`, `4:09`.
pub fn clock_text(d: Duration) -> String {
    let secs = d.as_secs();
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// What the timer is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Idle,
    Armed,
    Paused,
    /// The countdown ran out and the mode was started.
    Fired,
}

/// A countdown. No clock of its own: it is told how long it has been since the last tick.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Timer {
    mode: Option<Mode>,
    total: Duration,
    remaining: Duration,
    status: Status,
}

impl Timer {
    /// Start (or restart) counting down to `mode`. A zero duration is refused.
    pub fn arm(&mut self, mode: Mode, duration: Duration) -> bool {
        if duration.is_zero() {
            return false;
        }
        *self = Timer {
            mode: Some(mode),
            total: duration,
            remaining: duration,
            status: Status::Armed,
        };
        true
    }

    pub fn pause(&mut self) {
        if self.status == Status::Armed {
            self.status = Status::Paused;
        }
    }

    pub fn resume(&mut self) {
        if self.status == Status::Paused {
            self.status = Status::Armed;
        }
    }

    pub fn abort(&mut self) {
        *self = Timer::default();
    }

    /// Move the deadline by `seconds` (negative: nearer), never outside `0..=total`: a `+` does
    /// not stretch the countdown past what it was armed with, and a `-` to nothing means now.
    pub fn adjust(&mut self, seconds: i64) {
        if !matches!(self.status, Status::Armed | Status::Paused) {
            return;
        }
        let left = self.remaining.as_secs_f64() + seconds as f64;
        self.remaining = Duration::from_secs_f64(left.clamp(0.0, self.total.as_secs_f64()));
    }

    /// Count `elapsed`. True once, when it runs out.
    pub fn tick(&mut self, elapsed: Duration) -> bool {
        if self.status != Status::Armed {
            return false;
        }
        self.remaining = self.remaining.saturating_sub(elapsed);
        if self.remaining.is_zero() {
            self.status = Status::Fired;
            return true;
        }
        false
    }

    pub fn status(&self) -> Status {
        self.status
    }

    pub fn mode(&self) -> Option<Mode> {
        self.mode
    }

    pub fn remaining(&self) -> Duration {
        self.remaining
    }

    pub fn total(&self) -> Duration {
        self.total
    }

    /// The part still to go: 1 when armed, 0 at the end.
    pub fn fraction(&self) -> f32 {
        if self.total.is_zero() {
            return 0.0;
        }
        (self.remaining.as_secs_f64() / self.total.as_secs_f64()) as f32
    }

    pub fn counting(&self) -> bool {
        matches!(self.status, Status::Armed | Status::Paused)
    }
}

/// What the user can ask for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    PreviousMode,
    NextMode,
    Select(usize),
    ToggleDryRun,
    ToggleKeepAwake,
    /// Start the countdown with the picked mode and the typed when.
    Arm,
    PauseResume,
    /// Give up on the countdown, or clear what a finished one left.
    Abort,
    /// A minute more.
    Longer,
    /// A minute less.
    Shorter,
}

/// What `+` and `-` are worth.
pub const NUDGE: i64 = 60;

/// What a finished countdown has to say for itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Report {
    /// A dry run: what would have been done.
    Would(String),
    /// It was done, and the machine is awake again (or the script has finished).
    Ran,
    /// It was refused, or failed.
    Failed(String),
}

/// The widget's whole state apart from how it is drawn.
#[derive(Clone, Debug)]
pub struct Controller {
    /// The modes this machine offers, in display order.
    pub modes: Vec<Mode>,
    pub selected: usize,
    /// The when field.
    pub input: String,
    pub timer: Timer,
    /// Count down for real, then only say what would have run.
    pub dry_run: bool,
    /// Hold the screen awake while counting.
    pub keep_awake: bool,
    /// The mode of a finished countdown, and what became of it.
    pub fired: Option<Mode>,
    pub report: Option<Report>,
}

/// What the field holds before anyone types.
pub const DEFAULT_WHEN: &str = "30 min";

impl Default for Controller {
    fn default() -> Self {
        Controller {
            modes: Vec::new(),
            selected: 0,
            input: DEFAULT_WHEN.into(),
            timer: Timer::default(),
            dry_run: false,
            keep_awake: false,
            fired: None,
            report: None,
        }
    }
}

impl Controller {
    /// The modes the machine offers; the one picked stays picked if it is still among them.
    pub fn set_modes(&mut self, modes: Vec<Mode>) {
        let keep = self.selected_mode();
        self.modes = modes;
        self.selected = keep
            .and_then(|m| self.modes.iter().position(|x| *x == m))
            .unwrap_or(0);
    }

    pub fn selected_mode(&self) -> Option<Mode> {
        self.modes.get(self.selected).copied()
    }

    /// What the field means right now, or why it is not a when.
    pub fn when(&self) -> Result<When, ParseError> {
        parse_when(&self.input)
    }

    /// Whether arming would do anything.
    pub fn can_arm(&self) -> bool {
        !self.timer.counting() && self.selected_mode().is_some() && self.when().is_ok()
    }

    /// Whether the screen should be held awake right now.
    pub fn wants_awake(&self) -> bool {
        self.keep_awake && self.timer.counting()
    }

    /// Run a command. Arming waits for a countdown that is not running: one is never silently
    /// replaced.
    pub fn apply<Tz: TimeZone>(&mut self, act: Act, now: &DateTime<Tz>) {
        match act {
            Act::PreviousMode => self.move_selection(-1),
            Act::NextMode => self.move_selection(1),
            Act::Select(i) if i < self.modes.len() => self.selected = i,
            Act::Select(_) => {}
            Act::ToggleDryRun => self.dry_run = !self.dry_run,
            Act::ToggleKeepAwake => self.keep_awake = !self.keep_awake,
            Act::Arm => {
                if self.can_arm()
                    && let (Some(mode), Ok(when)) = (self.selected_mode(), self.when())
                {
                    self.fired = None;
                    self.report = None;
                    self.timer.arm(mode, when.duration_from(now));
                }
            }
            Act::PauseResume => match self.timer.status() {
                Status::Armed => self.timer.pause(),
                Status::Paused => self.timer.resume(),
                Status::Idle | Status::Fired => {}
            },
            Act::Abort => {
                self.timer.abort();
                self.fired = None;
                self.report = None;
            }
            Act::Longer => self.timer.adjust(NUDGE),
            Act::Shorter => self.timer.adjust(-NUDGE),
        }
    }

    fn move_selection(&mut self, step: isize) {
        let n = self.modes.len() as isize;
        if n > 0 {
            self.selected = (self.selected as isize + step).rem_euclid(n) as usize;
        }
    }

    /// Count `elapsed`; the mode to start, once, when the countdown runs out.
    pub fn tick(&mut self, elapsed: Duration) -> Option<Mode> {
        if !self.timer.tick(elapsed) {
            return None;
        }
        self.fired = self.timer.mode();
        self.fired
    }

    /// A dry run reached the end: say what would have happened.
    pub fn pretend(&mut self, mode: Mode, script: &Path) {
        self.report = Some(Report::Would(describe(mode, script)));
    }

    /// The system ran `mode`.
    pub fn performed(&mut self, mode: Mode, error: Option<String>) {
        if self.fired == Some(mode) {
            self.report = Some(error.map_or(Report::Ran, Report::Failed));
        }
    }

    /// The time left, in words.
    pub fn left(&self) -> String {
        spell(self.timer.remaining())
    }

    /// When the running countdown ends on the wall clock (`23:45`); a paused one has no deadline
    /// to promise.
    pub fn ends_at<Tz: TimeZone>(&self, now: &DateTime<Tz>) -> Option<String> {
        (self.timer.status() == Status::Armed).then(|| {
            let end = now.clone() + Span::seconds(self.timer.remaining().as_secs() as i64);
            format!("{:02}:{:02}", end.hour(), end.minute())
        })
    }

    /// The headline: what is happening, how long it takes and when exactly it ends.
    pub fn headline<Tz: TimeZone>(&self, now: &DateTime<Tz>) -> String {
        match self.timer.status() {
            Status::Idle => self.selected_mode().map_or_else(
                || "this machine offers nothing to arm".to_string(),
                |m| format!("press enter to arm the {} countdown", m.label()),
            ),
            Status::Paused => format!("paused, {} left", self.left()),
            Status::Armed => {
                let what = self.timer.mode().map_or("sleep", Mode::label);
                match self.ends_at(now) {
                    Some(at) => format!("{what} in {}, at {at}", self.left()),
                    None => format!("{what} in {}", self.left()),
                }
            }
            Status::Fired => format!(
                "time! {}…",
                self.fired
                    .map_or_else(|| "sleep the machine".into(), Mode::firing)
            ),
        }
    }

    /// The line under a finished countdown, if there is one.
    pub fn outcome(&self) -> Option<String> {
        match self.report.as_ref()? {
            Report::Would(what) => Some(format!("dry run: would have run {what}")),
            Report::Ran => None,
            Report::Failed(why) => Some(why.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, Utc};

    fn at(h: u32, m: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 4, h, m, s).unwrap()
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn durations_in_the_ways_people_write_them() {
        for (text, want) in [
            ("45s", 45),
            ("30 min", 1800),
            ("30min", 1800),
            ("1h30m", 5400),
            ("1 h 30 m", 5400),
            ("2 hours", 7200),
            ("1d", 86_400),
            ("1 Week", 604_800),
            ("  90 seconds ", 90),
        ] {
            assert_eq!(parse_duration(text), Ok(secs(want)), "{text}");
        }
    }

    #[test]
    fn what_is_no_duration_says_why() {
        assert_eq!(parse_when(""), Err(ParseError::Empty));
        assert_eq!(parse_when("   "), Err(ParseError::Empty));
        assert_eq!(parse_when("0s"), Err(ParseError::Zero));
        assert_eq!(parse_when("0 min 0 s"), Err(ParseError::Zero));
        for bad in ["soon", "45", "min", "5x", "1h 30", "-5m", "1.5h", "5 min 3"] {
            assert_eq!(parse_when(bad), Err(ParseError::Invalid), "{bad}");
        }
        // Too big to count.
        assert_eq!(
            parse_when("99999999999999999999 s"),
            Err(ParseError::Invalid)
        );
        assert_eq!(
            parse_when("18446744073709551615 w"),
            Err(ParseError::Invalid)
        );
    }

    #[test]
    fn clock_times_parse_in_their_own_grammar() {
        let t = |h, m, s| NaiveTime::from_hms_opt(h, m, s).unwrap();
        assert_eq!(parse_when("23:45"), Ok(When::At(t(23, 45, 0))));
        assert_eq!(parse_when("9:05"), Ok(When::At(t(9, 5, 0))));
        assert_eq!(parse_when(" 6:5:30 "), Ok(When::At(t(6, 5, 30))));
        for bad in [
            "24:00", "12:60", "12:", ":30", "1:2:3:4", "ab:cd", "123:00", "12:345",
        ] {
            assert_eq!(parse_when(bad), Err(ParseError::Clock), "{bad}");
        }
    }

    #[test]
    fn the_next_clock_time_is_tonight_or_tomorrow() {
        let t = |h, m| When::At(NaiveTime::from_hms_opt(h, m, 0).unwrap());
        assert_eq!(t(23, 45).duration_from(&at(22, 0, 0)), secs(105 * 60));
        // It has gone by: tomorrow's.
        assert_eq!(t(21, 0).duration_from(&at(22, 0, 0)), secs(23 * 3600));
        // Right on it counts as gone by, not as now.
        assert_eq!(t(22, 0).duration_from(&at(22, 0, 0)), secs(24 * 3600));
        // Whole seconds, rounded up.
        let now = at(22, 0, 0) + Span::milliseconds(500);
        assert_eq!(t(22, 1).duration_from(&now), secs(60));
        // Durations ignore the clock.
        assert_eq!(When::After(secs(7)).duration_from(&at(1, 2, 3)), secs(7));
    }

    #[test]
    fn a_clock_time_in_another_zone_is_that_zones() {
        let zone = FixedOffset::east_opt(2 * 3600).unwrap();
        let now = zone.with_ymd_and_hms(2026, 11, 4, 23, 0, 0).unwrap();
        let t = When::At(NaiveTime::from_hms_opt(23, 30, 0).unwrap());
        assert_eq!(t.duration_from(&now), secs(30 * 60));
    }

    #[test]
    fn the_echo_names_the_promise() {
        assert_eq!(parse_when("90 min").unwrap().echo(), "→ in 1h 30m");
        assert_eq!(parse_when("7:05").unwrap().echo(), "→ at 07:05");
        assert_eq!(spell(secs(0)), "0s");
        assert_eq!(spell(secs(90_061)), "1d 1h 1m 1s");
        assert_eq!(clock_text(secs(5405)), "1:30:05");
        assert_eq!(clock_text(secs(249)), "4:09");
    }

    #[test]
    fn the_words_are_systemds() {
        let words: Vec<String> = Mode::all().map(|m| m.to_string()).collect();
        assert_eq!(
            words,
            [
                "suspend",
                "hibernate",
                "hybrid-sleep",
                "suspend-then-hibernate",
                "poweroff",
                "run-script"
            ]
        );
        for m in Mode::all() {
            assert_eq!(m.to_string().parse(), Ok(m));
            assert!(!m.help().is_empty() && !m.label().is_empty());
        }
        assert_eq!("shutdown".parse::<Mode>(), Ok(Mode::Poweroff));
        assert!("nap".parse::<Mode>().is_err());
        // Everything but the script has a verb and a question for logind.
        for m in Mode::all().filter(|m| *m != Mode::RunScript) {
            assert!(m.verb().is_some());
            assert!(m.method().is_some_and(|q| q.starts_with("Can")));
        }
        assert_eq!(Mode::RunScript.verb(), None);
        assert_eq!(Mode::RunScript.method(), None);
    }

    #[test]
    fn logind_answers_are_read_as_systemd_means_them() {
        for yes in ["yes", "challenge", "platform", "Yes"] {
            assert_eq!(possible(yes), Some(true), "{yes}");
        }
        for no in ["no", "na", "disabled", "off"] {
            assert_eq!(possible(no), Some(false), "{no}");
        }
        assert_eq!(possible("banana"), None);
    }

    #[test]
    fn describing_a_mode_says_what_would_run() {
        let script = Path::new("/home/me/sleep.sh");
        assert_eq!(describe(Mode::Hibernate, script), "systemctl hibernate");
        assert_eq!(describe(Mode::Poweroff, script), "systemctl poweroff");
        assert_eq!(describe(Mode::RunScript, script), "/home/me/sleep.sh");
    }

    #[test]
    fn a_timer_counts_pauses_and_fires_once() {
        let mut t = Timer::default();
        assert!(!t.arm(Mode::Suspend, Duration::ZERO));
        assert_eq!(t.status(), Status::Idle);
        assert!(t.arm(Mode::Suspend, secs(60)));
        assert!(!t.tick(secs(20)));
        assert_eq!(t.remaining(), secs(40));
        t.pause();
        // Time spent paused is not counted.
        assert!(!t.tick(secs(100)));
        assert_eq!(t.remaining(), secs(40));
        t.resume();
        assert!(!t.tick(secs(39)));
        assert!(t.tick(secs(5)));
        assert_eq!(t.status(), Status::Fired);
        // Once.
        assert!(!t.tick(secs(5)));
        t.abort();
        assert_eq!(t, Timer::default());
    }

    #[test]
    fn nudging_stays_between_nothing_and_what_it_was_armed_with() {
        let mut t = Timer::default();
        t.adjust(60);
        assert_eq!(t.remaining(), Duration::ZERO, "idle has no deadline");
        t.arm(Mode::Suspend, secs(300));
        t.tick(secs(100));
        t.adjust(60);
        assert_eq!(t.remaining(), secs(260));
        t.adjust(600);
        assert_eq!(t.remaining(), secs(300));
        t.adjust(-100_000);
        assert_eq!(t.remaining(), Duration::ZERO);
        // Shortened to nothing, the next tick is the end.
        assert!(t.tick(secs(1)));
    }

    fn controller() -> Controller {
        let mut c = Controller::default();
        c.set_modes(vec![Mode::Suspend, Mode::Hibernate, Mode::Poweroff]);
        c
    }

    #[test]
    fn arming_needs_a_mode_and_a_when() {
        let now = at(10, 0, 0);
        let mut c = Controller::default();
        c.apply(Act::Arm, &now);
        assert_eq!(c.timer.status(), Status::Idle, "no modes, nothing to arm");
        assert_eq!(c.headline(&now), "this machine offers nothing to arm");
        let mut c = controller();
        c.input = "soon".into();
        assert!(!c.can_arm());
        c.apply(Act::Arm, &now);
        assert_eq!(c.timer.status(), Status::Idle);
        c.input = "10 min".into();
        c.apply(Act::Arm, &now);
        assert_eq!(c.timer.status(), Status::Armed);
        assert_eq!(c.timer.mode(), Some(Mode::Suspend));
        assert_eq!(c.timer.remaining(), secs(600));
    }

    #[test]
    fn a_running_countdown_is_never_replaced() {
        let now = at(10, 0, 0);
        let mut c = controller();
        c.apply(Act::Arm, &now);
        c.apply(Act::NextMode, &now);
        c.input = "5 min".into();
        c.apply(Act::Arm, &now);
        assert_eq!(c.timer.mode(), Some(Mode::Suspend));
        assert_eq!(c.timer.total(), secs(1800));
        // Not while paused either.
        c.apply(Act::PauseResume, &now);
        c.apply(Act::Arm, &now);
        assert_eq!(c.timer.total(), secs(1800));
        c.apply(Act::Abort, &now);
        c.apply(Act::Arm, &now);
        assert_eq!(c.timer.mode(), Some(Mode::Hibernate));
        assert_eq!(c.timer.total(), secs(300));
    }

    #[test]
    fn the_mode_highlight_wraps() {
        let now = at(10, 0, 0);
        let mut c = controller();
        c.apply(Act::PreviousMode, &now);
        assert_eq!(c.selected_mode(), Some(Mode::Poweroff));
        c.apply(Act::NextMode, &now);
        assert_eq!(c.selected_mode(), Some(Mode::Suspend));
        c.apply(Act::Select(1), &now);
        assert_eq!(c.selected_mode(), Some(Mode::Hibernate));
        c.apply(Act::Select(9), &now);
        assert_eq!(c.selected, 1);
        // A new list keeps the pick if it can.
        c.set_modes(vec![Mode::Poweroff, Mode::Hibernate]);
        assert_eq!(c.selected_mode(), Some(Mode::Hibernate));
        c.set_modes(vec![Mode::Suspend]);
        assert_eq!(c.selected_mode(), Some(Mode::Suspend));
    }

    #[test]
    fn the_headline_answers_when_exactly() {
        let now = at(22, 10, 0);
        let mut c = controller();
        c.input = "90 min".into();
        assert_eq!(c.headline(&now), "press enter to arm the suspend countdown");
        c.apply(Act::Arm, &now);
        assert_eq!(c.headline(&now), "suspend in 1h 30m, at 23:40");
        c.apply(Act::PauseResume, &now);
        assert_eq!(c.headline(&now), "paused, 1h 30m left");
        assert_eq!(c.ends_at(&now), None);
        c.apply(Act::PauseResume, &now);
        c.tick(secs(30 * 60));
        assert_eq!(c.headline(&now), "suspend in 1h, at 23:10");
    }

    #[test]
    fn a_clock_time_arms_the_countdown_that_reaches_it() {
        let now = at(22, 0, 0);
        let mut c = controller();
        c.input = "23:45".into();
        c.apply(Act::Arm, &now);
        assert_eq!(c.timer.remaining(), secs(105 * 60));
        assert_eq!(c.headline(&now), "suspend in 1h 45m, at 23:45");
    }

    #[test]
    fn firing_happens_once_and_a_dry_run_says_what_it_would_have_done() {
        let now = at(10, 0, 0);
        let script = Path::new("/etc/sleep.sh");
        let mut c = controller();
        c.input = "10s".into();
        c.dry_run = true;
        c.apply(Act::Arm, &now);
        assert_eq!(c.tick(secs(4)), None);
        assert_eq!(c.tick(secs(7)), Some(Mode::Suspend));
        assert_eq!(c.tick(secs(7)), None);
        assert_eq!(c.headline(&now), "time! suspend the machine…");
        c.pretend(Mode::Suspend, script);
        assert_eq!(
            c.outcome().as_deref(),
            Some("dry run: would have run systemctl suspend")
        );
        // Another go clears the report.
        c.apply(Act::Arm, &now);
        assert_eq!(c.outcome(), None);
        assert_eq!(c.fired, None);
    }

    #[test]
    fn a_refusal_is_reported_and_a_success_is_not() {
        let now = at(10, 0, 0);
        let mut c = controller();
        c.input = "1s".into();
        c.apply(Act::Arm, &now);
        c.tick(secs(2));
        c.performed(Mode::Suspend, Some("systemctl refused to suspend".into()));
        assert_eq!(c.outcome().as_deref(), Some("systemctl refused to suspend"));
        c.performed(Mode::Suspend, None);
        assert_eq!(c.report, Some(Report::Ran));
        assert_eq!(c.outcome(), None);
        // An answer for something that is not the last countdown is not its report.
        c.apply(Act::Abort, &now);
        c.performed(Mode::Suspend, Some("late".into()));
        assert_eq!(c.report, None);
    }

    #[test]
    fn abort_saves_the_machine_and_clears_the_report() {
        let now = at(10, 0, 0);
        let mut c = controller();
        c.apply(Act::Arm, &now);
        c.tick(secs(1));
        c.apply(Act::Abort, &now);
        assert_eq!(c.timer.status(), Status::Idle);
        c.input = "1s".into();
        c.apply(Act::Arm, &now);
        c.tick(secs(5));
        assert_eq!(c.timer.status(), Status::Fired);
        c.apply(Act::Abort, &now);
        assert_eq!(c.timer.status(), Status::Idle);
        assert_eq!(c.fired, None);
    }

    #[test]
    fn the_screen_is_held_only_while_counting_and_asked_to() {
        let now = at(10, 0, 0);
        let mut c = controller();
        assert!(!c.wants_awake());
        c.apply(Act::ToggleKeepAwake, &now);
        assert!(!c.wants_awake(), "nothing is counting yet");
        c.apply(Act::Arm, &now);
        assert!(c.wants_awake());
        c.apply(Act::PauseResume, &now);
        assert!(c.wants_awake(), "a pause still has a countdown to watch");
        c.apply(Act::Abort, &now);
        assert!(!c.wants_awake());
        c.apply(Act::ToggleDryRun, &now);
        assert!(c.dry_run);
    }

    #[test]
    fn nudges_move_the_deadline_a_minute() {
        let now = at(10, 0, 0);
        let mut c = controller();
        c.input = "10 min".into();
        c.apply(Act::Arm, &now);
        c.apply(Act::Shorter, &now);
        assert_eq!(c.timer.remaining(), secs(540));
        c.apply(Act::Longer, &now);
        c.apply(Act::Longer, &now);
        assert_eq!(
            c.timer.remaining(),
            secs(600),
            "no more than it was armed with"
        );
    }
}

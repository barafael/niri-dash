//! The Timer widget's state, apart from the UI so it can be tested: a countdown or a stopwatch,
//! as its `mode` setting picks.

use std::time::Duration;
use web_time::SystemTime;

/// A countdown. Its end is wall-clock time so it survives a restart of the daemon.
#[derive(Debug, Clone, PartialEq)]
pub struct Countdown {
    /// The chosen length.
    pub length: Duration,
    pub state: Run,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Run {
    Idle,
    Running { end: SystemTime },
    Paused { left: Duration },
}

impl Default for Countdown {
    fn default() -> Self {
        Self {
            length: Duration::from_secs(5 * 60),
            state: Run::Idle,
        }
    }
}

impl Countdown {
    pub fn left(&self, now: SystemTime) -> Duration {
        match self.state {
            Run::Idle => self.length,
            Run::Running { end } => end.duration_since(now).unwrap_or_default(),
            Run::Paused { left } => left,
        }
    }

    /// The part still to go, 1 at the start and 0 at the end.
    pub fn fraction(&self, now: SystemTime) -> f32 {
        if self.length.is_zero() {
            return 0.0;
        }
        (self.left(now).as_secs_f32() / self.length.as_secs_f32()).clamp(0.0, 1.0)
    }

    /// Start, pause or resume.
    pub fn toggle(&mut self, now: SystemTime) {
        self.state = match self.state {
            Run::Idle => Run::Running {
                end: now + self.length,
            },
            Run::Running { end } => Run::Paused {
                left: end.duration_since(now).unwrap_or_default(),
            },
            Run::Paused { left } => Run::Running { end: now + left },
        };
    }

    pub fn reset(&mut self) {
        self.state = Run::Idle;
    }

    /// Change the length by `minutes` (at least one minute); a running timer gets the time
    /// added or taken away too.
    pub fn adjust(&mut self, minutes: i64, now: SystemTime) {
        let step = Duration::from_secs(minutes.unsigned_abs() * 60);
        let add = minutes > 0;
        let shift = |d: Duration| {
            if add {
                d + step
            } else {
                d.saturating_sub(step).max(Duration::from_secs(1))
            }
        };
        self.length = if add {
            self.length + step
        } else {
            self.length
                .saturating_sub(step)
                .max(Duration::from_secs(60))
        };
        self.state = match self.state {
            Run::Idle => Run::Idle,
            Run::Running { end } => Run::Running {
                end: now + shift(end.duration_since(now).unwrap_or_default()),
            },
            Run::Paused { left } => Run::Paused { left: shift(left) },
        };
    }

    /// Whether a running timer has reached its end.
    pub fn finished(&self, now: SystemTime) -> bool {
        matches!(self.state, Run::Running { end } if now >= end)
    }
}

/// A stopwatch with laps.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stopwatch {
    /// Time from earlier runs (before the last pause).
    pub banked: Duration,
    pub started: Option<SystemTime>,
    /// Lap times (since the previous lap), newest first.
    pub laps: Vec<Duration>,
    /// The total at the last lap.
    pub last_lap_at: Duration,
}

impl Stopwatch {
    pub fn elapsed(&self, now: SystemTime) -> Duration {
        self.banked
            + self
                .started
                .and_then(|s| now.duration_since(s).ok())
                .unwrap_or_default()
    }

    pub fn running(&self) -> bool {
        self.started.is_some()
    }

    pub fn toggle(&mut self, now: SystemTime) {
        match self.started.take() {
            Some(start) => self.banked += now.duration_since(start).unwrap_or_default(),
            None => self.started = Some(now),
        }
    }

    pub fn lap(&mut self, now: SystemTime) {
        let total = self.elapsed(now);
        self.laps.insert(0, total.saturating_sub(self.last_lap_at));
        self.last_lap_at = total;
    }

    pub fn reset(&mut self) {
        *self = Stopwatch::default();
    }
}

/// "4:05", "1:02:03"; with `tenths`, "4:05.3".
pub fn format(d: Duration, tenths: bool) -> String {
    let total = d.as_secs();
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    let mut out = if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    };
    if tenths {
        out.push_str(&format!(".{}", d.subsec_millis() / 100));
    }
    out
}

/// A countdown shows whole seconds rounded up, so it reads 0:00 only at the end.
pub fn format_left(d: Duration) -> String {
    let secs = d.as_secs() + u64::from(d.subsec_nanos() > 0);
    format(Duration::from_secs(secs), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn countdown() {
        let mut c = Countdown::default();
        assert_eq!(c.left(t(0)), Duration::from_secs(300));
        c.toggle(t(0));
        assert_eq!(c.left(t(60)), Duration::from_secs(240));
        assert!((c.fraction(t(150)) - 0.5).abs() < 1e-6);
        c.toggle(t(60));
        assert_eq!(
            c.state,
            Run::Paused {
                left: Duration::from_secs(240)
            }
        );
        assert_eq!(c.left(t(1000)), Duration::from_secs(240));
        c.toggle(t(1000));
        assert!(!c.finished(t(1239)));
        assert!(c.finished(t(1240)));
        c.adjust(1, t(1000));
        assert_eq!(c.length, Duration::from_secs(360));
        assert_eq!(c.left(t(1000)), Duration::from_secs(300));
        c.adjust(-10, t(1000));
        assert_eq!(c.length, Duration::from_secs(60));
        assert_eq!(c.left(t(1000)), Duration::from_secs(1));
        c.reset();
        assert_eq!(c.left(t(5)), Duration::from_secs(60));
    }

    #[test]
    fn stopwatch() {
        let mut s = Stopwatch::default();
        s.toggle(t(10));
        assert_eq!(s.elapsed(t(15)), Duration::from_secs(5));
        s.lap(t(15));
        s.toggle(t(20));
        assert_eq!(s.elapsed(t(100)), Duration::from_secs(10));
        s.toggle(t(100));
        s.lap(t(103));
        assert_eq!(s.laps, vec![Duration::from_secs(8), Duration::from_secs(5)]);
    }

    #[test]
    fn formats() {
        assert_eq!(format(Duration::from_secs(245), false), "4:05");
        assert_eq!(format(Duration::from_secs(3723), false), "1:02:03");
        assert_eq!(format(Duration::from_millis(245_340), true), "4:05.3");
        assert_eq!(format_left(Duration::from_millis(1)), "0:01");
        assert_eq!(format_left(Duration::ZERO), "0:00");
    }
}

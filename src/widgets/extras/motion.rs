//! What the mascots do over time, as plain functions of how long ago something happened: blinking
//! and glancing, ducking when a new line comes, the pupils gliding to what they follow. The
//! views ask these when they are drawn, and ask for another frame only while one of them is
//! mid-way: a mascot at rest costs nothing.
//!
//! All times are in seconds, and `motion` is the look's animation speed (0: none, nothing moves).

/// How long an eye stays shut.
pub const BLINK_FOR: f32 = 0.13;

/// Easing that overshoots a little and settles (`ease-out-back`).
pub fn ease_out_back(x: f32) -> f32 {
    const C1: f32 = 1.70158;
    const C3: f32 = C1 + 1.0;
    let x = x.clamp(0.0, 1.0) - 1.0;
    1.0 + C3 * x * x * x + C1 * x * x
}

/// Where a blinker is `elapsed` seconds after it began, if it blinks every `period` seconds:
/// whether its eyes are shut, how many times it has blinked (the glancing's beat), and how long
/// until it next changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blink {
    pub shut: bool,
    pub beat: u32,
    pub next_change: f32,
}

pub fn blink(elapsed: f32, period: f32) -> Blink {
    let period = period.max(0.5);
    let beat = (elapsed.max(0.0) / period).floor();
    let into = elapsed - beat * period;
    let shut = beat >= 1.0 && into < BLINK_FOR;
    Blink {
        shut,
        beat: beat as u32,
        next_change: if shut {
            BLINK_FOR - into
        } else {
            (beat + 1.0) * period - elapsed
        },
    }
}

/// Between blinks it glances at its bubble every third beat.
pub fn glancing(beat: u32) -> bool {
    beat % 3 == 1
}

/// How a new line shows: what was there ducks, and the new one comes up. All durations in
/// seconds, already scaled by the motion.
#[derive(Debug, Clone, Copy)]
pub struct Pop {
    /// Going down.
    pub down: f32,
    /// When it starts coming up again.
    pub hold: f32,
    /// Coming up.
    pub up: f32,
    /// The bubble fading out, and in.
    pub fade_out: f32,
    pub fade_in: f32,
}

impl Pop {
    pub fn figure(motion: f32) -> Self {
        Self {
            down: 0.14 * motion,
            hold: 0.22 * motion,
            up: 0.42 * motion,
            fade_out: 0.09 * motion,
            fade_in: 0.2 * motion,
        }
    }

    pub fn cow(motion: f32) -> Self {
        Self {
            down: 0.09 * motion,
            hold: 0.16 * motion,
            up: 0.3 * motion,
            fade_out: 0.09 * motion,
            fade_in: 0.2 * motion,
        }
    }

    /// How long all of it takes.
    pub fn total(&self) -> f32 {
        (self.hold + self.up).max(self.hold + self.fade_in)
    }

    /// `t` seconds after the line changed: how far up it is (0 down, 1 up; it overshoots on the
    /// way), and how visible the bubble is (0 to 1).
    pub fn at(&self, t: f32) -> (f32, f32) {
        if t >= self.total() {
            return (1.0, 1.0);
        }
        let rise = if t < self.down {
            1.0 - ease_out_back(t / self.down)
        } else if t < self.hold {
            0.0
        } else {
            ease_out_back((t - self.hold) / self.up)
        };
        let alpha = if t < self.fade_out {
            1.0 - t / self.fade_out
        } else if t < self.hold {
            0.0
        } else {
            ((t - self.hold) / self.fade_in).min(1.0)
        };
        (rise, alpha)
    }
}

/// Pupils gliding from where they were to a new target, in `duration` seconds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Glide {
    pub from: (f32, f32),
    pub to: (f32, f32),
    /// Seconds on the caller's clock when it set off.
    pub since: f32,
}

impl Glide {
    pub fn at(&self, now: f32, duration: f32) -> (f32, f32) {
        if duration <= 0.0 {
            return self.to;
        }
        let k = ((now - self.since) / duration).clamp(0.0, 1.0);
        (
            self.from.0 + (self.to.0 - self.from.0) * k,
            self.from.1 + (self.to.1 - self.from.1) * k,
        )
    }

    pub fn moving(&self, now: f32, duration: f32) -> bool {
        duration > 0.0 && now - self.since < duration
    }

    /// Head for `to` (from wherever it is now) if that is not where it is heading already.
    pub fn aim(&mut self, to: (f32, f32), now: f32, duration: f32) {
        let same = (self.to.0 - to.0).abs() < 0.005 && (self.to.1 - to.1).abs() < 0.005;
        if !same {
            *self = Glide {
                from: self.at(now, duration),
                to,
                since: now,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_blinks_once_a_period_and_not_at_first() {
        assert!(!blink(0.0, 4.7).shut);
        assert!(!blink(4.0, 4.7).shut);
        let b = blink(4.75, 4.7);
        assert!(b.shut && b.beat == 1);
        assert!(!blink(4.7 + 0.2, 4.7).shut);
        assert!(blink(9.45, 4.7).shut);
    }

    #[test]
    fn it_says_when_it_changes_next() {
        let b = blink(1.0, 4.0);
        assert!((b.next_change - 3.0).abs() < 1e-4);
        let b = blink(4.05, 4.0);
        assert!(b.shut && (b.next_change - 0.08).abs() < 1e-4);
    }

    #[test]
    fn it_glances_every_third_beat() {
        let glances: Vec<bool> = (0..7).map(glancing).collect();
        assert_eq!(glances, [false, true, false, false, true, false, false]);
    }

    #[test]
    fn a_new_line_ducks_and_comes_back() {
        let pop = Pop::figure(1.0);
        // It starts where it was, all the way up, and ducks from there.
        assert_eq!(pop.at(0.0), (1.0, 1.0));
        let (down, alpha) = pop.at(0.18);
        assert!(down.abs() < 1e-4 && alpha == 0.0, "{down} {alpha}");
        let (up, alpha) = pop.at(pop.total() + 0.01);
        assert_eq!((up, alpha), (1.0, 1.0));
        // On the way up it passes the top, and settles.
        assert!((0..60).any(|i| pop.at(0.22 + i as f32 * 0.007).0 > 1.0));
    }

    #[test]
    fn without_motion_nothing_ducks() {
        let pop = Pop::figure(0.0);
        assert_eq!(pop.total(), 0.0);
        assert_eq!(pop.at(0.0), (1.0, 1.0));
    }

    #[test]
    fn pupils_glide_from_where_they_are() {
        let mut g = Glide::default();
        g.aim((1.0, 0.0), 10.0, 0.15);
        assert_eq!(g.at(10.0, 0.15), (0.0, 0.0));
        assert!((g.at(10.075, 0.15).0 - 0.5).abs() < 1e-4);
        assert!(g.moving(10.1, 0.15) && !g.moving(10.2, 0.15));
        // Turned round half-way: it sets off from where it is.
        g.aim((0.0, 0.0), 10.075, 0.15);
        assert!((g.at(10.075, 0.15).0 - 0.5).abs() < 1e-4);
        // No motion: there at once.
        assert_eq!(g.at(10.0, 0.0), (0.0, 0.0));
    }
}

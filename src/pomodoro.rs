//! The pomodoro widget's state, apart from the UI so it can be tested: focus sessions and the
//! breaks between them, on the countdown's clock (wall time, so it survives a restart).

use crate::timer::Run;
use std::time::Duration;
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Focus,
    ShortBreak,
    LongBreak,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Focus => "Focus",
            Phase::ShortBreak => "Short break",
            Phase::LongBreak => "Long break",
        }
    }

    pub fn is_break(self) -> bool {
        self != Phase::Focus
    }

    /// For the state file.
    pub fn index(self) -> u8 {
        match self {
            Phase::Focus => 0,
            Phase::ShortBreak => 1,
            Phase::LongBreak => 2,
        }
    }

    pub fn from_index(i: u8) -> Self {
        match i {
            1 => Phase::ShortBreak,
            2 => Phase::LongBreak,
            _ => Phase::Focus,
        }
    }
}

/// How long each part is, and whether the next one starts by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub focus_min: u32,
    pub short_min: u32,
    pub long_min: u32,
    /// Focus sessions before the long break.
    pub rounds: u32,
    pub auto_start: bool,
}

impl Default for Plan {
    fn default() -> Self {
        Self {
            focus_min: 25,
            short_min: 5,
            long_min: 15,
            rounds: 4,
            auto_start: false,
        }
    }
}

impl Plan {
    pub fn length(&self, phase: Phase) -> Duration {
        let minutes = match phase {
            Phase::Focus => self.focus_min,
            Phase::ShortBreak => self.short_min,
            Phase::LongBreak => self.long_min,
        };
        Duration::from_secs(u64::from(minutes.max(1)) * 60)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pomodoro {
    pub plan: Plan,
    pub phase: Phase,
    pub run: Run,
    /// Focus sessions finished in this round of them. It is `rounds` during the long break,
    /// which is when all the dots are lit; the break's end starts the next round.
    pub done: u32,
}

impl Default for Pomodoro {
    fn default() -> Self {
        Self::new(Plan::default())
    }
}

impl Pomodoro {
    pub fn new(plan: Plan) -> Self {
        Self {
            plan,
            phase: Phase::Focus,
            run: Run::Idle,
            done: 0,
        }
    }

    pub fn length(&self) -> Duration {
        self.plan.length(self.phase)
    }

    pub fn left(&self, now: SystemTime) -> Duration {
        match self.run {
            Run::Idle => self.length(),
            Run::Running { end } => end.duration_since(now).unwrap_or_default(),
            Run::Paused { left } => left,
        }
    }

    /// The part still to go, 1 at the start and 0 at the end.
    pub fn fraction(&self, now: SystemTime) -> f32 {
        (self.left(now).as_secs_f32() / self.length().as_secs_f32()).clamp(0.0, 1.0)
    }

    pub fn running(&self) -> bool {
        matches!(self.run, Run::Running { .. })
    }

    /// Start, pause or resume.
    pub fn toggle(&mut self, now: SystemTime) {
        self.run = match self.run {
            Run::Idle => Run::Running {
                end: now + self.length(),
            },
            Run::Running { end } => Run::Paused {
                left: end.duration_since(now).unwrap_or_default(),
            },
            Run::Paused { left } => Run::Running { end: now + left },
        };
    }

    /// Back to the start of this part, stopped.
    pub fn reset(&mut self) {
        self.run = Run::Idle;
    }

    /// Whether the running part has reached its end.
    pub fn finished(&self, now: SystemTime) -> bool {
        matches!(self.run, Run::Running { end } if now >= end)
    }

    /// What follows the current part.
    pub fn next_phase(&self) -> Phase {
        match self.phase {
            Phase::Focus if self.done + 1 >= self.plan.rounds => Phase::LongBreak,
            Phase::Focus => Phase::ShortBreak,
            Phase::ShortBreak | Phase::LongBreak => Phase::Focus,
        }
    }

    /// The part ran out: move on, and say which one it was. The next one waits to be started,
    /// unless the plan says to start it.
    pub fn advance(&mut self, now: SystemTime) -> Phase {
        let was = self.phase;
        self.move_on(true);
        self.run = if self.plan.auto_start {
            Run::Running {
                end: now + self.length(),
            }
        } else {
            Run::Idle
        };
        was
    }

    /// Give up on this part and go to the next, stopped (or, if one was running under a plan
    /// that starts parts by itself, running). A skipped focus session isn't counted.
    pub fn skip(&mut self, now: SystemTime) {
        let was_running = self.running();
        self.move_on(false);
        self.run = if self.plan.auto_start && was_running {
            Run::Running {
                end: now + self.length(),
            }
        } else {
            Run::Idle
        };
    }

    fn move_on(&mut self, completed: bool) {
        let next = if completed || self.phase != Phase::Focus {
            self.next_phase()
        } else {
            // Skipping a session earns the short break, whatever the count.
            Phase::ShortBreak
        };
        match self.phase {
            Phase::Focus if completed => self.done += 1,
            Phase::LongBreak => self.done = 0,
            _ => {}
        }
        self.phase = next;
    }

    /// Follow new settings. What is running keeps its own time; the length of a part only
    /// matters for the next start and for the ring.
    pub fn set_plan(&mut self, plan: Plan) {
        self.plan = plan;
        self.done = self.done.min(plan.rounds);
        // The long break is when every dot is lit, whatever the round now is.
        if self.phase == Phase::LongBreak {
            self.done = plan.rounds;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn two_rounds() -> Pomodoro {
        Pomodoro::new(Plan {
            rounds: 2,
            ..Plan::default()
        })
    }

    #[test]
    fn counts_down_pauses_and_resets() {
        let mut p = Pomodoro::default();
        assert_eq!(p.left(t(0)), Duration::from_secs(25 * 60));
        assert_eq!(p.fraction(t(0)), 1.0);
        p.toggle(t(100));
        assert!(p.running());
        assert_eq!(p.left(t(160)), Duration::from_secs(24 * 60));
        p.toggle(t(160));
        assert_eq!(p.left(t(9999)), Duration::from_secs(24 * 60));
        p.toggle(t(10_000));
        assert!(!p.finished(t(10_000 + 24 * 60 - 1)));
        assert!(p.finished(t(10_000 + 24 * 60)));
        p.reset();
        assert_eq!(p.left(t(0)), Duration::from_secs(25 * 60));
        assert!(!p.running());
    }

    #[test]
    fn rounds_end_in_the_long_break() {
        let mut p = two_rounds();
        assert_eq!(p.next_phase(), Phase::ShortBreak);
        assert_eq!(p.advance(t(0)), Phase::Focus);
        assert_eq!((p.phase, p.done), (Phase::ShortBreak, 1));
        assert_eq!(p.next_phase(), Phase::Focus);
        assert_eq!(p.advance(t(0)), Phase::ShortBreak);
        assert_eq!((p.phase, p.done), (Phase::Focus, 1));
        // The second session is the last of the round.
        assert_eq!(p.next_phase(), Phase::LongBreak);
        p.advance(t(0));
        assert_eq!((p.phase, p.done), (Phase::LongBreak, 2));
        assert_eq!(p.left(t(0)), Duration::from_secs(15 * 60));
        // After it, the next round begins from nothing.
        p.advance(t(0));
        assert_eq!((p.phase, p.done), (Phase::Focus, 0));
        assert!(!p.running(), "waits to be started");
    }

    #[test]
    fn skipping_does_not_count_a_session() {
        let mut p = two_rounds();
        p.toggle(t(0));
        p.skip(t(0));
        assert_eq!((p.phase, p.done), (Phase::ShortBreak, 0));
        assert!(!p.running());
        p.skip(t(0));
        assert_eq!((p.phase, p.done), (Phase::Focus, 0));
        // Even the last session of a round, skipped, only earns the short break.
        p.advance(t(0));
        p.skip(t(0));
        assert_eq!((p.phase, p.done), (Phase::Focus, 1));
        p.skip(t(0));
        assert_eq!((p.phase, p.done), (Phase::ShortBreak, 1));
        // A skipped long break still starts the round over.
        let mut p = two_rounds();
        p.advance(t(0));
        p.skip(t(0));
        p.advance(t(0));
        assert_eq!(p.phase, Phase::LongBreak);
        p.skip(t(0));
        assert_eq!((p.phase, p.done), (Phase::Focus, 0));
    }

    #[test]
    fn auto_start_runs_the_next_part_from_now() {
        let mut p = Pomodoro::new(Plan {
            auto_start: true,
            ..Plan::default()
        });
        p.toggle(t(0));
        // Noticed late, the break still gets all its minutes.
        p.advance(t(30 * 60));
        assert_eq!(p.phase, Phase::ShortBreak);
        assert_eq!(p.left(t(30 * 60)), Duration::from_secs(5 * 60));
        assert!(p.running());
    }

    #[test]
    fn skipping_keeps_an_automatic_run_going() {
        let mut p = Pomodoro::new(Plan {
            auto_start: true,
            ..Plan::default()
        });
        p.skip(t(0));
        assert!(!p.running(), "nothing was running");
        p.toggle(t(10));
        p.skip(t(20));
        assert_eq!(p.phase, Phase::Focus);
        assert_eq!(p.left(t(20)), Duration::from_secs(25 * 60));
        assert!(p.running());
        // Paused is not running: it waits to be started.
        p.toggle(t(30));
        p.skip(t(40));
        assert!(!p.running());
    }

    #[test]
    fn a_new_plan_keeps_the_time_running() {
        let mut p = Pomodoro::default();
        p.toggle(t(0));
        p.set_plan(Plan {
            focus_min: 50,
            ..Plan::default()
        });
        assert_eq!(p.left(t(60)), Duration::from_secs(24 * 60));
        assert_eq!(p.fraction(t(60)), 24.0 / 50.0);
        p.reset();
        assert_eq!(p.left(t(60)), Duration::from_secs(50 * 60));
        // Fewer rounds than done doesn't leave dots over.
        let mut p = Pomodoro {
            done: 3,
            ..Pomodoro::default()
        };
        p.set_plan(Plan {
            rounds: 2,
            ..Plan::default()
        });
        assert_eq!(p.done, 2);
    }

    #[test]
    fn phases_survive_the_state_file() {
        for p in [Phase::Focus, Phase::ShortBreak, Phase::LongBreak] {
            assert_eq!(Phase::from_index(p.index()), p);
        }
        assert_eq!(Phase::from_index(200), Phase::Focus);
    }
}

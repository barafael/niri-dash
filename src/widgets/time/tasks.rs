//! What waits on the clock: the end of a countdown or a pomodoro part (which goes off shown or
//! not), the sleep timer's count, and the ticker that keeps the displays running while the
//! dashboard is up.
//!
//! Hooks that have no `cx` can't start a task, so [`ensure`] is called by everything that has one
//! (an action, a key, a frame being drawn) and starts what the state calls for and isn't running
//! yet. A task that finds its state changed under it quietly ends.

use crate::dash::Dash;
use crate::timer::Run;
use gpui::{AsyncApp, Context, WeakEntity};
use std::cell::Cell;
use std::time::Duration;
use web_time::SystemTime;

/// The sleep timer counts this often; it shows whole seconds.
pub const SLEEP_TICK: Duration = Duration::from_millis(250);

/// How often the displays are redrawn while something runs and is shown.
const TICKER: Duration = Duration::from_millis(100);

/// One deadline being waited for.
#[derive(Default)]
pub struct Waiting {
    end: Cell<Option<SystemTime>>,
    epoch: Cell<u64>,
}

impl Waiting {
    /// Wait again for the same end (the task woke too early).
    pub fn forget(&self) {
        self.end.set(None);
    }
}

#[derive(Default)]
pub struct Tasks {
    ticker: Cell<bool>,
    pub timer: Waiting,
    pub pomo: Waiting,
    sleep: Cell<bool>,
}

#[derive(Clone, Copy)]
enum Which {
    Timer,
    Pomo,
}

/// Start what has to be running and isn't.
pub fn ensure(d: &Dash, cx: &mut Context<Dash>) {
    let t = &d.widgets.time;
    let now = t.now();
    let end_of = |run: Run| match run {
        Run::Running { end } => Some(end),
        _ => None,
    };
    let timer_end = end_of(t.timers.countdown.state);
    let pomo_end = end_of(t.pomo.pomodoro.run);
    for (which, waiting, end) in [
        (Which::Timer, &t.tasks.timer, timer_end),
        (Which::Pomo, &t.tasks.pomo, pomo_end),
    ] {
        if waiting.end.get() == end {
            continue;
        }
        waiting.end.set(end);
        let epoch = waiting.epoch.get() + 1;
        waiting.epoch.set(epoch);
        if let Some(end) = end {
            let left = end.duration_since(now).unwrap_or_default().max(Duration::from_millis(1));
            deadline(cx, which, epoch, left);
        }
    }
    let shown_and_running =
        d.visible && (timer_end.is_some() || t.timers.stopwatch.running() || pomo_end.is_some());
    if shown_and_running && !t.tasks.ticker.get() {
        t.tasks.ticker.set(true);
        ticker(cx);
    }
    let armed = t.sleep.armed();
    if armed {
        t.sleep.start_counting(now);
        if !t.tasks.sleep.get() {
            t.tasks.sleep.set(true);
            sleep_ticker(cx);
        }
    } else {
        t.sleep.stop_counting();
    }
}

fn deadline(cx: &mut Context<Dash>, which: Which, epoch: u64, left: Duration) {
    cx.spawn(async move |this: WeakEntity<Dash>, cx: &mut AsyncApp| {
        cx.background_executor().timer(left).await;
        let _ = this.update(cx, |d, cx| {
            let current = match which {
                Which::Timer => d.widgets.time.tasks.timer.epoch.get(),
                Which::Pomo => d.widgets.time.tasks.pomo.epoch.get(),
            };
            if current != epoch {
                return;
            }
            match which {
                Which::Timer => super::countdown::went_off(d, cx),
                Which::Pomo => super::pomo::went_off(d, cx),
            }
        });
    })
    .detach();
}

fn ticker(cx: &mut Context<Dash>) {
    cx.spawn(async move |this: WeakEntity<Dash>, cx: &mut AsyncApp| {
        loop {
            cx.background_executor().timer(TICKER).await;
            let go = this.update(cx, |d, cx| {
                let t = &d.widgets.time;
                let running = matches!(t.timers.countdown.state, Run::Running { .. })
                    || t.timers.stopwatch.running()
                    || matches!(t.pomo.pomodoro.run, Run::Running { .. });
                if d.visible && running {
                    cx.notify();
                    true
                } else {
                    t.tasks.ticker.set(false);
                    false
                }
            });
            if !matches!(go, Ok(true)) {
                break;
            }
        }
    })
    .detach();
}

fn sleep_ticker(cx: &mut Context<Dash>) {
    cx.spawn(async move |this: WeakEntity<Dash>, cx: &mut AsyncApp| {
        loop {
            cx.background_executor().timer(SLEEP_TICK).await;
            let go = this.update(cx, |d, cx| {
                if d.widgets.time.sleep.armed() {
                    super::sleep::tick(d, cx);
                    true
                } else {
                    d.widgets.time.tasks.sleep.set(false);
                    false
                }
            });
            if !matches!(go, Ok(true)) {
                break;
            }
        }
    })
    .detach();
}

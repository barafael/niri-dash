//! The time widgets (clock, calendar, weather, timer, stopwatch, pomodoro, sleep timer) on the
//! dashboard against channels standing in for the system: what they send when they are used,
//! what they show when the system reports something, what the keys do. The timers count on the
//! wall clock; a test lets time pass by skipping it (`pass`) and letting the dashboard's own
//! clock run down to the moment.
//!
//! A plain program (`harness = false`) that runs each scenario in turn on a fresh dashboard.

use chrono::Datelike;
use niri_dash::testing::{Harness, Monkey, Sandbox};
use niri_dash::{Control, testing};
use niri_dash_system::{Event, clipboard, host, notifications, sleep, weather};
use std::panic::AssertUnwindSafe;
use std::time::Duration;

/// Where the daemon keeps what it remembers (the sandbox's).
fn state_file() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("XDG_STATE_HOME").expect("a sandbox"))
        .join("niri-dash/state.json")
}

/// A dashboard with `widgets` (the lines of the layout), shown, that remembers nothing.
fn dashboard(widgets: &str) -> Harness {
    let _ = std::fs::remove_file(state_file());
    let mut h = Harness::new(&format!(
        "grid columns=16 rows=9\nlayout {{\n{widgets}\n}}\n"
    ));
    h.control(Control::Show);
    h
}

/// Enter the widget of `kind`, as its hint label would.
fn enter(h: &mut Harness, kind: &str) {
    h.update(|d, _| {
        let id = d
            .tiles
            .iter()
            .find(|t| t.kind().name() == kind)
            .unwrap_or_else(|| panic!("no {kind} on the dashboard"))
            .id();
        d.enter(id);
    });
}

fn id_of(h: &Harness, kind: &str) -> u32 {
    h.read(|d| {
        d.tiles
            .iter()
            .find(|t| t.kind().name() == kind)
            .unwrap_or_else(|| panic!("no {kind} on the dashboard"))
            .id()
    })
}

/// Let `secs` of wall-clock time go by: skipped for the timers, run for the dashboard's clock.
fn pass(h: &mut Harness, secs: u64) {
    h.update(|d, _| d.widgets.time.pass(Duration::from_secs(secs)));
    h.advance(secs);
}

// ---- pomodoro -------------------------------------------------------------------------------

const POMODORO: &str = "    pomodoro col=0 row=0 width=4 height=3 shape=\"rect\"";

/// The pomodoro starts, pauses, skips and starts over from the keyboard; a skipped focus session
/// is no session done.
fn pomodoro_keys() {
    let mut h = dashboard(POMODORO);
    let view = |h: &Harness| h.read(|d| d.widgets.time.pomo.view(d.widgets.time.now()));
    let v = view(&h);
    assert_eq!(v.text, "25:00");
    assert_eq!(v.label, "Focus");
    assert_eq!((v.done, v.rounds), (0, 4));
    enter(&mut h, "pomodoro");
    h.key(" ");
    assert!(view(&h).running);
    h.key(" ");
    assert!(view(&h).paused && !view(&h).running);
    // n: the next part, stopped, and not counted.
    h.key("n");
    let v = view(&h);
    assert_eq!(v.label, "Short break");
    assert!(v.on_break && !v.paused && !v.running);
    assert_eq!(v.text, "5:00");
    assert_eq!(v.next, "Next: focus, 25 min");
    h.key("n");
    assert_eq!(view(&h).label, "Focus");
    assert_eq!(view(&h).done, 0);
    h.key(" ");
    assert!(view(&h).running);
    h.key("r");
    let v = view(&h);
    assert!(!v.running && !v.paused);
    assert_eq!(v.text, "25:00");
}

/// The widget's settings are the plan.
fn pomodoro_follows_its_settings() {
    let h = dashboard(
        "    pomodoro col=0 row=0 width=4 height=3 shape=\"rect\" {\n        focus-minutes 50\n        short-break 10\n        rounds 2\n    }",
    );
    let v = h.read(|d| d.widgets.time.pomo.view(d.widgets.time.now()));
    assert_eq!(v.text, "50:00");
    assert_eq!(v.rounds, 2);
    assert_eq!(v.next, "Next: short break, 10 min");
}

/// Enter goes through the buttons: j to skip, Enter presses it.
fn pomodoro_buttons() {
    let mut h = dashboard(POMODORO);
    enter(&mut h, "pomodoro");
    h.key("enter");
    assert!(h.read(|d| d.widgets.time.pomo.pomodoro.running()));
    h.keys("j j");
    // Start, skip and, once running, reset.
    h.key("enter");
    assert_eq!(
        h.read(|d| d.widgets.time.pomo.pomodoro.phase.label()),
        "Focus"
    );
    h.click_on("Start");
    assert!(h.read(|d| d.widgets.time.pomo.pomodoro.running()));
    h.click_on("Skip to the next part");
    assert_eq!(
        h.read(|d| d.widgets.time.pomo.pomodoro.phase.label()),
        "Short break"
    );
}

/// A round pomodoro with room has both the play and the skip button under its dots (as Slint's
/// does; the round timer's only has the face to click).
fn pomodoro_round_buttons() {
    let mut h = dashboard("    pomodoro col=0 row=0 size=4 shape=\"round\"");
    assert!(h.has("Skip to the next part"));
    h.click_on("Skip to the next part");
    assert_eq!(
        h.read(|d| d.widgets.time.pomo.pomodoro.phase.label()),
        "Short break"
    );
    let h = dashboard("    timer col=0 row=0 size=4 shape=\"round\"");
    assert!(!h.has("Lap") && !h.has("Reset"));
}

/// A part that runs out says so, moves on, and is remembered across a restart.
fn pomodoro_goes_off() {
    let mut h = dashboard(POMODORO);
    while h.inbox.notifications.try_recv().is_ok() {}
    enter(&mut h, "pomodoro");
    h.key(" ");
    pass(&mut h, 25 * 60);
    let v = h.read(|d| d.widgets.time.pomo.view(d.widgets.time.now()));
    assert_eq!(v.label, "Short break");
    assert_eq!(v.done, 1);
    assert!(!v.running, "the next part waits to be started");
    let mut summary = None;
    while let Ok(c) = h.inbox.notifications.try_recv() {
        if let notifications::Command::AddLocal {
            app, summary: s, ..
        } = c
        {
            assert_eq!(app, "Pomodoro");
            summary = Some(s);
        }
    }
    assert_eq!(
        summary.as_deref(),
        Some("Focus session done. Take 5 minutes")
    );
    let mut sound = false;
    while let Ok(c) = h.inbox.host.try_recv() {
        sound |= matches!(c, host::Command::PlaySound(host::Sound::Alarm));
    }
    assert!(sound, "no alarm");
    // Where it stood is saved.
    let saved = h.read(|d| d.state.pomodoro);
    assert_eq!((saved.phase, saved.done, saved.end), (1, 1, None));
}

// ---- timer and stopwatch --------------------------------------------------------------------

const TIMER: &str = "    timer col=0 row=0 width=5 height=3 shape=\"rect\"";

/// Space starts and pauses, r resets, h/l take a minute off or add one, the presets set it.
fn timer_keys() {
    let mut h = dashboard(TIMER);
    let view = |h: &Harness| h.read(|d| d.widgets.time.timers.view(d.widgets.time.now()));
    assert_eq!(view(&h).text, "5:00");
    enter(&mut h, "timer");
    h.key("l");
    assert_eq!(view(&h).text, "6:00");
    h.key("h");
    h.key("h");
    assert_eq!(view(&h).text, "4:00");
    h.key(" ");
    assert!(view(&h).running);
    assert!(
        h.read(|d| d.state.timer_end.is_some()),
        "a running timer is saved"
    );
    h.key(" ");
    assert!(view(&h).paused && !view(&h).running);
    assert!(h.read(|d| d.state.timer_end.is_none()));
    h.key("r");
    let v = view(&h);
    assert!(!v.paused && !v.running);
    assert_eq!(v.text, "4:00");
    assert_eq!(h.read(|d| d.state.timer_secs), 240);
    // Down to the presets: the stops are start, then the presets.
    h.key("j");
    h.key("enter");
    let v = view(&h);
    assert!(v.running);
    assert_eq!(v.text, "1:00");
}

fn timer_clicks() {
    let mut h = dashboard(TIMER);
    h.click_on("Start");
    assert!(h.read(|d| d.widgets.time.timers.view(d.widgets.time.now()).running));
    h.click_on("Pause");
    assert!(h.read(|d| d.widgets.time.timers.view(d.widgets.time.now()).paused));
    h.click_on("Reset");
    assert!(h.read(|d| !d.widgets.time.timers.view(d.widgets.time.now()).paused));
    h.click_on("Add a minute");
    assert_eq!(
        h.read(|d| d.widgets.time.timers.view(d.widgets.time.now()).text),
        "6:00"
    );
    h.click_on("Set timer to 10 min");
    let v = h.read(|d| d.widgets.time.timers.view(d.widgets.time.now()));
    assert!(v.running);
    assert_eq!(v.text, "10:00");
}

/// A countdown that runs out says so, shown or not, and shows "Done" for a while.
fn timer_goes_off() {
    let mut h = dashboard(TIMER);
    while h.inbox.notifications.try_recv().is_ok() {}
    enter(&mut h, "timer");
    h.key("h");
    h.key("h");
    h.key("h");
    h.key("h");
    // One minute at least: it is 1:00 now.
    h.key(" ");
    // The dashboard goes away; the timer still goes off.
    h.control(Control::Hide);
    pass(&mut h, 61);
    let v = h.read(|d| d.widgets.time.timers.view(d.widgets.time.now()));
    assert!(v.done, "Done is shown after it went off");
    assert!(!v.running && !v.paused);
    let mut said = None;
    while let Ok(c) = h.inbox.notifications.try_recv() {
        if let notifications::Command::AddLocal { app, summary, .. } = c {
            assert_eq!(app, "Timer");
            said = Some(summary);
        }
    }
    assert_eq!(said.as_deref(), Some("1:00 timer is done"));
    let mut sound = false;
    while let Ok(c) = h.inbox.host.try_recv() {
        sound |= matches!(c, host::Command::PlaySound(host::Sound::Alarm));
    }
    assert!(sound, "no alarm");
    assert!(h.read(|d| d.state.timer_end.is_none()));
    // "Done" fades after a minute.
    pass(&mut h, 61);
    assert!(!h.read(|d| d.widgets.time.timers.view(d.widgets.time.now()).done));
}

/// A timer that was running when the daemon stopped is running when it starts again.
fn timer_survives_a_restart() {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let state = format!("{{\"timer_secs\":600,\"timer_end\":{}}}", now_ms + 120_000);
    // SAFETY: tests run one after the other on one thread.
    let path =
        std::env::temp_dir().join(format!("niri-dash-time-state-{}.json", std::process::id()));
    std::fs::write(&path, state).unwrap();
    unsafe { std::env::set_var("NIRI_DASH_STATE", &path) };
    let h = dashboard(TIMER);
    unsafe { std::env::remove_var("NIRI_DASH_STATE") };
    let _ = std::fs::remove_file(&path);
    let v = h.read(|d| d.widgets.time.timers.view(d.widgets.time.now()));
    assert!(v.running);
    assert!(v.text == "2:00" || v.text == "1:59", "{}", v.text);
    assert_eq!(
        h.read(|d| d.widgets.time.timers.countdown.length),
        Duration::from_secs(600)
    );
}

const STOPWATCH: &str = "    stopwatch col=0 row=0 width=5 height=5 shape=\"rect\"";

fn stopwatch_laps() {
    let mut h = dashboard(STOPWATCH);
    let view = |h: &Harness| h.read(|d| d.widgets.time.timers.view(d.widgets.time.now()));
    enter(&mut h, "stopwatch");
    assert_eq!(view(&h).sw_text, "0:00.0");
    h.key(" ");
    assert!(view(&h).sw_running);
    h.update(|d, _| d.widgets.time.pass(Duration::from_secs(5)));
    h.key("a");
    h.update(|d, _| d.widgets.time.pass(Duration::from_secs(3)));
    h.key("a");
    assert_eq!(view(&h).laps.len(), 2);
    assert!(view(&h).laps[0].starts_with("0:03"), "{:?}", view(&h).laps);
    // h/l mean nothing to a stopwatch.
    h.key("h");
    h.key(" ");
    assert!(!view(&h).sw_running && view(&h).sw_started);
    h.key("r");
    let v = view(&h);
    assert!(!v.sw_started && v.laps.is_empty());
}

// ---- sleep timer ----------------------------------------------------------------------------

const SLEEP: &str = "    sleep-timer col=0 row=0 width=4 height=3";

fn sleeper<R>(h: &Harness, f: impl FnOnce(&sleep::Controller) -> R) -> R {
    h.read(|d| f(&d.widgets.time.sleep.ctl))
}

/// The sleep timer asks which modes the machine offers, and arms, pauses, nudges and gives up on
/// a countdown from the keyboard, as sleepy's window does.
fn sleep_timer_keys() {
    let mut h = dashboard(SLEEP);
    assert!(matches!(
        h.inbox.sleep.try_recv(),
        Ok(sleep::Command::Detect { script }) if script.ends_with("niri-dash/sleep.sh")
    ));
    h.send(sleep::Event::Modes(vec![
        sleep::Mode::Suspend,
        sleep::Mode::Hibernate,
    ]));
    assert_eq!(sleeper(&h, |c| c.modes.len()), 2);
    assert_eq!(sleeper(&h, |c| c.when().unwrap().echo()), "→ in 30m");
    // A field that is no when says why, and refuses to arm.
    h.update(|d, _| d.widgets.time.sleep.ctl.input = "soon".into());
    assert_eq!(
        sleeper(&h, |c| c.when().unwrap_err().to_string()),
        "not a duration; try 45s, 30 min, 1h30m"
    );
    h.update(|d, _| d.widgets.time.sleep.ctl.input = "90 min".into());
    assert_eq!(sleeper(&h, |c| c.when().unwrap().echo()), "→ in 1h 30m");
    enter(&mut h, "sleep-timer");
    // d: the dry run; a: the screen stays on.
    h.key("d");
    assert!(sleeper(&h, |c| c.dry_run));
    h.key("a");
    assert!(sleeper(&h, |c| c.keep_awake));
    // j to the mode, l to the next; Enter arms with it.
    h.key("j");
    h.key("l");
    assert_eq!(sleeper(&h, |c| c.selected), 1);
    h.key("enter");
    assert_eq!(sleeper(&h, |c| c.timer.status()), sleep::Status::Armed);
    let headline = |h: &Harness| sleeper(h, |c| c.headline(&chrono::Local::now()));
    assert!(
        headline(&h).starts_with("hibernate in 1h 30m, at "),
        "{}",
        headline(&h)
    );
    // Holding the screen is the system's to do, while it counts.
    assert!(matches!(
        h.inbox.sleep.try_recv(),
        Ok(sleep::Command::KeepAwake(true))
    ));
    h.key("-");
    assert!(
        headline(&h).starts_with("hibernate in 1h 29m"),
        "{}",
        headline(&h)
    );
    h.key("+");
    h.key("+");
    assert!(
        headline(&h).starts_with("hibernate in 1h 30m"),
        "no more than it was armed with: {}",
        headline(&h)
    );
    h.key(" ");
    assert_eq!(sleeper(&h, |c| c.timer.status()), sleep::Status::Paused);
    assert_eq!(headline(&h), "paused, 1h 30m left");
    h.key(" ");
    assert_eq!(sleeper(&h, |c| c.timer.status()), sleep::Status::Armed);
    // Escape saves the machine, and lets go of the screen.
    h.key("esc");
    assert_eq!(sleeper(&h, |c| c.timer.status()), sleep::Status::Idle);
    assert!(matches!(
        h.inbox.sleep.try_recv(),
        Ok(sleep::Command::KeepAwake(false))
    ));
    assert!(h.inbox.sleep.try_recv().is_err(), "nothing was run");
}

/// Tall enough, the modes are a list, and the highlight follows the cursor down it.
fn sleep_timer_lists_modes() {
    let mut h = dashboard("    sleep-timer col=0 row=0 width=4 height=5");
    h.send(sleep::Event::Modes(vec![
        sleep::Mode::Suspend,
        sleep::Mode::Hibernate,
        sleep::Mode::Poweroff,
    ]));
    enter(&mut h, "sleep-timer");
    h.key("j");
    assert_eq!(sleeper(&h, |c| c.selected), 0);
    h.key("j");
    h.key("j");
    assert_eq!(sleeper(&h, |c| c.selected), 2);
    h.key("k");
    assert_eq!(sleeper(&h, |c| c.selected), 1);
    h.key("enter");
    assert_eq!(sleeper(&h, |c| c.timer.status()), sleep::Status::Armed);
    assert!(sleeper(&h, |c| c.headline(&chrono::Local::now())).starts_with("hibernate in 30m"));
}

/// A countdown that runs out starts the mode once, or for a dry run says what it would have done.
fn sleep_timer_fires() {
    let mut h = dashboard("    sleep-timer col=0 row=0 width=4 height=4");
    h.send(sleep::Event::Modes(vec![sleep::Mode::Suspend]));
    h.update(|d, _| d.widgets.time.sleep.ctl.input = "1s".into());
    enter(&mut h, "sleep-timer");
    h.keys("j enter");
    assert_eq!(sleeper(&h, |c| c.timer.status()), sleep::Status::Armed);
    let mut asked = None;
    for _ in 0..4 {
        pass(&mut h, 1);
        while let Ok(c) = h.inbox.sleep.try_recv() {
            if let sleep::Command::Run { mode, .. } = c {
                asked = Some(mode);
            }
        }
        if asked.is_some() {
            break;
        }
    }
    assert_eq!(asked, Some(sleep::Mode::Suspend));
    assert_eq!(sleeper(&h, |c| c.timer.status()), sleep::Status::Fired);
    assert_eq!(
        sleeper(&h, |c| c.headline(&chrono::Local::now())),
        "time! suspend the machine…"
    );
    // The system's answer, when it fails, is the line under the headline.
    h.send(sleep::Event::Done {
        mode: sleep::Mode::Suspend,
        error: Some("suspend refused: busy".into()),
    });
    assert_eq!(
        sleeper(&h, |c| c.outcome()).as_deref(),
        Some("suspend refused: busy")
    );
    // A dry run does nothing but say so.
    h.key("esc");
    assert_eq!(sleeper(&h, |c| c.timer.status()), sleep::Status::Idle);
    h.key("d");
    h.key("enter");
    assert_eq!(sleeper(&h, |c| c.timer.status()), sleep::Status::Armed);
    for _ in 0..4 {
        pass(&mut h, 1);
        if sleeper(&h, |c| c.timer.status()) == sleep::Status::Fired {
            break;
        }
    }
    assert_eq!(
        sleeper(&h, |c| c.outcome()).as_deref(),
        Some("dry run: would have run systemctl suspend")
    );
    while let Ok(c) = h.inbox.sleep.try_recv() {
        assert!(!matches!(c, sleep::Command::Run { .. }), "a dry run ran it");
    }
}

/// `i` (or Enter on the field) types in the when field: the letters are the field's, Enter arms
/// with what was typed and gives the keyboard back, Escape gives it back and keeps the text.
fn sleep_timer_field() {
    let mut h = dashboard(SLEEP);
    h.send(sleep::Event::Modes(vec![sleep::Mode::Suspend]));
    enter(&mut h, "sleep-timer");
    let editing = |h: &Harness| h.read(|d| d.widgets.time.sleep.editing);
    h.key("i");
    assert!(editing(&h));
    // Everything typed goes in the field, which starts with what was there; Ctrl+D still works.
    h.key("ctrl+a");
    h.type_text("90 min");
    assert_eq!(sleeper(&h, |c| c.input.clone()), "90 min");
    h.key("ctrl+d");
    assert!(sleeper(&h, |c| c.dry_run));
    assert!(editing(&h));
    // Escape: the keyboard is the dashboard's again, and so are the keys.
    h.key("esc");
    assert!(!editing(&h));
    assert_eq!(sleeper(&h, |c| c.input.clone()), "90 min");
    assert_eq!(sleeper(&h, |c| c.timer.status()), sleep::Status::Idle);
    h.key("d");
    assert!(!sleeper(&h, |c| c.dry_run));
    // Enter in the field arms with what is typed.
    h.key("i");
    h.key("ctrl+a");
    h.type_text("45 min");
    h.key("enter");
    assert!(!editing(&h));
    assert_eq!(sleeper(&h, |c| c.timer.status()), sleep::Status::Armed);
    assert!(sleeper(&h, |c| c.headline(&chrono::Local::now())).starts_with("suspend in 45m"));
    // The dashboard going away takes the keyboard out of the field.
    h.key("esc");
    h.key("i");
    assert!(editing(&h));
    h.control(Control::Hide);
    assert!(!editing(&h));
}

/// Leaving the widget with the keyboard in the field doesn't leave the keys in it.
fn sleep_timer_field_left_behind() {
    let mut h = dashboard(
        "    sleep-timer col=0 row=0 width=4 height=3\n    pomodoro col=4 row=0 width=4 height=3",
    );
    enter(&mut h, "sleep-timer");
    h.key("i");
    assert!(h.read(|d| d.widgets.time.sleep.editing));
    h.key("ctrl+l");
    assert_eq!(
        h.read(|d| d.tile(d.nav.active_id).unwrap().kind().name()),
        "pomodoro"
    );
    // Whatever is typed now is the dashboard's, not the field's.
    assert!(
        !h.read(|d| d.widgets.time.sleep.editing),
        "still typing in a field of a widget left"
    );
    h.key(" ");
    assert!(h.read(|d| d.widgets.time.pomo.pomodoro.running()));
    for _ in 0..4 {
        h.key("esc");
    }
    assert!(h.controls().contains(&Control::Hide), "Escape got nowhere");
}

/// The sleep timer counts while the dashboard is away.
fn sleep_timer_counts_while_hidden() {
    let mut h = dashboard("    sleep-timer col=0 row=0 width=4 height=4");
    h.send(sleep::Event::Modes(vec![sleep::Mode::Suspend]));
    h.update(|d, _| d.widgets.time.sleep.ctl.input = "2s".into());
    enter(&mut h, "sleep-timer");
    h.keys("j enter");
    h.control(Control::Hide);
    pass(&mut h, 3);
    assert_eq!(sleeper(&h, |c| c.timer.status()), sleep::Status::Fired);
}

fn sleep_timer_clicks() {
    let mut h = dashboard("    sleep-timer col=0 row=0 width=4 height=3");
    h.send(sleep::Event::Modes(vec![
        sleep::Mode::Suspend,
        sleep::Mode::Hibernate,
    ]));
    h.click_on("Mode");
    assert_eq!(sleeper(&h, |c| c.selected), 1);
    h.click_on("Dry run");
    assert!(sleeper(&h, |c| c.dry_run));
    h.click_on("Arm");
    assert_eq!(sleeper(&h, |c| c.timer.status()), sleep::Status::Armed);
    h.click_on("Pause");
    assert_eq!(sleeper(&h, |c| c.timer.status()), sleep::Status::Paused);
    h.click_on("Abort");
    assert_eq!(sleeper(&h, |c| c.timer.status()), sleep::Status::Idle);
}

// ---- calendar -------------------------------------------------------------------------------

/// A calendar, entered, with its cursor moved two days on from today; and where that is.
fn calendar_with_a_day_picked() -> (Harness, chrono::NaiveDate) {
    let mut h = dashboard("    calendar col=0 row=0 width=5 height=5");
    enter(&mut h, "calendar");
    h.key("l");
    h.key("l");
    let on = h.read(|d| d.widgets.time.calendar.cursor());
    assert_eq!(on, chrono::Local::now().date_naive() + chrono::Days::new(2));
    (h, on)
}

fn cursor(h: &Harness) -> chrono::NaiveDate {
    h.read(|d| d.widgets.time.calendar.cursor())
}

/// The picked day stays picked, whatever else is pressed: only what moves it may move it.
fn calendar_cursor_survives_other_keys() {
    let (mut h, on) = calendar_with_a_day_picked();
    let keys = [
        "enter",
        "space",
        "tab",
        "backtab",
        "backspace",
        "a",
        "b",
        "c",
        "d",
        "g",
        "G",
        "i",
        "m",
        "o",
        "q",
        "r",
        "u",
        "v",
        "w",
        "x",
        "y",
        "z",
        "1",
        "ctrl+c",
        "ctrl+a",
        "alt+x",
        "shift+tab",
    ];
    let lost: Vec<&str> = keys
        .iter()
        .copied()
        .filter(|key| {
            h.key(key);
            let lost = cursor(&h) != on;
            if lost {
                println!("  {key:?} took the cursor from {on} to {}", cursor(&h));
                // Back where it was, for the next key.
                h.key("t");
                h.key("l");
                h.key("l");
            }
            lost
        })
        .collect();
    assert!(lost.is_empty(), "lost the picked day on {lost:?}");
}

/// Blowing the widget up and leaving it for a moment don't lose the picked day either.
fn calendar_cursor_survives_expanding_and_leaving() {
    let (mut h, on) = calendar_with_a_day_picked();
    h.key("e");
    assert_eq!(cursor(&h), on);
    h.key("e");
    assert_eq!(cursor(&h), on);
    h.key("esc");
    enter(&mut h, "calendar");
    assert_eq!(cursor(&h), on);
}

/// Today is still one key away.
fn calendar_cursor_goes_home_on_t() {
    let (mut h, on) = calendar_with_a_day_picked();
    h.key("t");
    assert_ne!(cursor(&h), on);
    assert_eq!(cursor(&h), chrono::Local::now().date_naive());
}

/// j/k a week, n/p a month, Page Down/Up too; the month turns as the cursor leaves it.
fn calendar_moves_by_week_and_month() {
    let (mut h, on) = calendar_with_a_day_picked();
    h.key("j");
    assert_eq!(cursor(&h), on + chrono::Days::new(7));
    h.key("k");
    h.key("k");
    assert_eq!(cursor(&h), on - chrono::Days::new(7));
    let before = on - chrono::Days::new(7);
    h.key("n");
    assert_ne!(cursor(&h).month0(), before.month0());
    assert!(cursor(&h) > before && cursor(&h) <= before + chrono::Days::new(31));
    h.key("p");
    assert_eq!(cursor(&h), on - chrono::Days::new(7));
    // The month shown is where the cursor is.
    h.key("n");
    assert_eq!(h.read(|d| d.widgets.time.calendar.month_offset), 1);
}

/// Ctrl+C copies the day under the cursor as a date, and leaves it there.
fn calendar_copies_the_picked_day() {
    let (mut h, on) = calendar_with_a_day_picked();
    while h.inbox.clipboard.try_recv().is_ok() {}
    h.key("ctrl+c");
    let Ok(clipboard::Command::Set(clip)) = h.inbox.clipboard.try_recv() else {
        panic!("nothing copied");
    };
    assert_eq!(
        String::from_utf8_lossy(&clip.data),
        on.format("%Y-%m-%d").to_string()
    );
    assert_eq!(cursor(&h), on);
}

/// The buttons turn the month, and "This month" is for coming back.
fn calendar_buttons() {
    let mut h = dashboard("    calendar col=0 row=0 width=5 height=5");
    h.click_on("Next month");
    assert_eq!(h.read(|d| d.widgets.time.calendar.month_offset), 1);
    h.click_on("Previous month");
    h.click_on("Previous month");
    assert_eq!(h.read(|d| d.widgets.time.calendar.month_offset), -1);
    h.click_on("This month");
    assert_eq!(h.read(|d| d.widgets.time.calendar.month_offset), 0);
}

/// Round, h/l (or j/k) turn the month and Enter or t come back to this one.
fn calendar_round() {
    let mut h = dashboard("    calendar col=0 row=0 size=3 shape=\"round\"");
    enter(&mut h, "calendar");
    h.key("l");
    h.key("l");
    assert_eq!(h.read(|d| d.widgets.time.calendar.month_offset), 2);
    h.key("h");
    assert_eq!(h.read(|d| d.widgets.time.calendar.month_offset), 1);
    h.key("enter");
    assert_eq!(h.read(|d| d.widgets.time.calendar.month_offset), 0);
    h.key("j");
    assert_eq!(h.read(|d| d.widgets.time.calendar.month_offset), -1);
    h.key("t");
    assert_eq!(h.read(|d| d.widgets.time.calendar.month_offset), 0);
    // There is no day to copy.
    while h.inbox.clipboard.try_recv().is_ok() {}
    h.key("ctrl+c");
    assert!(h.inbox.clipboard.try_recv().is_err());
}

// ---- weather and clock ----------------------------------------------------------------------

fn forecast() -> weather::State {
    weather::State {
        place: "Hamburg".into(),
        temp: "12°".into(),
        feels: "feels like 9°".into(),
        description: "Partly cloudy".into(),
        icon: weather::Icon::CloudSun,
        details: "14° / 7°".into(),
        days: vec![weather::Day {
            name: "Wed".into(),
            icon: weather::Icon::Rain,
            max: "11°".into(),
            min: "6°".into(),
        }],
        error: String::new(),
    }
}

/// The forecast shows when it comes; an error keeps the last good one and is said instead.
fn weather_shows_what_comes() {
    let mut h = dashboard(
        "    weather col=0 row=0 width=5 height=4\n    weather col=6 row=0 size=3 shape=\"round\"",
    );
    assert!(!h.read(|d| d.widgets.time.weather.loaded));
    h.send(Event::Weather(weather::Event::State(forecast())));
    assert!(h.read(|d| d.widgets.time.weather.loaded));
    assert_eq!(
        h.read(|d| d
            .widgets
            .time
            .weather
            .forecast
            .as_ref()
            .map(|f| f.temp.clone())),
        Some("12°".to_string())
    );
    let mut failed = forecast();
    failed.error = "No network".into();
    h.send(Event::Weather(weather::Event::State(failed)));
    assert!(h.read(|d| d.widgets.time.weather.loaded));
    assert_eq!(
        h.read(|d| d.widgets.time.weather.error.clone()),
        "No network"
    );
    assert!(h.read(|d| d.widgets.time.weather.forecast.is_some()));
}

/// Other time zones are listed under each clock that has them; one that doesn't exist is dropped.
fn clock_lists_zones() {
    let mut h = dashboard(
        "    clock col=0 row=0 width=4 height=4 {\n        zones \"Asia/Tokyo, Nowhere/Atlantis\"\n    }\n    clock col=5 row=0 width=4 height=2",
    );
    h.advance(1);
    let (first, second) = (id_of(&h, "clock"), h.read(|d| d.tiles[1].id()));
    let zones =
        |h: &Harness, id: u32| h.read(|d| d.widgets.time.clock.zones.get(&id).map_or(0, Vec::len));
    assert_eq!(zones(&h, first), 1);
    assert_eq!(zones(&h, second), 0);
    let label = h.read(|d| d.widgets.time.clock.zones[&first][0].label.clone());
    assert!(label.contains("Tokyo"), "{label}");
}

/// Every kind of this group draws in each of its shapes, small and large, without trouble.
fn everything_draws_and_takes_keys() {
    let layout = [
        "    clock col=0 row=0 width=3 height=2",
        "    clock col=3 row=0 size=2 shape=\"round\" {\n        seconds #true\n    }",
        "    calendar col=5 row=0 width=4 height=4 {\n        week-numbers #true\n        first-day \"sunday\"\n    }",
        "    calendar col=9 row=0 size=2 shape=\"round\"",
        "    weather col=11 row=0 width=3 height=2",
        "    timer col=0 row=2 size=2 shape=\"round\"",
        "    stopwatch col=2 row=2 size=3 shape=\"round\"",
        "    pomodoro col=0 row=4 size=3 shape=\"round\"",
        "    pomodoro col=3 row=4 width=3 height=2",
        "    sleep-timer col=6 row=4 size=3 shape=\"round\"",
        "    sleep-timer col=9 row=2 width=3 height=2",
        "    timer col=12 row=2 width=2 height=1",
        "    stopwatch col=12 row=4 width=3 height=2",
    ]
    .join("\n");
    let mut h = dashboard(&layout);
    h.send(sleep::Event::Modes(vec![
        sleep::Mode::Suspend,
        sleep::Mode::Hibernate,
    ]));
    h.send(Event::Weather(weather::Event::State(forecast())));
    let ids: Vec<u32> = h.read(|d| d.tiles.iter().map(|t| t.id()).collect());
    for id in ids {
        h.update(|d, _| d.enter(id));
        for key in [
            "j", "k", "l", "h", " ", "enter", "tab", "g", "G", "r", "a", "n", "p", "t", "+", "-",
            "d", "space", "esc", "enter", "e", "e",
        ] {
            h.key(key);
        }
        h.key("esc");
    }
    h.advance(3);
}

// ---- the stress run on these widgets ----------------------------------------------------------

/// Random keys over every kind of this group, with the invariants checked after each.
fn stress_on_the_time_widgets() {
    let layout = [
        "grid columns=16 rows=9",
        "layout {",
        "    clock col=0 row=0 width=3 height=2",
        "    calendar col=3 row=0 width=4 height=4",
        "    weather col=7 row=0 width=3 height=2",
        "    timer col=10 row=0 width=4 height=3",
        "    stopwatch col=0 row=2 width=3 height=3",
        "    pomodoro col=7 row=2 width=3 height=2",
        "    sleep-timer col=10 row=3 width=4 height=4",
        "    timer col=0 row=5 size=2 shape=\"round\"",
        "    pomodoro col=2 row=5 size=2 shape=\"round\"",
        "    sleep-timer col=4 row=5 size=2 shape=\"round\"",
        "    calendar col=6 row=4 size=2 shape=\"round\"",
        "}",
    ]
    .join("\n");
    let ops = std::env::var("NIRI_DASH_STRESS_OPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1200u64);
    let verbose = std::env::var_os("NIRI_DASH_STRESS_LOG").is_some();
    let seeds: Vec<u64> = std::env::var("NIRI_DASH_STRESS_SEEDS")
        .ok()
        .map(|s| s.split(',').filter_map(|n| n.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![1, 2, 3]);
    let only_screen = std::env::var("NIRI_DASH_STRESS_SCREEN").ok();
    for seed in seeds {
        for (name, size) in [("large", (1706, 1066)), ("small", (1280, 800))] {
            if only_screen.as_deref().is_some_and(|o| o != name) {
                continue;
            }
            let monkey = Harness::with_mock_sized(&layout, size)
                .stress(Monkey::new(seed, verbose).keep_going(), ops)
                .unwrap_or_else(|f| panic!("seed {seed}: {f}"));
            if let Err(f) = monkey.finish() {
                panic!("seed {seed} on {size:?}: {f}");
            }
        }
    }
}

fn main() {
    let _sandbox = Sandbox::enter(None).expect("a temporary directory for the dashboard's files");
    // SAFETY: nothing else runs yet that could read the environment.
    unsafe { std::env::set_var("LC_ALL", "C") };
    let scenarios: &[(&str, fn())] = &[
        ("pomodoro_keys", pomodoro_keys),
        (
            "pomodoro_follows_its_settings",
            pomodoro_follows_its_settings,
        ),
        ("pomodoro_buttons", pomodoro_buttons),
        ("pomodoro_round_buttons", pomodoro_round_buttons),
        ("pomodoro_goes_off", pomodoro_goes_off),
        ("timer_keys", timer_keys),
        ("timer_clicks", timer_clicks),
        ("timer_goes_off", timer_goes_off),
        ("timer_survives_a_restart", timer_survives_a_restart),
        ("stopwatch_laps", stopwatch_laps),
        ("sleep_timer_keys", sleep_timer_keys),
        ("sleep_timer_lists_modes", sleep_timer_lists_modes),
        ("sleep_timer_fires", sleep_timer_fires),
        ("sleep_timer_field", sleep_timer_field),
        (
            "sleep_timer_field_left_behind",
            sleep_timer_field_left_behind,
        ),
        (
            "sleep_timer_counts_while_hidden",
            sleep_timer_counts_while_hidden,
        ),
        ("sleep_timer_clicks", sleep_timer_clicks),
        (
            "calendar_cursor_survives_other_keys",
            calendar_cursor_survives_other_keys,
        ),
        (
            "calendar_cursor_survives_expanding_and_leaving",
            calendar_cursor_survives_expanding_and_leaving,
        ),
        (
            "calendar_cursor_goes_home_on_t",
            calendar_cursor_goes_home_on_t,
        ),
        (
            "calendar_moves_by_week_and_month",
            calendar_moves_by_week_and_month,
        ),
        (
            "calendar_copies_the_picked_day",
            calendar_copies_the_picked_day,
        ),
        ("calendar_buttons", calendar_buttons),
        ("calendar_round", calendar_round),
        ("weather_shows_what_comes", weather_shows_what_comes),
        ("clock_lists_zones", clock_lists_zones),
        (
            "everything_draws_and_takes_keys",
            everything_draws_and_takes_keys,
        ),
        ("stress_on_the_time_widgets", stress_on_the_time_widgets),
    ];
    let only = std::env::args().nth(1);
    let mut failed = 0;
    let mut ran = 0;
    for (name, scenario) in scenarios {
        if only
            .as_deref()
            .is_some_and(|o| !name.contains(o) && o != "--nocapture")
        {
            continue;
        }
        ran += 1;
        match std::panic::catch_unwind(AssertUnwindSafe(scenario)) {
            Ok(()) => println!("test {name} ... ok"),
            Err(_) => {
                failed += 1;
                println!("test {name} ... FAILED");
            }
        }
    }
    println!(
        "\ntest result: {}. {} passed; {failed} failed",
        if failed == 0 { "ok" } else { "FAILED" },
        ran - failed
    );
    let _ = testing::default_config;
    if failed > 0 {
        std::process::exit(1);
    }
}

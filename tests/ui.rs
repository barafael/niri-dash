//! The dashboard against channels standing in for the system: what it sends when it is used, and
//! what it shows when the system reports something. No Wayland, no D-Bus.
//!
//! Slint wants one thread and one platform per process, so this is a plain program
//! (`harness = false`) that runs each scenario in turn on a fresh dashboard.

use niri_dash::testing::{Element, Harness};
use niri_dash::{Control, testing};
use niri_dash_system::{Demo, bluetooth, brightness, clipboard, color, session, sleep, tray, wifi};
use slint::{ComponentHandle, Model};
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

const LAYOUT: &str = r#"
layout {
    wifi          col=0 row=0 width=3 height=3
    bluetooth     col=3 row=0 width=3 height=2
    brightness    col=6 row=0 size=2 shape="round"
    tray          col=0 row=3 width=5 height=1
    session       col=0 row=4 width=4 height=1
    color-picker  col=5 row=3 width=3 height=2
}
"#;

fn dashboard() -> Harness {
    testing::Harness::new(LAYOUT)
}

/// Rescanning and connecting are commands to the Wi-Fi actor.
fn wifi_commands() {
    let mut h = dashboard();
    let w = h.ui().global::<niri_dash::Wifi>();
    w.invoke_rescan();
    w.invoke_connect("Harbour Café".into());
    assert!(matches!(h.inbox.wifi.try_recv(), Ok(wifi::Command::Rescan)));
    assert!(matches!(
        h.inbox.wifi.try_recv(),
        Ok(wifi::Command::Connect { key }) if key == "Harbour Café"
    ));
}

/// What NetworkManager reports shows up: the network we are on, and the others in reach.
fn wifi_state_shown() {
    let mut h = dashboard();
    h.send(wifi::Event::State(Box::new(Demo::new().wifi)));
    let w = h.ui().global::<niri_dash::Wifi>();
    assert_eq!(w.get_ssid(), "Lighthouse");
    assert!(w.get_enabled());
    assert_eq!(w.get_networks().row_count(), 3);
}

/// A password prompt appears, and what is typed into it goes back with its serial.
fn wifi_password_prompt() {
    let mut h = dashboard();
    let mut state = Demo::new().wifi;
    let mut prompt = wifi::Prompt::default();
    prompt.serial = 7;
    prompt.ssid = "Lighthouse".into();
    state.prompt = Some(prompt);
    h.send(wifi::Event::State(Box::new(state)));
    let w = h.ui().global::<niri_dash::Wifi>();
    assert_eq!(w.get_prompt().serial, 7);
    assert_eq!(w.get_prompt().ssid, "Lighthouse");
    w.invoke_answer(7, "".into(), "hunter2".into(), 0, "".into(), false);
    assert!(matches!(
        h.inbox.wifi.try_recv(),
        Ok(wifi::Command::Answer { serial: 7, answer }) if answer.password == "hunter2"
    ));
}

/// A pairing PIN is answered with the code typed; saying no declines.
fn bluetooth_pairing() {
    let mut h = dashboard();
    let mut state = Demo::new().bluetooth;
    state.prompt = Some(bluetooth::Prompt {
        serial: 3,
        name: "MX Keys".into(),
        address: "F4:73:35:44:55:66".into(),
        kind: bluetooth::PromptKind::EnterPin,
    });
    h.send(bluetooth::Event::State(Box::new(state)));
    let b = h.ui().global::<niri_dash::Bluetooth>();
    assert_eq!(b.get_prompt().serial, 3);
    b.invoke_answer(3, true, "0000".into());
    b.invoke_answer(3, false, "".into());
    assert!(matches!(
        h.inbox.bluetooth.try_recv(),
        Ok(bluetooth::Command::Answer {
            serial: 3,
            answer: bluetooth::Answer::Code(code),
        }) if code == "0000"
    ));
    assert!(matches!(
        h.inbox.bluetooth.try_recv(),
        Ok(bluetooth::Command::Decline)
    ));
}

/// A dragged slider keeps its value against a level the backlight reports late.
fn brightness_drag_outlasts_stale_level() {
    let mut h = dashboard();
    let b = h.ui().global::<niri_dash::Brightness>();
    b.invoke_set(0.3);
    assert!(matches!(
        h.inbox.brightness.try_recv(),
        Ok(brightness::Command::Set(v)) if v == 0.3
    ));
    h.send(brightness::Event::Level(Some(0.9)));
    let b = h.ui().global::<niri_dash::Brightness>();
    assert!(b.get_available());
    assert_eq!(b.get_level(), 0.3);
}

/// Locking hides the dashboard, then locks.
fn session_lock_hides_first() {
    let mut h = dashboard();
    h.ui().global::<niri_dash::Session>().invoke_lock();
    assert_eq!(h.controls(), [Control::Hide]);
    assert!(matches!(
        h.inbox.session.try_recv(),
        Ok(session::Command::Lock)
    ));
}

/// Clicking a tray item activates it where it was clicked, and gets out of the way.
fn tray_activate() {
    let mut h = dashboard();
    h.send(tray::Event::Items(Demo::new().tray));
    let address = ":1.40/StatusNotifierItem";
    h.ui()
        .global::<niri_dash::Tray>()
        .invoke_activate(address.into(), 10.0, 20.0);
    assert!(matches!(
        h.inbox.tray.try_recv(),
        Ok(tray::Command::Activate { address: a, x: 10, y: 20 }) if a == address
    ));
    assert_eq!(h.controls(), [Control::Hide]);
}

/// Picking a color first hides the dashboard and only picks once it is gone (the surface, which
/// isn't there in a test, says when); the picked color comes back, is kept and copied, and the
/// dashboard returns.
fn color_pick() {
    let mut h = dashboard();
    h.ui().global::<niri_dash::ColorPicker>().invoke_pick();
    assert_eq!(h.controls(), [Control::Hide]);
    assert!(
        h.inbox.color.try_recv().is_err(),
        "picked while still shown"
    );
    h.send(color::Event::Picked(Some([1.0, 0.5, 0.0])));
    assert_eq!(h.controls(), [Control::Show]);
    let colors = h.ui().global::<niri_dash::ColorPicker>().get_colors();
    assert_eq!(colors.row_count(), 1);
    assert!(matches!(
        h.inbox.clipboard.try_recv(),
        Ok(niri_dash_system::clipboard::Command::Set(_))
    ));
}

fn copied(text: &str) -> clipboard::Event {
    clipboard::Event::Copied(clipboard::Clip {
        kind: clipboard::ClipKind::Text,
        data: Arc::new(text.as_bytes().to_vec()),
    })
}

fn clips(h: &Harness) -> usize {
    h.ui()
        .global::<niri_dash::Clipboard>()
        .get_items()
        .row_count()
}

/// Out of the box nothing is recorded: the system isn't asked to, and a stray copy isn't kept.
fn clipboard_history_off_by_default() {
    let mut h = testing::Harness::new(&testing::default_config());
    assert!(h.inbox.clipboard.try_recv().is_err());
    h.send(copied("hunter2"));
    assert_eq!(clips(&h), 0);
}

/// A clipboard widget turns the recording on; removing it turns it off and forgets the history,
/// and adding one turns it back on.
fn clipboard_history_follows_the_widget() {
    let mut h = testing::Harness::new("layout {\n  clipboard col=0 row=0 width=4 height=4\n}");
    assert!(matches!(
        h.inbox.clipboard.try_recv(),
        Ok(clipboard::Command::Record(true))
    ));
    h.send(copied("kept"));
    assert_eq!(clips(&h), 1);

    let id = h.ui().get_widgets().row_data(0).unwrap().id;
    h.ui().invoke_remove_widget(id);
    h.pump();
    assert!(matches!(
        h.inbox.clipboard.try_recv(),
        Ok(clipboard::Command::Record(false))
    ));
    assert_eq!(clips(&h), 0, "the history outlived its widget");
    h.send(copied("read as the widget went"));
    assert_eq!(clips(&h), 0);

    h.ui()
        .invoke_add_widget_at(testing::ui_kind("clipboard"), 0, 0);
    h.pump();
    assert!(matches!(
        h.inbox.clipboard.try_recv(),
        Ok(clipboard::Command::Record(true))
    ));
}

/// The element labelled `label` with this role, as a screen reader would find it.
fn element(h: &Harness, label: &str, role: &str) -> Element {
    h.element(label, role)
        .unwrap_or_else(|| panic!("nothing labelled {label:?} with the role {role:?}"))
}

/// Enter the widget with this title by its hint label.
fn enter(h: &mut Harness, title: &str) {
    let widgets = h.ui().get_widgets();
    let hint = widgets
        .iter()
        .find(|w| w.title == title)
        .unwrap_or_else(|| panic!("no widget titled {title:?}"))
        .hint;
    h.press("f");
    for c in hint.chars() {
        h.press(&c.to_string());
    }
}

/// Move the cursor down to the stop whose spoken name has `name` in it.
fn walk_to(h: &mut Harness, name: &str) {
    for _ in 0..30 {
        let said = h.spoken().expect("a cursor that adds up");
        if said.is_some_and(|s| s.contains(name)) {
            return;
        }
        h.press("j");
    }
    panic!("never got to {name:?}");
}

/// A network is connected to through its row, and the row says how it stands.
fn a11y_wifi_rows() {
    let mut h = dashboard();
    h.send(wifi::Event::State(Box::new(Demo::new().wifi)));
    let on = element(&h, "Lighthouse", "listitem");
    assert!(on.spoken().value.contains("connected"));
    element(&h, "Harbour Café", "listitem").activate();
    // A network's key is its SSID after a letter for its kind of security: o for open.
    assert!(matches!(
        h.inbox.wifi.try_recv(),
        Ok(wifi::Command::Connect { key }) if key == "oHarbour Café"
    ));
}

/// A switch says whether it is on, and its action flips it.
fn a11y_wifi_switch() {
    let mut h = dashboard();
    h.send(wifi::Event::State(Box::new(Demo::new().wifi)));
    let switch = element(&h, "Wi-Fi", "switch");
    assert_eq!(switch.spoken().checked, Some(true));
    switch.activate();
    assert!(matches!(h.inbox.wifi.try_recv(), Ok(wifi::Command::Toggle)));
}

/// A slider reads out its value, and can be stepped without a pointer.
fn a11y_brightness_knob() {
    let mut h = dashboard();
    h.send(brightness::Event::Level(Some(0.7)));
    let knob = element(&h, "Brightness", "slider");
    assert_eq!(knob.spoken().value, "70%");
    knob.increment();
    assert!(matches!(
        h.inbox.brightness.try_recv(),
        Ok(brightness::Command::Set(v)) if (v - 0.75).abs() < 0.001
    ));
}

/// A button's action does what a click does: locking hides the dashboard first.
fn a11y_lock_button() {
    let mut h = dashboard();
    element(&h, "Lock", "button").activate();
    assert_eq!(h.controls(), [Control::Hide]);
    assert!(matches!(
        h.inbox.session.try_recv(),
        Ok(session::Command::Lock)
    ));
}

/// A typed password is in no label or value a screen reader could read out, and yet is what gets
/// answered.
fn a11y_password_stays_secret() {
    let mut h = dashboard();
    let mut state = Demo::new().wifi;
    let mut prompt = wifi::Prompt::default();
    prompt.serial = 7;
    prompt.ssid = "Lighthouse".into();
    state.prompt = Some(prompt);
    h.send(wifi::Event::State(Box::new(state)));
    // The prompt takes the keyboard as it appears: typing goes straight in.
    h.settle(5);
    for c in "hunter2".chars() {
        h.press(&c.to_string());
    }
    let readable = h.readable();
    assert!(
        readable.iter().any(|t| t == "Password"),
        "the field isn't there to read: {readable:?}"
    );
    if let Some(t) = readable.iter().find(|t| t.contains("hunter2")) {
        panic!("a screen reader could read out the password: {t}");
    }
    h.press("enter");
    assert!(matches!(
        h.inbox.wifi.try_recv(),
        Ok(wifi::Command::Answer { serial: 7, answer }) if answer.password == "hunter2"
    ));
}

/// Joining from a text field closes the form and the field with it. The keyboard comes back: the
/// keys still work, and Escape still gets out.
fn keyboard_survives_its_field_going_away() {
    let mut h = dashboard();
    h.send(wifi::Event::State(Box::new(Demo::new().wifi)));
    enter(&mut h, "Wi-Fi");
    walk_to(&mut h, "Hidden network");
    h.press("enter");
    for c in "Attic".chars() {
        h.press(&c.to_string());
    }
    // Enter joins from the password field, not from the name.
    h.press("tab");
    h.press("enter");
    h.settle(20);
    assert!(matches!(
        h.inbox.wifi.try_recv(),
        Ok(wifi::Command::JoinHidden { ssid, .. }) if ssid == "Attic"
    ));
    let said = h
        .spoken()
        .expect("a cursor that adds up")
        .unwrap_or_default();
    assert!(said.contains("Lighthouse"), "back on the list, but: {said}");
    h.press("esc");
    assert_eq!(h.spoken().map(|s| s.is_none()), Ok(true), "left the widget");
    h.press("esc");
    assert_eq!(h.controls(), [Control::Hide], "closed");
}

/// In the volume widget j is quieter and k louder, on the slider under the cursor; Tab goes to
/// the next slider.
fn volume_keys() {
    let mut h = testing::Harness::new(
        "grid columns=16 rows=9\nlayout {\n    volume col=0 row=0 width=4 height=4\n}\n",
    );
    let audio = |h: &Harness| {
        let a = h.ui().global::<niri_dash::Audio>();
        (a.get_volume(), a.get_mic_volume())
    };
    h.ui().global::<niri_dash::Audio>().set_volume(0.5);
    h.ui().global::<niri_dash::Audio>().set_mic_volume(0.5);
    h.key("f");
    let label = h.ui().get_widgets().row_data(0).unwrap().hint;
    for c in label.chars() {
        h.key(c.to_string());
    }
    let near = |a: f32, b: f32| (a - b).abs() < 0.001;
    h.key("j");
    assert!(near(audio(&h).0, 0.45), "j: {:?}", audio(&h));
    h.key("k");
    h.key("k");
    assert!(near(audio(&h).0, 0.55), "k: {:?}", audio(&h));
    h.special(slint::platform::Key::Tab);
    h.key("j");
    assert!(near(audio(&h).0, 0.55), "Tab then j: {:?}", audio(&h));
    assert!(near(audio(&h).1, 0.45), "Tab then j: {:?}", audio(&h));
}

/// A list growing above the row the cursor is on scrolls, so the cursor stays in sight.
fn cursor_stays_in_sight_as_a_list_grows() {
    let mut h = dashboard();
    let mut state = Demo::new().bluetooth;
    h.send(bluetooth::Event::State(Box::new(state.clone())));
    enter(&mut h, "Bluetooth");
    h.press("G");
    h.settle(20);
    h.render();
    let said = h
        .spoken()
        .expect("a cursor that adds up")
        .unwrap_or_default();
    assert!(
        said.contains("Add a device"),
        "on the last row, but: {said}"
    );
    state.discovering = true;
    h.send(bluetooth::Event::State(Box::new(state)));
    // As on screen: a frame, and what it sets off.
    for _ in 0..3 {
        h.render();
        h.settle(5);
    }
    let said = h
        .spoken()
        .expect("the cursor out of sight")
        .unwrap_or_default();
    assert!(
        said.contains("Stop looking"),
        "on the same row, but: {said}"
    );
}

fn pm(h: &Harness) -> niri_dash::Pomodoro<'_> {
    h.ui().global::<niri_dash::Pomodoro>()
}

/// The pomodoro starts, pauses, skips and starts over from the keyboard; a skipped focus session
/// is no session done.
fn pomodoro_keys() {
    let mut h = testing::Harness::new(
        "grid columns=16 rows=9\nlayout {\n    pomodoro col=0 row=0 width=4 height=3 shape=\"rect\"\n}\n",
    );
    assert_eq!(pm(&h).get_text(), "25:00");
    assert_eq!(pm(&h).get_label(), "Focus");
    assert_eq!((pm(&h).get_done(), pm(&h).get_rounds()), (0, 4));
    enter(&mut h, "Pomodoro");
    h.key(" ");
    assert!(pm(&h).get_running());
    h.key(" ");
    assert!(pm(&h).get_paused() && !pm(&h).get_running());
    // n: the next part, stopped, and not counted.
    h.key("n");
    assert_eq!(pm(&h).get_label(), "Short break");
    assert!(pm(&h).get_on_break() && !pm(&h).get_paused() && !pm(&h).get_running());
    assert_eq!(pm(&h).get_text(), "5:00");
    assert_eq!(pm(&h).get_next(), "Next: focus, 25 min");
    h.key("n");
    assert_eq!(pm(&h).get_label(), "Focus");
    assert_eq!(pm(&h).get_done(), 0);
    h.key(" ");
    assert!(pm(&h).get_running());
    h.key("r");
    assert!(!pm(&h).get_running() && !pm(&h).get_paused());
    assert_eq!(pm(&h).get_text(), "25:00");
}

/// The widget's settings are the plan.
fn pomodoro_follows_its_settings() {
    let h = testing::Harness::new(
        "grid columns=16 rows=9\nlayout {\n    pomodoro col=0 row=0 width=4 height=3 shape=\"rect\" {\n        focus-minutes 50\n        short-break 10\n        rounds 2\n    }\n}\n",
    );
    assert_eq!(pm(&h).get_text(), "50:00");
    assert_eq!(pm(&h).get_rounds(), 2);
    assert_eq!(pm(&h).get_next(), "Next: short break, 10 min");
}

fn sl(h: &Harness) -> niri_dash::Sleep<'_> {
    h.ui().global::<niri_dash::Sleep>()
}

/// The sleep timer asks which modes the machine offers, and arms, pauses, nudges and gives up on
/// a countdown from the keyboard, as sleepy's window does.
fn sleep_timer_keys() {
    let mut h = testing::Harness::new(
        "grid columns=16 rows=9\nlayout {\n    sleep-timer col=0 row=0 width=4 height=3\n}\n",
    );
    assert!(matches!(
        h.inbox.sleep.try_recv(),
        Ok(sleep::Command::Detect { script }) if script.ends_with("niri-dash/sleep.sh")
    ));
    h.send(sleep::Event::Modes(vec![
        sleep::Mode::Suspend,
        sleep::Mode::Hibernate,
    ]));
    let status = |h: &Harness| sl(h).get_status();
    let headline = |h: &Harness| sl(h).get_headline().to_string();
    assert_eq!(sl(&h).get_modes().row_count(), 2);
    assert_eq!(sl(&h).get_echo(), "→ in 30m");
    // A field that is no when says why, and refuses to arm.
    sl(&h).invoke_edited("soon".into());
    assert_eq!(
        sl(&h).get_problem(),
        "not a duration; try 45s, 30 min, 1h30m"
    );
    sl(&h).invoke_arm();
    assert_eq!(status(&h), niri_dash::SleepStatus::Idle);
    sl(&h).invoke_edited("90 min".into());
    assert_eq!(sl(&h).get_echo(), "→ in 1h 30m");
    enter(&mut h, "Sleep timer");
    // d: the dry run; a: the screen stays on.
    h.key("d");
    assert!(sl(&h).get_dry_run());
    h.key("a");
    assert!(sl(&h).get_keep_awake());
    // j to the mode, l to the next; Enter arms with it.
    h.key("j");
    h.key("l");
    assert_eq!(sl(&h).get_selected(), 1);
    h.special(slint::platform::Key::Return);
    assert_eq!(status(&h), niri_dash::SleepStatus::Armed);
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
    assert_eq!(status(&h), niri_dash::SleepStatus::Paused);
    assert_eq!(headline(&h), "paused, 1h 30m left");
    h.key(" ");
    assert_eq!(status(&h), niri_dash::SleepStatus::Armed);
    // Escape saves the machine, and lets go of the screen.
    h.special(slint::platform::Key::Escape);
    assert_eq!(status(&h), niri_dash::SleepStatus::Idle);
    assert!(matches!(
        h.inbox.sleep.try_recv(),
        Ok(sleep::Command::KeepAwake(false))
    ));
    assert!(h.inbox.sleep.try_recv().is_err(), "nothing was run");
}

/// Tall enough, the modes are a list, and the highlight follows the cursor down it.
fn sleep_timer_lists_modes() {
    let mut h = testing::Harness::new(
        "grid columns=16 rows=9\nlayout {\n    sleep-timer col=0 row=0 width=4 height=5\n}\n",
    );
    h.send(sleep::Event::Modes(vec![
        sleep::Mode::Suspend,
        sleep::Mode::Hibernate,
        sleep::Mode::Poweroff,
    ]));
    enter(&mut h, "Sleep timer");
    h.key("j");
    assert_eq!(sl(&h).get_selected(), 0);
    h.key("j");
    h.key("j");
    assert_eq!(sl(&h).get_selected(), 2);
    h.key("k");
    assert_eq!(sl(&h).get_selected(), 1);
    h.special(slint::platform::Key::Return);
    assert_eq!(sl(&h).get_status(), niri_dash::SleepStatus::Armed);
    assert!(sl(&h).get_headline().starts_with("hibernate in 30m"));
}

/// A countdown that runs out starts the mode once, or for a dry run says what it would have done.
fn sleep_timer_fires() {
    let mut h = testing::Harness::new(
        "grid columns=16 rows=9\nlayout {\n    sleep-timer col=0 row=0 width=4 height=4\n}\n",
    );
    h.send(sleep::Event::Modes(vec![sleep::Mode::Suspend]));
    sl(&h).invoke_edited("1s".into());
    sl(&h).invoke_arm();
    let mut asked = None;
    // The countdown is counted on real time, a quarter second at a time.
    for _ in 0..40 {
        h.settle(100);
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
    assert_eq!(sl(&h).get_status(), niri_dash::SleepStatus::Fired);
    assert_eq!(sl(&h).get_headline(), "time! suspend the machine…");
    // The system's answer, when it fails, is the line under the headline.
    h.send(sleep::Event::Done {
        mode: sleep::Mode::Suspend,
        error: Some("suspend refused: busy".into()),
    });
    assert_eq!(sl(&h).get_outcome(), "suspend refused: busy");
    // A dry run does nothing but say so.
    sl(&h).invoke_abort();
    sl(&h).invoke_toggle_dry_run();
    sl(&h).invoke_arm();
    for _ in 0..40 {
        h.settle(100);
        if sl(&h).get_status() == niri_dash::SleepStatus::Fired {
            break;
        }
    }
    assert_eq!(
        sl(&h).get_outcome(),
        "dry run: would have run systemctl suspend"
    );
    while let Ok(c) = h.inbox.sleep.try_recv() {
        assert!(!matches!(c, sleep::Command::Run { .. }), "a dry run ran it");
    }
}

fn main() {
    // The dashboard's config, state and notes go somewhere disposable; dates in a fixed locale.
    let dir = std::env::temp_dir().join(format!("niri-dash-ui-{}", std::process::id()));
    // SAFETY: nothing else runs yet that could read the environment.
    unsafe {
        std::env::set_var("NIRI_DASH_HOME", &dir);
        for (var, sub) in [
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_DATA_HOME", "data"),
            ("XDG_CACHE_HOME", "cache"),
        ] {
            std::env::set_var(var, dir.join(sub));
        }
        std::env::set_var("LC_ALL", "C");
    }
    let scenarios: &[(&str, fn())] = &[
        ("wifi: commands", wifi_commands),
        ("wifi: state shown", wifi_state_shown),
        ("wifi: password prompt", wifi_password_prompt),
        ("bluetooth: pairing", bluetooth_pairing),
        (
            "brightness: drag outlasts a stale level",
            brightness_drag_outlasts_stale_level,
        ),
        ("session: lock hides first", session_lock_hides_first),
        ("tray: activate", tray_activate),
        ("color picker: pick", color_pick),
        ("volume: j and k", volume_keys),
        (
            "clipboard history: off by default",
            clipboard_history_off_by_default,
        ),
        (
            "clipboard history: follows the widget",
            clipboard_history_follows_the_widget,
        ),
        ("a11y: Wi-Fi rows", a11y_wifi_rows),
        ("a11y: Wi-Fi switch", a11y_wifi_switch),
        ("a11y: brightness knob", a11y_brightness_knob),
        ("a11y: lock button", a11y_lock_button),
        ("a11y: password stays secret", a11y_password_stays_secret),
        (
            "cursor stays in sight as a list grows",
            cursor_stays_in_sight_as_a_list_grows,
        ),
        (
            "keyboard survives its field going away",
            keyboard_survives_its_field_going_away,
        ),
        ("sleep timer: keys", sleep_timer_keys),
        ("sleep timer: lists modes", sleep_timer_lists_modes),
        ("sleep timer: fires", sleep_timer_fires),
        ("pomodoro: keys", pomodoro_keys),
        (
            "pomodoro: follows its settings",
            pomodoro_follows_its_settings,
        ),
    ];
    let mut failed = Vec::new();
    for (name, scenario) in scenarios {
        let ok = std::panic::catch_unwind(AssertUnwindSafe(scenario)).is_ok();
        println!("test {name} ... {}", if ok { "ok" } else { "FAILED" });
        if !ok {
            failed.push(*name);
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    println!(
        "\ntest result: {}. {} passed; {} failed",
        if failed.is_empty() { "ok" } else { "FAILED" },
        scenarios.len() - failed.len(),
        failed.len()
    );
    if !failed.is_empty() {
        std::process::exit(1);
    }
}

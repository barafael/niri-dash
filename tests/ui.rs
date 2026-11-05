//! The dashboard against channels standing in for the system: what it sends when it is used, and
//! what it shows when the system reports something. No Wayland, no D-Bus.
//!
//! Slint wants one thread and one platform per process, so this is a plain program
//! (`harness = false`) that runs each scenario in turn on a fresh dashboard.

use niri_dash::testing::{Element, Harness};
use niri_dash::{Control, testing};
use niri_dash_system::{
    Demo, bluetooth, brightness, clipboard, color, privacy, session, tray, wifi, windows,
};
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

/// What a frame would show: laid out, and what that sets off (see `cursor_stays_in_sight…`).
fn shown(h: &mut Harness) {
    for _ in 0..2 {
        h.render();
        h.settle(5);
    }
}

/// What a screen reader says now; the check's complaint fails the test.
fn said(h: &mut Harness) -> String {
    shown(h);
    h.spoken()
        .expect("a cursor that adds up")
        .unwrap_or_default()
}

/// A network dropping off while the cursor is on its details button leaves the cursor on its row.
fn sub_part_gone_goes_back_to_the_row() {
    let mut h = dashboard();
    let mut state = Demo::new().wifi;
    h.send(wifi::Event::State(Box::new(state.clone())));
    enter(&mut h, "Wi-Fi");
    h.press("l");
    h.press("l");
    assert!(said(&mut h).contains("Details of Lighthouse"));
    state.networks[0].active = false;
    h.send(wifi::Event::State(Box::new(state)));
    let now = said(&mut h);
    assert!(now.starts_with("listitem \"Lighthouse\""), "on: {now}");
}

/// A network coming up while the cursor is on its details button must not turn that into
/// Disconnect, under a key press meant for something else.
fn sub_part_changing_meaning_goes_back_to_the_row() {
    let mut h = dashboard();
    let mut state = Demo::new().wifi;
    state.networks[0].active = false;
    h.send(wifi::Event::State(Box::new(state.clone())));
    enter(&mut h, "Wi-Fi");
    h.press("l");
    assert!(said(&mut h).contains("Details of Lighthouse"));
    state.networks[0].active = true;
    h.send(wifi::Event::State(Box::new(state)));
    let now = said(&mut h);
    assert!(now.starts_with("listitem \"Lighthouse\""), "on: {now}");
}

/// A tall Wi-Fi tile, so a form fits and only the stops are being tested.
fn tall_wifi() -> Harness {
    testing::Harness::new(
        "grid columns=16 rows=9\nlayout {\n    wifi col=0 row=0 width=4 height=7\n}\n",
    )
}

/// A new Enterprise network's prompt on an entered Wi-Fi widget.
fn enterprise_prompt(h: &mut Harness) {
    h.send(wifi::Event::State(Box::new(Demo::new().wifi)));
    enter(h, "Wi-Fi");
    let mut state = Demo::new().wifi;
    let mut prompt = wifi::Prompt::default();
    prompt.serial = 9;
    prompt.ssid = "Campus".into();
    prompt.kind = wifi::PromptKind::Enterprise;
    prompt.new = true;
    state.prompt = Some(prompt);
    h.send(wifi::Event::State(Box::new(state)));
    shown(h);
}

/// The hidden-network form, on the 802.1X choice with the server check, with a name typed.
fn hidden_enterprise_form(h: &mut Harness) {
    h.send(wifi::Event::State(Box::new(Demo::new().wifi)));
    enter(h, "Wi-Fi");
    walk_to(h, "Hidden network");
    h.press("enter");
    shown(h);
    for c in "Attic".chars() {
        h.press(&c.to_string());
    }
    h.press("esc");
    h.press("j");
    h.press("l");
    shown(h);
}

/// Turning the server check off keeps the cursor on the switch: pressed again, it can't cancel.
fn server_check_keeps_the_cursor_in_the_prompt() {
    let mut h = tall_wifi();
    enterprise_prompt(&mut h);
    h.press("esc");
    walk_to(&mut h, "Don't check");
    h.press("enter");
    let now = said(&mut h);
    assert!(now.contains("Don't check"), "on: {now}");
    h.press("enter");
    assert!(
        !matches!(h.inbox.wifi.try_recv(), Ok(wifi::Command::CancelPrompt)),
        "cancelled the prompt"
    );
}

/// Turning the server check off keeps the cursor on the switch: pressed again, it can't join a
/// network with the check off.
fn server_check_keeps_the_cursor_in_the_form() {
    let mut h = tall_wifi();
    hidden_enterprise_form(&mut h);
    walk_to(&mut h, "Don't check");
    h.press("enter");
    let now = said(&mut h);
    assert!(now.contains("Don't check"), "on: {now}");
    h.press("enter");
    assert!(
        !matches!(
            h.inbox.wifi.try_recv(),
            Ok(wifi::Command::JoinHidden { .. })
        ),
        "joined with the check off"
    );
}

/// The cursor says the field the keys go to: from the start, and after Tab.
fn cursor_follows_the_text_field() {
    let mut h = tall_wifi();
    enterprise_prompt(&mut h);
    let now = said(&mut h);
    assert!(now.contains("User name"), "typing into the name, on: {now}");
    h.press("tab");
    let now = said(&mut h);
    assert!(now.contains("Password"), "typing the password, on: {now}");
}

/// A window moving to another workspace can put a heading where the cursor is.
fn cursor_leaves_a_heading_that_moved_under_it() {
    let mut h = testing::Harness::new("layout {\n    windows col=0 row=0 width=4 height=4\n}\n");
    let mut state = Demo::new().windows;
    h.send(windows::Event::State(state.clone()));
    enter(&mut h, "Windows");
    h.press("j");
    assert!(said(&mut h).contains("Mozilla Firefox"));
    state.rows.swap(2, 3);
    h.send(windows::Event::State(state));
    let now = said(&mut h);
    assert!(now.starts_with("listitem"), "on: {now}");
}

/// `j` gets past however many stops in a row can't be stopped at.
fn cursor_crosses_many_skipped_stops() {
    let mut h =
        testing::Harness::new("layout {\n    claude-sessions col=0 row=0 width=4 height=7\n}\n");
    let mut state = Demo::new().claude;
    let session = state.sessions[0].clone();
    state.sessions = (0..8)
        .map(|i| niri_dash_system::claude::Session {
            pid: i,
            title: format!("Session {i}"),
            window: (i == 0 || i == 7).then_some(1),
            ..session.clone()
        })
        .collect();
    h.send(niri_dash_system::claude::Event::Sessions(state));
    enter(&mut h, "Claude sessions");
    assert!(said(&mut h).contains("Session 0"));
    h.press("j");
    let now = said(&mut h);
    assert!(now.contains("Session 7"), "on: {now}");
}

/// Every stop from here to the last has to be there to read, and in sight.
fn every_stop_in_sight(h: &mut Harness) {
    let mut last = said(h);
    for _ in 0..60 {
        h.press("j");
        let now = said(h);
        if now == last {
            return;
        }
        last = now;
    }
    panic!("never reached the last stop");
}

fn cast(i: u64) -> privacy::ScreenCast {
    privacy::ScreenCast {
        session_id: i,
        target: format!("Output {i}"),
        active: true,
        pid: None,
        process: Some(format!("App {i}")),
        pw_node: None,
    }
}

/// A short tile: every stop of the hidden-network form scrolls into sight.
fn stops_in_sight_hidden_form() {
    let mut h = testing::Harness::new("layout {\n    wifi col=0 row=0 width=3 height=2\n}\n");
    hidden_enterprise_form(&mut h);
    every_stop_in_sight(&mut h);
}

/// A short tile: the details page of a saved network.
fn stops_in_sight_wifi_details() {
    let mut h = testing::Harness::new("layout {\n    wifi col=0 row=0 width=3 height=2\n}\n");
    let mut state = Demo::new().wifi;
    h.send(wifi::Event::State(Box::new(state.clone())));
    enter(&mut h, "Wi-Fi");
    h.press("l");
    h.press("l");
    h.press("enter");
    state.detail = Some(wifi::Detail {
        key: state.networks[0].key(),
        ssid: "Lighthouse".into(),
        network: Some(state.networks[0].clone()),
        profile: Some(wifi::Profile {
            ssid: "Lighthouse".into(),
            ..Default::default()
        }),
    });
    h.send(wifi::Event::State(Box::new(state)));
    every_stop_in_sight(&mut h);
}

/// A short tile: the password and QR code appearing above Share push it down; it stays in sight.
fn stops_in_sight_wifi_share() {
    let mut h = testing::Harness::new("layout {\n    wifi col=0 row=0 width=3 height=2\n}\n");
    let mut state = Demo::new().wifi;
    h.send(wifi::Event::State(Box::new(state.clone())));
    enter(&mut h, "Wi-Fi");
    h.press("l");
    h.press("l");
    h.press("enter");
    state.detail = Some(wifi::Detail {
        key: state.networks[0].key(),
        ssid: "Lighthouse".into(),
        network: Some(state.networks[0].clone()),
        profile: Some(wifi::Profile {
            ssid: "Lighthouse".into(),
            uuid: "u".into(),
            ..Default::default()
        }),
    });
    h.send(wifi::Event::State(Box::new(state.clone())));
    for _ in 0..8 {
        if said(&mut h).contains("Share network") {
            break;
        }
        h.press("j");
    }
    assert!(said(&mut h).contains("Share network"));
    state.secret = Some(wifi::Secret {
        uuid: "u".into(),
        password: "hunter2".into(),
        qr: niri_dash_system::IconSrc::None,
    });
    h.send(wifi::Event::State(Box::new(state)));
    let now = said(&mut h);
    assert!(now.contains("Hide password"), "on: {now}");
}

/// A short tile: Bluetooth's details page.
fn stops_in_sight_bluetooth_details() {
    let mut h = testing::Harness::new("layout {\n    bluetooth col=0 row=0 width=3 height=2\n}\n");
    let mut state = Demo::new().bluetooth;
    h.send(bluetooth::Event::State(Box::new(state.clone())));
    enter(&mut h, "Bluetooth");
    h.press("l");
    h.press("enter");
    state.detail = Some(state.devices[0].clone());
    h.send(bluetooth::Event::State(Box::new(state)));
    every_stop_in_sight(&mut h);
}

/// A short tile: many screen shares, and the Stop button of each comes into sight.
fn stops_in_sight_privacy() {
    let mut h = testing::Harness::new("layout {\n    privacy col=0 row=0 width=3 height=2\n}\n");
    h.send(privacy::Event::Casts((0..8).map(cast).collect()));
    enter(&mut h, "Privacy");
    every_stop_in_sight(&mut h);
}

/// Short tiles: lists that wrap their rows, or aren't rows at all.
fn stops_in_sight_other_lists() {
    let mut h = testing::Harness::new("layout {\n    windows col=0 row=0 width=3 height=2\n}\n");
    let mut w = Demo::new().windows;
    let row = w.rows[1].clone();
    w.rows = (0..12)
        .map(|i| windows::Row {
            id: i,
            title: format!("Window {i}"),
            ..row.clone()
        })
        .collect();
    h.send(windows::Event::State(w));
    enter(&mut h, "Windows");
    every_stop_in_sight(&mut h);

    let mut h =
        testing::Harness::new("layout {\n    claude-sessions col=0 row=0 width=3 height=2\n}\n");
    let mut c = Demo::new().claude;
    let session = c.sessions[0].clone();
    c.sessions = (0..12)
        .map(|i| niri_dash_system::claude::Session {
            pid: i,
            title: format!("Session {i}"),
            window: Some(1),
            ..session.clone()
        })
        .collect();
    h.send(niri_dash_system::claude::Event::Sessions(c));
    enter(&mut h, "Claude sessions");
    every_stop_in_sight(&mut h);

    let mut h =
        testing::Harness::new("layout {\n    notifications col=0 row=0 width=3 height=2\n}\n");
    let mut n = Demo::new().notifications;
    let one = n.items[0].clone();
    n.items = (0..10)
        .map(|i| niri_dash_system::notifications::Notification {
            id: i,
            summary: format!("Note {i}"),
            ..one.clone()
        })
        .collect();
    h.send(niri_dash_system::notifications::Event::State(n));
    enter(&mut h, "Notifications");
    every_stop_in_sight(&mut h);

    let mut h = testing::Harness::new("layout {\n    volume col=0 row=0 width=3 height=2\n}\n");
    h.send(niri_dash_system::audio::Event::State(Demo::new().audio));
    enter(&mut h, "Volume");
    every_stop_in_sight(&mut h);
}

/// Only mic and camera apps: nothing to stop, so no stops; the list itself is what's read out.
fn privacy_without_a_share_has_no_stops() {
    let mut h = testing::Harness::new("layout {\n    privacy col=0 row=0 width=3 height=3\n}\n");
    h.send(privacy::Event::Usage(privacy::Usage {
        mic: vec!["Firefox".into()],
        ..Default::default()
    }));
    enter(&mut h, "Privacy");
    let now = said(&mut h);
    assert!(now.starts_with("list \"Privacy\""), "on: {now}");
}

/// Five mic apps above a screen share: the cursor starts on its Stop button.
fn privacy_stops_are_the_shares() {
    let mut h = testing::Harness::new("layout {\n    privacy col=0 row=0 width=3 height=4\n}\n");
    h.send(privacy::Event::Usage(privacy::Usage {
        mic: (0..5).map(|i| format!("Mic {i}")).collect(),
        ..Default::default()
    }));
    h.send(privacy::Event::Casts(vec![cast(1)]));
    enter(&mut h, "Privacy");
    let now = said(&mut h);
    assert!(now.contains("Stop sharing with App 1"), "on: {now}");
}

fn main() {
    // The dashboard's config, state and notes go somewhere disposable; dates in a fixed locale.
    let dir = std::env::temp_dir().join(format!("niri-dash-ui-{}", std::process::id()));
    // SAFETY: nothing else runs yet that could read the environment.
    unsafe {
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
            "sub_part_gone_goes_back_to_the_row",
            sub_part_gone_goes_back_to_the_row,
        ),
        (
            "sub_part_changing_meaning_goes_back_to_the_row",
            sub_part_changing_meaning_goes_back_to_the_row,
        ),
        (
            "server_check_keeps_the_cursor_in_the_prompt",
            server_check_keeps_the_cursor_in_the_prompt,
        ),
        (
            "server_check_keeps_the_cursor_in_the_form",
            server_check_keeps_the_cursor_in_the_form,
        ),
        (
            "cursor_follows_the_text_field",
            cursor_follows_the_text_field,
        ),
        (
            "cursor_leaves_a_heading_that_moved_under_it",
            cursor_leaves_a_heading_that_moved_under_it,
        ),
        (
            "cursor_crosses_many_skipped_stops",
            cursor_crosses_many_skipped_stops,
        ),
        ("stops_in_sight_hidden_form", stops_in_sight_hidden_form),
        ("stops_in_sight_wifi_details", stops_in_sight_wifi_details),
        ("stops_in_sight_wifi_share", stops_in_sight_wifi_share),
        (
            "stops_in_sight_bluetooth_details",
            stops_in_sight_bluetooth_details,
        ),
        ("stops_in_sight_privacy", stops_in_sight_privacy),
        ("stops_in_sight_other_lists", stops_in_sight_other_lists),
        (
            "privacy_without_a_share_has_no_stops",
            privacy_without_a_share_has_no_stops,
        ),
        ("privacy_stops_are_the_shares", privacy_stops_are_the_shares),
        (
            "keyboard survives its field going away",
            keyboard_survives_its_field_going_away,
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

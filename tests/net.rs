//! The Wi-Fi and Bluetooth widgets against channels standing in for the system: what they send
//! when they are used, what they show when the system reports something, and the keyboard.
//! Port of the Wi-Fi and Bluetooth scenarios of `legacy/tests/ui.rs`.
//!
//! A plain program (`harness = false`): each scenario runs in turn on a fresh dashboard.
//! `cargo test --test net -- sub_part` runs the scenarios with that in their names.

use niri_dash::Control;
use niri_dash::testing::{Harness, Monkey, Sandbox};
use niri_dash_system::{Demo, bluetooth, wifi};
use std::panic::AssertUnwindSafe;

const LAYOUT: &str = r#"
layout {
    wifi          col=0 row=0 width=3 height=3
    bluetooth     col=3 row=0 width=3 height=2
    brightness    col=6 row=0 size=2 shape="round"
    tray          col=0 row=3 width=5 height=1
    session       col=0 row=4 width=4 height=1
}
"#;

/// The dashboard shown.
fn up(mut h: Harness) -> Harness {
    h.control(Control::Show);
    h
}

fn dashboard() -> Harness {
    up(Harness::new(LAYOUT))
}

/// A tall Wi-Fi tile, so a form fits and only the stops are being tested.
fn tall_wifi() -> Harness {
    up(Harness::new(
        "grid columns=16 rows=9\nlayout {\n    wifi col=0 row=0 width=4 height=7\n}\n",
    ))
}

fn tall_bt() -> Harness {
    up(Harness::new(
        "grid columns=16 rows=9\nlayout {\n    bluetooth col=0 row=0 width=4 height=7\n}\n",
    ))
}

/// A short tile: lists have to scroll to keep the cursor in sight.
fn short(kind: &str) -> Harness {
    up(Harness::new(&format!(
        "layout {{\n    {kind} col=0 row=0 width=3 height=2\n}}\n"
    )))
}

fn send_wifi(h: &mut Harness, state: &wifi::State) {
    h.send(wifi::Event::State(Box::new(state.clone())));
}

fn send_bt(h: &mut Harness, state: &bluetooth::State) {
    h.send(bluetooth::Event::State(Box::new(state.clone())));
}

fn next_wifi(h: &mut Harness) -> Option<wifi::Command> {
    h.inbox.wifi.try_recv().ok()
}

fn next_bt(h: &mut Harness) -> Option<bluetooth::Command> {
    h.inbox.bluetooth.try_recv().ok()
}

/// Enter the widget with this title by its hint label.
fn enter(h: &mut Harness, title: &str) {
    let hint = h
        .read(|d| {
            d.tiles
                .iter()
                .find(|t| t.kind().info().title == title)
                .map(|t| t.hint.clone())
        })
        .unwrap_or_else(|| panic!("no widget titled {title:?}"));
    h.key("f");
    for c in hint.chars() {
        h.key(&c.to_string());
    }
}

/// What a frame would show: laid out, and what that sets off.
fn shown(h: &mut Harness) {
    for _ in 0..3 {
        h.settle();
    }
}

/// What a screen reader says of the cursor now, `None` with no widget entered.
fn spoken(h: &mut Harness) -> Option<String> {
    shown(h);
    h.read(|d| d.widgets.net.cursor_says(d))
}

fn said(h: &mut Harness) -> String {
    spoken(h).unwrap_or_default()
}

/// Move the cursor down to the stop whose spoken name has `name` in it.
fn walk_to(h: &mut Harness, name: &str) {
    for _ in 0..30 {
        if said(h).contains(name) {
            return;
        }
        h.key("j");
    }
    panic!("never got to {name:?}, on: {}", said(h));
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn prompt_state(serial: u64, ssid: &str) -> wifi::State {
    let mut state = Demo::new().wifi;
    let mut prompt = wifi::Prompt::default();
    prompt.serial = serial;
    prompt.ssid = ssid.into();
    state.prompt = Some(prompt);
    state
}

/// The label of what `spoken` says (`listitem "Lighthouse" connected`).
fn label_of(said: &str) -> String {
    said.split('"').nth(1).unwrap_or_default().to_string()
}

const LISTS: [&str; 6] = [
    "net-list",
    "detail-list",
    "hidden-list",
    "saved-list",
    "device-list",
    "bt-detail-list",
];

/// What the cursor is on, and whether that is in sight of the list it is in: no more than a
/// quarter of it cut off. (The list is that of the page; the row has to be on screen.)
fn sight(h: &mut Harness) -> (String, bool) {
    let said = said(h);
    let label = label_of(&said);
    let row = h
        .bounds_of(leak(label))
        .unwrap_or_else(|| panic!("the stop {said} is not on screen"));
    let list = LISTS
        .iter()
        .find_map(|l| h.bounds_of(l))
        .expect("a list on screen");
    let (rt, rb) = (f32::from(row.top()), f32::from(row.bottom()));
    let (lt, lb) = (f32::from(list.top()), f32::from(list.bottom()));
    let shown = rb.min(lb) - rt.max(lt);
    let height = (rb - rt).min(lb - lt);
    (said, shown >= 0.75 * height)
}

/// The stop under the cursor is in sight.
fn in_sight(h: &mut Harness) -> String {
    let (said, ok) = sight(h);
    assert!(ok, "{said} is out of sight");
    said
}

/// Every stop from here to the last has to be there to read, and in sight.
fn every_stop_in_sight(h: &mut Harness) {
    let mut last = in_sight(h);
    for _ in 0..60 {
        h.key("j");
        let now = in_sight(h);
        if now == last {
            return;
        }
        last = now;
    }
    panic!("never reached the last stop");
}

// ---- what the widgets send and show ------------------------------------------------------------

/// Rescanning and connecting are commands to the Wi-Fi actor.
fn wifi_commands() {
    let mut h = dashboard();
    send_wifi(&mut h, &Demo::new().wifi);
    h.click_on("Rescan");
    h.click_on("Harbour Café");
    assert!(matches!(next_wifi(&mut h), Some(wifi::Command::Rescan)));
    assert!(matches!(
        next_wifi(&mut h),
        Some(wifi::Command::Connect { key }) if key == "oHarbour Café"
    ));
}

/// What NetworkManager reports shows up: the network we are on, and the others in reach.
fn wifi_state_shown() {
    let mut h = dashboard();
    send_wifi(&mut h, &Demo::new().wifi);
    h.read(|d| {
        let s = &d.widgets.net.wifi.state;
        assert_eq!(s.ssid, "Lighthouse");
        assert!(s.enabled);
        assert_eq!(s.networks.len(), 3);
    });
    for name in ["Lighthouse", "Harbour Café", "FRITZ!Box 7590 QX", "Hidden network"] {
        assert!(h.has(leak(name.into())), "no row {name:?}");
    }
}

/// A password prompt appears, and what is typed into it goes back with its serial.
fn wifi_password_prompt() {
    let mut h = dashboard();
    send_wifi(&mut h, &prompt_state(7, "Lighthouse"));
    shown(&mut h);
    // The prompt takes the keyboard as it appears: typing goes straight in.
    h.type_text("hunter2");
    h.click_on("Join");
    assert!(matches!(
        next_wifi(&mut h),
        Some(wifi::Command::Answer { serial: 7, answer }) if answer.password == "hunter2"
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
    send_bt(&mut h, &state);
    shown(&mut h);
    h.type_text("0000");
    h.click_on("Pair");
    h.click_on("Cancel");
    assert!(matches!(
        next_bt(&mut h),
        Some(bluetooth::Command::Answer {
            serial: 3,
            answer: bluetooth::Answer::Code(code),
        }) if code == "0000"
    ));
    assert!(matches!(next_bt(&mut h), Some(bluetooth::Command::Decline)));
}

/// A network is connected to through its row, and the row says how it stands.
fn a11y_wifi_rows() {
    let mut h = dashboard();
    send_wifi(&mut h, &Demo::new().wifi);
    enter(&mut h, "Wi-Fi");
    let on = said(&mut h);
    assert!(
        on.starts_with("listitem \"Lighthouse\"") && on.contains("connected"),
        "{on}"
    );
    h.key("j");
    assert_eq!(said(&mut h), "listitem \"Harbour Café\" signal 54%, open");
    h.key("enter");
    // A network's key is its SSID after a letter for its kind of security: o for open.
    assert!(matches!(
        next_wifi(&mut h),
        Some(wifi::Command::Connect { key }) if key == "oHarbour Café"
    ));
}

/// A switch says whether it is on, and its action flips it.
fn a11y_wifi_switch() {
    let mut h = dashboard();
    send_wifi(&mut h, &Demo::new().wifi);
    h.click_on("Wi-Fi");
    assert!(matches!(next_wifi(&mut h), Some(wifi::Command::Toggle)));
}

/// A typed password is in no label or value a screen reader could read out, and yet is what gets
/// answered. (The field is a password field: it shows dots, and has no value for a reader.)
fn a11y_password_stays_secret() {
    let mut h = dashboard();
    send_wifi(&mut h, &Demo::new().wifi);
    enter(&mut h, "Wi-Fi");
    send_wifi(&mut h, &prompt_state(7, "Lighthouse"));
    shown(&mut h);
    h.type_text("hunter2");
    let says = said(&mut h);
    assert!(!says.contains("hunter2"), "the cursor reads out the password: {says}");
    assert!(says.contains("Password"), "the field isn't there to read: {says}");
    let (masked, value) = h.update(|d, cx| {
        let view = d.widgets.net.wifi.views.values().next().expect("a view");
        let pool = view.pool.as_ref().expect("the prompt's fields");
        let state = pool.state("pass").expect("a password field");
        let s = state.read(cx);
        (s.presentation().is_masked(), s.value().to_string())
    });
    assert!(masked, "the password shows as typed");
    assert_eq!(value, "hunter2");
    h.key("enter");
    assert!(matches!(
        next_wifi(&mut h),
        Some(wifi::Command::Answer { serial: 7, answer }) if answer.password == "hunter2"
    ));
}

// ---- the keyboard ----------------------------------------------------------------------------------

/// Joining from a text field closes the form and the field with it. The keyboard comes back: the
/// keys still work, and Escape still gets out.
fn keyboard_survives_its_field_going_away() {
    let mut h = dashboard();
    send_wifi(&mut h, &Demo::new().wifi);
    enter(&mut h, "Wi-Fi");
    walk_to(&mut h, "Hidden network");
    h.key("enter");
    shown(&mut h);
    h.type_text("Attic");
    // Enter joins from the password field, not from the name.
    h.key("tab");
    h.key("enter");
    shown(&mut h);
    assert!(matches!(
        next_wifi(&mut h),
        Some(wifi::Command::JoinHidden { ssid, .. }) if ssid == "Attic"
    ));
    let now = said(&mut h);
    assert!(now.contains("Lighthouse"), "back on the list, but: {now}");
    h.key("esc");
    assert_eq!(spoken(&mut h), None, "left the widget");
    h.key("esc");
    assert_eq!(h.controls(), [Control::Hide], "closed");
}

/// A network dropping off while the cursor is on its details button leaves the cursor on its row.
fn sub_part_gone_goes_back_to_the_row() {
    let mut h = dashboard();
    let mut state = Demo::new().wifi;
    send_wifi(&mut h, &state);
    enter(&mut h, "Wi-Fi");
    h.key("l");
    h.key("l");
    assert!(said(&mut h).contains("Details of Lighthouse"));
    state.networks[0].active = false;
    send_wifi(&mut h, &state);
    let now = said(&mut h);
    assert!(now.starts_with("listitem \"Lighthouse\""), "on: {now}");
}

/// A network coming up while the cursor is on its details button must not turn that into
/// Disconnect, under a key press meant for something else.
fn sub_part_changing_meaning_goes_back_to_the_row() {
    let mut h = dashboard();
    let mut state = Demo::new().wifi;
    state.networks[0].active = false;
    send_wifi(&mut h, &state);
    enter(&mut h, "Wi-Fi");
    h.key("l");
    assert!(said(&mut h).contains("Details of Lighthouse"));
    state.networks[0].active = true;
    send_wifi(&mut h, &state);
    let now = said(&mut h);
    assert!(now.starts_with("listitem \"Lighthouse\""), "on: {now}");
}

/// A new Enterprise network's prompt on an entered Wi-Fi widget.
fn enterprise_prompt(h: &mut Harness) {
    send_wifi(h, &Demo::new().wifi);
    enter(h, "Wi-Fi");
    let mut state = Demo::new().wifi;
    let mut prompt = wifi::Prompt::default();
    prompt.serial = 9;
    prompt.ssid = "Campus".into();
    prompt.kind = wifi::PromptKind::Enterprise;
    prompt.new = true;
    state.prompt = Some(prompt);
    send_wifi(h, &state);
    shown(h);
}

/// The hidden-network form, on the 802.1X choice with the server check, with a name typed.
fn hidden_enterprise_form(h: &mut Harness) {
    send_wifi(h, &Demo::new().wifi);
    enter(h, "Wi-Fi");
    walk_to(h, "Hidden network");
    h.key("enter");
    shown(h);
    h.type_text("Attic");
    h.key("esc");
    h.key("j");
    h.key("l");
    shown(h);
}

/// Turning the server check off keeps the cursor on the switch: pressed again, it can't cancel.
fn server_check_keeps_the_cursor_in_the_prompt() {
    let mut h = tall_wifi();
    enterprise_prompt(&mut h);
    h.key("esc");
    walk_to(&mut h, "Don't check");
    h.key("enter");
    let now = said(&mut h);
    assert!(now.contains("Don't check"), "on: {now}");
    h.key("enter");
    assert!(
        !matches!(next_wifi(&mut h), Some(wifi::Command::CancelPrompt)),
        "cancelled the prompt"
    );
}

/// Turning the server check off keeps the cursor on the switch: pressed again, it can't join a
/// network with the check off.
fn server_check_keeps_the_cursor_in_the_form() {
    let mut h = tall_wifi();
    hidden_enterprise_form(&mut h);
    walk_to(&mut h, "Don't check");
    h.key("enter");
    let now = said(&mut h);
    assert!(now.contains("Don't check"), "on: {now}");
    h.key("enter");
    assert!(
        !matches!(next_wifi(&mut h), Some(wifi::Command::JoinHidden { .. })),
        "joined with the check off"
    );
}

/// The cursor says the field the keys go to: from the start, and after Tab.
fn cursor_follows_the_text_field() {
    let mut h = tall_wifi();
    enterprise_prompt(&mut h);
    let now = said(&mut h);
    assert!(now.contains("User name"), "typing into the name, on: {now}");
    h.key("tab");
    let now = said(&mut h);
    assert!(now.contains("Password"), "typing the password, on: {now}");
}

/// A list growing above the row the cursor is on scrolls, so the cursor stays in sight.
fn cursor_stays_in_sight_as_a_list_grows() {
    let mut h = short("bluetooth");
    let mut state = Demo::new().bluetooth;
    send_bt(&mut h, &state);
    enter(&mut h, "Bluetooth");
    h.key("G");
    let now = in_sight(&mut h);
    assert!(now.contains("Add a device"), "on the last row, but: {now}");
    state.discovering = true;
    send_bt(&mut h, &state);
    let now = in_sight(&mut h);
    assert!(now.contains("Stop looking"), "on the same row, but: {now}");
}

/// A short tile: every stop of the hidden-network form scrolls into sight.
fn stops_in_sight_hidden_form() {
    let mut h = short("wifi");
    hidden_enterprise_form(&mut h);
    every_stop_in_sight(&mut h);
}

fn lighthouse_detail(state: &wifi::State, uuid: &str) -> wifi::Detail {
    wifi::Detail {
        key: state.networks[0].key(),
        ssid: "Lighthouse".into(),
        network: Some(state.networks[0].clone()),
        profile: Some(wifi::Profile {
            ssid: "Lighthouse".into(),
            uuid: uuid.into(),
            ..Default::default()
        }),
    }
}

/// The wheel lets go of the cursor until the next key, which brings the list back to it.
fn wheel_lets_go_until_the_next_key() {
    let mut h = short("wifi");
    hidden_enterprise_form(&mut h);
    h.key("G");
    assert!(sight(&mut h).1, "the last stop, in sight");
    let list = h.bounds_of("hidden-list").expect("the form's list");
    let (x, y) = (f32::from(list.center().x), f32::from(list.center().y));
    h.scroll(x, y, 600.0);
    shown(&mut h);
    let (said, ok) = sight(&mut h);
    assert!(!ok, "the wheel scrolled the list away from {said}");
    // Redrawn for something else (a network drifting): the list stays where it was put.
    send_wifi(&mut h, &Demo::new().wifi);
    assert!(!sight(&mut h).1, "the list went back by itself");
    h.key("k");
    h.key("j");
    assert!(sight(&mut h).1, "the next key brings it back");
}

/// A short tile: the details page of a saved network.
fn stops_in_sight_wifi_details() {
    let mut h = short("wifi");
    let mut state = Demo::new().wifi;
    send_wifi(&mut h, &state);
    enter(&mut h, "Wi-Fi");
    h.key("l");
    h.key("l");
    h.key("enter");
    state.detail = Some(lighthouse_detail(&state, ""));
    send_wifi(&mut h, &state);
    every_stop_in_sight(&mut h);
}

/// A short tile: the password and QR code appearing above Share push it down; it stays in sight.
fn stops_in_sight_wifi_share() {
    let mut h = short("wifi");
    let mut state = Demo::new().wifi;
    send_wifi(&mut h, &state);
    enter(&mut h, "Wi-Fi");
    h.key("l");
    h.key("l");
    h.key("enter");
    state.detail = Some(lighthouse_detail(&state, "u"));
    send_wifi(&mut h, &state);
    for _ in 0..8 {
        if said(&mut h).contains("Share network") {
            break;
        }
        h.key("j");
    }
    assert!(said(&mut h).contains("Share network"));
    // A QR code the size of a small real one.
    let side = 25;
    let data: Vec<u8> = (0..side * side)
        .flat_map(|i| {
            let v = if (i / side + i % side) % 3 == 0 { 0 } else { 255 };
            [v, v, v, 255]
        })
        .collect();
    state.secret = Some(wifi::Secret {
        uuid: "u".into(),
        password: "hunter2".into(),
        qr: niri_dash_system::IconSrc::Rgba {
            width: side as u32,
            height: side as u32,
            data: std::sync::Arc::new(data),
        },
    });
    send_wifi(&mut h, &state);
    let now = in_sight(&mut h);
    assert!(now.contains("Hide password"), "on: {now}");
    assert!(h.has("QR code to join the network"));
}

/// A short tile: Bluetooth's details page.
fn stops_in_sight_bluetooth_details() {
    let mut h = short("bluetooth");
    let mut state = Demo::new().bluetooth;
    send_bt(&mut h, &state);
    enter(&mut h, "Bluetooth");
    h.key("l");
    h.key("enter");
    state.detail = Some(state.devices[0].clone());
    send_bt(&mut h, &state);
    every_stop_in_sight(&mut h);
}

/// The keys of the networks page: r rescans, t switches the radio, d disconnects, Enter on the
/// network in use opens its details, on another joins it.
fn wifi_keys() {
    let mut h = dashboard();
    send_wifi(&mut h, &Demo::new().wifi);
    enter(&mut h, "Wi-Fi");
    h.key("r");
    assert!(matches!(next_wifi(&mut h), Some(wifi::Command::Rescan)));
    h.key("t");
    assert!(matches!(next_wifi(&mut h), Some(wifi::Command::Toggle)));
    h.key("d");
    assert!(matches!(next_wifi(&mut h), Some(wifi::Command::Disconnect)));
    // The row of the network in use: Enter opens its details.
    h.key("enter");
    assert!(matches!(
        next_wifi(&mut h),
        Some(wifi::Command::Select { key }) if key == "pLighthouse"
    ));
    // Back out: nothing selected, the cursor at the top of the list again.
    h.key("esc");
    assert!(matches!(
        next_wifi(&mut h),
        Some(wifi::Command::Select { key }) if key.is_empty()
    ));
    assert!(matches!(next_wifi(&mut h), Some(wifi::Command::HideSecret)));
    assert!(said(&mut h).starts_with("listitem \"Lighthouse\""));
    // Down to the last stop, Hidden network: no "Saved networks" while there are none.
    h.key("G");
    assert!(said(&mut h).contains("Hidden network"));
    // l on the row of a network that isn't the one in use: the details button.
    h.key("g");
    h.key("j");
    h.key("l");
    assert_eq!(said(&mut h), "button \"Details of Harbour Café\"");
    h.key("enter");
    assert!(matches!(
        next_wifi(&mut h),
        Some(wifi::Command::Select { key }) if key == "oHarbour Café"
    ));
}

/// Saved networks: the row that leads to them, their page, and a network's details from it; Back
/// goes back to the page it came from.
fn wifi_saved_networks() {
    let mut h = dashboard();
    let mut state = Demo::new().wifi;
    state.saved = vec![
        wifi::Profile {
            ssid: "Lighthouse".into(),
            uuid: "u1".into(),
            key_mgmt: "wpa-psk".into(),
            timestamp: 1_700_000_000,
            ..Default::default()
        },
        wifi::Profile {
            ssid: "Old café".into(),
            uuid: "u2".into(),
            ..Default::default()
        },
    ];
    send_wifi(&mut h, &state);
    enter(&mut h, "Wi-Fi");
    h.key("G");
    assert_eq!(said(&mut h), "listitem \"Saved networks (2)\"");
    h.key("enter");
    assert_eq!(
        said(&mut h),
        "listitem \"Lighthouse\" connected, in range",
        "the first saved network"
    );
    h.key("j");
    assert_eq!(said(&mut h), "listitem \"Old café\" never used");
    h.key("enter");
    assert!(matches!(
        next_wifi(&mut h),
        Some(wifi::Command::Select { key }) if key == "oOld café"
    ));
    // Back from its details to the saved networks, and from there to the list.
    h.key("esc");
    assert!(said(&mut h).starts_with("listitem \"Lighthouse\""));
    h.key("esc");
    assert!(said(&mut h).starts_with("listitem \"Lighthouse\" connected, signal"));
    assert!(h.has("Hidden network"), "back on the networks");
}

/// The details of a saved network: its switch, Share and Forget, and what they send.
fn wifi_details_buttons() {
    let mut h = dashboard();
    let mut state = Demo::new().wifi;
    send_wifi(&mut h, &state);
    enter(&mut h, "Wi-Fi");
    h.key("l");
    h.key("l");
    h.key("enter");
    state.detail = Some(lighthouse_detail(&state, "u1"));
    send_wifi(&mut h, &state);
    while next_wifi(&mut h).is_some() {}
    assert_eq!(said(&mut h), "switch \"Connect automatically\" (off)");
    h.key("enter");
    assert!(matches!(
        next_wifi(&mut h),
        Some(wifi::Command::SetAutoconnect { uuid, on: true }) if uuid == "u1"
    ));
    h.key("j");
    assert_eq!(said(&mut h), "button \"Disconnect\"");
    h.key("enter");
    assert!(matches!(next_wifi(&mut h), Some(wifi::Command::Disconnect)));
    h.key("j");
    assert_eq!(said(&mut h), "button \"Share network\"");
    h.key("enter");
    assert!(matches!(
        next_wifi(&mut h),
        Some(wifi::Command::Reveal { uuid }) if uuid == "u1"
    ));
    h.key("j");
    assert_eq!(said(&mut h), "button \"Forget network\"");
    h.key("enter");
    assert!(matches!(
        next_wifi(&mut h),
        Some(wifi::Command::Forget { uuid }) if uuid == "u1"
    ));
    // Forgetting leaves the page.
    assert!(said(&mut h).starts_with("listitem \"Lighthouse\""));
}

/// The hidden-network form: the security choice (by keys), what is typed, what is sent.
fn wifi_hidden_network() {
    let mut h = tall_wifi();
    send_wifi(&mut h, &Demo::new().wifi);
    enter(&mut h, "Wi-Fi");
    walk_to(&mut h, "Hidden network");
    h.key("enter");
    shown(&mut h);
    h.type_text("Attic");
    h.key("esc");
    // Security: h and l pick (Open, WPA, 802.1X).
    h.key("j");
    assert_eq!(said(&mut h), "button \"Security: WPA\" selected");
    h.key("h");
    assert_eq!(said(&mut h), "button \"Security: Open\" selected");
    h.key("l");
    h.key("l");
    h.key("l");
    assert_eq!(said(&mut h), "button \"Security: 802.1X\" selected");
    h.key("h");
    // WPA: the password, then Join.
    h.key("j");
    assert_eq!(said(&mut h), "textinput \"Password\"");
    h.key("enter");
    shown(&mut h);
    h.type_text("s3cret");
    h.key("esc");
    h.key("j");
    assert_eq!(said(&mut h), "button \"Join\"");
    h.key("enter");
    match next_wifi(&mut h) {
        Some(wifi::Command::JoinHidden {
            ssid,
            security,
            answer,
        }) => {
            assert_eq!(ssid, "Attic");
            assert_eq!(security, 1);
            assert_eq!(answer.password, "s3cret");
        }
        other => panic!("not a join: {other:?}"),
    }
    // The form is closed.
    assert!(said(&mut h).starts_with("listitem \"Lighthouse\""));
}

/// The hidden form's choices (Open, WPA, 802.1X) take their text's width and share what is left,
/// as Slint's stretch does: "802.1X" is wider than "Open", and they sit in one row.
fn hidden_form_choices_follow_their_text() {
    let mut h = tall_wifi();
    send_wifi(&mut h, &Demo::new().wifi);
    enter(&mut h, "Wi-Fi");
    walk_to(&mut h, "Hidden network");
    h.key("enter");
    shown(&mut h);
    let open = h.bounds_of("Security: Open").expect("the Open button");
    let wpa = h.bounds_of("Security: WPA").expect("the WPA button");
    let eap = h.bounds_of("Security: 802.1X").expect("the 802.1X button");
    assert!(
        open.size.width < eap.size.width,
        "Open is {:?} wide, 802.1X {:?}",
        open.size.width,
        eap.size.width
    );
    assert_eq!(open.origin.y, wpa.origin.y);
    assert_eq!(wpa.origin.y, eap.origin.y);
    assert!(open.right() < wpa.left() && wpa.right() < eap.left());
}

/// The code to compare is drawn digit by digit, 3 px apart (GPUI has no letter spacing), and is
/// still one thing for a screen reader.
fn pairing_code_is_one_spaced_line() {
    let mut h = tall_bt();
    send_bt(&mut h, &Demo::new().bluetooth);
    let mut state = Demo::new().bluetooth;
    state.prompt = Some(bluetooth::Prompt {
        serial: 9,
        name: "Arc Mouse".into(),
        address: "11:22:33:44:55:66".into(),
        kind: bluetooth::PromptKind::Confirm(7_654),
    });
    send_bt(&mut h, &state);
    shown(&mut h);
    assert!(h.has("pairing code"));
    let wide = h.bounds_of("pairing code").expect("the code");
    // The code is centered in the prompt, which is as wide as the card's inside.
    assert!(f32::from(wide.size.width) > 100.0);
}

/// The prompt's keys: Escape out of the field first, then the buttons; Escape cancels it.
fn wifi_prompt_keys() {
    let mut h = dashboard();
    send_wifi(&mut h, &Demo::new().wifi);
    enter(&mut h, "Wi-Fi");
    send_wifi(&mut h, &prompt_state(4, "Lighthouse"));
    shown(&mut h);
    assert!(said(&mut h).contains("Password"));
    h.type_text("abc");
    h.key("esc");
    assert!(next_wifi(&mut h).is_none(), "the first Escape only leaves the field");
    h.key("j");
    assert_eq!(said(&mut h), "button \"Cancel\"");
    h.key("j");
    assert_eq!(said(&mut h), "button \"Join\"");
    h.key("enter");
    assert!(matches!(
        next_wifi(&mut h),
        Some(wifi::Command::Answer { serial: 4, answer }) if answer.password == "abc"
    ));
    // What was sent is gone from the field. Letters are the field's while it has the keyboard: j
    // and f and e are text.
    h.key("g");
    h.key("enter");
    shown(&mut h);
    h.type_text("jfe");
    h.key("enter");
    assert!(matches!(
        next_wifi(&mut h),
        Some(wifi::Command::Answer { answer, .. }) if answer.password == "jfe"
    ));
    h.key("esc");
    h.key("esc");
    assert!(matches!(next_wifi(&mut h), Some(wifi::Command::CancelPrompt)));
}

/// A new prompt starts with empty fields.
fn wifi_prompt_starts_empty() {
    let mut h = dashboard();
    send_wifi(&mut h, &prompt_state(1, "Lighthouse"));
    shown(&mut h);
    h.type_text("first");
    send_wifi(&mut h, &prompt_state(2, "Harbour Café"));
    shown(&mut h);
    h.type_text("second");
    h.key("enter");
    assert!(matches!(
        next_wifi(&mut h),
        Some(wifi::Command::Answer { serial: 2, answer }) if answer.password == "second"
    ));
}

/// Wi-Fi off, blocked, absent: what is said, and no stops to walk.
fn wifi_not_available() {
    let mut h = dashboard();
    // Nothing reported yet: no device.
    enter(&mut h, "Wi-Fi");
    assert_eq!(spoken(&mut h), None);
    assert!(!h.has("Hidden network"));
    let mut state = Demo::new().wifi;
    state.enabled = false;
    send_wifi(&mut h, &state);
    shown(&mut h);
    assert!(!h.has("Lighthouse"), "a list while it is off");
    assert_eq!(spoken(&mut h), None);
    h.key("t");
    assert!(matches!(next_wifi(&mut h), Some(wifi::Command::Toggle)));
    state.blocked = true;
    send_wifi(&mut h, &state);
    assert!(!h.has("Lighthouse"));
}

/// A captive portal puts a row on top that signs in.
fn wifi_portal() {
    let mut h = dashboard();
    let mut state = Demo::new().wifi;
    state.link.connectivity = wifi::Connectivity::Portal;
    send_wifi(&mut h, &state);
    enter(&mut h, "Wi-Fi");
    assert_eq!(said(&mut h), "listitem \"Sign in to the network\"");
    h.key("enter");
    assert!(matches!(next_wifi(&mut h), Some(wifi::Command::SignIn)));
}

/// A round tile is the switch.
fn round_tiles() {
    let mut h = up(Harness::new(
        "layout {\n    wifi col=0 row=0 size=2 shape=\"round\"\n    bluetooth col=2 row=0 size=2 shape=\"round\"\n}\n",
    ));
    send_wifi(&mut h, &Demo::new().wifi);
    send_bt(&mut h, &Demo::new().bluetooth);
    h.click_on("Wi-Fi");
    assert!(matches!(next_wifi(&mut h), Some(wifi::Command::Toggle)));
    h.click_on("Bluetooth");
    assert!(matches!(next_bt(&mut h), Some(bluetooth::Command::Toggle)));
    enter(&mut h, "Wi-Fi");
    h.key("enter");
    assert!(matches!(next_wifi(&mut h), Some(wifi::Command::Toggle)));
}

/// Bluetooth's keys: r looks for devices, t switches the adapter, Enter on a device connects or
/// disconnects it, l goes to its details.
fn bluetooth_keys() {
    let mut h = dashboard();
    send_bt(&mut h, &Demo::new().bluetooth);
    enter(&mut h, "Bluetooth");
    assert_eq!(said(&mut h), "listitem \"Fairbuds\" connected, battery 80%");
    h.key("enter");
    assert!(matches!(
        next_bt(&mut h),
        Some(bluetooth::Command::Disconnect(a)) if a == "AC:80:0A:11:22:33"
    ));
    h.key("j");
    h.key("enter");
    assert!(matches!(
        next_bt(&mut h),
        Some(bluetooth::Command::Connect(a)) if a == "F4:73:35:44:55:66"
    ));
    h.key("r");
    assert!(matches!(next_bt(&mut h), Some(bluetooth::Command::Scan(true))));
    h.key("t");
    assert!(matches!(next_bt(&mut h), Some(bluetooth::Command::Toggle)));
    h.key("l");
    assert_eq!(said(&mut h), "button \"Details of MX Keys\"");
    h.key("enter");
    assert!(matches!(
        next_bt(&mut h),
        Some(bluetooth::Command::Select(a)) if a == "F4:73:35:44:55:66"
    ));
    h.key("esc");
    assert!(matches!(
        next_bt(&mut h),
        Some(bluetooth::Command::Select(a)) if a.is_empty()
    ));
    h.key("G");
    assert_eq!(said(&mut h), "listitem \"Add a device\"");
    h.key("enter");
    assert!(matches!(next_bt(&mut h), Some(bluetooth::Command::Scan(true))));
}

/// What a scan finds is paired from its row; a device's details have the switches, the name and
/// the buttons.
fn bluetooth_details() {
    let mut h = tall_bt();
    let mut state = Demo::new().bluetooth;
    let mut nearby = state.devices[1].clone();
    nearby.address = "11:22:33:44:55:66".into();
    nearby.name = "Arc Mouse".into();
    nearby.paired = false;
    nearby.trusted = false;
    nearby.rssi = Some(-60);
    state.nearby = vec![nearby];
    state.discovering = true;
    send_bt(&mut h, &state);
    enter(&mut h, "Bluetooth");
    h.keys("j j");
    assert!(
        said(&mut h).starts_with("listitem \"Arc Mouse\""),
        "{}",
        said(&mut h)
    );
    h.key("enter");
    assert!(matches!(
        next_bt(&mut h),
        Some(bluetooth::Command::Pair(a)) if a == "11:22:33:44:55:66"
    ));
    // The details of a paired device.
    h.key("g");
    h.key("l");
    h.key("enter");
    state.detail = Some(state.devices[0].clone());
    send_bt(&mut h, &state);
    while next_bt(&mut h).is_some() {}
    assert_eq!(said(&mut h), "switch \"Connect automatically\" (on)");
    h.key("enter");
    assert!(matches!(
        next_bt(&mut h),
        Some(bluetooth::Command::SetTrusted(_, false))
    ));
    h.key("j");
    assert_eq!(said(&mut h), "switch \"Block\" (off)");
    h.key("enter");
    assert!(matches!(
        next_bt(&mut h),
        Some(bluetooth::Command::SetBlocked(_, true))
    ));
    h.key("j");
    assert_eq!(said(&mut h), "textinput \"New name\"");
    h.key("enter");
    shown(&mut h);
    h.type_text("Buds");
    h.key("enter");
    assert!(matches!(
        next_bt(&mut h),
        Some(bluetooth::Command::Rename(a, n)) if a == "AC:80:0A:11:22:33" && n == "Buds"
    ));
    h.key("esc");
    h.key("j");
    assert_eq!(said(&mut h), "button \"Disconnect\"");
    h.key("j");
    assert_eq!(said(&mut h), "button \"Forget device\"");
    h.key("enter");
    assert!(matches!(
        next_bt(&mut h),
        Some(bluetooth::Command::Forget(a)) if a == "AC:80:0A:11:22:33"
    ));
}

/// A pairing question: the cursor starts on the answer, and Enter says yes.
fn bluetooth_confirm_prompt() {
    let mut h = dashboard();
    send_bt(&mut h, &Demo::new().bluetooth);
    enter(&mut h, "Bluetooth");
    let mut state = Demo::new().bluetooth;
    state.prompt = Some(bluetooth::Prompt {
        serial: 5,
        name: "Arc Mouse".into(),
        address: "11:22:33:44:55:66".into(),
        kind: bluetooth::PromptKind::Confirm(123_456),
    });
    send_bt(&mut h, &state);
    shown(&mut h);
    // The prompt appeared with the widget entered: the cursor starts on the first button.
    assert_eq!(said(&mut h), "button \"No\"");
    h.key("l");
    assert_eq!(said(&mut h), "button \"Yes, they match\"");
    h.key("enter");
    assert!(matches!(
        next_bt(&mut h),
        Some(bluetooth::Command::Answer {
            serial: 5,
            answer: bluetooth::Answer::Yes
        })
    ));
    // Escape says no.
    h.key("esc");
    assert!(matches!(next_bt(&mut h), Some(bluetooth::Command::Decline)));
    // Entering a widget with the question up: the cursor is on the answer.
    h.key("esc");
    enter(&mut h, "Bluetooth");
    assert_eq!(said(&mut h), "button \"Yes, they match\"");
}

/// Typing a code shown here on the device: Cancel is all there is to press.
fn bluetooth_code_shown() {
    let mut h = dashboard();
    let mut state = Demo::new().bluetooth;
    state.prompt = Some(bluetooth::Prompt {
        serial: 6,
        name: "Arc Keyboard".into(),
        address: "11:22:33:44:55:77".into(),
        kind: bluetooth::PromptKind::ShowPasskey(42, 2),
    });
    send_bt(&mut h, &state);
    enter(&mut h, "Bluetooth");
    assert_eq!(said(&mut h), "button \"Cancel\"");
    assert!(!h.has("Pair"));
    h.key("enter");
    assert!(matches!(
        next_bt(&mut h),
        Some(bluetooth::Command::CancelPairing)
    ));
}

/// A passkey is digits; a PIN is any text. The answer says what was typed.
fn bluetooth_passkey_is_digits() {
    let mut h = dashboard();
    let mut state = Demo::new().bluetooth;
    state.prompt = Some(bluetooth::Prompt {
        serial: 8,
        name: "Arc Keyboard".into(),
        address: "11:22:33:44:55:77".into(),
        kind: bluetooth::PromptKind::EnterPasskey,
    });
    send_bt(&mut h, &state);
    shown(&mut h);
    h.type_text("12a3-4");
    h.key("enter");
    assert!(matches!(
        next_bt(&mut h),
        Some(bluetooth::Command::Answer {
            serial: 8,
            answer: bluetooth::Answer::Code(code)
        }) if code == "1234"
    ));
}

/// Join is not there to press before the network has a name.
fn wifi_hidden_join_needs_a_name() {
    let mut h = tall_wifi();
    send_wifi(&mut h, &Demo::new().wifi);
    enter(&mut h, "Wi-Fi");
    walk_to(&mut h, "Hidden network");
    h.key("enter");
    shown(&mut h);
    h.click_on("Join");
    assert!(next_wifi(&mut h).is_none(), "joined without a name");
    h.type_text("Attic");
    h.click_on("Join");
    assert!(matches!(
        next_wifi(&mut h),
        Some(wifi::Command::JoinHidden { ssid, security: 1, .. }) if ssid == "Attic"
    ));
}

/// Bluetooth off, blocked, absent.
fn bluetooth_not_available() {
    let mut h = dashboard();
    enter(&mut h, "Bluetooth");
    assert_eq!(spoken(&mut h), None);
    let mut state = Demo::new().bluetooth;
    state.powered = false;
    send_bt(&mut h, &state);
    assert!(!h.has("Fairbuds"));
    assert_eq!(spoken(&mut h), None);
    state.blocked = true;
    send_bt(&mut h, &state);
    assert!(!h.has("Fairbuds"));
}

/// Two Wi-Fi widgets show the same networks, but each has its own page and cursor.
fn two_widgets_have_their_own_pages() {
    let mut h = up(Harness::new(
        "layout {\n    wifi col=0 row=0 width=3 height=3\n    wifi col=3 row=0 width=3 height=3\n}\n",
    ));
    send_wifi(&mut h, &Demo::new().wifi);
    enter(&mut h, "Wi-Fi");
    h.key("j");
    assert!(said(&mut h).contains("Harbour"));
    let mut cursors = h.read(|d| {
        d.widgets
            .net
            .wifi
            .views
            .iter()
            .map(|(id, v)| (*id, v.scope.cursor))
            .collect::<Vec<_>>()
    });
    cursors.sort();
    assert_eq!(
        cursors.iter().map(|c| c.1).collect::<Vec<_>>(),
        [1, 0],
        "only the entered one moved"
    );
}

/// The monkey on the mock system with nothing but Wi-Fi and Bluetooth widgets, big and small: it
/// walks them, types into their fields, scrolls and clicks them, and after every key the
/// dashboard (the cursor on a stop that exists and in sight, the keyboard where the cursor is) is
/// checked. `NIRI_DASH_STRESS_OPS` sets how many keys.
fn stress_net() {
    let ops: u64 = std::env::var("NIRI_DASH_STRESS_OPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1500);
    let layouts = [
        "layout {\n    wifi col=0 row=0 width=3 height=3\n    bluetooth col=3 row=0 width=3 height=3\n}\n",
        "layout {\n    wifi col=0 row=0 width=3 height=2\n    bluetooth col=3 row=0 width=3 height=2\n    wifi col=6 row=0 size=2 shape=\"round\"\n    bluetooth col=8 row=0 size=2 shape=\"round\"\n}\n",
    ];
    for (n, layout) in layouts.iter().enumerate() {
        let seeds: Vec<u64> = std::env::var("NIRI_DASH_STRESS_SEEDS")
            .ok()
            .map(|s| s.split(',').filter_map(|n| n.trim().parse().ok()).collect())
            .unwrap_or_else(|| vec![1, 2, 3, 4]);
        for seed in seeds {
            for size in [(1706, 1066), (1280, 800)] {
                let h = Harness::with_mock_sized(layout, size);
                let mut h = h;
                let monkey = h
                    .stress(Monkey::new(seed, std::env::var_os("NIRI_DASH_STRESS_LOG").is_some()).keep_going(), ops)
                    .unwrap_or_else(|f| panic!("layout {n}, seed {seed}, {size:?}:\n{f}"));
                if let Err(f) = monkey.finish() {
                    panic!("layout {n}, seed {seed}, {size:?}:\n{f}");
                }
            }
        }
    }
}

/// A small deterministic dice.
struct Dice(u64);

impl Dice {
    fn below(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) % n
    }

    fn yes(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

/// What the system might be reporting about Wi-Fi: a prompt of any kind, a details page of a
/// network in range or not, a network that wants a login, saved networks, a shared password.
fn any_wifi(dice: &mut Dice) -> wifi::State {
    let mut state = Demo::new().wifi;
    state.enabled = dice.yes(85);
    state.available = dice.yes(95);
    state.blocked = dice.yes(5);
    state.scanning = dice.yes(30);
    if dice.yes(30) {
        state.link.connectivity = wifi::Connectivity::Portal;
    }
    if dice.yes(20) {
        state.networks.truncate(dice.below(3) as usize);
    }
    if dice.yes(30) {
        state.networks[..].iter_mut().for_each(|n| n.active = false);
    }
    if dice.yes(40) {
        state.saved = (0..dice.below(5))
            .map(|i| wifi::Profile {
                ssid: format!("Saved {i}"),
                uuid: format!("u{i}"),
                timestamp: dice.below(2) * 1_700_000_000,
                ..Default::default()
            })
            .collect();
    }
    if dice.yes(35) {
        let mut prompt = wifi::Prompt::default();
        prompt.serial = dice.below(4);
        prompt.ssid = "Campus".into();
        prompt.kind = [
            wifi::PromptKind::Password,
            wifi::PromptKind::Wep,
            wifi::PromptKind::Enterprise,
        ][dice.below(3) as usize];
        prompt.new = dice.yes(60);
        prompt.retry = dice.yes(30);
        if dice.yes(30) {
            prompt.message = "Too short".into();
        }
        state.prompt = Some(prompt);
    }
    if dice.yes(50) && !state.networks.is_empty() {
        let net = dice.below(state.networks.len() as u64) as usize;
        let known = dice.yes(60);
        state.detail = Some(wifi::Detail {
            key: state.networks[net].key(),
            ssid: state.networks[net].ssid.clone(),
            network: dice.yes(80).then(|| state.networks[net].clone()),
            profile: known.then(|| wifi::Profile {
                ssid: state.networks[net].ssid.clone(),
                uuid: "u".into(),
                ..Default::default()
            }),
        });
        if known && dice.yes(50) {
            state.secret = Some(wifi::Secret {
                uuid: "u".into(),
                password: "hunter2".into(),
                qr: niri_dash_system::IconSrc::None,
            });
        }
    }
    state
}

fn any_bluetooth(dice: &mut Dice) -> bluetooth::State {
    let mut state = Demo::new().bluetooth;
    state.powered = dice.yes(85);
    state.available = dice.yes(95);
    state.blocked = dice.yes(5);
    state.discovering = dice.yes(40);
    state.unnamed = dice.below(3) as usize;
    if dice.yes(60) {
        let n = dice.below(4);
        state.nearby = (0..n)
            .map(|i| {
                let mut d = state.devices[0].clone();
                d.address = format!("11:22:33:44:55:{i:02}");
                d.name = format!("Nearby {i}");
                d.paired = false;
                d.connected = false;
                d.battery = -1;
                d.rssi = Some(-50 - 8 * i as i16);
                d
            })
            .collect();
    }
    if dice.yes(30) {
        state.devices.truncate(dice.below(3) as usize);
    }
    if dice.yes(30) {
        let kind = dice.below(6);
        use bluetooth::PromptKind as K;
        state.prompt = Some(bluetooth::Prompt {
            serial: dice.below(4),
            name: "Arc".into(),
            address: "11:22:33:44:55:00".into(),
            kind: match kind {
                0 => K::Confirm(123_456),
                1 => K::EnterPin,
                2 => K::EnterPasskey,
                3 => K::ShowPin("0000".into()),
                4 => K::ShowPasskey(42, 3),
                _ => K::Authorize,
            },
        });
    }
    if dice.yes(50) {
        let all: Vec<_> = state.devices.iter().chain(&state.nearby).cloned().collect();
        if !all.is_empty() {
            let mut det = all[dice.below(all.len() as u64) as usize].clone();
            det.blocked = dice.yes(20);
            det.connected &= dice.yes(70);
            state.detail = Some(det);
        }
    }
    state
}

/// The monkey again, with what the system reports changing under it between rounds: prompts of
/// every kind, details pages, lists that grow and shrink, a radio that goes off.
fn stress_net_states() {
    let ops: u64 = std::env::var("NIRI_DASH_STRESS_OPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1500);
    let layout = "layout {\n    wifi col=0 row=0 width=3 height=2\n    bluetooth col=3 row=0 width=3 height=2\n    wifi col=0 row=3 width=4 height=4\n    bluetooth col=4 row=3 width=4 height=4\n}\n";
    let seeds: Vec<u64> = std::env::var("NIRI_DASH_STRESS_SEEDS")
        .ok()
        .map(|s| s.split(',').filter_map(|n| n.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![1, 2, 3, 4]);
    for seed in seeds {
        for size in [(1706, 1066), (1280, 800)] {
            let mut h = Harness::with_mock_sized(layout, size);
            let mut dice = Dice(seed);
            let mut monkey = Monkey::new(seed, std::env::var_os("NIRI_DASH_STRESS_LOG").is_some())
                .keep_going();
            let round = 40;
            for r in 1..=ops.div_ceil(round) {
                let wifi = any_wifi(&mut dice);
                send_wifi(&mut h, &wifi);
                let bt = any_bluetooth(&mut dice);
                send_bt(&mut h, &bt);
                monkey = h
                    .stress(monkey, (r * round).min(ops))
                    .unwrap_or_else(|f| panic!("seed {seed}, {size:?}, round {r}:\n{f}"));
            }
            if let Err(f) = monkey.finish() {
                panic!("seed {seed}, {size:?}:\n{f}");
            }
        }
    }
}

fn main() {
    let _sandbox = Sandbox::enter(None).expect("a temporary directory for the dashboard's files");
    // SAFETY: nothing else runs yet that could read the environment.
    unsafe { std::env::set_var("LC_ALL", "C") };
    let scenarios: &[(&str, fn())] = &[
        ("wifi_commands", wifi_commands),
        ("wifi_state_shown", wifi_state_shown),
        ("wifi_password_prompt", wifi_password_prompt),
        ("bluetooth_pairing", bluetooth_pairing),
        ("a11y_wifi_rows", a11y_wifi_rows),
        ("a11y_wifi_switch", a11y_wifi_switch),
        ("a11y_password_stays_secret", a11y_password_stays_secret),
        (
            "keyboard_survives_its_field_going_away",
            keyboard_survives_its_field_going_away,
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
        ("cursor_follows_the_text_field", cursor_follows_the_text_field),
        (
            "cursor_stays_in_sight_as_a_list_grows",
            cursor_stays_in_sight_as_a_list_grows,
        ),
        ("stops_in_sight_hidden_form", stops_in_sight_hidden_form),
        (
            "wheel_lets_go_until_the_next_key",
            wheel_lets_go_until_the_next_key,
        ),
        ("stops_in_sight_wifi_details", stops_in_sight_wifi_details),
        ("stops_in_sight_wifi_share", stops_in_sight_wifi_share),
        (
            "stops_in_sight_bluetooth_details",
            stops_in_sight_bluetooth_details,
        ),
        ("wifi_keys", wifi_keys),
        ("wifi_saved_networks", wifi_saved_networks),
        ("wifi_details_buttons", wifi_details_buttons),
        ("wifi_hidden_network", wifi_hidden_network),
        (
            "hidden_form_choices_follow_their_text",
            hidden_form_choices_follow_their_text,
        ),
        (
            "pairing_code_is_one_spaced_line",
            pairing_code_is_one_spaced_line,
        ),
        ("wifi_prompt_keys", wifi_prompt_keys),
        ("wifi_prompt_starts_empty", wifi_prompt_starts_empty),
        ("wifi_not_available", wifi_not_available),
        ("wifi_portal", wifi_portal),
        ("round_tiles", round_tiles),
        ("bluetooth_keys", bluetooth_keys),
        ("bluetooth_details", bluetooth_details),
        ("bluetooth_confirm_prompt", bluetooth_confirm_prompt),
        ("bluetooth_code_shown", bluetooth_code_shown),
        ("bluetooth_passkey_is_digits", bluetooth_passkey_is_digits),
        (
            "wifi_hidden_join_needs_a_name",
            wifi_hidden_join_needs_a_name,
        ),
        ("bluetooth_not_available", bluetooth_not_available),
        (
            "two_widgets_have_their_own_pages",
            two_widgets_have_their_own_pages,
        ),
        ("stress_net", stress_net),
        ("stress_net_states", stress_net_states),
    ];
    let only = std::env::args().nth(1);
    let mut failed = 0;
    for (name, run) in scenarios {
        if only.as_deref().is_some_and(|o| !name.contains(o)) {
            continue;
        }
        match std::panic::catch_unwind(AssertUnwindSafe(run)) {
            Ok(()) => println!("test {name} ... ok"),
            Err(_) => {
                failed += 1;
                println!("test {name} ... FAILED");
            }
        }
    }
    if failed > 0 {
        eprintln!("{failed} scenario(s) failed");
        std::process::exit(1);
    }
}

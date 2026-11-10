//! The devices widgets (privacy, removable drives, phone) against channels standing in for the
//! system: what they show when the system reports something, what the keys and clicks do, and
//! that the keyboard cursor stays on a stop that exists and in sight (ports of
//! `privacy_*`, `stops_in_sight_privacy` and the drives and phone parts of the Slint tests).
//!
//! A plain program (`harness = false`): each scenario runs in turn on a fresh dashboard.

use niri_dash::Control;
use niri_dash::testing::{Harness, Sandbox};
use niri_dash_system::{Demo, drives, phones, privacy};
use std::panic::AssertUnwindSafe;

fn layout(widget: &str) -> String {
    format!("layout {{\n    {widget}\n}}\n")
}

/// A dashboard of one widget, shown.
fn show(widget: &str) -> Harness {
    let mut h = Harness::new(&layout(widget));
    h.control(Control::Show);
    h
}

/// Enter the widget titled `title` the way a person does: the hint labels, then its letters.
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
        h.type_text(&c.to_string());
    }
}

/// A selector the harness can look up: names made at run time live for the whole run.
fn name(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
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

fn usage(mic: &[&str], camera: &[&str]) -> privacy::Usage {
    privacy::Usage {
        mic: mic.iter().map(|s| s.to_string()).collect(),
        camera: camera.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

/// Where the privacy widget's keyboard cursor is.
fn privacy_cursor(h: &Harness) -> (usize, usize) {
    h.read(|d| {
        let s = d.widgets.devices.privacy.scope;
        (s.cursor, s.count)
    })
}

fn drives_cursor(h: &Harness) -> (usize, usize) {
    h.read(|d| {
        let s = d.widgets.devices.drives.scope;
        (s.cursor, s.count)
    })
}

fn phone_cursor(h: &Harness) -> (usize, usize) {
    h.read(|d| {
        let s = d.widgets.devices.phone.scope;
        (s.cursor, s.count)
    })
}

/// Whether the element called `what` is inside the one called `list` (the window's pixels).
fn in_sight(h: &Harness, what: &'static str, list: &'static str) -> bool {
    let (Some(a), Some(l)) = (h.bounds_of(what), h.bounds_of(list)) else {
        return false;
    };
    a.top() >= l.top() - gpui::px(1.0) && a.bottom() <= l.bottom() + gpui::px(1.0)
}

// ---- privacy -----------------------------------------------------------------------------------

/// The dots of a short tile say what they watch; Screen sharing is there on niri.
fn privacy_dots() {
    let mut h = show("privacy col=0 row=0 width=3 height=1");
    for dot in ["Microphone", "Camera", "Screen sharing"] {
        assert!(h.has(dot), "no {dot} dot");
    }
    h.send(privacy::Event::Usage(usage(&["Firefox"], &[])));
    h.send(privacy::Event::Casts(vec![cast(1)]));
    assert!(h.has("Microphone"));
    // A round one has the dots too.
    let h = show("privacy col=0 row=0 size=2 shape=\"round\"");
    assert!(h.has("Camera"));
}

/// Without a share, the list is the whole card; the keyboard has nothing to stop.
fn privacy_without_a_share_has_no_stops() {
    let mut h = show("privacy col=0 row=0 width=3 height=3");
    h.send(privacy::Event::Usage(usage(&["Firefox"], &[])));
    enter(&mut h, "Privacy");
    assert_eq!(privacy_cursor(&h), (0, 0));
    // Keys that would step or stop do nothing, and are the widget's.
    h.keys("j k enter");
    assert!(h.inbox.privacy.try_recv().is_err());
}

/// Five mic apps above a screen share: the cursor starts on its Stop button.
fn privacy_stops_are_the_shares() {
    let mut h = show("privacy col=0 row=0 width=3 height=4");
    h.send(privacy::Event::Usage(usage(
        &["Mic 0", "Mic 1", "Mic 2", "Mic 3", "Mic 4"],
        &[],
    )));
    h.send(privacy::Event::Casts(vec![cast(1)]));
    enter(&mut h, "Privacy");
    assert_eq!(privacy_cursor(&h), (0, 1));
    assert!(h.has("Stop sharing with App 1"));
    h.key("enter");
    assert!(matches!(
        h.inbox.privacy.try_recv(),
        Ok(privacy::Command::StopCast(1))
    ));
}

/// Clicking Stop stops that one.
fn privacy_stop_by_click() {
    let mut h = show("privacy col=0 row=0 width=3 height=4");
    h.send(privacy::Event::Casts(vec![cast(1), cast(2)]));
    h.click_on("Stop sharing with App 2");
    assert!(matches!(
        h.inbox.privacy.try_recv(),
        Ok(privacy::Command::StopCast(2))
    ));
    assert!(h.inbox.privacy.try_recv().is_err());
}

/// A short tile: many screen shares, and the Stop button of each comes into sight.
fn stops_in_sight_privacy() {
    let mut h = show("privacy col=0 row=0 width=3 height=2");
    h.send(privacy::Event::Casts((0..8).map(cast).collect()));
    enter(&mut h, "Privacy");
    let mut last = usize::MAX;
    for step in 0..30 {
        let (cursor, count) = privacy_cursor(&h);
        assert_eq!(count, 8);
        let stop = name(format!("Stop sharing with App {cursor}"));
        assert!(
            in_sight(&h, stop, "privacy list"),
            "step {step}: {stop} is out of sight"
        );
        if cursor == last {
            assert_eq!(cursor, 7, "never reached the last stop");
            return;
        }
        last = cursor;
        h.key("j");
    }
    panic!("never reached the last stop");
}

/// A share going away leaves the cursor on a stop that exists.
fn privacy_cursor_follows_the_list() {
    let mut h = show("privacy col=0 row=0 width=3 height=3");
    h.send(privacy::Event::Casts(vec![cast(1), cast(2), cast(3)]));
    enter(&mut h, "Privacy");
    h.key("G");
    assert_eq!(privacy_cursor(&h), (2, 3));
    h.send(privacy::Event::Casts(vec![cast(1)]));
    assert_eq!(privacy_cursor(&h), (0, 1));
    h.send(privacy::Event::Casts(vec![]));
    assert_eq!(privacy_cursor(&h).1, 0);
}

// ---- removable drives --------------------------------------------------------------------------

fn volume(path: &str, name: &str, mount: Option<&str>, locked: bool) -> drives::Volume {
    drives::Volume {
        path: path.into(),
        name: name.into(),
        size: 8_000_000_000,
        mount: mount.map(str::to_string),
        locked,
        backing: None,
    }
}

fn drive(path: &str, name: &str, volumes: Vec<drives::Volume>) -> drives::Drive {
    drives::Drive {
        path: path.into(),
        name: name.into(),
        size: 8_000_000_000,
        can_power_off: true,
        ejectable: true,
        volumes,
    }
}

fn two_drives() -> drives::State {
    drives::State {
        available: true,
        drives: vec![
            drive(
                "/d/stick",
                "Stick",
                vec![
                    volume("/dev/sda1", "STICK", Some("/run/media/me/STICK"), false),
                    volume("/dev/sda2", "SPARE", None, false),
                ],
            ),
            drive(
                "/d/card",
                "Card",
                vec![volume("/dev/sdb1", "CARD", None, false)],
            ),
        ],
    }
}

/// `j` gets past however many stops in a row can't be stopped at.
fn cursor_crosses_many_skipped_stops() {
    let mut h = show("removable-drives col=0 row=0 width=4 height=7");
    let mut state = Demo::new().drives;
    state.drives.truncate(1);
    // Locked volumes can't be stopped at; only the first and the last are open.
    state.drives[0].volumes = (0..8)
        .map(|i| {
            volume(
                &format!("/v{i}"),
                &format!("Volume {i}"),
                None,
                i != 0 && i != 7,
            )
        })
        .collect();
    h.send(drives::Event::State(state));
    enter(&mut h, "Removable drives");
    assert_eq!(drives_cursor(&h), (0, 8));
    h.key("j");
    assert_eq!(drives_cursor(&h).0, 7);
    h.key("k");
    assert_eq!(drives_cursor(&h).0, 0);
}

/// Enter opens a mounted volume (and the dashboard goes away, for the file manager) and mounts
/// one that isn't; u unmounts; x ejects its drive; X ejects all.
fn drives_keys() {
    let mut h = show("removable-drives col=0 row=0 width=4 height=5");
    h.send(drives::Event::State(two_drives()));
    enter(&mut h, "Removable drives");
    assert_eq!(drives_cursor(&h), (0, 3));
    h.key("enter");
    assert!(matches!(
        h.inbox.drives.try_recv(),
        Ok(drives::Command::Open(m)) if m == "/run/media/me/STICK"
    ));
    assert_eq!(h.controls(), [Control::Hide]);
    h.key("u");
    assert!(matches!(
        h.inbox.drives.try_recv(),
        Ok(drives::Command::Unmount(b)) if b == "/dev/sda1"
    ));
    h.key("j");
    h.key("enter");
    assert!(matches!(
        h.inbox.drives.try_recv(),
        Ok(drives::Command::Mount(b)) if b == "/dev/sda2"
    ));
    // Not mounted: nothing to unmount.
    h.key("u");
    assert!(h.inbox.drives.try_recv().is_err());
    h.key("j");
    h.key("x");
    assert!(matches!(
        h.inbox.drives.try_recv(),
        Ok(drives::Command::Eject(p)) if p == "/d/card"
    ));
    h.key("X");
    assert!(matches!(
        h.inbox.drives.try_recv(),
        Ok(drives::Command::EjectAll)
    ));
}

/// The buttons: Unmount is not a click on its row, Eject names its drive.
fn drives_clicks() {
    let mut h = show("removable-drives col=0 row=0 width=4 height=5");
    h.send(drives::Event::State(two_drives()));
    h.click_on("Unmount STICK");
    assert!(matches!(
        h.inbox.drives.try_recv(),
        Ok(drives::Command::Unmount(b)) if b == "/dev/sda1"
    ));
    assert!(
        h.inbox.drives.try_recv().is_err(),
        "the row was clicked too"
    );
    h.click_on("Eject Card");
    assert!(matches!(
        h.inbox.drives.try_recv(),
        Ok(drives::Command::Eject(p)) if p == "/d/card"
    ));
    h.click_on("Eject all");
    assert!(matches!(
        h.inbox.drives.try_recv(),
        Ok(drives::Command::EjectAll)
    ));
    h.click_on("CARD");
    assert!(matches!(
        h.inbox.drives.try_recv(),
        Ok(drives::Command::Mount(b)) if b == "/dev/sdb1"
    ));
    h.click_on("STICK");
    assert!(matches!(
        h.inbox.drives.try_recv(),
        Ok(drives::Command::Open(_))
    ));
    assert_eq!(h.controls(), [Control::Hide]);
}

/// One row: the summary, eject, and for one usable volume mount or open.
fn drives_compact() {
    let mut h = show("removable-drives col=0 row=0 width=4 height=1");
    // Nothing plugged in.
    h.send(drives::Event::State(drives::State {
        available: true,
        drives: vec![],
    }));
    assert!(!h.has("Eject"));
    let mut state = two_drives();
    state.drives.truncate(1);
    state.drives[0].volumes.truncate(1);
    h.send(drives::Event::State(state.clone()));
    h.click_on("Open folder");
    assert!(matches!(
        h.inbox.drives.try_recv(),
        Ok(drives::Command::Open(_))
    ));
    state.drives[0].volumes[0].mount = None;
    h.send(drives::Event::State(state));
    h.click_on("Mount");
    assert!(matches!(
        h.inbox.drives.try_recv(),
        Ok(drives::Command::Mount(b)) if b == "/dev/sda1"
    ));
    h.click_on("Eject");
    assert!(matches!(
        h.inbox.drives.try_recv(),
        Ok(drives::Command::Eject(p)) if p == "/d/stick"
    ));
    // Entered, the one row has no stops, but Enter mounts and x ejects.
    enter(&mut h, "Removable drives");
    assert_eq!(drives_cursor(&h).1, 0);
    h.key("enter");
    assert!(matches!(
        h.inbox.drives.try_recv(),
        Ok(drives::Command::Mount(_))
    ));
    h.key("x");
    assert!(matches!(
        h.inbox.drives.try_recv(),
        Ok(drives::Command::Eject(_))
    ));
}

/// A tall list of drives scrolls to the volume under the cursor.
fn drives_stops_in_sight() {
    let mut h = show("removable-drives col=0 row=0 width=3 height=2");
    let state = drives::State {
        available: true,
        drives: (0..4)
            .map(|i| {
                drive(
                    &format!("/d/{i}"),
                    &format!("Drive {i}"),
                    vec![volume(
                        &format!("/dev/sd{i}1"),
                        &format!("Vol {i}"),
                        None,
                        false,
                    )],
                )
            })
            .collect(),
    };
    h.send(drives::Event::State(state));
    enter(&mut h, "Removable drives");
    for i in 0..4 {
        let vol = name(format!("Vol {i}"));
        assert!(in_sight(&h, vol, "drives list"), "{vol} is out of sight");
        h.key("j");
    }
}

// ---- phone -------------------------------------------------------------------------------------

fn phone(id: &str, charge: Option<i32>, reachable: bool) -> phones::Phone {
    phones::Phone {
        id: id.into(),
        name: id.to_uppercase(),
        kind: "phone".into(),
        reachable,
        charge,
        charging: false,
        can_ring: true,
        can_share: true,
    }
}

fn phones_state(list: Vec<phones::Phone>) -> phones::Event {
    phones::Event::State(phones::State {
        available: true,
        phones: list,
    })
}

/// Ring and Send files, by click and by key; sending hides the dashboard (the file chooser
/// would be behind it).
fn phone_buttons_and_keys() {
    let mut h = show("phone col=0 row=0 width=4 height=2");
    h.send(phones_state(vec![phone("pixel", Some(82), true)]));
    h.click_on("Ring");
    assert!(matches!(
        h.inbox.phones.try_recv(),
        Ok(phones::Command::Ring(id)) if id == "pixel"
    ));
    h.click_on("Send files");
    assert!(matches!(
        h.inbox.phones.try_recv(),
        Ok(phones::Command::SendFiles(id)) if id == "pixel"
    ));
    assert_eq!(h.controls(), [Control::Hide]);
    enter(&mut h, "Phone");
    h.key("r");
    assert!(matches!(
        h.inbox.phones.try_recv(),
        Ok(phones::Command::Ring(_))
    ));
    h.key("u");
    assert!(matches!(
        h.inbox.phones.try_recv(),
        Ok(phones::Command::SendFiles(_))
    ));
    assert_eq!(h.controls(), [Control::Hide]);
}

/// A phone that can't ring or send doesn't, from the keys either.
fn phone_without_abilities() {
    let mut h = show("phone col=0 row=0 width=4 height=2");
    let mut p = phone("pixel", Some(50), true);
    p.can_ring = false;
    p.can_share = false;
    h.send(phones_state(vec![p]));
    enter(&mut h, "Phone");
    h.keys("r u");
    assert!(h.inbox.phones.try_recv().is_err());
    h.click_on("Ring");
    assert!(h.inbox.phones.try_recv().is_err());
}

/// Nothing paired: nothing to ring.
fn phone_none_paired() {
    let mut h = show("phone col=0 row=0 width=4 height=2");
    h.send(phones_state(vec![]));
    enter(&mut h, "Phone");
    h.keys("r u enter");
    assert!(h.inbox.phones.try_recv().is_err());
    // The one row says so too.
    let mut h = show("phone col=0 row=0 width=4 height=1");
    h.send(phones_state(vec![]));
    assert!(!h.has("Ring"));
}

/// With several paired and room, all are listed; j/k pick one, r and u act on it.
fn phone_list_of_all() {
    let mut h = show("phone col=0 row=0 width=4 height=3");
    h.send(phones_state(vec![
        phone("pixel", Some(82), true),
        phone("tablet", Some(15), true),
        phone("old", None, false),
    ]));
    enter(&mut h, "Phone");
    assert_eq!(phone_cursor(&h), (0, 3));
    h.keys("j r");
    assert!(matches!(
        h.inbox.phones.try_recv(),
        Ok(phones::Command::Ring(id)) if id == "tablet"
    ));
    h.click_on("Send files to OLD");
    assert!(matches!(
        h.inbox.phones.try_recv(),
        Ok(phones::Command::SendFiles(id)) if id == "old"
    ));
    h.key("G");
    assert_eq!(phone_cursor(&h).0, 2);
    // One goes away under the cursor.
    h.send(phones_state(vec![phone("pixel", Some(82), true)]));
    let (cursor, count) = phone_cursor(&h);
    assert!(cursor < count.max(1));
}

/// The ring (round) and the rest draw without trouble, for every state a phone can be in.
fn phone_draws() {
    for widget in [
        "phone col=0 row=0 size=2 shape=\"round\"",
        "phone col=0 row=0 width=3 height=1",
        "phone col=0 row=0 width=3 height=2",
        "phone col=0 row=0 width=4 height=4",
    ] {
        let mut h = show(widget);
        h.send(phones_state(vec![]));
        h.send(phones_state(vec![phone("pixel", None, true)]));
        h.send(phones_state(vec![
            phone("pixel", Some(10), true),
            phone("b", Some(100), false),
        ]));
        h.send(phone_unavailable());
    }
}

fn phone_unavailable() -> phones::Event {
    phones::Event::State(phones::State::default())
}

/// The empty line of a card wraps inside the card's padding and is centered, as Slint's `Empty`
/// is, instead of running out of the card in one line.
fn privacy_empty_line_wraps() {
    let h = show("privacy col=0 row=0 width=2 height=2");
    let (_, _, w, _) = h.read(|d| d.card_rect(d.tiles[0].id())).expect("the card");
    let b = h.bounds_of("empty state").expect("the empty line");
    assert!(
        f32::from(b.size.width) <= w - 2.0 * 16.0 + 0.5,
        "the line is {} px wide in a card {w} px wide",
        b.size.width
    );
    assert!(
        f32::from(b.size.height) >= 2.0 * 14.0,
        "the line is one row high, it did not wrap"
    );
}

/// The phone's ring row is as high as its ring and sits right under the header, as in Slint,
/// whatever the height of the card.
fn phone_ring_hugs_the_header() {
    let top = |rows: u32| {
        let mut h = show(&format!("phone col=0 row=0 width=3 height={rows}"));
        h.send(phones_state(vec![phone("pixel", Some(64), true)]));
        let (_, y, _, _) = h.read(|d| d.card_rect(d.tiles[0].id())).expect("the card");
        f32::from(h.bounds_of("Phone battery").expect("the ring").top()) - y
    };
    let (short, tall) = (top(2), top(3));
    assert!(
        (short - tall).abs() < 0.5,
        "the ring moved from {short} to {tall} px with the height"
    );
    // Padding 16, the header 28, the space 10.
    assert!(
        (tall - 54.0).abs() < 1.5,
        "the ring is {tall} px under the top of the card"
    );
}

/// A double click on a control is not a double click on the card behind it: the widget is not
/// picked up (the carry).
fn double_click_on_controls_does_not_carry() {
    let mut h = show("removable-drives col=0 row=0 width=4 height=5");
    h.send(drives::Event::State(two_drives()));
    for control in ["Unmount STICK", "Eject Card", "STICK"] {
        let c = h
            .bounds_of(control)
            .unwrap_or_else(|| panic!("nothing called {control:?}"))
            .center();
        h.double_click(f32::from(c.x), f32::from(c.y));
        assert_eq!(
            h.adjust_id(),
            0,
            "a double click on {control:?} picked the widget up"
        );
    }
    let mut h = show("phone col=0 row=0 width=4 height=2");
    h.send(phones_state(vec![phone("pixel", Some(82), true)]));
    for control in ["Ring", "Send files"] {
        let c = h
            .bounds_of(control)
            .unwrap_or_else(|| panic!("nothing called {control:?}"))
            .center();
        h.double_click(f32::from(c.x), f32::from(c.y));
        assert_eq!(
            h.adjust_id(),
            0,
            "a double click on {control:?} picked the widget up"
        );
    }
}

fn main() {
    let _sandbox = Sandbox::enter(None).expect("a temporary directory for the dashboard's files");
    let tests: &[(&str, fn())] = &[
        (
            "double click on controls does not carry",
            double_click_on_controls_does_not_carry,
        ),
        ("privacy: the empty line wraps", privacy_empty_line_wraps),
        (
            "phone: the ring hugs the header",
            phone_ring_hugs_the_header,
        ),
        ("privacy: the dots say what they watch", privacy_dots),
        (
            "privacy: no share, no stops",
            privacy_without_a_share_has_no_stops,
        ),
        (
            "privacy: the stops are the shares",
            privacy_stops_are_the_shares,
        ),
        ("privacy: Stop by click", privacy_stop_by_click),
        (
            "privacy: every stop in sight on a short tile",
            stops_in_sight_privacy,
        ),
        (
            "privacy: the cursor follows the list",
            privacy_cursor_follows_the_list,
        ),
        (
            "drives: the cursor crosses locked volumes",
            cursor_crosses_many_skipped_stops,
        ),
        ("drives: keys", drives_keys),
        ("drives: clicks", drives_clicks),
        ("drives: one row", drives_compact),
        ("drives: every volume in sight", drives_stops_in_sight),
        ("phone: buttons and keys", phone_buttons_and_keys),
        ("phone: can't ring, can't send", phone_without_abilities),
        ("phone: none paired", phone_none_paired),
        ("phone: all of them", phone_list_of_all),
        ("phone: draws in every shape", phone_draws),
    ];
    let filter = std::env::args().nth(1);
    let mut failed = 0;
    for (name, test) in tests {
        if filter.as_ref().is_some_and(|f| !name.contains(f.as_str())) {
            continue;
        }
        match std::panic::catch_unwind(AssertUnwindSafe(test)) {
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
        tests.len() - failed
    );
    if failed > 0 {
        std::process::exit(1);
    }
}

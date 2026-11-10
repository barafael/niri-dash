//! What a screen reader is given, and what it can make the dashboard do, without a bus: the
//! window's accessibility tree as GPUI builds it (what the AT-SPI adapter would publish), read
//! back by `Harness::a11y`, and the requests a screen reader makes of it delivered the way the
//! adapter delivers them. What reaches the real accessibility bus is `tests/atspi.rs`'s business;
//! nothing here may touch the session's bus.
//!
//! A plain program (`harness = false`): each scenario runs in turn on a fresh dashboard.

use niri_dash::testing::{Action, Harness, Role, Sandbox};
use niri_dash_system::{Demo, brightness, wifi};
use std::panic::AssertUnwindSafe;

const LAYOUT: &str = r#"
layout {
    wifi          col=0 row=0 width=3 height=3
    brightness    col=3 row=0 size=2 shape="round"
    session       col=0 row=3 width=4 height=1
}
"#;

fn dashboard() -> Harness {
    let mut h = Harness::new(LAYOUT);
    h.control(niri_dash::Control::Show);
    h.send(wifi::Event::State(Box::new(Demo::new().wifi)));
    h.send(brightness::Event::Level(Some(0.7)));
    h.advance(2);
    h
}

/// Into the widget that is `n`th on the dashboard: f, then its letters.
fn enter(h: &mut Harness, n: usize) {
    let hint = h
        .read(|d| d.tiles.get(n).map(|t| t.hint.clone()))
        .expect("a widget");
    h.key("f");
    for c in hint.chars() {
        h.type_text(&c.to_string());
    }
}

/// Nothing is built until a screen reader turns up; then it gets the whole tree, a window with
/// the widgets in it.
fn activation() {
    let mut h = dashboard();
    assert!(!h.a11y_connected(), "a tree built for no one");
    let tree = h.a11y();
    assert!(h.a11y_connected());
    assert_eq!(tree.window().map(|w| w.role), Some(Role::Window));
    assert!(tree.elements.len() > 20, "{} nodes", tree.elements.len());
    assert!(
        tree.find(Role::Switch, "Wi-Fi").is_some(),
        "the Wi-Fi switch is in the tree:\n{}",
        tree.dump()
    );
    assert!(
        tree.find(Role::Slider, "Brightness").is_some(),
        "the brightness knob is in the tree:\n{}",
        tree.dump()
    );
    let problems = tree.problems();
    assert!(problems.is_empty(), "{problems:?}\n{}", tree.dump());
}

/// A switch says whether it is on, a slider its value and range, a row what it stands for.
fn states() {
    let mut h = dashboard();
    let tree = h.a11y();
    let switch = tree.find(Role::Switch, "Wi-Fi").expect("the switch");
    assert_eq!(switch.on(), Some(true), "Wi-Fi is on: {}", switch.says());
    let knob = tree.find(Role::Slider, "Brightness").expect("the knob");
    assert_eq!(knob.numeric, Some(70.0));
    assert_eq!((knob.min, knob.max), (Some(0.0), Some(100.0)));
    assert!(knob.says().contains("70%"), "{}", knob.says());
    // A network row: its name, and how it stands as its description (AT-SPI has no string value
    // for a row; the description is what Orca reads after the name).
    let row = tree
        .find(Role::ListItem, "Lighthouse")
        .expect("the connected network");
    assert!(
        row.description.contains("connected") && row.description.contains("78%"),
        "the row says {:?} / {:?}",
        row.value,
        row.description
    );
    // What changes shows in the tree: Wi-Fi off.
    h.send(wifi::Event::State(Box::new({
        let mut w = Demo::new().wifi;
        w.enabled = false;
        w
    })));
    let switch = h.a11y_find(Role::Switch, "Wi-Fi").expect("the switch");
    assert_eq!(switch.on(), Some(false));
}

/// What a card only shows as text is in the tree as text, as Slint's text items were: the clock's
/// time and date, a gauge's figures.
fn text_is_read() {
    let mut h = Harness::with_mock("layout {\n    clock col=0 row=0 width=4 height=3\n    memory col=4 row=0 width=4 height=4\n}\n");
    h.advance(3);
    let tree = h.a11y();
    let texts: Vec<&str> = tree
        .elements
        .iter()
        .filter(|e| e.role == Role::Label)
        .map(|e| e.value.as_str())
        .collect();
    assert!(
        texts.iter().any(|t| t.contains(':') && t.chars().any(|c| c.is_ascii_digit())),
        "no time among the texts {texts:?}\n{}",
        tree.dump()
    );
    assert!(
        texts.iter().any(|t| t.contains("GiB") || t.contains("MiB")),
        "no memory figures among the texts {texts:?}"
    );
    assert!(tree.problems().is_empty(), "{:?}", tree.problems());
}

/// The same elements keep their ids when the dashboard is drawn again, and when what they say
/// changes, so a screen reader hears "this changed" rather than "this is new".
fn ids_hold() {
    let mut h = dashboard();
    let before = h.a11y();
    let knob = before.find(Role::Slider, "Brightness").expect("the knob").id;
    let switch = before.find(Role::Switch, "Wi-Fi").expect("the switch").id;
    h.send(brightness::Event::Level(Some(0.3)));
    h.advance(3);
    let after = h.a11y();
    let moved = after.find(Role::Slider, "Brightness").expect("the knob");
    assert_eq!(moved.id, knob, "the knob got another id");
    assert_eq!(moved.numeric, Some(30.0), "{}", moved.says());
    assert_eq!(
        after.find(Role::Switch, "Wi-Fi").map(|e| e.id),
        Some(switch),
        "the switch got another id"
    );
    assert_eq!(
        before.elements.len(),
        after.elements.len(),
        "a changed value rebuilt nodes"
    );
}

/// Standing still changes nothing: an unchanged dashboard builds the same tree every time.
fn quiet() {
    let mut h = dashboard();
    let first = h.a11y().dump();
    for _ in 0..5 {
        h.advance(10);
    }
    assert_eq!(first, h.a11y().dump(), "the tree changed on its own");
}

/// The tree's focus goes where the keyboard goes: one move per step, to what is spoken.
fn follows_the_keyboard() {
    let mut h = dashboard();
    let tree = h.a11y();
    assert!(tree.focus_is_root(), "the focus is on a widget before one is entered");
    enter(&mut h, 0);
    let mut moves = 0;
    let mut last = {
        let tree = h.a11y();
        let said = h
            .spoken_in(&tree)
            .expect("a cursor")
            .expect("a widget entered");
        let focus = tree.focused().map(|f| f.says()).unwrap_or_default();
        assert_eq!(
            focus, said,
            "after entering: the keyboard says {said}, the tree's focus is {focus}"
        );
        said
    };
    for step in 0..12 {
        h.key("j");
        let tree = h.a11y();
        let now = h
            .spoken_in(&tree)
            .expect("a cursor")
            .expect("a widget entered");
        let focus = tree.focused().map(|f| f.says()).unwrap_or_default();
        assert_eq!(
            focus, now,
            "step {step}: the keyboard says {now}, the tree's focus is {focus}"
        );
        if now != last {
            moves += 1;
        }
        last = now;
    }
    assert!(moves >= 2, "the cursor never moved ({moves})");
}

/// What a screen reader asks of an element is what a click or a key does.
fn actions() {
    let mut h = dashboard();
    h.a11y_do(Action::Click, Role::Switch, "Wi-Fi");
    h.settle();
    assert!(matches!(h.inbox.wifi.try_recv(), Ok(wifi::Command::Toggle)));

    h.a11y_do(Action::Increment, Role::Slider, "Brightness");
    h.settle();
    assert!(matches!(
        h.inbox.brightness.try_recv(),
        Ok(brightness::Command::Set(v)) if (v - 0.75).abs() < 0.001
    ));
    // One step down from where the knob stands (it shows what was just asked for, 75).
    h.a11y_do(Action::Decrement, Role::Slider, "Brightness");
    h.settle();
    let got = h.inbox.brightness.try_recv();
    assert!(
        matches!(&got, Ok(brightness::Command::Set(v)) if (v - 0.70).abs() < 0.001 || (v - 0.65).abs() < 0.001),
        "a step down gave {got:?}"
    );

    // An element that isn't there any more is not an error.
    h.a11y_request(Action::Click, u64::MAX);
}

/// Asking for focus on something in a widget enters the widget, as typing its letters does: the
/// keyboard goes there, and the tree's focus with it.
fn focus_enters_the_widget() {
    let mut h = dashboard();
    assert_eq!(h.spoken(), Ok(None), "a widget is entered already");
    h.a11y_do(Action::Focus, Role::Slider, "Brightness");
    let said = h.spoken().expect("a cursor").expect("a widget entered");
    assert!(said.contains("Brightness"), "the keyboard says {said}");
    let tree = h.a11y();
    assert_eq!(
        tree.focused().map(|f| f.name().to_string()).as_deref(),
        Some("Brightness"),
        "the tree's focus"
    );
    // And on the other widget.
    h.key("esc");
    h.a11y_do(Action::Focus, Role::Switch, "Wi-Fi");
    // The widget is entered, at the stop the keyboard has there (its first, a network): not the
    // switch itself, which isn't a stop of its own. The tree's focus says what the keyboard does.
    let said = h.spoken().expect("a cursor").expect("a widget entered");
    let tree = h.a11y();
    let focus = tree
        .focused()
        .map(|f| f.name().to_string())
        .unwrap_or_default();
    assert!(
        said.contains("Lighthouse") || said.contains("Wi-Fi"),
        "the keyboard isn't in the Wi-Fi widget: {said}"
    );
    assert!(
        said.contains(&format!("{focus:?}")),
        "the keyboard says {said}, the tree's focus is {focus:?}"
    );
}

/// A screen reader that leaves is let go; one that comes back gets a whole tree again.
fn leaving_and_coming_back() {
    let mut h = dashboard();
    h.a11y();
    h.a11y_disconnect();
    assert!(!h.a11y_connected(), "a tree for a screen reader that left");
    h.send(brightness::Event::Level(Some(0.2)));
    h.advance(3);
    assert!(!h.a11y_connected(), "a tree built for a screen reader that left");
    let tree = h.a11y();
    assert!(tree.elements.len() > 20);
    assert_eq!(
        tree.find(Role::Slider, "Brightness").and_then(|e| e.numeric),
        Some(20.0),
        "what changed while it was away"
    );
}

/// Every unnamed thing that can be acted on is a bug for someone who can't see it: the audit
/// (`tests/a11y.rs`) checks every widget; this checks the dashboard's own controls.
fn the_dashboard_itself() {
    let mut h = dashboard();
    let tree = h.a11y();
    assert!(
        tree.find(Role::Button, "Edit dashboard").is_some(),
        "no way to edit for a screen reader:\n{}",
        tree.dump()
    );
    assert!(
        tree.find(Role::Button, "Close dashboard").is_some(),
        "no way to close for a screen reader:\n{}",
        tree.dump()
    );
    // Closing from the tree does what clicking does.
    h.a11y_do(Action::Click, Role::Button, "Close dashboard");
    assert!(
        h.controls().contains(&niri_dash::Control::Hide),
        "the close button did nothing"
    );
}

fn main() {
    let _sandbox = Sandbox::enter(None).expect("a temporary directory for the dashboard's files");
    // SAFETY: nothing else runs yet that could read the environment.
    unsafe { std::env::set_var("LC_ALL", "C") };
    let scenarios: &[(&str, fn())] = &[
        ("access: activation", activation),
        ("access: states and values", states),
        ("access: text is read", text_is_read),
        ("access: ids hold", ids_hold),
        ("access: quiet when nothing changes", quiet),
        ("access: focus follows the keyboard", follows_the_keyboard),
        ("access: actions", actions),
        ("access: focus enters the widget", focus_enters_the_widget),
        ("access: leaving and coming back", leaving_and_coming_back),
        ("access: the dashboard's own controls", the_dashboard_itself),
    ];
    let only = std::env::var("NIRI_DASH_ACCESS_ONLY").unwrap_or_default();
    let mut failed = 0;
    let mut run = 0;
    for (name, scenario) in scenarios.iter().filter(|(n, _)| n.contains(only.as_str())) {
        run += 1;
        match std::panic::catch_unwind(AssertUnwindSafe(scenario)) {
            Ok(()) => println!("test {name} ... ok"),
            Err(e) => {
                failed += 1;
                let msg = e
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                println!("test {name} ... FAILED\n  {msg}");
            }
        }
    }
    println!(
        "\ntest result: {}. {} passed; {failed} failed",
        if failed == 0 { "ok" } else { "FAILED" },
        run - failed
    );
    if failed > 0 {
        std::process::exit(1);
    }
}

//! What a screen reader is given, and what it can make the dashboard do, without a bus: the
//! dashboard's sink for it is replaced by one that writes the trees down, and `accesskit_consumer`
//! (the code a platform adapter is built on) reads them back. What reaches the real accessibility
//! bus is `tests/atspi.rs`'s business; nothing here may touch the session's bus.
//!
//! Slint wants one thread and one platform per process, so this is a plain program
//! (`harness = false`) that runs each scenario in turn on a fresh dashboard.

use accesskit::{Action, ActionRequest, NodeId, Role, TreeId};
use accesskit_consumer::{Node, Tree, TreeChangeHandler};
use niri_dash::testing::{AccessEvent, Harness, Sandbox};
use niri_dash_system::{Demo, brightness, wifi};
use slint::Model;
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
    h.send(wifi::Event::State(Box::new(Demo::new().wifi)));
    h.send(brightness::Event::Level(Some(0.7)));
    h.settle(20);
    h
}

/// What a screen reader's adapter keeps of what it is given, and what it would say about the
/// changes.
#[derive(Default)]
struct Reader {
    tree: Option<Tree>,
    log: Log,
}

#[derive(Default)]
struct Log {
    /// The names focus moved to, in order.
    focus: Vec<String>,
    added: usize,
    updated: usize,
    removed: usize,
}

fn name(node: &Node) -> String {
    if node.role() == Role::Label {
        node.value().unwrap_or_default()
    } else {
        node.label().unwrap_or_default()
    }
}

impl TreeChangeHandler for Log {
    fn node_added(&mut self, _: &Node) {
        self.added += 1;
    }
    fn node_updated(&mut self, _: &Node, _: &Node) {
        self.updated += 1;
    }
    fn focus_moved(&mut self, _: Option<&Node>, new: Option<&Node>) {
        if let Some(n) = new {
            self.focus.push(name(n));
        }
    }
    fn node_removed(&mut self, _: &Node) {
        self.removed += 1;
    }
}

impl Reader {
    /// Take what the dashboard gave since last time. The first tree starts it.
    fn take(&mut self, h: &mut Harness) {
        self.feed(h.access_events());
    }

    fn feed(&mut self, events: Vec<AccessEvent>) {
        self.log = Log::default();
        for event in events {
            match (event, &mut self.tree) {
                (AccessEvent::Update(u), None) => self.tree = Some(Tree::new(u, false)),
                (AccessEvent::Update(u), Some(t)) => t.update_and_process_changes(u, &mut self.log),
                (AccessEvent::WindowFocus(f), Some(t)) => {
                    t.update_host_focus_state_and_process_changes(f, &mut self.log)
                }
                (AccessEvent::WindowFocus(_), None) => {}
            }
        }
    }

    fn find(&self, role: Role, label: &str) -> Option<NodeId> {
        let state = self.tree.as_ref()?.state();
        let mut stack = vec![state.root()];
        while let Some(n) = stack.pop() {
            if n.role() == role && name(&n) == label {
                return Some(n.locate().0);
            }
            stack.extend(n.children());
        }
        None
    }

    fn focus(&self) -> Option<String> {
        self.tree.as_ref()?.state().focus().map(|n| name(&n))
    }

    fn numeric_value(&self, id: NodeId) -> Option<f64> {
        let mut stack = vec![self.tree.as_ref()?.state().root()];
        while let Some(n) = stack.pop() {
            if n.locate().0 == id {
                return n.numeric_value();
            }
            stack.extend(n.children());
        }
        None
    }

    fn nodes(&self) -> usize {
        let Some(t) = &self.tree else { return 0 };
        let mut n = 0;
        let mut stack = vec![t.state().root()];
        while let Some(node) = stack.pop() {
            n += 1;
            stack.extend(node.children());
        }
        n
    }
}

fn request(action: Action, target: NodeId) -> ActionRequest {
    ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: target,
        data: None,
    }
}

/// Nothing is built or sent until a screen reader turns up; then it gets the whole tree, and is
/// told the window has the keyboard.
fn activation() {
    let mut h = dashboard();
    h.access_connect();
    h.settle(10);
    assert!(h.access_events().is_empty(), "sent before anyone asked");
    h.access_activate();
    let events = h.access_events();
    assert!(
        matches!(events.first(), Some(AccessEvent::Update(u)) if u.tree.is_some() && u.nodes.len() > 20),
        "the first thing is the whole tree"
    );
    assert!(
        matches!(events.last(), Some(AccessEvent::WindowFocus(true))),
        "told the window has the keyboard: {:?}",
        events.last()
    );
    let mut r = Reader::default();
    r.feed(events);
    assert_eq!(
        r.tree.as_ref().map(|t| t.state().root().role()),
        Some(Role::Window)
    );
    assert!(
        r.find(Role::Switch, "Wi-Fi").is_some(),
        "the Wi-Fi switch is in the tree"
    );
    assert!(
        r.find(Role::Slider, "Brightness").is_some(),
        "the brightness knob is in the tree"
    );
}

/// Standing still costs nothing: an unchanged dashboard sends nothing again.
fn quiet() {
    let mut h = dashboard();
    h.access_connect();
    h.access_activate();
    h.access_events();
    for _ in 0..10 {
        h.settle(60);
    }
    let sent = h.access_events();
    assert!(
        sent.iter().all(|e| !matches!(e, AccessEvent::Update(_))),
        "{} trees sent for nothing",
        sent.len()
    );
}

/// The tree's focus goes where the keyboard goes: one move per step, to what is spoken.
fn follows_the_keyboard() {
    let mut h = dashboard();
    h.access_connect();
    h.access_activate();
    let mut r = Reader::default();
    r.take(&mut h);
    // Into the Wi-Fi widget: f, then its letter.
    h.press("f");
    let hint = h.ui().get_widgets().row_data(0).expect("a widget").hint;
    for c in hint.chars() {
        h.press(&c.to_string());
    }
    h.settle(60);
    r.take(&mut h);
    let mut moves = 0;
    let mut last = h.spoken().expect("a cursor").expect("a widget entered");
    assert_eq!(
        r.focus()
            .map(|n| format!("{n:?}"))
            .filter(|n| last.contains(n)),
        r.focus().map(|n| format!("{n:?}")),
        "after entering: the keyboard says {last}, the tree's focus is {:?}",
        r.focus()
    );
    for step in 0..12 {
        h.press("j");
        h.settle(60);
        r.take(&mut h);
        let now = h.spoken().expect("a cursor").expect("a widget entered");
        let focus = r.focus().unwrap_or_default();
        assert!(
            now.contains(&format!("{focus:?}")),
            "step {step}: the keyboard says {now}, the tree's focus is {focus:?}"
        );
        if now != last {
            moves += 1;
            assert_eq!(
                r.log.focus.len(),
                1,
                "step {step}: the cursor moved to {now} and focus moved {:?}",
                r.log.focus
            );
        } else {
            assert!(
                r.log.focus.is_empty(),
                "step {step}: focus moved with the cursor still"
            );
        }
        last = now;
    }
    assert!(moves >= 2, "the cursor never moved ({moves})");
}

/// What a screen reader asks of an element is what a click or a key does.
fn actions() {
    let mut h = dashboard();
    h.access_connect();
    h.access_activate();
    let mut r = Reader::default();
    r.take(&mut h);

    let switch = r.find(Role::Switch, "Wi-Fi").expect("the Wi-Fi switch");
    h.access_do(request(Action::Click, switch));
    assert!(matches!(h.inbox.wifi.try_recv(), Ok(wifi::Command::Toggle)));

    let knob = r
        .find(Role::Slider, "Brightness")
        .expect("the brightness knob");
    h.access_do(request(Action::Increment, knob));
    assert!(matches!(
        h.inbox.brightness.try_recv(),
        Ok(brightness::Command::Set(v)) if (v - 0.75).abs() < 0.001
    ));

    // An element that isn't there any more is not an error.
    h.access_do(request(Action::Click, NodeId(u64::MAX)));
}

/// Asking for focus on something in a widget enters the widget, as typing its letters does: the
/// keyboard goes there, and the tree's focus with it.
fn focus_enters_the_widget() {
    let mut h = dashboard();
    h.access_connect();
    h.access_activate();
    let mut r = Reader::default();
    r.take(&mut h);
    assert_eq!(h.spoken(), Ok(None), "a widget is entered already");
    let knob = r.find(Role::Slider, "Brightness").expect("the knob");
    h.access_do(request(Action::Focus, knob));
    h.settle(80);
    r.take(&mut h);
    let said = h.spoken().expect("a cursor").expect("a widget entered");
    assert!(said.contains("Brightness"), "the keyboard says {said}");
    assert_eq!(r.focus().as_deref(), Some("Brightness"), "the tree's focus");
    // And on the other widget.
    let switch = r.find(Role::Switch, "Wi-Fi").expect("the switch");
    h.access_do(request(Action::Focus, switch));
    h.settle(80);
    r.take(&mut h);
    // The widget is entered, at the stop the keyboard has there (its first, a network): not the
    // switch itself, which isn't a stop of its own. The tree's focus says what the keyboard does.
    let said = h.spoken().expect("a cursor").expect("a widget entered");
    let focus = r.focus().unwrap_or_default();
    assert!(
        said.contains("Lighthouse") || said.contains("Wi-Fi"),
        "the keyboard isn't in the Wi-Fi widget: {said}"
    );
    assert!(
        said.contains(&format!("{focus:?}")),
        "the keyboard says {said}, the tree's focus is {focus:?}"
    );
}

/// What changed on the dashboard reaches the reader as a change to a node, not a new tree.
fn changes_arrive_as_changes() {
    let mut h = dashboard();
    h.access_connect();
    h.access_activate();
    let mut r = Reader::default();
    r.take(&mut h);
    let before = r.nodes();
    h.send(brightness::Event::Level(Some(0.3)));
    h.settle(120);
    r.take(&mut h);
    assert!(
        r.log.updated >= 1,
        "the knob's value didn't reach the reader"
    );
    assert_eq!(
        (r.log.added, r.log.removed),
        (0, 0),
        "a changed value rebuilt nodes"
    );
    assert_eq!(r.nodes(), before);
    let knob = r.find(Role::Slider, "Brightness").expect("the knob");
    let value = r.numeric_value(knob);
    assert!(
        value.is_some_and(|v| (v - 30.0).abs() < 0.5),
        "the knob says {value:?}"
    );
}

/// A screen reader that leaves is let go; one that comes back gets a whole tree again.
fn leaving_and_coming_back() {
    let mut h = dashboard();
    h.access_connect();
    h.access_activate();
    h.access_events();
    h.access_deactivate();
    h.send(brightness::Event::Level(Some(0.2)));
    h.settle(120);
    assert!(
        h.access_events().is_empty(),
        "sent to a screen reader that left"
    );
    h.access_activate();
    let events = h.access_events();
    assert!(
        matches!(events.first(), Some(AccessEvent::Update(u)) if u.tree.is_some()),
        "no whole tree on its return"
    );
}

fn main() {
    // A screen reader's sink here only writes down; the session's bus stays untouched.
    let _sandbox = Sandbox::enter(None).expect("a temporary directory for the dashboard's files");
    // SAFETY: nothing else runs yet that could read the environment.
    unsafe { std::env::set_var("LC_ALL", "C") };
    let scenarios: &[(&str, fn())] = &[
        ("access: activation", activation),
        ("access: quiet when nothing changes", quiet),
        ("access: focus follows the keyboard", follows_the_keyboard),
        ("access: actions", actions),
        ("access: focus enters the widget", focus_enters_the_widget),
        (
            "access: changes arrive as changes",
            changes_arrive_as_changes,
        ),
        ("access: leaving and coming back", leaving_and_coming_back),
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

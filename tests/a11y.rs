//! What a screen reader would say, widget by widget: each kind alone on the dashboard, in each of
//! its shapes, on the mock system's data, with the keyboard walked over all its stops and their
//! parts. Every stop has to have a name, and the cursor has to be on exactly one thing.
//!
//! `NIRI_DASH_A11Y_KIND=wifi` limits it to one kind; the transcript goes to standard output.

use accesskit::{Action, NodeId, Role, TreeUpdate};
use accesskit_consumer::Tree;
use niri_dash::testing::{Harness, Sandbox, can_be_round, kind_names, min_size};
use slint::Model;
use std::collections::HashSet;

/// More stops than any widget has, so a broken walk still ends.
const MOST_STOPS: usize = 80;

/// What a node says about itself: its name (a text says itself, in its value).
fn name_of(node: &accesskit_consumer::Node) -> String {
    if node.role() == Role::Label {
        node.value().unwrap_or_default()
    } else {
        node.label().unwrap_or_default()
    }
}

/// Every node of the tree under `node`, as the consumer sees them.
fn walk_tree<'a>(node: accesskit_consumer::Node<'a>, out: &mut Vec<accesskit_consumer::Node<'a>>) {
    out.push(node);
    for child in node.children() {
        walk_tree(child, out);
    }
}

/// What is wrong with the tree a screen reader is given now. `spoken` is what `Harness::spoken`
/// says for the keyboard's place, if a widget is entered: the tree's focus has to say the same.
fn tree_problems(h: &Harness, spoken: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    let update = h.access_tree();
    let count = update.nodes.len();
    let root = update.tree.as_ref().map(|t| t.root);
    let ids: Vec<NodeId> = update.nodes.iter().map(|(id, _)| *id).collect();
    // Building it is the first check: a child listed twice or a node nobody owns panics.
    let tree = match std::panic::catch_unwind(|| Tree::new(update, true)) {
        Ok(tree) => tree,
        Err(_) => return vec!["the tree doesn't build".to_string()],
    };
    let state = tree.state();
    if ids.iter().collect::<HashSet<_>>().len() != count {
        out.push("two nodes with one id".to_string());
    }
    let mut all = Vec::new();
    walk_tree(state.root(), &mut all);
    if all.len() != count {
        out.push(format!(
            "{count} nodes in the update, {} reachable from the root",
            all.len()
        ));
    }
    if root != Some(state.root().locate().0) {
        out.push("the root isn't the root".to_string());
    }
    if state.root().role() != Role::Window {
        out.push(format!(
            "the root is a {:?}, not a window",
            state.root().role()
        ));
    }
    // What can be acted on has a name, or a screen reader can only say "button".
    for node in &all {
        let data = node.data();
        let acts = data.supports_action(Action::Click)
            || data.supports_action(Action::Increment)
            || data.supports_action(Action::SetValue);
        if acts && !node.is_hidden() {
            let described = node.description().is_some_and(|d| !d.is_empty());
            if name_of(node).trim().is_empty() && !described {
                out.push(format!(
                    "a {:?} with no name (id {})",
                    node.role(),
                    node.locate().0.0
                ));
            }
        }
    }
    // The focus is where the keyboard is.
    match (spoken, state.focus()) {
        (Some(said), Some(node)) => {
            let name = name_of(&node);
            if !said.contains(&format!("{name:?}")) {
                out.push(format!(
                    "the keyboard says {said}, the tree's focus is a {:?} {name:?}",
                    node.role()
                ));
            }
            if node.id() == state.root().id() {
                out.push(format!(
                    "the keyboard is on {said}; the tree's focus is the window"
                ));
            }
        }
        (Some(said), None) => out.push(format!("the keyboard says {said}; the tree has no focus")),
        (None, _) => {}
    }
    out
}

/// What is wrong when the tree is built twice in a row: the same elements must keep the same ids.
fn unstable_ids(h: &Harness) -> Option<String> {
    let ids = |u: &TreeUpdate| -> Vec<(u64, String)> {
        let mut v: Vec<_> = u
            .nodes
            .iter()
            .map(|(id, n)| (id.0, format!("{:?} {:?}", n.role(), n.label())))
            .collect();
        v.sort();
        v
    };
    let (a, b) = (ids(&h.access_tree()), ids(&h.access_tree()));
    (a != b).then(|| "building the tree twice gave different ids".to_string())
}

/// The keyboard inside the only widget, walked from the first stop to the last. What the screen
/// reader says goes to `said`, what doesn't add up to `problems`.
fn walk(h: &mut Harness, said: &mut Vec<String>, problems: &mut Vec<String>) {
    let mut note = |h: &Harness, key: &str| match h.spoken() {
        Ok(Some(s)) => {
            for p in tree_problems(h, Some(&s)) {
                let p = format!("after {key}: {p}");
                if !problems.contains(&p) {
                    problems.push(p);
                }
            }
            if !said.contains(&s) {
                said.push(s);
            }
        }
        Ok(None) => problems.push(format!("after {key}: no longer in the widget")),
        Err(e) => {
            let e = format!("after {key}: {e}");
            if !problems.contains(&e) {
                problems.push(e);
            }
        }
    };
    h.press("g");
    h.settle(5);
    note(h, "g");
    let mut last = h.spoken();
    for _ in 0..MOST_STOPS {
        // The parts of this stop, if it has any, and back.
        h.press("l");
        h.settle(5);
        note(h, "l");
        h.press("h");
        h.settle(5);
        note(h, "h");
        h.press("j");
        h.settle(5);
        note(h, "j");
        let now = h.spoken();
        if now == last {
            break;
        }
        last = now;
    }
}

/// `kind` alone on the dashboard: its transcript and what's wrong with it.
fn audit(kind: &str, shape: &str) -> (Vec<String>, Vec<String>) {
    let config = format!("layout {{\n    {kind} col=0 row=0 {shape}\n}}\n");
    let mut h = Harness::with_mock(&config);
    // What the mock system has to say arrives on its own time.
    h.settle(30);
    let (mut said, mut problems) = (Vec::new(), Vec::new());
    let widgets = h.ui().get_widgets();
    let Some(w) = widgets.row_data(0) else {
        return (said, vec!["isn't on the dashboard".to_string()]);
    };
    h.press("f");
    for c in w.hint.chars() {
        h.press(&c.to_string());
    }
    h.settle(5);
    problems.extend(
        tree_problems(&h, None)
            .into_iter()
            .map(|p| format!("idle: {p}")),
    );
    problems.extend(unstable_ids(&h));
    walk(&mut h, &mut said, &mut problems);
    // The check has to be able to fail: a keyboard that says one thing and a tree that says
    // another is reported.
    if h.spoken().is_ok_and(|s| s.is_some())
        && tree_problems(&h, Some("button \"nothing like the focus\"")).is_empty()
    {
        problems.push("the tree check can't tell a wrong focus".to_string());
    }
    said.push(format!(
        "({} nodes in the tree)",
        h.access_tree().nodes.len()
    ));
    (said, problems)
}

fn main() {
    let _sandbox = Sandbox::enter(None).expect("a temporary directory for the dashboard's files");
    // SAFETY: nothing else runs yet that could read the environment.
    unsafe { std::env::set_var("LC_ALL", "C") };
    let only = std::env::var("NIRI_DASH_A11Y_KIND").ok();
    let mut bad = 0;
    let mut total = 0;
    for kind in kind_names() {
        if only.as_deref().is_some_and(|o| o != kind) {
            continue;
        }
        let (min_w, min_h) = min_size(kind);
        let mut shapes = vec![
            ("", "width=8 height=7".to_string()),
            (" (smallest)", format!("width={min_w} height={min_h}")),
        ];
        if can_be_round(kind) {
            shapes.push((" (round)", r#"size=4 shape="round""#.to_string()));
        }
        for (suffix, shape) in &shapes {
            total += 1;
            let name = format!("{kind}{suffix}");
            let (said, problems) = audit(kind, shape);
            println!("{name}:");
            for s in &said {
                println!("    {s}");
            }
            if !problems.is_empty() {
                bad += 1;
                for p in &problems {
                    println!("  PROBLEM {p}");
                }
            }
        }
    }
    println!(
        "\ntest result: {}. {} passed; {bad} failed",
        if bad == 0 { "ok" } else { "FAILED" },
        total - bad
    );
    if bad > 0 {
        std::process::exit(1);
    }
}

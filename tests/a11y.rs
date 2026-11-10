//! What a screen reader would say, widget by widget: each kind alone on the dashboard, in each of
//! its shapes, on the mock system's data, with the keyboard walked over all its stops and their
//! parts. Every stop has to have a name, and the cursor has to be on exactly one thing, which is
//! also where the screen reader's focus is. The accessibility tree is the one GPUI builds from the
//! views (`Harness::a11y`).
//!
//! `NIRI_DASH_A11Y_KIND=wifi` limits it to one kind; `NIRI_DASH_A11Y_DUMP=1` prints each kind's
//! tree; the transcript goes to standard output. A plain program (`harness = false`).

use niri_dash::testing::{Harness, Sandbox, Tree, can_be_round, kind_names, min_size};
use std::panic::AssertUnwindSafe;

/// More stops than any widget has, so a broken walk still ends.
const MOST_STOPS: usize = 80;

/// What is wrong with the tree a screen reader is given now. `spoken` is what `Harness::spoken`
/// says for the keyboard's place, if a widget is entered: the tree's focus has to say the same.
fn tree_problems(tree: &Tree, spoken: Option<&str>) -> Vec<String> {
    let mut out = tree.problems();
    // The focus is where the keyboard is.
    if let Some(said) = spoken {
        match tree.focused() {
            Some(node) if !tree.focus_is_root() => {
                let name = node.name();
                if !said.contains(&format!("{name:?}")) {
                    out.push(format!(
                        "the keyboard says {said}, the tree's focus is a {:?} {name:?}",
                        node.role
                    ));
                }
            }
            _ => out.push(format!(
                "the keyboard is on {said}; the tree's focus is the window"
            )),
        }
    }
    out
}

/// What is wrong when the tree is built twice in a row: the same elements must keep the same ids.
fn unstable_ids(h: &mut Harness) -> Option<String> {
    let ids = |t: &Tree| -> Vec<(u64, String)> {
        let mut v: Vec<_> = t
            .elements
            .iter()
            .map(|e| (e.id, format!("{:?} {:?}", e.role, e.label)))
            .collect();
        v.sort();
        v
    };
    let a = ids(&h.a11y());
    h.draw();
    let b = ids(&h.a11y());
    (a != b).then(|| "building the tree twice gave different ids".to_string())
}

/// The keyboard inside the only widget, walked from the first stop to the last. What the screen
/// reader says goes to `said`, what doesn't add up to `problems`.
fn walk(h: &mut Harness, said: &mut Vec<String>, problems: &mut Vec<String>) {
    let mut note = |h: &mut Harness, key: &str| {
        let tree = h.a11y();
        match h.spoken_in(&tree) {
            Ok(Some(s)) => {
                for p in tree_problems(&tree, Some(&s)) {
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
        }
    };
    h.key("g");
    note(h, "g");
    let mut last = h.spoken();
    for _ in 0..MOST_STOPS {
        // The parts of this stop, if it has any, and back.
        h.key("l");
        note(h, "l");
        h.key("h");
        note(h, "h");
        h.key("j");
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
    h.advance(3);
    let (mut said, mut problems) = (Vec::new(), Vec::new());
    let Some(hint) = h.read(|d| d.tiles.first().map(|t| t.hint.clone())) else {
        return (said, vec!["isn't on the dashboard".to_string()]);
    };
    h.key("f");
    for c in hint.chars() {
        h.type_text(&c.to_string());
    }
    let tree = h.a11y();
    if std::env::var_os("NIRI_DASH_A11Y_DUMP").is_some() {
        println!("{}", tree.dump());
    }
    problems.extend(
        tree_problems(&tree, None)
            .into_iter()
            .map(|p| format!("idle: {p}")),
    );
    problems.extend(unstable_ids(&mut h));
    walk(&mut h, &mut said, &mut problems);
    // The check has to be able to fail: a keyboard that says one thing and a tree that says
    // another is reported.
    let tree = h.a11y();
    if h.spoken().is_ok_and(|s| s.is_some())
        && tree_problems(&tree, Some("button \"nothing like the focus\"")).is_empty()
    {
        problems.push("the tree check can't tell a wrong focus".to_string());
    }
    said.push(format!("({} nodes in the tree)", tree.elements.len()));
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
            // GPUI panics (in a debug build) when two elements claim the cursor in one frame.
            let (said, problems) = std::panic::catch_unwind(AssertUnwindSafe(|| audit(kind, shape)))
                .unwrap_or_else(|e| {
                    let msg = e
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                        .unwrap_or_default();
                    (Vec::new(), vec![format!("panicked: {msg}")])
                });
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

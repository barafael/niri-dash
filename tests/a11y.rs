//! What a screen reader would say, widget by widget: each kind alone on the dashboard, in each of
//! its shapes, on the mock system's data, with the keyboard walked over all its stops and their
//! parts. Every stop has to have a name, and the cursor has to be on exactly one thing.
//!
//! `NIRI_DASH_A11Y_KIND=wifi` limits it to one kind; the transcript goes to standard output.

use niri_dash::testing::{Harness, Sandbox, can_be_round, kind_names, min_size};
use slint::Model;

/// More stops than any widget has, so a broken walk still ends.
const MOST_STOPS: usize = 80;

/// The keyboard inside the only widget, walked from the first stop to the last. What the screen
/// reader says goes to `said`, what doesn't add up to `problems`.
fn walk(h: &mut Harness, said: &mut Vec<String>, problems: &mut Vec<String>) {
    let mut note = |h: &Harness, key: &str| match h.spoken() {
        Ok(Some(s)) => {
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
    walk(&mut h, &mut said, &mut problems);
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

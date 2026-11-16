//! The quick tips against the keys, widget by widget: each kind alone on the dashboard, in each of
//! its shapes, on the mock system's data. Entered, a widget's tips have to fit on their line, and
//! a key they don't list must do nothing: not move the cursor, not change what the widget says.
//!
//! `NIRI_DASH_KEYS_KIND=wifi` limits it to one kind; what each widget's tips say goes to standard
//! output.

use niri_dash::testing::{Harness, Sandbox, can_be_round, kind_names, min_size};
use slint::{ComponentHandle, Model};

/// The dashboard's own keys while a widget is entered: these do something, and its line of the
/// tips says what.
const DASHBOARD: &str = "esfHJKL";

/// Keys to try that a widget might take.
const TRIED: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+-=_/[]<>.,;' ";

/// What the tips say the entered widget's keys do, as (keys, does).
fn tips(h: &Harness) -> Vec<(String, String)> {
    h.ui()
        .global::<niri_dash::Nav>()
        .get_widget_tips()
        .iter()
        .map(|t| (t.key.to_string(), t.does.to_string()))
        .collect()
}

/// Whether the tips list this key. Space does what Enter does unless it is listed itself, and a
/// tip without keys ("Type to write") says that every key is the widget's.
fn listed(tips: &[(String, String)], key: char) -> bool {
    tips.iter().any(|(keys, _)| {
        keys.is_empty()
            || keys.split(' ').any(|k| match k {
                "Space" | "Enter" => key == ' ',
                "0-9" => key.is_ascii_digit(),
                k => k.len() == 1 && k.starts_with(key),
            })
    })
}

/// What the widget shows and where the keyboard is.
fn state(h: &Harness) -> (Result<Option<String>, String>, Vec<String>) {
    (h.spoken(), h.readable())
}

/// `kind` alone on a dashboard of this size: what its tips say, and what's wrong.
fn audit(kind: &str, shape: &str, size: (u32, u32)) -> (Vec<String>, Vec<String>) {
    let config = format!("layout {{\n    {kind} col=0 row=0 {shape}\n}}\n");
    let mut h = Harness::with_mock_sized(&config, size);
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
    let shown = tips(&h);
    said.push(
        shown
            .iter()
            .map(|(k, d)| format!("{k} {d}"))
            .collect::<Vec<_>>()
            .join(" | "),
    );
    // (The rest of what `problems` checks is about config.kdl, which this dashboard doesn't have.)
    problems.extend(
        h.problems()
            .into_iter()
            .filter(|p| p.starts_with("the quick tips")),
    );
    for (keys, does) in &shown {
        if does.is_empty() {
            problems.push(format!("{keys:?} is listed without what it does"));
        }
    }
    for key in TRIED.chars() {
        if DASHBOARD.contains(key) || listed(&tips(&h), key) {
            continue;
        }
        let before = state(&h);
        h.key(key.to_string());
        h.settle(2);
        if state(&h) != before {
            problems.push(format!("{key:?} isn't in the tips but does something"));
            break;
        }
    }
    (said, problems)
}

fn main() {
    let _sandbox = Sandbox::enter(None).expect("a temporary directory for the dashboard's files");
    // SAFETY: nothing else runs yet that could read the environment.
    unsafe { std::env::set_var("LC_ALL", "C") };
    let only = std::env::var("NIRI_DASH_KEYS_KIND").ok();
    let (mut bad, mut total) = (0, 0);
    for kind in kind_names() {
        if only.as_deref().is_some_and(|o| o != kind) {
            continue;
        }
        let (min_w, min_h) = min_size(kind);
        let mut shapes = vec![
            ("", "width=8 height=7".to_string(), (1706, 1066)),
            (
                " (smallest, small screen)",
                format!("width={min_w} height={min_h}"),
                (1280, 800),
            ),
        ];
        if can_be_round(kind) {
            shapes.push((
                " (round)",
                r#"size=4 shape="round""#.to_string(),
                (1706, 1066),
            ));
        }
        for (suffix, shape, size) in &shapes {
            total += 1;
            let (said, problems) = audit(kind, shape, *size);
            println!("{kind}{suffix}:");
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

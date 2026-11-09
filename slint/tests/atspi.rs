//! The dashboard on a real accessibility bus, read by libatspi (the library Orca is built on):
//! what the tests in `access.rs` check with a recording stand-in, checked against the real thing.
//!
//! It never touches the session's bus. This program starts itself again under `dbus-run-session`,
//! a private session bus whose own accessibility bus and registry are started on demand, in a
//! temporary runtime directory; only that second run publishes anything, and only because it was
//! told it is on a private bus (`NIRI_DASH_PRIVATE_BUS`; the harness refuses to publish without
//! it). It starts programs (a bus, a screen reader's client), so it runs in the test box only
//! (`cargo testbox test atspi`); anywhere else it says so and passes.
//!
//! Slint wants one thread and one platform per process, so this is a plain program
//! (`harness = false`).

mod bus;
#[path = "../../testbox/inside.rs"]
mod inside;

use niri_dash::testing::{Harness, Sandbox};
use niri_dash_system::{Demo, brightness, wifi};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const LAYOUT: &str = r#"
layout {
    wifi          col=0 row=0 width=3 height=3
    brightness    col=3 row=0 size=2 shape="round"
    session       col=0 row=3 width=4 height=1
}
"#;

/// The screen reader's client, beside this program: the box has the test binaries and what they
/// run, not the source tree (`cargo testbox` puts the file there).
fn client() -> std::path::PathBuf {
    std::env::current_exe()
        .expect("this program's path")
        .with_file_name("atspi_client.py")
}

fn main() {
    if !inside::inside() {
        inside::skipped("atspi");
        return;
    }
    if std::env::var_os("NIRI_DASH_PRIVATE_BUS").is_some() {
        return inner();
    }
    // The box has what these need, so missing it is a failure, not a reason to skip.
    if let Some(what) = bus::missing(&[]) {
        println!("test atspi: FAILED, {what} isn't in the box\n\ntest result: FAILED");
        std::process::exit(1);
    }
    if !bus::rerun_privately("atspi") {
        std::process::exit(1);
    }
}

/// Under the private bus: the dashboard publishes, a screen reader reads and acts on it.
fn inner() {
    let _sandbox = Sandbox::enter(None).expect("a temporary directory for the dashboard's files");
    // SAFETY: nothing else runs yet that could read the environment.
    unsafe { std::env::set_var("LC_ALL", "C") };
    let mut h = Harness::new(LAYOUT);
    h.send(wifi::Event::State(Box::new(Demo::new().wifi)));
    h.send(brightness::Event::Level(Some(0.7)));
    h.settle(20);
    h.access_connect_unix();

    // A desktop turns accessibility on when a screen reader starts; this is that.
    bus::accessibility_on();

    let exe = std::env::current_exe().expect("this program's path");
    let prefix = exe
        .file_name()
        .map(|n| n.to_string_lossy().chars().take(6).collect::<String>())
        .unwrap_or_default();
    let mut client = Command::new("python3")
        .arg(client())
        .arg(&prefix)
        .stdout(Stdio::piped())
        .spawn()
        .expect("starting the screen reader");
    // The dashboard goes on with its loop while the reader reads it.
    let start = Instant::now();
    while client.try_wait().expect("the screen reader").is_none() {
        assert!(
            start.elapsed() < Duration::from_secs(90),
            "the screen reader didn't finish"
        );
        h.settle(20);
    }
    h.settle(100);
    let out = client.wait_with_output().expect("its output");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "the screen reader failed: {text}");
    let found: serde_json::Value = serde_json::from_str(text.trim()).expect("JSON from the reader");
    println!("what libatspi found: {found}");

    let mut problems = Vec::new();
    let nodes = found["nodes"].as_u64().unwrap_or(0);
    if nodes < 20 {
        problems.push(format!("only {nodes} nodes on the bus"));
    }
    // libatspi's names: a switch is a "toggle button".
    let roles = found["roles"].as_array().cloned().unwrap_or_default();
    for role in ["toggle button", "slider", "frame", "list item"] {
        if !roles.iter().any(|r| r == role) {
            problems.push(format!("no {role} among the roles {roles:?}"));
        }
    }
    // On is "pressed" for libatspi's toggle buttons.
    if !found["switch_states"]
        .as_array()
        .is_some_and(|s| s.iter().any(|v| v == "pressed"))
    {
        problems.push(format!("Wi-Fi isn't on: {}", found["switch_states"]));
    }
    if found["did_action"] != true {
        problems.push(format!("the switch's action: {}", found["did_action"]));
    }
    // What libatspi did arrived as what a click does.
    if !matches!(h.inbox.wifi.try_recv(), Ok(wifi::Command::Toggle)) {
        problems.push("the action on the switch didn't toggle Wi-Fi".into());
    }
    let slider = &found["slider"];
    let value = slider["value"].as_f64();
    if !value.is_some_and(|v| (v - 70.0).abs() < 0.5) || slider["max"].as_f64() != Some(100.0) {
        problems.push(format!("the brightness knob reads {slider}"));
    }
    if found["grabbed_focus"] != true {
        problems.push(format!("grab_focus: {}", found["grabbed_focus"]));
    }
    if !found["slider_states_after_grab"]
        .as_array()
        .is_some_and(|s| s.iter().any(|v| v == "focused"))
    {
        problems.push(format!(
            "the knob isn't focused after grab_focus: {}",
            found["slider_states_after_grab"]
        ));
    }
    if problems.is_empty() {
        println!("test atspi: a screen reader reads and drives the dashboard ... ok");
        println!("\ntest result: ok. 1 passed; 0 failed");
    } else {
        println!("test atspi: a screen reader reads and drives the dashboard ... FAILED");
        for p in &problems {
            println!("  - {p}");
        }
        println!("\ntest result: FAILED. 0 passed; 1 failed");
        std::process::exit(1);
    }
}

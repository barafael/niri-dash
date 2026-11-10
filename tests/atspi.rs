//! The dashboard on a real accessibility bus, read by libatspi (the library Orca is built on):
//! what `access.rs` checks in the process, checked against the real thing.
//!
//! GPUI publishes the tree itself, from the window's own AccessKit adapter, so this starts the
//! dashboard's program (`niri-dash daemon --show`, on the mock system) on a virtual KWin, in a
//! private session bus, and reads it from outside, as a screen reader does. It never touches the
//! session's bus: this program starts itself again under `dbus-run-session`, a private session bus
//! whose own accessibility bus and registry are started on demand, in a temporary runtime
//! directory (`tests/bus/mod.rs`). It starts programs, so it runs in the test box only
//! (`cargo testbox test atspi`); anywhere else it says so and passes.
//!
//! A plain program (`harness = false`).

mod bus;
#[path = "../testbox/inside.rs"]
mod inside;

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
    let beside = std::env::current_exe()
        .expect("this program's path")
        .with_file_name("atspi_client.py");
    if beside.exists() {
        beside
    } else {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/atspi_client.py")
    }
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
    if let Some(what) = bus::missing(&["kwin_wayland"]) {
        println!("test atspi: FAILED, {what} isn't in the box\n\ntest result: FAILED");
        std::process::exit(1);
    }
    if !bus::rerun_privately("atspi") {
        std::process::exit(1);
    }
}

/// Under the private bus: the dashboard publishes, a screen reader reads and acts on it.
fn inner() {
    let dir = std::env::temp_dir().join(format!("niri-dash-atspi-files-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory for the files");
    let _kwin = bus::compositor(&dir);
    // A desktop turns accessibility on when a screen reader starts; this is that. The dashboard's
    // adapter notices, whichever comes first.
    bus::accessibility_on();
    let mut dash = bus::dashboard(&dir, LAYOUT);

    let mut client = Command::new("python3")
        .arg(client())
        .arg("niri-dash")
        .stdout(Stdio::piped())
        .spawn()
        .expect("starting the screen reader");
    let start = Instant::now();
    while client.try_wait().expect("the screen reader").is_none() {
        if start.elapsed() > Duration::from_secs(120) {
            let _ = client.kill();
            panic!(
                "the screen reader didn't finish. The dashboard's log ends:\n{}",
                bus::dashboard_log(&dir)
            );
        }
        if let Some(status) = dash.exited() {
            let _ = client.kill();
            panic!(
                "the dashboard exited ({status}). Its log ends:\n{}",
                bus::dashboard_log(&dir)
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let out = client.wait_with_output().expect("its output");
    let text = String::from_utf8_lossy(&out.stdout);
    let found: serde_json::Value = match serde_json::from_str(text.trim()) {
        Ok(found) => found,
        Err(_) => panic!(
            "no JSON from the screen reader: {text}\nThe dashboard's log ends:\n{}",
            bus::dashboard_log(&dir)
        ),
    };
    println!("what libatspi found: {found}");
    if out.status.code() != Some(0) {
        println!("{}", bus::dashboard_log(&dir));
        println!("\ntest atspi: FAILED, the screen reader failed: {found}\n\ntest result: FAILED");
        std::process::exit(1);
    }

    let mut problems = Vec::new();
    let nodes = found["nodes"].as_u64().unwrap_or(0);
    if nodes < 20 {
        problems.push(format!("only {nodes} nodes on the bus"));
    }
    // libatspi's names: a switch is a "toggle button".
    let roles = found["roles"].as_array().cloned().unwrap_or_default();
    for role in ["toggle button", "slider", "frame", "button", "list item"] {
        if !roles.iter().any(|r| r == role) {
            problems.push(format!("no {role} among the roles {roles:?}"));
        }
    }
    // On is "pressed" for libatspi's toggle buttons: the mock's Wi-Fi is on.
    if !found["switch_states"]
        .as_array()
        .is_some_and(|s| s.iter().any(|v| v == "pressed"))
    {
        problems.push(format!("Wi-Fi isn't on: {}", found["switch_states"]));
    }
    if found["did_action"] != true {
        problems.push(format!("the switch's action: {}", found["did_action"]));
    }
    // What libatspi did arrived as what a click does: the mock turned Wi-Fi off.
    if found["switch_states_after"]
        .as_array()
        .is_none_or(|s| s.iter().any(|v| v == "pressed"))
    {
        problems.push(format!(
            "the action on the switch didn't turn Wi-Fi off: {}",
            found["switch_states_after"]
        ));
    }
    let slider = &found["slider"];
    let value = slider["value"].as_f64();
    if value.is_none_or(|v| !(0.0..=100.0).contains(&v)) || slider["max"].as_f64() != Some(100.0) {
        problems.push(format!("the brightness knob reads {slider}"));
    }
    // Asking for the focus on the knob: the Slint version entered the widget and put the screen
    // reader's focus on it. GPUI hands a `Focus` request that nothing answers to no one (a patched
    // copy of it handed it to the widget's frame), so this is reported, not required.
    let focused = found["slider_states_after_grab"]
        .as_array()
        .is_some_and(|s| s.iter().any(|v| v == "focused"));
    println!(
        "note: grab_focus on the brightness knob: {} (focused afterwards: {focused}); \
         the Slint version focused it",
        found["grabbed_focus"]
    );
    let _ = std::fs::remove_dir_all(&dir);
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

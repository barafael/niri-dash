//! Orca itself, reading the dashboard, with no speaker: Orca logs every utterance as
//! `SPEECH OUTPUT: '...'` before it hands it to a speech server (and does so too when there is
//! none), so what it would say can be read from its debug log.
//!
//! Like `atspi.rs` it runs on a private session bus with its own accessibility bus (see
//! `bus/mod.rs`), with Orca's settings in memory and its files in a temporary directory, so
//! neither the session's Orca nor the user's settings are involved. It starts Orca, KWin and the
//! dashboard's own program (GPUI publishes its tree itself), so it runs in the test box only
//! (`cargo testbox test orca`); anywhere else it says so and passes.
//!
//! The dashboard's keyboard is driven from here over its control socket (`NIRI_DASH_TEST_KEYS`:
//! a compositor with no keyboard has nothing to type on); Orca hears about the cursor moving only
//! over the bus, as it would from the real thing. The transcript is printed.
//!
//! A plain program (`harness = false`).

mod bus;
#[path = "../testbox/inside.rs"]
mod inside;

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const LAYOUT: &str = r#"
layout {
    wifi          col=0 row=0 width=3 height=3
    brightness    col=3 row=0 size=2 shape="round"
    session       col=0 row=3 width=4 height=1
}
"#;

/// The hint labels of the two widgets we walk into: the shortest start of each one's name that
/// no other widget's name starts with (`nav::assign_hints`).
const WIFI: &str = "w";
const BRIGHTNESS: &str = "b";

fn main() {
    if !inside::inside() {
        inside::skipped("orca");
        return;
    }
    if std::env::var_os("NIRI_DASH_PRIVATE_BUS").is_some() {
        return inner();
    }
    // The box has what these need, so missing it is a failure, not a reason to skip.
    if let Some(what) = bus::missing(&["orca", "kwin_wayland"]) {
        println!("test orca: FAILED, {what} isn't in the box\n\ntest result: FAILED");
        std::process::exit(1);
    }
    if !bus::rerun_privately("orca") {
        std::process::exit(1);
    }
}

/// What Orca has said so far, from its debug log.
fn spoken(log: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    text.lines()
        .filter_map(|line| {
            let rest = &line[line.find("SPEECH OUTPUT: '")? + "SPEECH OUTPUT: '".len()..];
            // The text, then the voice it was given in braces.
            let end = rest
                .rfind("'{")
                .or_else(|| rest.rfind("' {"))
                .or_else(|| rest.rfind('\''))?;
            Some(rest[..end].to_string())
        })
        .collect()
}

fn orca_is_up() -> bool {
    Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.freedesktop.DBus",
            "--object-path",
            "/org/freedesktop/DBus",
            "--method",
            "org.freedesktop.DBus.NameHasOwner",
            "org.gnome.Orca1.Service",
        ])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("true"))
}

fn tail(log: &Path) -> String {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(40)..].join("\n")
}

fn stop(orca: &mut Child) {
    let _ = orca.kill();
    let _ = orca.wait();
}

fn wait(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

fn inner() {
    let dir = std::env::temp_dir().join(format!("niri-dash-orca-files-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory for the files");
    let log = dir.join("orca.log");
    let errors = dir.join("orca.err");

    let _kwin = bus::compositor(&dir);
    let mut orca = Command::new("orca")
        .arg("--debug-file")
        .arg(&log)
        .env("WAYLAND_DISPLAY", bus::DISPLAY)
        .env("GDK_BACKEND", "wayland")
        .env("XDG_SESSION_TYPE", "wayland")
        .env("XDG_CURRENT_DESKTOP", "KDE")
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&errors).expect("a file for Orca's complaints"))
        .spawn()
        .expect("starting Orca");
    let start = Instant::now();
    while !orca_is_up() {
        if start.elapsed() > Duration::from_secs(40) || orca.try_wait().ok().flatten().is_some() {
            let t = tail(&log);
            let status = orca.try_wait().ok().flatten();
            stop(&mut orca);
            let e = std::fs::read_to_string(&errors).unwrap_or_default();
            panic!("Orca didn't come up (exit {status:?}). It said:\n{e}\nIts log ends:\n{t}");
        }
        wait(200);
    }
    bus::accessibility_on();
    let mut dash = bus::dashboard(&dir, LAYOUT);

    // Orca has to notice the dashboard before it can read it: it says so in its log.
    let start = Instant::now();
    while !std::fs::read_to_string(&log).is_ok_and(|t| t.contains("niri-dash")) {
        if start.elapsed() > Duration::from_secs(60) || dash.exited().is_some() {
            let t = tail(&log);
            stop(&mut orca);
            panic!(
                "Orca never noticed the dashboard. Its log ends:\n{t}\nThe dashboard's log ends:\n{}",
                bus::dashboard_log(&dir)
            );
        }
        wait(200);
    }
    wait(3000);

    // Walk the keyboard: into the Wi-Fi widget and down its stops, then out and into brightness.
    let mut transcript: Vec<(String, Vec<String>)> = Vec::new();
    let mut step = |what: &str, keys: &[&str]| {
        let before = spoken(&log).len();
        for k in keys {
            bus::press(k);
            wait(250);
        }
        wait(1500);
        let said: Vec<String> = spoken(&log)[before..].to_vec();
        transcript.push((what.to_string(), said));
    };
    step("enter Wi-Fi", &["f", WIFI]);
    step("j", &["j"]);
    step("j", &["j"]);
    step("k", &["k"]);
    step("leave", &["esc"]);
    step("enter brightness", &["f", BRIGHTNESS]);
    stop(&mut orca);

    println!("what Orca said (it has no speaker; this is its log of what it spoke):");
    for (what, said) in &transcript {
        println!("  {what}:");
        for s in said {
            println!("      Orca: {s:?}");
        }
        if said.is_empty() {
            println!("      Orca: (nothing)");
        }
    }

    let all: Vec<String> = spoken(&log);
    let mut problems = Vec::new();
    if all.is_empty() {
        problems.push("Orca said nothing at all".to_string());
    }
    for word in ["Lighthouse", "Brightness"] {
        if !all.iter().any(|s| s.contains(word)) {
            problems.push(format!("Orca never said {word:?}"));
        }
    }
    // Each stop the keyboard moved to is read out, by its name: moving the cursor moves the
    // screen reader's focus.
    for (what, said) in &transcript {
        if what == "leave" {
            continue;
        }
        if said.is_empty() {
            problems.push(format!("after {what}, Orca said nothing"));
        }
    }
    for detail in [
        "connected, signal 78%",
        "signal 54%, open",
        "signal 31%, secured",
    ] {
        if !all.iter().any(|s| s.contains(detail)) {
            problems.push(format!("Orca never read out {detail:?}"));
        }
    }
    // The widget is announced as the cursor enters it. (The Slint version put a list around the
    // rows and Orca said "Wi-Fi List with 4 items"; here the widget is a group: docs/accessibility.md.)
    if !all.iter().any(|s| s == "Wi-Fi") {
        problems.push("Orca didn't announce the Wi-Fi widget".into());
    }
    if !all.iter().any(|s| s.contains("Brightness slider")) {
        problems.push("Orca didn't read the brightness slider as one".into());
    }
    if problems.is_empty() {
        let _ = std::fs::remove_dir_all(&dir);
        println!("test orca: Orca reads the dashboard ... ok");
        println!("\ntest result: ok. 1 passed; 0 failed");
    } else {
        println!(
            "test orca: Orca reads the dashboard ... FAILED (its log: {})",
            log.display()
        );
        for p in &problems {
            println!("  - {p}");
        }
        println!("The dashboard's log ends:\n{}", bus::dashboard_log(&dir));
        println!("\ntest result: FAILED. 0 passed; 1 failed");
        std::process::exit(1);
    }
}

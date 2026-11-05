//! Orca itself, reading the dashboard, with no speaker: Orca logs every utterance as
//! `SPEECH OUTPUT: '...'` before it hands it to a speech server (and does so too when there is
//! none), so what it would say can be read from its debug log.
//!
//! Like `atspi.rs` it runs on a private session bus with its own accessibility bus (see
//! `bus/mod.rs`), with Orca's settings in memory and its files in a temporary directory, so
//! neither the session's Orca nor the user's settings are involved. It skips, and passes, where
//! Orca or the rest of what that needs isn't installed.
//!
//! The dashboard's keyboard is driven from here (the harness has no compositor to type into); Orca
//! hears about it only over the bus, as it would from the real thing. The transcript is printed.
//!
//! Slint wants one thread and one platform per process, so this is a plain program
//! (`harness = false`).

mod bus;

use niri_dash::testing::{Harness, Sandbox};
use niri_dash_system::{Demo, brightness, wifi};
use slint::Model;
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

fn main() {
    if std::env::var_os("NIRI_DASH_PRIVATE_BUS").is_some() {
        return inner();
    }
    if let Some(what) = bus::missing(&["orca", "kwin_wayland"]) {
        println!(
            "test orca: skipped, {what} isn't installed\n\ntest result: ok. 0 passed; 0 failed"
        );
        return;
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

/// Let the dashboard run (the harness's loop) for `ms`.
fn wait(h: &mut Harness, ms: u64) {
    h.settle(ms);
}

fn stop(orca: &mut Child) {
    let _ = orca.kill();
    let _ = orca.wait();
}

/// A process that is stopped whatever happens to the test.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        stop(&mut self.0);
    }
}

/// The Wayland socket of the compositor Orca's toolkit needs to find a display on.
const DISPLAY: &str = "niri-dash-test";

/// KDE's own compositor, without a screen: Orca needs a display of some kind to set up its
/// keyboard handling (it dies without one), and on KDE it is the real thing it would meet.
fn compositor(dir: &Path) -> Running {
    let runtime = std::env::var("XDG_RUNTIME_DIR").expect("a runtime directory");
    let kwin = Command::new("kwin_wayland")
        .args([
            "--virtual",
            "--no-lockscreen",
            "--no-global-shortcuts",
            "--no-kactivities",
            "--socket",
            DISPLAY,
        ])
        .env("KWIN_COMPOSE", "Q")
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(dir.join("kwin.log")).expect("a file for KWin's log"))
        .spawn()
        .expect("starting KWin");
    let mut kwin = Running(kwin);
    let socket = Path::new(&runtime).join(DISPLAY);
    let start = Instant::now();
    while !socket.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(30) && kwin.0.try_wait().ok().flatten().is_none(),
            "KWin didn't come up: {}",
            std::fs::read_to_string(dir.join("kwin.log")).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    kwin
}

fn inner() {
    let _sandbox = Sandbox::enter(None).expect("a temporary directory for the files");
    let log = std::env::temp_dir().join(format!("niri-dash-orca-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);

    let errors = log.with_extension("err");
    let _kwin = compositor(&std::env::temp_dir());
    let mut orca = Command::new("orca")
        .arg("--debug-file")
        .arg(&log)
        .env("WAYLAND_DISPLAY", DISPLAY)
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
        std::thread::sleep(Duration::from_millis(200));
    }
    bus::accessibility_on();

    let mut h = Harness::new(LAYOUT);
    h.send(wifi::Event::State(Box::new(Demo::new().wifi)));
    h.send(brightness::Event::Level(Some(0.7)));
    h.settle(20);
    h.access_connect_unix();

    // Orca has to notice the dashboard before it can read it: it says so in its log.
    let exe = std::env::current_exe().expect("this program's path");
    let name = exe
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let start = Instant::now();
    while !std::fs::read_to_string(&log).is_ok_and(|t| t.contains(&name)) {
        if start.elapsed() > Duration::from_secs(40) {
            let t = tail(&log);
            stop(&mut orca);
            panic!("Orca never noticed the dashboard ({name}). Its log ends:\n{t}");
        }
        wait(&mut h, 200);
    }
    wait(&mut h, 1500);

    // Walk the keyboard: into the Wi-Fi widget and down its stops, then out and into brightness.
    let hints: Vec<String> = h
        .ui()
        .get_widgets()
        .iter()
        .map(|w| w.hint.to_string())
        .collect();
    let mut transcript: Vec<(String, String, Vec<String>)> = Vec::new();
    let mut step = |h: &mut Harness, what: &str, keys: &[&str]| {
        let before = spoken(&log).len();
        for k in keys {
            h.press(k);
            wait(h, 150);
        }
        wait(h, 1200);
        let ours = h
            .spoken()
            .ok()
            .flatten()
            .unwrap_or_else(|| "(nothing entered)".into());
        let said: Vec<String> = spoken(&log)[before..].to_vec();
        transcript.push((what.to_string(), ours, said));
    };
    let wifi_keys: Vec<String> = std::iter::once("f".to_string())
        .chain(hints[0].chars().map(|c| c.to_string()))
        .collect();
    let wifi_keys: Vec<&str> = wifi_keys.iter().map(String::as_str).collect();
    step(&mut h, "enter Wi-Fi", &wifi_keys);
    step(&mut h, "j", &["j"]);
    step(&mut h, "j", &["j"]);
    step(&mut h, "k", &["k"]);
    step(&mut h, "leave", &["esc"]);
    let knob_keys: Vec<String> = std::iter::once("f".to_string())
        .chain(hints[1].chars().map(|c| c.to_string()))
        .collect();
    let knob_keys: Vec<&str> = knob_keys.iter().map(String::as_str).collect();
    step(&mut h, "enter brightness", &knob_keys);
    stop(&mut orca);

    println!("what Orca said (it has no speaker; this is its log of what it spoke):");
    for (what, ours, said) in &transcript {
        println!("  {what}: the keyboard is on {ours}");
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
    // Each stop the keyboard moved to is read out, by its name, and a row's details after it.
    for (what, ours, said) in &transcript {
        if what == "leave" {
            continue;
        }
        let label = ours.split('"').nth(1).unwrap_or_default();
        if !said.iter().any(|s| s.contains(label)) {
            problems.push(format!(
                "after {what}, on {ours}, Orca never said {label:?}: {said:?}"
            ));
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
    if !all.iter().any(|s| s.starts_with("Wi-Fi List with 4 items")) {
        problems.push("Orca didn't announce the Wi-Fi list and its length".into());
    }
    if !all.iter().any(|s| s.contains("Brightness slider")) {
        problems.push("Orca didn't read the brightness slider as one".into());
    }
    if problems.is_empty() {
        let _ = std::fs::remove_file(&log);
        let _ = std::fs::remove_file(&errors);
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
        println!("\ntest result: FAILED. 0 passed; 1 failed");
        std::process::exit(1);
    }
}

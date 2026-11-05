//! A private session bus with its own accessibility bus, for the tests that put the dashboard on a
//! real one (`atspi.rs`, `orca.rs`). Nothing here may reach the session's own buses.
//!
//! The test program starts itself again under `dbus-run-session`, in a temporary runtime
//! directory, telling the second run it is on a private bus (`NIRI_DASH_PRIVATE_BUS`: the
//! harness refuses to publish without it). The second run is `inner`.

use std::process::{Command, Stdio};

pub fn installed(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// What these tests need, or the first thing that is missing.
pub fn missing(also: &[&str]) -> Option<String> {
    for p in ["dbus-run-session", "gdbus", "python3"].iter().chain(also) {
        if !installed(p) {
            return Some((*p).to_string());
        }
    }
    let bindings = Command::new("python3")
        .args([
            "-c",
            "import gi; gi.require_version('Atspi', '2.0'); from gi.repository import Atspi",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if !bindings.is_ok_and(|s| s.success()) {
        return Some("python3 with the Atspi bindings (gi)".into());
    }
    let launcher = [
        "/usr/lib/at-spi-bus-launcher",
        "/usr/libexec/at-spi-bus-launcher",
    ];
    if !launcher.iter().any(|p| std::path::Path::new(p).exists()) {
        return Some("at-spi-bus-launcher".into());
    }
    None
}

/// Run this program again on a private bus; true if it succeeded. In English, so what a screen
/// reader says can be compared with words.
pub fn rerun_privately(name: &str) -> bool {
    let runtime = std::env::temp_dir().join(format!("niri-dash-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&runtime).expect("a runtime directory");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700))
            .expect("a private runtime directory");
    }
    // Everything the session starts, services the bus activates included, keeps its files here.
    let mut session = Command::new("dbus-run-session");
    for (var, sub) in [
        ("HOME", "home"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_STATE_HOME", "state"),
    ] {
        let dir = runtime.join(sub);
        std::fs::create_dir_all(&dir).expect("a directory for the session's files");
        session.env(var, dir);
    }
    let exe = std::env::current_exe().expect("this program's path");
    let status = session
        .arg("--")
        .arg(exe)
        .env("NIRI_DASH_PRIVATE_BUS", "1")
        .env("XDG_RUNTIME_DIR", &runtime)
        // The accessibility bus is made by a plain dbus-daemon: dbus-broker, where it is the
        // default, wants a systemd user manager that a private session doesn't have.
        .env("ATSPI_DBUS_IMPLEMENTATION", "dbus-daemon")
        // Settings of a screen reader stay in memory, out of the user's.
        .env("GSETTINGS_BACKEND", "memory")
        .env("LC_ALL", "C")
        .env("LANGUAGE", "en")
        .env_remove("DBUS_SESSION_BUS_ADDRESS")
        .env_remove("AT_SPI_BUS_ADDRESS")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("DISPLAY")
        .status();
    let _ = std::fs::remove_dir_all(&runtime);
    match status {
        Ok(s) if s.success() => true,
        Ok(s) => {
            println!("\ntest result: FAILED ({s})");
            false
        }
        Err(e) => {
            println!("\ntest result: FAILED (dbus-run-session: {e})");
            false
        }
    }
}

/// Switch accessibility on, as a desktop does when a screen reader starts.
pub fn accessibility_on() {
    let on = Command::new("gdbus")
        .args([
            "call",
            "--session",
            "--dest",
            "org.a11y.Bus",
            "--object-path",
            "/org/a11y/bus",
            "--method",
            "org.freedesktop.DBus.Properties.Set",
            "org.a11y.Status",
            "IsEnabled",
            "<true>",
        ])
        .output()
        .expect("running gdbus");
    assert!(
        on.status.success(),
        "turning accessibility on: {}",
        String::from_utf8_lossy(&on.stderr)
    );
}

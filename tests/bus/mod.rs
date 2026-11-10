//! A private session bus with its own accessibility bus, for the tests that put the dashboard on a
//! real one (`atspi.rs`, `orca.rs`). Nothing here may reach the session's own buses.
//!
//! The test program starts itself again under `dbus-run-session`, in a temporary runtime
//! directory, telling the second run it is on a private bus (`NIRI_DASH_PRIVATE_BUS`). The
//! second run is `inner`. It starts KWin (`--virtual`) and the dashboard's own program on it: GPUI
//! publishes its accessibility tree from the window's own adapter, which finds the accessibility
//! bus through whatever session bus the program is started in, so there is no way to publish
//! onto the bus from the test program itself, as the Slint version's harness did.

#![allow(dead_code)]

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

/// A process that is stopped whatever happens to the test.
pub struct Running(pub std::process::Child);

impl Running {
    pub fn exited(&mut self) -> Option<std::process::ExitStatus> {
        self.0.try_wait().ok().flatten()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The Wayland socket of the compositor the dashboard (and Orca's toolkit) needs to find a
/// display on.
pub const DISPLAY: &str = "niri-dash-test";

/// KDE's own compositor, without a screen: the dashboard is a layer-shell window, and Orca needs
/// a display of some kind to set up its keyboard handling (it dies without one).
pub fn compositor(dir: &std::path::Path) -> Running {
    use std::time::{Duration, Instant};
    let runtime = std::env::var("XDG_RUNTIME_DIR").expect("a runtime directory");
    let kwin = Command::new("kwin_wayland")
        .args([
            "--virtual",
            "--no-lockscreen",
            "--no-global-shortcuts",
            "--no-kactivities",
            "--width",
            "1600",
            "--height",
            "900",
            "--socket",
            DISPLAY,
        ])
        .env("KWIN_COMPOSE", "Q")
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(dir.join("kwin.log")).expect("a file for KWin's log"))
        .spawn()
        .expect("starting KWin");
    let mut kwin = Running(kwin);
    let socket = std::path::Path::new(&runtime).join(DISPLAY);
    let start = Instant::now();
    while !socket.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(30) && kwin.exited().is_none(),
            "KWin didn't come up: {}",
            std::fs::read_to_string(dir.join("kwin.log")).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    kwin
}

/// The dashboard's own program: beside the test program in the box (`cargo testbox` puts it
/// there), or in cargo's target directory.
pub fn dashboard_binary() -> std::path::PathBuf {
    let exe = std::env::current_exe().expect("this program's path");
    let beside = exe.with_file_name("niri-dash");
    if beside.exists() {
        return beside;
    }
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_niri-dash"))
}

/// The dashboard, showing `layout` on the mock system, on the compositor `compositor` started
/// (everything it keeps goes under `dir`). Its log is `dir/dash.log`.
pub fn dashboard(dir: &std::path::Path, layout: &str) -> Running {
    let config = dir.join("config.kdl");
    std::fs::write(&config, layout).expect("a config for the dashboard");
    let home = dir.join("niri-dash-home");
    std::fs::create_dir_all(&home).expect("a home for the dashboard's files");
    let log = std::fs::File::create(dir.join("dash.log")).expect("a file for the dashboard's log");
    let child = Command::new(dashboard_binary())
        .args(["daemon", "--show"])
        .env("NIRI_DASH_MOCK", "1")
        // The window is the dashboard's own, on a compositor with no keyboard to type on: keys
        // come in over the control socket (`press`).
        .env("NIRI_DASH_TEST_KEYS", "1")
        .env("NIRI_DASH_CONFIG", &config)
        .env("NIRI_DASH_HOME", &home)
        .env("WAYLAND_DISPLAY", DISPLAY)
        .env("RUST_LOG", "info")
        .env_remove("DISPLAY")
        .env_remove("NIRI_SOCKET")
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
        .expect("starting the dashboard");
    Running(child)
}

/// Press a key (`"j"`, `"ctrl+h"`, `"esc"`) in the dashboard's window, as its control socket takes
/// it (`NIRI_DASH_TEST_KEYS`): what GPUI does with it is what it does with a keyboard's.
pub fn press(key: &str) {
    use std::io::Write;
    let runtime = std::env::var("XDG_RUNTIME_DIR").expect("a runtime directory");
    let path = std::path::Path::new(&runtime).join(format!("niri-dash-{DISPLAY}.sock"));
    let mut socket = std::os::unix::net::UnixStream::connect(&path)
        .unwrap_or_else(|e| panic!("the dashboard's control socket {}: {e}", path.display()));
    writeln!(socket, "key {key}").expect("writing a key to the dashboard");
}

/// What the dashboard has logged, for a failure.
pub fn dashboard_log(dir: &std::path::Path) -> String {
    let text = std::fs::read_to_string(dir.join("dash.log")).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(40)..].join("\n")
}

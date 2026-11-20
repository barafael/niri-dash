//! What every system promises the dashboard, checked against each one that can run without a
//! desktop: the mock and the simulated one, and the live one where it may (in the test box,
//! `testbox/`, which `cargo testbox test system` starts). A new system is checked by adding it to
//! `systems()`.

#[path = "../../../testbox/inside.rs"]
mod inside;

use niri_dash_system::{
    Event, EventSender, Options, Refused, System, Tell, audio, brightness, host, live, mock, power,
    session, simulated, wifi,
};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

type Start = fn(EventSender) -> std::io::Result<System>;

/// The systems to check: all of them, as the dashboard runs them.
fn systems() -> Vec<(&'static str, Start)> {
    let mut systems: Vec<(&'static str, Start)> = vec![("mock", mock), ("simulated", simulated)];
    if inside::inside() {
        systems.push(("live", |events| {
            live(
                events,
                Options {
                    feeds: None,
                    wayland: None,
                    gamma: false,
                },
            )
        }));
    }
    systems
}

/// The applet an event is from: its variant's name.
fn applet(e: &Event) -> String {
    let text = format!("{e:?}");
    text.split('(').next().unwrap_or_default().to_string()
}

/// The applets heard from within `wait` of coming up.
fn heard_from(start: Start, wait: Duration) -> (niri_dash_system::Capabilities, BTreeSet<String>) {
    let (events, rx) = EventSender::channel();
    let System { commands, caps } = start(events).expect("the system starts");
    commands.tell(host::Command::Visible(true));
    let deadline = Instant::now() + wait;
    let mut heard = BTreeSet::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        if let Ok(e) = rx.recv_timeout(left.min(Duration::from_millis(50))) {
            heard.insert(applet(&e));
        }
    }
    (caps, heard)
}

#[test]
fn a_system_reports_what_its_capabilities_say_it_can() {
    for (name, start) in systems() {
        let (caps, heard) = heard_from(start, Duration::from_millis(1500));
        let reports = |applet: &str| heard.contains(applet);
        // What every system has.
        for always in ["Wifi", "Audio", "Power", "Brightness"] {
            assert!(reports(always), "{name} never reported {always}: {heard:?}");
        }
        // What it has only where the capability says so.
        for (applet, offered) in [
            ("Windows", caps.windows),
            ("Keyboard", caps.keyboard_layouts),
        ] {
            assert_eq!(
                reports(applet),
                offered,
                "{name}: {applet} is {} but its capability is {offered}: {heard:?}",
                if reports(applet) {
                    "reported"
                } else {
                    "silent"
                },
            );
        }
    }
}

#[test]
fn a_system_keeps_running_under_commands() {
    for (name, start) in systems() {
        let (events, _rx) = EventSender::channel();
        let System { commands, .. } = start(events).expect("the system starts");
        // Telling a system that is running never fails or panics, however often.
        for _ in 0..3 {
            commands.tell(host::Command::Refresh);
            commands.tell(brightness::Command::Set(0.5));
            commands.tell(audio::Command::ToggleMute);
            commands.tell(wifi::Command::Toggle);
            commands.tell(power::Command::SetProfile(power::Profile::Balanced));
            commands.tell(session::Command::Lock);
        }
        std::thread::sleep(Duration::from_millis(100));
        let still = commands.send(host::Command::Refresh.into());
        assert_ne!(still, Err(Refused::Closed), "{name} stopped on commands");
    }
}

#[test]
fn simulated_means_nothing_is_real() {
    for (name, start) in systems() {
        let (events, _rx) = EventSender::channel();
        let System { caps, .. } = start(events).expect("the system starts");
        assert_eq!(caps.simulated, name == "simulated", "{name}");
    }
}

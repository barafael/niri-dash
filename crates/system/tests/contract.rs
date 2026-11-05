//! What every system promises the dashboard, checked against each one that can run without a
//! desktop: the mock and the simulated one. A new system is checked by adding it to `SYSTEMS`.

use niri_dash_system::{
    Event, EventSender, Started, Tell, audio, brightness, host, mock, power, session, simulated,
    wifi,
};
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

type Start = fn(EventSender) -> std::io::Result<Started>;

const SYSTEMS: &[(&str, Start)] = &[("mock", mock), ("simulated", simulated)];

/// The applet an event is from: its variant's name.
fn applet(e: &Event) -> String {
    let text = format!("{e:?}");
    text.split('(').next().unwrap_or_default().to_string()
}

/// The applets heard from within `wait` of coming up.
fn heard_from(start: Start, wait: Duration) -> (niri_dash_system::Capabilities, BTreeSet<String>) {
    let (events, rx) = EventSender::channel();
    let Started { senders, caps } = start(events).expect("the system starts");
    senders.host.tell(host::Command::Visible(true));
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
    for (name, start) in SYSTEMS {
        let (caps, heard) = heard_from(*start, Duration::from_millis(1500));
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
    for (name, start) in SYSTEMS {
        let (events, _rx) = EventSender::channel();
        let Started { senders, .. } = start(events).expect("the system starts");
        // Commands are never refused: telling a system that is running never fails or panics,
        // however often.
        for _ in 0..3 {
            senders.host.tell(host::Command::Refresh);
            senders.brightness.tell(brightness::Command::Set(0.5));
            senders.audio.tell(audio::Command::ToggleMute);
            senders.wifi.tell(wifi::Command::Toggle);
            senders
                .power
                .tell(power::Command::SetProfile(power::Profile::Balanced));
            senders.session.tell(session::Command::Lock);
        }
        std::thread::sleep(Duration::from_millis(100));
        assert!(!senders.host.is_closed(), "{name} stopped on commands");
    }
}

#[test]
fn simulated_means_nothing_is_real() {
    for (name, start) in SYSTEMS {
        let (events, _rx) = EventSender::channel();
        let Started { caps, .. } = start(events).expect("the system starts");
        assert_eq!(caps.simulated, *name == "simulated", "{name}");
    }
}

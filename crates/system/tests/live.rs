//! The real system, with nothing mocked on our side: `live()` runs as the dashboard runs it (minus
//! the compositor) and is told to do things; the services it talks to answer.
//!
//! This changes the machine it runs on: it turns radios off and on. So it only runs in the
//! container that `tests/system/` builds, where the services are mocks on a bus of their own, and
//! not anywhere else, however it is started. `tests/system/run.sh` starts it.

use niri_dash_system::{
    Event, EventSender, Feed, Options, Started, Tell, bluetooth, live, notifications, power, wifi,
};
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

/// A container that was made for this, and said so.
fn allowed() -> bool {
    let in_container =
        Path::new("/.dockerenv").exists() || Path::new("/run/.containerenv").exists();
    in_container && std::env::var_os("NIRI_DASH_SYSTEM_TESTS").is_some()
}

struct System {
    started: Started,
    events: Receiver<Event>,
}

impl System {
    fn start(feeds: &'static [Feed]) -> Self {
        let (sink, events) = EventSender::channel();
        let started = live(
            sink,
            Options {
                feeds: Some(feeds),
                wayland: None,
                gamma: false,
            },
        )
        .expect("the live system starts");
        // Sources only watch and poll while the dashboard is up.
        started
            .senders
            .host
            .tell(niri_dash_system::host::Command::Visible(true));
        Self { started, events }
    }

    /// The first event within `wait` that `pick` takes.
    fn wait_for<T>(&self, wait: Duration, mut pick: impl FnMut(&Event) -> Option<T>) -> Option<T> {
        let end = Instant::now() + wait;
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            if let Ok(e) = self.events.recv_timeout(left)
                && let Some(found) = pick(&e)
            {
                return Some(found);
            }
        }
        None
    }

    fn wifi(&self, wait: Duration, want: impl Fn(&wifi::State) -> bool) -> Option<wifi::State> {
        self.wait_for(wait, |e| match e {
            Event::Wifi(wifi::Event::State(s)) if want(s) => Some((**s).clone()),
            _ => None,
        })
    }

    fn bluetooth(
        &self,
        wait: Duration,
        want: impl Fn(&bluetooth::State) -> bool,
    ) -> Option<bluetooth::State> {
        self.wait_for(wait, |e| match e {
            Event::Bluetooth(bluetooth::Event::State(s)) if want(s) => Some((**s).clone()),
            _ => None,
        })
    }
}

const WAIT: Duration = Duration::from_secs(10);

type Scenario = (&'static str, fn() -> Result<(), String>);

/// Wi-Fi goes off when told to, and comes back.
fn wifi_toggles() -> Result<(), String> {
    let sys = System::start(&[Feed::Wifi]);
    let first = sys
        .wifi(WAIT, |s| s.available)
        .ok_or("no Wi-Fi device was reported")?;
    if !first.enabled {
        return Err("Wi-Fi starts off in the mock".into());
    }
    sys.started.senders.wifi.tell(wifi::Command::Toggle);
    sys.wifi(WAIT, |s| !s.enabled)
        .ok_or("Wi-Fi never reported being off after Toggle")?;
    sys.started.senders.wifi.tell(wifi::Command::Toggle);
    sys.wifi(WAIT, |s| s.enabled)
        .ok_or("Wi-Fi never reported being on after the second Toggle")?;
    Ok(())
}

/// The Bluetooth adapter goes off when told to, and comes back.
fn bluetooth_toggles() -> Result<(), String> {
    let sys = System::start(&[Feed::Bluetooth]);
    let first = sys
        .bluetooth(WAIT, |s| s.available)
        .ok_or("no Bluetooth adapter was reported")?;
    if !first.powered {
        return Err("the Bluetooth adapter starts off in the mock".into());
    }
    sys.started
        .senders
        .bluetooth
        .tell(bluetooth::Command::Toggle);
    sys.bluetooth(WAIT, |s| !s.powered)
        .ok_or("Bluetooth never reported being off after Toggle")?;
    sys.started
        .senders
        .bluetooth
        .tell(bluetooth::Command::Toggle);
    sys.bluetooth(WAIT, |s| s.powered)
        .ok_or("Bluetooth never reported being on after the second Toggle")?;
    Ok(())
}

/// The battery of a device that UPower knows (a mouse) is reported with its charge. (The
/// machine's own battery comes from sysfs, which a container can't fake.)
fn peripheral_battery_is_reported() -> Result<(), String> {
    let sys = System::start(&[Feed::Power]);
    let state = sys
        .wait_for(WAIT, |e| match e {
            Event::Power(power::Event::State(s)) if !s.peripherals.is_empty() => Some(s.clone()),
            _ => None,
        })
        .ok_or("no device battery was reported")?;
    let mouse = &state.peripherals[0];
    if (mouse.name.as_str(), mouse.percent) != ("Mock Mouse", 55) {
        return Err(format!("{} at {}%", mouse.name, mouse.percent));
    }
    if mouse.kind != power::DeviceKind::Mouse {
        return Err(format!("a mouse reported as {:?}", mouse.kind));
    }
    Ok(())
}

/// A program sends a notification, as `notify-send` would; the dashboard keeps it, and drops it
/// when told to.
fn notification_is_kept_and_dismissed() -> Result<(), String> {
    use std::collections::HashMap;
    use zbus::zvariant::Value;
    let sys = System::start(&[Feed::Notifications]);
    let notes = |sys: &System, want: &dyn Fn(&notifications::State) -> bool| {
        sys.wait_for(WAIT, |e| match e {
            Event::Notifications(notifications::Event::State(s)) if want(s) => Some(s.clone()),
            _ => None,
        })
    };
    notes(&sys, &|s| s.server).ok_or("the dashboard never became the notification server")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let id: u32 = runtime
        .block_on(async {
            let bus = zbus::Connection::session().await?;
            let hints: HashMap<&str, Value> = HashMap::new();
            bus.call_method(
                Some("org.freedesktop.Notifications"),
                "/org/freedesktop/Notifications",
                Some("org.freedesktop.Notifications"),
                "Notify",
                &(
                    "system-test",
                    0u32,
                    "",
                    "Hello",
                    "From a test",
                    Vec::<&str>::new(),
                    hints,
                    -1i32,
                ),
            )
            .await?
            .body()
            .deserialize()
        })
        .map_err(|e| format!("sending a notification: {e}"))?;
    let state = notes(&sys, &|s| s.items.iter().any(|n| n.id == id))
        .ok_or("the notification never showed up")?;
    let note = state.items.iter().find(|n| n.id == id).ok_or("lost it")?;
    if (note.app.as_str(), note.summary.as_str(), note.body.as_str())
        != ("system-test", "Hello", "From a test")
    {
        return Err(format!(
            "it arrived as {:?}",
            (&note.app, &note.summary, &note.body)
        ));
    }
    sys.started
        .senders
        .notifications
        .tell(notifications::Command::Dismiss(id));
    notes(&sys, &|s| !s.items.iter().any(|n| n.id == id))
        .ok_or("the notification stayed after Dismiss")?;
    Ok(())
}

fn main() {
    if !allowed() {
        eprintln!(
            "skipped: these tests switch radios, so they only run in the container \
             (tests/system/run.sh)"
        );
        return;
    }
    let scenarios: &[Scenario] = &[
        ("wifi: toggles", wifi_toggles),
        ("bluetooth: toggles", bluetooth_toggles),
        (
            "power: a device battery is reported",
            peripheral_battery_is_reported,
        ),
        (
            "notifications: kept and dismissed",
            notification_is_kept_and_dismissed,
        ),
    ];
    let only = std::env::var("NIRI_DASH_SYSTEM_ONLY")
        .ok()
        .filter(|o| !o.is_empty());
    let mut failed = 0;
    for (name, run) in scenarios {
        if only.as_deref().is_some_and(|o| !name.contains(o)) {
            continue;
        }
        match run() {
            Ok(()) => println!("test {name} ... ok"),
            Err(why) => {
                failed += 1;
                println!("test {name} ... FAILED: {why}");
            }
        }
    }
    println!(
        "\ntest result: {}. {} passed; {failed} failed",
        if failed == 0 { "ok" } else { "FAILED" },
        scenarios.len() - failed
    );
    if failed > 0 {
        std::process::exit(1);
    }
}

//! The real system, with nothing mocked on our side: `live()` runs as the dashboard runs it (minus
//! the compositor) and is told to do things; the services it talks to answer.
//!
//! This changes the machine it runs on: it turns radios off and on. So it only runs in the test
//! box (`testbox/`), where the services are mocks on a bus of their own, and not anywhere else,
//! however it is started. `cargo testbox test system` starts it.

#[path = "../../../testbox/inside.rs"]
mod inside;

use niri_dash_system::{
    Event, EventSender, Feed, Options, Started, Tell, bluetooth, live, notifications, power, wifi,
};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

struct System {
    started: Started,
    events: Receiver<Event>,
    seen: std::cell::RefCell<Seen>,
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
        Self {
            started,
            events,
            seen: Default::default(),
        }
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

/// What the system last said, for each applet the scenarios follow. Every state is a whole
/// snapshot, so the last one is the truth: a scenario asks what holds now, not what was said.
#[derive(Debug, Default)]
struct Seen {
    wifi: Option<wifi::State>,
    bluetooth: Option<bluetooth::State>,
    power: Option<power::State>,
    notes: Option<notifications::State>,
}

impl Seen {
    fn take(&mut self, event: Event) {
        match event {
            Event::Wifi(wifi::Event::State(s)) => self.wifi = Some(*s),
            Event::Bluetooth(bluetooth::Event::State(s)) => self.bluetooth = Some(*s),
            Event::Power(power::Event::State(s)) => self.power = Some(s),
            Event::Notifications(notifications::Event::State(s)) => self.notes = Some(s),
            _ => {}
        }
    }
}

impl System {
    /// Waits until what the system last said satisfies `get`. Checks what was said already first,
    /// so a state that arrived while the scenario was busy elsewhere isn't missed.
    fn until<T>(&self, what: &str, get: impl Fn(&Seen) -> Option<T>) -> Result<T, String> {
        let end = Instant::now() + WAIT;
        loop {
            if let Some(found) = get(&self.seen.borrow()) {
                return Ok(found);
            }
            let Some(left) = end.checked_duration_since(Instant::now()) else {
                let seen = format!("{:#?}", self.seen.borrow());
                let seen: String = seen.chars().take(1800).collect();
                return Err(format!("never saw {what}. The last it said:\n{seen}"));
            };
            if let Ok(e) = self.events.recv_timeout(left) {
                self.seen.borrow_mut().take(e);
            }
        }
    }

    /// Checks that `get` holds of what was said, after letting the system say what it has for a
    /// moment: for "and nothing else changed".
    fn still(&self, what: &str, get: impl Fn(&Seen) -> bool) -> Result<(), String> {
        std::thread::sleep(Duration::from_millis(700));
        while let Ok(e) = self.events.try_recv() {
            self.seen.borrow_mut().take(e);
        }
        if get(&self.seen.borrow()) {
            Ok(())
        } else {
            let seen = format!("{:#?}", self.seen.borrow());
            let seen: String = seen.chars().take(1800).collect();
            Err(format!("{what} doesn't hold. The last it said:\n{seen}"))
        }
    }
}

/// A connection to one of the box's buses, for playing the part of the world: what the mocked
/// services are told to do shows in the system as it would from the real thing.
struct Bus {
    runtime: tokio::runtime::Runtime,
    connection: zbus::Connection,
}

impl Bus {
    fn system() -> Result<Self, String> {
        Self::connect(true)
    }

    fn session() -> Result<Self, String> {
        Self::connect(false)
    }

    fn connect(system: bool) -> Result<Self, String> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let connection = runtime
            .block_on(async {
                if system {
                    zbus::Connection::system().await
                } else {
                    zbus::Connection::session().await
                }
            })
            .map_err(|e| format!("connecting to the bus: {e}"))?;
        Ok(Self {
            runtime,
            connection,
        })
    }

    fn call<B>(
        &self,
        service: &str,
        path: &str,
        interface: &str,
        method: &str,
        body: &B,
    ) -> Result<zbus::Message, String>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
    {
        self.runtime
            .block_on(self.connection.call_method(
                Some(service),
                path,
                Some(interface),
                method,
                body,
            ))
            .map_err(|e| format!("{service} {method}: {e}"))
    }
}

/// The mocked NetworkManager's names (testbox/mocks.py makes the device).
mod nm {
    pub const SERVICE: &str = "org.freedesktop.NetworkManager";
    pub const PATH: &str = "/org/freedesktop/NetworkManager";
    pub const MOCK: &str = "org.freedesktop.DBus.Mock";
    pub const DEVICE: &str = "/org/freedesktop/NetworkManager/Devices/mock_wlan0";
    pub const AP: &str = "org.freedesktop.NetworkManager.AccessPoint";
    pub const WPA_PSK: u32 = 0x100;

    pub fn ap_path(name: &str) -> String {
        format!("/org/freedesktop/NetworkManager/AccessPoint/{name}")
    }
}

impl Bus {
    /// An access point appears in the air.
    fn add_ap(
        &self,
        name: &str,
        ssid: &str,
        mac: &str,
        mhz: u32,
        strength: u8,
        sec: u32,
    ) -> Result<(), String> {
        self.call(
            nm::SERVICE,
            nm::PATH,
            nm::MOCK,
            "AddAccessPoint",
            &(
                nm::DEVICE,
                name,
                ssid,
                mac,
                2u32,
                mhz,
                5400u32,
                strength,
                sec,
            ),
        )
        .map(|_| ())
    }

    fn remove_ap(&self, name: &str) -> Result<(), String> {
        self.call(
            nm::SERVICE,
            nm::PATH,
            nm::MOCK,
            "RemoveAccessPoint",
            &(nm::DEVICE, nm::ap_path(name)),
        )
        .map(|_| ())
    }

    fn set_ap_strength(&self, name: &str, strength: u8) -> Result<(), String> {
        self.call(
            nm::SERVICE,
            nm::PATH,
            nm::MOCK,
            "SetProperty",
            &(
                nm::ap_path(name),
                nm::AP,
                "Strength",
                zbus::zvariant::Value::U8(strength),
            ),
        )
        .map(|_| ())
    }

    /// A boolean property of a service's object: the world's truth, to hold the dashboard to.
    fn get_bool(
        &self,
        service: &str,
        path: &str,
        interface: &str,
        name: &str,
    ) -> Result<bool, String> {
        let answer = self.call(
            service,
            path,
            "org.freedesktop.DBus.Properties",
            "Get",
            &(interface, name),
        )?;
        let value: zbus::zvariant::OwnedValue = answer
            .body()
            .deserialize()
            .map_err(|e| format!("{service} {name}: {e}"))?;
        bool::try_from(value).map_err(|e| format!("{service} {name} isn't a bool: {e}"))
    }

    /// BlueZ's `Device1.Connect` and `Disconnect` in the mock only flip a Python attribute and
    /// send a signal; the `Connected` property stays as it was, so a client that asks again is
    /// told the device never connected. This makes them do what BlueZ does: set the property.
    fn make_links_real(&self, device_path: &str) -> Result<(), String> {
        for (method, connected) in [("Connect", "True"), ("Disconnect", "False")] {
            self.call(
                "org.bluez",
                device_path,
                "org.freedesktop.DBus.Mock",
                "AddMethod",
                &(
                    "org.bluez.Device1",
                    method,
                    "",
                    "",
                    format!(
                        "self.UpdateProperties('org.bluez.Device1', {{'Connected': {connected}}})"
                    ),
                ),
            )?;
        }
        Ok(())
    }

    fn bluez(&self, method: &str, adapter: &str, address: &str) -> Result<(), String> {
        self.call(
            "org.bluez",
            "/org/bluez",
            "org.bluez.Mock",
            method,
            &(adapter, address),
        )
        .map(|_| ())
    }

    fn set_battery(&self, device: &str, percent: f64) -> Result<(), String> {
        use std::collections::HashMap;
        use zbus::zvariant::{ObjectPath, Value};
        let props: HashMap<&str, Value> = HashMap::from([("Percentage", Value::F64(percent))]);
        let path = ObjectPath::try_from(format!("/org/freedesktop/UPower/devices/{device}"))
            .map_err(|e| e.to_string())?;
        self.call(
            "org.freedesktop.UPower",
            "/org/freedesktop/UPower",
            "org.freedesktop.DBus.Mock",
            "SetDeviceProperties",
            &(path, props),
        )
        .map(|_| ())
    }

    /// A device battery appears on UPower (UPower's type numbers: 5 mouse, 6 keyboard, 7 phone).
    fn add_peripheral(
        &self,
        device: &str,
        model: &str,
        kind: u32,
        percent: f64,
    ) -> Result<(), String> {
        use std::collections::HashMap;
        use zbus::zvariant::{ObjectPath, Value};
        let path = format!("/org/freedesktop/UPower/devices/{device}");
        let props: HashMap<&str, Value> = HashMap::from([
            ("PowerSupply", Value::Bool(false)),
            ("IsPresent", Value::Bool(true)),
            ("Model", Value::from(model)),
            ("Percentage", Value::F64(percent)),
            ("State", Value::U32(2)),
            ("Type", Value::U32(kind)),
        ]);
        let methods: Vec<(&str, &str, &str, &str)> = Vec::new();
        self.call(
            "org.freedesktop.UPower",
            "/org/freedesktop/UPower",
            "org.freedesktop.DBus.Mock",
            "AddObject",
            &(
                path.as_str(),
                "org.freedesktop.UPower.Device",
                props,
                methods,
            ),
        )?;
        let signal_args = vec![Value::from(
            ObjectPath::try_from(path).map_err(|e| e.to_string())?,
        )];
        self.call(
            "org.freedesktop.UPower",
            "/org/freedesktop/UPower",
            "org.freedesktop.DBus.Mock",
            "EmitSignal",
            &("org.freedesktop.UPower", "DeviceAdded", "o", signal_args),
        )
        .map(|_| ())
    }

    fn remove_peripheral(&self, device: &str) -> Result<(), String> {
        let path = zbus::zvariant::ObjectPath::try_from(format!(
            "/org/freedesktop/UPower/devices/{device}"
        ))
        .map_err(|e| e.to_string())?;
        self.call(
            "org.freedesktop.UPower",
            "/org/freedesktop/UPower",
            "org.freedesktop.DBus.Mock",
            "RemoveDevice",
            &(path,),
        )
        .map(|_| ())
    }

    /// A program calls the notification server, as `notify-send` does; the id it gets back.
    fn notify(
        &self,
        app: &str,
        replaces: u32,
        summary: &str,
        body: &str,
        actions: &[&str],
        urgency: u8,
    ) -> Result<u32, String> {
        use std::collections::HashMap;
        use zbus::zvariant::Value;
        let hints: HashMap<&str, Value> = HashMap::from([("urgency", Value::U8(urgency))]);
        self.call(
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "Notify",
            &(
                app,
                replaces,
                "",
                summary,
                body,
                actions.to_vec(),
                hints,
                -1i32,
            ),
        )?
        .body()
        .deserialize()
        .map_err(|e| format!("Notify's answer: {e}"))
    }

    fn close_notification(&self, id: u32) -> Result<(), String> {
        self.call(
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
            "CloseNotification",
            &(id,),
        )
        .map(|_| ())
    }
}

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

fn check(ok: bool, why: impl FnOnce() -> String) -> Result<(), String> {
    if ok { Ok(()) } else { Err(why()) }
}

/// The air changes under a running system: networks come, merge, fade and go; the person joins
/// one, leaves it, forgets it, looks at another's details, and turns the radio off and on.
fn wifi_follows_the_air() -> Result<(), String> {
    let sys = System::start(&[Feed::Wifi]);
    let world = Bus::system()?;
    let ssids = |s: &Seen| -> Vec<String> {
        s.wifi
            .iter()
            .flat_map(|w| w.networks.iter().map(|n| n.ssid.clone()))
            .collect()
    };
    let net = |s: &Seen, ssid: &str| -> Option<wifi::Network> {
        s.wifi
            .as_ref()?
            .networks
            .iter()
            .find(|n| n.ssid == ssid)
            .cloned()
    };

    // The three the box starts with, strongest first.
    let first = sys.until("the three networks of the box", |s| {
        s.wifi.as_ref().filter(|w| w.networks.len() == 3).cloned()
    })?;
    let order: Vec<&str> = first.networks.iter().map(|n| n.ssid.as_str()).collect();
    check(order == ["Lighthouse", "Harbour Cafe", "Neighbour"], || {
        format!("listed as {order:?}, not strongest first")
    })?;
    check(
        first
            .networks
            .windows(2)
            .all(|w| w[0].signal >= w[1].signal),
        || {
            format!(
                "signals {:?} aren't in order",
                first.networks.iter().map(|n| n.signal).collect::<Vec<_>>()
            )
        },
    )?;

    // A fourth appears: with a password, on 2.4 GHz.
    world.add_ap(
        "mock_ap3",
        "Newcomer",
        "00:23:F8:7E:12:B3",
        2437,
        66,
        nm::WPA_PSK,
    )?;
    let new = sys.until("Newcomer in the list", |s| net(s, "Newcomer"))?;
    check(new.security == wifi::Security::Personal, || {
        format!("Newcomer has a password but is {:?}", new.security)
    })?;
    check(new.bands == 0b001 && new.aps == 1, || {
        format!(
            "one 2.4 GHz access point, but bands {:03b} and {} aps",
            new.bands, new.aps
        )
    })?;

    // A second access point of the same network, on 5 GHz and stronger: one network, two aps.
    world.add_ap(
        "mock_ap4",
        "Newcomer",
        "00:23:F8:7E:12:B4",
        5180,
        90,
        nm::WPA_PSK,
    )?;
    let both = sys.until("Newcomer merged over two access points", |s| {
        net(s, "Newcomer").filter(|n| n.aps == 2)
    })?;
    check(both.bands == 0b011, || {
        format!("2.4 and 5 GHz, but bands {:03b}", both.bands)
    })?;
    check(both.signal >= 85, || {
        format!(
            "the stronger access point is 90, the network shows {}",
            both.signal
        )
    })?;
    sys.still("one Newcomer, not two", |s| {
        ssids(s).iter().filter(|n| *n == "Newcomer").count() == 1
    })?;

    // The strongest fades: the order follows the signals.
    world.set_ap_strength("mock_ap0", 10)?;
    sys.until("Lighthouse fading to the end of the list", |s| {
        let w = s.wifi.as_ref()?;
        (w.networks.last()?.ssid == "Lighthouse").then_some(())
    })
    .map_err(|e| format!("a changed signal doesn't reach the list: {e}"))?;

    // Joining an open network through the dashboard's command; NetworkManager makes the profile.
    let harbour = net(&sys.seen.borrow(), "Harbour Cafe").ok_or("Harbour Cafe went missing")?;
    sys.started
        .senders
        .wifi
        .tell(wifi::Command::Connect { key: harbour.key() });
    sys.until("being joined to Harbour Cafe", |s| {
        let w = s.wifi.as_ref()?;
        (w.ssid == "Harbour Cafe" || w.connecting == "Harbour Cafe").then_some(())
    })
    .map_err(|e| format!("Connect to an open network did nothing: {e}"))?;
    sys.until("Harbour Cafe saved as a profile", |s| {
        s.wifi
            .as_ref()?
            .saved
            .iter()
            .find(|p| p.ssid == "Harbour Cafe")
            .cloned()
    })
    .map_err(|e| format!("a joined network isn't kept: {e}"))?;

    // Details of a network open, then closed.
    let key = net(&sys.seen.borrow(), "Neighbour")
        .ok_or("Neighbour went missing")?
        .key();
    sys.started.senders.wifi.tell(wifi::Command::Select { key });
    sys.until("Neighbour's details", |s| {
        s.wifi
            .as_ref()?
            .detail
            .as_ref()
            .filter(|d| d.ssid == "Neighbour")
            .map(|_| ())
    })?;
    sys.started
        .senders
        .wifi
        .tell(wifi::Command::Select { key: String::new() });
    sys.until("the details closed", |s| {
        s.wifi.as_ref().filter(|w| w.detail.is_none()).map(|_| ())
    })?;

    // They go: the second access point, then the whole network.
    world.remove_ap("mock_ap4")?;
    sys.until("Newcomer back to one access point", |s| {
        net(s, "Newcomer").filter(|n| n.aps == 1)
    })?;
    world.remove_ap("mock_ap3")?;
    sys.until("Newcomer gone", |s| {
        s.wifi
            .as_ref()
            .filter(|_| net(s, "Newcomer").is_none())
            .map(|_| ())
    })?;

    // The radio off: nothing is on offer. On again: the air is back.
    sys.started.senders.wifi.tell(wifi::Command::Toggle);
    sys.until("the radio off", |s| {
        s.wifi.as_ref().filter(|w| !w.enabled).cloned()
    })
    .and_then(|w| {
        check(w.networks.is_empty(), || {
            format!(
                "the radio is off, and {} networks are still listed",
                w.networks.len()
            )
        })
    })?;
    sys.started.senders.wifi.tell(wifi::Command::Toggle);
    sys.until("the radio on, and the air back", |s| {
        s.wifi
            .as_ref()
            .filter(|w| w.enabled && w.networks.len() >= 3)
            .map(|_| ())
    })?;
    Ok(())
}

/// A Bluetooth session: a scan finds devices (one appears while it runs), one is paired,
/// connected, loses its link, is trusted and renamed, and is forgotten; the adapter goes off and on.
fn bluetooth_session() -> Result<(), String> {
    const KEYS: &str = "11:22:33:44:55:66";
    const BUDS: &str = "AA:BB:CC:DD:EE:02";
    let sys = System::start(&[Feed::Bluetooth]);
    let world = Bus::system()?;
    let find = |s: &Seen, address: &str| -> Option<bluetooth::Device> {
        let b = s.bluetooth.as_ref()?;
        b.devices
            .iter()
            .chain(&b.nearby)
            .find(|d| d.address == address)
            .cloned()
    };

    let first = sys.until("the adapter", |s| {
        s.bluetooth.as_ref().filter(|b| b.available).cloned()
    })?;
    check(first.adapter_name == "mock-computer", || {
        format!("the adapter is called {:?}", first.adapter_name)
    })?;

    // A scan: the keyboard of the box is found, and not paired.
    sys.started
        .senders
        .bluetooth
        .tell(bluetooth::Command::Scan(true));
    sys.until("scanning", |s| {
        s.bluetooth.as_ref().filter(|b| b.discovering).map(|_| ())
    })?;
    let keys = sys.until("Mock Keys found", |s| find(s, KEYS))?;
    check(!keys.paired && !keys.connected, || {
        format!("Mock Keys {keys:?} starts out paired or connected")
    })?;

    // Another device shows up while scanning. (Its links are made to behave as BlueZ's do.)
    world.call(
        "org.bluez",
        "/org/bluez",
        "org.bluez.Mock",
        "AddDevice",
        &("hci0", BUDS, "Mock Buds"),
    )?;
    world.make_links_real("/org/bluez/hci0/dev_AA_BB_CC_DD_EE_02")?;
    sys.until("Mock Buds found while scanning", |s| find(s, BUDS))?;

    // Pairing pairs, trusts and connects: what the person wants of a new device.
    sys.started
        .senders
        .bluetooth
        .tell(bluetooth::Command::Pair(BUDS.into()));
    let paired = sys.until("Mock Buds paired", |s| find(s, BUDS).filter(|d| d.paired))?;
    check(
        sys.seen
            .borrow()
            .bluetooth
            .as_ref()
            .is_some_and(|b| b.devices.iter().any(|d| d.address == BUDS)),
        || format!("paired, but not among the paired devices: {paired:?}"),
    )?;
    sys.until("Mock Buds trusted and connected by the pairing", |s| {
        find(s, BUDS).filter(|d| d.trusted && d.connected)
    })
    .map_err(|e| format!("pairing should also trust and connect: {e}"))?;

    // The person disconnects it, and connects it again.
    sys.started
        .senders
        .bluetooth
        .tell(bluetooth::Command::Disconnect(BUDS.into()));
    sys.until("disconnected on request", |s| {
        find(s, BUDS).filter(|d| !d.connected)
    })?;
    sys.started
        .senders
        .bluetooth
        .tell(bluetooth::Command::Connect(BUDS.into()));
    sys.until("connected on request", |s| {
        find(s, BUDS).filter(|d| d.connected)
    })?;

    // The link drops from the other side.
    world.bluez("DisconnectDevice", "hci0", BUDS)?;
    sys.until("the link lost", |s| find(s, BUDS).filter(|d| !d.connected))
        .map_err(|e| format!("a device that drops its link still shows as connected: {e}"))?;

    // Trusted, renamed.
    sys.started
        .senders
        .bluetooth
        .tell(bluetooth::Command::SetTrusted(BUDS.into(), true));
    sys.until("trusted", |s| find(s, BUDS).filter(|d| d.trusted))?;
    sys.started
        .senders
        .bluetooth
        .tell(bluetooth::Command::Rename(BUDS.into(), "Desk Buds".into()));
    sys.until("renamed", |s| {
        find(s, BUDS).filter(|d| d.name == "Desk Buds")
    })
    .map_err(|e| format!("a rename doesn't show: {e}"))?;

    // The details page.
    sys.started
        .senders
        .bluetooth
        .tell(bluetooth::Command::Select(BUDS.into()));
    sys.until("the details", |s| {
        s.bluetooth
            .as_ref()?
            .detail
            .as_ref()
            .filter(|d| d.address == BUDS)
            .map(|_| ())
    })?;

    // Forget it: gone from the paired devices (it may still be seen by the scan).
    sys.started
        .senders
        .bluetooth
        .tell(bluetooth::Command::Forget(BUDS.into()));
    sys.until("Mock Buds forgotten", |s| {
        let b = s.bluetooth.as_ref()?;
        (!b.devices.iter().any(|d| d.address == BUDS)).then_some(())
    })?;
    check(
        sys.seen
            .borrow()
            .bluetooth
            .as_ref()
            .is_some_and(|b| b.detail.as_ref().is_none_or(|d| d.address != BUDS)),
        || "the details of a forgotten device are still open".into(),
    )?;

    // The adapter off: nothing connected, no scan. On again.
    sys.started
        .senders
        .bluetooth
        .tell(bluetooth::Command::Toggle);
    let off = sys.until("the adapter off", |s| {
        s.bluetooth.as_ref().filter(|b| !b.powered).cloned()
    })?;
    check(!off.discovering, || {
        "the adapter is off and still scanning".into()
    })?;
    sys.started
        .senders
        .bluetooth
        .tell(bluetooth::Command::Toggle);
    sys.until("the adapter on", |s| {
        s.bluetooth.as_ref().filter(|b| b.powered).map(|_| ())
    })?;
    Ok(())
}

/// Device batteries come and go and drain, as a mouse's does in a day.
fn peripheral_batteries_live() -> Result<(), String> {
    let sys = System::start(&[Feed::Power]);
    let world = Bus::system()?;
    let level = |s: &Seen, name: &str| -> Option<i32> {
        s.power
            .as_ref()?
            .peripherals
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.percent)
    };

    sys.until("the mouse at 55", |s| {
        level(s, "Mock Mouse").filter(|p| *p == 55)
    })?;

    // It drains.
    for percent in [40.0, 12.0, 3.0] {
        world.set_battery("mock_mouse", percent)?;
        sys.until(&format!("the mouse at {percent}"), |s| {
            level(s, "Mock Mouse").filter(|p| *p == percent as i32)
        })
        .map_err(|e| format!("a draining battery isn't followed: {e}"))?;
    }

    // A keyboard and a phone are put on; each with its own kind.
    world.add_peripheral("mock_keyboard", "Mock Board", 6, 80.0)?;
    world.add_peripheral("mock_phone", "Mock Phone", 8, 64.0)?;
    let state = sys
        .until("a keyboard and a phone", |s| {
            let p = s.power.as_ref()?;
            (level(s, "Mock Board") == Some(80) && level(s, "Mock Phone") == Some(64))
                .then(|| p.clone())
        })
        .map_err(|e| format!("a device that appears while running isn't picked up: {e}"))?;
    let kind = |name: &str| {
        state
            .peripherals
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.kind)
    };
    check(
        kind("Mock Board") == Some(power::DeviceKind::Keyboard),
        || format!("the keyboard is a {:?}", kind("Mock Board")),
    )?;
    check(kind("Mock Mouse") == Some(power::DeviceKind::Mouse), || {
        format!("the mouse is a {:?}", kind("Mock Mouse"))
    })?;

    // One goes away.
    world.remove_peripheral("mock_keyboard")?;
    sys.until("the keyboard gone", |s| {
        let p = s.power.as_ref()?;
        (!p.peripherals.iter().any(|p| p.name == "Mock Board")).then_some(())
    })
    .map_err(|e| format!("a device that goes away stays listed: {e}"))?;
    check(level(&sys.seen.borrow(), "Mock Mouse") == Some(3), || {
        "the mouse lost its level".into()
    })?;
    Ok(())
}

/// A busy notification session: several programs, an urgent one, a replacement, a program that
/// withdraws its own, do not disturb, an action, dismissing and clearing.
fn notifications_busy_session() -> Result<(), String> {
    let sys = System::start(&[Feed::Notifications]);
    let world = Bus::session()?;
    let with = |s: &Seen, id: u32| -> Option<notifications::Notification> {
        s.notes.as_ref()?.items.iter().find(|n| n.id == id).cloned()
    };
    sys.until("being the server", |s| {
        s.notes.as_ref().filter(|n| n.server).map(|_| ())
    })?;

    // The server tells programs what it can.
    let info = world.call(
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
        "GetServerInformation",
        &(),
    )?;
    let (name, _vendor, _version, spec): (String, String, String, String) = info
        .body()
        .deserialize()
        .map_err(|e| format!("GetServerInformation: {e}"))?;
    check(!name.is_empty() && spec.starts_with("1."), || {
        format!("server {name:?}, spec {spec:?}")
    })?;

    // Three programs.
    let mail = world.notify(
        "mail",
        0,
        "Inbox",
        "3 new messages",
        &["default", "Open"],
        1,
    )?;
    let build = world.notify("build", 0, "Build failed", "exit 101", &[], 2)?;
    let chat = world.notify("chat", 0, "Hello", "are you there?", &[], 1)?;
    check(mail != build && build != chat && mail != chat, || {
        format!("ids {mail}, {build}, {chat} aren't distinct")
    })?;
    let state = sys.until("all three kept", |s| {
        let n = s.notes.as_ref()?;
        (with(s, mail).is_some() && with(s, build).is_some() && with(s, chat).is_some())
            .then(|| n.clone())
    })?;
    check(state.items[0].id == chat, || {
        format!("newest first, but {:?} is on top", state.items[0].summary)
    })?;
    check(
        with(&sys.seen.borrow(), build).is_some_and(|n| n.urgent),
        || "urgency 2 isn't urgent".into(),
    )?;
    check(
        !with(&sys.seen.borrow(), chat).is_some_and(|n| n.urgent),
        || "a normal one is urgent".into(),
    )?;
    check(
        with(&sys.seen.borrow(), mail).is_some_and(|n| n.has_default()),
        || "the default action is lost".into(),
    )?;

    // A program replaces its own: same id, new text, still one entry.
    let again = world.notify("chat", chat, "Hello?", "anyone?", &[], 1)?;
    check(again == chat, || format!("replacing {chat} gave {again}"))?;
    sys.until("the replacement", |s| {
        with(s, chat).filter(|n| n.summary == "Hello?")
    })?;
    check(
        sys.seen
            .borrow()
            .notes
            .as_ref()
            .is_some_and(|n| n.items.iter().filter(|i| i.id == chat).count() == 1),
        || "a replaced notification is listed twice".into(),
    )?;

    // A program withdraws one.
    world.close_notification(build)?;
    sys.until("the build one withdrawn", |s| {
        s.notes
            .as_ref()
            .filter(|_| with(s, build).is_none())
            .map(|_| ())
    })
    .map_err(|e| format!("CloseNotification doesn't remove it: {e}"))?;

    // Do not disturb: kept, counted as missed.
    sys.started
        .senders
        .notifications
        .tell(notifications::Command::SetDnd(notifications::Dnd::Forever));
    sys.until("do not disturb on", |s| {
        s.notes
            .as_ref()
            .filter(|n| n.dnd == notifications::Dnd::Forever)
            .map(|_| ())
    })?;
    let quiet = world.notify("news", 0, "Headline", "", &[], 1)?;
    let n = sys.until("a notification during do not disturb", |s| {
        s.notes
            .as_ref()
            .filter(|_| with(s, quiet).is_some())
            .cloned()
    })?;
    check(n.missed >= 1, || {
        format!("{} missed during do not disturb", n.missed)
    })?;
    sys.started
        .senders
        .notifications
        .tell(notifications::Command::SetDnd(notifications::Dnd::Off));
    sys.until("do not disturb off", |s| {
        s.notes
            .as_ref()
            .filter(|n| n.dnd == notifications::Dnd::Off)
            .map(|_| ())
    })?;

    // Dismiss one; clear the rest.
    sys.started
        .senders
        .notifications
        .tell(notifications::Command::Dismiss(mail));
    sys.until("the mail dismissed", |s| {
        s.notes
            .as_ref()
            .filter(|_| with(s, mail).is_none())
            .map(|_| ())
    })?;
    sys.started
        .senders
        .notifications
        .tell(notifications::Command::Clear);
    sys.until("everything cleared", |s| {
        s.notes.as_ref().filter(|n| n.items.is_empty()).map(|_| ())
    })?;
    Ok(())
}

/// The whole system under strain: every source up, the world changing and the person tapping at
/// once, then stopping, and starting again.
fn the_whole_system_under_strain() -> Result<(), String> {
    // Everything, as the dashboard starts it.
    let (sink, events) = EventSender::channel();
    let started = live(
        sink,
        Options {
            feeds: None,
            wayland: None,
            gamma: false,
        },
    )
    .map_err(|e| format!("the system doesn't start: {e}"))?;
    started
        .senders
        .host
        .tell(niri_dash_system::host::Command::Visible(true));
    let sys = System {
        started,
        events,
        seen: Default::default(),
    };
    let world = Bus::system()?;
    sys.until(
        "wifi, bluetooth, power and notifications all reporting",
        |s| {
            (s.wifi.is_some() && s.bluetooth.is_some() && s.power.is_some() && s.notes.is_some())
                .then_some(())
        },
    )?;

    // A flood of commands, while the air and the batteries change underneath.
    for round in 0..60u32 {
        sys.started.senders.wifi.tell(wifi::Command::Toggle);
        sys.started
            .senders
            .bluetooth
            .tell(bluetooth::Command::Toggle);
        sys.started.senders.wifi.tell(wifi::Command::Rescan);
        sys.started
            .senders
            .power
            .tell(power::Command::SetProfile(power::Profile::Balanced));
        sys.started
            .senders
            .notifications
            .tell(notifications::Command::AddLocal {
                app: "strain".into(),
                summary: format!("round {round}"),
                body: String::new(),
            });
        if round % 10 == 0 {
            let name = format!("strain_ap{round}");
            world.add_ap(
                &name,
                &format!("Strain {round}"),
                &format!("00:23:F8:7E:{round:02X}:00"),
                2412,
                40,
                0,
            )?;
        }
        if round % 10 == 5 {
            world.set_battery("mock_mouse", f64::from(90 - round))?;
        }
    }

    // Wait for it to settle: the radios stop changing.
    let radios = |s: &Seen| Some((s.wifi.as_ref()?.enabled, s.bluetooth.as_ref()?.powered));
    let mut now = radios(&sys.seen.borrow());
    let mut changed = Instant::now();
    while changed.elapsed() < Duration::from_secs(2) && changed.elapsed() < Duration::from_secs(30)
    {
        if let Ok(e) = sys.events.recv_timeout(Duration::from_millis(100)) {
            sys.seen.borrow_mut().take(e);
        }
        let seen = radios(&sys.seen.borrow());
        if seen != now {
            now = seen;
            changed = Instant::now();
        }
    }

    // What it shows is what is so: the mocked radios say the same.
    let truth_wifi = world.get_bool(nm::SERVICE, nm::PATH, nm::SERVICE, "WirelessEnabled")?;
    let truth_bluetooth = world.get_bool(
        "org.bluez",
        "/org/bluez/hci0",
        "org.bluez.Adapter1",
        "Powered",
    )?;
    println!(
        "    after 60 toggles each: Wi-Fi {}, Bluetooth {} (an even count; an odd end means two taps made one)",
        if truth_wifi { "on" } else { "off" },
        if truth_bluetooth { "on" } else { "off" },
    );
    check(now == Some((truth_wifi, truth_bluetooth)), || {
        format!(
            "it shows (Wi-Fi, Bluetooth) = {now:?}, the radios are ({truth_wifi}, {truth_bluetooth})"
        )
    })?;

    // None of the 60 notifications was lost, whatever else was going on.
    let strain = sys
        .until("sixty notifications of the strain", |s| {
            let n = s.notes.as_ref()?;
            let count = n.items.iter().filter(|i| i.app == "strain").count();
            (count == 60).then_some(count)
        })
        .map_err(|e| format!("notifications were lost in the flood: {e}"))?;
    check(strain == 60, || format!("{strain} of 60"))?;

    // The system still answers: bring the radios up if the flood left them down.
    if !truth_wifi {
        sys.started.senders.wifi.tell(wifi::Command::Toggle);
    }
    if !truth_bluetooth {
        sys.started
            .senders
            .bluetooth
            .tell(bluetooth::Command::Toggle);
    }
    let air = sys.until("the radios on, and the networks the strain made", |s| {
        let w = s.wifi.as_ref()?;
        (w.enabled
            && s.bluetooth.as_ref()?.powered
            && w.networks.iter().any(|n| n.ssid == "Strain 50"))
        .then(|| w.clone())
    })?;
    check(
        air.networks.windows(2).all(|w| w[0].signal >= w[1].signal),
        || "the list lost its order".into(),
    )?;
    let mut keys: Vec<_> = air.networks.iter().map(|n| n.key()).collect();
    let all = keys.len();
    keys.sort();
    keys.dedup();
    check(keys.len() == all, || "a network is listed twice".into())?;

    // Stop it by letting go of every sender; a new system takes the bus names over.
    drop(sys);
    let again = System::start(&[Feed::Notifications]);
    again
        .until("a second system becoming the notification server", |s| {
            s.notes.as_ref().filter(|n| n.server).map(|_| ())
        })
        .map_err(|e| {
            format!("the system doesn't let go of the notification server when it stops: {e}")
        })?;
    Ok(())
}

/// Taps on a radio's switch in quick succession. What must hold: the dashboard shows the radio as
/// it is. What it prints, and doesn't assert: where n taps leave the radio. Known: `Toggle` aims
/// at the opposite of the last state it was *told*, so taps that come before that state changes
/// all aim at the same target, and any number of quick taps does the work of one.
fn quick_taps_on_a_radio() -> Result<(), String> {
    let sys = System::start(&[Feed::Wifi, Feed::Bluetooth]);
    let world = Bus::system()?;
    sys.until("both radios on", |s| {
        (s.wifi.as_ref()?.enabled && s.bluetooth.as_ref()?.powered).then_some(())
    })?;
    let radios = || -> Result<(bool, bool), String> {
        Ok((
            world.get_bool(nm::SERVICE, nm::PATH, nm::SERVICE, "WirelessEnabled")?,
            world.get_bool(
                "org.bluez",
                "/org/bluez/hci0",
                "org.bluez.Adapter1",
                "Powered",
            )?,
        ))
    };
    for taps in 1..=4 {
        for _ in 0..taps {
            sys.started.senders.wifi.tell(wifi::Command::Toggle);
            sys.started
                .senders
                .bluetooth
                .tell(bluetooth::Command::Toggle);
        }
        std::thread::sleep(Duration::from_secs(3));
        let truth = radios()?;
        println!("    {taps} quick tap(s) from (on, on): (Wi-Fi, Bluetooth) = {truth:?}");
        sys.until("what it shows being what is so", |s| {
            ((s.wifi.as_ref()?.enabled, s.bluetooth.as_ref()?.powered) == truth).then_some(())
        })?;
        // Back to on, whatever it ended as, one tap at a time.
        for _ in 0..2 {
            let (w, b) = radios()?;
            if w && b {
                break;
            }
            if !w {
                sys.started.senders.wifi.tell(wifi::Command::Toggle);
            }
            if !b {
                sys.started
                    .senders
                    .bluetooth
                    .tell(bluetooth::Command::Toggle);
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    }
    let (wifi_on, bluetooth_on) = (radios()?.0, radios()?.1);
    check(wifi_on && bluetooth_on, || {
        "couldn't get the radios back on".into()
    })
}

fn main() {
    if !inside::inside() {
        inside::skipped("live");
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
        ("wifi: follows the air", wifi_follows_the_air),
        ("bluetooth: a session", bluetooth_session),
        (
            "power: batteries come, drain and go",
            peripheral_batteries_live,
        ),
        ("notifications: a busy session", notifications_busy_session),
        (
            "system: under strain, then again",
            the_whole_system_under_strain,
        ),
        ("radios: quick taps", quick_taps_on_a_radio),
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

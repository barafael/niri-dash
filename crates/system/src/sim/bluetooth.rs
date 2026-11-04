//! Simulated Bluetooth: paired devices that connect and draw down their batteries on their own
//! schedule, strangers a scan turns up, and pairing with its prompts.

use super::time::{hash, into, mix, noise};
use crate::bluetooth::{Activity, Answer, Command, Device, Prompt, PromptKind, State};
use crate::power::DeviceKind;

/// How long a scan runs by itself, and how long connecting and pairing steps take.
const SCAN: f64 = 40.0;
const CONNECTING: f64 = 1.3;
const PAIRING: f64 = 0.9;

/// The headphones are on for seven minutes of eleven, unless someone took charge of them.
const HEADPHONES_CYCLE: f64 = 660.0;
const HEADPHONES_ON: f64 = 420.0;
const HEADPHONES: &str = "AC:80:0A:11:22:33";
/// How long a device someone connected or disconnected stays that way.
const HANDS_OFF: f64 = 900.0;

fn device(address: &str, name: &str, kind: DeviceKind, profiles: &[&'static str]) -> Device {
    Device {
        path: format!("/org/bluez/hci0/dev_{}", address.replace(':', "_")),
        address: address.into(),
        name: name.into(),
        claimed: String::new(),
        named: true,
        kind,
        paired: false,
        trusted: false,
        blocked: false,
        connected: false,
        legacy: false,
        random_address: false,
        battery: -1,
        rssi: None,
        profiles: profiles.to_vec(),
        busy: false,
    }
}

fn paired(address: &str, name: &str, kind: DeviceKind, profiles: &[&'static str]) -> Device {
    Device {
        paired: true,
        trusted: true,
        ..device(address, name, kind, profiles)
    }
}

/// Who a scan can find, in the order they show up.
fn strangers() -> Vec<Device> {
    vec![
        device(
            "5C:44:3E:90:12:01",
            "Kitchen Speaker",
            DeviceKind::Headphones,
            &["Audio sink"],
        ),
        device(
            "D4:9A:20:71:0C:55",
            "Bike Computer",
            DeviceKind::Other,
            &["Heart rate"],
        ),
        Device {
            claimed: "BT-KB-3".into(),
            legacy: true,
            ..device(
                "00:1A:7D:DA:71:13",
                "Old Keyboard",
                DeviceKind::Keyboard,
                &["Human interface device"],
            )
        },
        Device {
            random_address: true,
            ..device(
                "7A:12:9F:3B:40:A2",
                "Galaxy S24",
                DeviceKind::Phone,
                &["Phonebook", "Audio source"],
            )
        },
    ]
}

/// A step that finishes later.
enum Pending {
    Connect(String, bool),
    /// Pairing got as far as the prompt.
    Ask(String),
    /// The prompt was answered: pairing is done.
    Paired(String),
}

pub struct Bluetooth {
    powered: bool,
    devices: Vec<Device>,
    /// Devices left alone by the schedule until this time, by address.
    hands_off: Vec<(String, f64)>,
    forgotten: Vec<String>,
    scan_until: f64,
    pending: Vec<(f64, Pending)>,
    prompt: Option<Prompt>,
    activity: Activity,
    selected: String,
    serial: u64,
}

impl Bluetooth {
    pub fn new() -> Self {
        let mut keys = paired(
            "F4:73:35:44:55:66",
            "MX Keys",
            DeviceKind::Keyboard,
            &["Human interface device"],
        );
        keys.connected = true;
        let mut mouse = paired(
            "F4:73:35:77:88:99",
            "MX Master 3S",
            DeviceKind::Mouse,
            &["Human interface device"],
        );
        mouse.connected = true;
        mouse.random_address = true;
        Self {
            powered: true,
            devices: vec![
                paired(
                    HEADPHONES,
                    "Fairbuds",
                    DeviceKind::Headphones,
                    &["Audio sink", "Hands-free"],
                ),
                keys,
                mouse,
                paired(
                    "A0:C5:89:0B:CD:EF",
                    "Pixel",
                    DeviceKind::Phone,
                    &["Audio source", "Phonebook"],
                ),
            ],
            hands_off: Vec::new(),
            forgotten: Vec::new(),
            scan_until: 0.0,
            pending: Vec::new(),
            prompt: None,
            activity: Activity::Idle,
            selected: String::new(),
            serial: 0,
        }
    }

    fn held(&self, address: &str, t: f64) -> bool {
        self.hands_off
            .iter()
            .any(|(a, until)| a == address && t < *until)
    }

    fn hold(&mut self, address: &str, t: f64) {
        self.hands_off.retain(|(a, _)| a != address);
        self.hands_off.push((address.to_string(), t + HANDS_OFF));
    }

    fn dev(&mut self, address: &str) -> Option<&mut Device> {
        self.devices.iter_mut().find(|d| d.address == address)
    }

    /// The headphones' battery while they are connected.
    pub fn headphones(&self) -> Option<i32> {
        self.devices
            .iter()
            .find(|d| d.address == HEADPHONES && d.connected)
            .map(|d| d.battery)
    }

    /// Follow the schedule and finish what is due.
    pub fn advance(&mut self, t: f64) {
        let wanted = self.powered && into(t, HEADPHONES_CYCLE) < HEADPHONES_ON;
        if !self.held(HEADPHONES, t)
            && !self.forgotten.iter().any(|a| a == HEADPHONES)
            && let Some(d) = self.dev(HEADPHONES)
            && !d.busy
            && !d.blocked
        {
            d.connected = wanted;
        }
        for d in &mut self.devices {
            let seed = d.address.bytes().map(u64::from).sum::<u64>();
            d.battery = match d.kind {
                _ if !d.connected => -1,
                DeviceKind::Headphones => mix(95.0, 25.0, into(t, 28_000.0) / 28_000.0) as i32,
                DeviceKind::Mouse | DeviceKind::Keyboard => {
                    (40.0 + 55.0 * hash(seed, (t / 7200.0) as u64)) as i32
                }
                _ => -1,
            };
            d.rssi = d
                .connected
                .then(|| (-48.0 - 22.0 * noise(t, 13.0, seed)) as i16);
        }
        self.finish_due(t);
    }

    /// Finish the steps that are due.
    fn finish_due(&mut self, t: f64) {
        let mut later = Vec::new();
        for (at, step) in std::mem::take(&mut self.pending) {
            if at > t {
                later.push((at, step));
                continue;
            }
            match step {
                Pending::Connect(address, on) => {
                    if let Some(d) = self.dev(&address) {
                        d.busy = false;
                        d.connected = on;
                    }
                }
                Pending::Ask(address) => self.ask(&address, t),
                Pending::Paired(address) => {
                    if let Some(mut d) = strangers().into_iter().find(|d| d.address == address) {
                        d.paired = true;
                        d.connected = true;
                        d.trusted = true;
                        self.forgotten.retain(|a| *a != address);
                        self.devices.retain(|known| known.address != address);
                        self.devices.push(d);
                    } else if let Some(d) = self.dev(&address) {
                        d.paired = true;
                        d.connected = true;
                    }
                    self.activity = Activity::Idle;
                }
            }
        }
        self.pending.extend(later);
    }

    fn ask(&mut self, address: &str, t: f64) {
        let Some(d) = strangers().into_iter().find(|d| d.address == address) else {
            return;
        };
        self.serial += 1;
        let kind = if d.legacy {
            PromptKind::EnterPin
        } else if d.kind == DeviceKind::Keyboard {
            PromptKind::ShowPasskey((hash(81, t as u64) * 999_999.0) as u32, 0)
        } else if d.kind == DeviceKind::Headphones {
            PromptKind::Authorize
        } else {
            PromptKind::Confirm((hash(82, t as u64) * 999_999.0) as u32)
        };
        self.prompt = Some(Prompt {
            serial: self.serial,
            name: d.name,
            address: address.to_string(),
            kind,
        });
    }

    pub fn state(&self, t: f64) -> State {
        let discovering = self.powered && t < self.scan_until;
        let mut nearby = Vec::new();
        let mut unnamed = 0;
        if discovering {
            // Who has answered so far, as a scan fills up over its first seconds.
            let age = t - (self.scan_until - SCAN);
            for (i, mut d) in strangers().into_iter().enumerate() {
                let known = self.devices.iter().any(|k| k.address == d.address);
                if age > 1.0 + i as f64 * 2.5 && !known {
                    d.rssi = Some((-52.0 - 30.0 * noise(t, 6.0, 90 + i as u64)) as i16);
                    nearby.push(d);
                }
            }
            nearby.sort_by(|a, b| b.rssi.cmp(&a.rssi).then(a.name.cmp(&b.name)));
            unnamed = (age / 5.0) as usize;
        }
        let mut devices = if self.powered {
            self.devices.clone()
        } else {
            let mut off = self.devices.clone();
            for d in &mut off {
                d.connected = false;
                d.battery = -1;
                d.rssi = None;
            }
            off
        };
        devices.sort_by(|a, b| {
            b.connected
                .cmp(&a.connected)
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        let detail = (!self.selected.is_empty())
            .then(|| {
                devices
                    .iter()
                    .chain(&nearby)
                    .find(|d| d.address == self.selected)
                    .cloned()
            })
            .flatten();
        State {
            available: true,
            powered: self.powered,
            blocked: false,
            adapter: "/org/bluez/hci0".into(),
            adapter_name: "laptop".into(),
            discovering,
            devices,
            nearby,
            unnamed,
            activity: self.activity.clone(),
            prompt: self.prompt.clone(),
            detail,
        }
    }

    pub fn handle(&mut self, cmd: Command, t: f64) {
        match cmd {
            Command::Toggle => {
                self.powered = !self.powered;
                if !self.powered {
                    self.scan_until = 0.0;
                    self.prompt = None;
                    self.activity = Activity::Idle;
                }
            }
            Command::Scan(on) => self.scan_until = if on { t + SCAN } else { 0.0 },
            Command::Connect(address) => {
                self.hold(&address, t);
                if let Some(d) = self.dev(&address) {
                    d.busy = true;
                    self.pending
                        .push((t + CONNECTING, Pending::Connect(address, true)));
                }
            }
            Command::Disconnect(address) => {
                self.hold(&address, t);
                if let Some(d) = self.dev(&address) {
                    d.busy = true;
                    self.pending
                        .push((t + CONNECTING / 2.0, Pending::Connect(address, false)));
                }
            }
            Command::SetTrusted(address, on) => {
                if let Some(d) = self.dev(&address) {
                    d.trusted = on;
                }
            }
            Command::SetBlocked(address, on) => {
                self.hold(&address, t);
                if let Some(d) = self.dev(&address) {
                    d.blocked = on;
                    if on {
                        d.connected = false;
                    }
                }
            }
            Command::Forget(address) => {
                self.forgotten.push(address.clone());
                self.devices.retain(|d| d.address != address);
                if self.selected == address {
                    self.selected.clear();
                }
            }
            Command::Rename(address, name) => {
                if let Some(d) = self.dev(&address) {
                    d.claimed = d.name.clone();
                    d.name = name;
                }
            }
            Command::Pair(address) => {
                let name = strangers()
                    .into_iter()
                    .find(|d| d.address == address)
                    .map_or_else(|| address.clone(), |d| d.name);
                self.activity = Activity::Pairing(name);
                self.pending.push((t + PAIRING, Pending::Ask(address)));
            }
            Command::CancelPairing | Command::Decline => {
                self.prompt = None;
                self.activity = Activity::Idle;
                self.pending
                    .retain(|(_, p)| !matches!(p, Pending::Ask(_) | Pending::Paired(_)));
            }
            Command::Answer { serial, answer } => {
                let Some(prompt) = self.prompt.take_if(|p| p.serial == serial) else {
                    return;
                };
                if let (PromptKind::EnterPin, Answer::Code(code)) = (&prompt.kind, &answer)
                    && code.is_empty()
                {
                    self.activity = Activity::Failed("The PIN was empty".into());
                    return;
                }
                self.pending
                    .push((t + PAIRING, Pending::Paired(prompt.address)));
            }
            Command::Select(address) => self.selected = address,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: f64 = 1_800_000_000.0;

    fn stranger(b: &Bluetooth, name: &str, t: f64) -> String {
        b.state(t)
            .nearby
            .into_iter()
            .find(|d| d.name == name)
            .map(|d| d.address)
            .expect("found by the scan")
    }

    #[test]
    fn the_headphones_follow_their_schedule_until_someone_steps_in() {
        let on = T - into(T, HEADPHONES_CYCLE) + HEADPHONES_CYCLE + 10.0;
        let off = on + HEADPHONES_ON + 10.0;
        let mut b = Bluetooth::new();
        b.advance(on);
        assert!(b.headphones().is_some());
        b.advance(off);
        assert!(b.headphones().is_none());
        b.handle(Command::Connect(HEADPHONES.into()), off);
        b.finish_due(off + CONNECTING);
        b.advance(off + CONNECTING);
        assert!(b.headphones().is_some(), "stays connected as asked");
    }

    #[test]
    fn a_scan_finds_strangers_and_stops_by_itself() {
        let mut b = Bluetooth::new();
        b.handle(Command::Scan(true), T);
        assert!(b.state(T).nearby.is_empty());
        assert!(b.state(T + 15.0).nearby.len() >= 3);
        assert!(!b.state(T + SCAN + 1.0).discovering);
        assert!(b.state(T + SCAN + 1.0).nearby.is_empty());
    }

    #[test]
    fn pairing_goes_through_its_prompt() {
        let mut b = Bluetooth::new();
        b.handle(Command::Scan(true), T);
        let t = T + 15.0;
        let address = stranger(&b, "Galaxy S24", t);
        b.handle(Command::Pair(address.clone()), t);
        assert_eq!(b.state(t).activity, Activity::Pairing("Galaxy S24".into()));
        let t = t + PAIRING;
        b.finish_due(t);
        let prompt = b.state(t).prompt.expect("a prompt");
        assert!(matches!(prompt.kind, PromptKind::Confirm(_)));
        // An answer to an earlier prompt goes nowhere.
        b.handle(
            Command::Answer {
                serial: prompt.serial + 1,
                answer: Answer::Yes,
            },
            t,
        );
        assert!(b.state(t).prompt.is_some());
        b.handle(
            Command::Answer {
                serial: prompt.serial,
                answer: Answer::Yes,
            },
            t,
        );
        let t = t + PAIRING;
        b.finish_due(t);
        let state = b.state(t);
        assert_eq!(state.activity, Activity::Idle);
        let d = state
            .devices
            .iter()
            .find(|d| d.address == address)
            .expect("paired now");
        assert!(d.paired && d.connected);
        assert!(state.nearby.iter().all(|d| d.address != address));
    }

    #[test]
    fn declining_leaves_it_unpaired() {
        let mut b = Bluetooth::new();
        b.handle(Command::Scan(true), T);
        let t = T + 15.0;
        let address = stranger(&b, "Old Keyboard", t);
        b.handle(Command::Pair(address.clone()), t);
        b.finish_due(t + PAIRING);
        assert!(matches!(
            b.state(t).prompt.expect("a prompt").kind,
            PromptKind::EnterPin
        ));
        b.handle(Command::Decline, t);
        assert_eq!(b.state(t).prompt, None);
        assert!(b.state(t).devices.iter().all(|d| d.address != address));
    }

    #[test]
    fn switching_off_disconnects_everything_on_show() {
        let mut b = Bluetooth::new();
        b.advance(T);
        b.handle(Command::Toggle, T);
        assert!(b.state(T).devices.iter().all(|d| !d.connected));
        b.handle(Command::Toggle, T);
        assert!(b.state(T).devices.iter().any(|d| d.connected));
    }

    #[test]
    fn forgotten_stays_forgotten() {
        let mut b = Bluetooth::new();
        b.handle(Command::Forget(HEADPHONES.into()), T);
        let on = T - into(T, HEADPHONES_CYCLE) + HEADPHONES_CYCLE + 10.0;
        b.advance(on);
        assert!(b.headphones().is_none());
        assert!(b.state(on).devices.iter().all(|d| d.address != HEADPHONES));
    }

    #[test]
    fn details_follow_the_device() {
        let mut b = Bluetooth::new();
        b.advance(T);
        b.handle(Command::Select("F4:73:35:44:55:66".into()), T);
        assert_eq!(b.state(T).detail.expect("details").name, "MX Keys");
        b.handle(
            Command::Rename("F4:73:35:44:55:66".into(), "Desk keyboard".into()),
            T,
        );
        let detail = b.state(T).detail.expect("details");
        assert_eq!(detail.name, "Desk keyboard");
        assert_eq!(detail.claimed, "MX Keys");
    }
}

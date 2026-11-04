//! Simulated Wi-Fi: networks that drift in and out of range, and everything the widget can ask for,
//! from joining with a password through a captive portal to showing a saved network's QR code.

use super::time::{hash, into, noise, wave};
use crate::wifi::{
    Activity, Answer, Command, Connectivity, Detail, Link, Network, Profile, Prompt, PromptKind,
    Secret, Security, State, key, qr_image,
};

/// How long joining a network and scanning for them take.
const JOINING: f64 = 1.6;
const SCANNING: f64 = 2.4;

/// A network that exists in this world.
struct Spec {
    ssid: &'static str,
    security: Security,
    wpa3: bool,
    /// Bit 0: 2.4 GHz, bit 1: 5 GHz, bit 2: 6 GHz.
    bands: u8,
    aps: usize,
    /// The usual signal, in percent.
    signal: f64,
    /// A network out of range part of the time: the cycle, and how much of it is in range.
    comes_and_goes: Option<(f64, f64)>,
    /// Its login page has to be passed first.
    portal: bool,
}

const fn net(ssid: &'static str, security: Security, bands: u8, signal: f64) -> Spec {
    Spec {
        ssid,
        security,
        wpa3: false,
        bands,
        aps: 1,
        signal,
        comes_and_goes: None,
        portal: false,
    }
}

const WORLD: [Spec; 8] = [
    Spec {
        wpa3: true,
        aps: 2,
        ..net("Lighthouse", Security::Personal, 0b011, 82.0)
    },
    Spec {
        aps: 3,
        ..net("eduroam", Security::Enterprise, 0b011, 58.0)
    },
    net("FRITZ!Box 7590 QX", Security::Personal, 0b011, 44.0),
    Spec {
        portal: true,
        ..net("Harbour Café", Security::Open, 0b001, 51.0)
    },
    Spec {
        comes_and_goes: Some((1260.0, 700.0)),
        ..net("Vodafone Hotspot", Security::Open, 0b001, 33.0)
    },
    Spec {
        wpa3: true,
        ..net("Attic 6E", Security::Personal, 0b110, 29.0)
    },
    net("Old Router", Security::Wep, 0b001, 22.0),
    Spec {
        comes_and_goes: Some((700.0, 260.0)),
        ..net("Pixel Hotspot", Security::Personal, 0b010, 66.0)
    },
];

fn key_mgmt_of(security: Security) -> &'static str {
    match security {
        Security::Personal => "wpa-psk",
        Security::Enterprise => "wpa-eap",
        Security::Wep => "none",
        Security::Owe => "owe",
        Security::Open => "",
    }
}

fn profile(ssid: &str, security: Security, timestamp: u64) -> Profile {
    let seed = ssid
        .bytes()
        .fold(7, |h: u64, b| h.wrapping_mul(131).wrapping_add(b.into()));
    Profile {
        path: format!("/org/freedesktop/NetworkManager/Settings/{seed}"),
        uuid: format!("{:016x}", (hash(51, seed) * u64::MAX as f64) as u64),
        name: ssid.into(),
        ssid: ssid.into(),
        autoconnect: true,
        hidden: false,
        key_mgmt: key_mgmt_of(security).into(),
        timestamp,
        hotspot: false,
    }
}

pub struct Wifi {
    enabled: bool,
    /// The key of the network we are on.
    active: Option<String>,
    /// The key being joined, and when that is done.
    joining: Option<(String, f64)>,
    scan_until: f64,
    saved: Vec<Profile>,
    /// A sign-in page is in the way on the current network.
    portal: bool,
    /// A hidden network joined by hand, in range from then on.
    hidden: Vec<(String, Security)>,
    prompt: Option<Prompt>,
    /// The network the prompt is about.
    prompt_key: String,
    activity: Activity,
    selected: String,
    secret: Option<Secret>,
    serial: u64,
}

impl Wifi {
    pub fn new(t: f64) -> Self {
        let day = t as u64;
        let saved = vec![
            profile("Lighthouse", Security::Personal, day.saturating_sub(120)),
            profile(
                "eduroam",
                Security::Enterprise,
                day.saturating_sub(9 * 86_400),
            ),
            profile(
                "Harbour Café",
                Security::Open,
                day.saturating_sub(23 * 86_400),
            ),
            profile(
                "Mum & Dad",
                Security::Personal,
                day.saturating_sub(61 * 86_400),
            ),
        ];
        Self {
            enabled: true,
            active: Some(key("Lighthouse", Security::Personal)),
            joining: None,
            scan_until: 0.0,
            saved,
            portal: false,
            hidden: Vec::new(),
            prompt: None,
            prompt_key: String::new(),
            activity: Activity::Idle,
            selected: String::new(),
            secret: None,
            serial: 0,
        }
    }

    /// Whether we are on a network, with the internet behind it.
    pub fn online(&self) -> bool {
        self.enabled && self.active.is_some() && !self.portal
    }

    fn profile_of(&self, key_wanted: &str) -> Option<Profile> {
        self.saved.iter().find(|p| p.key() == key_wanted).cloned()
    }

    /// The networks in range right now, the one we are on first, then by signal.
    fn networks(&self, t: f64) -> Vec<Network> {
        let specs = WORLD.iter().enumerate().filter(|(_, s)| {
            s.comes_and_goes
                .is_none_or(|(cycle, up)| into(t, cycle) < up)
        });
        let mut list: Vec<Network> = specs
            .map(|(i, s)| {
                let seed = 60 + i as u64;
                let signal = s.signal
                    + 10.0 * (noise(t, 17.0, seed) - 0.5)
                    + 8.0 * (wave(t, 293.0 + i as f64 * 31.0, 0.0) - 0.5);
                let k = key(s.ssid, s.security);
                Network {
                    ssid: s.ssid.into(),
                    signal: signal.clamp(5.0, 99.0) as i32,
                    security: s.security,
                    wpa2: s.security == Security::Personal,
                    wpa3: s.wpa3,
                    bands: s.bands,
                    aps: s.aps,
                    ap: format!("/org/freedesktop/NetworkManager/AccessPoint/{i}"),
                    active: self.active.as_deref() == Some(k.as_str()),
                    profile: self.profile_of(&k),
                }
            })
            .collect();
        for (ssid, security) in &self.hidden {
            let k = key(ssid, *security);
            list.push(Network {
                ssid: ssid.clone(),
                signal: 47,
                security: *security,
                wpa2: *security == Security::Personal,
                bands: 0b010,
                aps: 1,
                active: self.active.as_deref() == Some(k.as_str()),
                profile: self.profile_of(&k),
                ..Default::default()
            });
        }
        list.sort_by(|a, b| b.active.cmp(&a.active).then(b.signal.cmp(&a.signal)));
        list
    }

    fn link(&self, network: &Network, t: f64) -> Link {
        let channel_freq = if network.bands & 0b100 != 0 {
            5975
        } else if network.bands & 0b010 != 0 {
            5240
        } else {
            2437
        };
        Link {
            ip4: vec!["192.168.1.23/24".into()],
            gateway4: "192.168.1.1".into(),
            dns: vec!["192.168.1.1".into(), "9.9.9.9".into()],
            ip6: vec![
                "fe80::a11:22ff:fe33:4455/64".into(),
                "2a02:810b:c3f:1::23/64".into(),
            ],
            gateway6: "fe80::1".into(),
            bitrate_kbps: 1000
                * (260 + (network.signal as u32) * 9 + (40.0 * noise(t, 11.0, 70)) as u32),
            freq: channel_freq,
            bssid: "AC:80:0A:7F:21:4E".into(),
            mac: "F4:7B:09:11:22:33".into(),
            connectivity: if self.portal {
                Connectivity::Portal
            } else {
                Connectivity::Full
            },
        }
    }

    /// Finish what is due: a network joined, a scan done.
    pub fn advance(&mut self, t: f64) {
        if self.joining.as_ref().is_some_and(|(_, at)| t >= *at)
            && let Some((k, _)) = self.joining.take()
        {
            self.portal = WORLD
                .iter()
                .any(|s| s.portal && key(s.ssid, s.security) == k);
            for p in self.saved.iter_mut().filter(|p| p.key() == k) {
                p.timestamp = t as u64;
            }
            self.saved.sort_by_key(|p| std::cmp::Reverse(p.timestamp));
            self.active = Some(k);
            self.activity = Activity::Idle;
        }
    }

    pub fn state(&self, t: f64) -> State {
        let mut s = State {
            available: true,
            enabled: self.enabled,
            scanning: self.enabled && t < self.scan_until,
            saved: self.saved.clone(),
            activity: self.activity.clone(),
            prompt: self.prompt.clone(),
            secret: self.secret.clone(),
            ..Default::default()
        };
        if !self.enabled {
            return s;
        }
        s.networks = self.networks(t);
        if let Some((k, _)) = &self.joining {
            s.connecting_key = k.clone();
            s.connecting = s
                .networks
                .iter()
                .find(|n| n.key() == *k)
                .map(|n| n.ssid.clone())
                .unwrap_or_default();
        }
        if let Some(n) = s.networks.iter().find(|n| n.active) {
            s.ssid = n.ssid.clone();
            s.signal = n.signal;
            s.link = self.link(n, t);
        }
        if !self.selected.is_empty() {
            let network = s
                .networks
                .iter()
                .find(|n| n.key() == self.selected)
                .cloned();
            let profile = self.profile_of(&self.selected);
            let ssid = network
                .as_ref()
                .map(|n| n.ssid.clone())
                .or_else(|| profile.as_ref().map(|p| p.ssid.clone()));
            if let Some(ssid) = ssid {
                s.detail = Some(Detail {
                    key: self.selected.clone(),
                    ssid,
                    network,
                    profile,
                });
            }
        }
        s
    }

    fn security_of_key(&self, k: &str, t: f64) -> Option<(String, Security)> {
        self.networks(t)
            .into_iter()
            .find(|n| n.key() == k)
            .map(|n| (n.ssid, n.security))
    }

    fn join(&mut self, k: String, ssid: &str, t: f64) {
        self.prompt = None;
        self.activity = Activity::Connecting(ssid.to_string());
        self.joining = Some((k, t + JOINING));
    }

    fn ask(&mut self, k: String, ssid: String, security: Security, retry: bool, message: &str) {
        self.serial += 1;
        self.prompt_key = k;
        self.activity = Activity::Idle;
        self.prompt = Some(Prompt {
            serial: self.serial,
            ssid,
            kind: PromptKind::of(security),
            retry,
            new: true,
            message: message.to_string(),
            key_mgmt: key_mgmt_of(security).into(),
            ..Default::default()
        });
    }

    /// Why a password won't do, as NetworkManager's rules have it.
    fn wrong(security: Security, answer: &Answer) -> Option<&'static str> {
        match security {
            Security::Personal if !(8..=63).contains(&answer.password.chars().count()) => {
                Some("A WPA password has 8 to 63 characters.")
            }
            Security::Enterprise if answer.identity.trim().is_empty() => Some("Enter a user name."),
            Security::Enterprise | Security::Wep if answer.password.is_empty() => {
                Some("Enter the password.")
            }
            _ => None,
        }
    }

    pub fn handle(&mut self, cmd: Command, t: f64) {
        match cmd {
            Command::Toggle => {
                self.enabled = !self.enabled;
                self.joining = None;
                self.prompt = None;
                self.portal = false;
                self.activity = Activity::Idle;
                self.active = self.enabled.then(|| key("Lighthouse", Security::Personal));
            }
            Command::Rescan => self.scan_until = t + SCANNING,
            Command::Connect { key: k } => {
                let known = self.profile_of(&k);
                match (self.security_of_key(&k, t), known) {
                    (Some((ssid, _)), Some(_)) => self.join(k, &ssid, t),
                    (Some((ssid, security)), None) if !security.needs_secret() => {
                        self.saved.insert(0, profile(&ssid, security, t as u64));
                        self.join(k, &ssid, t);
                    }
                    (Some((ssid, security)), None) => self.ask(k, ssid, security, false, ""),
                    (None, Some(p)) => {
                        self.activity = Activity::Failed(format!("{} is out of range", p.ssid))
                    }
                    (None, None) => {}
                }
            }
            Command::JoinHidden {
                ssid,
                security,
                answer,
            } => {
                let security = match security {
                    0 => Security::Open,
                    2 => Security::Wep,
                    3 => Security::Enterprise,
                    _ => Security::Personal,
                };
                let k = key(&ssid, security);
                if let Some(message) = Self::wrong(security, &answer) {
                    self.ask(k, ssid, security, true, message);
                    return;
                }
                let mut saved = profile(&ssid, security, t as u64);
                saved.hidden = true;
                self.saved.insert(0, saved);
                self.hidden.push((ssid.clone(), security));
                self.join(k, &ssid, t);
            }
            Command::Answer { serial, answer } => {
                let current = self.prompt.as_ref().is_some_and(|p| p.serial == serial);
                if !current {
                    return;
                }
                let k = self.prompt_key.clone();
                let Some((ssid, security)) = self.security_of_key(&k, t) else {
                    self.prompt = None;
                    return;
                };
                if let Some(message) = Self::wrong(security, &answer) {
                    self.ask(k, ssid, security, true, message);
                    return;
                }
                self.saved.insert(0, profile(&ssid, security, t as u64));
                self.join(k, &ssid, t);
            }
            Command::CancelPrompt => {
                self.prompt = None;
                self.activity = Activity::Idle;
            }
            Command::Disconnect => {
                self.active = None;
                self.portal = false;
            }
            Command::Forget { uuid } => {
                let gone: Vec<String> = self
                    .saved
                    .iter()
                    .filter(|p| p.uuid == uuid)
                    .map(Profile::key)
                    .collect();
                self.saved.retain(|p| p.uuid != uuid);
                if self.active.as_ref().is_some_and(|k| gone.contains(k)) {
                    self.active = None;
                    self.portal = false;
                }
            }
            Command::SetAutoconnect { uuid, on } => {
                for p in self.saved.iter_mut().filter(|p| p.uuid == uuid) {
                    p.autoconnect = on;
                }
            }
            Command::Select { key: k } => {
                self.selected = k;
                self.secret = None;
            }
            Command::Reveal { uuid } => {
                let found = self.saved.iter().find(|p| p.uuid == uuid);
                if let Some(p) = found {
                    let password = "correct-horse-battery";
                    self.secret = Some(Secret {
                        uuid: uuid.clone(),
                        password: password.into(),
                        qr: qr_image(&format!("WIFI:T:WPA;S:{};P:{password};;", p.ssid)),
                    });
                }
            }
            Command::HideSecret => self.secret = None,
            Command::SignIn => self.portal = false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: f64 = 1_800_000_000.0;

    fn network(w: &Wifi, ssid: &str) -> Network {
        w.state(T)
            .networks
            .into_iter()
            .find(|n| n.ssid == ssid)
            .expect("in range")
    }

    #[test]
    fn starts_on_a_network_with_the_internet() {
        let w = Wifi::new(T);
        assert!(w.online());
        assert_eq!(w.state(T).ssid, "Lighthouse");
        assert_eq!(w.state(T).networks[0].ssid, "Lighthouse");
    }

    #[test]
    fn a_new_network_asks_for_its_password_and_checks_it() {
        let mut w = Wifi::new(T);
        let k = network(&w, "FRITZ!Box 7590 QX").key();
        w.handle(Command::Connect { key: k }, T);
        let serial = w.state(T).prompt.expect("a prompt").serial;
        let short = Answer {
            password: "short".into(),
            ..Default::default()
        };
        w.handle(
            Command::Answer {
                serial,
                answer: short,
            },
            T,
        );
        let again = w.state(T).prompt.expect("asked again");
        assert!(again.retry);
        // An answer to the earlier prompt goes nowhere.
        let long = Answer {
            password: "long enough".into(),
            ..Default::default()
        };
        w.handle(
            Command::Answer {
                serial,
                answer: long.clone(),
            },
            T,
        );
        assert!(w.state(T).prompt.is_some());
        w.handle(
            Command::Answer {
                serial: again.serial,
                answer: long,
            },
            T,
        );
        assert!(w.state(T).prompt.is_none());
        assert_eq!(w.state(T).connecting, "FRITZ!Box 7590 QX");
        w.advance(T + JOINING);
        assert_eq!(w.state(T + JOINING).ssid, "FRITZ!Box 7590 QX");
        assert!(network(&w, "FRITZ!Box 7590 QX").known());
    }

    #[test]
    fn a_captive_portal_is_in_the_way_until_signing_in() {
        let mut w = Wifi::new(T);
        let k = network(&w, "Harbour Café").key();
        w.handle(Command::Connect { key: k }, T);
        w.advance(T + JOINING);
        assert!(!w.online());
        assert_eq!(w.state(T).link.connectivity, Connectivity::Portal);
        w.handle(Command::SignIn, T);
        assert!(w.online());
    }

    #[test]
    fn switching_off_and_on_rejoins() {
        let mut w = Wifi::new(T);
        w.handle(Command::Toggle, T);
        assert!(!w.online());
        assert!(w.state(T).networks.is_empty());
        w.handle(Command::Toggle, T);
        assert!(w.online());
    }

    #[test]
    fn forgetting_the_current_network_leaves_it() {
        let mut w = Wifi::new(T);
        let uuid = network(&w, "Lighthouse").profile.expect("saved").uuid;
        w.handle(Command::Forget { uuid }, T);
        assert!(!w.online());
        assert!(!network(&w, "Lighthouse").known());
    }

    #[test]
    fn profiles_have_uuids_of_their_own() {
        let w = Wifi::new(T);
        let mut uuids: Vec<&str> = w.saved.iter().map(|p| p.uuid.as_str()).collect();
        uuids.sort_unstable();
        uuids.dedup();
        assert_eq!(uuids.len(), w.saved.len());
    }

    #[test]
    fn details_and_the_password_with_its_qr_code() {
        let mut w = Wifi::new(T);
        let n = network(&w, "Lighthouse");
        w.handle(Command::Select { key: n.key() }, T);
        let detail = w.state(T).detail.expect("details");
        assert_eq!(detail.ssid, "Lighthouse");
        w.handle(
            Command::Reveal {
                uuid: n.profile.expect("saved").uuid,
            },
            T,
        );
        assert!(w.state(T).secret.is_some());
        w.handle(Command::HideSecret, T);
        assert!(w.state(T).secret.is_none());
    }

    #[test]
    fn some_networks_come_and_go() {
        let w = Wifi::new(T);
        let counts: std::collections::HashSet<usize> = (0..2000)
            .map(|i| w.networks(T + i as f64 * 2.0).len())
            .collect();
        assert!(counts.len() > 1);
    }
}

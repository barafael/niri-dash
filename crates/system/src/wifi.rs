//! Wi-Fi, from NetworkManager.

use crate::IconSrc;
use std::sync::Arc;

/// What it takes to join a network, from easiest to hardest.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Security {
    #[default]
    Open,
    /// "Enhanced open": encrypted, but no password.
    Owe,
    /// WPA2/WPA3 with a password.
    Personal,
    Wep,
    /// WPA2/WPA3 Enterprise (802.1X): a user name and password.
    Enterprise,
}

impl Security {
    pub fn needs_secret(self) -> bool {
        matches!(self, Self::Personal | Self::Wep | Self::Enterprise)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Owe => "Enhanced open (OWE)",
            Self::Personal => "WPA2/WPA3",
            Self::Wep => "WEP",
            Self::Enterprise => "Enterprise (802.1X)",
        }
    }
}

/// The inner authentication of a new Enterprise connection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Eap {
    /// PEAP with MSCHAPv2, the usual one (eduroam, most companies).
    #[default]
    Peap,
    /// TTLS with PAP.
    Ttls,
}

impl Eap {
    pub fn from_index(i: i32) -> Self {
        if i == 1 { Self::Ttls } else { Self::Peap }
    }
}

/// What someone typed into the password prompt.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Answer {
    pub identity: String,
    pub password: String,
    pub eap: Eap,
    /// For a new Enterprise network: the domain its server's certificate must be issued for.
    pub domain: String,
    /// For a new Enterprise network: don't check the server's certificate at all.
    pub insecure: bool,
}

/// What the password prompt asks for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PromptKind {
    #[default]
    Password,
    Wep,
    /// A user name and password, and for a new network the method.
    Enterprise,
}

impl PromptKind {
    pub(crate) fn of(security: Security) -> Self {
        match security {
            Security::Wep => Self::Wep,
            Security::Enterprise => Self::Enterprise,
            _ => Self::Password,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Prompt {
    /// Which prompt this is: the UI clears its fields for a new one, and an answer typed for
    /// an earlier one is dropped rather than sent to another network.
    pub serial: u64,
    pub ssid: String,
    pub kind: PromptKind,
    /// The last password didn't work.
    pub retry: bool,
    /// A network without a profile yet: the prompt can choose the Enterprise method.
    pub new: bool,
    pub identity: String,
    /// Why the prompt is asking again (a password too short, say).
    pub message: String,
    /// For a new network: how to make its connection.
    pub(crate) key_mgmt: String,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) ap: String,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) hidden: bool,
    /// The saved profile to put the answer in when no agent request is waiting for it.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) profile: Option<Profile>,
}

/// A saved network's password, shown on request, with a QR code to join it from a phone.
#[derive(Debug, Clone, PartialEq)]
pub struct Secret {
    pub uuid: String,
    pub password: String,
    pub qr: IconSrc,
}

/// What the widget is doing beyond NetworkManager's own state.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Activity {
    #[default]
    Idle,
    Connecting(String),
    Failed(String),
}

impl std::fmt::Display for Activity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Activity::Idle => Ok(()),
            Activity::Connecting(ssid) => write!(f, "Connecting to {ssid}…"),
            Activity::Failed(why) => write!(f, "Could not connect: {why}"),
        }
    }
}

/// The network the details page shows: in range, saved, or both.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Detail {
    pub key: String,
    pub ssid: String,
    pub network: Option<Network>,
    pub profile: Option<Profile>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub available: bool,
    pub enabled: bool,
    /// Switched off by a hardware switch or rfkill.
    pub blocked: bool,
    pub ssid: String,
    pub signal: i32,
    /// The network NetworkManager is joining right now (also when it does so by itself): its
    /// SSID, and its key (see `nm::key`).
    pub connecting: String,
    pub connecting_key: String,
    pub networks: Vec<Network>,
    /// Saved networks, last used first.
    pub saved: Vec<Profile>,
    pub link: Link,
    pub activity: Activity,
    pub scanning: bool,
    pub prompt: Option<Prompt>,
    pub detail: Option<Detail>,
    pub secret: Option<Secret>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) device: String,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) check_uri: String,
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
impl State {
    /// The network in range with this key (see `nm::key`).
    pub(crate) fn network(&self, key: &str) -> Option<&Network> {
        self.networks.iter().find(|n| n.key() == key)
    }

    /// The saved profile for this key.
    pub(crate) fn profile(&self, key: &str) -> Option<&Profile> {
        self.network(key)
            .and_then(|n| n.profile.as_ref())
            .or_else(|| self.saved.iter().find(|p| p.key() == key))
    }
}

/// "3 Nov 2026" for a profile's timestamp.
pub fn day(timestamp: u64) -> String {
    chrono::DateTime::from_timestamp(timestamp as i64, 0)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%-d %b %Y")
                .to_string()
        })
        .unwrap_or_default()
}

/// The details page's lines (label, value) for a network.
pub fn facts(d: &Detail, link: &Link) -> Vec<(String, String)> {
    let mut f: Vec<(String, String)> = Vec::new();
    let mut add = |label: &str, value: String| {
        if !value.is_empty() {
            f.push((label.into(), value));
        }
    };
    match &d.network {
        Some(n) => {
            add("Signal", format!("{} %", n.signal));
            add("Security", n.security_label());
            add("Band", n.bands_label());
            if n.aps > 1 {
                add("Access points", n.aps.to_string());
            }
        }
        None => {
            add("Signal", "Not in range".into());
            if let Some(p) = &d.profile {
                add("Security", p.security().label().into());
            }
        }
    }
    if d.network.as_ref().is_some_and(|n| n.active) {
        add(
            "Internet",
            match link.connectivity {
                Connectivity::Full => "Connected",
                Connectivity::Portal => "Sign-in needed",
                Connectivity::Limited => "Limited",
                Connectivity::None => "None",
                Connectivity::Unknown => "",
            }
            .into(),
        );
        if link.freq > 0 {
            let band = match band_bit(link.freq) {
                1 => "2.4 GHz",
                2 => "5 GHz",
                _ => "6 GHz",
            };
            add("Channel", format!("{} ({band})", channel(link.freq)));
        }
        if link.bitrate_kbps > 0 {
            add("Speed", format!("{} Mbit/s", link.bitrate_kbps / 1000));
        }
        for a in &link.ip4 {
            add("IPv4", a.clone());
        }
        add("Gateway", link.gateway4.clone());
        // Link-local addresses say nothing anyone needs.
        for a in link.ip6.iter().filter(|a| !a.starts_with("fe80:")) {
            add("IPv6", a.clone());
        }
        add("DNS", link.dns.join(", "));
        add("BSSID", link.bssid.clone());
        add("MAC address", link.mac.clone());
    }
    if let Some(p) = &d.profile {
        if p.timestamp > 0 {
            add("Last used", day(p.timestamp));
        }
        if p.name != p.ssid {
            add("Profile", p.name.clone());
        }
        if p.hidden {
            add("Hidden", "yes".into());
        }
    }
    f
}

/// A QR code as an image, one pixel per module (the UI scales it up without smoothing), with a
/// quiet zone around it.
pub(crate) fn qr_image(text: &str) -> IconSrc {
    let Ok(code) = qrcode::QrCode::new(text.as_bytes()) else {
        return IconSrc::None;
    };
    const QUIET: usize = 2;
    let w = code.width();
    let size = w + 2 * QUIET;
    let colors = code.to_colors();
    let mut data = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let dark = (QUIET..QUIET + w).contains(&x)
                && (QUIET..QUIET + w).contains(&y)
                && colors[(y - QUIET) * w + (x - QUIET)] == qrcode::Color::Dark;
            let v = if dark { 0 } else { 255 };
            data.extend_from_slice(&[v, v, v, 255]);
        }
    }
    IconSrc::Rgba {
        width: size as u32,
        height: size as u32,
        data: Arc::new(data),
    }
}

/// A saved Wi-Fi connection profile.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Profile {
    pub path: String,
    pub uuid: String,
    pub name: String,
    pub ssid: String,
    pub autoconnect: bool,
    pub hidden: bool,
    /// key-mgmt of its security setting; empty for an open network.
    pub key_mgmt: String,
    /// When it was last connected (Unix seconds), 0 if never.
    pub timestamp: u64,
    /// A hotspot this machine makes rather than a network it joins.
    pub hotspot: bool,
}

/// The security a connection's key-mgmt stands for.
pub fn security_of(key_mgmt: &str) -> Security {
    match key_mgmt {
        "" => Security::Open,
        "owe" => Security::Owe,
        "none" | "ieee8021x" => Security::Wep,
        "wpa-eap" | "wpa-eap-suite-b-192" => Security::Enterprise,
        _ => Security::Personal,
    }
}

/// What identifies a network: its SSID and how it is joined. Access points that share an SSID
/// but differ in that are different networks, so an open access point named like a WPA network
/// (an evil twin) gets a row of its own rather than making the network look open.
pub fn key(ssid: &str, security: Security) -> String {
    let class = match security {
        Security::Open | Security::Owe => 'o',
        Security::Personal => 'p',
        Security::Wep => 'w',
        Security::Enterprise => 'e',
    };
    format!("{class}{ssid}")
}

impl Profile {
    pub fn security(&self) -> Security {
        security_of(&self.key_mgmt)
    }

    pub fn key(&self) -> String {
        key(&self.ssid, self.security())
    }
}

/// One network: the access points that share an SSID and how they are joined.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Network {
    pub ssid: String,
    pub signal: i32,
    /// The easiest way in among its access points: a network with an open access point among
    /// WPA ones is open.
    pub security: Security,
    /// For `Personal`: whether any access point takes WPA2 (PSK) and WPA3 (SAE).
    pub wpa2: bool,
    pub wpa3: bool,
    /// Bit 0: 2.4 GHz, bit 1: 5 GHz, bit 2: 6 GHz.
    pub bands: u8,
    pub aps: usize,
    /// The strongest access point with that security, to connect through.
    pub ap: String,
    pub active: bool,
    pub profile: Option<Profile>,
}

impl Network {
    pub fn key(&self) -> String {
        key(&self.ssid, self.security)
    }

    pub fn known(&self) -> bool {
        self.profile.is_some()
    }

    pub fn security_label(&self) -> String {
        match self.security {
            Security::Personal => match (self.wpa2, self.wpa3) {
                (true, true) => "WPA2/WPA3",
                (false, true) => "WPA3",
                _ => "WPA2",
            }
            .into(),
            s => s.label().into(),
        }
    }

    pub fn bands_label(&self) -> String {
        let names: Vec<&str> = [(1, "2.4"), (2, "5"), (4, "6")]
            .into_iter()
            .filter(|(bit, _)| self.bands & bit != 0)
            .map(|(_, name)| name)
            .collect();
        if names.is_empty() {
            String::new()
        } else {
            format!("{} GHz", names.join(" · "))
        }
    }

    /// key-mgmt for a new connection with a password. wpa-psk also does WPA3 (SAE) with
    /// NetworkManager, so it is used unless no access point offers WPA2.
    pub fn key_mgmt(&self) -> &'static str {
        match self.security {
            Security::Personal if !self.wpa2 => "sae",
            Security::Personal => "wpa-psk",
            Security::Enterprise => "wpa-eap",
            Security::Wep => "none",
            Security::Owe => "owe",
            Security::Open => "",
        }
    }
}

pub fn band_bit(freq: u32) -> u8 {
    match freq {
        0 => 0,
        f if f < 3000 => 1,
        f if f < 5925 => 2,
        _ => 4,
    }
}

pub fn channel(freq: u32) -> u32 {
    match freq {
        2484 => 14,
        f if f < 3000 => f.saturating_sub(2407) / 5,
        f if f < 5925 => f.saturating_sub(5000) / 5,
        f => f.saturating_sub(5950) / 5,
    }
}

/// NMConnectivityState.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Connectivity {
    #[default]
    Unknown,
    None,
    Portal,
    Limited,
    Full,
}

impl Connectivity {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub(crate) fn from_u32(v: u32) -> Self {
        match v {
            1 => Self::None,
            2 => Self::Portal,
            3 => Self::Limited,
            4 => Self::Full,
            _ => Self::Unknown,
        }
    }
}

/// The connection in use: addresses and radio details.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Link {
    pub ip4: Vec<String>,
    pub gateway4: String,
    pub dns: Vec<String>,
    pub ip6: Vec<String>,
    pub gateway6: String,
    pub bitrate_kbps: u32,
    pub freq: u32,
    pub bssid: String,
    pub mac: String,
    pub connectivity: Connectivity,
}

#[derive(Debug)]
pub enum Command {
    /// Turn the radio on or off.
    Toggle,
    Rescan,
    /// Connect to the network with this key (saved profile or visible network).
    Connect {
        key: String,
    },
    JoinHidden {
        ssid: String,
        security: i32,
        answer: Answer,
    },
    /// What was typed into the prompt with this serial.
    Answer {
        serial: u64,
        answer: Answer,
    },
    CancelPrompt,
    Disconnect,
    Forget {
        uuid: String,
    },
    SetAutoconnect {
        uuid: String,
        on: bool,
    },
    /// Show the details page of the network with this key ("" to leave it).
    Select {
        key: String,
    },
    /// Read a saved network's password, to show it and its QR code.
    Reveal {
        uuid: String,
    },
    HideSecret,
    /// Open the captive portal's login page.
    SignIn,
}

#[derive(Debug)]
pub enum Event {
    State(Box<State>),
}

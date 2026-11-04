//! NetworkManager over D-Bus: what it reports about Wi-Fi, turned into networks, and the
//! settings for new connections. The parsing is kept apart from the bus calls so it can be tested.

use super::{Answer, Eap, Error, Security};
pub use crate::wifi::{Connectivity, Link, Network, Profile, band_bit, key, security_of};
use std::collections::HashMap;
use zbus::Connection;
use zbus::names::OwnedInterfaceName;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

pub const NM: &str = "org.freedesktop.NetworkManager";
pub const NM_PATH: &str = "/org/freedesktop/NetworkManager";
pub const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
pub const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
pub const AP: &str = "org.freedesktop.NetworkManager.AccessPoint";
pub const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
pub const IP4: &str = "org.freedesktop.NetworkManager.IP4Config";
pub const IP6: &str = "org.freedesktop.NetworkManager.IP6Config";
pub const CONNECTION: &str = "org.freedesktop.NetworkManager.Settings.Connection";

/// A connection's settings as NetworkManager passes them around: setting name → key → value.
pub type Settings = HashMap<String, HashMap<String, OwnedValue>>;
type Props = HashMap<String, OwnedValue>;
type Objects = HashMap<OwnedObjectPath, HashMap<OwnedInterfaceName, Props>>;

// NM_802_11_AP_FLAGS_* and NM_802_11_AP_SEC_*.
const AP_PRIVACY: u32 = 0x1;
const SEC_PSK: u32 = 0x100;
const SEC_8021X: u32 = 0x200;
const SEC_SAE: u32 = 0x400;
const SEC_OWE: u32 = 0x800;
const SEC_OWE_TM: u32 = 0x1000;
const SEC_SUITE_B: u32 = 0x2000;

/// Device states (NMDeviceState) the activation tracking looks at.
pub const STATE_DISCONNECTED: u32 = 30;
pub const STATE_PREPARE: u32 = 40;
pub const STATE_ACTIVATED: u32 = 100;
pub const STATE_DEACTIVATING: u32 = 110;
pub const STATE_FAILED: u32 = 120;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ap {
    pub path: String,
    pub ssid: String,
    pub strength: u8,
    pub flags: u32,
    pub wpa: u32,
    pub rsn: u32,
    pub freq: u32,
    pub bssid: String,
}

impl Ap {
    pub fn security(&self) -> Security {
        let km = self.wpa | self.rsn;
        if km & (SEC_8021X | SEC_SUITE_B) != 0 {
            Security::Enterprise
        } else if km & (SEC_PSK | SEC_SAE) != 0 {
            Security::Personal
        } else if km & (SEC_OWE | SEC_OWE_TM) != 0 {
            Security::Owe
        } else if self.flags & AP_PRIVACY != 0 {
            Security::Wep
        } else {
            Security::Open
        }
    }
}

/// Whether an SSID names its network: hidden ones come empty or as NUL bytes.
fn named(ssid: &str) -> bool {
    ssid.chars().any(|c| !c.is_control())
}

/// Group access points into networks, sorted active first, then saved, then by signal.
pub fn networks(aps: &[Ap], profiles: &[Profile], active_key: &str) -> Vec<Network> {
    let mut nets: Vec<Network> = Vec::new();
    for ap in aps.iter().filter(|a| named(&a.ssid)) {
        let sec = ap.security();
        let km = ap.wpa | ap.rsn;
        let k = key(&ap.ssid, sec);
        let i = match nets.iter().position(|n| n.key() == k) {
            Some(i) => i,
            None => {
                nets.push(Network {
                    ssid: ap.ssid.clone(),
                    security: sec,
                    ap: ap.path.clone(),
                    signal: -1,
                    ..Default::default()
                });
                nets.len() - 1
            }
        };
        let n = &mut nets[i];
        // Open and OWE access points of one network: it is joined through an open one.
        n.aps += 1;
        n.bands |= band_bit(ap.freq);
        n.signal = n.signal.max(ap.strength.into());
        if sec < n.security {
            n.security = sec;
            n.ap = ap.path.clone();
            n.wpa2 = false;
            n.wpa3 = false;
        }
        if sec == n.security {
            n.wpa2 |= km & SEC_PSK != 0;
            n.wpa3 |= km & SEC_SAE != 0;
        }
    }
    // The access point to use is the strongest one of the network's security.
    for n in &mut nets {
        if let Some(best) = aps
            .iter()
            .filter(|a| a.ssid == n.ssid && a.security() == n.security)
            .max_by_key(|a| a.strength)
        {
            n.ap = best.path.clone();
        }
        let k = n.key();
        n.active = k == active_key;
        n.profile = profiles
            .iter()
            .filter(|p| !p.hotspot && p.key() == k)
            .max_by_key(|p| p.timestamp)
            .cloned();
    }
    nets.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then(b.known().cmp(&a.known()))
            .then(b.signal.cmp(&a.signal))
            .then(a.ssid.cmp(&b.ssid))
    });
    nets
}

fn get<'a, T>(props: &'a Props, key: &str) -> Option<T>
where
    T: TryFrom<&'a Value<'a>>,
    <T as TryFrom<&'a Value<'a>>>::Error: Into<zbus::zvariant::Error>,
{
    super::super::prop(props, key)
}

fn get_path(props: &Props, key: &str) -> Option<String> {
    get::<ObjectPath>(props, key)
        .map(|p| p.to_string())
        .filter(|p| p != "/")
}

fn get_bytes(props: &Props, key: &str) -> Vec<u8> {
    props
        .get(key)
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| Vec::<u8>::try_from(v).ok())
        .unwrap_or_default()
}

fn get_dicts(props: &Props, key: &str) -> Vec<Props> {
    props
        .get(key)
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| Vec::<Props>::try_from(v).ok())
        .unwrap_or_default()
}

pub fn ssid_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A profile from a connection's settings, if it is a Wi-Fi one.
pub fn profile(path: &str, s: &Settings) -> Option<Profile> {
    let conn = s.get("connection")?;
    if get::<&str>(conn, "type")? != "802-11-wireless" {
        return None;
    }
    let wifi = s.get("802-11-wireless")?;
    let mode = get::<&str>(wifi, "mode").unwrap_or("infrastructure");
    Some(Profile {
        path: path.into(),
        uuid: get::<&str>(conn, "uuid").unwrap_or_default().into(),
        name: get::<&str>(conn, "id").unwrap_or_default().into(),
        ssid: ssid_text(&get_bytes(wifi, "ssid")),
        autoconnect: get::<bool>(conn, "autoconnect").unwrap_or(true),
        hidden: get::<bool>(wifi, "hidden").unwrap_or(false),
        key_mgmt: s
            .get("802-11-wireless-security")
            .and_then(|w| get::<&str>(w, "key-mgmt"))
            .unwrap_or_default()
            .into(),
        timestamp: get::<u64>(conn, "timestamp").unwrap_or(0),
        hotspot: mode == "ap" || mode == "adhoc",
    })
}

/// Everything read in one GetManagedObjects call.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub enabled: bool,
    pub hw_enabled: bool,
    pub connectivity: Connectivity,
    pub check_uri: String,
    /// The Wi-Fi device's object path, interface name and state.
    pub device: Option<(String, String, u32)>,
    pub aps: Vec<Ap>,
    /// The profile path of the device's active connection and whether it is still activating.
    pub active: Option<(String, bool)>,
    pub active_ap: String,
    pub last_scan: i64,
    pub link: Link,
    /// Paths of all saved connections.
    pub connections: Vec<String>,
}

fn find<'a>(ifaces: &'a HashMap<OwnedInterfaceName, Props>, name: &str) -> Option<&'a Props> {
    ifaces
        .iter()
        .find(|(k, _)| k.as_str() == name)
        .map(|(_, v)| v)
}

fn object<'a>(objects: &'a Objects, path: &str, iface: &str) -> Option<&'a Props> {
    objects
        .iter()
        .find(|(p, _)| p.as_str() == path)
        .and_then(|(_, i)| find(i, iface))
}

pub async fn snapshot(conn: &Connection) -> Result<Snapshot, Error> {
    let reply = conn
        .call_method(
            Some(NM),
            "/org/freedesktop",
            Some("org.freedesktop.DBus.ObjectManager"),
            "GetManagedObjects",
            &(),
        )
        .await?;
    let objects: Objects = reply.body().deserialize()?;
    let mut snap = Snapshot::default();
    if let Some(m) = object(&objects, NM_PATH, NM) {
        snap.enabled = get(m, "WirelessEnabled").unwrap_or(false);
        snap.hw_enabled = get(m, "WirelessHardwareEnabled").unwrap_or(true);
        snap.connectivity = Connectivity::from_u32(get(m, "Connectivity").unwrap_or(0));
        snap.check_uri = get::<&str>(m, "ConnectivityCheckUri")
            .unwrap_or_default()
            .into();
    }
    for (path, ifaces) in &objects {
        if find(ifaces, CONNECTION).is_some() {
            snap.connections.push(path.to_string());
        }
    }
    snap.connections.sort();
    let wifi = objects.iter().find_map(|(path, ifaces)| {
        Some((
            path.as_str(),
            find(ifaces, DEVICE)?,
            find(ifaces, WIRELESS)?,
        ))
    });
    let Some((dev_path, dev, wireless)) = wifi else {
        return Ok(snap);
    };
    let state: u32 = get(dev, "State").unwrap_or(0);
    snap.device = Some((
        dev_path.to_string(),
        get::<&str>(dev, "Interface").unwrap_or_default().into(),
        state,
    ));
    snap.last_scan = get(wireless, "LastScan").unwrap_or(0);
    snap.active_ap = get_path(wireless, "ActiveAccessPoint").unwrap_or_default();
    for (path, ifaces) in &objects {
        if let Some(a) = find(ifaces, AP) {
            snap.aps.push(Ap {
                path: path.to_string(),
                ssid: ssid_text(&get_bytes(a, "Ssid")),
                strength: get(a, "Strength").unwrap_or(0),
                flags: get(a, "Flags").unwrap_or(0),
                wpa: get(a, "WpaFlags").unwrap_or(0),
                rsn: get(a, "RsnFlags").unwrap_or(0),
                freq: get(a, "Frequency").unwrap_or(0),
                bssid: get::<&str>(a, "HwAddress").unwrap_or_default().into(),
            });
        }
    }
    // Only the device's own access points: a second Wi-Fi device would list its own.
    let own: Vec<String> = wireless
        .get("AccessPoints")
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| Vec::<OwnedObjectPath>::try_from(v).ok())
        .map(|v| v.iter().map(|p| p.to_string()).collect())
        .unwrap_or_default();
    if !own.is_empty() {
        snap.aps.retain(|a| own.contains(&a.path));
    }
    if let Some(ac) = get_path(dev, "ActiveConnection")
        .and_then(|p| object(&objects, &p, ACTIVE))
        .filter(|_| (STATE_PREPARE..=STATE_ACTIVATED).contains(&state))
        && let Some(profile) = get_path(ac, "Connection")
    {
        snap.active = Some((profile, state < STATE_ACTIVATED));
    }
    if state == STATE_ACTIVATED {
        let link = &mut snap.link;
        link.bitrate_kbps = get(wireless, "Bitrate").unwrap_or(0);
        link.mac = get::<&str>(wireless, "HwAddress")
            .unwrap_or_default()
            .into();
        if let Some(ap) = snap.aps.iter().find(|a| a.path == snap.active_ap) {
            link.freq = ap.freq;
            link.bssid = ap.bssid.clone();
        }
        let c4: u32 = get(dev, "Ip4Connectivity").unwrap_or(0);
        let c6: u32 = get(dev, "Ip6Connectivity").unwrap_or(0);
        link.connectivity = Connectivity::from_u32(c4.max(c6));
        if let Some(ip4) = get_path(dev, "Ip4Config").and_then(|p| object(&objects, &p, IP4)) {
            link.ip4 = addresses(ip4);
            link.gateway4 = get::<&str>(ip4, "Gateway").unwrap_or_default().into();
            link.dns = get_dicts(ip4, "NameserverData")
                .iter()
                .filter_map(|d| get::<&str>(d, "address").map(str::to_string))
                .collect();
        }
        if let Some(ip6) = get_path(dev, "Ip6Config").and_then(|p| object(&objects, &p, IP6)) {
            link.ip6 = addresses(ip6);
            link.gateway6 = get::<&str>(ip6, "Gateway").unwrap_or_default().into();
            // IPv6 name servers come as 16-byte arrays.
            let dns6 = ip6
                .get("Nameservers")
                .and_then(|v| v.try_clone().ok())
                .and_then(|v| Vec::<Vec<u8>>::try_from(v).ok())
                .unwrap_or_default();
            link.dns.extend(dns6.iter().filter_map(|b| {
                let b: [u8; 16] = b.as_slice().try_into().ok()?;
                Some(std::net::Ipv6Addr::from(b).to_string())
            }));
        }
    }
    Ok(snap)
}

/// "address/prefix" for each entry of an IP config's AddressData.
fn addresses(config: &Props) -> Vec<String> {
    get_dicts(config, "AddressData")
        .iter()
        .filter_map(|d| {
            let addr = get::<&str>(d, "address")?;
            let prefix: u32 = get(d, "prefix").unwrap_or(0);
            Some(format!("{addr}/{prefix}"))
        })
        .collect()
}

pub async fn settings(conn: &Connection, path: &str) -> Result<Settings, Error> {
    let reply = conn
        .call_method(Some(NM), path, Some(CONNECTION), "GetSettings", &())
        .await?;
    Ok(reply.body().deserialize()?)
}

/// The secrets of one setting of a saved connection (NetworkManager may ask an agent for them).
pub async fn secrets(conn: &Connection, path: &str, setting: &str) -> Result<Settings, Error> {
    let reply = conn
        .call_method(Some(NM), path, Some(CONNECTION), "GetSecrets", &(setting,))
        .await?;
    Ok(reply.body().deserialize()?)
}

pub async fn call(
    conn: &Connection,
    path: &str,
    iface: &str,
    method: &str,
    body: &(impl serde::Serialize + zbus::zvariant::DynamicType),
) -> Result<zbus::Message, Error> {
    Ok(conn
        .call_method(Some(NM), path, Some(iface), method, body)
        .await?)
}

pub fn path(p: &str) -> Result<ObjectPath<'_>, Error> {
    Ok(ObjectPath::try_from(p)?)
}

/// Settings for a new connection to `ssid` (`a{sa{sv}}`), with the secret in `answer` if the
/// security takes one. NetworkManager fills in the rest from the access point.
pub fn new_connection(
    ssid: &str,
    key_mgmt: &str,
    hidden: bool,
    answer: Option<&Answer>,
) -> HashMap<&'static str, HashMap<&'static str, Value<'static>>> {
    let mut s: HashMap<&'static str, HashMap<&'static str, Value<'static>>> = HashMap::new();
    s.insert(
        "connection",
        HashMap::from([
            ("type", Value::from("802-11-wireless")),
            ("id", Value::from(ssid.to_string())),
        ]),
    );
    let mut wifi = HashMap::from([
        ("ssid", Value::from(ssid.as_bytes().to_vec())),
        ("mode", Value::from("infrastructure")),
    ]);
    if hidden {
        wifi.insert("hidden", Value::from(true));
    }
    s.insert("802-11-wireless", wifi);
    if key_mgmt.is_empty() {
        return s;
    }
    let mut sec = HashMap::from([("key-mgmt", Value::from(key_mgmt.to_string()))]);
    let password = answer.map(|a| a.password.clone()).unwrap_or_default();
    match key_mgmt {
        "wpa-psk" | "sae" => {
            sec.insert("psk", Value::from(password));
            sec.insert("psk-flags", Value::from(0u32));
        }
        "none" => {
            // 5 or 13 characters, or 10 or 26 hex digits, are a key; anything else a passphrase.
            let hex = password.chars().all(|c| c.is_ascii_hexdigit());
            let key =
                matches!(password.len(), 5 | 13) || (hex && matches!(password.len(), 10 | 26));
            sec.insert("wep-key-type", Value::from(if key { 1u32 } else { 2u32 }));
            sec.insert("wep-key0", Value::from(password));
            sec.insert("wep-key-flags", Value::from(0u32));
        }
        "wpa-eap" => {
            let (eap, phase2) = match answer.map(|a| a.eap).unwrap_or_default() {
                Eap::Peap => ("peap", "mschapv2"),
                Eap::Ttls => ("ttls", "pap"),
            };
            let identity = answer.map(|a| a.identity.clone()).unwrap_or_default();
            let mut eap_settings = HashMap::from([
                ("eap", Value::from(vec![eap.to_string()])),
                ("phase2-auth", Value::from(phase2)),
                ("identity", Value::from(identity)),
                ("password", Value::from(password)),
                ("password-flags", Value::from(0u32)),
            ]);
            // The server proves who it is with a certificate from a known authority, issued
            // for the network's domain; without that check an evil twin gets the password.
            if let Some(a) = answer.filter(|a| !a.insecure) {
                eap_settings.insert("system-ca-certs", Value::from(true));
                eap_settings.insert(
                    "domain-suffix-match",
                    Value::from(a.domain.trim().to_string()),
                );
            }
            s.insert("802-1x", eap_settings);
        }
        _ => {}
    }
    s.insert("802-11-wireless-security", sec);
    s
}

/// What a failed activation's NMDeviceStateReason means to someone joining a network.
pub fn failure(reason: u32) -> String {
    match reason {
        7 => "no password was given".into(),
        8 => "the network refused the password".into(),
        9 | 10 => "authentication failed".into(),
        11 => "the network did not answer in time".into(),
        5 | 15..=17 => "got no IP address".into(),
        53 => "the network is out of range".into(),
        64 => "the IP address is already in use".into(),
        r => format!("reason {r}"),
    }
}

/// `WIFI:` text for a QR code that phones join a network from.
pub fn qr_text(ssid: &str, security: Security, password: &str, hidden: bool) -> String {
    fn esc(s: &str) -> String {
        s.chars()
            .flat_map(|c| {
                let escape = matches!(c, '\\' | ';' | ',' | ':' | '"');
                escape.then_some('\\').into_iter().chain([c])
            })
            .collect()
    }
    let t = match security {
        Security::Wep => "WEP",
        Security::Open | Security::Owe => "nopass",
        _ => "WPA",
    };
    let mut s = format!("WIFI:T:{t};S:{};", esc(ssid));
    if t != "nopass" {
        s += &format!("P:{};", esc(password));
    }
    if hidden {
        s += "H:true;";
    }
    s + ";"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wifi::channel;

    fn ap(ssid: &str, strength: u8, rsn: u32, freq: u32) -> Ap {
        Ap {
            path: format!("/ap/{ssid}/{strength}"),
            ssid: ssid.into(),
            strength,
            flags: if rsn != 0 { AP_PRIVACY } else { 0 },
            rsn,
            freq,
            ..Default::default()
        }
    }

    #[test]
    fn grouping() {
        let aps = [
            ap("Pixel", 86, SEC_PSK | SEC_SAE, 2447),
            ap("pycon", 84, SEC_SAE, 6135),
            ap("Centro", 84, SEC_OWE, 6135),
            ap("pycon", 67, SEC_PSK | SEC_SAE, 5220),
            ap("Centro", 65, 0, 2437),
            ap("Centro", 60, SEC_OWE, 6295),
            ap("eduroam", 50, SEC_8021X, 5500),
            ap("", 30, SEC_PSK, 2412),
            ap("\0\0\0\0", 30, SEC_PSK, 2412),
        ];
        let profiles = [Profile {
            ssid: "Pixel".into(),
            uuid: "u1".into(),
            key_mgmt: "sae".into(),
            ..Default::default()
        }];
        let nets = networks(&aps, &profiles, "pPixel");
        let summary: Vec<_> = nets
            .iter()
            .map(|n| {
                (
                    n.ssid.as_str(),
                    n.signal,
                    n.security,
                    n.active,
                    n.known(),
                    n.aps,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("Pixel", 86, Security::Personal, true, true, 1),
                ("Centro", 84, Security::Open, false, false, 3),
                ("pycon", 84, Security::Personal, false, false, 2),
                ("eduroam", 50, Security::Enterprise, false, false, 1),
            ]
        );
        let centro = &nets[1];
        // Joined through the open access point, the only one of that security.
        assert_eq!(centro.ap, "/ap/Centro/65");
        assert_eq!(centro.bands_label(), "2.4 · 6 GHz");
        let pycon = &nets[2];
        assert_eq!((pycon.wpa2, pycon.wpa3), (true, true));
        assert_eq!(pycon.security_label(), "WPA2/WPA3");
        assert_eq!(pycon.key_mgmt(), "wpa-psk");
        assert_eq!(pycon.ap, "/ap/pycon/84");
    }

    #[test]
    fn evil_twin() {
        // An open access point named like a saved WPA network is a network of its own, not a
        // reason to join "Corp" without a password; neither does it take Corp's profile.
        let aps = [ap("Corp", 60, SEC_PSK, 2412), ap("Corp", 90, 0, 2437)];
        let profiles = [Profile {
            ssid: "Corp".into(),
            key_mgmt: "wpa-psk".into(),
            ..Default::default()
        }];
        let nets = networks(&aps, &profiles, "");
        let summary: Vec<_> = nets
            .iter()
            .map(|n| (n.key(), n.security, n.known(), n.ap.as_str()))
            .collect();
        assert_eq!(
            summary,
            [
                ("pCorp".to_string(), Security::Personal, true, "/ap/Corp/60"),
                ("oCorp".to_string(), Security::Open, false, "/ap/Corp/90"),
            ]
        );
    }

    #[test]
    fn eap_checks_the_server() {
        let answer = Answer {
            identity: "me".into(),
            password: "pw".into(),
            domain: "example.org".into(),
            ..Default::default()
        };
        let s = new_connection("eduroam", "wpa-eap", false, Some(&answer));
        let eap = &s["802-1x"];
        assert_eq!(eap["system-ca-certs"], Value::from(true));
        assert_eq!(eap["domain-suffix-match"], Value::from("example.org"));
        let unsafe_answer = Answer {
            insecure: true,
            ..answer
        };
        let s = new_connection("eduroam", "wpa-eap", false, Some(&unsafe_answer));
        assert!(!s["802-1x"].contains_key("system-ca-certs"));
    }

    #[test]
    fn wpa3_only() {
        let nets = networks(&[ap("six", 70, SEC_SAE, 6135)], &[], "");
        assert_eq!(nets[0].key_mgmt(), "sae");
        assert_eq!(nets[0].security_label(), "WPA3");
    }

    #[test]
    fn channels() {
        assert_eq!(channel(2447), 8);
        assert_eq!(channel(2484), 14);
        assert_eq!(channel(5220), 44);
        assert_eq!(channel(6135), 37);
    }

    #[test]
    fn qr() {
        assert_eq!(
            qr_text("My;Net", Security::Personal, "pa:ss", false),
            r"WIFI:T:WPA;S:My\;Net;P:pa\:ss;;"
        );
        assert_eq!(
            qr_text("Cafe", Security::Open, "", true),
            "WIFI:T:nopass;S:Cafe;H:true;;"
        );
    }

    #[test]
    fn profile_from_settings() {
        let v = |v: Value<'static>| OwnedValue::try_from(v).unwrap();
        let s: Settings = HashMap::from([
            (
                "connection".to_string(),
                HashMap::from([
                    ("type".to_string(), v(Value::from("802-11-wireless"))),
                    ("id".to_string(), v(Value::from("Home"))),
                    ("uuid".to_string(), v(Value::from("u-1"))),
                    ("autoconnect".to_string(), v(Value::from(false))),
                    ("timestamp".to_string(), v(Value::from(1700000000u64))),
                ]),
            ),
            (
                "802-11-wireless".to_string(),
                HashMap::from([("ssid".to_string(), v(Value::from(b"Home 5G".to_vec())))]),
            ),
            (
                "802-11-wireless-security".to_string(),
                HashMap::from([("key-mgmt".to_string(), v(Value::from("sae")))]),
            ),
        ]);
        let p = profile("/c/1", &s).unwrap();
        assert_eq!(
            (p.name.as_str(), p.ssid.as_str(), p.uuid.as_str()),
            ("Home", "Home 5G", "u-1")
        );
        assert!(!p.autoconnect && !p.hidden && !p.hotspot);
        assert_eq!(p.security(), Security::Personal);
        assert_eq!(p.timestamp, 1700000000);
    }
}

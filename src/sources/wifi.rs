//! Wi-Fi through NetworkManager's `nmcli`.

use super::{CmdError, Ctx, Dedup, Msg, Trigger, run_with, signals_from};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Network {
    pub ssid: String,
    pub signal: i32,
    pub secure: bool,
    pub active: bool,
    /// UUID of the saved profile for this SSID, if there is one.
    pub profile: Option<String>,
}

impl Network {
    pub fn known(&self) -> bool {
        self.profile.is_some()
    }
}

/// What the Wi-Fi widget is doing right now, beyond the network state.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum Activity {
    #[default]
    Idle,
    Scanning,
    Connecting(String),
    NeedsPassword(String),
    Failed(String),
}

impl Activity {
    pub fn busy(&self) -> bool {
        matches!(self, Activity::Scanning | Activity::Connecting(_))
    }
}

impl std::fmt::Display for Activity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Activity::Idle => Ok(()),
            Activity::Scanning => write!(f, "Scanning…"),
            Activity::Connecting(ssid) => write!(f, "Connecting to {ssid}…"),
            Activity::NeedsPassword(ssid) => write!(f, "{ssid} needs a password"),
            Activity::Failed(why) => write!(f, "Could not connect: {why}"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub available: bool,
    pub enabled: bool,
    pub device: String,
    pub ssid: String,
    pub signal: i32,
    pub networks: Vec<Network>,
    pub activity: Activity,
}

/// Split one line of `nmcli -t` output: fields are separated by ':' and literal ':' is '\:'.
pub fn split_terse(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    fields.last_mut().expect("non-empty").push(n);
                }
            }
            ':' => fields.push(String::new()),
            c => fields.last_mut().expect("non-empty").push(c),
        }
    }
    fields
}

/// Run `nmcli` with untranslated output: its messages are matched below, and a translated
/// "Secrets were required" would not be.
async fn nmcli(args: &[&str]) -> Result<String, CmdError> {
    run_with("nmcli", args, &[("LC_ALL", "C")], None).await
}

/// Whether joining a network with this `nmcli` SECURITY value takes a password. OWE ("enhanced
/// open") is encrypted but asks for none, like an open network ("" or "--").
fn needs_password(security: &str) -> bool {
    security
        .split_whitespace()
        .any(|t| t != "--" && !t.starts_with("OWE"))
}

/// The saved Wi-Fi profiles as (UUID, SSID), from `nmcli -t -f
/// connection.uuid,802-11-wireless.ssid connection show <uuids>`: one `field:value` line each.
fn parse_profiles(out: &str) -> Vec<(String, String)> {
    let mut profiles = Vec::new();
    let mut uuid = None;
    for line in out.lines() {
        let f = split_terse(line);
        // An SSID with a ':' may or may not come escaped; joining the rest covers both.
        let value = f[1..].join(":");
        match f[0].as_str() {
            "connection.uuid" => uuid = Some(value),
            "802-11-wireless.ssid" => {
                if let Some(u) = uuid.take() {
                    profiles.push((u, value));
                }
            }
            _ => {}
        }
    }
    profiles
}

/// One entry per SSID from `nmcli -t -f IN-USE,SSID,SIGNAL,SECURITY device wifi list`, which
/// has a line per access point. A network takes a password only if every access point of it
/// does, so a name with an open or OWE access point among WPA ones is joined without asking.
fn parse_networks(list: &str, profiles: &[(String, String)]) -> Vec<Network> {
    let mut nets: Vec<Network> = Vec::new();
    for f in list.lines().map(split_terse) {
        if f.len() < 4 || f[1].is_empty() {
            continue;
        }
        let n = Network {
            active: f[0] == "*",
            ssid: f[1].clone(),
            signal: f[2].parse().unwrap_or(0),
            secure: needs_password(&f[3]),
            profile: profiles
                .iter()
                .find(|(_, ssid)| ssid == &f[1])
                .map(|(uuid, _)| uuid.clone()),
        };
        match nets.iter_mut().find(|e| e.ssid == n.ssid) {
            Some(e) => {
                e.active |= n.active;
                e.secure &= n.secure;
                e.signal = e.signal.max(n.signal);
            }
            None => nets.push(n),
        }
    }
    nets.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then(b.known().cmp(&a.known()))
            .then(b.signal.cmp(&a.signal))
    });
    nets
}

async fn fetch(activity: &Mutex<Activity>) -> Result<State, CmdError> {
    let devices = nmcli(&["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "device"]).await?;
    let dev = devices
        .lines()
        .map(split_terse)
        .find(|f| f.get(1).is_some_and(|t| t == "wifi"));
    let Some(dev) = dev else {
        return Ok(State::default());
    };
    let device = dev[0].clone();
    let radio = nmcli(&["radio", "wifi"]).await?;
    let enabled = radio.trim() == "enabled";
    let mut state = State {
        available: true,
        enabled,
        device,
        activity: activity.lock().map(|s| s.clone()).unwrap_or_default(),
        ..Default::default()
    };
    if !enabled {
        return Ok(state);
    }
    // Profiles are matched to networks by SSID: a profile's name is often the SSID, but needn't be.
    let uuids: Vec<String> = nmcli(&["-t", "-f", "UUID,TYPE", "connection", "show"])
        .await?
        .lines()
        .map(split_terse)
        .filter(|f| f.get(1).is_some_and(|t| t == "802-11-wireless"))
        .map(|f| f[0].clone())
        .collect();
    let profiles = if uuids.is_empty() {
        Vec::new()
    } else {
        let mut args = vec![
            "-t",
            "-f",
            "connection.uuid,802-11-wireless.ssid",
            "connection",
            "show",
        ];
        args.extend(uuids.iter().map(String::as_str));
        parse_profiles(&nmcli(&args).await?)
    };
    let list = nmcli(&[
        "-t",
        "-f",
        "IN-USE,SSID,SIGNAL,SECURITY",
        "device",
        "wifi",
        "list",
        "--rescan",
        "no",
    ])
    .await?;
    state.networks = parse_networks(&list, &profiles);
    if let Some(a) = state.networks.iter().find(|n| n.active) {
        state.ssid = a.ssid.clone();
        state.signal = a.signal;
    }
    Ok(state)
}

/// Store `password` in a saved profile, so that NetworkManager has it on the next activation
/// rather than asking a secret agent (there may be none). A profile saved as open gets WPA-PSK.
async fn set_password(uuid: &str, password: &str) -> Result<String, CmdError> {
    let key_mgmt = nmcli(&[
        "-g",
        "802-11-wireless-security.key-mgmt",
        "connection",
        "show",
        "uuid",
        uuid,
    ])
    .await?;
    let mut args = vec!["connection", "modify", "uuid", uuid];
    if key_mgmt.trim().is_empty() {
        args.extend(["802-11-wireless-security.key-mgmt", "wpa-psk"]);
    }
    args.extend([
        "802-11-wireless-security.psk",
        password,
        "802-11-wireless-security.psk-flags",
        "0",
    ]);
    nmcli(&args).await
}

#[derive(Clone)]
pub struct Wifi {
    ctx: Ctx,
    last: Arc<Mutex<State>>,
    activity: Arc<Mutex<Activity>>,
}

impl Wifi {
    pub fn start(ctx: Ctx) -> Self {
        let me = Self {
            ctx: ctx.clone(),
            last: Default::default(),
            activity: Default::default(),
        };
        let m = me.clone();
        let trigger = Trigger::default();
        ctx.on_dbus_signals(
            true,
            signals_from("org.freedesktop.NetworkManager"),
            Duration::from_secs(1),
            trigger.clone(),
        );
        let sent = Arc::new(Dedup::default());
        ctx.poll_events(trigger, Duration::from_secs(60), move || {
            let (m, sent) = (m.clone(), sent.clone());
            async move {
                match fetch(&m.activity).await {
                    Ok(s) => {
                        if let Ok(mut l) = m.last.lock() {
                            *l = s.clone();
                        }
                        if sent.changed(&s) {
                            m.ctx.send(Msg::Wifi(s));
                        }
                    }
                    Err(e) => log::debug!("wifi: {e}"),
                }
            }
        });
        me
    }

    fn set_activity(&self, a: Activity) {
        if let Ok(mut st) = self.activity.lock() {
            *st = a;
        }
        self.ctx.refresh();
    }

    pub fn toggle(&self) {
        let on = self.last.lock().map(|l| !l.enabled).unwrap_or(true);
        self.ctx.act("Wi-Fi", async move {
            nmcli(&["radio", "wifi", if on { "on" } else { "off" }])
                .await
                .map(|_| ())
        });
    }

    pub fn rescan(&self) {
        let me = self.clone();
        self.set_activity(Activity::Scanning);
        self.ctx.spawn(async move {
            let _ = nmcli(&["device", "wifi", "rescan"]).await;
            tokio::time::sleep(Duration::from_secs(3)).await;
            me.set_activity(Activity::Idle);
        });
    }

    pub fn connect(&self, ssid: String, password: String) {
        let me = self.clone();
        let profile = self.last.lock().ok().and_then(|l| {
            l.networks
                .iter()
                .find(|n| n.ssid == ssid)
                .and_then(|n| n.profile.clone())
        });
        self.set_activity(Activity::Connecting(ssid.clone()));
        self.ctx.spawn(async move {
            let result = match profile {
                Some(uuid) => {
                    let stored = if password.is_empty() {
                        Ok(String::new())
                    } else {
                        set_password(&uuid, &password).await
                    };
                    match stored {
                        Ok(_) => nmcli(&["connection", "up", "uuid", &uuid]).await,
                        Err(e) => Err(e),
                    }
                }
                None if password.is_empty() => nmcli(&["device", "wifi", "connect", &ssid]).await,
                None => nmcli(&["device", "wifi", "connect", &ssid, "password", &password]).await,
            };
            match result {
                Ok(_) => me.set_activity(Activity::Idle),
                Err(e) => {
                    let msg = e.to_string();
                    me.set_activity(
                        if msg.contains("Secrets were required")
                            || msg.contains("802-11-wireless-security")
                        {
                            Activity::NeedsPassword(ssid)
                        } else {
                            Activity::Failed(msg)
                        },
                    );
                    tokio::time::sleep(Duration::from_secs(8)).await;
                    me.set_activity(Activity::Idle);
                }
            }
        });
    }

    pub fn disconnect(&self) {
        let dev = self
            .last
            .lock()
            .map(|l| l.device.clone())
            .unwrap_or_default();
        // Without a Wi-Fi device there is no connection to drop.
        if dev.is_empty() {
            return;
        }
        self.ctx.act("Disconnect", async move {
            nmcli(&["device", "disconnect", &dev]).await.map(|_| ())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{needs_password, parse_networks, parse_profiles, split_terse};

    /// Part of a real scan at a conference: one SSID per several access points.
    const LIST: &str = "*:Pixel_2058:86:WPA2 WPA3
 :pyconIT-SPONSOR:84:WPA3
 :Centro_Congressi:84:OWE
 :Taimo's S23:74:WPA2
 :pyconIT-SPONSOR:67:WPA2 WPA3
 :Centro_Congressi:65:
 :Centro_Congressi:60:OWE
 :Can I has wireless? :40:WPA2
 ::30:WPA2
";

    #[test]
    fn security() {
        assert!(needs_password("WPA2 WPA3"));
        assert!(needs_password("WPA2 802.1X"));
        assert!(!needs_password(""));
        assert!(!needs_password("--"));
        assert!(!needs_password("OWE"));
    }

    #[test]
    fn profiles() {
        let out = "connection.uuid:4ebb\n802-11-wireless.ssid:Pixel_2058\n\n\
                   connection.uuid:5301\n802-11-wireless.ssid:My:Net\n\n\
                   connection.uuid:43b2\n802-11-wireless.ssid:Esc\\:aped\n";
        assert_eq!(
            parse_profiles(out),
            [
                ("4ebb", "Pixel_2058"),
                ("5301", "My:Net"),
                ("43b2", "Esc:aped")
            ]
            .map(|(u, s)| (u.to_string(), s.to_string()))
        );
    }

    #[test]
    fn networks() {
        let profiles = [("uuid-hotspot".to_string(), "Pixel_2058".to_string())];
        let nets = parse_networks(LIST, &profiles);
        let summary: Vec<_> = nets
            .iter()
            .map(|n| (n.ssid.as_str(), n.signal, n.secure, n.active, n.known()))
            .collect();
        assert_eq!(
            summary,
            [
                ("Pixel_2058", 86, true, true, true),
                ("pyconIT-SPONSOR", 84, true, false, false),
                // OWE and open access points: no password.
                ("Centro_Congressi", 84, false, false, false),
                ("Taimo's S23", 74, true, false, false),
                ("Can I has wireless? ", 40, true, false, false),
            ]
        );
        assert_eq!(nets[0].profile.as_deref(), Some("uuid-hotspot"));
    }

    #[test]
    fn terse_escapes() {
        assert_eq!(
            split_terse(r"*:My\:Net:57:WPA2"),
            vec!["*", "My:Net", "57", "WPA2"]
        );
        assert_eq!(split_terse(" :SAVOIA:43:"), vec![" ", "SAVOIA", "43", ""]);
    }
}

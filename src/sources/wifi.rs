//! Wi-Fi through NetworkManager's `nmcli`.

use super::{CmdError, Ctx, Dedup, Msg, Trigger, run, signals_from};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Network {
    pub ssid: String,
    pub signal: i32,
    pub secure: bool,
    pub active: bool,
    pub known: bool,
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

async fn fetch(activity: &Mutex<Activity>) -> Result<State, CmdError> {
    let devices = run(
        "nmcli",
        &["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "device"],
    )
    .await?;
    let dev = devices
        .lines()
        .map(split_terse)
        .find(|f| f.get(1).is_some_and(|t| t == "wifi"));
    let Some(dev) = dev else {
        return Ok(State::default());
    };
    let device = dev[0].clone();
    let radio = run("nmcli", &["radio", "wifi"]).await?;
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
    let known: Vec<String> = run("nmcli", &["-t", "-f", "NAME,TYPE", "connection", "show"])
        .await?
        .lines()
        .map(split_terse)
        .filter(|f| f.get(1).is_some_and(|t| t.contains("wireless")))
        .map(|f| f[0].clone())
        .collect();
    let list = run(
        "nmcli",
        &[
            "-t",
            "-f",
            "IN-USE,SSID,SIGNAL,SECURITY",
            "device",
            "wifi",
            "list",
            "--rescan",
            "no",
        ],
    )
    .await?;
    let mut nets: Vec<Network> = Vec::new();
    for f in list.lines().map(split_terse) {
        if f.len() < 4 || f[1].is_empty() {
            continue;
        }
        let n = Network {
            active: f[0] == "*",
            ssid: f[1].clone(),
            signal: f[2].parse().unwrap_or(0),
            secure: !f[3].is_empty() && f[3] != "--",
            known: known.iter().any(|k| k == &f[1]),
        };
        match nets.iter_mut().find(|e| e.ssid == n.ssid) {
            Some(e) => {
                e.active |= n.active;
                if n.signal > e.signal {
                    e.signal = n.signal;
                }
            }
            None => nets.push(n),
        }
    }
    nets.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then(b.known.cmp(&a.known))
            .then(b.signal.cmp(&a.signal))
    });
    if let Some(a) = nets.iter().find(|n| n.active) {
        state.ssid = a.ssid.clone();
        state.signal = a.signal;
    }
    state.networks = nets;
    Ok(state)
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
            run("nmcli", &["radio", "wifi", if on { "on" } else { "off" }])
                .await
                .map(|_| ())
        });
    }

    pub fn rescan(&self) {
        let me = self.clone();
        self.set_activity(Activity::Scanning);
        self.ctx.spawn(async move {
            let _ = run("nmcli", &["device", "wifi", "rescan"]).await;
            tokio::time::sleep(Duration::from_secs(3)).await;
            me.set_activity(Activity::Idle);
        });
    }

    pub fn connect(&self, ssid: String, password: String) {
        let me = self.clone();
        let known = self
            .last
            .lock()
            .map(|l| l.networks.iter().any(|n| n.ssid == ssid && n.known))
            .unwrap_or(false);
        self.set_activity(Activity::Connecting(ssid.clone()));
        self.ctx.spawn(async move {
            let result = if known && password.is_empty() {
                run("nmcli", &["connection", "up", "id", &ssid]).await
            } else if password.is_empty() {
                run("nmcli", &["device", "wifi", "connect", &ssid]).await
            } else {
                run(
                    "nmcli",
                    &["device", "wifi", "connect", &ssid, "password", &password],
                )
                .await
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
            run("nmcli", &["device", "disconnect", &dev])
                .await
                .map(|_| ())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::split_terse;

    #[test]
    fn terse_escapes() {
        assert_eq!(
            split_terse(r"*:My\:Net:57:WPA2"),
            vec!["*", "My:Net", "57", "WPA2"]
        );
        assert_eq!(split_terse(" :SAVOIA:43:"), vec![" ", "SAVOIA", "43", ""]);
    }
}

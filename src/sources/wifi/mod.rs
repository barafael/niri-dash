//! Wi-Fi through NetworkManager's D-Bus API. Besides listing and joining networks it registers
//! a secret agent, so NetworkManager asks the widget when a network needs a (new) password.

mod agent;
pub mod nm;

use super::{Ctx, Dedup, IconSrc, Msg, Trigger, signals_from};
use futures_util::StreamExt;
pub use nm::{Connectivity, Link, Network, Profile};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;

/// How often to ask for a full scan while the dashboard is shown. NetworkManager's own
/// background scans while connected can leave out most networks around.
const SCAN_EVERY: Duration = Duration::from_secs(30);
/// Saved profiles are read again after this long, in case another tool changed them.
const PROFILES_FOR: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}", dbus_message(.0))]
    Dbus(Box<zbus::Error>),
    #[error("{0}")]
    Nm(Box<nmrs::ConnectionError>),
    #[error("{0}")]
    Other(String),
}

impl From<zbus::Error> for Error {
    fn from(e: zbus::Error) -> Self {
        Self::Dbus(Box::new(e))
    }
}

impl From<nmrs::ConnectionError> for Error {
    fn from(e: nmrs::ConnectionError) -> Self {
        Self::Nm(Box::new(e))
    }
}

impl From<zbus::zvariant::Error> for Error {
    fn from(e: zbus::zvariant::Error) -> Self {
        Self::Dbus(Box::new(e.into()))
    }
}

/// NetworkManager's own message for an error it returned, without the D-Bus error name.
fn dbus_message(e: &zbus::Error) -> String {
    match e {
        zbus::Error::MethodError(_, Some(msg), _) => msg.clone(),
        e => e.to_string(),
    }
}

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
    fn of(security: Security) -> Self {
        match security {
            Security::Wep => Self::Wep,
            Security::Enterprise => Self::Enterprise,
            _ => Self::Password,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Prompt {
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
    key_mgmt: String,
    ap: String,
    hidden: bool,
    /// The saved profile to put the answer in when no agent request is waiting for it.
    profile: Option<Profile>,
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
    /// The network NetworkManager is joining right now (also when it does so by itself).
    pub connecting: String,
    pub networks: Vec<Network>,
    /// Saved networks, last used first.
    pub saved: Vec<Profile>,
    pub link: Link,
    pub activity: Activity,
    pub scanning: bool,
    pub prompt: Option<Prompt>,
    pub detail: Option<Detail>,
    pub secret: Option<Secret>,
    device: String,
    check_uri: String,
}

impl State {
    fn network(&self, ssid: &str) -> Option<&Network> {
        self.networks.iter().find(|n| n.ssid == ssid)
    }

    fn profile(&self, ssid: &str) -> Option<&Profile> {
        self.network(ssid)
            .and_then(|n| n.profile.as_ref())
            .or_else(|| self.saved.iter().find(|p| p.ssid == ssid))
    }
}

/// The widget's side of things, kept between refreshes.
#[derive(Default)]
struct Local {
    activity: Activity,
    prompt: Option<Prompt>,
    /// The secret agent's request the prompt answers, and its connection path.
    reply: Option<(String, oneshot::Sender<Option<Answer>>)>,
    /// The network last joined from the widget, and when: the agent asks for its passwords
    /// even while the dashboard is hidden.
    initiated: Option<(String, Instant)>,
    /// The agent asked for a password during the current activation.
    agent_asked: bool,
    /// The prompt was cancelled during the current activation.
    cancelled: bool,
    selected: String,
    secret: Option<Secret>,
    scan_requested: Option<(Instant, i64)>,
    portal_checked: Option<Instant>,
}

#[derive(Default)]
struct ProfileCache {
    map: HashMap<String, Option<Profile>>,
    since: Option<Instant>,
}

/// How an activation starts.
enum Activation {
    Profile(String),
    New {
        key_mgmt: String,
        ap: String,
        hidden: bool,
        answer: Option<Answer>,
    },
}

#[derive(Clone)]
pub struct Wifi {
    ctx: Ctx,
    last: Arc<Mutex<State>>,
    local: Arc<Mutex<Local>>,
    profiles: Arc<Mutex<ProfileCache>>,
    /// nmrs's client, for changing saved profiles and the radio; made on first use.
    nmrs: Arc<tokio::sync::OnceCell<nmrs::NetworkManager>>,
}

impl Wifi {
    pub fn start(ctx: Ctx) -> Self {
        let me = Self {
            ctx: ctx.clone(),
            last: Default::default(),
            local: Default::default(),
            profiles: Default::default(),
            nmrs: Default::default(),
        };
        let trigger = Trigger::default();
        ctx.on_dbus_signals(
            true,
            signals_from(nm::NM),
            Duration::from_millis(500),
            trigger.clone(),
        );
        let sent = Arc::new(Dedup::default());
        let m = me.clone();
        ctx.poll_events(trigger, SCAN_EVERY, move || {
            let (m, sent) = (m.clone(), sent.clone());
            async move {
                match m.fetch().await {
                    Ok(s) => {
                        if let Ok(mut l) = m.last.lock() {
                            *l = s.clone();
                        }
                        if sent.changed(&s) {
                            m.ctx.send(Msg::Wifi(Box::new(s)));
                        }
                    }
                    Err(e) => log::debug!("wifi: {e}"),
                }
            }
        });
        agent::start(me.clone());
        me
    }

    fn local(&self) -> std::sync::MutexGuard<'_, Local> {
        self.local.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn last(&self) -> State {
        self.last.lock().map(|l| l.clone()).unwrap_or_default()
    }

    async fn fetch(&self) -> Result<State, Error> {
        let conn = self.ctx.bus(true).await?;
        let snap = nm::snapshot(&conn).await?;
        let profiles = self.profiles(&conn, &snap.connections).await;
        let mut state = State {
            available: snap.device.is_some(),
            enabled: snap.enabled,
            blocked: !snap.hw_enabled,
            check_uri: snap.check_uri.clone(),
            ..Default::default()
        };
        let Some((dev_path, _iface, _)) = &snap.device else {
            return Ok(state);
        };
        state.device = dev_path.clone();
        let mut saved: Vec<Profile> = profiles.iter().filter(|p| !p.hotspot).cloned().collect();
        saved.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then(a.ssid.cmp(&b.ssid)));
        state.saved = saved;
        if snap.enabled {
            self.scan_if_due(&conn, dev_path, snap.last_scan).await;
            let active_ssid = match &snap.active {
                Some((path, activating)) => {
                    let ssid = profiles
                        .iter()
                        .find(|p| &p.path == path)
                        .map(|p| p.ssid.clone())
                        .unwrap_or_default();
                    if *activating {
                        state.connecting = ssid;
                        String::new()
                    } else {
                        ssid
                    }
                }
                None => String::new(),
            };
            state.networks = nm::networks(&snap.aps, &profiles, &active_ssid);
            if let Some(a) = state.networks.iter().find(|n| n.active) {
                state.ssid = a.ssid.clone();
                state.signal = a.signal;
            } else if !active_ssid.is_empty() {
                // A hidden network the scan doesn't name.
                state.ssid = active_ssid;
            }
            if !state.ssid.is_empty() {
                state.link = snap.link.clone();
                if state.link.connectivity == Connectivity::Portal {
                    self.check_portal_if_due(&conn).await;
                }
            }
        }
        let mut local = self.local();
        state.activity = local.activity.clone();
        state.prompt = local.prompt.clone();
        state.scanning = local.scan_requested.is_some_and(|(at, from)| {
            at.elapsed() < Duration::from_secs(10) && from == snap.last_scan
        });
        if !local.selected.is_empty() {
            let ssid = local.selected.clone();
            let network = state.network(&ssid).cloned();
            let profile = state.profile(&ssid).cloned();
            if network.is_some() || profile.is_some() {
                state.detail = Some(Detail {
                    ssid,
                    network,
                    profile,
                });
            }
        }
        // The password shown goes away with its page.
        let shown = state
            .detail
            .as_ref()
            .and_then(|d| d.profile.as_ref())
            .map(|p| p.uuid.clone());
        if local
            .secret
            .as_ref()
            .map(|s| Some(&s.uuid) != shown.as_ref())
            == Some(true)
        {
            local.secret = None;
        }
        state.secret = local.secret.clone();
        Ok(state)
    }

    /// The saved Wi-Fi profiles. Their settings are read once and kept for a while, since a
    /// refresh happens for every NetworkManager signal.
    async fn profiles(&self, conn: &zbus::Connection, paths: &[String]) -> Vec<Profile> {
        let missing: Vec<String> = {
            let mut c = self.profiles.lock().unwrap_or_else(|e| e.into_inner());
            if c.since.is_none_or(|t| t.elapsed() > PROFILES_FOR) {
                c.map.clear();
                c.since = Some(Instant::now());
            }
            c.map.retain(|p, _| paths.contains(p));
            paths
                .iter()
                .filter(|p| !c.map.contains_key(*p))
                .cloned()
                .collect()
        };
        let mut read = Vec::new();
        for path in missing {
            let profile = match nm::settings(conn, &path).await {
                Ok(s) => nm::profile(&path, &s),
                Err(e) => {
                    log::debug!("wifi: settings of {path}: {e}");
                    None
                }
            };
            read.push((path, profile));
        }
        let mut c = self.profiles.lock().unwrap_or_else(|e| e.into_inner());
        c.map.extend(read);
        c.map.values().flatten().cloned().collect()
    }

    async fn nmrs(&self) -> Result<&nmrs::NetworkManager, Error> {
        Ok(self.nmrs.get_or_try_init(nmrs::NetworkManager::new).await?)
    }

    /// The saved Wi-Fi profile at a settings path, if it has been read.
    fn profile_at(&self, path: &str) -> Option<Profile> {
        let c = self.profiles.lock().unwrap_or_else(|e| e.into_inner());
        c.map.get(path).cloned().flatten()
    }

    fn forget_profiles(&self) {
        if let Ok(mut c) = self.profiles.lock() {
            c.since = None;
        }
    }

    async fn scan_if_due(&self, conn: &zbus::Connection, dev: &str, last_scan: i64) {
        {
            let mut local = self.local();
            if local
                .scan_requested
                .is_some_and(|(at, _)| at.elapsed() < SCAN_EVERY)
            {
                return;
            }
            local.scan_requested = Some((Instant::now(), last_scan));
        }
        let options: HashMap<&str, zbus::zvariant::Value> = HashMap::new();
        // Refused while a scan runs or right after one; either way a fresh list is coming.
        if let Err(e) = nm::call(conn, dev, nm::WIRELESS, "RequestScan", &(options,)).await {
            log::debug!("wifi: scan: {e}");
        }
    }

    async fn check_portal_if_due(&self, conn: &zbus::Connection) {
        {
            let mut local = self.local();
            if local
                .portal_checked
                .is_some_and(|t| t.elapsed() < Duration::from_secs(15))
            {
                return;
            }
            local.portal_checked = Some(Instant::now());
        }
        let conn = conn.clone();
        self.ctx.spawn(async move {
            let _ = nm::call(&conn, nm::NM_PATH, nm::NM, "CheckConnectivity", &()).await;
        });
    }

    fn set_activity(&self, a: Activity) {
        self.local().activity = a;
        self.ctx.refresh();
    }

    /// Show `why` for a while, unless something else happened meanwhile.
    fn fail(&self, why: String) {
        let failed = Activity::Failed(why);
        self.set_activity(failed.clone());
        let me = self.clone();
        self.ctx.spawn(async move {
            tokio::time::sleep(Duration::from_secs(8)).await;
            let mut local = me.local();
            if local.activity == failed {
                local.activity = Activity::Idle;
                drop(local);
                me.ctx.refresh();
            }
        });
    }

    pub fn toggle(&self) {
        let on = !self.last().enabled;
        let me = self.clone();
        self.ctx.act("Wi-Fi", async move {
            me.nmrs().await?.set_wireless_enabled(on).await?;
            Ok::<_, Error>(())
        });
    }

    /// Scan now, whenever the last scan was.
    pub fn rescan(&self) {
        self.local().scan_requested = None;
        self.ctx.refresh();
    }

    /// Join `ssid`: through its saved profile, or a new one, asking for a password first if it
    /// takes one.
    pub fn connect(&self, ssid: String) {
        let last = self.last();
        if let Some(p) = last.profile(&ssid) {
            self.activate(ssid, Activation::Profile(p.path.clone()));
            return;
        }
        let Some(net) = last.network(&ssid).cloned() else {
            return;
        };
        if net.security.needs_secret() {
            self.ask(Prompt {
                ssid,
                kind: PromptKind::of(net.security),
                new: true,
                key_mgmt: net.key_mgmt().into(),
                ap: net.ap.clone(),
                ..Default::default()
            });
        } else {
            self.activate(
                ssid,
                Activation::New {
                    key_mgmt: net.key_mgmt().into(),
                    ap: net.ap,
                    hidden: false,
                    answer: None,
                },
            );
        }
    }

    /// Join a network that doesn't broadcast its name. `security`: 0 open, 1 WPA2/WPA3
    /// personal, 2 enterprise, 3 WEP.
    pub fn join_hidden(&self, ssid: String, security: i32, answer: Answer) {
        let ssid = ssid.trim().to_string();
        if ssid.is_empty() {
            return;
        }
        let key_mgmt = match security {
            1 => "wpa-psk",
            2 => "wpa-eap",
            3 => "none",
            _ => "",
        };
        if let Some(why) = check_answer(key_mgmt, &answer) {
            self.fail(why);
            return;
        }
        self.activate(
            ssid,
            Activation::New {
                key_mgmt: key_mgmt.into(),
                ap: "/".into(),
                hidden: true,
                answer: Some(answer),
            },
        );
    }

    fn ask(&self, prompt: Prompt) {
        let mut local = self.local();
        if let Some((_, tx)) = local.reply.take() {
            let _ = tx.send(None);
        }
        local.prompt = Some(prompt);
        drop(local);
        self.ctx.refresh();
    }

    /// The prompt was answered: hand the answer to the agent's waiting request, or connect.
    pub fn answer(&self, answer: Answer) {
        let mut local = self.local();
        let Some(prompt) = local.prompt.clone() else {
            return;
        };
        let key_mgmt = match prompt.kind {
            PromptKind::Wep => "none",
            PromptKind::Enterprise => "wpa-eap",
            PromptKind::Password => "wpa-psk",
        };
        if let Some(why) = check_answer(key_mgmt, &answer) {
            local.prompt = Some(Prompt {
                message: why,
                ..prompt
            });
            drop(local);
            self.ctx.refresh();
            return;
        }
        local.prompt = None;
        if let Some((_, tx)) = local.reply.take() {
            let _ = tx.send(Some(answer));
            drop(local);
            self.ctx.refresh();
            return;
        }
        drop(local);
        match prompt.profile {
            Some(profile) => {
                let me = self.clone();
                let ssid = prompt.ssid.clone();
                self.ctx.spawn(async move {
                    match me.store_secret(&profile.uuid, prompt.kind, &answer).await {
                        Ok(()) => {
                            me.forget_profiles();
                            me.activate(ssid, Activation::Profile(profile.path));
                        }
                        Err(e) => me.fail(e.to_string()),
                    }
                });
            }
            None => self.activate(
                prompt.ssid,
                Activation::New {
                    key_mgmt: prompt.key_mgmt,
                    ap: prompt.ap,
                    hidden: prompt.hidden,
                    answer: Some(answer),
                },
            ),
        }
    }

    pub fn cancel_prompt(&self) {
        let mut local = self.local();
        local.prompt = None;
        local.cancelled = true;
        if let Some((_, tx)) = local.reply.take() {
            let _ = tx.send(None);
        }
        drop(local);
        self.ctx.refresh();
    }

    fn activate(&self, ssid: String, how: Activation) {
        let me = self.clone();
        {
            let mut local = self.local();
            local.activity = Activity::Connecting(ssid.clone());
            local.initiated = Some((ssid.clone(), Instant::now()));
            local.agent_asked = false;
            local.cancelled = false;
        }
        self.ctx.refresh();
        self.ctx.spawn(async move {
            match me.run_activation(&ssid, how).await {
                Ok(()) => me.set_activity(Activity::Idle),
                Err(e) => me.fail(e.to_string()),
            }
        });
    }

    async fn run_activation(&self, ssid: &str, how: Activation) -> Result<(), Error> {
        let conn = self.ctx.bus(true).await?;
        let dev = self.last().device;
        if dev.is_empty() {
            return Err(Error::Other("no Wi-Fi device".into()));
        }
        // Listen before activating, so a quick failure isn't missed.
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(nm::NM)?
            .path(dev.as_str())?
            .interface(nm::DEVICE)?
            .member("StateChanged")?
            .build();
        let mut changes = zbus::MessageStream::for_match_rule(rule, &conn, Some(64)).await?;
        let (created, retry_with) = match how {
            Activation::Profile(path) => {
                nm::call(
                    &conn,
                    nm::NM_PATH,
                    nm::NM,
                    "ActivateConnection",
                    &(nm::path(&path)?, nm::path(&dev)?, nm::path("/")?),
                )
                .await?;
                (None, None)
            }
            Activation::New {
                key_mgmt,
                ap,
                hidden,
                answer,
            } => {
                let settings = nm::new_connection(ssid, &key_mgmt, hidden, answer.as_ref());
                let reply = nm::call(
                    &conn,
                    nm::NM_PATH,
                    nm::NM,
                    "AddAndActivateConnection",
                    &(settings, nm::path(&dev)?, nm::path(&ap)?),
                )
                .await?;
                let (path, _active): (
                    zbus::zvariant::OwnedObjectPath,
                    zbus::zvariant::OwnedObjectPath,
                ) = reply.body().deserialize()?;
                self.forget_profiles();
                (Some(path.to_string()), Some((key_mgmt, ap, hidden)))
            }
        };
        let outcome = tokio::time::timeout(Duration::from_secs(180), async {
            let mut started = false;
            while let Some(msg) = changes.next().await {
                let Ok(msg) = msg else { continue };
                let Ok((new, _old, reason)) = msg.body().deserialize::<(u32, u32, u32)>() else {
                    continue;
                };
                if (nm::STATE_PREPARE..nm::STATE_ACTIVATED).contains(&new) {
                    started = true;
                }
                match new {
                    nm::STATE_ACTIVATED if started => return Ok(()),
                    nm::STATE_FAILED => return Err(reason),
                    nm::STATE_DISCONNECTED | nm::STATE_DEACTIVATING if started => {
                        return Err(reason);
                    }
                    _ => {}
                }
            }
            Err(0)
        })
        .await;
        let reason = match outcome {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(reason)) => reason,
            Err(_) => {
                if let Some(path) = &created {
                    let _ = nm::call(&conn, path, nm::CONNECTION, "Delete", &()).await;
                }
                return Err(Error::Other("timed out".into()));
            }
        };
        // A connection made just now that never worked isn't worth keeping.
        if let Some(path) = &created {
            let _ = nm::call(&conn, path, nm::CONNECTION, "Delete", &()).await;
            self.forget_profiles();
        }
        let (asked, cancelled) = {
            let local = self.local();
            (local.agent_asked, local.cancelled)
        };
        // Replaced by another activation, or stopped by the user.
        if matches!(reason, 39 | 60) || cancelled {
            return Ok(());
        }
        // A password problem nobody was asked about (no agent got the request): ask here.
        if matches!(reason, 7..=11) && !asked {
            let last = self.last();
            let security = last
                .network(ssid)
                .map(|n| n.security)
                .or_else(|| last.profile(ssid).map(|p| p.security()))
                .unwrap_or_default();
            if security.needs_secret() {
                let profile = match &created {
                    Some(_) => None,
                    None => last.profile(ssid).cloned(),
                };
                let (key_mgmt, ap, hidden) = retry_with.unwrap_or_default();
                self.local().activity = Activity::Idle;
                self.ask(Prompt {
                    ssid: ssid.into(),
                    kind: PromptKind::of(security),
                    retry: true,
                    new: profile.is_none(),
                    key_mgmt,
                    ap,
                    hidden,
                    profile,
                    ..Default::default()
                });
                return Ok(());
            }
        }
        Err(Error::Other(nm::failure(reason)))
    }

    pub fn disconnect(&self) {
        let dev = self.last().device;
        if dev.is_empty() {
            return;
        }
        let ctx = self.ctx.clone();
        self.ctx.act("Disconnect", async move {
            let conn = ctx.bus(true).await?;
            nm::call(&conn, &dev, nm::DEVICE, "Disconnect", &())
                .await
                .map(|_| ())
        });
    }

    /// Delete a saved network.
    pub fn forget(&self, uuid: String) {
        let me = self.clone();
        self.ctx.act("Forget network", async move {
            let r = me.nmrs().await?.delete_saved_connection(&uuid).await;
            me.forget_profiles();
            Ok::<_, Error>(r?)
        });
    }

    pub fn set_autoconnect(&self, uuid: String, on: bool) {
        let me = self.clone();
        self.ctx.act("Auto-connect", async move {
            let mut patch = nmrs::SettingsPatch::default();
            patch.autoconnect = Some(on);
            let r = me.nmrs().await?.update_saved_connection(&uuid, patch).await;
            me.forget_profiles();
            Ok::<_, Error>(r?)
        });
    }

    /// Put a password into a saved profile, stored by NetworkManager itself (not an agent).
    /// NetworkManager keeps the profile's other secrets.
    async fn store_secret(
        &self,
        uuid: &str,
        kind: PromptKind,
        answer: &Answer,
    ) -> Result<(), Error> {
        let mut patch = nmrs::SettingsPatch::default();
        patch.raw_overlay = Some(secret_overlay(kind, answer));
        self.nmrs()
            .await?
            .update_saved_connection(uuid, patch)
            .await?;
        Ok(())
    }

    /// Show the details page of `ssid` ("" to leave it).
    pub fn select(&self, ssid: String) {
        let mut local = self.local();
        if local.selected != ssid {
            local.secret = None;
        }
        local.selected = ssid;
        drop(local);
        self.ctx.refresh();
    }

    /// Read a saved network's password, to show it and its QR code.
    pub fn reveal(&self, uuid: String) {
        let last = self.last();
        let Some(profile) = last.saved.iter().find(|p| p.uuid == uuid).cloned() else {
            return;
        };
        let me = self.clone();
        self.ctx.act("Show password", async move {
            let security = profile.security();
            let password = if security.needs_secret() {
                let conn = me.ctx.bus(true).await?;
                let setting = if security == Security::Enterprise {
                    "802-1x"
                } else {
                    "802-11-wireless-security"
                };
                let s = nm::secrets(&conn, &profile.path, setting).await?;
                let values = s.get(setting);
                ["psk", "wep-key0", "password"]
                    .iter()
                    .find_map(|k| {
                        values
                            .and_then(|v| super::prop::<&str>(v, k))
                            .map(str::to_string)
                    })
                    .ok_or_else(|| Error::Other("NetworkManager has no password stored".into()))?
            } else {
                String::new()
            };
            let text = nm::qr_text(&profile.ssid, security, &password, profile.hidden);
            me.local().secret = Some(Secret {
                uuid,
                password,
                qr: qr_image(&text),
            });
            Ok::<_, Error>(())
        });
    }

    pub fn hide_secret(&self) {
        self.local().secret = None;
        self.ctx.refresh();
    }

    /// Open the captive portal's login page.
    pub fn sign_in(&self) {
        let uri = self.last().check_uri;
        let uri = if uri.starts_with("http://") {
            uri
        } else {
            "http://neverssl.com".into()
        };
        if let Err(e) = super::spawn_detached("xdg-open", &[&uri]) {
            self.ctx.toast(format!("Sign in: {e}"));
        }
        self.local().portal_checked = None;
    }

    /// For the agent: whether to ask about `ssid` now rather than leave it to another agent.
    fn may_ask(&self, ssid: &str) -> bool {
        self.ctx.visible()
            || self
                .local()
                .initiated
                .as_ref()
                .is_some_and(|(s, at)| s == ssid && at.elapsed() < Duration::from_secs(180))
    }

    /// For the agent: show `prompt` and wait for its answer (None: cancelled).
    fn ask_agent(&self, prompt: Prompt, connection: String) -> oneshot::Receiver<Option<Answer>> {
        let (tx, rx) = oneshot::channel();
        let mut local = self.local();
        if let Some((_, old)) = local.reply.take() {
            let _ = old.send(None);
        }
        local.prompt = Some(prompt);
        local.reply = Some((connection, tx));
        local.agent_asked = true;
        if matches!(local.activity, Activity::Connecting(_)) {
            // The prompt says what's going on.
        } else {
            local.activity = Activity::Idle;
        }
        drop(local);
        self.ctx.refresh();
        rx
    }

    /// For the agent: NetworkManager no longer needs the answer.
    fn drop_agent_request(&self, connection: &str) {
        let mut local = self.local();
        if local.reply.as_ref().is_some_and(|(c, _)| c == connection) {
            local.reply = None;
            local.prompt = None;
        }
        drop(local);
        self.ctx.refresh();
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
            let band = match nm::band_bit(link.freq) {
                1 => "2.4 GHz",
                2 => "5 GHz",
                _ => "6 GHz",
            };
            add("Channel", format!("{} ({band})", nm::channel(link.freq)));
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

/// Why `answer` can't work for `key_mgmt`, if it can't.
fn check_answer(key_mgmt: &str, answer: &Answer) -> Option<String> {
    let pw = &answer.password;
    match key_mgmt {
        "wpa-psk" | "sae" => {
            let hex_key = pw.len() == 64 && pw.chars().all(|c| c.is_ascii_hexdigit());
            (!(8..=63).contains(&pw.chars().count()) && !hex_key)
                .then(|| "A WPA password has 8 to 63 characters.".into())
        }
        "wpa-eap" if answer.identity.trim().is_empty() => Some("Enter a user name.".into()),
        "none" | "wpa-eap" if pw.is_empty() => Some("Enter the password.".into()),
        _ => None,
    }
}

/// The settings that put `answer` into a profile, as system-owned secrets.
fn secret_overlay(kind: PromptKind, answer: &Answer) -> nm::Settings {
    let v = |v: zbus::zvariant::Value<'_>| {
        zbus::zvariant::OwnedValue::try_from(v).expect("plain values have no fds")
    };
    let (setting, values) = match kind {
        PromptKind::Password => (
            "802-11-wireless-security",
            vec![
                ("psk", v(answer.password.as_str().into())),
                ("psk-flags", v(0u32.into())),
            ],
        ),
        PromptKind::Wep => (
            "802-11-wireless-security",
            vec![
                ("wep-key0", v(answer.password.as_str().into())),
                ("wep-key-flags", v(0u32.into())),
            ],
        ),
        PromptKind::Enterprise => (
            "802-1x",
            vec![
                ("identity", v(answer.identity.as_str().into())),
                ("password", v(answer.password.as_str().into())),
                ("password-flags", v(0u32.into())),
            ],
        ),
    };
    HashMap::from([(
        setting.to_string(),
        values
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )])
}

/// A QR code as an image, one pixel per module (the UI scales it up without smoothing), with a
/// quiet zone around it.
fn qr_image(text: &str) -> IconSrc {
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

#[cfg(test)]
mod tests {
    use super::{Answer, check_answer};

    #[test]
    fn answers() {
        let pw = |p: &str| Answer {
            password: p.into(),
            ..Default::default()
        };
        assert!(check_answer("wpa-psk", &pw("short")).is_some());
        assert!(check_answer("wpa-psk", &pw("long enough")).is_none());
        assert!(check_answer("sae", &pw(&"z".repeat(64))).is_some());
        assert!(check_answer("wpa-psk", &pw(&"ab".repeat(32))).is_none());
        assert!(check_answer("wpa-eap", &pw("secret")).is_some());
        assert!(check_answer("", &pw("")).is_none());
    }
}

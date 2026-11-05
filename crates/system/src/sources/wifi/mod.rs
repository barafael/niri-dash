//! Wi-Fi through NetworkManager's D-Bus API. Besides listing and joining networks it registers
//! a secret agent, so NetworkManager asks the widget when a network needs a (new) password.

mod agent;
pub mod nm;

use super::connector::Connector;
use super::{Ctx, Dedup, Mailbox, Trigger, signals_from};
use crate::live::Stateful;
use crate::wifi::qr_image;
pub use crate::wifi::{
    Activity, Answer, Connectivity, Detail, Eap, Profile, Prompt, PromptKind, Secret, Security,
    State,
};
use futures_util::StreamExt;
use nmrs::agent::{SecretAgentFlags, SecretRequest, SecretSetting};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::{oneshot, watch};

/// How often to ask for a full scan while the dashboard is shown. NetworkManager's own
/// background scans while connected can leave out most networks around.
const SCAN_EVERY: Duration = Duration::from_secs(30);
/// Saved profiles are read again after this long, in case another tool changed them.
const PROFILES_FOR: Duration = Duration::from_secs(30);
/// How often to ask NetworkManager to look at a captive portal again.
const PORTAL_EVERY: Duration = Duration::from_secs(15);

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

/// How an activation starts.
#[derive(Debug, PartialEq)]
enum Activation {
    Profile(String),
    New {
        key_mgmt: String,
        ap: String,
        hidden: bool,
        answer: Option<Answer>,
    },
}

/// How an activation ended, as far as the task watching it could tell.
#[derive(Debug, PartialEq)]
pub enum Ended {
    Activated,
    /// NetworkManager gave up, for this reason.
    Failed {
        reason: u32,
        /// The profile that was activated, not just any with that SSID.
        activated: Option<String>,
        /// How a new connection was going to be made.
        retry_with: Option<(String, String, bool)>,
    },
    /// It never got that far, or took too long.
    Error(String),
}

/// One look at NetworkManager, with the saved profiles it has.
#[derive(Debug, Default)]
pub struct Looked {
    snap: nm::Snapshot,
    /// By settings path; `None`: not a Wi-Fi profile, or unreadable.
    profiles: HashMap<String, Option<Profile>>,
}

/// What reaches the core from the actor's tasks.
#[derive(Debug)]
pub enum Found {
    Fetched(Result<Box<Looked>, Error>),
    /// The dashboard was hidden: a shown password goes with it.
    Hidden,
    ActivationEnded {
        ssid: String,
        ended: Ended,
    },
    /// The password went into the profile: join with it.
    SecretStored {
        ssid: String,
        path: String,
    },
    /// Show this for a while.
    Failed(String),
    /// A failure has been on show for long enough.
    FailureOver(String),
    /// A password was read, for the page it was asked for at this generation.
    Revealed {
        generation: u64,
        secret: Secret,
    },
    /// NetworkManager no longer needs an answer about this connection, or nobody gave one.
    AgentGone(String),
}

/// What reaches the actor. A request is the whole of what the agent asks, in one message, so what
/// it depends on can't change between looking and asking.
pub enum Inner {
    Core(Found),
    Agent(Box<SecretRequest>),
}

impl From<Found> for Inner {
    fn from(found: Found) -> Self {
        Self::Core(found)
    }
}

/// What the agent asks for, without the means to answer.
#[derive(Debug, PartialEq)]
struct AgentAsk {
    path: String,
    connection_id: String,
    asks: Asks,
    /// NetworkManager lets us bother the user.
    interactive: bool,
    /// What it has didn't work.
    request_new: bool,
}

#[derive(Debug, PartialEq)]
enum Asks {
    Psk { ssid: String },
    Eap { identity: Option<String> },
    Other,
}

/// What to tell the agent: nothing here, or wait for the prompt.
#[derive(Debug)]
enum AgentReply {
    NoSecrets,
    Wait(PromptKind, oneshot::Receiver<Option<Answer>>),
}

/// What the actor does for what `Core` decided. Each is one whole job, in order within itself.
#[derive(Debug, PartialEq)]
enum Effect {
    Publish(Box<State>),
    Toast(String),
    /// Wake the pollers, which is how what changed here reaches the widget.
    Refresh,
    SetEnabled(bool),
    RequestScan {
        device: String,
    },
    CheckConnectivity,
    Activate {
        ssid: String,
        device: String,
        how: Activation,
    },
    StoreSecret {
        ssid: String,
        profile: Box<Profile>,
        kind: PromptKind,
        answer: Answer,
    },
    Disconnect {
        device: String,
    },
    Forget {
        uuid: String,
    },
    SetAutoconnect {
        uuid: String,
        on: bool,
    },
    Reveal {
        profile: Box<Profile>,
        generation: u64,
    },
    SignIn {
        uri: String,
    },
    /// Say `FailureOver` a while from now.
    EndFailure(String),
}

/// The widget's side of things: all the decisions, none of the talking to NetworkManager.
#[derive(Default)]
struct Core {
    last: State,
    sent: Dedup<State>,
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
    /// The key of the network whose details are shown.
    selected: String,
    secret: Option<Secret>,
    /// Bumped whenever a shown password must go: a reveal still on its way is then dropped.
    secret_gen: u64,
    prompt_serial: u64,
    scan_requested: Option<(Instant, i64)>,
    portal_checked: Option<Instant>,
    /// The saved profiles at the last look, hotspots too, which the lists leave out.
    profiles: HashMap<String, Option<Profile>>,
}

impl Core {
    fn on_command(&mut self, cmd: crate::wifi::Command, now: Instant) -> Vec<Effect> {
        use crate::wifi::Command::*;
        match cmd {
            Toggle => vec![Effect::SetEnabled(!self.last.enabled)],
            Rescan => {
                self.scan_requested = None;
                vec![Effect::Refresh]
            }
            Connect { key } => self.connect(key, now),
            JoinHidden {
                ssid,
                security,
                answer,
            } => self.join_hidden(ssid, security, answer, now),
            Answer { serial, answer } => self.answer(serial, answer, now),
            CancelPrompt => {
                self.prompt = None;
                self.cancelled = true;
                if let Some((_, reply)) = self.reply.take() {
                    let _ = reply.send(None);
                }
                vec![Effect::Refresh]
            }
            Disconnect => {
                if self.last.device.is_empty() {
                    return Vec::new();
                }
                vec![Effect::Disconnect {
                    device: self.last.device.clone(),
                }]
            }
            Forget { uuid } => vec![Effect::Forget { uuid }],
            SetAutoconnect { uuid, on } => vec![Effect::SetAutoconnect { uuid, on }],
            Select { key } => {
                if self.selected != key {
                    self.drop_secret();
                }
                self.selected = key;
                vec![Effect::Refresh]
            }
            Reveal { uuid } => {
                let Some(profile) = self.last.saved.iter().find(|p| p.uuid == uuid).cloned() else {
                    return Vec::new();
                };
                vec![Effect::Reveal {
                    profile: Box::new(profile),
                    generation: self.secret_gen,
                }]
            }
            HideSecret => {
                self.drop_secret();
                vec![Effect::Refresh]
            }
            SignIn => {
                let uri = if self.last.check_uri.starts_with("http://") {
                    self.last.check_uri.clone()
                } else {
                    "http://neverssl.com".into()
                };
                self.portal_checked = None;
                vec![Effect::SignIn { uri }]
            }
        }
    }

    fn on_found(&mut self, found: Found, now: Instant) -> Vec<Effect> {
        match found {
            Found::Fetched(Ok(looked)) => self.looked(*looked, now),
            Found::Fetched(Err(e)) => {
                // What it was stays on show.
                log::debug!("wifi: {e}");
                Vec::new()
            }
            Found::Hidden => {
                self.drop_secret();
                let mut effects = vec![Effect::Refresh];
                if self.last.secret.take().is_some() {
                    // Remembered without the password too, as what was last sent.
                    self.sent.changed(&self.last);
                    effects.push(Effect::Publish(Box::new(self.last.clone())));
                }
                effects
            }
            Found::ActivationEnded { ssid, ended } => self.activation_ended(ssid, ended),
            Found::SecretStored { ssid, path } => {
                self.activate(ssid, Activation::Profile(path), now)
            }
            Found::Failed(why) => self.fail(why),
            Found::FailureOver(why) => {
                if self.activity == Activity::Failed(why) {
                    self.set_activity(Activity::Idle)
                } else {
                    Vec::new()
                }
            }
            Found::Revealed { generation, secret } => {
                // Hidden, or another page, while NetworkManager answered: don't show it after all.
                if self.secret_gen == generation {
                    self.secret = Some(secret);
                }
                Vec::new()
            }
            Found::AgentGone(path) => {
                if self.reply.as_ref().is_some_and(|(c, _)| *c == path) {
                    self.reply = None;
                    self.prompt = None;
                }
                vec![Effect::Refresh]
            }
        }
    }

    fn drop_secret(&mut self) {
        self.secret = None;
        self.secret_gen += 1;
    }

    /// Make the state from a look at NetworkManager, with the widget's side added.
    fn looked(&mut self, looked: Looked, now: Instant) -> Vec<Effect> {
        let Looked { snap, profiles } = looked;
        let list: Vec<Profile> = profiles.values().flatten().cloned().collect();
        self.profiles = profiles;
        let mut effects = Vec::new();
        let mut state = State {
            available: snap.device.is_some(),
            enabled: snap.enabled,
            blocked: !snap.hw_enabled,
            check_uri: snap.check_uri.clone(),
            ..Default::default()
        };
        if let Some((dev_path, _iface, _)) = &snap.device {
            state.device = dev_path.clone();
            let mut saved: Vec<Profile> = list.iter().filter(|p| !p.hotspot).cloned().collect();
            saved.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then(a.ssid.cmp(&b.ssid)));
            state.saved = saved;
            if snap.enabled {
                if !self
                    .scan_requested
                    .is_some_and(|(at, _)| now.saturating_duration_since(at) < SCAN_EVERY)
                {
                    self.scan_requested = Some((now, snap.last_scan));
                    effects.push(Effect::RequestScan {
                        device: dev_path.clone(),
                    });
                }
                let active = snap.active.as_ref().and_then(|(path, activating)| {
                    let p = list.iter().find(|p| &p.path == path)?;
                    Some((p.ssid.clone(), p.key(), *activating))
                });
                let (active_ssid, active_key) = match active {
                    Some((ssid, key, true)) => {
                        state.connecting = ssid;
                        state.connecting_key = key;
                        (String::new(), String::new())
                    }
                    Some((ssid, key, false)) => (ssid, key),
                    None => Default::default(),
                };
                state.networks = nm::networks(&snap.aps, &list, &active_key);
                if let Some(a) = state.networks.iter().find(|n| n.active) {
                    state.ssid = a.ssid.clone();
                    state.signal = a.signal;
                } else if !active_ssid.is_empty() {
                    // A hidden network the scan doesn't name.
                    state.ssid = active_ssid;
                }
                if !state.ssid.is_empty() {
                    state.link = snap.link.clone();
                    if state.link.connectivity == Connectivity::Portal
                        && !self
                            .portal_checked
                            .is_some_and(|t| now.saturating_duration_since(t) < PORTAL_EVERY)
                    {
                        self.portal_checked = Some(now);
                        effects.push(Effect::CheckConnectivity);
                    }
                }
            }
            state.activity = self.activity.clone();
            state.prompt = self.prompt.clone();
            state.scanning = self.scan_requested.is_some_and(|(at, from)| {
                now.saturating_duration_since(at) < Duration::from_secs(10)
                    && from == snap.last_scan
            });
            if !self.selected.is_empty() {
                let key = self.selected.clone();
                let network = state.network(&key).cloned();
                let profile = state.profile(&key).cloned();
                let ssid = network
                    .as_ref()
                    .map(|n| n.ssid.clone())
                    .or_else(|| profile.as_ref().map(|p| p.ssid.clone()));
                if let Some(ssid) = ssid {
                    state.detail = Some(Detail {
                        key,
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
            if self
                .secret
                .as_ref()
                .is_some_and(|s| Some(&s.uuid) != shown.as_ref())
            {
                self.drop_secret();
            }
            state.secret = self.secret.clone();
        }
        self.last = state.clone();
        if self.sent.changed(&state) {
            effects.insert(0, Effect::Publish(Box::new(state)));
        }
        effects
    }

    fn set_activity(&mut self, activity: Activity) -> Vec<Effect> {
        self.activity = activity;
        vec![Effect::Refresh]
    }

    /// Show `why` for a while, unless something else happened meanwhile.
    fn fail(&mut self, why: String) -> Vec<Effect> {
        self.activity = Activity::Failed(why.clone());
        vec![Effect::Refresh, Effect::EndFailure(why)]
    }

    /// Join the network with this key (see `nm::key`): through its saved profile, or a new
    /// one, asking for a password first if it takes one.
    fn connect(&mut self, key: String, now: Instant) -> Vec<Effect> {
        if let Some(p) = self.last.profile(&key) {
            let (ssid, path) = (p.ssid.clone(), p.path.clone());
            return self.activate(ssid, Activation::Profile(path), now);
        }
        let Some(net) = self.last.network(&key).cloned() else {
            return Vec::new();
        };
        let ssid = net.ssid.clone();
        if net.security.needs_secret() {
            self.ask(Prompt {
                ssid,
                kind: PromptKind::of(net.security),
                new: true,
                key_mgmt: net.key_mgmt().into(),
                ap: net.ap.clone(),
                ..Default::default()
            })
        } else {
            self.activate(
                ssid,
                Activation::New {
                    key_mgmt: net.key_mgmt().into(),
                    ap: net.ap,
                    hidden: false,
                    answer: None,
                },
                now,
            )
        }
    }

    /// Join a network that doesn't broadcast its name. `security`: 0 open, 1 WPA2/WPA3
    /// personal, 2 enterprise, 3 WEP.
    fn join_hidden(
        &mut self,
        ssid: String,
        security: i32,
        answer: Answer,
        now: Instant,
    ) -> Vec<Effect> {
        let ssid = ssid.trim().to_string();
        if ssid.is_empty() {
            return Vec::new();
        }
        let key_mgmt = match security {
            1 => "wpa-psk",
            2 => "wpa-eap",
            3 => "none",
            _ => "",
        };
        if let Some(why) = check_answer(key_mgmt, true, &answer) {
            return self.fail(why);
        }
        self.activate(
            ssid,
            Activation::New {
                key_mgmt: key_mgmt.into(),
                ap: "/".into(),
                hidden: true,
                answer: Some(answer),
            },
            now,
        )
    }

    /// Show `prompt`. A request waiting on the one it replaces hears no.
    fn ask(&mut self, prompt: Prompt) -> Vec<Effect> {
        if let Some((_, reply)) = self.reply.take() {
            let _ = reply.send(None);
        }
        self.prompt_serial += 1;
        self.prompt = Some(Prompt {
            serial: self.prompt_serial,
            ..prompt
        });
        vec![Effect::Refresh]
    }

    /// The prompt `serial` was answered: hand the answer to the agent's waiting request, or
    /// connect. An answer to a prompt that has since been replaced goes nowhere.
    fn answer(&mut self, serial: u64, answer: Answer, now: Instant) -> Vec<Effect> {
        let Some(prompt) = self.prompt.clone() else {
            return Vec::new();
        };
        if prompt.serial != serial {
            log::debug!("wifi: dropping an answer to an earlier prompt");
            return Vec::new();
        }
        let key_mgmt = match prompt.kind {
            PromptKind::Wep => "none",
            PromptKind::Enterprise => "wpa-eap",
            PromptKind::Password => "wpa-psk",
        };
        if let Some(why) = check_answer(key_mgmt, prompt.new, &answer) {
            self.prompt = Some(Prompt {
                message: why,
                ..prompt
            });
            return vec![Effect::Refresh];
        }
        self.prompt = None;
        if let Some((_, reply)) = self.reply.take() {
            let _ = reply.send(Some(answer));
            return vec![Effect::Refresh];
        }
        match prompt.profile {
            Some(profile) => vec![Effect::StoreSecret {
                ssid: prompt.ssid,
                profile: Box::new(profile),
                kind: prompt.kind,
                answer,
            }],
            None => self.activate(
                prompt.ssid,
                Activation::New {
                    key_mgmt: prompt.key_mgmt,
                    ap: prompt.ap,
                    hidden: prompt.hidden,
                    answer: Some(answer),
                },
                now,
            ),
        }
    }

    fn activate(&mut self, ssid: String, how: Activation, now: Instant) -> Vec<Effect> {
        self.activity = Activity::Connecting(ssid.clone());
        self.initiated = Some((ssid.clone(), now));
        self.agent_asked = false;
        self.cancelled = false;
        vec![
            Effect::Refresh,
            Effect::Activate {
                ssid,
                device: self.last.device.clone(),
                how,
            },
        ]
    }

    fn activation_ended(&mut self, ssid: String, ended: Ended) -> Vec<Effect> {
        let (reason, activated, retry_with) = match ended {
            Ended::Activated => return self.set_activity(Activity::Idle),
            Ended::Error(why) => return self.fail(why),
            Ended::Failed {
                reason,
                activated,
                retry_with,
            } => (reason, activated, retry_with),
        };
        // Replaced by another activation, or stopped by the user.
        if matches!(reason, 39 | 60) || self.cancelled {
            return self.set_activity(Activity::Idle);
        }
        // A password problem nobody was asked about (no agent got the request): ask here.
        if matches!(reason, 7..=11) && !self.agent_asked {
            let profile = activated.and_then(|path| {
                self.profile_at(&path)
                    .or_else(|| self.last.saved.iter().find(|p| p.path == path).cloned())
            });
            let (key_mgmt, ap, hidden) = retry_with.unwrap_or_default();
            let security = match &profile {
                Some(p) => p.security(),
                None => nm::security_of(&key_mgmt),
            };
            if security.needs_secret() {
                let mut effects = self.ask(Prompt {
                    ssid,
                    kind: PromptKind::of(security),
                    retry: true,
                    new: profile.is_none(),
                    key_mgmt,
                    ap,
                    hidden,
                    profile,
                    ..Default::default()
                });
                effects.extend(self.set_activity(Activity::Idle));
                return effects;
            }
        }
        self.fail(nm::failure(reason))
    }

    /// The saved Wi-Fi profile at a settings path, if it has been read.
    fn profile_at(&self, path: &str) -> Option<Profile> {
        self.profiles.get(path).cloned().flatten()
    }

    /// Whether to ask about `ssid` now rather than leave it to another agent.
    fn may_ask(&self, ssid: &str, visible: bool, now: Instant) -> bool {
        visible
            || self.initiated.as_ref().is_some_and(|(s, at)| {
                s == ssid && now.saturating_duration_since(*at) < Duration::from_secs(180)
            })
    }

    /// The agent asks: show the prompt if we should, or say there's nothing here, so another agent
    /// (a desktop's own) can answer instead.
    fn agent_asks(
        &mut self,
        ask: AgentAsk,
        visible: bool,
        now: Instant,
    ) -> (AgentReply, Vec<Effect>) {
        let profile = self.profile_at(&ask.path);
        let of_profile = || {
            profile
                .as_ref()
                .map(|p| p.ssid.clone())
                .unwrap_or_else(|| ask.connection_id.clone())
        };
        let (ssid, kind, identity) = match &ask.asks {
            Asks::Psk { ssid } => {
                let wep = profile.as_ref().is_some_and(|p| p.key_mgmt == "none");
                (
                    if ssid.is_empty() {
                        of_profile()
                    } else {
                        ssid.clone()
                    },
                    if wep {
                        PromptKind::Wep
                    } else {
                        PromptKind::Password
                    },
                    String::new(),
                )
            }
            Asks::Eap { identity } => (
                of_profile(),
                PromptKind::Enterprise,
                identity.clone().unwrap_or_default(),
            ),
            Asks::Other => return (AgentReply::NoSecrets, Vec::new()),
        };
        if !ask.interactive || !self.may_ask(&ssid, visible, now) {
            return (AgentReply::NoSecrets, Vec::new());
        }
        let (reply, answer) = oneshot::channel();
        let effects = self.ask(Prompt {
            ssid,
            kind,
            retry: ask.request_new,
            identity,
            ..Default::default()
        });
        self.reply = Some((ask.path, reply));
        self.agent_asked = true;
        // While connecting, the prompt says what's going on.
        if !matches!(self.activity, Activity::Connecting(_)) {
            self.activity = Activity::Idle;
        }
        (AgentReply::Wait(kind, answer), effects)
    }
}

/// The saved profiles, kept for a while by the task that looks at NetworkManager: a look happens
/// for every signal of it, and reading their settings takes a call each.
#[derive(Default)]
struct ProfileCache {
    map: HashMap<String, Option<Profile>>,
    since: Option<Instant>,
}

impl ProfileCache {
    /// What to read for `paths`, after forgetting what is old or gone.
    fn missing(&mut self, paths: &[String], now: Instant) -> Vec<String> {
        if self
            .since
            .is_none_or(|t| now.saturating_duration_since(t) > PROFILES_FOR)
        {
            self.map.clear();
            self.since = Some(now);
        }
        self.map.retain(|p, _| paths.contains(p));
        paths
            .iter()
            .filter(|p| !self.map.contains_key(*p))
            .cloned()
            .collect()
    }

    /// Read everything again next time.
    fn forget(&mut self) {
        self.since = None;
    }
}

async fn look(
    ctx: &Ctx,
    cache: &mut ProfileCache,
    forgotten: &mut watch::Receiver<u64>,
) -> Result<Looked, Error> {
    let conn = ctx.bus(true).await?;
    let snap = nm::snapshot(&conn).await?;
    if forgotten.has_changed().unwrap_or(false) {
        forgotten.mark_unchanged();
        cache.forget();
    }
    for path in cache.missing(&snap.connections, Instant::now()) {
        let profile = match nm::settings(&conn, &path).await {
            Ok(s) => nm::profile(&path, &s),
            Err(e) => {
                log::debug!("wifi: settings of {path}: {e}");
                None
            }
        };
        cache.map.insert(path, profile);
    }
    Ok(Looked {
        snap,
        profiles: cache.map.clone(),
    })
}

pub struct Wifi {
    ctx: Ctx,
    mailbox: Mailbox<Inner>,
    core: Core,
    /// nmrs's client, for changing saved profiles and the radio; made on first use.
    nmrs: Connector<nmrs::NetworkManager, nmrs::ConnectionError>,
    /// Bumped when the saved profiles changed, before the refresh that shows it.
    forgotten: watch::Sender<u64>,
}

impl Wifi {
    pub fn start(ctx: Ctx) -> Self {
        let mut me = Self {
            ctx: ctx.clone(),
            mailbox: Default::default(),
            core: Core::default(),
            nmrs: Connector::new(ctx.runtime(), nmrs::NetworkManager::new),
            forgotten: watch::Sender::new(0),
        };
        let trigger = Trigger::default();
        ctx.on_dbus_signals(
            true,
            signals_from(nm::NM),
            Duration::from_millis(500),
            trigger.clone(),
        );
        let mut ticks = ctx.ticks(trigger);
        let mut forgotten = me.forgotten.subscribe();
        let (c, found) = (ctx.clone(), me.mailbox.sender());
        me.mailbox.spawn(&ctx, async move {
            let mut cache = ProfileCache::default();
            ticks.ready().await;
            loop {
                let looked = look(&c, &mut cache, &mut forgotten).await;
                let _ = found.send(Found::Fetched(looked.map(Box::new)).into());
                ticks.next(SCAN_EVERY).await;
            }
        });
        let found = me.mailbox.sender();
        me.mailbox.spawn(&ctx, agent::serve_forever(found));
        // A shown password doesn't outlive the dashboard being hidden.
        let (c, found) = (ctx.clone(), me.mailbox.sender());
        me.mailbox.spawn(&ctx, async move {
            loop {
                c.until_visible(true).await;
                c.until_visible(false).await;
                let _ = found.send(Found::Hidden.into());
            }
        });
        me
    }

    fn apply(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            self.run(effect);
        }
    }

    fn run(&mut self, effect: Effect) {
        let found = self.mailbox.sender();
        let ctx = self.ctx.clone();
        let client = self.nmrs.clone();
        let forgotten = self.forgotten.clone();
        let forget = move || forgotten.send_modify(|n| *n = n.wrapping_add(1));
        match effect {
            Effect::Publish(state) => self.ctx.send(crate::wifi::Event::State(state)),
            Effect::Toast(text) => self.ctx.toast(text),
            Effect::Refresh => self.ctx.refresh(),
            Effect::SetEnabled(on) => {
                self.ctx.act("Wi-Fi", async move {
                    client.get().await?.set_wireless_enabled(on).await?;
                    Ok::<_, Error>(())
                });
            }
            Effect::RequestScan { device } => {
                self.mailbox.spawn(&self.ctx, async move {
                    let Ok(conn) = ctx.bus(true).await else {
                        return;
                    };
                    let options: HashMap<&str, zbus::zvariant::Value> = HashMap::new();
                    // Refused while a scan runs or right after one; either way a fresh list is
                    // coming.
                    if let Err(e) =
                        nm::call(&conn, &device, nm::WIRELESS, "RequestScan", &(options,)).await
                    {
                        log::debug!("wifi: scan: {e}");
                    }
                });
            }
            Effect::CheckConnectivity => {
                self.mailbox.spawn(&self.ctx, async move {
                    if let Ok(conn) = ctx.bus(true).await {
                        let _ =
                            nm::call(&conn, nm::NM_PATH, nm::NM, "CheckConnectivity", &()).await;
                    }
                });
            }
            Effect::Activate { ssid, device, how } => {
                self.mailbox.spawn(&self.ctx, async move {
                    let ended = run_activation(&ctx, &ssid, &device, how, forget)
                        .await
                        .unwrap_or_else(|e| Ended::Error(e.to_string()));
                    let _ = found.send(Found::ActivationEnded { ssid, ended }.into());
                });
            }
            Effect::StoreSecret {
                ssid,
                profile,
                kind,
                answer,
            } => {
                self.mailbox.spawn(&self.ctx, async move {
                    let message = match store_secret(&ctx, &client, &profile, kind, &answer).await {
                        Ok(()) => {
                            forget();
                            Found::SecretStored {
                                ssid,
                                path: profile.path.clone(),
                            }
                        }
                        Err(e) => Found::Failed(e.to_string()),
                    };
                    let _ = found.send(message.into());
                });
            }
            Effect::Disconnect { device } => {
                self.ctx.act("Disconnect", async move {
                    let conn = ctx.bus(true).await?;
                    nm::call(&conn, &device, nm::DEVICE, "Disconnect", &())
                        .await
                        .map(|_| ())
                });
            }
            Effect::Forget { uuid } => {
                self.ctx.act("Forget network", async move {
                    let r = client.get().await?.delete_saved_connection(&uuid).await;
                    forget();
                    Ok::<_, Error>(r?)
                });
            }
            Effect::SetAutoconnect { uuid, on } => {
                self.ctx.act("Auto-connect", async move {
                    let mut patch = nmrs::SettingsPatch::default();
                    patch.autoconnect = Some(on);
                    let r = client
                        .get()
                        .await?
                        .update_saved_connection(&uuid, patch)
                        .await;
                    forget();
                    Ok::<_, Error>(r?)
                });
            }
            Effect::Reveal {
                profile,
                generation,
            } => {
                self.ctx.act("Show password", async move {
                    let secret = read_secret(&ctx, &profile).await?;
                    let _ = found.send(Found::Revealed { generation, secret }.into());
                    Ok::<_, Error>(())
                });
            }
            Effect::SignIn { uri } => {
                if let Err(e) = super::spawn_detached("xdg-open", &[&uri]) {
                    self.ctx.toast(format!("Sign in: {e}"));
                }
            }
            Effect::EndFailure(why) => {
                self.mailbox.spawn(&self.ctx, async move {
                    tokio::time::sleep(Duration::from_secs(8)).await;
                    let _ = found.send(Found::FailureOver(why).into());
                });
            }
        }
    }

    /// The agent asks. Its answer is awaited in a task of ours, so it goes on while the agent
    /// registers again: a dropped request would tell NetworkManager there are no secrets.
    fn agent_request(&mut self, request: SecretRequest) {
        let ask = AgentAsk {
            path: request.connection_path.to_string(),
            connection_id: request.connection_id.clone(),
            asks: match &request.setting {
                SecretSetting::WifiPsk { ssid } => Asks::Psk { ssid: ssid.clone() },
                SecretSetting::WifiEap { identity, .. } => Asks::Eap {
                    identity: identity.clone(),
                },
                _ => Asks::Other,
            },
            interactive: request.flags.contains(SecretAgentFlags::ALLOW_INTERACTION),
            request_new: request.flags.contains(SecretAgentFlags::REQUEST_NEW),
        };
        let path = ask.path.clone();
        let (reply, effects) = self
            .core
            .agent_asks(ask, self.ctx.visible(), Instant::now());
        self.apply(effects);
        let found = self.mailbox.sender();
        self.mailbox
            .spawn(&self.ctx, agent::respond(request, reply, path, found));
    }
}

impl Stateful for Wifi {
    type Command = crate::wifi::Command;
    type Inner = Inner;

    fn mailbox(&mut self) -> &mut Mailbox<Inner> {
        &mut self.mailbox
    }

    fn on_command(&mut self, cmd: crate::wifi::Command) {
        let effects = self.core.on_command(cmd, Instant::now());
        self.apply(effects);
    }

    fn on_inner(&mut self, inner: Inner) {
        match inner {
            Inner::Core(found) => {
                let effects = self.core.on_found(found, Instant::now());
                self.apply(effects);
            }
            Inner::Agent(request) => self.agent_request(*request),
        }
    }
}

/// Start an activation and watch it to its end.
async fn run_activation(
    ctx: &Ctx,
    ssid: &str,
    dev: &str,
    how: Activation,
    forget: impl Fn(),
) -> Result<Ended, Error> {
    let conn = ctx.bus(true).await?;
    if dev.is_empty() {
        return Err(Error::Other("no Wi-Fi device".into()));
    }
    // Listen before activating, so a quick failure isn't missed.
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(nm::NM)?
        .path(dev)?
        .interface(nm::DEVICE)?
        .member("StateChanged")?
        .build();
    let mut changes = zbus::MessageStream::for_match_rule(rule, &conn, Some(64)).await?;
    let mut activated = None;
    let (created, retry_with) = match how {
        Activation::Profile(path) => {
            activated = Some(path.clone());
            nm::call(
                &conn,
                nm::NM_PATH,
                nm::NM,
                "ActivateConnection",
                &(nm::path(&path)?, nm::path(dev)?, nm::path("/")?),
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
                &(settings, nm::path(dev)?, nm::path(&ap)?),
            )
            .await?;
            let (path, _active): (
                zbus::zvariant::OwnedObjectPath,
                zbus::zvariant::OwnedObjectPath,
            ) = reply.body().deserialize()?;
            forget();
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
        Ok(Ok(())) => return Ok(Ended::Activated),
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
        forget();
    }
    Ok(Ended::Failed {
        reason,
        activated,
        retry_with,
    })
}

/// Put a password into a saved profile whose password NetworkManager keeps itself.
/// NetworkManager keeps the profile's other secrets. A password kept elsewhere (a keyring,
/// through another agent) or never stored stays that way: it isn't moved into
/// NetworkManager's files.
async fn store_secret(
    ctx: &Ctx,
    client: &Connector<nmrs::NetworkManager, nmrs::ConnectionError>,
    profile: &Profile,
    kind: PromptKind,
    answer: &Answer,
) -> Result<(), Error> {
    let conn = ctx.bus(true).await?;
    let settings = nm::settings(&conn, &profile.path).await?;
    let (setting, flags_key) = match kind {
        PromptKind::Password => ("802-11-wireless-security", "psk-flags"),
        PromptKind::Wep => ("802-11-wireless-security", "wep-key-flags"),
        PromptKind::Enterprise => ("802-1x", "password-flags"),
    };
    let flags: u32 = settings
        .get(setting)
        .and_then(|v| super::prop(v, flags_key))
        .unwrap_or(0);
    if flags != 0 {
        return Err(Error::Other(
            "its password is kept outside NetworkManager (a keyring); change it there".into(),
        ));
    }
    let mut patch = nmrs::SettingsPatch::default();
    patch.raw_overlay = Some(secret_overlay(kind, answer));
    client
        .get()
        .await?
        .update_saved_connection(&profile.uuid, patch)
        .await?;
    Ok(())
}

/// A saved network's password and its QR code.
async fn read_secret(ctx: &Ctx, profile: &Profile) -> Result<Secret, Error> {
    let security = profile.security();
    let password = if security.needs_secret() {
        let conn = ctx.bus(true).await?;
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
    Ok(Secret {
        uuid: profile.uuid.clone(),
        password,
        qr: qr_image(&text),
    })
}

/// Why `answer` can't work for `key_mgmt`, if it can't.
/// `new`: for a connection still to be made (an Enterprise one then needs the server's domain).
fn check_answer(key_mgmt: &str, new: bool, answer: &Answer) -> Option<String> {
    let pw = &answer.password;
    match key_mgmt {
        "wpa-psk" | "sae" => {
            let hex_key = pw.len() == 64 && pw.chars().all(|c| c.is_ascii_hexdigit());
            (!(8..=63).contains(&pw.chars().count()) && !hex_key)
                .then(|| "A WPA password has 8 to 63 characters.".into())
        }
        "wpa-eap" if answer.identity.trim().is_empty() => Some("Enter a user name.".into()),
        "wpa-eap" if new && !answer.insecure && answer.domain.trim().is_empty() => Some(
            "Enter the server's domain (the network's admins know it), or turn off the \
             certificate check."
                .into(),
        ),
        "none" | "wpa-eap" if pw.is_empty() => Some("Enter the password.".into()),
        _ => None,
    }
}

/// The settings that put `answer` into a profile (whose secrets NetworkManager keeps).
fn secret_overlay(kind: PromptKind, answer: &Answer) -> nm::Settings {
    let v = |v: zbus::zvariant::Value<'_>| {
        zbus::zvariant::OwnedValue::try_from(v).expect("plain values have no fds")
    };
    let (setting, values) = match kind {
        PromptKind::Password => (
            "802-11-wireless-security",
            vec![("psk", v(answer.password.as_str().into()))],
        ),
        PromptKind::Wep => (
            "802-11-wireless-security",
            vec![("wep-key0", v(answer.password.as_str().into()))],
        ),
        PromptKind::Enterprise => (
            "802-1x",
            vec![
                ("identity", v(answer.identity.as_str().into())),
                ("password", v(answer.password.as_str().into())),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IconSrc;
    use crate::live::Actor;
    use crate::wifi::Command;

    const DEV: &str = "/org/freedesktop/NetworkManager/Devices/3";

    fn profile(path: &str, ssid: &str, uuid: &str) -> Profile {
        Profile {
            path: path.into(),
            ssid: ssid.into(),
            uuid: uuid.into(),
            key_mgmt: "wpa-psk".into(),
            ..Default::default()
        }
    }

    /// A look at NetworkManager with a device and these profiles.
    fn look_with(profiles: Vec<Profile>) -> Looked {
        Looked {
            snap: nm::Snapshot {
                enabled: true,
                hw_enabled: true,
                device: Some((DEV.into(), "wlan0".into(), 100)),
                connections: profiles.iter().map(|p| p.path.clone()).collect(),
                ..Default::default()
            },
            profiles: profiles
                .into_iter()
                .map(|p| (p.path.clone(), Some(p)))
                .collect(),
        }
    }

    /// A core that has seen a device with a profile for "Home".
    fn core() -> Core {
        let mut c = Core::default();
        c.on_found(
            Found::Fetched(Ok(Box::new(look_with(vec![profile("/p/1", "Home", "u1")])))),
            Instant::now(),
        );
        c
    }

    fn password(p: &str) -> Answer {
        Answer {
            password: p.into(),
            ..Default::default()
        }
    }

    fn ask_for(ssid: &str) -> AgentAsk {
        AgentAsk {
            path: "/p/1".into(),
            connection_id: "Home".into(),
            asks: Asks::Psk { ssid: ssid.into() },
            interactive: true,
            request_new: false,
        }
    }

    fn secret(uuid: &str) -> Secret {
        Secret {
            uuid: uuid.into(),
            password: "hunter2".into(),
            qr: IconSrc::None,
        }
    }

    fn select_home(c: &mut Core, now: Instant) {
        let key = c.last.saved[0].key();
        c.on_command(Command::Select { key }, now);
        c.on_found(
            Found::Fetched(Ok(Box::new(look_with(vec![profile("/p/1", "Home", "u1")])))),
            now,
        );
    }

    // What the agent gets to ask.

    #[test]
    fn the_agent_asks_while_shown() {
        let mut c = core();
        let (reply, _) = c.agent_asks(ask_for("Home"), true, Instant::now());
        assert!(matches!(reply, AgentReply::Wait(PromptKind::Password, _)));
        assert_eq!(c.prompt.as_ref().unwrap().ssid, "Home");
        assert!(c.agent_asked);
    }

    #[test]
    fn hidden_the_agent_leaves_it_to_another_agent() {
        let mut c = core();
        let (reply, effects) = c.agent_asks(ask_for("Home"), false, Instant::now());
        assert!(matches!(reply, AgentReply::NoSecrets));
        assert!(effects.is_empty());
        assert!(c.prompt.is_none());
    }

    /// Hidden, but this network was joined from the widget just now.
    #[test]
    fn hidden_the_agent_asks_for_a_network_joined_from_here() {
        let now = Instant::now();
        let mut c = core();
        let key = c.last.saved[0].key();
        c.on_command(Command::Connect { key }, now);
        let (soon, _) = c.agent_asks(ask_for("Home"), false, now + Duration::from_secs(10));
        assert!(matches!(soon, AgentReply::Wait(..)));
        let mut c = core();
        let key = c.last.saved[0].key();
        c.on_command(Command::Connect { key }, now);
        let (late, _) = c.agent_asks(ask_for("Home"), false, now + Duration::from_secs(181));
        assert!(matches!(late, AgentReply::NoSecrets));
        // Another network doesn't count.
        let (other, _) = c.agent_asks(ask_for("Other"), false, now);
        assert!(matches!(other, AgentReply::NoSecrets));
    }

    #[test]
    fn the_agent_doesnt_bother_the_user_when_told_not_to() {
        let mut c = core();
        let ask = AgentAsk {
            interactive: false,
            ..ask_for("Home")
        };
        assert!(matches!(
            c.agent_asks(ask, true, Instant::now()).0,
            AgentReply::NoSecrets
        ));
    }

    #[test]
    fn the_agent_only_knows_wifi() {
        let mut c = core();
        let ask = AgentAsk {
            asks: Asks::Other,
            ..ask_for("Home")
        };
        assert!(matches!(
            c.agent_asks(ask, true, Instant::now()).0,
            AgentReply::NoSecrets
        ));
    }

    #[test]
    fn what_the_agent_asks_comes_from_the_profile() {
        let now = Instant::now();
        let mut c = Core::default();
        let wep = Profile {
            key_mgmt: "none".into(),
            ..profile("/p/1", "Old", "u1")
        };
        c.on_found(Found::Fetched(Ok(Box::new(look_with(vec![wep])))), now);
        // No SSID in the request: the profile knows it. Its security says WEP.
        let (reply, _) = c.agent_asks(ask_for(""), true, now);
        assert!(matches!(reply, AgentReply::Wait(PromptKind::Wep, _)));
        assert_eq!(c.prompt.as_ref().unwrap().ssid, "Old");
        let eap = AgentAsk {
            asks: Asks::Eap {
                identity: Some("me".into()),
            },
            ..ask_for("")
        };
        let (reply, _) = c.agent_asks(eap, true, now);
        assert!(matches!(reply, AgentReply::Wait(PromptKind::Enterprise, _)));
        assert_eq!(c.prompt.as_ref().unwrap().identity, "me");
    }

    /// NetworkManager says the old password failed: the prompt says so.
    #[test]
    fn a_request_for_a_new_password_says_the_last_failed() {
        let mut c = core();
        let ask = AgentAsk {
            request_new: true,
            ..ask_for("Home")
        };
        c.agent_asks(ask, true, Instant::now());
        assert!(c.prompt.as_ref().unwrap().retry);
    }

    // Prompts and their answers.

    fn wait(reply: AgentReply) -> oneshot::Receiver<Option<Answer>> {
        match reply {
            AgentReply::Wait(_, answer) => answer,
            AgentReply::NoSecrets => panic!("should wait"),
        }
    }

    #[test]
    fn a_prompt_replaced_tells_the_old_request_no() {
        let now = Instant::now();
        let mut c = core();
        let mut first = wait(c.agent_asks(ask_for("Home"), true, now).0);
        let old_serial = c.prompt.as_ref().unwrap().serial;
        let mut second = wait(c.agent_asks(ask_for("Home"), true, now).0);
        assert_eq!(first.try_recv(), Ok(None));
        let serial = c.prompt.as_ref().unwrap().serial;
        assert_ne!(serial, old_serial);
        // An answer to the old one goes nowhere: it can't reach another network.
        let late = c.on_command(
            Command::Answer {
                serial: old_serial,
                answer: password("password1"),
            },
            now,
        );
        assert!(late.is_empty());
        assert!(second.try_recv().is_err());
        c.on_command(
            Command::Answer {
                serial,
                answer: password("password1"),
            },
            now,
        );
        assert_eq!(second.try_recv(), Ok(Some(password("password1"))));
        assert!(c.prompt.is_none());
    }

    #[test]
    fn a_password_too_short_asks_again_and_says_why() {
        let now = Instant::now();
        let mut c = core();
        let mut waiting = wait(c.agent_asks(ask_for("Home"), true, now).0);
        let serial = c.prompt.as_ref().unwrap().serial;
        c.on_command(
            Command::Answer {
                serial,
                answer: password("short"),
            },
            now,
        );
        assert!(waiting.try_recv().is_err(), "still waiting");
        assert!(c.prompt.as_ref().unwrap().message.contains("8 to 63"));
        assert_eq!(c.prompt.as_ref().unwrap().serial, serial);
    }

    #[test]
    fn cancelling_tells_the_request_and_remembers() {
        let now = Instant::now();
        let mut c = core();
        let mut waiting = wait(c.agent_asks(ask_for("Home"), true, now).0);
        c.on_command(Command::CancelPrompt, now);
        assert_eq!(waiting.try_recv(), Ok(None));
        assert!(c.prompt.is_none());
        assert!(c.cancelled);
    }

    #[test]
    fn a_request_gone_takes_only_its_own_prompt() {
        let now = Instant::now();
        let mut c = core();
        let _waiting = wait(c.agent_asks(ask_for("Home"), true, now).0);
        c.on_found(Found::AgentGone("/p/other".into()), now);
        assert!(c.prompt.is_some());
        c.on_found(Found::AgentGone("/p/1".into()), now);
        assert!(c.prompt.is_none());
    }

    #[test]
    fn a_prompt_for_a_saved_profile_stores_the_password() {
        let now = Instant::now();
        let mut c = core();
        c.ask(Prompt {
            ssid: "Home".into(),
            profile: Some(profile("/p/1", "Home", "u1")),
            ..Default::default()
        });
        let serial = c.prompt.as_ref().unwrap().serial;
        let effects = c.on_command(
            Command::Answer {
                serial,
                answer: password("password1"),
            },
            now,
        );
        assert!(matches!(&effects[..], [Effect::StoreSecret { .. }]));
        // Stored: join with it.
        let effects = c.on_found(
            Found::SecretStored {
                ssid: "Home".into(),
                path: "/p/1".into(),
            },
            now,
        );
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::Activate { how: Activation::Profile(p), .. } if p == "/p/1"
        )));
    }

    // Joining.

    #[test]
    fn a_saved_network_activates_its_profile() {
        let now = Instant::now();
        let mut c = core();
        let key = c.last.saved[0].key();
        let effects = c.on_command(Command::Connect { key }, now);
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::Activate { how: Activation::Profile(p), device, .. }
                if p == "/p/1" && device == DEV
        )));
        assert_eq!(c.activity, Activity::Connecting("Home".into()));
    }

    #[test]
    fn a_network_nobody_knows_does_nothing() {
        let mut c = core();
        let effects = c.on_command(
            Command::Connect {
                key: "pNowhere".into(),
            },
            Instant::now(),
        );
        assert!(effects.is_empty());
    }

    #[test]
    fn a_hidden_network_needs_a_name_and_a_good_password() {
        let now = Instant::now();
        let mut c = core();
        let join = |ssid: &str, answer| Command::JoinHidden {
            ssid: ssid.into(),
            security: 1,
            answer,
        };
        assert!(
            c.on_command(join("  ", password("password1")), now)
                .is_empty()
        );
        c.on_command(join("Attic", password("short")), now);
        assert!(matches!(c.activity, Activity::Failed(_)));
        let effects = c.on_command(join(" Attic ", password("password1")), now);
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::Activate { ssid, how: Activation::New { hidden: true, .. }, .. }
                if ssid == "Attic"
        )));
    }

    fn failed(reason: u32) -> Ended {
        Ended::Failed {
            reason,
            activated: Some("/p/1".into()),
            retry_with: None,
        }
    }

    fn end(c: &mut Core, ended: Ended) -> Vec<Effect> {
        c.on_found(
            Found::ActivationEnded {
                ssid: "Home".into(),
                ended,
            },
            Instant::now(),
        )
    }

    #[test]
    fn a_wrong_password_nobody_asked_about_asks_here() {
        let mut c = core();
        end(&mut c, failed(7));
        let prompt = c.prompt.as_ref().expect("asks");
        assert!(prompt.retry);
        assert_eq!(prompt.profile.as_ref().unwrap().path, "/p/1");
        assert!(!prompt.new);
        assert_eq!(c.activity, Activity::Idle);
    }

    /// The agent already asked, and that didn't work: asking again here wouldn't help.
    #[test]
    fn a_wrong_password_the_agent_asked_about_is_a_failure() {
        let now = Instant::now();
        let mut c = core();
        let _waiting = c.agent_asks(ask_for("Home"), true, now);
        c.prompt = None;
        end(&mut c, failed(7));
        assert!(c.prompt.is_none());
        assert!(matches!(c.activity, Activity::Failed(_)));
    }

    /// Replaced by another activation, or stopped by the user: no failure to show.
    #[test]
    fn what_was_meant_is_no_failure() {
        for reason in [39, 60] {
            let mut c = core();
            end(&mut c, failed(reason));
            assert_eq!(c.activity, Activity::Idle);
            assert!(c.prompt.is_none());
        }
        let mut c = core();
        c.on_command(Command::CancelPrompt, Instant::now());
        end(&mut c, failed(7));
        assert_eq!(c.activity, Activity::Idle);
        assert!(c.prompt.is_none(), "cancelled: not asked again");
    }

    #[test]
    fn other_failures_say_why_for_a_while() {
        let mut c = core();
        let effects = end(&mut c, failed(53));
        let Activity::Failed(why) = c.activity.clone() else {
            panic!("should fail");
        };
        assert!(effects.contains(&Effect::EndFailure(why.clone())));
        c.on_found(Found::FailureOver(why), Instant::now());
        assert_eq!(c.activity, Activity::Idle);
    }

    #[test]
    fn a_failure_doesnt_end_what_happened_since() {
        let mut c = core();
        c.on_found(Found::Failed("no".into()), Instant::now());
        c.activity = Activity::Connecting("Home".into());
        assert!(
            c.on_found(Found::FailureOver("no".into()), Instant::now())
                .is_empty()
        );
        assert_eq!(c.activity, Activity::Connecting("Home".into()));
    }

    #[test]
    fn a_new_connection_that_failed_asks_with_how_it_was_made() {
        let mut c = core();
        end(
            &mut c,
            Ended::Failed {
                reason: 8,
                activated: None,
                retry_with: Some(("wpa-psk".into(), "/ap/1".into(), false)),
            },
        );
        let prompt = c.prompt.as_ref().expect("asks");
        assert!(prompt.new);
        assert_eq!(prompt.key_mgmt, "wpa-psk");
        assert_eq!(prompt.ap, "/ap/1");
    }

    /// Joining again starts from a clean slate.
    #[test]
    fn a_new_activation_forgets_the_last() {
        let now = Instant::now();
        let mut c = core();
        c.cancelled = true;
        c.agent_asked = true;
        let key = c.last.saved[0].key();
        c.on_command(Command::Connect { key }, now);
        assert!(!c.cancelled);
        assert!(!c.agent_asked);
    }

    // Passwords on show.

    #[test]
    fn a_late_reveal_is_dropped() {
        let now = Instant::now();
        let mut c = core();
        select_home(&mut c, now);
        let generation = c.secret_gen;
        c.on_command(Command::HideSecret, now);
        c.on_found(
            Found::Revealed {
                generation,
                secret: secret("u1"),
            },
            now,
        );
        assert!(c.secret.is_none());
        // One for this generation shows.
        c.on_found(
            Found::Revealed {
                generation: c.secret_gen,
                secret: secret("u1"),
            },
            now,
        );
        assert!(c.secret.is_some());
    }

    #[test]
    fn a_password_goes_with_its_page() {
        let now = Instant::now();
        let mut c = core();
        select_home(&mut c, now);
        c.secret = Some(secret("u1"));
        // Leaving the page.
        c.on_command(Command::Select { key: String::new() }, now);
        assert!(c.secret.is_none());
        // Its profile gone from the page by the next look.
        select_home(&mut c, now);
        c.secret = Some(secret("somewhere-else"));
        c.on_found(
            Found::Fetched(Ok(Box::new(look_with(vec![profile("/p/1", "Home", "u1")])))),
            now,
        );
        assert!(c.secret.is_none());
        assert!(c.last.secret.is_none());
    }

    /// Hidden: the password goes, what the widget has is replaced straight away, and nothing keeps
    /// a copy, not even what was last sent.
    #[test]
    fn hiding_takes_the_password_away_everywhere() {
        let now = Instant::now();
        let mut c = core();
        select_home(&mut c, now);
        c.secret = Some(secret("u1"));
        c.on_found(
            Found::Fetched(Ok(Box::new(look_with(vec![profile("/p/1", "Home", "u1")])))),
            now,
        );
        assert!(c.last.secret.is_some());
        let effects = c.on_found(Found::Hidden, now);
        assert!(c.secret.is_none());
        assert!(c.last.secret.is_none());
        let published = effects.iter().find_map(|e| match e {
            Effect::Publish(s) => Some(s),
            _ => None,
        });
        assert!(published.expect("the widget hears").secret.is_none());
        // What was sent no longer has it: the same state isn't sent again.
        let same = c.last.clone();
        assert!(!c.sent.changed(&same));
    }

    /// A look under way when the dashboard is hidden can't bring the password back: what it adds
    /// is what's known when it arrives.
    #[test]
    fn a_look_arriving_after_hiding_cant_bring_a_password_back() {
        let now = Instant::now();
        let mut c = core();
        select_home(&mut c, now);
        c.secret = Some(secret("u1"));
        c.on_found(Found::Hidden, now);
        c.on_found(
            Found::Fetched(Ok(Box::new(look_with(vec![profile("/p/1", "Home", "u1")])))),
            now,
        );
        assert!(c.last.secret.is_none());
    }

    #[test]
    fn reveal_needs_a_saved_network() {
        let now = Instant::now();
        let mut c = core();
        let effects = c.on_command(
            Command::Reveal {
                uuid: "nope".into(),
            },
            now,
        );
        assert!(effects.is_empty());
        let effects = c.on_command(Command::Reveal { uuid: "u1".into() }, now);
        assert!(matches!(&effects[..], [Effect::Reveal { .. }]));
    }

    // Looking at NetworkManager.

    #[test]
    fn a_scan_is_asked_for_now_and_then() {
        let now = Instant::now();
        let mut c = Core::default();
        let look = |c: &mut Core, at| {
            c.on_found(Found::Fetched(Ok(Box::new(look_with(vec![])))), at)
                .contains(&Effect::RequestScan { device: DEV.into() })
        };
        assert!(look(&mut c, now));
        assert!(!look(&mut c, now + Duration::from_secs(5)));
        assert!(look(&mut c, now + SCAN_EVERY));
        // Rescan: right away.
        c.on_command(Command::Rescan, now + SCAN_EVERY);
        assert!(look(&mut c, now + SCAN_EVERY));
    }

    #[test]
    fn it_scans_until_the_list_is_new() {
        let now = Instant::now();
        let mut c = core();
        c.on_command(Command::Rescan, now);
        c.on_found(Found::Fetched(Ok(Box::new(look_with(vec![])))), now);
        assert!(c.last.scanning);
        let mut newer = look_with(vec![]);
        newer.snap.last_scan = 7;
        c.on_found(
            Found::Fetched(Ok(Box::new(newer))),
            now + Duration::from_secs(1),
        );
        assert!(!c.last.scanning);
    }

    /// Wi-Fi off, nothing asked of it.
    #[test]
    fn no_scan_while_off() {
        let mut c = Core::default();
        let mut off = look_with(vec![]);
        off.snap.enabled = false;
        let effects = c.on_found(Found::Fetched(Ok(Box::new(off))), Instant::now());
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::RequestScan { .. }))
        );
    }

    #[test]
    fn no_device_says_so_and_nothing_else() {
        let mut c = core();
        c.activity = Activity::Connecting("Home".into());
        let gone = Looked::default();
        c.on_found(Found::Fetched(Ok(Box::new(gone))), Instant::now());
        assert!(!c.last.available);
        assert_eq!(c.last.activity, Activity::Idle);
    }

    /// A look that failed changes nothing on show.
    #[test]
    fn a_failed_look_keeps_what_was_there() {
        let mut c = core();
        let effects = c.on_found(
            Found::Fetched(Err(Error::Other("gone".into()))),
            Instant::now(),
        );
        assert!(effects.is_empty());
        assert!(c.last.available);
        assert_eq!(c.last.saved.len(), 1);
    }

    #[test]
    fn the_same_look_again_isnt_sent_again() {
        let now = Instant::now();
        let mut c = Core::default();
        let publishes = |e: &[Effect]| e.iter().any(|e| matches!(e, Effect::Publish(_)));
        let first = c.on_found(Found::Fetched(Ok(Box::new(look_with(vec![])))), now);
        assert!(publishes(&first));
        let again = c.on_found(Found::Fetched(Ok(Box::new(look_with(vec![])))), now);
        assert!(!publishes(&again));
    }

    /// Hotspot profiles aren't in the lists, but the agent has to know them.
    #[test]
    fn a_hotspot_is_known_though_not_listed() {
        let mut c = Core::default();
        let hotspot = Profile {
            hotspot: true,
            ..profile("/p/9", "Mine", "u9")
        };
        c.on_found(
            Found::Fetched(Ok(Box::new(look_with(vec![hotspot])))),
            Instant::now(),
        );
        assert!(c.last.saved.is_empty());
        assert!(c.profile_at("/p/9").is_some());
    }

    #[test]
    fn profiles_are_read_again_after_a_while_and_when_told() {
        let now = Instant::now();
        let paths = vec!["/p/1".to_string(), "/p/2".to_string()];
        let mut cache = ProfileCache::default();
        assert_eq!(cache.missing(&paths, now), paths);
        cache.map.insert("/p/1".into(), None);
        cache.map.insert("/p/2".into(), None);
        assert!(cache.missing(&paths, now).is_empty());
        // One gone: forgotten; none to read.
        assert!(cache.missing(&paths[..1], now).is_empty());
        assert_eq!(cache.map.len(), 1);
        // Old.
        assert_eq!(cache.missing(&paths[..1], now + PROFILES_FOR * 2).len(), 1);
        // Told.
        cache.map.insert("/p/1".into(), None);
        assert!(
            cache
                .missing(&paths[..1], now + PROFILES_FOR * 2)
                .is_empty()
        );
        cache.forget();
        assert_eq!(cache.missing(&paths[..1], now + PROFILES_FOR * 2).len(), 1);
    }

    /// The blog's way: queue what happens, drop the sender, await the loop, look at what's left.
    #[tokio::test]
    async fn the_actor_handles_what_it_finds_before_commands() {
        let (ctx, events) = crate::sources::testing::ctx();
        let me = Wifi::start(ctx);
        let (commands, inbox) = tokio::sync::mpsc::unbounded_channel();
        // Queued first, but it needs the profile found below.
        let key = profile("/p/1", "Home", "u1").key();
        let _ = commands.send(Command::Connect { key });
        let _ = me.mailbox.sender().send(
            Found::Fetched(Ok(Box::new(look_with(vec![profile("/p/1", "Home", "u1")])))).into(),
        );
        drop(commands);
        let me = me.event_loop(inbox).await;
        assert_eq!(me.core.activity, Activity::Connecting("Home".into()));
        assert_eq!(crate::sources::testing::reported(events).len(), 1);
    }

    #[test]
    fn answers() {
        let pw = |p: &str| Answer {
            password: p.into(),
            ..Default::default()
        };
        assert!(check_answer("wpa-psk", true, &pw("short")).is_some());
        assert!(check_answer("wpa-psk", true, &pw("long enough")).is_none());
        assert!(check_answer("sae", true, &pw(&"z".repeat(64))).is_some());
        assert!(check_answer("wpa-psk", true, &pw(&"ab".repeat(32))).is_none());
        assert!(check_answer("wpa-eap", true, &pw("secret")).is_some());
        assert!(check_answer("", true, &pw("")).is_none());
        // A new Enterprise network needs the server's domain, or an explicit "don't check".
        let eap = Answer {
            identity: "me".into(),
            password: "secret".into(),
            ..Default::default()
        };
        assert!(check_answer("wpa-eap", true, &eap).is_some());
        assert!(check_answer("wpa-eap", false, &eap).is_none());
        let with_domain = Answer {
            domain: "example.org".into(),
            ..eap.clone()
        };
        assert!(check_answer("wpa-eap", true, &with_domain).is_none());
        let insecure = Answer {
            insecure: true,
            ..eap
        };
        assert!(check_answer("wpa-eap", true, &insecure).is_none());
    }
}

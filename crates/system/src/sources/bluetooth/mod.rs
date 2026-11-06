//! Bluetooth through BlueZ on the system bus: the adapter, paired and nearby devices, and
//! pairing (with an agent of our own for PINs and passkeys), trust, blocking, renaming and
//! forgetting.
//!
//! Names come from the devices themselves and can claim anything; they are cleaned of
//! invisible characters before they are shown, and pairing prompts show the address too.

mod agent;

use super::connector::Connector;
use super::power::DeviceKind;
use super::{Ctx, Dedup, Mailbox, Trigger, clean_label, prop, prop_str, signals_from};
pub use crate::bluetooth::{Activity, Answer, Device, Prompt, PromptKind, State};
use crate::live::Stateful;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use zbus::Connection;
use zbus::names::OwnedInterfaceName;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

const BLUEZ: &str = "org.bluez";
const ADAPTER: &str = "org.bluez.Adapter1";
const DEVICE: &str = "org.bluez.Device1";
const BATTERY: &str = "org.bluez.Battery1";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
/// How long a scan for new devices runs.
const SCAN_FOR: Duration = Duration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}", bluez_message(.0))]
    Dbus(Box<zbus::Error>),
}

impl From<zbus::Error> for Error {
    fn from(e: zbus::Error) -> Self {
        Self::Dbus(Box::new(e))
    }
}

impl From<zbus::zvariant::Error> for Error {
    fn from(e: zbus::zvariant::Error) -> Self {
        Self::Dbus(Box::new(e.into()))
    }
}

impl From<zbus::fdo::Error> for Error {
    fn from(e: zbus::fdo::Error) -> Self {
        Self::Dbus(Box::new(e.into()))
    }
}

impl Error {
    /// The BlueZ error name, e.g. "org.bluez.Error.AuthenticationCanceled".
    fn name(&self) -> Option<&str> {
        let Self::Dbus(e) = self;
        match e.as_ref() {
            zbus::Error::MethodError(name, _, _) => Some(name.as_str()),
            _ => None,
        }
    }
}

/// What a BlueZ error means to someone using the widget.
fn bluez_message(e: &zbus::Error) -> String {
    let zbus::Error::MethodError(name, msg, _) = e else {
        return e.to_string();
    };
    let msg = msg.clone().unwrap_or_default();
    let short = name.as_str().rsplit('.').next().unwrap_or_default();
    explain(short, &msg)
}

/// A BlueZ error (the last part of its name, and its message), explained.
fn explain(name: &str, msg: &str) -> String {
    let lower = msg.to_lowercase();
    let known = match name {
        "AuthenticationFailed" => "pairing failed (a wrong PIN, or the device said no)",
        "AuthenticationCanceled" => "pairing was cancelled",
        "AuthenticationRejected" => "the device refused to pair",
        "AuthenticationTimeout" => "the device didn't answer in time",
        "ConnectionAttemptFailed" => "couldn't reach the device",
        "AlreadyExists" => "it is already paired",
        "InProgress" => "that is already under way",
        "NotReady" => "the adapter isn't ready (is Bluetooth on?)",
        "DoesNotExist" => "the device is gone",
        "Blocked" | "NotPermitted" if lower.contains("rfkill") || lower.contains("block") => {
            "Bluetooth is blocked (airplane mode or a hardware switch)"
        }
        _ if lower.contains("page-timeout")
            || lower.contains("page timeout")
            || lower.contains("host is down") =>
        {
            "the device is off or out of range"
        }
        _ if lower.contains("profile-unavailable") || lower.contains("protocol not available") => {
            "it offers nothing this machine can use (for audio: is PipeWire's Bluetooth \
             support installed?)"
        }
        _ if lower.contains("abort-by-local") || lower.contains("abort-by-remote") => {
            "the connection was dropped"
        }
        _ => "",
    };
    if !known.is_empty() {
        known.into()
    } else if !msg.is_empty() {
        msg.into()
    } else {
        name.into()
    }
}

/// What a device does, from the service UUIDs it offers (the common ones).
fn profile(uuid: &str) -> Option<&'static str> {
    let short = uuid
        .strip_suffix("-0000-1000-8000-00805f9b34fb")
        .and_then(|u| u.strip_prefix("0000"))?;
    Some(match short {
        "110a" | "110b" | "110d" => "Audio",
        "1108" | "1112" | "111e" | "111f" => "Headset (calls)",
        "110c" | "110e" | "110f" => "Remote control",
        "1124" | "1812" => "Input (keyboard, mouse, controller)",
        "1105" | "1106" => "File transfer",
        "1115" | "1116" | "1117" => "Network (tethering)",
        "112f" => "Phonebook",
        "1132" | "1133" | "1134" => "Messages",
        "180f" => "Battery level",
        "180d" => "Heart rate",
        "184e" | "184f" | "1850" | "1853" => "LE Audio",
        _ => return None,
    })
}

fn kind_of(icon: &str) -> DeviceKind {
    match icon {
        "audio-headset" | "audio-headphones" | "audio-card" => DeviceKind::Headphones,
        "input-keyboard" => DeviceKind::Keyboard,
        "input-mouse" | "input-tablet" => DeviceKind::Mouse,
        "input-gaming" => DeviceKind::Gamepad,
        "phone" => DeviceKind::Phone,
        _ => DeviceKind::Other,
    }
}

type Props = HashMap<String, OwnedValue>;

fn strings(props: &Props, key: &str) -> Vec<String> {
    props
        .get(key)
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| Vec::<String>::try_from(v).ok())
        .unwrap_or_default()
}

fn device_from(path: &str, props: &Props) -> Device {
    let address = prop_str(props, "Address").unwrap_or_default();
    let alias = clean_label(&prop_str(props, "Alias").unwrap_or_default());
    let name = clean_label(&prop_str(props, "Name").unwrap_or_default());
    // BlueZ makes the alias up from the address when there is no name.
    let named = !name.is_empty() || (!alias.is_empty() && alias.replace('-', ":") != address);
    let mut profiles: Vec<&'static str> = strings(props, "UUIDs")
        .iter()
        .filter_map(|u| profile(u))
        .collect();
    profiles.sort_unstable();
    profiles.dedup();
    Device {
        path: path.into(),
        claimed: if !name.is_empty() && name != alias {
            name
        } else {
            String::new()
        },
        name: if named { alias } else { address.clone() },
        named,
        kind: kind_of(&prop_str(props, "Icon").unwrap_or_default()),
        paired: prop(props, "Paired").unwrap_or(false) || prop(props, "Bonded").unwrap_or(false),
        trusted: prop(props, "Trusted").unwrap_or(false),
        blocked: prop(props, "Blocked").unwrap_or(false),
        connected: prop(props, "Connected").unwrap_or(false),
        legacy: prop(props, "LegacyPairing").unwrap_or(false),
        random_address: prop_str(props, "AddressType").as_deref() == Some("random"),
        battery: -1,
        rssi: prop(props, "RSSI"),
        profiles,
        busy: false,
        address,
    }
}

async fn fetch(conn: &Connection) -> Result<State, Error> {
    let reply = conn
        .call_method(
            Some(BLUEZ),
            "/",
            Some("org.freedesktop.DBus.ObjectManager"),
            "GetManagedObjects",
            &(),
        )
        .await?;
    let objects: HashMap<OwnedObjectPath, HashMap<OwnedInterfaceName, Props>> =
        reply.body().deserialize()?;
    let mut state = State::default();
    let mut batteries: HashMap<String, i32> = HashMap::new();
    let mut all = Vec::new();
    // The first adapter, by path (hci0 before hci1).
    let mut entries: Vec<_> = objects.iter().collect();
    entries.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
    for (path, ifaces) in entries {
        let path = path.as_str();
        for (iface, props) in ifaces {
            match iface.as_str() {
                ADAPTER if state.adapter.is_empty() => {
                    state.available = true;
                    state.adapter = path.to_string();
                    state.powered = prop(props, "Powered").unwrap_or(false);
                    state.blocked = prop_str(props, "PowerState").as_deref() == Some("off-blocked");
                    state.discovering = prop(props, "Discovering").unwrap_or(false);
                    state.adapter_name = prop_str(props, "Alias")
                        .or_else(|| prop_str(props, "Name"))
                        .unwrap_or_default();
                }
                DEVICE => all.push(device_from(path, props)),
                BATTERY => {
                    if let Some(p) = prop::<u8>(props, "Percentage") {
                        batteries.insert(path.to_string(), p.into());
                    }
                }
                _ => {}
            }
        }
    }
    for mut d in all {
        // Only the chosen adapter's devices.
        if !d.path.starts_with(&format!("{}/", state.adapter)) {
            continue;
        }
        if let Some(b) = batteries.get(&d.path) {
            d.battery = *b;
        }
        if d.paired || d.connected {
            state.devices.push(d);
        } else if d.rssi.is_some() {
            // Seen by the running (or a recent) scan.
            if d.named {
                state.nearby.push(d);
            } else {
                state.unnamed += 1;
            }
        }
    }
    state.devices.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    state
        .nearby
        .sort_by(|a, b| b.rssi.cmp(&a.rssi).then(a.name.cmp(&b.name)));
    Ok(state)
}

/// What a command does to a device.
#[derive(Debug, PartialEq)]
enum DeviceCall {
    Connect,
    Disconnect,
    /// Allow the device to connect without asking (and so reconnect by itself).
    SetTrusted(bool),
    /// Refuse every connection from the device (it stays paired).
    SetBlocked(bool),
    /// Unpair and forget the device, on this adapter.
    Forget(String),
    /// Call the device something else here ("" for its own name again).
    Rename(String),
}

/// What the agent hears back about a prompt.
#[derive(Debug, PartialEq)]
pub enum Asked {
    /// Not the device being paired from the widget.
    NotOurs,
    Answer(Answer),
    Declined,
}

/// What reaches the actor from its tasks and from the agent.
pub enum Inner {
    Fetched(Result<Box<State>, Error>),
    /// The dashboard was hidden: scanning stops with it.
    Hidden,
    ScanFailed(String),
    /// The scan with this end time has run for as long as it should.
    ScanTimeUp(Instant),
    DeviceDone {
        address: String,
        error: Option<String>,
    },
    /// `Pair` returned, however it went.
    PairReturned {
        address: String,
    },
    /// The whole pairing is over; `Err` says what to show.
    PairDone(Result<(), String>),
    /// A failure has been on show for long enough.
    FailureOver(String),
    /// The agent asks for an answer about this device.
    Ask {
        device: String,
        kind: PromptKind,
        reply: oneshot::Sender<Asked>,
    },
    /// The agent has a code to show; the reply says whether the device is ours.
    Show {
        device: String,
        kind: PromptKind,
        reply: oneshot::Sender<bool>,
    },
    /// Whether this is the device being paired from the widget.
    IsOurs {
        device: String,
        reply: oneshot::Sender<bool>,
    },
    /// BlueZ no longer needs an answer, or nobody gave one in time.
    PromptGone,
}

/// What the actor does for what `Core` decided. Each is one whole job, in order within itself.
#[derive(Debug, PartialEq)]
enum Effect {
    Publish(Box<State>),
    Toast(String),
    /// Wake the pollers, which is how what changed here reaches the widget.
    Refresh,
    SetPowered {
        adapter: String,
        on: bool,
    },
    StartScan {
        adapter: String,
        until: Instant,
    },
    StopScan {
        adapter: String,
    },
    Device {
        address: String,
        path: String,
        call: DeviceCall,
    },
    /// Pair with a device a scan found, trust it, and connect. A running scan goes first:
    /// pairing and connecting work better without.
    Pair {
        address: String,
        path: String,
        name: String,
        stop_scan_on: Option<String>,
    },
    CancelPairing {
        path: String,
    },
    /// Say `FailureOver` a while from now.
    EndFailure(String),
}

/// The widget's side of things: all the decisions, none of the talking to BlueZ.
#[derive(Default)]
struct Core {
    last: State,
    sent: Dedup<State>,
    activity: Activity,
    prompt: Option<Prompt>,
    /// The agent's request the prompt answers.
    reply: Option<oneshot::Sender<Asked>>,
    prompt_serial: u64,
    /// The device being paired from the widget: the agent only answers for it.
    pairing: Option<String>,
    /// When the scan we started ends.
    scan_until: Option<Instant>,
    selected: String,
    busy: Vec<String>,
}

impl Core {
    fn on_command(&mut self, cmd: crate::bluetooth::Command, now: Instant) -> Vec<Effect> {
        use crate::bluetooth::Command::*;
        match cmd {
            Toggle => self.toggle(),
            Scan(true) => self.start_scan(now),
            Scan(false) => self.stop_scan(),
            Connect(address) => self.device(address, DeviceCall::Connect),
            Disconnect(address) => self.device(address, DeviceCall::Disconnect),
            SetTrusted(address, on) => self.device(address, DeviceCall::SetTrusted(on)),
            SetBlocked(address, on) => self.device(address, DeviceCall::SetBlocked(on)),
            Forget(address) => {
                let adapter = self.last.adapter.clone();
                self.device(address, DeviceCall::Forget(adapter))
            }
            Rename(address, name) => self.device(address, DeviceCall::Rename(clean_label(&name))),
            Pair(address) => self.pair(address),
            CancelPairing => self.cancel_pairing(),
            Answer { serial, answer } => self.answer(serial, answer),
            Decline => self.decline(),
            Select(address) => {
                self.selected = address;
                vec![Effect::Refresh]
            }
        }
    }

    fn on_inner(&mut self, inner: Inner) -> Vec<Effect> {
        match inner {
            Inner::Fetched(result) => self.fetched(result.map(|state| *state)),
            Inner::Hidden => {
                if self.scan_until.is_some() {
                    self.stop_scan()
                } else {
                    Vec::new()
                }
            }
            Inner::ScanFailed(why) => {
                self.scan_until = None;
                self.fail(why)
            }
            Inner::ScanTimeUp(until) => {
                // A newer scan's time isn't up yet.
                if self.scan_until == Some(until) {
                    self.stop_scan()
                } else {
                    Vec::new()
                }
            }
            Inner::DeviceDone { address, error } => {
                self.busy.retain(|a| a != &address);
                let mut effects = Vec::new();
                if let Some(e) = error {
                    effects.push(Effect::Toast(format!("{}: {e}", self.name_of(&address))));
                }
                effects.push(Effect::Refresh);
                effects
            }
            Inner::PairReturned { address } => {
                self.pairing = None;
                self.busy.retain(|a| a != &address);
                if self.prompt.as_ref().is_some_and(|p| p.address == address) {
                    self.prompt = None;
                    self.reply = None;
                }
                Vec::new()
            }
            Inner::PairDone(Ok(())) => self.set_activity(Activity::Idle),
            Inner::PairDone(Err(why)) => self.fail(why),
            Inner::FailureOver(why) => {
                if self.activity == Activity::Failed(why) {
                    self.set_activity(Activity::Idle)
                } else {
                    Vec::new()
                }
            }
            Inner::Ask {
                device,
                kind,
                reply,
            } => {
                if self.ours(&device) {
                    self.show_prompt(&device, kind, Some(reply))
                } else {
                    let _ = reply.send(Asked::NotOurs);
                    Vec::new()
                }
            }
            Inner::Show {
                device,
                kind,
                reply,
            } => {
                let ours = self.ours(&device);
                let _ = reply.send(ours);
                if ours {
                    self.show_prompt(&device, kind, None)
                } else {
                    Vec::new()
                }
            }
            Inner::IsOurs { device, reply } => {
                let _ = reply.send(self.ours(&device));
                Vec::new()
            }
            Inner::PromptGone => {
                self.prompt = None;
                self.reply = None;
                vec![Effect::Refresh]
            }
        }
    }

    /// Only the device being paired from the widget gets an answer.
    fn ours(&self, device: &str) -> bool {
        self.pairing.as_deref() == Some(device)
    }

    fn fetched(&mut self, result: Result<State, Error>) -> Vec<Effect> {
        let mut state = result.unwrap_or_else(|e| {
            log::debug!("bluetooth: {e}");
            State::default()
        });
        for d in state.devices.iter_mut().chain(state.nearby.iter_mut()) {
            d.busy = self.busy.contains(&d.address);
        }
        state.activity = self.activity.clone();
        state.prompt = self.prompt.clone();
        if !self.selected.is_empty() {
            state.detail = state.device(&self.selected).cloned();
        }
        self.last = state.clone();
        if self.sent.changed(&state) {
            vec![Effect::Publish(Box::new(state))]
        } else {
            Vec::new()
        }
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

    fn toggle(&mut self) -> Vec<Effect> {
        if self.last.blocked {
            return vec![Effect::Toast(
                "Bluetooth is blocked (airplane mode or a hardware switch)".into(),
            )];
        }
        vec![Effect::SetPowered {
            adapter: self.last.adapter.clone(),
            on: !self.last.powered,
        }]
    }

    /// Look for devices to pair, for a minute.
    fn start_scan(&mut self, now: Instant) -> Vec<Effect> {
        if self.last.adapter.is_empty() {
            return Vec::new();
        }
        let until = now + SCAN_FOR;
        self.scan_until = Some(until);
        vec![Effect::StartScan {
            adapter: self.last.adapter.clone(),
            until,
        }]
    }

    fn stop_scan(&mut self) -> Vec<Effect> {
        self.scan_until = None;
        vec![Effect::StopScan {
            adapter: self.last.adapter.clone(),
        }]
    }

    fn name_of(&self, address: &str) -> String {
        self.last
            .device(address)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| address.into())
    }

    /// Run `call` for a device with it marked busy.
    fn device(&mut self, address: String, call: DeviceCall) -> Vec<Effect> {
        let Some(path) = self.last.device(&address).map(|d| d.path.clone()) else {
            return Vec::new();
        };
        self.busy.push(address.clone());
        vec![
            Effect::Refresh,
            Effect::Device {
                address,
                path,
                call,
            },
        ]
    }

    fn pair(&mut self, address: String) -> Vec<Effect> {
        let Some(path) = self.last.device(&address).map(|d| d.path.clone()) else {
            return Vec::new();
        };
        let name = self.name_of(&address);
        self.pairing = Some(path.clone());
        self.activity = Activity::Pairing(name.clone());
        self.busy.push(address.clone());
        let stop_scan_on = self.scan_until.take().map(|_| self.last.adapter.clone());
        vec![
            Effect::Refresh,
            Effect::Pair {
                address,
                path,
                name,
                stop_scan_on,
            },
        ]
    }

    /// Stop a pairing under way.
    fn cancel_pairing(&mut self) -> Vec<Effect> {
        let path = self.pairing.clone();
        let mut effects = self.decline();
        if let Some(path) = path {
            effects.push(Effect::CancelPairing { path });
        }
        effects
    }

    /// The prompt `serial` was answered. An answer to a prompt since replaced goes nowhere.
    fn answer(&mut self, serial: u64, answer: Answer) -> Vec<Effect> {
        if self.prompt.as_ref().map(|p| p.serial) != Some(serial) {
            return Vec::new();
        }
        self.prompt = None;
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(Asked::Answer(answer));
        }
        vec![Effect::Refresh]
    }

    /// Say no to the prompt.
    fn decline(&mut self) -> Vec<Effect> {
        self.prompt = None;
        if let Some(reply) = self.reply.take() {
            let _ = reply.send(Asked::Declined);
        }
        vec![Effect::Refresh]
    }

    /// Show a prompt about the device at `path`. The one it replaces hears no.
    fn show_prompt(
        &mut self,
        path: &str,
        kind: PromptKind,
        reply: Option<oneshot::Sender<Asked>>,
    ) -> Vec<Effect> {
        let device = self
            .last
            .devices
            .iter()
            .chain(&self.last.nearby)
            .find(|d| d.path == path);
        // The address is in the object path too, should the device have dropped out of the
        // lists (the prompt shows only with an address).
        let address = device
            .map(|d| d.address.clone())
            .unwrap_or_else(|| address_of_path(path));
        let name = device
            .map(|d| d.name.clone())
            .unwrap_or_else(|| address.clone());
        if let Some(old) = self.reply.take() {
            let _ = old.send(Asked::Declined);
        }
        self.prompt_serial += 1;
        self.prompt = Some(Prompt {
            serial: self.prompt_serial,
            name,
            address,
            kind,
        });
        self.reply = reply;
        vec![Effect::Refresh]
    }
}

pub struct Bluetooth {
    ctx: Ctx,
    mailbox: Mailbox<Inner>,
    core: Core,
    /// The connection the agent is served on. Pairing and scanning go through it too: BlueZ
    /// asks the agent of the connection that called Pair, and ends a scan when the
    /// connection that started it goes away.
    agent_conn: Connector<Connection, zbus::Error>,
}

impl Bluetooth {
    pub fn start(ctx: Ctx) -> Self {
        let mut me = Self {
            ctx: ctx.clone(),
            mailbox: Default::default(),
            core: Core::default(),
            agent_conn: Connector::new(ctx.runtime(), Connection::system),
        };
        let trigger = Trigger::default();
        ctx.on_dbus_signals(
            true,
            signals_from(BLUEZ),
            Duration::from_millis(300),
            trigger.clone(),
        );
        let mut ticks = ctx.ticks(trigger);
        let (c, found) = (ctx.clone(), me.mailbox.sender());
        me.mailbox.spawn(&ctx, async move {
            ticks.ready().await;
            loop {
                let result = async { fetch(&c.bus(true).await?).await }.await;
                let _ = found.send(Inner::Fetched(result.map(Box::new)));
                ticks.next(Duration::from_secs(60)).await;
            }
        });
        let (conn, found) = (me.agent_conn.clone(), me.mailbox.sender());
        me.mailbox.spawn(&ctx, agent::serve_forever(conn, found));
        // Scanning stops when the dashboard is hidden.
        let (c, found) = (ctx.clone(), me.mailbox.sender());
        me.mailbox.spawn(&ctx, async move {
            loop {
                c.until_visible(true).await;
                c.until_visible(false).await;
                let _ = found.send(Inner::Hidden);
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
        let agent_conn = self.agent_conn.clone();
        match effect {
            Effect::Publish(state) => self.ctx.send(crate::bluetooth::Event::State(state)),
            Effect::Toast(text) => self.ctx.toast(text),
            Effect::Refresh => self.ctx.refresh(),
            Effect::SetPowered { adapter, on } => {
                self.ctx.act("Bluetooth", async move {
                    set_property(&ctx, &adapter, ADAPTER, "Powered", Value::from(on)).await
                });
            }
            Effect::StartScan { adapter, until } => {
                self.mailbox.spawn(&self.ctx, async move {
                    if let Err(e) = start_discovery(&agent_conn, &adapter).await
                        && e.name() != Some("org.bluez.Error.InProgress")
                    {
                        let _ = found.send(Inner::ScanFailed(format!("Couldn't scan: {e}")));
                        return;
                    }
                    ctx.refresh();
                    tokio::time::sleep(SCAN_FOR).await;
                    let _ = found.send(Inner::ScanTimeUp(until));
                });
            }
            Effect::StopScan { adapter } => {
                self.mailbox.spawn(&self.ctx, async move {
                    stop_discovery(&agent_conn, &adapter).await;
                    ctx.refresh();
                });
            }
            Effect::Device {
                address,
                path,
                call,
            } => {
                self.mailbox.spawn(&self.ctx, async move {
                    let error = device_call(&ctx, &path, call)
                        .await
                        .err()
                        .map(|e: Error| e.to_string());
                    let _ = found.send(Inner::DeviceDone { address, error });
                });
            }
            Effect::Pair {
                address,
                path,
                name,
                stop_scan_on,
            } => {
                self.mailbox.spawn(&self.ctx, async move {
                    if let Some(adapter) = stop_scan_on {
                        stop_discovery(&agent_conn, &adapter).await;
                        ctx.refresh();
                    }
                    let paired = async {
                        let conn = agent_conn.get().await?;
                        conn.call_method(Some(BLUEZ), path.as_str(), Some(DEVICE), "Pair", &())
                            .await?;
                        Ok::<_, Error>(())
                    }
                    .await;
                    // Before connecting: the prompt goes, and the agent stops answering for it.
                    let _ = found.send(Inner::PairReturned { address });
                    let done = match paired {
                        Ok(()) => set_up_paired(&ctx, &path, &name).await,
                        Err(e) if e.name() == Some("org.bluez.Error.AuthenticationCanceled") => {
                            Ok(())
                        }
                        Err(e) => Err(format!("Couldn't pair with {name}: {e}")),
                    };
                    let _ = found.send(Inner::PairDone(done));
                });
            }
            Effect::CancelPairing { path } => {
                self.mailbox.spawn(&self.ctx, async move {
                    if let Ok(conn) = agent_conn.get().await {
                        let _ = conn
                            .call_method(
                                Some(BLUEZ),
                                path.as_str(),
                                Some(DEVICE),
                                "CancelPairing",
                                &(),
                            )
                            .await;
                    }
                });
            }
            Effect::EndFailure(why) => {
                self.mailbox.spawn(&self.ctx, async move {
                    tokio::time::sleep(Duration::from_secs(8)).await;
                    let _ = found.send(Inner::FailureOver(why));
                });
            }
        }
    }
}

impl Stateful for Bluetooth {
    type Command = crate::bluetooth::Command;
    type Inner = Inner;

    fn mailbox(&mut self) -> &mut Mailbox<Inner> {
        &mut self.mailbox
    }

    fn on_command(&mut self, cmd: crate::bluetooth::Command) {
        let effects = self.core.on_command(cmd, Instant::now());
        self.apply(effects);
    }

    fn on_inner(&mut self, inner: Inner) {
        let effects = self.core.on_inner(inner);
        self.apply(effects);
    }
}

async fn set_property(
    ctx: &Ctx,
    path: &str,
    iface: &str,
    name: &str,
    value: Value<'_>,
) -> Result<(), Error> {
    let conn = ctx.bus(true).await?;
    conn.call_method(
        Some(BLUEZ),
        path,
        Some(PROPERTIES),
        "Set",
        &(iface, name, value),
    )
    .await?;
    Ok(())
}

async fn call_device(ctx: &Ctx, path: &str, method: &str) -> Result<(), Error> {
    let conn = ctx.bus(true).await?;
    conn.call_method(Some(BLUEZ), path, Some(DEVICE), method, &())
        .await?;
    Ok(())
}

async fn device_call(ctx: &Ctx, path: &str, call: DeviceCall) -> Result<(), Error> {
    match call {
        DeviceCall::Connect => call_device(ctx, path, "Connect").await,
        DeviceCall::Disconnect => call_device(ctx, path, "Disconnect").await,
        DeviceCall::SetTrusted(on) => {
            set_property(ctx, path, DEVICE, "Trusted", Value::from(on)).await
        }
        DeviceCall::SetBlocked(on) => {
            set_property(ctx, path, DEVICE, "Blocked", Value::from(on)).await
        }
        DeviceCall::Rename(name) => {
            set_property(ctx, path, DEVICE, "Alias", Value::from(name)).await
        }
        DeviceCall::Forget(adapter) => {
            let conn = ctx.bus(true).await?;
            conn.call_method(
                Some(BLUEZ),
                adapter.as_str(),
                Some(ADAPTER),
                "RemoveDevice",
                &(ObjectPath::try_from(path)?,),
            )
            .await?;
            Ok(())
        }
    }
}

/// A device paired on purpose may reconnect by itself; the details page can take that back.
async fn set_up_paired(ctx: &Ctx, path: &str, name: &str) -> Result<(), String> {
    let trusted = set_property(ctx, path, DEVICE, "Trusted", Value::from(true)).await;
    let connected = call_device(ctx, path, "Connect").await;
    match (trusted, connected) {
        (_, Err(e)) => Err(format!("Paired with {name}, but couldn't connect: {e}")),
        (Err(e), Ok(())) => Err(format!("Paired with {name}, but couldn't trust it: {e}")),
        (Ok(()), Ok(())) => Ok(()),
    }
}

async fn start_discovery(
    agent_conn: &Connector<Connection, zbus::Error>,
    adapter: &str,
) -> Result<(), Error> {
    let conn = agent_conn.get().await?;
    let filter: HashMap<&str, Value> = HashMap::from([("Transport", Value::from("auto"))]);
    // Older BlueZ may not take the filter; the scan works without it.
    let _ = conn
        .call_method(
            Some(BLUEZ),
            adapter,
            Some(ADAPTER),
            "SetDiscoveryFilter",
            &(filter,),
        )
        .await;
    conn.call_method(Some(BLUEZ), adapter, Some(ADAPTER), "StartDiscovery", &())
        .await?;
    Ok(())
}

async fn stop_discovery(agent_conn: &Connector<Connection, zbus::Error>, adapter: &str) {
    if let Ok(conn) = agent_conn.get().await {
        let _ = conn
            .call_method(Some(BLUEZ), adapter, Some(ADAPTER), "StopDiscovery", &())
            .await;
    }
}

/// "AA:BB:CC:DD:EE:FF" from a device's object path ".../dev_AA_BB_CC_DD_EE_FF".
fn address_of_path(path: &str) -> String {
    path.rsplit('/')
        .next()
        .and_then(|last| last.strip_prefix("dev_"))
        .map(|a| a.replace('_', ":"))
        .unwrap_or_else(|| "unknown device".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::Command;
    use crate::live::Actor;

    fn device(address: &str) -> Device {
        Device {
            address: address.into(),
            name: format!("Device {address}"),
            path: format!("/org/bluez/hci0/dev_{}", address.replace(':', "_")),
            claimed: String::new(),
            named: true,
            kind: DeviceKind::Other,
            paired: false,
            trusted: false,
            blocked: false,
            connected: false,
            legacy: false,
            random_address: false,
            battery: -1,
            rssi: None,
            profiles: Vec::new(),
            busy: false,
        }
    }

    /// A core that knows an adapter and one device nearby.
    fn core() -> Core {
        let mut c = Core::default();
        c.last.adapter = "/org/bluez/hci0".into();
        c.last.nearby = vec![device("AA:BB"), device("CC:DD")];
        c
    }

    const PATH_A: &str = "/org/bluez/hci0/dev_AA_BB";

    /// A core pairing with AA:BB.
    fn pairing() -> Core {
        let mut c = core();
        c.on_command(Command::Pair("AA:BB".into()), Instant::now());
        c
    }

    fn ask(c: &mut Core, device: &str, kind: PromptKind) -> oneshot::Receiver<Asked> {
        let (reply, answer) = oneshot::channel();
        c.on_inner(Inner::Ask {
            device: device.into(),
            kind,
            reply,
        });
        answer
    }

    #[tokio::test]
    async fn the_agent_only_answers_for_the_device_being_paired() {
        let mut c = pairing();
        let mut other = ask(&mut c, "/org/bluez/hci0/dev_CC_DD", PromptKind::Authorize);
        assert_eq!(other.try_recv(), Ok(Asked::NotOurs));
        assert!(c.prompt.is_none(), "a stranger must not raise a prompt");
        let mut ours = ask(&mut c, PATH_A, PromptKind::Authorize);
        assert!(ours.try_recv().is_err(), "waits for the user");
        assert_eq!(c.prompt.as_ref().unwrap().address, "AA:BB");
    }

    #[tokio::test]
    async fn nothing_is_ours_without_a_pairing() {
        let mut c = core();
        let mut asked = ask(&mut c, PATH_A, PromptKind::Authorize);
        assert_eq!(asked.try_recv(), Ok(Asked::NotOurs));
        let (reply, mut shown) = oneshot::channel();
        c.on_inner(Inner::Show {
            device: PATH_A.into(),
            kind: PromptKind::ShowPin("0000".into()),
            reply,
        });
        assert_eq!(shown.try_recv(), Ok(false));
        assert!(c.prompt.is_none());
    }

    #[tokio::test]
    async fn the_answer_goes_to_the_prompt_it_was_for() {
        let mut c = pairing();
        let mut first = ask(&mut c, PATH_A, PromptKind::EnterPin);
        let first_serial = c.prompt.as_ref().unwrap().serial;
        // A newer prompt replaces it, and the old request hears no.
        let mut second = ask(&mut c, PATH_A, PromptKind::Authorize);
        assert_eq!(first.try_recv(), Ok(Asked::Declined));
        let serial = c.prompt.as_ref().unwrap().serial;
        assert_ne!(serial, first_serial);
        // An answer to the old prompt goes nowhere.
        let late = c.on_command(
            Command::Answer {
                serial: first_serial,
                answer: Answer::Yes,
            },
            Instant::now(),
        );
        assert!(late.is_empty());
        assert!(second.try_recv().is_err());
        assert!(c.prompt.is_some());
        // The right one does.
        c.on_command(
            Command::Answer {
                serial,
                answer: Answer::Yes,
            },
            Instant::now(),
        );
        assert_eq!(second.try_recv(), Ok(Asked::Answer(Answer::Yes)));
        assert!(c.prompt.is_none());
    }

    #[tokio::test]
    async fn declining_says_no() {
        let mut c = pairing();
        let mut asked = ask(&mut c, PATH_A, PromptKind::Authorize);
        c.on_command(Command::Decline, Instant::now());
        assert_eq!(asked.try_recv(), Ok(Asked::Declined));
        assert!(c.prompt.is_none());
    }

    /// Nobody answered, or BlueZ gave up: the prompt goes, and nobody's told no.
    #[tokio::test]
    async fn a_prompt_nobody_needs_goes() {
        let mut c = pairing();
        let mut asked = ask(&mut c, PATH_A, PromptKind::Authorize);
        c.on_inner(Inner::PromptGone);
        assert!(c.prompt.is_none());
        assert!(asked.try_recv().is_err());
    }

    #[test]
    fn pairing_marks_the_device_and_the_activity() {
        let c = pairing();
        assert_eq!(c.pairing.as_deref(), Some(PATH_A));
        assert_eq!(c.busy, ["AA:BB"]);
        assert_eq!(c.activity, Activity::Pairing("Device AA:BB".into()));
    }

    /// Until the prompt is gone the agent keeps its promise; after `Pair` returns it says no.
    #[tokio::test]
    async fn when_pair_returns_only_its_prompt_goes() {
        let mut c = pairing();
        ask(&mut c, PATH_A, PromptKind::Authorize);
        c.on_inner(Inner::PairReturned {
            address: "AA:BB".into(),
        });
        assert_eq!(c.pairing, None);
        assert!(c.busy.is_empty());
        assert!(c.prompt.is_none());
        assert!(!c.ours(PATH_A));

        // Another device's prompt stays when this one returns.
        let mut c = pairing();
        c.prompt = Some(Prompt {
            serial: 9,
            name: "Other".into(),
            address: "CC:DD".into(),
            kind: PromptKind::Authorize,
        });
        c.on_inner(Inner::PairReturned {
            address: "AA:BB".into(),
        });
        assert!(c.prompt.is_some());
    }

    #[test]
    fn pairing_with_a_scan_running_stops_the_scan_first() {
        let now = Instant::now();
        let mut c = core();
        c.on_command(Command::Scan(true), now);
        assert!(c.scan_until.is_some());
        let effects = c.on_command(Command::Pair("AA:BB".into()), now);
        assert!(c.scan_until.is_none());
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::Pair { stop_scan_on: Some(a), .. } if a == "/org/bluez/hci0"
        )));
        // Without one, it doesn't.
        let effects = core().on_command(Command::Pair("AA:BB".into()), now);
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::Pair {
                stop_scan_on: None,
                ..
            }
        )));
    }

    #[test]
    fn a_scan_ends_on_time_but_an_old_timer_doesnt_end_a_new_scan() {
        let now = Instant::now();
        let mut c = core();
        c.on_command(Command::Scan(true), now);
        let first = c.scan_until.unwrap();
        // Stopped and started again.
        c.on_command(Command::Scan(false), now);
        let later = now + Duration::from_secs(5);
        c.on_command(Command::Scan(true), later);
        let second = c.scan_until.unwrap();
        assert_ne!(first, second);
        assert!(c.on_inner(Inner::ScanTimeUp(first)).is_empty());
        assert_eq!(c.scan_until, Some(second));
        assert_eq!(
            c.on_inner(Inner::ScanTimeUp(second)),
            [Effect::StopScan {
                adapter: "/org/bluez/hci0".into()
            }]
        );
        assert_eq!(c.scan_until, None);
    }

    #[test]
    fn hiding_stops_a_scan_only_if_there_is_one() {
        let now = Instant::now();
        let mut c = core();
        assert!(c.on_inner(Inner::Hidden).is_empty());
        c.on_command(Command::Scan(true), now);
        assert!(!c.on_inner(Inner::Hidden).is_empty());
        assert_eq!(c.scan_until, None);
    }

    #[test]
    fn no_adapter_no_scan() {
        let mut c = Core::default();
        assert!(c.on_command(Command::Scan(true), Instant::now()).is_empty());
        assert_eq!(c.scan_until, None);
    }

    #[test]
    fn a_scan_that_failed_shows_why_for_a_while() {
        let now = Instant::now();
        let mut c = core();
        c.on_command(Command::Scan(true), now);
        let effects = c.on_inner(Inner::ScanFailed("no".into()));
        assert_eq!(c.scan_until, None);
        assert_eq!(c.activity, Activity::Failed("no".into()));
        assert!(effects.contains(&Effect::EndFailure("no".into())));
        // Time's up: it goes.
        c.on_inner(Inner::FailureOver("no".into()));
        assert_eq!(c.activity, Activity::Idle);
    }

    #[test]
    fn a_failure_doesnt_end_what_happened_since() {
        let mut c = core();
        c.on_inner(Inner::ScanFailed("no".into()));
        c.activity = Activity::Pairing("x".into());
        assert!(c.on_inner(Inner::FailureOver("no".into())).is_empty());
        assert_eq!(c.activity, Activity::Pairing("x".into()));
    }

    #[test]
    fn a_device_is_busy_while_a_call_is_under_way() {
        let now = Instant::now();
        let mut c = core();
        let effects = c.on_command(Command::Connect("AA:BB".into()), now);
        assert_eq!(c.busy, ["AA:BB"]);
        assert!(effects.iter().any(|e| matches!(
            e,
            Effect::Device {
                call: DeviceCall::Connect,
                ..
            }
        )));
        let effects = c.on_inner(Inner::DeviceDone {
            address: "AA:BB".into(),
            error: Some("off".into()),
        });
        assert!(c.busy.is_empty());
        assert!(effects.contains(&Effect::Toast("Device AA:BB: off".into())));
    }

    #[test]
    fn a_device_nobody_knows_does_nothing() {
        let mut c = core();
        let now = Instant::now();
        assert!(
            c.on_command(Command::Connect("EE:FF".into()), now)
                .is_empty()
        );
        assert!(c.on_command(Command::Pair("EE:FF".into()), now).is_empty());
        assert!(c.busy.is_empty());
        assert_eq!(c.pairing, None);
    }

    #[test]
    fn what_was_fetched_gets_the_widgets_side() {
        let mut c = core();
        c.busy = vec!["AA:BB".into()];
        c.selected = "CC:DD".into();
        c.activity = Activity::Pairing("x".into());
        let fetched = State {
            adapter: "/org/bluez/hci0".into(),
            nearby: vec![device("AA:BB"), device("CC:DD")],
            ..Default::default()
        };
        let effects = c.on_inner(Inner::Fetched(Ok(Box::new(fetched.clone()))));
        let [Effect::Publish(state)] = &effects[..] else {
            panic!("should publish");
        };
        assert!(state.nearby[0].busy);
        assert!(!state.nearby[1].busy);
        assert_eq!(state.detail.as_ref().unwrap().address, "CC:DD");
        assert_eq!(state.activity, Activity::Pairing("x".into()));
        // The same again isn't sent again.
        assert!(c.on_inner(Inner::Fetched(Ok(Box::new(fetched)))).is_empty());
    }

    /// A failed fetch means no Bluetooth to show, as far as we can tell.
    #[test]
    fn a_failed_fetch_shows_none() {
        let mut c = core();
        c.on_inner(Inner::Fetched(Err(
            zbus::Error::Failure("gone".into()).into()
        )));
        assert!(c.last.nearby.is_empty());
        assert_eq!(c.last.adapter, "");
    }

    #[test]
    fn blocked_says_so_instead_of_trying() {
        let mut c = core();
        c.last.blocked = true;
        let effects = c.on_command(Command::Toggle, Instant::now());
        assert!(matches!(&effects[..], [Effect::Toast(_)]));
    }

    #[test]
    fn toggling_asks_for_the_opposite() {
        let mut c = core();
        c.last.powered = true;
        assert_eq!(
            c.on_command(Command::Toggle, Instant::now()),
            [Effect::SetPowered {
                adapter: "/org/bluez/hci0".into(),
                on: false
            }]
        );
    }

    #[test]
    fn cancelling_a_pairing_declines_and_cancels() {
        let mut c = pairing();
        let effects = c.on_command(Command::CancelPairing, Instant::now());
        assert!(effects.contains(&Effect::CancelPairing {
            path: PATH_A.into()
        }));
        // Without a pairing there's nothing to cancel.
        let effects = core().on_command(Command::CancelPairing, Instant::now());
        assert_eq!(effects, [Effect::Refresh]);
    }

    /// The blog's way: queue what happens, drop the sender, await the loop, look at what's left.
    /// On a dormant context, so the effects don't go near a bus.
    #[tokio::test]
    async fn the_actor_handles_what_it_finds_before_commands() {
        let (ctx, events) = crate::sources::testing::ctx();
        let me = Bluetooth::start(ctx);
        let (commands, inbox) = tokio::sync::mpsc::unbounded_channel();
        // Queued first, but it needs the adapter that's found below.
        let _ = commands.send(Command::Scan(true));
        let _ = me.mailbox.sender().send(Inner::Fetched(Ok(Box::new(State {
            adapter: "/org/bluez/hci0".into(),
            ..Default::default()
        }))));
        drop(commands);
        let me = me.event_loop(inbox).await;
        assert!(me.core.scan_until.is_some());
        assert_eq!(crate::sources::testing::reported(events).len(), 1);
    }

    #[test]
    fn addresses() {
        assert_eq!(
            address_of_path("/org/bluez/hci0/dev_20_24_03_07_0B_62"),
            "20:24:03:07:0B:62"
        );
    }

    #[test]
    fn errors() {
        assert_eq!(
            explain("Failed", "br-connection-page-timeout"),
            "the device is off or out of range"
        );
        assert_eq!(
            explain("AuthenticationRejected", ""),
            "the device refused to pair"
        );
        assert_eq!(explain("Failed", "Something else"), "Something else");
        assert_eq!(explain("Odd", ""), "Odd");
    }

    #[test]
    fn profiles() {
        assert_eq!(
            profile("0000110b-0000-1000-8000-00805f9b34fb"),
            Some("Audio")
        );
        assert_eq!(
            profile("00001812-0000-1000-8000-00805f9b34fb"),
            Some("Input (keyboard, mouse, controller)")
        );
        assert_eq!(profile("0000fe2c-0000-1000-8000-00805f9b34fb"), None);
        assert_eq!(profile("not-a-uuid"), None);
    }

    #[test]
    fn devices() {
        let v = |v: Value<'static>| OwnedValue::try_from(v).unwrap();
        let props: Props = HashMap::from([
            ("Address".to_string(), v(Value::from("20:24:03:07:0B:62"))),
            ("Alias".to_string(), v(Value::from("My buds"))),
            // A name with a bidi override, to look like something else.
            ("Name".to_string(), v(Value::from("Fair\u{202E}sdub"))),
            ("Icon".to_string(), v(Value::from("audio-headset"))),
            ("Paired".to_string(), v(Value::from(true))),
            ("RSSI".to_string(), v(Value::from(-60i16))),
            (
                "UUIDs".to_string(),
                v(Value::from(vec![
                    "0000110b-0000-1000-8000-00805f9b34fb".to_string(),
                    "0000110a-0000-1000-8000-00805f9b34fb".to_string(),
                ])),
            ),
        ]);
        let d = device_from("/org/bluez/hci0/dev_20", &props);
        assert_eq!(d.name, "My buds");
        assert_eq!(d.claimed, "Fairsdub");
        assert_eq!(d.kind, DeviceKind::Headphones);
        assert!(d.paired && d.named);
        assert_eq!(d.bars(), 3);
        assert_eq!(d.profiles, ["Audio"]);
        // No name: BlueZ's alias is the address with dashes.
        let props: Props = HashMap::from([
            ("Address".to_string(), v(Value::from("AA:BB:CC:DD:EE:FF"))),
            ("Alias".to_string(), v(Value::from("AA-BB-CC-DD-EE-FF"))),
        ]);
        let d = device_from("/org/bluez/hci0/dev_AA", &props);
        assert!(!d.named);
        assert_eq!(d.name, "AA:BB:CC:DD:EE:FF");
    }
}

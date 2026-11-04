//! Bluetooth through BlueZ on the system bus: the adapter, paired and nearby devices, and
//! pairing (with an agent of our own for PINs and passkeys), trust, blocking, renaming and
//! forgetting.
//!
//! Names come from the devices themselves and can claim anything; they are cleaned of
//! invisible characters before they are shown, and pairing prompts show the address too.

mod agent;

use super::power::DeviceKind;
use super::{Ctx, Dedup, Trigger, clean_label, prop, prop_str, signals_from};
pub use crate::bluetooth::{Activity, Answer, Device, Prompt, PromptKind, State};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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

/// The widget's side of things, kept between refreshes.
#[derive(Default)]
struct Local {
    activity: Activity,
    prompt: Option<Prompt>,
    /// The agent's request the prompt answers (None: declined).
    reply: Option<oneshot::Sender<Option<Answer>>>,
    prompt_serial: u64,
    /// The device being paired from the widget: the agent only answers for it.
    pairing: Option<String>,
    /// When the scan we started ends.
    scan_until: Option<Instant>,
    selected: String,
    busy: Vec<String>,
}

#[derive(Clone)]
pub struct Bluetooth {
    ctx: Ctx,
    last: Arc<Mutex<State>>,
    sent: Arc<Dedup<State>>,
    local: Arc<Mutex<Local>>,
    /// The connection the agent is served on. Pairing and scanning go through it too: BlueZ
    /// asks the agent of the connection that called Pair, and ends a scan when the
    /// connection that started it goes away.
    agent_conn: Arc<tokio::sync::OnceCell<Connection>>,
}

impl Bluetooth {
    pub fn start(ctx: Ctx) -> Self {
        let me = Self {
            ctx: ctx.clone(),
            last: Default::default(),
            sent: Default::default(),
            local: Default::default(),
            agent_conn: Default::default(),
        };
        let trigger = Trigger::default();
        ctx.on_dbus_signals(
            true,
            signals_from(BLUEZ),
            Duration::from_millis(300),
            trigger.clone(),
        );
        let m = me.clone();
        ctx.poll_events(trigger, Duration::from_secs(60), move || {
            let m = m.clone();
            async move { m.refresh().await }
        });
        agent::start(me.clone());
        // Scanning stops when the dashboard is hidden.
        let m = me.clone();
        ctx.spawn(async move {
            loop {
                m.ctx.until_visible(true).await;
                m.ctx.until_visible(false).await;
                if m.local().scan_until.is_some() {
                    m.stop_scan().await;
                }
            }
        });
        me
    }

    fn local(&self) -> std::sync::MutexGuard<'_, Local> {
        self.local.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn last(&self) -> State {
        self.last.lock().map(|l| l.clone()).unwrap_or_default()
    }

    async fn agent_conn(&self) -> Result<&Connection, Error> {
        Ok(self
            .agent_conn
            .get_or_try_init(|| async { Connection::system().await })
            .await?)
    }

    async fn refresh(&self) {
        let result = async { fetch(&self.ctx.bus(true).await?).await }.await;
        let mut state = match result {
            Ok(s) => s,
            Err(e) => {
                log::debug!("bluetooth: {e}");
                State::default()
            }
        };
        {
            let local = self.local();
            for d in state.devices.iter_mut().chain(state.nearby.iter_mut()) {
                d.busy = local.busy.contains(&d.address);
            }
            state.activity = local.activity.clone();
            state.prompt = local.prompt.clone();
            if !local.selected.is_empty() {
                state.detail = state.device(&local.selected).cloned();
            }
        }
        if let Ok(mut l) = self.last.lock() {
            *l = state.clone();
        }
        if self.sent.changed(&state) {
            self.ctx
                .send(crate::bluetooth::Event::State(Box::new(state)));
        }
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

    async fn set_property(
        &self,
        path: &str,
        iface: &str,
        name: &str,
        value: Value<'_>,
    ) -> Result<(), Error> {
        let conn = self.ctx.bus(true).await?;
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

    pub fn toggle(&self) {
        let last = self.last();
        if last.blocked {
            self.ctx
                .toast("Bluetooth is blocked (airplane mode or a hardware switch)");
            return;
        }
        let me = self.clone();
        self.ctx.act("Bluetooth", async move {
            me.set_property(
                &last.adapter,
                ADAPTER,
                "Powered",
                Value::from(!last.powered),
            )
            .await
        });
    }

    /// Look for devices to pair, for a minute.
    pub fn scan(&self, on: bool) {
        let me = self.clone();
        if !on {
            self.ctx.spawn(async move { me.stop_scan().await });
            return;
        }
        let adapter = self.last().adapter;
        if adapter.is_empty() {
            return;
        }
        let until = Instant::now() + SCAN_FOR;
        self.local().scan_until = Some(until);
        self.ctx.spawn(async move {
            let started = async {
                let conn = me.agent_conn().await?;
                let filter: HashMap<&str, Value> =
                    HashMap::from([("Transport", Value::from("auto"))]);
                // Older BlueZ may not take the filter; the scan works without it.
                let _ = conn
                    .call_method(
                        Some(BLUEZ),
                        adapter.as_str(),
                        Some(ADAPTER),
                        "SetDiscoveryFilter",
                        &(filter,),
                    )
                    .await;
                conn.call_method(
                    Some(BLUEZ),
                    adapter.as_str(),
                    Some(ADAPTER),
                    "StartDiscovery",
                    &(),
                )
                .await?;
                Ok::<_, Error>(())
            }
            .await;
            if let Err(e) = started
                && e.name() != Some("org.bluez.Error.InProgress")
            {
                me.local().scan_until = None;
                me.fail(format!("Couldn't scan: {e}"));
                return;
            }
            me.ctx.refresh();
            tokio::time::sleep(SCAN_FOR).await;
            if me.local().scan_until == Some(until) {
                me.stop_scan().await;
            }
        });
    }

    async fn stop_scan(&self) {
        self.local().scan_until = None;
        let adapter = self.last().adapter;
        if let Ok(conn) = self.agent_conn().await {
            let _ = conn
                .call_method(
                    Some(BLUEZ),
                    adapter.as_str(),
                    Some(ADAPTER),
                    "StopDiscovery",
                    &(),
                )
                .await;
        }
        self.ctx.refresh();
    }

    fn path_of(&self, address: &str) -> Option<String> {
        self.last().device(address).map(|d| d.path.clone())
    }

    fn name_of(&self, address: &str) -> String {
        self.last()
            .device(address)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| address.into())
    }

    /// Run `call` for a device with it marked busy; a failure becomes a toast.
    fn device_action<F, Fut>(&self, address: String, call: F)
    where
        F: FnOnce(Self, String) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<(), Error>> + Send,
    {
        let Some(path) = self.path_of(&address) else {
            return;
        };
        self.local().busy.push(address.clone());
        self.ctx.refresh();
        let me = self.clone();
        self.ctx.spawn(async move {
            let result = call(me.clone(), path).await;
            me.local().busy.retain(|a| a != &address);
            if let Err(e) = result {
                me.ctx.toast(format!("{}: {e}", me.name_of(&address)));
            }
            me.ctx.refresh();
        });
    }

    async fn device_call(&self, path: &str, method: &str) -> Result<(), Error> {
        let conn = self.ctx.bus(true).await?;
        conn.call_method(Some(BLUEZ), path, Some(DEVICE), method, &())
            .await?;
        Ok(())
    }

    pub fn connect(&self, address: String) {
        self.device_action(address, |me, path| async move {
            me.device_call(&path, "Connect").await
        });
    }

    pub fn disconnect(&self, address: String) {
        self.device_action(address, |me, path| async move {
            me.device_call(&path, "Disconnect").await
        });
    }

    /// Allow the device to connect without asking (and so reconnect by itself).
    pub fn set_trusted(&self, address: String, on: bool) {
        self.device_action(address, move |me, path| async move {
            me.set_property(&path, DEVICE, "Trusted", Value::from(on))
                .await
        });
    }

    /// Refuse every connection from the device (it stays paired).
    pub fn set_blocked(&self, address: String, on: bool) {
        self.device_action(address, move |me, path| async move {
            me.set_property(&path, DEVICE, "Blocked", Value::from(on))
                .await
        });
    }

    /// Unpair and forget the device.
    pub fn forget(&self, address: String) {
        let adapter = self.last().adapter;
        self.device_action(address, move |me, path| async move {
            let conn = me.ctx.bus(true).await?;
            conn.call_method(
                Some(BLUEZ),
                adapter.as_str(),
                Some(ADAPTER),
                "RemoveDevice",
                &(ObjectPath::try_from(path.as_str())?,),
            )
            .await?;
            Ok(())
        });
    }

    /// Call the device something else here ("" for its own name again).
    pub fn rename(&self, address: String, name: String) {
        let name = clean_label(&name);
        self.device_action(address, move |me, path| async move {
            me.set_property(&path, DEVICE, "Alias", Value::from(name))
                .await
        });
    }

    /// Pair with a device a scan found, trust it, and connect.
    pub fn pair(&self, address: String) {
        let Some(path) = self.path_of(&address) else {
            return;
        };
        let name = self.name_of(&address);
        {
            let mut local = self.local();
            local.pairing = Some(path.clone());
            local.activity = Activity::Pairing(name.clone());
            local.busy.push(address.clone());
        }
        self.ctx.refresh();
        let me = self.clone();
        self.ctx.spawn(async move {
            // Pairing and connecting work better without a scan running.
            if me.local().scan_until.is_some() {
                me.stop_scan().await;
            }
            let paired = async {
                let conn = me.agent_conn().await?;
                conn.call_method(Some(BLUEZ), path.as_str(), Some(DEVICE), "Pair", &())
                    .await?;
                Ok::<_, Error>(())
            }
            .await;
            {
                let mut local = me.local();
                local.pairing = None;
                local.busy.retain(|a| a != &address);
                if local.prompt.as_ref().is_some_and(|p| p.address == address) {
                    local.prompt = None;
                    local.reply = None;
                }
            }
            match paired {
                Ok(()) => {
                    // A device paired on purpose may reconnect by itself; the details page
                    // can take that back.
                    let trusted = me
                        .set_property(&path, DEVICE, "Trusted", Value::from(true))
                        .await;
                    let connected = me.device_call(&path, "Connect").await;
                    match (trusted, connected) {
                        (_, Err(e)) => {
                            me.fail(format!("Paired with {name}, but couldn't connect: {e}"))
                        }
                        (Err(e), Ok(())) => {
                            me.fail(format!("Paired with {name}, but couldn't trust it: {e}"))
                        }
                        (Ok(()), Ok(())) => me.set_activity(Activity::Idle),
                    }
                }
                Err(e) if e.name() == Some("org.bluez.Error.AuthenticationCanceled") => {
                    me.set_activity(Activity::Idle);
                }
                Err(e) => me.fail(format!("Couldn't pair with {name}: {e}")),
            }
        });
    }

    /// Stop a pairing under way.
    pub fn cancel_pairing(&self) {
        let path = self.local().pairing.clone();
        self.decline();
        if let Some(path) = path {
            let me = self.clone();
            self.ctx.spawn(async move {
                if let Ok(conn) = me.agent_conn().await {
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
    }

    /// The prompt `serial` was answered. An answer to a prompt since replaced goes nowhere.
    pub fn answer(&self, serial: u64, answer: Answer) {
        let mut local = self.local();
        if local.prompt.as_ref().map(|p| p.serial) != Some(serial) {
            return;
        }
        local.prompt = None;
        if let Some(tx) = local.reply.take() {
            let _ = tx.send(Some(answer));
        }
        drop(local);
        self.ctx.refresh();
    }

    /// Say no to the prompt.
    pub fn decline(&self) {
        let mut local = self.local();
        local.prompt = None;
        if let Some(tx) = local.reply.take() {
            let _ = tx.send(None);
        }
        drop(local);
        self.ctx.refresh();
    }

    /// Show the details of a device ("" to leave them).
    pub fn select(&self, address: String) {
        self.local().selected = address;
        self.ctx.refresh();
    }

    /// For the agent: the device being paired from the widget, if any.
    fn pairing(&self) -> Option<String> {
        self.local().pairing.clone()
    }

    /// For the agent: show a prompt about `path` and wait for its answer (None: declined).
    fn ask(&self, path: &str, kind: PromptKind) -> oneshot::Receiver<Option<Answer>> {
        let (tx, rx) = oneshot::channel();
        self.prompt(path, kind, Some(tx));
        rx
    }

    /// For the agent: show a code to type on the device; there is nothing to answer.
    fn show(&self, path: &str, kind: PromptKind) {
        self.prompt(path, kind, None);
    }

    fn prompt(&self, path: &str, kind: PromptKind, reply: Option<oneshot::Sender<Option<Answer>>>) {
        let last = self.last();
        let device = last
            .devices
            .iter()
            .chain(&last.nearby)
            .find(|d| d.path == path);
        // The address is in the object path too, should the device have dropped out of the
        // lists (the prompt shows only with an address).
        let address = device
            .map(|d| d.address.clone())
            .unwrap_or_else(|| address_of_path(path));
        let mut local = self.local();
        if let Some(old) = local.reply.take() {
            let _ = old.send(None);
        }
        local.prompt_serial += 1;
        local.prompt = Some(Prompt {
            serial: local.prompt_serial,
            name: device
                .map(|d| d.name.clone())
                .unwrap_or_else(|| address.clone()),
            address,
            kind,
        });
        local.reply = reply;
        drop(local);
        self.ctx.refresh();
    }

    /// For the agent: BlueZ no longer needs an answer.
    fn drop_prompt(&self) {
        let mut local = self.local();
        local.prompt = None;
        local.reply = None;
        drop(local);
        self.ctx.refresh();
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

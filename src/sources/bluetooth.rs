//! Bluetooth through BlueZ on the system bus.

use super::{Ctx, Dedup, Msg, Trigger, prop, prop_str, signals_from};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zbus::Connection;
use zbus::fdo::{ObjectManagerProxy, PropertiesProxy};
use zbus::names::InterfaceName;
use zbus::zvariant::{OwnedObjectPath, Value};

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("BlueZ is not running: {0}")]
    NotRunning(#[source] zbus::fdo::Error),
    #[error("switching the adapter (is it blocked by rfkill?): {0}")]
    Switch(#[source] zbus::fdo::Error),
    #[error(transparent)]
    Dbus(#[from] zbus::Error),
    #[error(transparent)]
    Name(#[from] zbus::names::Error),
    #[error(transparent)]
    Value(#[from] zbus::zvariant::Error),
}

type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Device {
    pub path: String,
    pub address: String,
    pub name: String,
    pub kind: String,
    pub connected: bool,
    pub paired: bool,
    pub battery: i32,
    pub busy: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub available: bool,
    pub powered: bool,
    pub adapter: String,
    pub devices: Vec<Device>,
    pub status: String,
}

async fn fetch(conn: &Connection) -> Result<State> {
    let om = ObjectManagerProxy::builder(conn)
        .destination("org.bluez")?
        .path("/")?
        .build()
        .await?;
    let objects = om.get_managed_objects().await.map_err(Error::NotRunning)?;
    let mut state = State::default();
    let mut batteries: HashMap<String, i32> = HashMap::new();
    for (path, ifaces) in &objects {
        let path = path.as_str();
        for (iface, props) in ifaces {
            match iface.as_str() {
                "org.bluez.Adapter1" if state.adapter.is_empty() => {
                    state.available = true;
                    state.adapter = path.to_string();
                    state.powered = prop(props, "Powered").unwrap_or(false);
                }
                "org.bluez.Device1" => {
                    let paired = prop(props, "Paired").unwrap_or(false);
                    let connected = prop(props, "Connected").unwrap_or(false);
                    if !paired && !connected {
                        continue;
                    }
                    state.devices.push(Device {
                        path: path.to_string(),
                        address: prop_str(props, "Address").unwrap_or_default(),
                        name: prop_str(props, "Alias")
                            .or_else(|| prop_str(props, "Name"))
                            .unwrap_or_else(|| "Unknown".into()),
                        kind: prop_str(props, "Icon").unwrap_or_default(),
                        connected,
                        paired,
                        battery: -1,
                        busy: false,
                    });
                }
                "org.bluez.Battery1" => {
                    if let Some(p) = prop::<u8>(props, "Percentage") {
                        batteries.insert(path.to_string(), p as i32);
                    }
                }
                _ => {}
            }
        }
    }
    for d in &mut state.devices {
        if let Some(b) = batteries.get(&d.path) {
            d.battery = *b;
        }
    }
    state.devices.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(state)
}

#[derive(Clone)]
pub struct Bluetooth {
    ctx: Ctx,
    last: Arc<Mutex<State>>,
    sent: Arc<Dedup<State>>,
    busy: Arc<Mutex<Vec<String>>>,
    status: Arc<Mutex<String>>,
}

impl Bluetooth {
    pub fn start(ctx: Ctx) -> Self {
        let me = Self {
            ctx: ctx.clone(),
            last: Default::default(),
            sent: Default::default(),
            busy: Default::default(),
            status: Default::default(),
        };
        let m = me.clone();
        let trigger = Trigger::default();
        ctx.on_dbus_signals(
            true,
            signals_from("org.bluez"),
            Duration::from_millis(300),
            trigger.clone(),
        );
        ctx.poll_events(trigger, Duration::from_secs(60), move || {
            let m = m.clone();
            async move { m.refresh().await }
        });
        me
    }

    async fn conn(&self) -> Result<Connection> {
        Ok(self.ctx.bus(true).await?)
    }

    async fn refresh(&self) {
        let result = async { fetch(&self.conn().await?).await }.await;
        let mut state = match result {
            Ok(s) => s,
            Err(e) => {
                log::debug!("bluetooth: {e}");
                State::default()
            }
        };
        let busy = self.busy.lock().map(|b| b.clone()).unwrap_or_default();
        for d in &mut state.devices {
            d.busy = busy.contains(&d.address);
        }
        state.status = self.status.lock().map(|s| s.clone()).unwrap_or_default();
        if let Ok(mut l) = self.last.lock() {
            *l = state.clone();
        }
        if self.sent.changed(&state) {
            self.ctx.send(Msg::Bluetooth(state));
        }
    }

    pub fn toggle(&self) {
        let me = self.clone();
        let (adapter, on) = self
            .last
            .lock()
            .map(|l| (l.adapter.clone(), !l.powered))
            .unwrap_or_default();
        self.ctx.act("Bluetooth", async move {
            let conn = me.conn().await?;
            let props = PropertiesProxy::builder(&conn)
                .destination("org.bluez")?
                .path(adapter)?
                .build()
                .await?;
            props
                .set(
                    InterfaceName::from_static_str("org.bluez.Adapter1")?,
                    "Powered",
                    Value::from(on),
                )
                .await
                .map_err(Error::Switch)?;
            Ok::<_, Error>(())
        });
    }

    fn device_call(&self, address: String, method: &'static str) {
        let me = self.clone();
        let path = self.last.lock().ok().and_then(|l| {
            l.devices
                .iter()
                .find(|d| d.address == address)
                .map(|d| d.path.clone())
        });
        let Some(path) = path else { return };
        if let Ok(mut b) = self.busy.lock() {
            b.push(address.clone());
        }
        self.ctx.refresh();
        self.ctx.spawn(async move {
            let result = async {
                let conn = me.conn().await?;
                let path = OwnedObjectPath::try_from(path)?;
                conn.call_method(
                    Some("org.bluez"),
                    &path,
                    Some("org.bluez.Device1"),
                    method,
                    &(),
                )
                .await?;
                Ok::<_, Error>(())
            }
            .await;
            if let Ok(mut b) = me.busy.lock() {
                b.retain(|a| a != &address);
            }
            if let Err(e) = result {
                let name = me.last.lock().ok().and_then(|l| {
                    l.devices
                        .iter()
                        .find(|d| d.address == address)
                        .map(|d| d.name.clone())
                });
                me.ctx.toast(format!("{}: {e}", name.unwrap_or(address)));
            }
            me.ctx.refresh();
        });
    }

    pub fn connect(&self, address: String) {
        self.device_call(address, "Connect");
    }

    pub fn disconnect(&self, address: String) {
        self.device_call(address, "Disconnect");
    }
}

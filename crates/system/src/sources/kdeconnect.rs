//! Phones paired through KDE Connect (its daemon on the session bus): battery, ringing a lost
//! phone, sending files to it.
#![allow(clippy::disallowed_types)] // Still to be moved over to an actor.

use super::{Ctx, Dedup, Trigger, prop, prop_str, signals_from};
pub use crate::phones::{Phone, State};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zbus::Connection;
use zbus::fdo::PropertiesProxy;
use zbus::names::InterfaceName;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const DEST: &str = "org.kde.kdeconnect";
const BASE: &str = "/modules/kdeconnect";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("KDE Connect isn't running")]
    NotRunning,
    #[error("{0} is not connected")]
    Unreachable(String),
    #[error("the file chooser (xdg-desktop-portal) failed: {0}")]
    Portal(#[source] zbus::Error),
    #[error(transparent)]
    Dbus(#[from] zbus::Error),
    #[error(transparent)]
    Fdo(#[from] zbus::fdo::Error),
    #[error(transparent)]
    Name(#[from] zbus::names::Error),
}

type Result<T, E = Error> = std::result::Result<T, E>;

fn device_path(id: &str) -> String {
    format!("{BASE}/devices/{id}")
}

async fn props(
    conn: &Connection,
    path: String,
    iface: &'static str,
) -> Result<HashMap<String, OwnedValue>> {
    let p = PropertiesProxy::builder(conn)
        .destination(DEST)?
        .path(path)?
        .build()
        .await?;
    Ok(p.get_all(InterfaceName::from_static_str(iface)?).await?)
}

async fn fetch(conn: &Connection) -> Result<Vec<Phone>> {
    let daemon = zbus::Proxy::new(conn, DEST, BASE, "org.kde.kdeconnect.daemon").await?;
    let ids: Vec<String> = daemon
        .call("devices", &(false, true))
        .await
        .map_err(|_| Error::NotRunning)?;
    let mut phones = Vec::new();
    for id in ids {
        let path = device_path(&id);
        let Ok(d) = props(conn, path.clone(), "org.kde.kdeconnect.device").await else {
            continue;
        };
        let reachable = prop(&d, "isReachable").unwrap_or(false);
        let plugins: Vec<String> = d
            .get("supportedPlugins")
            .and_then(|v| v.try_clone().ok())
            .and_then(|v| Vec::<String>::try_from(v).ok())
            .unwrap_or_default();
        let has = |p: &str| reachable && plugins.iter().any(|x| x == p);
        let (mut charge, mut charging) = (None, false);
        if has("kdeconnect_battery")
            && let Ok(b) = props(
                conn,
                format!("{path}/battery"),
                "org.kde.kdeconnect.device.battery",
            )
            .await
        {
            // -1 until the phone has reported.
            charge = prop::<i32>(&b, "charge").filter(|&c| c >= 0);
            charging = prop(&b, "isCharging").unwrap_or(false);
        }
        phones.push(Phone {
            name: prop_str(&d, "name").unwrap_or_else(|| id.clone()),
            kind: prop_str(&d, "type").unwrap_or_default(),
            reachable,
            charge,
            charging,
            can_ring: has("kdeconnect_findmyphone"),
            can_share: has("kdeconnect_share"),
            id,
        });
    }
    let rank = |p: &Phone| {
        (
            !p.reachable,
            !matches!(p.kind.as_str(), "phone" | "tablet"),
            p.name.to_lowercase(),
        )
    };
    phones.sort_by_key(rank);
    Ok(phones)
}

/// Ask the desktop portal for files to send; empty if the user cancels.
async fn pick_files(conn: &Connection, title: &str) -> Result<Vec<String>, zbus::Error> {
    let token = format!(
        "niri_dash_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    let sender = conn
        .unique_name()
        .map(|n| n.trim_start_matches(':').replace('.', "_"))
        .unwrap_or_default();
    // Subscribe before asking, so a quick answer isn't missed.
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.portal.Request")?
        .member("Response")?
        .path(format!(
            "/org/freedesktop/portal/desktop/request/{sender}/{token}"
        ))?
        .build();
    let mut responses = zbus::MessageStream::for_match_rule(rule, conn, Some(4)).await?;
    let chooser = zbus::Proxy::new(
        conn,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.FileChooser",
    )
    .await?;
    let mut options: HashMap<&str, Value> = HashMap::new();
    options.insert("handle_token", Value::from(token.as_str()));
    options.insert("multiple", Value::from(true));
    options.insert("accept_label", Value::from("Send"));
    let _: OwnedObjectPath = chooser.call("OpenFile", &("", title, options)).await?;
    let Some(msg) = responses.next().await else {
        return Ok(Vec::new());
    };
    let (code, results): (u32, HashMap<String, OwnedValue>) = msg?.body().deserialize()?;
    if code != 0 {
        return Ok(Vec::new());
    }
    Ok(results
        .get("uris")
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| Vec::<String>::try_from(v).ok())
        .unwrap_or_default())
}

#[derive(Clone)]
pub struct KdeConnect {
    ctx: Ctx,
    last: Arc<Mutex<State>>,
    sent: Arc<Dedup<State>>,
}

impl KdeConnect {
    pub fn start(ctx: Ctx) -> Self {
        let me = Self {
            ctx: ctx.clone(),
            last: Default::default(),
            sent: Default::default(),
        };
        let m = me.clone();
        let trigger = Trigger::default();
        ctx.on_dbus_signals(
            false,
            signals_from(DEST),
            Duration::from_millis(500),
            trigger.clone(),
        );
        ctx.poll_events(trigger, Duration::from_secs(60), move || {
            let m = m.clone();
            async move { m.refresh().await }
        });
        me
    }

    async fn refresh(&self) {
        let state = match async { fetch(&self.ctx.bus(false).await?).await }.await {
            Ok(phones) => State {
                available: true,
                phones,
            },
            Err(e) => {
                log::debug!("kdeconnect: {e}");
                State::default()
            }
        };
        if let Ok(mut l) = self.last.lock() {
            *l = state.clone();
        }
        if self.sent.changed(&state) {
            self.ctx.send(crate::phones::Event::State(state));
        }
    }

    fn phone(&self, id: &str) -> Option<Phone> {
        let last = self.last.lock().ok()?;
        last.phones.iter().find(|p| p.id == id).cloned()
    }

    /// Make the phone ring, to find it.
    pub fn ring(&self, id: String) {
        let ctx = self.ctx.clone();
        let phone = self.phone(&id);
        self.ctx.act("Ring", async move {
            let phone = phone.ok_or(Error::NotRunning)?;
            if !phone.reachable {
                return Err(Error::Unreachable(phone.name));
            }
            let conn = ctx.bus(false).await?;
            let p = zbus::Proxy::new(
                &conn,
                DEST,
                format!("{}/findmyphone", device_path(&id)),
                "org.kde.kdeconnect.device.findmyphone",
            )
            .await?;
            p.call_method("ring", &()).await?;
            Ok(())
        });
    }

    /// Pick files with the desktop's file chooser and send them. (The dashboard hides itself
    /// first, as it would cover the dialog.)
    pub fn send_files(&self, id: String) {
        let ctx = self.ctx.clone();
        let phone = self.phone(&id);
        self.ctx.act("Send files", async move {
            let phone = phone.ok_or(Error::NotRunning)?;
            if !phone.reachable {
                return Err(Error::Unreachable(phone.name));
            }
            let conn = ctx.bus(false).await?;
            let uris = pick_files(&conn, &format!("Send to {}", phone.name))
                .await
                .map_err(Error::Portal)?;
            if uris.is_empty() {
                return Ok(());
            }
            let share = zbus::Proxy::new(
                &conn,
                DEST,
                format!("{}/share", device_path(&id)),
                "org.kde.kdeconnect.device.share",
            )
            .await?;
            for uri in &uris {
                share.call_method("shareUrl", &(uri.as_str(),)).await?;
            }
            let what = match uris.len() {
                1 => "1 file".to_string(),
                n => format!("{n} files"),
            };
            // The dashboard is hidden by now, so a toast wouldn't be seen.
            super::osd::Osd::default().show(
                &ctx,
                &format!("Sending {what} to {}", phone.name),
                "document-send",
            );
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_line() {
        let mut p = Phone {
            reachable: true,
            charge: Some(82),
            charging: true,
            ..Default::default()
        };
        assert_eq!(p.status(), "82% · charging");
        p.charging = false;
        assert_eq!(p.status(), "82%");
        p.charge = None;
        assert_eq!(p.status(), "connected");
        p.reachable = false;
        assert_eq!(p.status(), "not connected");
    }
}

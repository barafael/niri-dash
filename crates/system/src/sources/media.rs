//! Now playing, from MPRIS players on the session bus.

use super::{Ctx, Dedup, Trigger, prop, prop_str};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zbus::Connection;
use zbus::zvariant::OwnedValue;

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)]
    Dbus(#[from] zbus::Error),
    #[error(transparent)]
    Fdo(#[from] zbus::fdo::Error),
    #[error("no player")]
    NoPlayer,
}

type Result<T, E = Error> = std::result::Result<T, E>;

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const OBJ: &str = "/org/mpris/MediaPlayer2";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub bus_name: String,
    pub player: String,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub art: Option<std::path::PathBuf>,
    pub progress: f32,
}

async fn player_proxy<'a>(conn: &Connection, name: &'a str) -> Result<zbus::Proxy<'a>> {
    Ok(zbus::Proxy::new(conn, name.to_string(), OBJ, "org.mpris.MediaPlayer2.Player").await?)
}

fn meta_artist(m: &HashMap<String, OwnedValue>) -> String {
    m.get("xesam:artist")
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| Vec::<String>::try_from(v).ok())
        .map(|a| a.join(", "))
        .or_else(|| prop_str(m, "xesam:artist"))
        .unwrap_or_default()
}

fn meta_len(m: &HashMap<String, OwnedValue>) -> Option<i64> {
    prop::<i64>(m, "mpris:length").or_else(|| prop::<u64>(m, "mpris:length").map(|u| u as i64))
}

async fn read_player(conn: &Connection, name: &str) -> Result<State> {
    let p = player_proxy(conn, name).await?;
    let status: String = p.get_property("PlaybackStatus").await?;
    let meta: HashMap<String, OwnedValue> = p.get_property("Metadata").await.unwrap_or_default();
    let position: i64 = p.get_property("Position").await.unwrap_or(0);
    let identity: String = zbus::Proxy::new(conn, name.to_string(), OBJ, "org.mpris.MediaPlayer2")
        .await?
        .get_property("Identity")
        .await
        .unwrap_or_else(|_| name.trim_start_matches(PREFIX).to_string());
    let len = meta_len(&meta);
    let art = prop_str(&meta, "mpris:artUrl")
        .and_then(|u| {
            u.strip_prefix("file://")
                .map(|p| std::path::PathBuf::from(p.replace("%20", " ")))
        })
        .filter(|p| p.is_file());
    Ok(State {
        bus_name: name.to_string(),
        player: identity,
        title: prop_str(&meta, "xesam:title").unwrap_or_default(),
        artist: meta_artist(&meta),
        playing: status == "Playing",
        art,
        progress: len
            .filter(|l| *l > 0)
            .map(|l| position as f32 / l as f32)
            .unwrap_or(0.0),
    })
}

async fn fetch(conn: &Connection) -> Result<Option<State>> {
    let names = zbus::fdo::DBusProxy::new(conn).await?.list_names().await?;
    let mut players = Vec::new();
    for n in names
        .iter()
        .map(|n| n.as_str())
        .filter(|n| n.starts_with(PREFIX) && !n.ends_with(".playerctld"))
    {
        if let Ok(s) = read_player(conn, n).await {
            players.push(s);
        }
    }
    // Prefer a playing player, then one with a title.
    players.sort_by_key(|p| (!p.playing, p.title.is_empty()));
    Ok(players.into_iter().next())
}

#[derive(Clone)]
pub struct Media {
    ctx: Ctx,
    current: Arc<Mutex<String>>,
    sent: Arc<Dedup<Option<State>>>,
}

impl Media {
    pub fn start(ctx: Ctx) -> Self {
        let me = Self {
            ctx: ctx.clone(),
            current: Default::default(),
            sent: Default::default(),
        };
        // Players announce changes (track, play/pause) as PropertiesChanged; new and closed
        // players show up as bus name changes. Only the progress bar needs polling, and only
        // while something plays.
        let trigger = Trigger::default();
        let props = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.freedesktop.DBus.Properties")
            .and_then(|b| b.member("PropertiesChanged"))
            .and_then(|b| b.path(OBJ))
            .map(|b| b.build());
        let names = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.freedesktop.DBus")
            .and_then(|b| b.member("NameOwnerChanged"))
            .and_then(|b| b.arg0ns("org.mpris.MediaPlayer2"))
            .map(|b| b.build());
        for rule in [props, names].into_iter().flatten() {
            ctx.on_dbus_signals(false, rule, Duration::from_millis(100), trigger.clone());
        }
        let m = me.clone();
        ctx.poll_on(trigger, move || {
            let m = m.clone();
            async move {
                let r = async { fetch(&m.conn().await?).await }.await;
                let state = r.unwrap_or_else(|e| {
                    log::debug!("media: {e}");
                    None
                });
                if let Ok(mut c) = m.current.lock() {
                    *c = state
                        .as_ref()
                        .map(|s| s.bus_name.clone())
                        .unwrap_or_default();
                }
                let playing = state.as_ref().is_some_and(|s| s.playing);
                if m.sent.changed(&state) {
                    m.ctx.send(crate::media::Event::State(state));
                }
                if playing {
                    Duration::from_secs(1)
                } else {
                    Duration::from_secs(60)
                }
            }
        });
        me
    }

    async fn conn(&self) -> Result<Connection> {
        Ok(self.ctx.bus(false).await?)
    }

    fn call(&self, method: &'static str) {
        let me = self.clone();
        let name = self.current.lock().map(|c| c.clone()).unwrap_or_default();
        self.ctx.act("Media", async move {
            if name.is_empty() {
                return Err(Error::NoPlayer);
            }
            let conn = me.conn().await?;
            player_proxy(&conn, &name)
                .await?
                .call_method(method, &())
                .await?;
            Ok(())
        });
    }

    pub fn play_pause(&self) {
        self.call("PlayPause");
    }
    pub fn next(&self) {
        self.call("Next");
    }
    pub fn previous(&self) {
        self.call("Previous");
    }
}

//! Now playing, from MPRIS players on the session bus.

use super::{Ctx, Dedup, IconSrc, Mailbox, Trigger, prop, prop_str};
use crate::live::Stateful;
pub use crate::media::State;
use std::collections::HashMap;
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
        art: art.map(IconSrc::Path),
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

pub struct Media {
    ctx: Ctx,
    mailbox: Mailbox<Option<State>>,
    /// The bus name of the player shown, which the buttons go to.
    current: String,
    sent: Dedup<Option<State>>,
}

impl Media {
    pub fn start(ctx: Ctx) -> Self {
        let mut me = Self {
            ctx: ctx.clone(),
            mailbox: Default::default(),
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
        let mut ticks = ctx.ticks(trigger);
        let (c, found) = (ctx.clone(), me.mailbox.sender());
        me.mailbox.spawn(&ctx, async move {
            ticks.ready().await;
            loop {
                let r = async { fetch(&c.bus(false).await?).await }.await;
                let state = r.unwrap_or_else(|e| {
                    log::debug!("media: {e}");
                    None
                });
                let playing = state.as_ref().is_some_and(|s| s.playing);
                let _ = found.send(state);
                ticks
                    .next(if playing {
                        Duration::from_secs(1)
                    } else {
                        Duration::from_secs(60)
                    })
                    .await;
            }
        });
        me
    }

    fn call(&self, method: &'static str) {
        let ctx = self.ctx.clone();
        let name = self.current.clone();
        self.ctx.act("Media", async move {
            if name.is_empty() {
                return Err(Error::NoPlayer);
            }
            let conn = ctx.bus(false).await?;
            player_proxy(&conn, &name)
                .await?
                .call_method(method, &())
                .await?;
            Ok(())
        });
    }
}

impl Stateful for Media {
    type Command = crate::media::Command;
    type Inner = Option<State>;

    fn mailbox(&mut self) -> &mut Mailbox<Option<State>> {
        &mut self.mailbox
    }

    fn on_command(&mut self, cmd: crate::media::Command) {
        use crate::media::Command::*;
        match cmd {
            PlayPause => self.call("PlayPause"),
            Next => self.call("Next"),
            Previous => self.call("Previous"),
        }
    }

    fn on_inner(&mut self, state: Option<State>) {
        self.current = state
            .as_ref()
            .map(|s| s.bus_name.clone())
            .unwrap_or_default();
        if self.sent.changed(&state) {
            self.ctx.send(crate::media::Event::State(state));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::Actor;
    use crate::sources::testing::{ctx, reported, settle};
    use crate::{Event, media};
    use tokio::sync::mpsc::unbounded_channel;

    fn player(name: &str) -> Option<State> {
        Some(State {
            bus_name: name.into(),
            title: "Clair de lune".into(),
            ..Default::default()
        })
    }

    #[tokio::test]
    async fn a_player_found_is_reported_once_and_remembered() {
        let (ctx, events) = ctx();
        let me = Media::start(ctx);
        let found = me.mailbox.sender();
        let _ = found.send(player("org.mpris.MediaPlayer2.a"));
        let _ = found.send(player("org.mpris.MediaPlayer2.a"));
        let (commands, inbox) = unbounded_channel();
        drop(commands);
        let me = me.event_loop(inbox).await;
        assert_eq!(me.current, "org.mpris.MediaPlayer2.a");
        let seen = reported(events);
        assert_eq!(seen.len(), 1);
        assert!(matches!(
            seen[0],
            Event::Media(media::Event::State(Some(_)))
        ));
    }

    #[tokio::test]
    async fn a_player_gone_is_forgotten() {
        let (ctx, _events) = ctx();
        let mut me = Media::start(ctx);
        me.on_inner(player("org.mpris.MediaPlayer2.a"));
        me.on_inner(None);
        assert_eq!(me.current, "");
    }

    #[tokio::test]
    async fn the_buttons_say_so_when_there_is_no_player() {
        let (ctx, events) = ctx();
        let me = Media::start(ctx);
        let (commands, inbox) = unbounded_channel();
        let _ = commands.send(media::Command::PlayPause);
        drop(commands);
        me.event_loop(inbox).await;
        settle().await;
        let seen = reported(events);
        assert!(
            matches!(&seen[..], [Event::Toast(t)] if t.starts_with("Media")),
            "{} reports",
            seen.len()
        );
    }
}

//! KDE Plasma's own services on the session bus, for what niri does over its IPC: keyboard layouts
//! (KWin's `org.kde.keyboard`), the logout prompt, KWin's color picker and night light, and
//! Plasma's on-screen messages.
#![allow(clippy::disallowed_types)] // Still to be moved over to an actor.

use super::{Ctx, Dedup, Trigger, keyboard, prop};
pub use crate::night_light::NightLightState;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::watch;
use zbus::zvariant::OwnedValue;

/// zbus errors are boxed: a `zbus::Error` is large, and unboxed it would make every `Result`
/// here as big.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Dbus(Box<zbus::Error>),
    /// KWin turned the color picker down; KWin's message.
    #[error("KWin refused: {0}")]
    PickRefused(String),
}

impl From<zbus::Error> for Error {
    fn from(e: zbus::Error) -> Self {
        Error::Dbus(Box::new(e))
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

const KWIN: &str = "org.kde.KWin";

/// Whether KWin is on the session bus (a Plasma session that doesn't say so in its environment).
pub async fn kwin_running() -> bool {
    async {
        let conn = zbus::Connection::session().await?;
        let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
        Ok::<_, zbus::Error>(dbus.name_has_owner(KWIN.try_into()?).await?)
    }
    .await
    .unwrap_or(false)
}

/// Signals of one interface at one path, from `sender`.
fn signals(
    sender: &'static str,
    path: &'static str,
    interface: &'static str,
) -> zbus::MatchRule<'static> {
    zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(sender)
        .and_then(|b| b.path(path))
        .and_then(|b| b.interface(interface))
        .expect("valid match rule")
        .build()
}

// Keyboard layouts.

const KEYBOARD: (&str, &str, &str) = ("org.kde.keyboard", "/Layouts", "org.kde.KeyboardLayouts");

/// Plasma's (short name, display name, long name) triples as the widget shows them: the display
/// name is the label the user gave a layout in System Settings, if any.
fn layouts(list: Vec<(String, String, String)>, current: u32) -> keyboard::Layouts {
    keyboard::Layouts {
        layouts: list
            .into_iter()
            .map(|(short, display, long)| keyboard::Layout {
                short: if display.is_empty() { short } else { display }.to_uppercase(),
                name: long,
            })
            .collect(),
        current: current as usize,
    }
}

async fn fetch_layouts(ctx: &Ctx) -> Result<keyboard::Layouts> {
    let (dest, path, iface) = KEYBOARD;
    let conn = ctx.bus(false).await?;
    let list: Vec<(String, String, String)> = conn
        .call_method(Some(dest), path, Some(iface), "getLayoutsList", &())
        .await?
        .body()
        .deserialize()?;
    let current: u32 = conn
        .call_method(Some(dest), path, Some(iface), "getLayout", &())
        .await?
        .body()
        .deserialize()?;
    Ok(layouts(list, current))
}

/// Report Plasma's keyboard layouts, again whenever they change.
pub fn start_keyboard(ctx: Ctx) {
    let trigger = Trigger::default();
    let (dest, path, iface) = KEYBOARD;
    ctx.on_dbus_signals(
        false,
        signals(dest, path, iface),
        Duration::from_millis(50),
        trigger.clone(),
    );
    let c = ctx.clone();
    let mut ticks = ctx.ticks(trigger);
    ctx.spawn(async move {
        let sent = Dedup::default();
        ticks.ready().await;
        loop {
            let layouts = fetch_layouts(&c)
                .await
                .map_err(|e| log::debug!("keyboard layouts: {e}"))
                .ok();
            if sent.changed(&layouts) {
                c.send(crate::keyboard::Event::Layouts(layouts));
            }
            ticks.next(Duration::from_secs(60)).await;
        }
    });
}

pub fn switch_layout(ctx: &Ctx, to: keyboard::Switch) {
    let c = ctx.clone();
    ctx.act("Switch layout", async move {
        let (dest, path, iface) = KEYBOARD;
        let conn = c.bus(false).await?;
        match to {
            keyboard::Switch::Next => {
                conn.call_method(Some(dest), path, Some(iface), "switchToNextLayout", &())
                    .await?
            }
            keyboard::Switch::Prev => {
                conn.call_method(Some(dest), path, Some(iface), "switchToPreviousLayout", &())
                    .await?
            }
            keyboard::Switch::Index(i) => {
                conn.call_method(Some(dest), path, Some(iface), "setLayout", &(i,))
                    .await?
            }
        };
        Ok::<_, Error>(())
    });
}

// Session.

/// Plasma's logout screen, which asks before it logs out (as niri's dialog does).
pub async fn logout(ctx: &Ctx) -> Result<()> {
    ctx.bus(false)
        .await?
        .call_method(
            Some("org.kde.LogoutPrompt"),
            "/LogoutPrompt",
            Some("org.kde.LogoutPrompt"),
            "promptLogout",
            &(),
        )
        .await?;
    Ok(())
}

// Color picker.

/// An ARGB value as KWin's color picker returns it, as RGB fractions.
fn argb_to_rgb(argb: u32) -> [f64; 3] {
    [16, 8, 0].map(|shift| ((argb >> shift) & 0xff) as f64 / 255.0)
}

/// Let the user click a pixel anywhere with KWin's color picker; `None` if they cancelled.
pub async fn pick_color(ctx: &Ctx) -> Result<Option<[f64; 3]>> {
    let conn = ctx.bus(false).await?;
    let reply = conn
        .call_method(
            Some(KWIN),
            "/ColorPicker",
            Some("org.kde.kwin.ColorPicker"),
            "pick",
            &(),
        )
        .await;
    match reply {
        Ok(msg) => {
            let (argb,): (u32,) = msg.body().deserialize()?;
            Ok(Some(argb_to_rgb(argb)))
        }
        Err(zbus::Error::MethodError(name, _, _)) if name.ends_with(".Cancelled") => Ok(None),
        // KWin may limit the picker to programs whose desktop file allows it
        // (X-KDE-DBUS-Restricted-Interfaces).
        Err(zbus::Error::MethodError(name, text, _)) => Err(Error::PickRefused(
            text.filter(|t| !t.is_empty())
                .unwrap_or_else(|| name.to_string()),
        )),
        Err(e) => Err(e.into()),
    }
}

// On-screen messages.

/// Show `text` in Plasma's on-screen display, with an icon name from the icon theme.
pub async fn show_osd(ctx: &Ctx, text: &str, icon: &str) -> Result<()> {
    ctx.bus(false)
        .await?
        .call_method(
            Some("org.kde.plasmashell"),
            "/org/kde/osdService",
            Some("org.kde.osdService"),
            "showText",
            &(icon, text),
        )
        .await?;
    Ok(())
}

// Night light.

const NIGHT_LIGHT: (&str, &str, &str) = (
    "org.kde.KWin.NightLight",
    "/org/kde/KWin/NightLight",
    "org.kde.KWin.NightLight",
);

fn night_light_state(props: &HashMap<String, OwnedValue>, ours: bool) -> NightLightState {
    let flag = |k| prop::<bool>(props, k).unwrap_or(false);
    NightLightState {
        available: flag("available"),
        enabled: flag("enabled"),
        inhibited: flag("inhibited"),
        paused_by_us: ours,
        temperature: prop::<u32>(props, "currentTemperature")
            .unwrap_or(crate::night_light::NEUTRAL),
    }
}

/// Pausing KWin's night light: KWin keeps it paused while we hold the cookie, and lets go by
/// itself when our bus connection closes (the daemon exits).
struct Pause {
    conn: zbus::Connection,
    cookie: u32,
}

#[derive(Clone)]
pub struct NightLight {
    ctx: Ctx,
    /// Whether we want KWin's night light paused.
    pause: watch::Sender<bool>,
    /// Whether we hold a pause right now.
    ours: Arc<AtomicBool>,
}

impl NightLight {
    pub fn start(ctx: Ctx) -> Self {
        let trigger = Trigger::default();
        let (dest, path, _) = NIGHT_LIGHT;
        ctx.on_dbus_signals(
            false,
            signals(dest, path, "org.freedesktop.DBus.Properties"),
            Duration::from_millis(100),
            trigger.clone(),
        );
        let ours = Arc::new(AtomicBool::new(false));
        let me = Self {
            ctx: ctx.clone(),
            pause: watch::Sender::new(false),
            ours: ours.clone(),
        };
        let m = me.clone();
        let sent = Arc::new(Dedup::default());
        ctx.poll_events(trigger.clone(), Duration::from_secs(60), move || {
            let (m, sent) = (m.clone(), sent.clone());
            async move {
                let state = m.state().await.unwrap_or_else(|e| {
                    log::debug!("KWin night light: {e}");
                    NightLightState::default()
                });
                if sent.changed(&state) {
                    m.ctx.send(crate::night_light::Event::Kwin(state));
                }
            }
        });
        let mut rx = me.pause.subscribe();
        let c = ctx.clone();
        ctx.spawn(async move {
            let mut held: Option<Pause> = None;
            loop {
                let want = *rx.borrow_and_update();
                let result = match (want, held.take()) {
                    (true, None) => take_pause(&c).await.map(|p| held = Some(p)),
                    (false, Some(p)) => release_pause(p).await,
                    (_, kept) => {
                        held = kept;
                        Ok(())
                    }
                };
                if let Err(e) = result {
                    log::warn!("KWin night light: {e}");
                    c.toast(format!("Night light: {e}"));
                }
                ours.store(held.is_some(), Ordering::Relaxed);
                trigger.fire();
                if rx.changed().await.is_err() {
                    return;
                }
            }
        });
        me
    }

    /// KWin's night light right now.
    async fn state(&self) -> Result<NightLightState> {
        let (dest, path, iface) = NIGHT_LIGHT;
        let conn = self.ctx.bus(false).await?;
        let p = zbus::fdo::PropertiesProxy::builder(&conn)
            .destination(dest)?
            .path(path)?
            .build()
            .await?;
        let iface = iface.try_into().map_err(zbus::Error::from)?;
        let props = p.get_all(iface).await.map_err(zbus::Error::from)?;
        // A pause asked for but not taken yet counts as ours.
        let ours = self.ours.load(Ordering::Relaxed) || *self.pause.borrow();
        Ok(night_light_state(&props, ours))
    }

    /// Carry out `niri-dash night-light …` or a click on the widget: on lets KWin's schedule run,
    /// off pauses it. The temperature is KWin's (System Settings), so a temperature only turns it
    /// on. With `announce`, the result shows on screen; otherwise only problems do, as a toast.
    pub fn apply(&self, cmd: crate::night_light::Cmd, announce: bool) {
        use crate::night_light::Cmd;
        let me = self.clone();
        self.ctx.spawn(async move {
            let s = match me.state().await {
                Ok(s) => s,
                Err(e) => {
                    me.ctx.toast(format!("Night light: {e}"));
                    return;
                }
            };
            let want_on = match cmd {
                Cmd::Toggle => !s.on(),
                Cmd::On | Cmd::Kelvin(_) => true,
                Cmd::Off => false,
            };
            let (text, problem) = if !s.available {
                ("KWin's night light isn't available", true)
            } else if !s.enabled {
                ("Night light is turned off in System Settings", true)
            } else if want_on && s.inhibited && !s.paused_by_us {
                ("Night light is paused by another program", true)
            } else {
                me.pause.send_replace(!want_on);
                match (want_on, cmd) {
                    (true, Cmd::Kelvin(_)) => ("Night light on · Plasma sets its warmth", false),
                    (true, _) => ("Night light on", false),
                    (false, _) => ("Night light paused", false),
                }
            };
            if announce {
                if let Err(e) = show_osd(&me.ctx, text, "weather-clear-night").await {
                    log::debug!("plasma osd: {e}");
                }
            } else if problem {
                me.ctx.toast(text);
            }
        });
    }
}

async fn take_pause(ctx: &Ctx) -> Result<Pause> {
    let (dest, path, iface) = NIGHT_LIGHT;
    let conn = ctx.bus(false).await?;
    let cookie: u32 = conn
        .call_method(Some(dest), path, Some(iface), "inhibit", &())
        .await?
        .body()
        .deserialize()?;
    log::info!("KWin night light paused");
    Ok(Pause { conn, cookie })
}

async fn release_pause(p: Pause) -> Result<()> {
    let (dest, path, iface) = NIGHT_LIGHT;
    p.conn
        .call_method(Some(dest), path, Some(iface), "uninhibit", &(p.cookie,))
        .await?;
    log::info!("KWin night light resumed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plasma_layouts() {
        let l = layouts(
            vec![
                ("us".into(), "".into(), "English (USA)".into()),
                ("de".into(), "neo".into(), "Deutsch (Neo 2)".into()),
            ],
            1,
        );
        assert_eq!(l.current, 1);
        assert_eq!(l.layouts[0].short, "US");
        assert_eq!(l.layouts[0].name, "English (USA)");
        assert_eq!(l.layouts[1].short, "NEO");
    }

    #[test]
    fn picked_colors() {
        assert_eq!(argb_to_rgb(0xff_ff_80_00), [1.0, 128.0 / 255.0, 0.0]);
    }

    #[test]
    fn night_light_props() {
        let mut props = HashMap::new();
        for (k, v) in [("available", true), ("enabled", true), ("inhibited", false)] {
            props.insert(k.to_string(), OwnedValue::from(v));
        }
        props.insert("currentTemperature".into(), OwnedValue::from(4200u32));
        let s = night_light_state(&props, false);
        assert!(s.on());
        assert_eq!(s.temperature, 4200);
        props.insert("inhibited".into(), OwnedValue::from(true));
        assert!(!night_light_state(&props, true).on());
    }
}

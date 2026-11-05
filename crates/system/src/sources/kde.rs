//! KDE Plasma's own services on the session bus, for what niri does over its IPC: keyboard layouts
//! (KWin's `org.kde.keyboard`), the logout prompt, KWin's color picker and night light, and
//! Plasma's on-screen messages.

use super::{Ctx, Dedup, Mailbox, Trigger, keyboard, prop};
pub use crate::night_light::NightLightState;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;
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

/// KWin's night light properties, as read.
type Props = HashMap<String, OwnedValue>;

/// What the night light's tasks tell it.
pub enum NightLightInner {
    /// KWin's state, for the widget.
    Read(Result<Props>),
    /// Whether we hold a pause now.
    Held(bool),
    /// KWin's state, to decide what a command means.
    Decide {
        cmd: crate::night_light::Cmd,
        announce: bool,
        read: Result<Props>,
    },
}

/// What a command comes to.
#[derive(Debug, PartialEq)]
struct Decision {
    text: &'static str,
    /// Something got in the way: it shows as a toast even if not announcing.
    problem: bool,
    /// Whether we should want KWin's night light paused; `None` leaves that as it is.
    pause: Option<bool>,
}

/// On lets KWin's schedule run, off pauses it. The temperature is KWin's (System Settings), so a
/// temperature only turns it on.
fn decide(cmd: crate::night_light::Cmd, s: NightLightState) -> Decision {
    use crate::night_light::Cmd;
    let want_on = match cmd {
        Cmd::Toggle => !s.on(),
        Cmd::On | Cmd::Kelvin(_) => true,
        Cmd::Off => false,
    };
    let problem = |text| Decision {
        text,
        problem: true,
        pause: None,
    };
    if !s.available {
        problem("KWin's night light isn't available")
    } else if !s.enabled {
        problem("Night light is turned off in System Settings")
    } else if want_on && s.inhibited && !s.paused_by_us {
        problem("Night light is paused by another program")
    } else {
        Decision {
            text: match (want_on, cmd) {
                (true, Cmd::Kelvin(_)) => "Night light on · Plasma sets its warmth",
                (true, _) => "Night light on",
                (false, _) => "Night light paused",
            },
            problem: false,
            pause: Some(!want_on),
        }
    }
}

/// KWin's night light right now.
async fn read_props(ctx: &Ctx) -> Result<Props> {
    let (dest, path, iface) = NIGHT_LIGHT;
    let conn = ctx.bus(false).await?;
    let p = zbus::fdo::PropertiesProxy::builder(&conn)
        .destination(dest)?
        .path(path)?
        .build()
        .await?;
    let iface = iface.try_into().map_err(zbus::Error::from)?;
    Ok(p.get_all(iface).await.map_err(zbus::Error::from)?)
}

/// KWin's night light: the part of the night light actor that is only there on Plasma.
pub struct NightLight {
    ctx: Ctx,
    /// Where tasks send what they found.
    found: UnboundedSender<NightLightInner>,
    /// Whether we want KWin's night light paused.
    pause: watch::Sender<bool>,
    /// Whether we hold a pause right now.
    ours: bool,
    sent: Dedup<NightLightState>,
}

impl NightLight {
    pub fn start(ctx: Ctx, mailbox: &mut Mailbox<NightLightInner>) -> Self {
        let trigger = Trigger::default();
        let (dest, path, _) = NIGHT_LIGHT;
        ctx.on_dbus_signals(
            false,
            signals(dest, path, "org.freedesktop.DBus.Properties"),
            Duration::from_millis(100),
            trigger.clone(),
        );
        let me = Self {
            ctx: ctx.clone(),
            found: mailbox.sender(),
            pause: watch::Sender::new(false),
            ours: false,
            sent: Default::default(),
        };
        let mut ticks = ctx.ticks(trigger.clone());
        let (c, found) = (ctx.clone(), mailbox.sender());
        mailbox.spawn(&ctx, async move {
            ticks.ready().await;
            loop {
                let _ = found.send(NightLightInner::Read(read_props(&c).await));
                ticks.next(Duration::from_secs(60)).await;
            }
        });
        // Taking and letting go of the pause, for the latest wish, one at a time.
        let mut rx = me.pause.subscribe();
        let (c, found) = (ctx.clone(), mailbox.sender());
        mailbox.spawn(&ctx, async move {
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
                // Before the fetch this sets off, so that fetch reads as ours.
                let _ = found.send(NightLightInner::Held(held.is_some()));
                trigger.fire();
                if rx.changed().await.is_err() {
                    return;
                }
            }
        });
        me
    }

    /// Carry out `niri-dash night-light …` or a click on the widget. With `announce`, the result
    /// shows on screen; otherwise only problems do, as a toast.
    pub fn apply(&self, cmd: crate::night_light::Cmd, announce: bool) {
        let (c, found) = (self.ctx.clone(), self.found.clone());
        self.ctx.spawn(async move {
            let read = read_props(&c).await;
            let _ = found.send(NightLightInner::Decide {
                cmd,
                announce,
                read,
            });
        });
    }

    /// `props` as the widget should see them. A pause asked for but not taken yet counts as ours.
    fn state(&self, props: &Props) -> NightLightState {
        night_light_state(props, self.ours || *self.pause.borrow())
    }

    pub fn on_inner(&mut self, inner: NightLightInner) {
        match inner {
            NightLightInner::Held(held) => self.ours = held,
            NightLightInner::Read(read) => {
                let state = read.map_or_else(
                    |e| {
                        log::debug!("KWin night light: {e}");
                        NightLightState::default()
                    },
                    |props| self.state(&props),
                );
                if self.sent.changed(&state) {
                    self.ctx.send(crate::night_light::Event::Kwin(state));
                }
            }
            NightLightInner::Decide {
                cmd,
                announce,
                read,
            } => {
                let props = match read {
                    Ok(props) => props,
                    Err(e) => {
                        self.ctx.toast(format!("Night light: {e}"));
                        return;
                    }
                };
                let decision = decide(cmd, self.state(&props));
                if let Some(pause) = decision.pause {
                    self.pause.send_replace(pause);
                }
                if announce {
                    let c = self.ctx.clone();
                    self.ctx.spawn(async move {
                        if let Err(e) = show_osd(&c, decision.text, "weather-clear-night").await {
                            log::debug!("plasma osd: {e}");
                        }
                    });
                } else if decision.problem {
                    self.ctx.toast(decision.text);
                }
            }
        }
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

    fn kwin(enabled: bool, inhibited: bool, paused_by_us: bool) -> NightLightState {
        NightLightState {
            available: true,
            enabled,
            inhibited,
            paused_by_us,
            temperature: 4200,
        }
    }

    #[test]
    fn what_commands_come_to() {
        use crate::night_light::Cmd;
        let on = kwin(true, false, false);
        let paused = kwin(true, true, true);
        // (command, state, resulting pause wish)
        for (cmd, state, pause) in [
            (Cmd::Toggle, on, true),
            (Cmd::Off, on, true),
            (Cmd::On, on, false),
            (Cmd::Toggle, paused, false),
            (Cmd::On, paused, false),
            (Cmd::Kelvin(4000), paused, false),
        ] {
            let d = decide(cmd, state);
            assert_eq!(d.pause, Some(pause), "{cmd:?} on {state:?}");
            assert!(!d.problem);
        }
    }

    #[test]
    fn what_gets_in_the_way() {
        use crate::night_light::Cmd;
        let gone = NightLightState {
            available: false,
            ..kwin(true, false, false)
        };
        let off_in_settings = kwin(false, false, false);
        let paused_by_another = kwin(true, true, false);
        for (cmd, state) in [
            (Cmd::On, gone),
            (Cmd::Off, gone),
            (Cmd::On, off_in_settings),
            (Cmd::On, paused_by_another),
            (Cmd::Toggle, paused_by_another),
        ] {
            let d = decide(cmd, state);
            assert!(d.problem, "{cmd:?} on {state:?}");
            assert_eq!(d.pause, None, "must leave the pause alone");
        }
        // Turning off what another program paused is no problem: we pause it too.
        assert_eq!(decide(Cmd::Off, paused_by_another).pause, Some(true));
    }

    fn props(inhibited: bool) -> Props {
        let mut props = HashMap::new();
        for (k, v) in [
            ("available", true),
            ("enabled", true),
            ("inhibited", inhibited),
        ] {
            props.insert(k.to_string(), OwnedValue::from(v));
        }
        props
    }

    fn night_light(ctx: Ctx) -> (NightLight, Mailbox<NightLightInner>) {
        let mut mailbox = Mailbox::default();
        (NightLight::start(ctx, &mut mailbox), mailbox)
    }

    /// The pause we hold shows as ours in what the widget gets, and a read that says the same again
    /// isn't reported again.
    #[tokio::test]
    async fn a_pause_held_reads_as_ours() {
        let (ctx, events) = crate::sources::testing::ctx();
        let (mut me, _mailbox) = night_light(ctx);
        me.on_inner(NightLightInner::Held(true));
        me.on_inner(NightLightInner::Read(Ok(props(true))));
        me.on_inner(NightLightInner::Read(Ok(props(true))));
        let seen = crate::sources::testing::reported(events);
        assert_eq!(seen.len(), 1);
        let crate::Event::NightLight(crate::night_light::Event::Kwin(s)) = seen[0] else {
            panic!("reported something else");
        };
        assert!(s.paused_by_us);
    }

    /// A pause asked for but not taken yet counts as ours: the switch doesn't flicker.
    #[tokio::test]
    async fn a_pause_asked_for_reads_as_ours() {
        let (ctx, _events) = crate::sources::testing::ctx();
        let (mut me, _mailbox) = night_light(ctx);
        me.on_inner(NightLightInner::Decide {
            cmd: crate::night_light::Cmd::Off,
            announce: false,
            read: Ok(props(false)),
        });
        assert!(*me.pause.borrow());
        assert!(me.state(&props(true)).paused_by_us);
    }

    /// When we could not even read KWin's state, nothing changes and it says why.
    #[tokio::test]
    async fn a_read_that_failed_only_says_so() {
        let (ctx, events) = crate::sources::testing::ctx();
        let (mut me, _mailbox) = night_light(ctx);
        me.on_inner(NightLightInner::Decide {
            cmd: crate::night_light::Cmd::Off,
            announce: true,
            read: Err(zbus::Error::Failure("gone".into()).into()),
        });
        assert!(!*me.pause.borrow());
        let seen = crate::sources::testing::reported(events);
        assert!(matches!(&seen[..], [crate::Event::Toast(t)] if t.contains("gone")));
    }
}

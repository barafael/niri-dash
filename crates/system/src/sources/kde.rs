//! KDE Plasma's own services on the session bus, for what niri does over its IPC: keyboard layouts
//! (KWin's `org.kde.keyboard`), the logout prompt, KWin's color picker and night light, its windows
//! (through short-lived scripts), and Plasma's on-screen messages.

use super::{Ctx, Dedup, Trigger, keyboard, prop};
pub use crate::night_light::NightLightState;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{oneshot, watch};
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
    /// Nowhere private to put a script for KWin to read.
    #[error("no runtime or cache directory for a KWin script")]
    NoScriptDir,
    #[error("writing a KWin script: {0}")]
    ScriptFile(std::io::Error),
    #[error("KWin wouldn't load a script")]
    ScriptNotLoaded,
    #[error("no answer from a KWin script")]
    ScriptSilent,
    #[error("a KWin script answered {0:?}")]
    ScriptAnswer(String),
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
    let sent = Arc::new(Dedup::default());
    ctx.poll_events(trigger, Duration::from_secs(60), move || {
        let (c, sent) = (c.clone(), sent.clone());
        async move {
            let layouts = fetch_layouts(&c)
                .await
                .map_err(|e| log::debug!("keyboard layouts: {e}"))
                .ok();
            if sent.changed(&layouts) {
                c.send(crate::keyboard::Event::Layouts(layouts));
            }
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

// Windows.

/// How long a script gets to answer: it only looks through KWin's windows.
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(2);
const SCRIPTING: &str = "org.kde.kwin.Scripting";
const REPORT_INTERFACE: &str = "org.niri_dash.KWinScript";

/// Where a running script sends its answer.
struct ScriptReport(Option<oneshot::Sender<String>>);

#[zbus::interface(name = "org.niri_dash.KWinScript")]
impl ScriptReport {
    fn report(&mut self, answer: String) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(answer);
        }
    }
}

/// A script that runs `body` (a function body) and sends what it returns, as JSON, to `report`.
fn script_source(me: &str, report: &str, body: &str) -> String {
    format!(
        "const answer = (function () {{ {body} }})();\n\
         callDBus(\"{me}\", \"{report}\", \"{REPORT_INTERFACE}\", \"Report\", JSON.stringify(answer));\n"
    )
}

/// KWin has no call for its windows, only scripts can get at them: run `body` in one, and return
/// its answer. KWin reads the script from a file, so the file goes in the runtime directory, which
/// only we can write to: anything that could put one there would be running code inside KWin.
async fn run_script(conn: &zbus::Connection, body: &str) -> Result<String> {
    static RUNS: AtomicU64 = AtomicU64::new(0);
    let n = RUNS.fetch_add(1, Ordering::Relaxed);
    let name = format!("niri-dash-{}-{n}", std::process::id());
    let file = dirs::runtime_dir()
        .or_else(dirs::cache_dir)
        .ok_or(Error::NoScriptDir)?
        .join(format!("{name}.js"));
    let report = format!("/org/niri_dash/KWinScript/{n}");
    let me = conn
        .unique_name()
        .ok_or(Error::ScriptNotLoaded)?
        .to_string();
    let (tx, answer) = oneshot::channel();
    conn.object_server()
        .at(report.as_str(), ScriptReport(Some(tx)))
        .await?;
    std::fs::write(&file, script_source(&me, &report, body)).map_err(Error::ScriptFile)?;
    let result = async {
        let path = file.to_string_lossy();
        let loaded = conn
            .call_method(
                Some(KWIN),
                "/Scripting",
                Some(SCRIPTING),
                "loadScript",
                &(path.as_ref(), name.as_str()),
            )
            .await?;
        let (id,): (i32,) = loaded.body().deserialize()?;
        if id < 0 {
            return Err(Error::ScriptNotLoaded);
        }
        conn.call_method(
            Some(KWIN),
            format!("/Scripting/Script{id}").as_str(),
            Some("org.kde.kwin.Script"),
            "run",
            &(),
        )
        .await?;
        // Not before this: KWin reads the file some time after being told to run it.
        tokio::time::timeout(SCRIPT_TIMEOUT, answer)
            .await
            .map_err(|_| Error::ScriptSilent)?
            .map_err(|_| Error::ScriptSilent)
    }
    .await;
    let _ = conn
        .call_method(
            Some(KWIN),
            "/Scripting",
            Some(SCRIPTING),
            "unloadScript",
            &(name.as_str(),),
        )
        .await;
    let _ = std::fs::remove_file(&file);
    let _ = conn
        .object_server()
        .remove::<ScriptReport, _>(report.as_str())
        .await;
    result
}

/// A window's id for the rest of the system, which counts windows in `u64`s: the first half of
/// KWin's UUID, which is plenty to tell a desktop's windows apart. `window_key` finds it again.
fn window_id(uuid: &str) -> Option<u64> {
    let hex: String = uuid.chars().filter(char::is_ascii_hexdigit).collect();
    u64::from_str_radix(hex.get(..16)?, 16).ok()
}

/// Script code for what `window_id` makes of window `w`, written out as `format!("{id:016x}")`.
const WINDOW_KEY: &str =
    r#"w.internalId.toString().replace(/[^0-9a-fA-F]/g, "").slice(0, 16).toLowerCase()"#;

/// Every ordinary window (not a panel or a dialog's shadow) by the pid of its process. A process
/// with several keeps one of them.
async fn list_windows(conn: &zbus::Connection) -> Result<HashMap<i32, u64>> {
    let answer = run_script(
        conn,
        "return workspace.windowList()\
         .filter(w => w.normalWindow && w.pid > 0)\
         .map(w => [w.pid, w.internalId.toString()]);",
    )
    .await?;
    let windows: Vec<(i32, String)> =
        serde_json::from_str(&answer).map_err(|_| Error::ScriptAnswer(answer.clone()))?;
    Ok(windows
        .into_iter()
        .filter_map(|(pid, uuid)| Some((pid, window_id(&uuid)?)))
        .collect())
}

pub async fn windows_by_pid(ctx: &Ctx) -> Result<HashMap<i32, u64>> {
    list_windows(&ctx.bus(false).await?).await
}

/// Bring up the first window `pick` (script code about `w`) holds for: from another desktop, and
/// out of being minimized, too. If there is none it has gone, and there is nothing to do.
async fn activate(conn: &zbus::Connection, pick: &str) -> Result<()> {
    run_script(
        conn,
        &format!(
            "const found = workspace.windowList().find(w => {pick});\
             if (found) {{ found.minimized = false; workspace.activeWindow = found; }}\
             return found !== undefined;"
        ),
    )
    .await
    .map(|_| ())
}

async fn activate_window(conn: &zbus::Connection, id: u64) -> Result<()> {
    activate(conn, &format!(r#"{WINDOW_KEY} === "{id:016x}""#)).await
}

pub fn focus_window(ctx: &Ctx, id: u64) {
    let c = ctx.clone();
    ctx.act("Focus window", async move {
        activate_window(&c.bus(false).await?, id).await
    });
}

/// Focus the window of process `pid`, if it has one.
pub fn focus_pid(ctx: &Ctx, pid: i32) {
    let c = ctx.clone();
    ctx.act("Focus window", async move {
        let conn = c.bus(false).await?;
        activate(&conn, &format!("w.normalWindow && w.pid === {pid}")).await
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
    fn window_ids_are_the_first_half_of_the_uuid() {
        let uuid = "{0123abcd-4567-89ef-0011-223344556677}";
        assert_eq!(window_id(uuid), Some(0x0123_abcd_4567_89ef));
        assert_eq!(
            format!("{:016x}", window_id(uuid).unwrap()),
            "0123abcd456789ef"
        );
        // Leading zeros stay, as the script compares text.
        assert_eq!(
            format!(
                "{:016x}",
                window_id("{00000000-0000-00ab-0011-223344556677}").unwrap()
            ),
            "00000000000000ab"
        );
        assert_eq!(window_id("{0123}"), None);
    }

    /// Against a real KWin, whose focus it moves around: start a throwaway one with two windows
    /// and run this in its D-Bus session, with NIRI_DASH_TEST_KWIN=1.
    #[tokio::test]
    #[ignore = "needs a KWin to change the focus of"]
    async fn windows_in_a_real_kwin() {
        assert!(
            std::env::var_os("NIRI_DASH_TEST_KWIN").is_some(),
            "this moves the focus around: set NIRI_DASH_TEST_KWIN=1 in a throwaway session"
        );
        let conn = zbus::Connection::session().await.unwrap();
        let windows = list_windows(&conn).await.unwrap();
        assert!(windows.len() >= 2, "windows: {windows:?}");
        let active = async || {
            let answer = run_script(
                &conn,
                &format!(
                    "const w = workspace.activeWindow;\
                     return w ? [{WINDOW_KEY}, w.minimized, w.desktops[0] === workspace.currentDesktop] : null;"
                ),
            )
            .await
            .unwrap();
            serde_json::from_str::<Option<(String, bool, bool)>>(&answer).unwrap()
        };
        let ids: Vec<u64> = windows.values().copied().collect();
        let (a, b) = (ids[0], ids[1]);
        let key = |id: u64| format!("{id:016x}");
        for id in [a, b, a] {
            activate_window(&conn, id).await.unwrap();
            assert_eq!(active().await, Some((key(id), false, true)));
        }
        // Out of being minimized.
        run_script(
            &conn,
            &format!(
                r#"workspace.windowList().find(w => {WINDOW_KEY} === "{}").minimized = true; return 1;"#,
                key(b)
            ),
        )
        .await
        .unwrap();
        activate_window(&conn, b).await.unwrap();
        assert_eq!(active().await, Some((key(b), false, true)));
        // From another desktop.
        run_script(
            &conn,
            &format!(
                r#"workspace.createDesktop(1, "other");
                   const other = workspace.desktops[1];
                   workspace.windowList().find(w => {WINDOW_KEY} === "{}").desktops = [other];
                   workspace.currentDesktop = workspace.desktops[0];
                   return 1;"#,
                key(a)
            ),
        )
        .await
        .unwrap();
        activate_window(&conn, a).await.unwrap();
        assert_eq!(active().await, Some((key(a), false, true)));
        // A window that has gone is nothing to do, and an answer still comes.
        activate_window(&conn, 1).await.unwrap();
        // Nothing left behind for KWin.
        let left: Vec<_> = std::fs::read_dir(dirs::runtime_dir().unwrap())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("niri-dash-"))
            .filter(|e| e.file_name().to_string_lossy().ends_with(".js"))
            .collect();
        assert!(left.is_empty(), "left behind: {left:?}");
    }

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

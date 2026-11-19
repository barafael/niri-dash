//! Screen brightness. A backlight (laptop panels) is read from sysfs and written through logind's
//! Session.SetBrightness (no root needed). Without one, on Plasma, through PowerDevil's
//! org.kde.ScreenBrightness, which also covers external monitors (DDC/CI, or KWin dimming an HDR
//! screen in software).

use super::{Ctx, Dedup, Trigger, sysfs};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::watch;
use zbus::zvariant::OwnedValue;

const PLASMA: &str = "org.kde.Solid.PowerManagement";
const PLASMA_PATH: &str = "/org/kde/ScreenBrightness";
const PLASMA_IFACE: &str = "org.kde.ScreenBrightness";
const PLASMA_DISPLAY: &str = "org.kde.ScreenBrightness.Display";

fn device() -> Option<PathBuf> {
    std::fs::read_dir("/sys/class/backlight")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .next()
}

fn sysfs_level(dev: &std::path::Path) -> Option<f32> {
    let max: u32 = sysfs(dev.join("max_brightness"))?;
    Some(sysfs::<u32>(dev.join("brightness"))? as f32 / max.max(1) as f32)
}

async fn level(ctx: &Ctx) -> Option<f32> {
    if let Some(dev) = device() {
        return sysfs_level(&dev);
    }
    if !ctx.is_kwin() {
        return None;
    }
    plasma_level(ctx)
        .await
        .map_err(|e| log::debug!("Plasma brightness: {e}"))
        .ok()
        .flatten()
}

/// A property of a PowerDevil brightness object.
async fn plasma_prop<T>(
    conn: &zbus::Connection,
    path: &str,
    iface: &str,
    name: &str,
) -> zbus::Result<T>
where
    T: TryFrom<OwnedValue>,
    T::Error: Into<zbus::Error>,
{
    let v: OwnedValue = conn
        .call_method(
            Some(PLASMA),
            path,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(iface, name),
        )
        .await?
        .body()
        .deserialize()?;
    T::try_from(v).map_err(Into::into)
}

/// PowerDevil's displays, as object paths.
async fn plasma_displays(conn: &zbus::Connection) -> zbus::Result<Vec<String>> {
    let names: Vec<String> =
        plasma_prop(conn, PLASMA_PATH, PLASMA_IFACE, "DisplaysDBusNames").await?;
    Ok(names
        .into_iter()
        .map(|n| format!("{PLASMA_PATH}/{n}"))
        .collect())
}

/// The first display's brightness; None if PowerDevil has no displays to control.
async fn plasma_level(ctx: &Ctx) -> zbus::Result<Option<f32>> {
    let conn = ctx.bus(false).await?;
    let Some(path) = plasma_displays(&conn).await?.into_iter().next() else {
        return Ok(None);
    };
    let cur: i32 = plasma_prop(&conn, &path, PLASMA_DISPLAY, "Brightness").await?;
    let max: i32 = plasma_prop(&conn, &path, PLASMA_DISPLAY, "MaxBrightness").await?;
    Ok(Some(cur as f32 / max.max(1) as f32))
}

/// Set every display PowerDevil knows to `v` (0..1).
async fn plasma_apply(ctx: &Ctx, v: f32) -> zbus::Result<()> {
    let conn = ctx.bus(false).await?;
    for path in plasma_displays(&conn).await? {
        let max: i32 = plasma_prop(&conn, &path, PLASMA_DISPLAY, "MaxBrightness").await?;
        let raw = ((v.clamp(0.0, 1.0) * max as f32).round() as i32).clamp(0, max);
        // Flags 0x1 would hide Plasma's own on-screen indicator; leave it showing like a key would.
        conn.call_method(
            Some(PLASMA),
            path.as_str(),
            Some(PLASMA_DISPLAY),
            "SetBrightness",
            &(raw, 0u32),
        )
        .await?;
    }
    Ok(())
}

/// Set the backlight to `v` (0..1) through logind.
async fn logind_apply(ctx: &Ctx, dev: PathBuf, v: f32) {
    let Some(max) = sysfs::<u32>(dev.join("max_brightness")) else {
        return;
    };
    let name = dev
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let raw = ((v.clamp(0.01, 1.0) * max as f32).round() as u32).max(1);
    let conn = match ctx.bus(true).await {
        Ok(conn) => conn,
        Err(e) => {
            ctx.toast(format!("Brightness: no system bus: {e}"));
            return;
        }
    };
    let r = conn
        .call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1/session/auto",
            Some("org.freedesktop.login1.Session"),
            "SetBrightness",
            &("backlight", name.as_str(), raw),
        )
        .await;
    if let Err(e) = r {
        ctx.toast(format!("Brightness: {e}"));
    }
}

async fn apply(ctx: &Ctx, v: f32) {
    if let Some(dev) = device() {
        logind_apply(ctx, dev, v).await;
    } else if ctx.is_kwin() {
        if let Err(e) = plasma_apply(ctx, v).await {
            ctx.toast(format!("Brightness: {e}"));
        }
    } else {
        ctx.toast("Brightness: no backlight to control");
    }
}

#[derive(Clone)]
pub struct Brightness {
    tx: watch::Sender<Option<f32>>,
}

impl Brightness {
    pub fn start(ctx: Ctx) -> Self {
        let c = ctx.clone();
        // Any change (keys, logind, other tools) emits a backlight uevent; on Plasma, PowerDevil
        // signals it.
        let trigger = Trigger::default();
        ctx.on_command_lines(
            "udevadm",
            &["monitor", "--kernel", "--subsystem-match=backlight"],
            |l| l.starts_with("KERNEL["),
            trigger.clone(),
        );
        if ctx.is_kwin() {
            ctx.on_dbus_signals(
                false,
                zbus::MatchRule::builder()
                    .msg_type(zbus::message::Type::Signal)
                    .sender(PLASMA)
                    .expect("valid bus name")
                    .interface(PLASMA_IFACE)
                    .expect("valid interface")
                    .build(),
                Duration::from_millis(50),
                trigger.clone(),
            );
        }
        let mut ticks = ctx.ticks(trigger);
        ctx.spawn(async move {
            let mut sent = Dedup::default();
            ticks.ready().await;
            loop {
                let level = level(&c).await;
                if sent.changed(&level) {
                    c.send(crate::brightness::Event::Level(level));
                }
                ticks.next(Duration::from_secs(60)).await;
            }
        });
        let c = ctx.clone();
        let tx = ctx.latest(move |v| {
            let c = c.clone();
            async move { apply(&c, v).await }
        });
        Self { tx }
    }

    pub fn set(&self, v: f32) {
        let _ = self.tx.send(Some(v));
    }
}

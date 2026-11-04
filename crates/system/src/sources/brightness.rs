//! Backlight: read from sysfs, written through logind's Session.SetBrightness (no root needed).

use super::{Ctx, Dedup, Trigger, sysfs};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;

fn device() -> Option<PathBuf> {
    std::fs::read_dir("/sys/class/backlight")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .next()
}

fn level() -> Option<f32> {
    let dev = device()?;
    let max: u32 = sysfs(dev.join("max_brightness"))?;
    Some(sysfs::<u32>(dev.join("brightness"))? as f32 / max.max(1) as f32)
}

/// Set the backlight to `v` (0..1) through logind.
async fn apply(ctx: &Ctx, v: f32) {
    let Some(dev) = device() else { return };
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

#[derive(Clone)]
pub struct Brightness {
    tx: watch::Sender<Option<f32>>,
}

impl Brightness {
    pub fn start(ctx: Ctx) -> Self {
        let c = ctx.clone();
        // Any change (keys, logind, other tools) emits a backlight uevent.
        let trigger = Trigger::default();
        ctx.on_command_lines(
            "udevadm",
            &["monitor", "--kernel", "--subsystem-match=backlight"],
            |l| l.starts_with("KERNEL["),
            trigger.clone(),
        );
        let sent = Arc::new(Dedup::default());
        ctx.poll_events(trigger, Duration::from_secs(60), move || {
            let (c, sent) = (c.clone(), sent.clone());
            async move {
                let level = level();
                if sent.changed(&level) {
                    c.send(crate::brightness::Event::Level(level));
                }
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

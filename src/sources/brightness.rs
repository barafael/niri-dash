//! Backlight: read from sysfs, written through logind's Session.SetBrightness (no root needed).

use super::{Ctx, Msg, Trigger};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::watch;

fn device() -> Option<PathBuf> {
    std::fs::read_dir("/sys/class/backlight")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .next()
}

fn read(dev: &std::path::Path, f: &str) -> Option<u32> {
    std::fs::read_to_string(dev.join(f))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn level() -> Option<f32> {
    let dev = device()?;
    Some(read(&dev, "brightness")? as f32 / read(&dev, "max_brightness")?.max(1) as f32)
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
        ctx.poll_events(trigger, Duration::from_secs(60), move || {
            let c = c.clone();
            async move { c.send(Msg::Brightness(level())) }
        });
        let (tx, mut rx) = watch::channel(None::<f32>);
        let c = ctx.clone();
        ctx.spawn(async move {
            while rx.changed().await.is_ok() {
                let Some(v) = *rx.borrow_and_update() else {
                    continue;
                };
                let Some(dev) = device() else { continue };
                let Some(max) = read(&dev, "max_brightness") else {
                    continue;
                };
                let name = dev
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let raw = ((v.clamp(0.01, 1.0) * max as f32).round() as u32).max(1);
                let conn = match c.bus(true).await {
                    Ok(conn) => conn,
                    Err(e) => {
                        c.toast(format!("Brightness: no system bus: {e}"));
                        continue;
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
                    c.toast(format!("Brightness: {e}"));
                }
            }
        });
        Self { tx }
    }

    pub fn set(&self, v: f32) {
        let _ = self.tx.send(Some(v));
    }
}

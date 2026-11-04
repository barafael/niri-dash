//! On-screen messages through SwayOSD (`swayosd-client --custom-message`): notification pop-ups,
//! "timer done", "screen sharing started". Without swayosd installed (or its server running)
//! nothing is shown; the dashboard keeps the notification history either way. On Plasma they go to
//! Plasma's own on-screen display instead.

use super::{Ctx, one_line, run, which};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Messages within this window of each other are folded into one "N new notifications".
const BURST: Duration = Duration::from_millis(1000);

#[derive(Default)]
struct Burst {
    /// When the current burst started and how many messages it has had.
    start: Option<Instant>,
    count: usize,
}

#[derive(Clone, Default)]
pub struct Osd {
    burst: Arc<Mutex<Burst>>,
}

impl Osd {
    /// Show `text` with an icon name from the icon theme. A burst of messages shows the first one
    /// and then a single summary when it settles.
    pub fn show(&self, ctx: &Ctx, text: &str, icon: &str) {
        if !ctx.is_kwin() && which("swayosd-client").is_none() {
            return;
        }
        let now = Instant::now();
        let first = {
            let mut b = self.burst.lock().unwrap();
            match b.start {
                Some(t) if now.duration_since(t) < BURST => {
                    b.count += 1;
                    false
                }
                _ => {
                    b.start = Some(now);
                    b.count = 1;
                    true
                }
            }
        };
        if first {
            send(ctx, text.to_string(), icon.to_string());
            // When the burst is over, sum up whatever came after the first message.
            let burst = self.burst.clone();
            let c = ctx.clone();
            ctx.spawn(async move {
                tokio::time::sleep(BURST).await;
                let more = {
                    let mut b = burst.lock().unwrap();
                    let more = b.count.saturating_sub(1);
                    b.start = None;
                    b.count = 0;
                    more
                };
                if more > 0 {
                    let text = if more == 1 {
                        "1 more notification".to_string()
                    } else {
                        format!("{more} more notifications")
                    };
                    send(&c, text, "preferences-system-notifications".into());
                }
            });
        }
    }
}

fn send(ctx: &Ctx, text: String, icon: String) {
    let c = ctx.clone();
    ctx.spawn(async move {
        // The OSD is one short line.
        let text = one_line(&text, 90);
        if c.is_kwin() {
            if let Err(e) = super::kde::show_osd(&c, &text, &icon).await {
                log::debug!("plasma osd: {e}");
            }
            return;
        }
        if let Err(e) = run(
            "swayosd-client",
            &["--custom-message", &text, "--custom-icon", &icon],
        )
        .await
        {
            log::debug!("swayosd: {e}");
        }
    });
}

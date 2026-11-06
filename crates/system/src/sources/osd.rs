//! On-screen messages through SwayOSD (`swayosd-client --custom-message`): notification pop-ups,
//! "timer done", "screen sharing started". Without swayosd installed (or its server running)
//! nothing is shown; the dashboard keeps the notification history either way. On Plasma they go to
//! Plasma's own on-screen display instead.

use super::{Ctx, one_line, run, which};
use std::time::{Duration, Instant};

/// Messages within this window of each other are folded into one "N new notifications".
pub const BURST: Duration = Duration::from_millis(1000);

/// A burst of messages shows the first one and then a single summary when it settles.
#[derive(Default)]
pub struct Burst {
    /// When the current burst started and how many messages it has had.
    start: Option<Instant>,
    count: usize,
}

impl Burst {
    /// A message came: whether it is the first of a burst, and so should show.
    pub fn shown(&mut self, now: Instant) -> bool {
        match self.start {
            Some(t) if now.duration_since(t) < BURST => {
                self.count += 1;
                false
            }
            _ => {
                self.start = Some(now);
                self.count = 1;
                true
            }
        }
    }

    /// The burst is over: how many came after the first.
    pub fn over(&mut self) -> usize {
        let more = self.count.saturating_sub(1);
        self.start = None;
        self.count = 0;
        more
    }
}

/// What to say about `more` messages that came after the first.
pub fn summary(more: usize) -> Option<String> {
    match more {
        0 => None,
        1 => Some("1 more notification".to_string()),
        more => Some(format!("{more} more notifications")),
    }
}

/// Show `text` with an icon name from the icon theme.
pub fn send(ctx: &Ctx, text: String, icon: String) {
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
        if which("swayosd-client").is_none() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_shows_and_the_rest_are_counted() {
        let start = Instant::now();
        let mut b = Burst::default();
        assert!(b.shown(start));
        assert!(!b.shown(start + Duration::from_millis(100)));
        assert!(!b.shown(start + Duration::from_millis(900)));
        assert_eq!(b.over(), 2);
    }

    #[test]
    fn past_the_window_is_a_new_burst() {
        let start = Instant::now();
        let mut b = Burst::default();
        assert!(b.shown(start));
        assert!(b.shown(start + BURST));
        assert_eq!(b.over(), 0);
    }

    #[test]
    fn after_a_burst_the_next_shows_again() {
        let start = Instant::now();
        let mut b = Burst::default();
        assert!(b.shown(start));
        assert_eq!(b.over(), 0);
        assert!(b.shown(start + Duration::from_millis(1)));
    }

    #[test]
    fn what_it_says() {
        assert_eq!(summary(0), None);
        assert_eq!(summary(1).as_deref(), Some("1 more notification"));
        assert_eq!(summary(3).as_deref(), Some("3 more notifications"));
    }
}

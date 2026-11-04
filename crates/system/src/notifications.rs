//! Notifications: our own server's history (or Plasma's pop-ups), and on-screen messages.

use crate::IconSrc;

#[derive(Debug, Clone)]
pub struct Notification {
    pub id: u32,
    pub app: String,
    pub summary: String,
    pub body: String,
    pub icon: IconSrc,
    pub actions: Vec<(String, String)>,
    pub urgent: bool,
    pub time: chrono::DateTime<chrono::Local>,
}

impl Notification {
    pub fn has_default(&self) -> bool {
        self.actions.iter().any(|(k, _)| k == "default")
    }
}

/// Do not disturb: new notifications go into the history without a pop-up (critical ones still
/// pop up).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Dnd {
    #[default]
    Off,
    Until(chrono::DateTime<chrono::Local>),
    Forever,
}

impl Dnd {
    pub fn active(self, now: chrono::DateTime<chrono::Local>) -> bool {
        match self {
            Dnd::Off => false,
            Dnd::Until(t) => now < t,
            Dnd::Forever => true,
        }
    }

    /// As stored in state.json: Unix seconds, `i64::MAX` for forever.
    pub fn to_saved(self) -> Option<i64> {
        match self {
            Dnd::Off => None,
            Dnd::Until(t) => Some(t.timestamp()),
            Dnd::Forever => Some(i64::MAX),
        }
    }

    pub fn from_saved(v: Option<i64>) -> Dnd {
        use chrono::TimeZone;
        match v {
            None => Dnd::Off,
            Some(i64::MAX) => Dnd::Forever,
            Some(t) => chrono::Local
                .timestamp_opt(t, 0)
                .single()
                .map_or(Dnd::Off, Dnd::Until),
        }
    }
}

/// The next `hour`:00 after `now` (tomorrow if that hour has passed today).
pub fn next_hour(
    now: chrono::DateTime<chrono::Local>,
    hour: u32,
) -> chrono::DateTime<chrono::Local> {
    use chrono::{Days, TimeZone};
    let mut day = now.date_naive();
    loop {
        if let Some(t) = day
            .and_hms_opt(hour, 0, 0)
            .and_then(|t| chrono::Local.from_local_datetime(&t).earliest())
            && t > now
        {
            return t;
        }
        day = day + Days::new(1);
    }
}

#[derive(Debug, Clone, Default)]
pub struct State {
    /// We are the notification server.
    pub server: bool,
    /// Another server (Plasma's) shows the notifications, and do not disturb pauses its pop-ups.
    pub can_pause: bool,
    pub items: Vec<Notification>,
    pub dnd: Dnd,
    /// Notifications that came in without a pop-up during do not disturb.
    pub missed: u32,
}

#[derive(Debug)]
pub enum Command {
    SetDnd(Dnd),
    Dismiss(u32),
    Clear,
    /// Run a notification's default action.
    Invoke(u32),
    /// One of our own messages (a timer going off), into the history or to Plasma.
    AddLocal {
        app: String,
        summary: String,
        body: String,
    },
    /// A short on-screen message.
    Osd {
        text: String,
        icon: String,
    },
}

#[derive(Debug)]
pub enum Event {
    State(State),
}

//! Running Claude Code sessions, and Claude usage.

/// What Claude Code reports a session doing. Unknown values (newer Claude Code versions) are
/// kept as they are.
#[derive(Debug, Clone, PartialEq, Eq, Default, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum Status {
    Busy,
    #[default]
    Idle,
    #[strum(default)]
    Other(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub pid: i32,
    pub title: String,
    pub place: String,
    pub status: Status,
    pub status_text: String,
    pub window: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub sessions: Vec<Session>,
    pub busy: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usage {
    pub plan: String,
    pub window_active: bool,
    pub window_tokens: String,
    /// Just the number, for small spaces ("2.2M").
    pub window_count: String,
    pub window_cached: String,
    pub window_messages: u32,
    pub window_left: String,
    pub window_progress: f32,
    pub window_resets: String,
    pub burn: String,
    pub today: String,
    pub week: String,
    /// Tokens per hour over the last 24 hours, scaled to the busiest hour.
    pub hourly: Vec<f32>,
    /// Models used in the current window, by share of tokens.
    pub models: Vec<(String, f32)>,
}

#[derive(Debug)]
pub enum Event {
    Sessions(State),
    Usage(Box<Usage>),
}

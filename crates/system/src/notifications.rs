//! Notifications: our own server's history (or Plasma's pop-ups), and on-screen messages.

pub use crate::sources::notifications::{Dnd, Notification, State, next_hour};

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

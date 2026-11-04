//! The media player (MPRIS).

pub use crate::sources::media::State;

#[derive(Debug)]
pub enum Command {
    PlayPause,
    Next,
    Previous,
}

#[derive(Debug)]
pub enum Event {
    /// None: no player.
    State(Option<State>),
}

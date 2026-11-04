//! The media player (MPRIS).

use crate::IconSrc;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub bus_name: String,
    pub player: String,
    pub title: String,
    pub artist: String,
    pub playing: bool,
    pub art: Option<IconSrc>,
    pub progress: f32,
}

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

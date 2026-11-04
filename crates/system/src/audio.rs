//! Volume and the mixer, from PipeWire.

use crate::IconSrc;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub volume: f32,
    pub muted: bool,
    pub mic: Option<(f32, bool)>,
    pub sink_name: String,
    /// Output devices, to pick the default from.
    pub sinks: Vec<Sink>,
    /// Apps playing sound.
    pub streams: Vec<Stream>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sink {
    pub id: u32,
    pub name: String,
    pub default: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    pub id: u32,
    pub app: String,
    pub icon: IconSrc,
    pub volume: f32,
    pub muted: bool,
}

/// Streams and sinks are named by their PipeWire id.
#[derive(Debug)]
pub enum Command {
    SetVolume(f32),
    SetMicVolume(f32),
    ToggleMute,
    ToggleMicMute,
    SetStreamVolume(u32, f32),
    ToggleStreamMute(u32),
    SetDefaultSink(u32),
}

#[derive(Debug)]
pub enum Event {
    State(State),
}

//! Volume and the mixer, from PipeWire.

pub use crate::sources::audio::State;

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

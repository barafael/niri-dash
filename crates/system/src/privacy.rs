//! Who uses the microphone and camera (PipeWire), and who shares the screen (niri).

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usage {
    /// Apps recording audio.
    pub mic: Vec<String>,
    /// Apps reading a camera.
    pub camera: Vec<String>,
    /// For each video node that something reads (screencasts among them): who reads it.
    pub video_readers: Vec<(u32, String)>,
}

/// A screencast as the privacy widget shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenCast {
    pub session_id: u64,
    /// What is shared: a monitor name or a window title.
    pub target: String,
    pub active: bool,
    /// The consumer's process (wlr-screencopy only).
    pub pid: Option<i32>,
    /// That process's name.
    pub process: Option<String>,
    /// The PipeWire node of the stream, to find who reads it.
    pub pw_node: Option<u32>,
}

#[derive(Debug)]
pub enum Command {
    StopCast(u64),
}

#[derive(Debug)]
pub enum Event {
    Usage(Usage),
    Casts(Vec<ScreenCast>),
}

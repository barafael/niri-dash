//! Who uses the microphone and camera (PipeWire), and who shares the screen (niri).

pub use crate::sources::niri::ScreenCast;
pub use crate::sources::privacy::Usage;

#[derive(Debug)]
pub enum Command {
    StopCast(u64),
}

#[derive(Debug)]
pub enum Event {
    Usage(Usage),
    Casts(Vec<ScreenCast>),
}

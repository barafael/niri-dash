//! Night light: our own gamma tables (wlr-gamma-control), or on Plasma, KWin's own night light.

pub use super::wayland::gamma::{Cmd, NEUTRAL, WARMEST};
pub use crate::sources::kde::NightLightState;

#[derive(Debug)]
pub enum Command {
    /// Warm the screen to this temperature through the gamma tables; None turns it off.
    Gamma(Option<u32>),
    /// Apply `cmd` to KWin's night light; `announce` shows the result on screen.
    Kwin { cmd: Cmd, announce: bool },
}

#[derive(Debug)]
pub enum Event {
    Kwin(NightLightState),
}

//! Open windows by workspace (niri only).

pub use crate::sources::niri::{Row, WindowsState as State};

#[derive(Debug)]
pub enum Command {
    Focus(u64),
    /// Focus the window of this process, if it has one.
    FocusPid(i32),
    Close(u64),
}

#[derive(Debug)]
pub enum Event {
    State(State),
}

//! Keyboard layouts, from niri or Plasma.

pub use crate::sources::keyboard::{Layouts, Switch};

#[derive(Debug)]
pub enum Command {
    Switch(Switch),
}

#[derive(Debug)]
pub enum Event {
    /// None: the compositor doesn't say.
    Layouts(Option<Layouts>),
}

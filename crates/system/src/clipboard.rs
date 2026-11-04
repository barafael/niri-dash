//! The clipboard, through the Wayland data-control protocols.

pub use crate::sources::clipboard::{Clip, ClipKind, MAX_ENTRIES, decode_png, preview, shrink};

#[derive(Debug)]
pub enum Command {
    /// Put something on the clipboard. Works whether or not the clipboard is being recorded.
    Set(Clip),
    /// Start or stop reading what gets copied. It starts off: until told otherwise, nothing copied
    /// is read. Starting reads what is on the clipboard at that moment too.
    Record(bool),
}

#[derive(Debug)]
pub enum Event {
    /// Something new was copied, while recording.
    Copied(Clip),
}

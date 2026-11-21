//! What concerns the system as a whole rather than one applet.

use std::path::PathBuf;

/// A sound the dashboard asks for by what it means; which file plays is the system's business.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    /// A timer or a phase has run out.
    Alarm,
}

#[derive(Debug)]
pub enum Command {
    /// The dashboard came up or went away. Sources only watch and poll while it is up; coming
    /// up refreshes them all.
    Visible(bool),
    /// Refresh every source now.
    Refresh,
    /// Open a file or folder with the desktop's default app.
    Open(PathBuf),
    /// Open a web page in the desktop's browser.
    OpenUrl(String),
    /// Play one of the sounds the dashboard makes, if the system has one to play.
    PlaySound(Sound),
}

/// The output a surface that doesn't name one will appear on: the one with the focus.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Output {
    /// Its size in logical pixels.
    pub width: u32,
    pub height: u32,
    /// Physical pixels per logical one.
    pub scale: f64,
}

#[derive(Debug)]
pub enum Event {
    /// Where the dashboard would appear now, if the desktop says (niri does). Reported when the
    /// system starts, before [`crate::live`] returns, and again when it changes, so a surface can
    /// be laid out for it before its first frame.
    Output(Option<Output>),
}

//! What concerns the system as a whole rather than one applet.

use std::path::PathBuf;
use tokio::sync::oneshot;

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
    /// The logical size and scale of the output the dashboard is about to appear on, if the
    /// compositor says.
    FocusedOutput {
        reply: oneshot::Sender<Option<(u32, u32, f64)>>,
    },
}

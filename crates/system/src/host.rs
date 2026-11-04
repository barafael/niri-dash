//! What concerns the system as a whole rather than one applet.

use std::path::PathBuf;
use tokio::sync::oneshot;

#[derive(Debug)]
pub enum Command {
    /// The dashboard came up or went away. Sources only watch and poll while it is up; coming
    /// up refreshes them all.
    Visible(bool),
    /// Refresh every source now.
    Refresh,
    /// Open a file or folder with the desktop's default app.
    Open(PathBuf),
    /// Play a sound file, if it exists.
    PlaySound(PathBuf),
    /// The logical size and scale of the output the dashboard is about to appear on, if the
    /// compositor says.
    FocusedOutput {
        reply: oneshot::Sender<Option<(u32, u32, f64)>>,
    },
}

//! The mascots' commands.

#[derive(Debug)]
pub enum Command {
    /// Run a mascot widget's shell command, with `NIRI_DASH_MASCOT` set to its name.
    Run {
        id: u32,
        name: &'static str,
        command: String,
    },
}

#[derive(Debug)]
pub enum Event {
    /// What the command of widget `id` printed; None if it failed or hung.
    Output { id: u32, stdout: Option<String> },
}

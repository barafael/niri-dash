//! Lock, suspend, log out, reboot, power off.

#[derive(Debug)]
pub enum Command {
    Lock,
    Suspend,
    /// niri and Plasma both ask for confirmation first.
    Logout,
    Reboot,
    Poweroff,
}

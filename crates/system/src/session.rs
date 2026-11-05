//! Lock, suspend, log out, reboot, power off.
//!
//! The dashboard sends these straight through and asks nobody. A system must not carry out
//! `Logout`, `Reboot` or `Poweroff` unless the user has confirmed it: under niri and Plasma the
//! compositor asks, anywhere else the system has to.

#[derive(Debug)]
pub enum Command {
    Lock,
    Suspend,
    Logout,
    Reboot,
    Poweroff,
}

//! Lock, suspend, log out, reboot, power off.

use super::{CmdError, Ctx, run, spawn_detached, which};
use niri_ipc::Action;

#[derive(Debug, thiserror::Error)]
enum LockError {
    #[error(transparent)]
    Cmd(#[from] CmdError),
    #[error(
        "no screen locker found (install swaylock, hyprlock or gtklock); sent loginctl lock-session"
    )]
    NoLocker,
}

const LOCKERS: [(&str, &[&str]); 4] = [
    ("swaylock", &["-f"]),
    ("hyprlock", &[]),
    ("gtklock", &["-d"]),
    ("waylock", &[]),
];

pub fn lock(ctx: &Ctx) {
    ctx.act("Lock", async {
        for (prog, args) in LOCKERS {
            if which(prog).is_some() {
                return spawn_detached(prog, args).map_err(LockError::from);
            }
        }
        // Maybe a locker listens for logind's Lock signal (swayidle -w lock ...).
        run("loginctl", &["lock-session"]).await?;
        Err(LockError::NoLocker)
    });
}

pub fn suspend(ctx: &Ctx) {
    ctx.act("Suspend", async {
        run("systemctl", &["suspend"]).await.map(|_| ())
    });
}

pub fn reboot(ctx: &Ctx) {
    ctx.act("Restart", async {
        run("systemctl", &["reboot"]).await.map(|_| ())
    });
}

pub fn poweroff(ctx: &Ctx) {
    ctx.act("Power off", async {
        run("systemctl", &["poweroff"]).await.map(|_| ())
    });
}

pub fn logout(ctx: &Ctx) {
    ctx.act("Log out", async {
        // niri shows its own confirmation dialog.
        super::niri::action_async(Action::Quit {
            skip_confirmation: false,
        })
        .await
    });
}

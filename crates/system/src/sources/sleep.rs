//! The sleep timer's side of the machine: which modes logind offers, running one, and the idle
//! inhibitor that keeps the screen from blanking while the countdown runs.

use super::{CmdError, Ctx, run};
use crate::sleep::{self, Mode};
use std::path::{Path, PathBuf};
use tokio::sync::watch;
use zbus::zvariant::OwnedFd;

const LOGIND: &str = "org.freedesktop.login1";

/// logind's answer to a `Can…` question, or why there is none.
async fn can(ctx: &Ctx, method: &str) -> zbus::Result<Option<bool>> {
    let answer: String = ctx
        .bus(true)
        .await?
        .call_method(
            Some(LOGIND),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            method,
            &(),
        )
        .await?
        .body()
        .deserialize()?;
    Ok(sleep::possible(&answer))
}

/// The modes this machine offers, then the script's if it is there. A machine where logind
/// cannot be asked gets the two modes nearly every Linux machine has.
pub fn detect(ctx: &Ctx, script: PathBuf) {
    let c = ctx.clone();
    ctx.spawn(async move {
        let mut modes = Vec::new();
        let mut asked = false;
        for mode in Mode::all() {
            let Some(method) = mode.method() else {
                continue;
            };
            match can(&c, method).await {
                Ok(answer) => {
                    asked = true;
                    if answer == Some(true) {
                        modes.push(mode);
                    }
                }
                Err(e) => log::debug!("logind {method}: {e}"),
            }
        }
        if !asked {
            modes = vec![Mode::Suspend, Mode::Poweroff];
        }
        if script.is_file() {
            modes.push(Mode::RunScript);
        }
        c.send(sleep::Event::Modes(modes));
    });
}

/// Run `mode` and say how it went.
pub fn perform(ctx: &Ctx, mode: Mode, script: PathBuf) {
    let c = ctx.clone();
    ctx.spawn(async move {
        let result = match mode.verb() {
            Some(verb) => run("systemctl", &[&verb]).await.map(|_| ()),
            None => run_script(&script).await,
        };
        let error = result.err().map(|e| match (mode, e) {
            (Mode::RunScript, CmdError::Failed { message, .. }) if message.is_empty() => {
                format!("{} failed", script.display())
            }
            (Mode::RunScript, e) => format!("{}: {e}", script.display()),
            (_, CmdError::Failed { message, .. }) if !message.is_empty() => {
                format!("{} refused: {message}", mode.label())
            }
            (_, e) => format!("{} failed: {e}", mode.label()),
        });
        if let Some(e) = &error {
            log::warn!("sleep timer: {e}");
        }
        c.send(sleep::Event::Done { mode, error });
    });
}

/// The user's script: run as it is if it can be, through `sh` if it was never made executable.
async fn run_script(script: &Path) -> Result<(), CmdError> {
    use std::os::unix::fs::PermissionsExt;
    let path = script.to_string_lossy();
    let executable = std::fs::metadata(script).is_ok_and(|m| m.permissions().mode() & 0o111 != 0);
    if executable {
        run(&path, &[]).await.map(|_| ())
    } else {
        run("sh", &[&path]).await.map(|_| ())
    }
}

/// Holds logind's idle inhibitor while asked to. Ours alone: the power widget's switch is the
/// user's, and letting go of this must not let go of that.
pub struct Awake {
    want: watch::Sender<bool>,
}

impl Awake {
    pub fn start(ctx: Ctx) -> Self {
        let (want, mut rx) = watch::channel(false);
        let c = ctx.clone();
        ctx.spawn(async move {
            // logind lifts the block when the last copy of the descriptor is closed.
            let mut held: Option<OwnedFd> = None;
            while rx.changed().await.is_ok() {
                let on = *rx.borrow_and_update();
                if on && held.is_none() {
                    held = take(&c)
                        .await
                        .map_err(|e| log::warn!("keep awake: {e}"))
                        .ok();
                } else if !on {
                    held = None;
                }
            }
        });
        Self { want }
    }

    pub fn set(&self, on: bool) {
        self.want.send_replace(on);
    }
}

async fn take(ctx: &Ctx) -> zbus::Result<OwnedFd> {
    ctx.bus(true)
        .await?
        .call_method(
            Some(LOGIND),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            "Inhibit",
            &(
                "idle",
                "niri-dash sleep timer",
                "Counting down to sleep",
                "block",
            ),
        )
        .await?
        .body()
        .deserialize()
}

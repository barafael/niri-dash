//! Which compositor we run under, decided once when the system starts and carried in every
//! source's [`Ctx`], and the actions that depend on it. Each feature that needs the compositor goes
//! through one function here, which picks niri's IPC or KDE Plasma's D-Bus services (or does
//! without).

use crate::Compositor;
use crate::sources::{Ctx, kde, keyboard, niri};
use std::collections::HashMap;
use std::time::Duration;

/// The compositor the environment names, if it names one: niri sets `NIRI_SOCKET` (and its
/// session `XDG_CURRENT_DESKTOP=niri`), a Plasma session `XDG_CURRENT_DESKTOP=KDE` and
/// `KDE_FULL_SESSION`.
pub fn from_env(
    niri_socket: Option<&str>,
    desktop: Option<&str>,
    kde_session: Option<&str>,
) -> Option<Compositor> {
    let set = |v: Option<&str>| v.is_some_and(|v| !v.is_empty());
    let desktop_is = |name: &str| {
        desktop.is_some_and(|d| d.split(':').any(|d| d.trim().eq_ignore_ascii_case(name)))
    };
    if set(niri_socket) || desktop_is("niri") {
        Some(Compositor::Niri)
    } else if desktop_is("KDE") || set(kde_session) {
        Some(Compositor::Kwin)
    } else {
        None
    }
}

fn env_guess() -> Option<Compositor> {
    let var = |k| std::env::var(k).ok();
    from_env(
        var("NIRI_SOCKET").as_deref(),
        var("XDG_CURRENT_DESKTOP").as_deref(),
        var("KDE_FULL_SESSION").as_deref(),
    )
}

/// The compositor as the environment names it, without asking anyone (for the command line).
pub fn guess() -> Compositor {
    env_guess().unwrap_or(Compositor::Other)
}

/// Decide which compositor this is: from the environment, else by whether KWin is on the session
/// bus. Called once when the system starts.
pub fn detect(rt: &tokio::runtime::Handle) -> Compositor {
    let found = env_guess().unwrap_or_else(|| {
        let kwin = rt.block_on(async {
            tokio::time::timeout(Duration::from_millis(500), kde::kwin_running())
                .await
                .unwrap_or(false)
        });
        if kwin {
            Compositor::Kwin
        } else {
            Compositor::Other
        }
    });
    log::info!("compositor: {found:?}");
    found
}

/// Something only one compositor can do was asked of another.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Niri(#[from] niri::Error),
    #[error(transparent)]
    Kde(#[from] kde::Error),
    #[error("{0} needs niri or KDE Plasma")]
    Unsupported(&'static str),
}

/// Start what the compositor reports by itself: niri's event stream (windows, keyboard layouts,
/// screencasts), or Plasma's keyboard layouts.
pub fn start(ctx: Ctx) {
    match ctx.compositor() {
        Compositor::Niri => niri::start(ctx),
        Compositor::Kwin => kde::start_keyboard(ctx),
        Compositor::Other => {}
    }
}

/// The size and scale of the output the dashboard is about to appear on, if the compositor says.
/// Blocks (niri's socket is synchronous).
pub fn focused_output(compositor: Compositor) -> Option<(u32, u32, f64)> {
    if compositor != Compositor::Niri {
        return None;
    }
    match niri::request(niri_ipc::Request::FocusedOutput) {
        Ok(niri_ipc::Response::FocusedOutput(Some(out))) => {
            let l = out.logical?;
            Some((l.width, l.height, l.scale))
        }
        _ => None,
    }
}

pub fn switch_layout(ctx: &Ctx, to: keyboard::Switch) {
    match ctx.compositor() {
        Compositor::Niri => niri::switch_layout(ctx, to),
        Compositor::Kwin => kde::switch_layout(ctx, to),
        Compositor::Other => {}
    }
}

pub fn focus_window(ctx: &Ctx, id: u64) {
    if ctx.compositor() == Compositor::Niri {
        niri::focus(ctx, id);
    }
}

/// Focus the window of process `pid`, if it has one.
pub fn focus_pid(ctx: &Ctx, pid: i32) {
    if ctx.compositor() == Compositor::Niri {
        niri::focus_pid(ctx, pid);
    }
}

pub fn close_window(ctx: &Ctx, id: u64) {
    if ctx.compositor() == Compositor::Niri {
        niri::close(ctx, id);
    }
}

pub fn stop_cast(ctx: &Ctx, session_id: u64) {
    if ctx.compositor() == Compositor::Niri {
        niri::stop_cast(ctx, session_id);
    }
}

/// Every window by the pid of its process: (window id, title). Empty where windows aren't known.
/// Blocks (niri's socket is synchronous).
pub fn windows_by_pid(compositor: Compositor) -> HashMap<i32, (u64, String)> {
    if compositor != Compositor::Niri {
        return HashMap::new();
    }
    match niri::request(niri_ipc::Request::Windows) {
        Ok(niri_ipc::Response::Windows(ws)) => ws
            .into_iter()
            .filter_map(|w| Some((w.pid?, (w.id, w.title.unwrap_or_default()))))
            .collect(),
        _ => HashMap::new(),
    }
}

/// Let the user click a pixel anywhere; `None` if they cancelled. Returns once they did.
pub async fn pick_color(ctx: &Ctx) -> Result<Option<[f64; 3]>, Error> {
    match ctx.compositor() {
        Compositor::Niri => Ok(tokio::task::spawn_blocking(niri::pick_color)
            .await
            .map_err(niri::Error::from)??),
        Compositor::Kwin => Ok(kde::pick_color(ctx).await?),
        Compositor::Other => Err(Error::Unsupported("Picking a color")),
    }
}

/// Ask to end the session; niri and Plasma both show their own confirmation first.
pub async fn logout(ctx: &Ctx) -> Result<(), Error> {
    match ctx.compositor() {
        Compositor::Niri => Ok(niri::action_async(niri_ipc::Action::Quit {
            skip_confirmation: false,
        })
        .await?),
        Compositor::Kwin => Ok(kde::logout(ctx).await?),
        Compositor::Other => Err(Error::Unsupported("Logging out")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection() {
        use Compositor::*;
        assert_eq!(
            from_env(Some("/run/niri.sock"), Some("KDE"), None),
            Some(Niri)
        );
        assert_eq!(from_env(None, Some("niri"), None), Some(Niri));
        assert_eq!(from_env(Some(""), Some("KDE"), None), Some(Kwin));
        assert_eq!(from_env(None, Some("ubuntu:KDE"), None), Some(Kwin));
        assert_eq!(from_env(None, None, Some("true")), Some(Kwin));
        assert_eq!(from_env(None, Some("sway"), None), None);
        assert_eq!(from_env(None, None, None), None);
    }
}

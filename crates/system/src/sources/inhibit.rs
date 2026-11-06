//! Manually block sleep and screen locking, like the switch in Plasma's battery widget.
//!
//! Screen locking is blocked through `org.freedesktop.ScreenSaver.Inhibit`, which niri (and
//! KWin) implement: while it's held, niri's idle notifier stays quiet, so swayidle / hypridle
//! never fire. Idle sleep is blocked with a logind `idle` inhibitor, which logind's own
//! `IdleAction` and hypridle honor; suspending by hand or by closing the lid still works.
//! powerdevil's `org.freedesktop.PowerManagement.Inhibit` is taken too where it runs.

use super::{Ctx, Dedup};
pub use crate::power::Blocker;
use tokio::sync::watch;
use zbus::Connection;
use zbus::zvariant::OwnedFd;

/// zbus errors are boxed: a `zbus::Error` is large, and unboxed it would make every `Result`
/// here as big.
#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("{dest}.Inhibit: {source}")]
    Inhibit {
        dest: &'static str,
        #[source]
        source: Box<zbus::Error>,
    },
    #[error(transparent)]
    Dbus(Box<zbus::Error>),
}

impl From<zbus::Error> for Error {
    fn from(e: zbus::Error) -> Self {
        Error::Dbus(Box::new(e))
    }
}

type Result<T, E = Error> = std::result::Result<T, E>;

const WHO: &str = "niri-dash";
const WHY: &str = "Blocked manually from the dashboard";

const SCREENSAVER: (&str, &str, &str) = (
    "org.freedesktop.ScreenSaver",
    "/org/freedesktop/ScreenSaver",
    "org.freedesktop.ScreenSaver",
);
const POWER_MANAGEMENT: (&str, &str, &str) = (
    "org.freedesktop.PowerManagement",
    "/org/freedesktop/PowerManagement/Inhibit",
    "org.freedesktop.PowerManagement.Inhibit",
);

/// An inhibitor held through a cookie; it's released by calling `UnInhibit`, or by our bus
/// connection going away.
struct Cookie {
    conn: Connection,
    target: (&'static str, &'static str, &'static str),
    cookie: u32,
}

impl Cookie {
    async fn take(
        conn: &Connection,
        target: (&'static str, &'static str, &'static str),
    ) -> Result<Self> {
        let (dest, path, iface) = target;
        let cookie: u32 = conn
            .call_method(Some(dest), path, Some(iface), "Inhibit", &(WHO, WHY))
            .await
            .map_err(|e| Error::Inhibit {
                dest,
                source: Box::new(e),
            })?
            .body()
            .deserialize()?;
        Ok(Self {
            conn: conn.clone(),
            target,
            cookie,
        })
    }

    async fn release(self) {
        let (dest, path, iface) = self.target;
        if let Err(e) = self
            .conn
            .call_method(Some(dest), path, Some(iface), "UnInhibit", &(self.cookie,))
            .await
        {
            log::debug!("{dest}.UnInhibit: {e}");
        }
    }
}

#[derive(Default)]
struct Held {
    screensaver: Option<Cookie>,
    power_management: Option<Cookie>,
    /// logind releases the inhibitor when the last copy of this fd is closed.
    logind: Option<OwnedFd>,
}

impl Held {
    fn any(&self) -> bool {
        self.screensaver.is_some() || self.power_management.is_some() || self.logind.is_some()
    }

    async fn release(&mut self) {
        for c in [self.screensaver.take(), self.power_management.take()]
            .into_iter()
            .flatten()
        {
            c.release().await;
        }
        self.logind = None;
    }
}

async fn has_owner(conn: &Connection, name: &str) -> bool {
    let Ok(dbus) = zbus::fdo::DBusProxy::new(conn).await else {
        return false;
    };
    let Ok(name) = zbus::names::BusName::try_from(name) else {
        return false;
    };
    dbus.name_has_owner(name).await.unwrap_or(false)
}

async fn logind_inhibit(ctx: &Ctx) -> Result<OwnedFd> {
    let fd: OwnedFd = ctx
        .bus(true)
        .await?
        .call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            "Inhibit",
            &("idle", WHO, WHY, "block"),
        )
        .await
        .map_err(|e| Error::Inhibit {
            dest: "org.freedesktop.login1",
            source: Box::new(e),
        })?
        .body()
        .deserialize()?;
    Ok(fd)
}

/// Take whatever inhibitors this session offers. Errors are only for the user when screen
/// locking couldn't be blocked, since that's what the switch is mostly for.
async fn acquire(ctx: &Ctx) -> (Held, Option<String>) {
    let mut held = Held::default();
    let mut problem = None;
    match ctx.bus(false).await {
        Ok(conn) => {
            match Cookie::take(&conn, SCREENSAVER).await {
                Ok(c) => held.screensaver = Some(c),
                Err(e) => {
                    log::warn!("inhibit: {e}");
                    problem = Some(
                        "screen locking isn't blocked: nothing provides org.freedesktop.ScreenSaver"
                            .to_string(),
                    );
                }
            }
            if has_owner(&conn, POWER_MANAGEMENT.0).await {
                match Cookie::take(&conn, POWER_MANAGEMENT).await {
                    Ok(c) => held.power_management = Some(c),
                    Err(e) => log::debug!("inhibit: {e}"),
                }
            }
        }
        Err(e) => problem = Some(format!("no session bus: {e}")),
    }
    match logind_inhibit(ctx).await {
        Ok(fd) => held.logind = Some(fd),
        Err(e) => log::warn!("inhibit: {e}"),
    }
    (held, problem)
}

async fn blockers(ctx: &Ctx) -> zbus::Result<Vec<Blocker>> {
    let conn = ctx.bus(true).await?;
    let list: Vec<(String, String, String, String, u32, u32)> = conn
        .call_method(
            Some("org.freedesktop.login1"),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            "ListInhibitors",
            &(),
        )
        .await?
        .body()
        .deserialize()?;
    Ok(filter_blockers(list))
}

/// Keep the ones that block (rather than delay) sleep or idling, except our own switch.
fn filter_blockers(list: Vec<(String, String, String, String, u32, u32)>) -> Vec<Blocker> {
    let mut out: Vec<Blocker> = list
        .into_iter()
        .filter(|(what, who, _, mode, _, _)| {
            mode == "block" && who != WHO && what.split(':').any(|w| w == "sleep" || w == "idle")
        })
        .map(|(_, who, why, ..)| Blocker { who, why })
        .collect();
    out.dedup();
    out
}

/// The switch. Flipping it is asynchronous; the UI hears back through `power::Event::Inhibited`.
#[derive(Clone)]
pub struct Inhibit {
    want: watch::Sender<bool>,
}

impl Inhibit {
    pub fn start(ctx: Ctx) -> Self {
        // logind has no signal for its inhibitor list: look again every few seconds while shown.
        let c = ctx.clone();
        let mut ticks = ctx.ticks(Default::default());
        ctx.spawn(async move {
            let mut sent = Dedup::default();
            ticks.ready().await;
            loop {
                let list = blockers(&c).await.unwrap_or_else(|e| {
                    log::debug!("ListInhibitors: {e}");
                    Vec::new()
                });
                if sent.changed(&list) {
                    c.send(crate::power::Event::SleepBlockers(list));
                }
                ticks.next(std::time::Duration::from_secs(5)).await;
            }
        });
        let want = watch::Sender::new(false);
        let mut rx = want.subscribe();
        let w = want.clone();
        let c = ctx.clone();
        c.spawn(async move {
            let mut held = Held::default();
            loop {
                let on = *rx.borrow_and_update();
                if on && !held.any() {
                    let problem;
                    (held, problem) = acquire(&ctx).await;
                    if let Some(p) = problem {
                        ctx.toast(format!("Block sleep: {p}"));
                    }
                    log::info!(
                        "inhibit on: screensaver={} powerdevil={} logind={}",
                        held.screensaver.is_some(),
                        held.power_management.is_some(),
                        held.logind.is_some()
                    );
                    if !held.any() {
                        // Nothing could be held: flip the switch back so it doesn't look on.
                        w.send_replace(false);
                        rx.borrow_and_update();
                    }
                } else if !on && held.any() {
                    held.release().await;
                    log::info!("inhibit off");
                }
                ctx.send(crate::power::Event::Inhibited(held.any()));
                if rx.changed().await.is_err() {
                    return;
                }
            }
        });
        Self { want }
    }

    pub fn toggle(&self) {
        self.want.send_modify(|w| *w = !*w);
    }
}

#[cfg(test)]
mod tests {
    use super::filter_blockers;

    #[test]
    fn blockers() {
        let e = |what: &str, who: &str, mode: &str| {
            (
                what.to_string(),
                who.to_string(),
                "why".to_string(),
                mode.to_string(),
                1000,
                1,
            )
        };
        let b = filter_blockers(vec![
            e("sleep", "NetworkManager", "delay"),
            e("handle-power-key:handle-lid-switch", "niri", "block"),
            e("idle", "niri-dash", "block"),
            e("sleep:idle", "Steam", "block"),
            e("shutdown:sleep", "Package manager", "block"),
        ]);
        let who: Vec<&str> = b.iter().map(|b| b.who.as_str()).collect();
        assert_eq!(who, ["Steam", "Package manager"]);
    }
}

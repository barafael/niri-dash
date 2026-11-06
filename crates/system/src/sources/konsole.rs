//! Konsole's tabs. Going to a process's window only gets as far as whichever tab is current there;
//! Konsole, on D-Bus under any compositor, can bring up the right one.
//!
//! Konsole tells each tab's shell where it is in three environment variables, but everything
//! started from there inherits them, wherever it ends up: an editor started from Konsole hands
//! them to the terminals inside it, and a tmux server keeps those of the tab it started in. So
//! they are only a claim. It holds if the Konsole they name says that tab's shell is the process
//! itself or one it descends from.

use super::Ctx;
use super::claude::proc_stat;

const SESSION: &str = "org.kde.konsole.Session";
const WINDOW: &str = "org.kde.konsole.Window";
/// As far up the process tree as a tab's shell is looked for.
const MAX_DEPTH: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Tab {
    /// The Konsole's name on the bus.
    service: String,
    /// `/Windows/1`.
    window: String,
    /// The number in `/Sessions/1`.
    session: i32,
}

impl Tab {
    fn session_path(&self) -> String {
        format!("/Sessions/{}", self.session)
    }
}

/// The tab an environment block (`/proc/<pid>/environ`) claims to be in.
fn claimed_tab(environ: &[u8]) -> Option<Tab> {
    let var = |name: &str| {
        environ
            .split(|&b| b == 0)
            .find_map(|entry| entry.strip_prefix(name.as_bytes())?.strip_prefix(b"="))
            .and_then(|value| std::str::from_utf8(value).ok())
            .filter(|value| !value.is_empty())
    };
    Some(Tab {
        service: var("KONSOLE_DBUS_SERVICE")?.to_string(),
        window: var("KONSOLE_DBUS_WINDOW")?.to_string(),
        session: var("KONSOLE_DBUS_SESSION")?
            .strip_prefix("/Sessions/")?
            .parse()
            .ok()?,
    })
}

/// `pid`, its parent, and so on up.
fn lineage(pid: i32) -> Vec<i32> {
    let mut all = vec![pid];
    let mut at = pid;
    while all.len() < MAX_DEPTH {
        match proc_stat(at) {
            Some((parent, _)) if parent > 1 => {
                all.push(parent);
                at = parent;
            }
            _ => break,
        }
    }
    all
}

/// The Konsole tab `pid` really runs in. A Konsole that has gone, or doesn't know the tab, makes it
/// none at all: this only ever adds to going to the window.
async fn find_tab(conn: &zbus::Connection, pid: i32) -> Option<Tab> {
    let environ = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    let tab = claimed_tab(&environ)?;
    let reply = conn
        .call_method(
            Some(tab.service.as_str()),
            tab.session_path().as_str(),
            Some(SESSION),
            "processId",
            &(),
        )
        .await
        .ok()?;
    let (shell,): (i32,) = reply.body().deserialize().ok()?;
    lineage(pid).contains(&shell).then_some(tab)
}

async fn select(conn: &zbus::Connection, pid: i32) -> zbus::Result<()> {
    let Some(tab) = find_tab(conn, pid).await else {
        return Ok(());
    };
    conn.call_method(
        Some(tab.service.as_str()),
        tab.window.as_str(),
        Some(WINDOW),
        "setCurrentSession",
        &(tab.session,),
    )
    .await
    .map(|_| ())
}

/// Bring up the Konsole tab process `pid` runs in, if that is where it runs.
pub fn show_tab(ctx: &Ctx, pid: i32) {
    let c = ctx.clone();
    ctx.spawn(async move {
        // Not worth a toast: the window comes up either way, just on another tab.
        let shown = async { select(&c.bus(false).await?, pid).await };
        if let Err(e) = shown.await {
            log::warn!("Showing a Konsole tab: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_the_environment_claims() {
        let tab = claimed_tab(
            b"HOME=/home/me\0KONSOLE_DBUS_SERVICE=:1.3532\0KONSOLE_DBUS_WINDOW=/Windows/1\0\
              KONSOLE_DBUS_SESSION=/Sessions/2\0KONSOLE_VERSION=260400\0",
        );
        assert_eq!(
            tab,
            Some(Tab {
                service: ":1.3532".into(),
                window: "/Windows/1".into(),
                session: 2,
            })
        );
    }

    #[test]
    fn no_claim_without_all_of_it() {
        assert_eq!(claimed_tab(b"HOME=/home/me\0"), None);
        assert_eq!(claimed_tab(b""), None);
        let partial = b"KONSOLE_DBUS_SERVICE=:1.1\0KONSOLE_DBUS_WINDOW=/Windows/1\0";
        assert_eq!(claimed_tab(partial), None);
        let odd = b"KONSOLE_DBUS_SERVICE=:1.1\0KONSOLE_DBUS_WINDOW=/Windows/1\0\
                    KONSOLE_DBUS_SESSION=/Elsewhere/2\0";
        assert_eq!(claimed_tab(odd), None);
        // A name that only starts the same isn't the variable.
        let longer = b"KONSOLE_DBUS_SERVICE_X=:1.1\0KONSOLE_DBUS_WINDOW=/Windows/1\0\
                       KONSOLE_DBUS_SESSION=/Sessions/2\0";
        assert_eq!(claimed_tab(longer), None);
    }

    #[test]
    fn lineage_starts_at_the_process_and_goes_up() {
        let me = std::process::id() as i32;
        let line = lineage(me);
        assert_eq!(line[0], me);
        assert!(line.len() > 1);
        assert!(!line.contains(&1));
        // One that doesn't exist has only itself.
        assert_eq!(lineage(i32::MAX), [i32::MAX]);
    }

    /// Against a real Konsole, whose tabs it switches: start a throwaway one and run this in its
    /// D-Bus session, with NIRI_DASH_TEST_KONSOLE=1.
    #[tokio::test]
    #[ignore = "needs a Konsole to switch the tabs of"]
    async fn tabs_in_a_real_konsole() {
        assert!(
            std::env::var_os("NIRI_DASH_TEST_KONSOLE").is_some(),
            "this switches tabs: set NIRI_DASH_TEST_KONSOLE=1 in a throwaway session"
        );
        let conn = zbus::Connection::session().await.unwrap();
        let dbus = zbus::fdo::DBusProxy::new(&conn).await.unwrap();
        let service = dbus
            .list_names()
            .await
            .unwrap()
            .into_iter()
            .map(|n| n.to_string())
            .find(|n| n.starts_with("org.kde.konsole"))
            .expect("a Konsole on the bus");
        let call = async |path: &str, iface: &str, method: &str| {
            conn.call_method(Some(service.as_str()), path, Some(iface), method, &())
                .await
                .unwrap()
        };
        let current = async || -> i32 {
            call("/Windows/1", WINDOW, "currentSession")
                .await
                .body()
                .deserialize::<(i32,)>()
                .unwrap()
                .0
        };
        let shell = async |id: i32| -> i32 {
            call(&format!("/Sessions/{id}"), SESSION, "processId")
                .await
                .body()
                .deserialize::<(i32,)>()
                .unwrap()
                .0
        };
        let first = current().await;
        let second = call("/Windows/1", WINDOW, "newSession")
            .await
            .body()
            .deserialize::<(i32,)>()
            .unwrap()
            .0;
        // Give the shell a moment to start and its process to take its environment.
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        assert_ne!(first, second);

        // A tab's own shell, and something running inside it, get to their tab and back.
        for id in [first, second, first] {
            let pid = shell(id).await;
            let found = find_tab(&conn, pid).await.expect("the shell's tab");
            assert_eq!(found.session, id);
            select(&conn, pid).await.unwrap();
            assert_eq!(current().await, id, "shell {pid}");
        }

        // What an editor started from Konsole, or a tmux server, hands on: the variables of a real
        // tab, in a process that isn't in it. The tab stays as it is.
        let forged = |service: &str, session: i32| {
            std::process::Command::new("sleep")
                .arg("30")
                .env("KONSOLE_DBUS_SERVICE", service)
                .env("KONSOLE_DBUS_WINDOW", "/Windows/1")
                .env("KONSOLE_DBUS_SESSION", format!("/Sessions/{session}"))
                .spawn()
                .unwrap()
        };
        let mut stray = forged(&service, second);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(find_tab(&conn, stray.id() as i32).await, None);
        select(&conn, stray.id() as i32).await.unwrap();
        assert_eq!(current().await, first);
        stray.kill().unwrap();
        stray.wait().unwrap();

        // A Konsole long gone, and a tab that isn't there.
        for (name, session) in [(":1.999999", second), (service.as_str(), 9999)] {
            let mut stray = forged(name, session);
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            assert_eq!(find_tab(&conn, stray.id() as i32).await, None);
            stray.kill().unwrap();
            stray.wait().unwrap();
        }
        assert_eq!(current().await, first);
    }
}

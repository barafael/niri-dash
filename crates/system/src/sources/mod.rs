//! Background data sources. Each one runs on the tokio runtime (or its own thread), sends plain
//! [`Event`]s to the UI thread through a calloop channel, and exposes methods its actor
//! (`system/live.rs`) calls for commands. Polled sources only poll while the dashboard is visible.

pub mod audio;
pub mod bluetooth;
pub mod brightness;
pub mod claude;
pub mod claude_usage;
pub mod icons;
pub mod inhibit;
pub mod kde;
pub mod kdeconnect;
pub mod media;
pub mod niri;
pub mod notifications;
pub mod osd;
pub mod perf;
pub mod pipewire;
pub mod power;
pub mod privacy;
pub mod session;
pub mod tray;
pub mod udisks;
pub mod weather;
pub mod wifi;

use smithay_client_toolkit::reexports::calloop::channel::Sender;
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, watch};
use zbus::zvariant::{OwnedValue, Value};

pub use crate::text::{clean_label, locale, one_line, strip_spinner, unescape_entities};
pub use crate::{Compositor, Event, IconSrc, clipboard, keyboard};

/// Wakes one source early, e.g. when a change event says its data is stale. If the source is
/// busy, the wake-up is kept for when it's done.
#[derive(Clone, Default)]
pub struct Trigger(Arc<Notify>);

impl Trigger {
    pub fn fire(&self) {
        self.0.notify_one();
    }
}

/// The last value a source sent, to skip sending the same again. Every update replaces the UI's
/// models and redraws, and with the GPU renderer every frame makes niri recomposite the whole
/// screen, so refreshes that change nothing (most of them) shouldn't reach the UI.
pub struct Dedup<T>(std::sync::Mutex<Option<T>>);

impl<T> Default for Dedup<T> {
    fn default() -> Self {
        Self(std::sync::Mutex::new(None))
    }
}

impl<T: PartialEq + Clone> Dedup<T> {
    /// Whether `v` differs from the last value seen; remembers it if so.
    pub fn changed(&self, v: &T) -> bool {
        let Ok(mut last) = self.0.lock() else {
            return true;
        };
        if last.as_ref() == Some(v) {
            return false;
        }
        *last = Some(v.clone());
        true
    }
}

/// The shortest time between two runs of a poller.
const MIN_POLL_GAP: Duration = Duration::from_millis(150);

/// D-Bus match rule for all signals from one bus name.
pub fn signals_from(sender: &'static str) -> zbus::MatchRule<'static> {
    zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender(sender)
        .expect("valid bus name")
        .build()
}

/// One connection per bus for the signal watchers, made on first use and dropped when it fails.
#[derive(Default)]
struct Buses {
    system: tokio::sync::Mutex<Option<zbus::Connection>>,
    session: tokio::sync::Mutex<Option<zbus::Connection>>,
}

#[derive(Clone)]
pub struct Ctx {
    tx: Sender<Event>,
    /// Whether the dashboard is up; watchers and pollers wait on this rather than waking to check.
    visible: watch::Sender<bool>,
    wake: Arc<Notify>,
    buses: Arc<Buses>,
    /// A dormant context starts nothing: a source built with it stays idle (standalone apps
    /// only run the sources their widget shows).
    dormant: bool,
    rt: tokio::runtime::Handle,
    /// Which compositor we run under, decided when the system started.
    compositor: Compositor,
}

impl Ctx {
    pub fn new(tx: Sender<Event>, rt: tokio::runtime::Handle, compositor: Compositor) -> Self {
        Self {
            tx,
            visible: watch::Sender::new(false),
            wake: Arc::new(Notify::new()),
            buses: Default::default(),
            dormant: false,
            rt,
            compositor,
        }
    }

    pub fn compositor(&self) -> Compositor {
        self.compositor
    }

    pub fn is_kwin(&self) -> bool {
        self.compositor == Compositor::Kwin
    }

    /// This context, or a dormant copy of it unless `live`.
    pub fn live_if(&self, live: bool) -> Self {
        Self {
            dormant: self.dormant || !live,
            ..self.clone()
        }
    }

    pub fn send(&self, event: impl Into<Event>) {
        let _ = self.tx.send(event.into());
    }

    pub fn toast(&self, text: impl Into<String>) {
        self.send(Event::Toast(text.into()));
    }

    pub fn visible(&self) -> bool {
        *self.visible.borrow()
    }

    pub fn set_visible(&self, v: bool) {
        self.visible
            .send_if_modified(|cur| std::mem::replace(cur, v) != v);
        if v {
            self.refresh();
        }
    }

    /// Wait until the dashboard is shown (`true`) or hidden (`false`).
    pub async fn until_visible(&self, v: bool) {
        let mut rx = self.visible.subscribe();
        let _ = rx.wait_for(|&cur| cur == v).await;
    }

    /// The shared system or session bus connection.
    pub async fn bus(&self, system: bool) -> zbus::Result<zbus::Connection> {
        let slot = if system {
            &self.buses.system
        } else {
            &self.buses.session
        };
        let mut slot = slot.lock().await;
        if let Some(c) = slot.as_ref() {
            return Ok(c.clone());
        }
        let c = if system {
            zbus::Connection::system().await?
        } else {
            zbus::Connection::session().await?
        };
        *slot = Some(c.clone());
        Ok(c)
    }

    /// Forget a bus connection that failed, so the next `bus` call reconnects.
    async fn drop_bus(&self, system: bool) {
        let slot = if system {
            &self.buses.system
        } else {
            &self.buses.session
        };
        *slot.lock().await = None;
    }

    /// The runtime the sources run on.
    pub fn runtime(&self) -> &tokio::runtime::Handle {
        &self.rt
    }

    /// Wake every poller now.
    pub fn refresh(&self) {
        self.wake.notify_waiters();
    }

    /// Run `f` every `period` while the dashboard is visible, and right away on `refresh()`.
    pub fn poll<F, Fut>(&self, period: Duration, mut f: F)
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send,
    {
        self.poll_on(Trigger::default(), move || {
            let fut = f();
            async move {
                fut.await;
                period
            }
        });
    }

    /// Run `f` when `trigger` fires (and on show), with a slow fallback poll in case an event
    /// is missed.
    pub fn poll_events<F, Fut>(&self, trigger: Trigger, fallback: Duration, mut f: F)
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send,
    {
        self.poll_on(trigger, move || {
            let fut = f();
            async move {
                fut.await;
                fallback
            }
        });
    }

    /// Like `poll`, but also runs right away whenever `trigger` fires (a change event), and `f`
    /// returns how long to wait until the next fallback poll.
    pub fn poll_on<F, Fut>(&self, trigger: Trigger, mut f: F)
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = Duration> + Send,
    {
        if self.dormant {
            return;
        }
        let ctx = self.clone();
        self.rt.spawn(async move {
            loop {
                // Hidden: sleep until shown (which also refreshes) rather than waking to check.
                ctx.until_visible(true).await;
                let woken = ctx.wake.notified();
                let period = f().await;
                // Bursts of change events (a volume scroll is one per step) arrive faster than a
                // fetch is worth; ones that come in meanwhile are folded into the next run.
                tokio::time::sleep(MIN_POLL_GAP).await;
                tokio::select! {
                    _ = tokio::time::sleep(period) => {}
                    _ = woken => {}
                    _ = trigger.0.notified() => {}
                }
            }
        });
    }

    /// Fire `trigger` whenever a D-Bus signal matching `rule` arrives; signals within `settle` of
    /// each other are coalesced into one refresh. Only subscribed while the dashboard is shown:
    /// the pollers don't run while hidden, and showing refreshes them anyway.
    pub fn on_dbus_signals(
        &self,
        system: bool,
        rule: zbus::MatchRule<'static>,
        settle: Duration,
        trigger: Trigger,
    ) {
        use futures_util::{FutureExt, StreamExt};
        if self.dormant {
            return;
        }
        let ctx = self.clone();
        self.rt.spawn(async move {
            loop {
                ctx.until_visible(true).await;
                let watch = async {
                    let conn = ctx.bus(system).await?;
                    let mut stream =
                        zbus::MessageStream::for_match_rule(rule.clone(), &conn, Some(256)).await?;
                    while stream.next().await.is_some() {
                        tokio::time::sleep(settle).await;
                        while let Some(Some(_)) = stream.next().now_or_never() {}
                        log::debug!("change: {rule}");
                        trigger.fire();
                    }
                    zbus::Result::Ok(())
                };
                // Dropping the stream on hide removes the match rule from the bus.
                let result = tokio::select! {
                    r = watch => r,
                    _ = ctx.until_visible(false) => continue,
                };
                if let Err(e) = result {
                    log::debug!("dbus watch {rule}: {e}");
                    ctx.drop_bus(system).await;
                }
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        });
    }

    /// Run a monitoring command (udevadm monitor, pactl subscribe) and fire `trigger` for every
    /// output line `matches` accepts. Restarts the command if it exits. Like the D-Bus watchers,
    /// it only runs while the dashboard is shown and is killed on hide.
    pub fn on_command_lines(
        &self,
        program: &'static str,
        args: &'static [&'static str],
        matches: fn(&str) -> bool,
        trigger: Trigger,
    ) {
        use tokio::io::AsyncBufReadExt;
        if self.dormant {
            return;
        }
        let ctx = self.clone();
        self.rt.spawn(async move {
            loop {
                ctx.until_visible(true).await;
                let child = tokio::process::Command::new(program)
                    .args(args)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .kill_on_drop(true)
                    .spawn();
                match child {
                    Ok(mut child) => {
                        let out = child.stdout.take();
                        let read = async {
                            if let Some(out) = out {
                                let mut lines = tokio::io::BufReader::new(out).lines();
                                while let Ok(Some(line)) = lines.next_line().await {
                                    if matches(&line) {
                                        log::debug!("change: {program}: {line}");
                                        trigger.fire();
                                    }
                                }
                            }
                            let _ = child.wait().await;
                        };
                        // On hide, dropping `read` drops the child, which kills it.
                        tokio::select! {
                            _ = read => {}
                            _ = ctx.until_visible(false) => continue,
                        }
                    }
                    Err(e) => {
                        log::warn!("{program}: {e}; falling back to polling");
                        return;
                    }
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
    }

    /// Fire `trigger` when files in `dir` change. Like the other watchers, it only watches while
    /// the dashboard is shown. A directory that doesn't exist (yet) isn't watched; the source's
    /// fallback poll covers it.
    pub fn on_dir_changes(&self, dir: PathBuf, trigger: Trigger) {
        use notify::{RecursiveMode, Watcher};
        if self.dormant {
            return;
        }
        let ctx = self.clone();
        self.rt.spawn(async move {
            loop {
                ctx.until_visible(true).await;
                let (t, d) = (trigger.clone(), dir.clone());
                let watcher =
                    notify::recommended_watcher(move |ev: notify::Result<notify::Event>| {
                        if ev.is_ok_and(|e| !e.kind.is_access()) {
                            log::debug!("change: {}", d.display());
                            t.fire();
                        }
                    })
                    .and_then(|mut w| w.watch(&dir, RecursiveMode::NonRecursive).map(|()| w));
                if let Err(e) = &watcher {
                    log::debug!("watching {}: {e}", dir.display());
                }
                // On hide, dropping the watcher stops it.
                ctx.until_visible(false).await;
                drop(watcher);
            }
        });
    }

    /// A channel where only the latest value counts: `apply` runs for the newest one whenever it
    /// is free, so slider drags don't queue up work.
    pub fn latest<T, F, Fut>(&self, mut apply: F) -> watch::Sender<Option<T>>
    where
        T: Clone + Send + Sync + 'static,
        F: FnMut(T) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send,
    {
        let (tx, mut rx) = watch::channel(None::<T>);
        self.spawn(async move {
            while rx.changed().await.is_ok() {
                let v = rx.borrow_and_update().clone();
                if let Some(v) = v {
                    apply(v).await;
                }
            }
        });
        tx
    }

    pub fn spawn<Fut>(&self, fut: Fut)
    where
        Fut: Future<Output = ()> + Send + 'static,
    {
        if !self.dormant {
            self.rt.spawn(fut);
        }
    }

    /// Spawn an action; on error, show it as a toast. Then refresh the pollers.
    pub fn act<Fut, E>(&self, what: &'static str, fut: Fut)
    where
        Fut: Future<Output = Result<(), E>> + Send + 'static,
        E: std::fmt::Display,
    {
        let ctx = self.clone();
        self.rt.spawn(async move {
            if let Err(e) = fut.await {
                log::warn!("{what}: {e}");
                ctx.toast(format!("{what}: {e}"));
            }
            ctx.refresh();
        });
    }
}

/// Running a helper program failed.
#[derive(Debug, thiserror::Error)]
pub enum CmdError {
    #[error("running {program}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    /// It ran and exited with an error; the message is what it printed.
    #[error("{message}")]
    Failed { program: String, message: String },
    #[error("{program} took over {} s", .after.as_secs())]
    TimedOut { program: String, after: Duration },
}

/// Run a command and return its stdout; a non-zero exit is an error carrying stderr.
pub async fn run(program: &str, args: &[&str]) -> Result<String, CmdError> {
    run_with(program, args, &[], None).await
}

/// Like [`run`], with extra environment variables, and killed if it takes longer than `timeout`.
pub async fn run_with(
    program: &str,
    args: &[&str],
    env: &[(&str, &str)],
    timeout: Option<Duration>,
) -> Result<String, CmdError> {
    let output = tokio::process::Command::new(program)
        .args(args)
        .envs(env.iter().copied())
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let output = match timeout {
        Some(after) => {
            tokio::time::timeout(after, output)
                .await
                .map_err(|_| CmdError::TimedOut {
                    program: program.into(),
                    after,
                })?
        }
        None => output.await,
    };
    let out = output.map_err(|source| CmdError::Spawn {
        program: program.into(),
        source,
    })?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        let msg = if err.is_empty() {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        } else {
            err.to_string()
        };
        return Err(CmdError::Failed {
            program: program.into(),
            message: msg,
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Start a program detached from us (no output capture, not killed with us).
pub fn spawn_detached(program: &str, args: &[&str]) -> Result<(), CmdError> {
    use std::os::unix::process::CommandExt;
    let child = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|source| CmdError::Spawn {
            program: program.into(),
            source,
        })?;
    // Reap it when it exits.
    let mut child = child;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// A sysfs attribute: the file's contents, trimmed and parsed.
pub fn sysfs<T: FromStr>(path: impl AsRef<Path>) -> Option<T> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// A value from a D-Bus property map (`a{sv}`), if it's there with the right type.
pub fn prop<'a, T>(map: &'a HashMap<String, OwnedValue>, key: &str) -> Option<T>
where
    T: TryFrom<&'a Value<'a>>,
    <T as TryFrom<&'a Value<'a>>>::Error: Into<zbus::zvariant::Error>,
{
    let v: &'a Value<'a> = map.get(key)?;
    v.downcast_ref().ok()
}

/// A string from a D-Bus property map.
pub fn prop_str(map: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    prop::<&str>(map, key).map(str::to_string)
}

pub fn which(program: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|p| p.join(program))
            .find(|p| p.is_file())
    })
}

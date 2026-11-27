//! Running the dashboard: the BUI application, the window it shows in, and what feeds the
//! [`Dash`]: the system's events, the control socket, a ticker and a watcher on config.kdl.
//!
//! The window is a layer-shell overlay over everything (a normal window for a standalone app).
//! BUI windows can't be hidden, so it is closed when the dashboard hides and opened again when it
//! shows; the `Dash` entity stays.

use crate::Control;
use crate::dash::{Dash, WinOp};
use crate::layout::{Kind, Layout};
use crate::view::DashView;
use blenderui::prelude::*;
use niri_dash_system::{self as sys, Feed};
use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// What the app keeps in BUI's globals: the dashboard, and its window while it is open.
pub struct Shell {
    pub dash: Entity<Dash>,
    pub window: Option<WindowId>,
    pub solo: Option<Kind>,
    /// What screen readers are served by (the real app only, not tests or snapshots).
    pub a11y: Option<crate::a11y::Bridge>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("niri-dash-bui is already running ({})", .0.display())]
    AlreadyRunning(PathBuf),
    #[error("binding {}: {source}", path.display())]
    Bind {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("starting the system: {0}")]
    System(#[from] std::io::Error),
}

/// Start the system the environment asks for: the simulated one (`NIRI_DASH_SIM`), the mock one
/// (`NIRI_DASH_MOCK`) or the real one, with only the feeds a standalone app needs.
pub fn start_system(events: sys::EventSender, solo: Option<Kind>) -> std::io::Result<sys::System> {
    if std::env::var_os("NIRI_DASH_SIM").is_some() {
        log::info!("running on the simulated system");
        return sys::simulated(events);
    }
    if std::env::var_os("NIRI_DASH_MOCK").is_some() {
        log::info!("running on the mock system");
        return sys::mock(events);
    }
    sys::live(
        events,
        sys::Options {
            feeds: solo.map(feeds),
            wayland: true,
            hold_night_light: solo.is_none(),
        },
    )
}

/// What a standalone app of `kind` reads from the system.
pub fn feeds(kind: Kind) -> &'static [Feed] {
    use Kind::*;
    match kind {
        SystemMonitor | Memory | Disks | Network | Gpu => &[Feed::Perf],
        Windows | KeyboardLayout | ColorPicker => &[Feed::Compositor],
        ClaudeSessions => &[Feed::Claude, Feed::Compositor],
        ClaudeUsage => &[Feed::ClaudeUsage],
        Wifi => &[Feed::Wifi],
        Bluetooth => &[Feed::Bluetooth],
        Battery => &[Feed::Power, Feed::Inhibit],
        Volume => &[Feed::Audio],
        Brightness => &[Feed::Brightness],
        Notifications => &[Feed::Notifications],
        Tray => &[Feed::Tray],
        Media => &[Feed::Media],
        Weather => &[Feed::Weather],
        Privacy => &[Feed::Privacy, Feed::Compositor],
        NightLight => &[Feed::NightLight],
        RemovableDrives => &[Feed::Drives],
        Phone => &[Feed::Phone],
        _ => &[],
    }
}

/// A sink for the system's events into the UI thread: a queue of [`sys::EVENT_QUEUE`], and one
/// wake-up of the loop for whatever is in it (a burst of events is one redraw).
pub fn event_sink(ui: UiSender) -> sys::EventSender {
    let (tx, rx) = std::sync::mpsc::sync_channel::<sys::Event>(sys::EVENT_QUEUE);
    let rx = Arc::new(std::sync::Mutex::new(rx));
    let woken = Arc::new(AtomicBool::new(false));
    sys::Sink::new("the dashboard's events", move |e| {
        tx.try_send(e).map_err(|e| match e {
            std::sync::mpsc::TrySendError::Full(_) => sys::Refused::Full,
            std::sync::mpsc::TrySendError::Disconnected(_) => sys::Refused::Closed,
        })?;
        if !woken.swap(true, Ordering::AcqRel) {
            let rx = rx.clone();
            let woken = woken.clone();
            ui.send(move |app| {
                woken.store(false, Ordering::Release);
                let events: Vec<sys::Event> = rx
                    .lock()
                    .map(|r| r.try_iter().collect())
                    .unwrap_or_default();
                with_dash(app, |d, cx| {
                    for e in events {
                        d.event(e);
                    }
                    if d.visible {
                        cx.notify();
                    }
                });
            });
        }
        Ok(())
    })
}

/// Do `f` to the dashboard.
pub fn with_dash<R>(
    app: &mut App,
    f: impl FnOnce(&mut Dash, &mut Context<Dash>) -> R,
) -> Option<R> {
    let dash = app.try_global::<Shell>()?.dash;
    Some(dash.update(app, f))
}

/// What the dashboard asked of its window ([`Dash::request`]).
pub fn apply(app: &mut App) {
    let Some(ops) = with_dash(app, |d, _| std::mem::take(&mut d.ops)) else {
        return;
    };
    for op in ops {
        match op {
            WinOp::Show => open(app),
            WinOp::Hide => {
                // The window goes once the exit animation is done, unless the dashboard came
                // back meanwhile.
                let ms = with_dash(app, |d, _| d.look.ms(80.0)).unwrap_or(0.0);
                let ui = with_dash(app, |d, _| d.ui.clone()).flatten();
                match ui.filter(|_| ms > 0.0) {
                    Some(ui) => {
                        std::thread::spawn(move || {
                            std::thread::sleep(Duration::from_millis(ms as u64 + 20));
                            ui.send(close_if_hidden);
                        });
                    }
                    None => close_if_hidden(app),
                }
            }
            WinOp::Quit => {
                log::info!("quitting");
                if let Some(kind) = app.global::<Shell>().solo {
                    let _ = std::fs::remove_file(crate::cli::named_socket_path(kind.name()));
                } else {
                    let _ = std::fs::remove_file(crate::cli::socket_path());
                }
                crate::files::flush();
                app.quit();
                std::process::exit(0);
            }
        }
    }
}

/// Close the window, if the dashboard is still meant to be hidden.
fn close_if_hidden(app: &mut App) {
    if with_dash(app, |d, _| d.visible).unwrap_or(false) {
        return;
    }
    if let Some(id) = app.global_mut::<Shell>().window.take() {
        app.close_window(id);
    }
}

/// Type into the text field the dashboard asked for ([`Dash::type_into`]). BUI starts editing a
/// field only on a click, so it gets one, in its middle.
pub fn apply_focus(app: &mut App) {
    let Some(id) = with_dash(app, |d, _| d.focus.take()).flatten() else {
        return;
    };
    let Some(win) = app.global::<Shell>().window else {
        return;
    };
    match app.element_bounds(win, &id) {
        Some(b) => app.simulate_click(
            win,
            b.origin.x + b.size.width / 2.0,
            b.origin.y + b.size.height / 2.0,
        ),
        None => log::debug!("no field {id} to type into"),
    }
}

/// The window's options: an overlay over the whole output, or a window of its own.
fn options(solo: Option<Kind>, headless_size: Option<(f32, f32)>) -> WindowOptions {
    let (w, h) = headless_size.unwrap_or((1280.0, 800.0));
    let bounds = Bounds {
        origin: point(px(0.0), px(0.0)),
        size: size(px(w), px(h)),
    };
    match solo {
        Some(kind) => {
            let (cw, ch) = kind.info().default_size;
            let (w, h) = headless_size.unwrap_or((
                (cw as f32 * 110.0).max(320.0),
                (ch as f32 * 110.0 + 60.0).max(240.0),
            ));
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(0.0), px(0.0)),
                    size: size(px(w), px(h)),
                })),
                titlebar: Some(TitlebarOptions {
                    title: Some(format!("{} (niri-dash)", kind.info().title).into()),
                }),
                window_min_size: Some(size(px(240.0), px(160.0))),
                ..Default::default()
            }
        }
        None => WindowOptions {
            window_bounds: Some(WindowBounds::Fullscreen(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("niri-dash".into()),
            }),
            window_background: WindowBackgroundAppearance::Transparent,
            kind: WindowKind::LayerShell(LayerShellOptions {
                layer: Layer::Overlay,
                anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                exclusive_zone: None,
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
            }),
            ..Default::default()
        },
    }
}

/// Open the window, if it isn't.
fn open(app: &mut App) {
    let shell = app.global::<Shell>();
    if shell.window.is_some() {
        return;
    }
    let (dash, solo) = (shell.dash, shell.solo);
    match app.open_window(options(solo, None), move |_, cx| {
        cx.new(|_| DashView::new(dash))
    }) {
        Ok(w) => {
            log::info!("window {:?} open", w.id);
            app.global_mut::<Shell>().window = Some(w.id);
            app.set_app_id_for(w.id, solo);
        }
        Err(e) => log::error!("opening the window: {e}"),
    }
}

trait AppId {
    fn set_app_id_for(&mut self, id: WindowId, solo: Option<Kind>);
}

impl AppId for App {
    fn set_app_id_for(&mut self, _id: WindowId, _solo: Option<Kind>) {
        // BUI names the app on Linux from the window title; nothing to do.
    }
}

/// Listen on the control socket: each line a [`Control`], a night-light command or a mascot's
/// line, handed to the UI thread.
fn serve(listener: UnixListener, ui: UiSender) {
    std::thread::Builder::new()
        .name("niri-dash-socket".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let ui = ui.clone();
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    let line = line.trim().to_string();
                    if let Ok(c) = line.parse::<Control>() {
                        ui.send(move |app| {
                            with_dash(app, |d, cx| d.control(c, cx));
                        });
                    } else if let Some(cmd) = sys::night_light::Cmd::parse_wire(&line) {
                        ui.send(move |app| {
                            with_dash(app, |d, cx| {
                                d.tell(sys::night_light::Command::Apply {
                                    cmd,
                                    announce: true,
                                });
                                cx.notify();
                            });
                        });
                    } else if let Some(say) = crate::mascots::Say::parse_wire(&line) {
                        ui.send(move |app| {
                            with_dash(app, |d, cx| {
                                match say {
                                    Ok(say) => d.w.say(&say),
                                    Err(e) => log::warn!("a bad line for the mascots: {e}"),
                                }
                                cx.notify();
                            });
                        });
                    } else {
                        log::warn!("unknown command on the socket: {line:?}");
                    }
                }
            }
        })
        .expect("spawning the socket thread");
}

/// Tick ten times a second: timers go off while hidden too, so this always runs. The dashboard
/// draws again only when a tick changed something.
fn ticker(ui: UiSender) {
    std::thread::Builder::new()
        .name("niri-dash-tick".into())
        .spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(100));
                let alive = ui.send(|app| {
                    with_dash(app, |d, cx| {
                        if d.tick() {
                            cx.notify();
                        }
                    });
                    crate::a11y::update(app);
                });
                if !alive {
                    return;
                }
            }
        })
        .expect("spawning the ticker");
}

/// Watch config.kdl and the theme directories; a change reloads them.
fn watch_config(ui: UiSender) -> Option<notify::RecommendedWatcher> {
    use notify::Watcher;
    let path = Layout::path();
    let dir = path.parent()?.to_path_buf();
    let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        if !(ev.kind.is_modify() || ev.kind.is_create()) {
            return;
        }
        if ev
            .paths
            .iter()
            .any(|p| p.extension().is_some_and(|e| e == "kdl"))
        {
            ui.send(|app| {
                with_dash(app, |d, cx| {
                    d.reload();
                    cx.notify();
                });
            });
        }
    })
    .map_err(|e| log::warn!("can't watch the config: {e}"))
    .ok()?;
    if let Err(e) = w.watch(&dir, notify::RecursiveMode::Recursive) {
        log::warn!("watching {}: {e}", dir.display());
    }
    Some(w)
}

/// Bind the control socket, unless someone answers there already.
fn bind(path: &PathBuf) -> Result<UnixListener, Error> {
    if std::os::unix::net::UnixStream::connect(path).is_ok() {
        return Err(Error::AlreadyRunning(path.clone()));
    }
    let _ = std::fs::remove_file(path);
    UnixListener::bind(path).map_err(|source| Error::Bind {
        path: path.clone(),
        source,
    })
}

/// Run the dashboard (`solo`: a standalone app of one widget) until it is told to quit.
pub fn run(first: Option<Control>, solo: Option<Kind>) -> Result<(), Error> {
    let sock = match solo {
        Some(k) => crate::cli::named_socket_path(k.name()),
        None => crate::cli::socket_path(),
    };
    let listener = bind(&sock)?;
    let mut failed = None;
    Application::new().keep_alive(true).run(|cx| {
        let ui = cx.ui_sender();
        let system = match start_system(event_sink(ui.clone()), solo) {
            Ok(s) => s,
            Err(e) => {
                failed = Some(e);
                cx.quit();
                return;
            }
        };
        let config = Layout::load();
        let dash = cx.new(|_| {
            let mut d = Dash::new(system, config, solo);
            d.ui = Some(ui.clone());
            d
        });
        cx.set_global(Shell {
            dash,
            window: None,
            solo,
            a11y: Some(crate::a11y::Bridge::start(ui.clone())),
        });
        cx.on_window_activation(|id, active, app| {
            if app.global::<Shell>().window == Some(id) {
                crate::a11y::window_focus(app, active);
            }
        });
        cx.on_window_closed(|id, app| {
            log::info!("window {id:?} closed");
            let shell = app.global_mut::<Shell>();
            if shell.window == Some(id) {
                shell.window = None;
                let solo = shell.solo.is_some();
                with_dash(app, |d, cx| {
                    d.window_closed();
                    if solo {
                        d.request(WinOp::Quit);
                    }
                    cx.notify();
                });
            }
        });
        serve(listener, ui.clone());
        ticker(ui.clone());
        // Kept for as long as the app runs.
        let watcher = watch_config(ui.clone());
        cx.set_global(watcher);
        let first = first.or(solo.map(|_| Control::Show));
        if let Some(c) = first {
            with_dash(cx, |d, cx| d.control(c, cx));
        }
    });
    log::info!("the event loop ended");
    if let Some(e) = failed {
        return Err(e.into());
    }
    let _ = std::fs::remove_file(sock);
    Ok(())
}

/// The options [`crate::snapshot`] opens its offscreen window with.
pub fn headless_options(solo: Option<Kind>, w: f32, h: f32) -> WindowOptions {
    options(solo, Some((w, h)))
}

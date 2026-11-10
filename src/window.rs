//! Where the dashboard appears, and what feeds it: a layer surface over everything (or a normal
//! window for a standalone app), the system's events, the control socket, and the clock that
//! ticks while it is up. The state is the [`Dash`] entity, which outlives the window: hiding the
//! dashboard closes the window, showing it opens one.

use crate::dash::{Dash, DashEvent};
use crate::layout::{Kind, Layout, Loaded};
use crate::{Control, named_socket_path, socket_path};
use gpui::layer_shell::{Anchor, KeyboardInteractivity, Layer, LayerShellOptions};
use gpui::{
    AnyWindowHandle, App, AppContext as _, AsyncApp, Entity, TitlebarOptions, WindowBackgroundAppearance,
    WindowHandle, WindowKind, WindowOptions, px, size,
};
use niri_dash_system::{self as sys, Event, Feed, Started};
use std::cell::RefCell;
use std::io::{BufRead, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

/// Starting the dashboard failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("niri-dash is already running ({})", .0.display())]
    AlreadyRunning(PathBuf),
    #[error("binding {}: {source}", path.display())]
    Bind {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("starting the system: {0}")]
    System(#[from] std::io::Error),
    #[error("{0}")]
    Ui(String),
    /// The stress run found the dashboard in a state it shouldn't be in.
    #[error("stress run failed {0}")]
    Stress(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// What shows the dashboard.
#[derive(Debug, Clone)]
pub enum Mode {
    /// A layer surface over everything, on the focused output.
    Dashboard,
    /// A standalone widget in a normal window.
    App {
        app_id: String,
        title: String,
        size: (u32, u32),
        min_size: (u32, u32),
    },
}

/// What reaches the dashboard from outside the system: the control socket and the config
/// watcher.
#[derive(Debug)]
pub enum Input {
    Control(Control),
    /// config.kdl or the theme file changed on disk.
    ConfigChanged,
    /// `niri-dash night-light …`, or a standalone night-light app's change.
    NightLight(sys::night_light::Cmd),
    /// A line for the mascots, from `niri-dash say`.
    MascotSay(crate::mascots::Say),
    /// A key to press, from a test's control socket line `key ctrl+h` (only with
    /// `NIRI_DASH_TEST_KEYS` set: the screen reader tests have no keyboard to type on).
    Key(String),
}

/// The sources a standalone app of `kind` has to run. Clock and calendar tick on their own, the
/// clipboard comes from the Wayland side.
fn feeds(kind: Kind) -> &'static [Feed] {
    match kind {
        Kind::Clock | Kind::Calendar | Kind::Clipboard | Kind::Session => &[],
        Kind::SystemMonitor | Kind::Memory | Kind::Disks | Kind::Network | Kind::Gpu => {
            &[Feed::Perf]
        }
        Kind::Windows => &[Feed::Compositor],
        Kind::ClaudeSessions => &[Feed::Claude],
        Kind::ClaudeUsage => &[Feed::ClaudeUsage],
        Kind::Wifi => &[Feed::Wifi],
        Kind::Bluetooth => &[Feed::Bluetooth],
        Kind::Battery => &[Feed::Power, Feed::Inhibit],
        Kind::Volume => &[Feed::Audio],
        Kind::Brightness => &[Feed::Brightness],
        Kind::Notifications => &[Feed::Notifications],
        Kind::Tray => &[Feed::Tray],
        Kind::Media => &[Feed::Media],
        Kind::Weather => &[Feed::Weather],
        Kind::Mascot => &[],
        Kind::Privacy => &[Feed::Compositor, Feed::Privacy],
        Kind::Notes => &[],
        Kind::Timer => &[Feed::Notifications],
        Kind::Stopwatch => &[],
        Kind::ColorPicker => &[],
        Kind::KeyboardLayout => &[Feed::Compositor],
        // A standalone night-light app asks the daemon, which holds the gamma control (or the
        // pause of KWin's night light).
        Kind::NightLight => &[Feed::NightLight],
        Kind::RemovableDrives => &[Feed::Drives],
        Kind::Phone => &[Feed::Phone],
        // The sleep timer's actor always runs; there is nothing to watch.
        Kind::SleepTimer => &[],
        // Its notices are local ones, like the timer's.
        Kind::Pomodoro => &[Feed::Notifications],
        Kind::About => &[],
    }
}

/// Whether a standalone app keeps running when its window closes, so the next `niri-dash app`
/// shows it again with its state. The clipboard does: its history is what it is for.
fn resident(kind: Kind) -> bool {
    kind == Kind::Clipboard
}

/// The real system, with the sources `solo`'s widget needs (all of them for the dashboard),
/// reporting to `events`. Also decides which compositor this is, before anything asks. For
/// development, `NIRI_DASH_MOCK=1` runs on the mock system's made-up data instead, and
/// `NIRI_DASH_SIM=1` on the simulated system (the web version's, a made-up machine that keeps
/// moving).
pub fn start_system(events: sys::EventSender, solo: Option<Kind>) -> Result<Started> {
    if std::env::var_os("NIRI_DASH_SIM").is_some() {
        log::info!("running on the simulated system");
        return Ok(sys::simulated(events)?);
    }
    if std::env::var_os("NIRI_DASH_MOCK").is_some() {
        log::info!("running on the mock system");
        return Ok(sys::mock(events)?);
    }
    // The clipboard and night light need a Wayland connection of their own: the window's belongs
    // to GPUI.
    let wayland = sys::wayland_connection();
    Ok(sys::live(
        events,
        sys::Options {
            feeds: solo.map(feeds),
            wayland,
            // Only the dashboard holds the gamma tables; a standalone app asks it.
            gamma: solo.is_none(),
        },
    )?)
}

pub fn run(initial: Option<Control>) -> Result<()> {
    let path = socket_path();
    if UnixStream::connect(&path).is_ok() {
        return Err(Error::AlreadyRunning(path));
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).map_err(|source| Error::Bind {
        path: path.clone(),
        source,
    })?;
    log::info!("niri-dash running; control socket {}", path.display());
    serve(Some(listener), Mode::Dashboard, None, initial, Layout::load())
}

/// Run one widget as a standalone app in a normal window, with a control socket of its own (so a
/// second `niri-dash app` shows this one instead of starting another).
pub fn run_app(kind: Kind) -> Result<()> {
    let path = named_socket_path(kind.name());
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).map_err(|source| Error::Bind {
        path: path.clone(),
        source,
    })?;
    // Open at the size the widget has on the dashboard; niri may tile it to another one.
    let config = Layout::load();
    let placed = config.layout.solo_widget(kind);
    let px = |cells: i32| (cells.max(1) as f32 * crate::layout::AUTO_CELL).round() as u32;
    let (min_w, min_h) = kind.info().min_size;
    let mode = Mode::App {
        app_id: crate::desktop::app_id(kind),
        title: crate::desktop::name(kind),
        size: (px(placed.w), px(placed.h)),
        min_size: (px(min_w), px(min_h)),
    };
    log::info!("{} running; control socket {}", kind.name(), path.display());
    let result = serve(Some(listener), mode, Some(kind), Some(Control::Show), config);
    let _ = std::fs::remove_file(&path);
    result
}

/// What reads the control socket: one line per connection.
fn control_server(listener: UnixListener, tx: sys::Sink<Input>) {
    std::thread::Builder::new()
        .name("control".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut line = String::new();
                let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                if BufReader::new(&stream).read_line(&mut line).is_err() {
                    continue;
                }
                if let Some(name) = line.trim().strip_prefix("key ")
                    && std::env::var_os("NIRI_DASH_TEST_KEYS").is_some()
                {
                    let _ = tx.send(Input::Key(name.trim().to_string()));
                    continue;
                }
                if let Some(cmd) = sys::night_light::Cmd::parse_wire(line.trim()) {
                    let _ = tx.send(Input::NightLight(cmd));
                    continue;
                }
                if let Some(say) = crate::mascots::Say::parse_wire(line.trim()) {
                    match say {
                        Ok(say) => {
                            let _ = tx.send(Input::MascotSay(say));
                        }
                        Err(e) => log::warn!("bad say command: {e}"),
                    }
                    continue;
                }
                let Ok(c) = line.trim().parse::<Control>() else {
                    log::warn!("unknown control command {:?}", line.trim());
                    continue;
                };
                let _ = tx.send(Input::Control(c));
            }
        })
        .expect("spawning control thread");
}

/// A sink that hands every value to an async receiver, from any thread.
pub fn channel<T: Send + 'static>() -> (sys::Sink<T>, async_channel::Receiver<T>) {
    let (tx, rx) = async_channel::unbounded();
    (
        sys::Sink::new(move |v| tx.send_blocking(v).map_err(|_| sys::Closed)),
        rx,
    )
}

/// The window the dashboard is in, if it is up.
#[derive(Clone)]
pub struct Host {
    pub dash: Entity<Dash>,
    mode: Mode,
    window: Rc<RefCell<Option<AnyWindowHandle>>>,
}

impl Host {
    pub fn new(dash: Entity<Dash>, mode: Mode) -> Self {
        Self {
            dash,
            mode,
            window: Default::default(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.window.borrow().is_some()
    }

    fn options(&self) -> WindowOptions {
        match &self.mode {
            Mode::Dashboard => WindowOptions {
                titlebar: None,
                // Size 0 by 0: the compositor stretches a surface anchored to all four edges to
                // the output; any size asked for would be kept instead.
                window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds {
                    origin: gpui::point(px(0.0), px(0.0)),
                    size: size(px(0.0), px(0.0)),
                })),
                focus: true,
                show: true,
                kind: WindowKind::LayerShell(LayerShellOptions {
                    namespace: "niri-dash".into(),
                    layer: Layer::Overlay,
                    anchor: Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT,
                    exclusive_zone: Some(px(-1.0)),
                    exclusive_edge: None,
                    margin: None,
                    // NIRI_DASH_NO_KEYBOARD=1 is for development: show without grabbing the keyboard.
                    keyboard_interactivity: if std::env::var_os("NIRI_DASH_NO_KEYBOARD").is_some() {
                        KeyboardInteractivity::None
                    } else {
                        KeyboardInteractivity::Exclusive
                    },
                }),
                window_background: WindowBackgroundAppearance::Transparent,
                app_id: Some("niri-dash".into()),
                ..WindowOptions::default()
            },
            Mode::App {
                app_id,
                title,
                size: s,
                min_size,
            } => WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(title.clone().into()),
                    ..TitlebarOptions::default()
                }),
                window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds {
                    origin: gpui::point(px(0.0), px(0.0)),
                    size: size(px(s.0 as f32), px(s.1 as f32)),
                })),
                window_min_size: Some(size(px(min_size.0 as f32), px(min_size.1 as f32))),
                window_background: WindowBackgroundAppearance::Transparent,
                app_id: Some(app_id.clone()),
                ..WindowOptions::default()
            },
        }
    }

    /// Put the dashboard on screen.
    pub fn open(&self, cx: &mut App) {
        if let Some(handle) = *self.window.borrow() {
            // Already up (a standalone app asked to show again): bring it to the front.
            let _ = handle.update(cx, |_, window, _| window.activate_window());
            return;
        }
        let dash = self.dash.clone();
        let opened: gpui::Result<WindowHandle<Dash>> = cx.open_window(self.options(), {
            let dash = dash.clone();
            move |window, cx| {
                let focus = dash.read(cx).focus.clone();
                window.focus(&focus, cx);
                // What a screen reader calls the window (a layer surface has no title bar to
                // take it from).
                window.set_window_title("niri-dash");
                dash.clone()
            }
        });
        match opened {
            Ok(handle) => *self.window.borrow_mut() = Some(handle.into()),
            Err(e) => {
                log::error!("opening the window: {e:#}");
                self.dash.update(cx, |d, cx| {
                    d.toast(&format!("Could not open the window: {e}"));
                    cx.notify();
                });
            }
        }
    }

    /// Press a key (`"j"`, `"ctrl+h"`) in the window, through GPUI's own dispatch as a keyboard
    /// would. For tests that drive the real window from outside (`tests/orca.rs`).
    fn press(&self, name: &str, cx: &mut App) {
        let (Some(key), Some(handle)) = (crate::keys::Key::parse(name), *self.window.borrow())
        else {
            log::warn!("no key {name:?}, or no window to press it in");
            return;
        };
        let Ok(keystroke) =
            gpui::Keystroke::parse_with_os(&key.to_gpui(), gpui::OperatingSystem::Linux)
        else {
            log::warn!("no keystroke for {name:?}");
            return;
        };
        let _ = handle.update(cx, |_, window, cx| window.dispatch_keystroke(keystroke, cx));
    }

    /// Take the dashboard off the screen; the state stays.
    pub fn close(&self, cx: &mut App) {
        if let Some(handle) = self.window.borrow_mut().take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    /// A control: what it asks of the window, then of the state.
    pub fn control(&self, c: Control, cx: &mut App) {
        let wants_open = match c {
            Control::Show | Control::Edit => true,
            Control::Toggle => !self.dash.read(cx).visible,
            _ => false,
        };
        match c {
            Control::Quit => {
                self.close(cx);
                cx.quit();
                return;
            }
            Control::Hide => self.close(cx),
            Control::Toggle if !wants_open => self.close(cx),
            _ if wants_open => self.open(cx),
            _ => {}
        }
        self.dash.update(cx, |d, cx| d.control(c, cx));
    }
}

/// The event loop shared by the dashboard and the standalone apps. Returns when a (non-resident)
/// app's window closes; the dashboard runs until it is told to quit.
fn serve(
    listener: Option<UnixListener>,
    mode: Mode,
    solo: Option<Kind>,
    initial: Option<Control>,
    config: Loaded,
) -> Result<()> {
    let (events_tx, events) = channel::<Event>();
    let (input_tx, inputs) = channel::<Input>();
    let system = start_system(events_tx, solo)?;
    if let Some(listener) = listener {
        control_server(listener, input_tx.clone());
    }
    let exit_when_closed = solo.is_some_and(|k| !resident(k));
    gpui_kit::application()
        .with_assets(crate::assets::Assets)
        // Hiding the dashboard closes its window; the process stays (a standalone app that isn't
        // resident quits itself when its window closes).
        .with_quit_mode(gpui::QuitMode::Explicit)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            let dash = cx.new(|cx| Dash::new(system, config, solo, cx));
            // config.kdl and the theme files are watched: an edit in an editor shows at once.
            let (watch, watched) = std::sync::mpsc::channel();
            crate::config::watch_config(watched, input_tx.clone());
            dash.update(cx, |d, _| {
                d.watch = Some(watch);
                d.watch_files();
            });
            let host = Host::new(dash.clone(), mode);
            cx.subscribe(&dash, {
                let host = host.clone();
                move |_, event: &DashEvent, cx| match event {
                    DashEvent::Hide => host.control(Control::Hide, cx),
                    DashEvent::Control(c) => host.control(*c, cx),
                }
            })
            .detach();
            pump_events(&dash, events, cx);
            pump_inputs(&host, inputs, cx);
            tick(&dash, cx);
            if exit_when_closed {
                // The window went (the compositor closed it, or Escape): so does the app.
                let host = host.clone();
                cx.on_window_closed(move |cx, _| {
                    if !host.is_open() {
                        cx.quit();
                    }
                })
                .detach();
            }
            if let Some(c) = initial {
                host.control(c, cx);
            }
        });
    Ok(())
}

/// What the system reports, applied on the UI's thread in bursts: one redraw for many events.
pub fn pump_events(dash: &Entity<Dash>, events: async_channel::Receiver<Event>, cx: &mut App) {
    let dash = dash.clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        const BATCH_TIME: Duration = Duration::from_millis(8);
        while let Ok(first) = events.recv().await {
            let started = std::time::Instant::now();
            dash.update(cx, |d, cx| {
                d.handle_event(first, cx);
                while started.elapsed() < BATCH_TIME
                    && let Ok(next) = events.try_recv()
                {
                    d.handle_event(next, cx);
                }
            });
            if !events.is_empty() {
                cx.background_executor().timer(Duration::from_millis(1)).await;
            }
        }
    })
    .detach();
}

fn pump_inputs(host: &Host, inputs: async_channel::Receiver<Input>, cx: &mut App) {
    let host = host.clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Ok(input) = inputs.recv().await {
            let host = host.clone();
            cx.update(|cx| match input {
                Input::Control(c) => host.control(c, cx),
                Input::ConfigChanged => {
                    host.dash.update(cx, |d, cx| d.reload_config(cx));
                }
                Input::NightLight(cmd) => {
                    host.dash.update(cx, |d, cx| d.night_light_command(cmd, cx));
                }
                Input::MascotSay(say) => {
                    host.dash.update(cx, |d, cx| d.mascot_say(say, cx));
                }
                Input::Key(name) => host.press(&name, cx),
            });
        }
    })
    .detach();
}

/// Once a second, just after the second changes, while the dashboard is up.
pub fn tick(dash: &Entity<Dash>, cx: &mut App) {
    let dash = dash.clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        loop {
            let ms = 1_005 - chrono::Local::now().timestamp_subsec_millis().min(999) as u64;
            cx.background_executor().timer(Duration::from_millis(ms)).await;
            dash.update(cx, |d, cx| d.tick(cx));
        }
    })
    .detach();
}

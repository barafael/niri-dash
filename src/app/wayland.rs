//! The dashboard on Wayland: the event loop that ties the shell, the system and the control socket
//! together, and starting it, as a daemon or as a standalone app.

use super::{App, Error, Result, Surface, build, new_ui, stress};
use crate::layout::{Kind, Layout, Loaded, RendererChoice};
use crate::shell::{Mode, Shell};
use crate::{Clipboard as UiClipboard, Control, named_socket_path, socket_path};
use niri_dash_system::{self as sys, Event, Feed, Started, Tell};
use slint::ComponentHandle;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use smithay_client_toolkit::reexports::calloop::{EventLoop, channel};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::{Connection, globals::registry_queue_init};
use std::io::{BufRead, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::rc::Rc;
use std::time::Duration;

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
        Kind::NiriDashInfo => &[],
    }
}
/// The pointer the UI asks for, as a cursor shape.
fn cursor_from_ui(p: crate::PointerShape) -> smithay_client_toolkit::seat::pointer::CursorIcon {
    use smithay_client_toolkit::seat::pointer::CursorIcon;
    match p {
        crate::PointerShape::Normal => CursorIcon::Default,
        crate::PointerShape::Move => CursorIcon::Move,
        crate::PointerShape::ResizeEw => CursorIcon::EwResize,
        crate::PointerShape::ResizeNs => CursorIcon::NsResize,
        crate::PointerShape::ResizeNwse => CursorIcon::NwseResize,
        crate::PointerShape::ResizeNesw => CursorIcon::NeswResize,
    }
}

impl Surface for Shell {
    fn is_visible(&self) -> bool {
        Shell::is_visible(self)
    }

    fn is_configured(&self) -> bool {
        Shell::is_configured(self)
    }

    fn presented(&self) -> u32 {
        self.presented
    }

    fn show(&mut self) {
        Shell::show(self);
    }

    fn hide(&mut self) {
        Shell::hide(self);
    }

    fn prepare(&mut self, app: &App) {
        app.prepare_for_output(self);
    }
}

struct LayerPlatform {
    window: Rc<dyn WindowAdapter>,
}

impl Platform for LayerPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }
}

/// What reaches the dashboard from outside the system: the control socket and the config
/// watcher.
pub(super) enum Input {
    Control(Control),
    /// config.kdl or the theme file changed on disk.
    ConfigChanged,
    /// `niri-dash night-light …`, or a standalone night-light app's change.
    NightLight(sys::night_light::Cmd),
    /// A line for the mascots, from `niri-dash say`.
    MascotSay(crate::mascots::Say),
    /// A screen reader wants something (see `a11y/bridge.rs`).
    Access(super::a11y::bridge::Request),
}

impl App {
    /// What the control socket or the config watcher asked for.
    pub(super) fn handle_input(&self, input: Input) -> Option<Control> {
        match input {
            Input::Control(c) => return Some(c),
            Input::ConfigChanged => self.reload_config(),
            Input::NightLight(cmd) => self.night_light(cmd, true),
            Input::MascotSay(say) => self.mascot_say(say),
            Input::Access(request) => self.access_request(request),
        }
        None
    }

    fn prepare_for_output(&self, shell: &mut Shell) {
        let (reply, output) = sys::oneshot::channel();
        self.tx
            .host
            .tell(sys::host::Command::FocusedOutput { reply });
        // Blocks until the compositor answers, as the first frame needs the size.
        if let Ok(Some((width, height, scale))) = output.blocking_recv() {
            shell.prepare(width, height, scale);
            self.sync_grid();
        }
    }
}

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

/// Whether a standalone app keeps running when its window closes, so the next `niri-dash app`
/// shows it again with its state. The clipboard does: its history is what it is for.
fn resident(kind: Kind) -> bool {
    kind == Kind::Clipboard
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
    serve(
        Some(listener),
        Mode::Dashboard,
        None,
        initial,
        Layout::load(),
        None,
    )
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
    let result = serve(
        Some(listener),
        mode,
        Some(kind),
        Some(Control::Show),
        config,
        None,
    );
    let _ = std::fs::remove_file(&path);
    result
}

/// The real system, with the sources `solo`'s widget needs (all of them for the dashboard),
/// reporting to `events`; with the clipboard and night light if it gets the Wayland connection.
/// Also decides which compositor this is, before anything asks. For development, `NIRI_DASH_MOCK=1`
/// runs on the mock system's made-up data instead, and `NIRI_DASH_SIM=1` on the web version's
/// simulated system.
pub(super) fn start_system(
    events: sys::EventSender,
    solo: Option<Kind>,
    wayland: Option<Connection>,
) -> Result<Started> {
    if std::env::var_os("NIRI_DASH_SIM").is_some() {
        log::info!("running on the simulated system");
        return Ok(sys::simulated(events)?);
    }
    if std::env::var_os("NIRI_DASH_MOCK").is_some() {
        log::info!("running on the mock system");
        return Ok(sys::mock(events)?);
    }
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

/// Have Slint draw into `window`.
pub(super) fn set_platform(window: Rc<dyn WindowAdapter>) -> Result<()> {
    slint::platform::set_platform(Box::new(LayerPlatform { window }))
        .map_err(|e| Error::Ui(format!("slint platform: {e}")))
}

/// The renderer `NIRI_DASH_RENDERER` asks for, overriding the config (for development).
pub(super) fn renderer_override() -> Option<RendererChoice> {
    std::env::var("NIRI_DASH_RENDERER")
        .ok()
        .and_then(|r| r.parse().ok())
}

/// `niri-dash stress` on screen: the whole dashboard in a normal window, on the mock system, with
/// no control socket. Ends when the keys run out or the window is closed.
pub(super) fn stress_window(mut run: stress::Run) -> Result<stress::Monkey> {
    let mode = Mode::App {
        app_id: "niri-dash-stress".into(),
        title: "niri-dash stress run".into(),
        size: (1280, 800),
        min_size: (640, 400),
    };
    let config = Layout::load();
    serve(
        None,
        mode,
        None,
        Some(Control::Show),
        config,
        Some(&mut run),
    )?;
    Ok(run.monkey)
}

/// The event loop shared by the dashboard and the standalone apps. Returns when a (non-resident)
/// app's window closes; the dashboard runs until it is told to quit. A stress run (`stress`)
/// runs on the mock system, keeps its window when the dashboard hides, and returns when it is
/// over.
fn serve(
    listener: Option<UnixListener>,
    mode: Mode,
    solo: Option<Kind>,
    initial: Option<Control>,
    config: Loaded,
    mut stress: Option<&mut stress::Run>,
) -> Result<()> {
    let (events_tx, events) = super::loop_channel::<Event>();
    let (input_tx, inputs) = super::loop_channel::<Input>();
    let conn = Connection::connect_to_env()?;
    let keep = stress.is_some();
    let system = if keep {
        sys::mock(events_tx)?
    } else {
        start_system(events_tx, solo, Some(conn.clone()))?
    };
    // A stress run's window stays up when the dashboard hides.
    let route = move |a: &App, shell: &mut Shell, c: Control| {
        if keep {
            a.control_in_place(c)
        } else {
            a.control(shell, c)
        }
    };
    let (globals, queue) = registry_queue_init::<Shell>(&conn)?;
    let mut event_loop: EventLoop<'static, Shell> = EventLoop::try_new()?;
    let backend = choose_backend(&conn, config.layout.renderer);
    let renderer = backend.renderer_name();
    let window = backend.adapter();
    set_platform(window.clone())?;
    let mut shell = Shell::new(
        conn.clone(),
        &globals,
        queue.handle(),
        event_loop.handle(),
        backend,
        mode,
    )?;
    WaylandSource::new(conn.clone(), queue)
        .insert(event_loop.handle())
        .map_err(|e| Error::Ui(format!("wayland source: {e}")))?;

    let ui = new_ui()?;

    let has_clipboard = system.caps.clipboard;
    let app = build(&ui, system, input_tx.clone(), solo, config, false);
    ui.global::<UiClipboard>().set_available(has_clipboard);
    super::info::show(&ui, renderer);
    // A screen reader finds the dashboard on the accessibility bus. Never in a stress run, which
    // is a test: tests keep off the session's bus.
    if stress.is_none() && std::env::var_os("NIRI_DASH_ATSPI").is_none_or(|v| v != "0") {
        app.access_attach(super::a11y::bridge::unix(input_tx.clone()));
    }

    if let Some(run) = stress.as_deref() {
        run.attach(&app);
    }
    shell.watch_pointer(super::hold::watcher(&app));
    let a = app.clone();
    event_loop
        .handle()
        .insert_source(events, move |ev, _, shell| {
            if let channel::Event::Msg(e) = ev
                && let Some(c) = a.handle_event(e)
            {
                route(&a, shell, c);
            }
        })
        .map_err(|e| Error::Ui(format!("event source: {e}")))?;
    let a = app.clone();
    event_loop
        .handle()
        .insert_source(inputs, move |ev, _, shell| {
            if let channel::Event::Msg(i) = ev
                && let Some(c) = a.handle_input(i)
            {
                route(&a, shell, c);
            }
        })
        .map_err(|e| Error::Ui(format!("input source: {e}")))?;
    if let Some(listener) = listener {
        control_server(listener, input_tx);
    }

    if solo.is_none() {
        // Size the UI for the current output right away, so the first show is already laid out.
        app.prepare_for_output(&mut shell);
    }
    if let Some(c) = initial {
        app.control(&mut shell, c);
    }
    let exit_when_closed = solo.is_some_and(|k| !resident(k));

    // NIRI_DASH_BENCH=1 is for development: redraw every frame, to measure what a frame costs.
    let bench = std::env::var_os("NIRI_DASH_BENCH").is_some();
    loop {
        let timeout = if shell.is_visible() {
            if window.window().has_active_animations() {
                Duration::from_millis(8)
            } else {
                slint::platform::duration_until_next_timer_update()
                    .unwrap_or(Duration::from_secs(1))
                    .min(Duration::from_secs(1))
            }
        } else {
            // Hidden: sleep until an event or a pending Slint timer (a toast, a theme blend).
            slint::platform::duration_until_next_timer_update().unwrap_or(Duration::MAX)
        };
        let timeout = shell.frame_wait().map_or(timeout, |w| w.min(timeout));
        let timeout = app.access_wait().map_or(timeout, |w| w.min(timeout));
        let timeout = stress.as_ref().map_or(timeout, |r| r.wait().min(timeout));
        event_loop.dispatch(Some(timeout).filter(|t| *t != Duration::MAX), &mut shell)?;
        slint::platform::update_timers_and_animations();
        app.keep_keyboard();
        let pending: Vec<Control> = app.pending.borrow_mut().drain(..).collect();
        for c in pending {
            route(&app, &mut shell, c);
        }
        app.tick(&mut shell);
        app.access_sync(false);
        if let Some(run) = stress.as_deref_mut() {
            if shell.closed_by_compositor {
                return Ok(());
            }
            if shell.is_configured() {
                let drive = run
                    .drive(&app, window.window())
                    .map_err(|f| Error::Stress(f.to_string()))?;
                if drive == stress::Drive::Finished {
                    return Ok(());
                }
            }
        }
        if shell.closed_by_compositor {
            shell.closed_by_compositor = false;
            app.hide(&mut shell);
        }
        if exit_when_closed && !shell.is_visible() {
            let _ = conn.flush();
            return Ok(());
        }
        if shell.is_visible() {
            app.sync_grid();
            let ui = app.ui();
            shell.set_glass(ui.global::<crate::Theme>().get_glass());
            shell.set_cursor(cursor_from_ui(ui.get_cursor()));
            shell.set_animating(app.blending());
            if bench {
                window.window().request_redraw();
            }
            shell.render_if_needed();
        }
        let _ = conn.flush();
    }
}

/// The renderer from the config (`configured`), or `NIRI_DASH_RENDERER`. `auto` prefers the GPU
/// and falls back to software when EGL/OpenGL ES can't be set up.
fn choose_backend(conn: &Connection, configured: RendererChoice) -> crate::shell::Backend {
    use crate::shell::Backend;
    let choice = renderer_override().unwrap_or(configured);
    let software =
        || Backend::Software(MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer));
    if choice == RendererChoice::Software {
        log::info!("rendering in software");
        return software();
    }
    let gpu = crate::gpu::Gl::new(conn)
        .and_then(|gl| Ok(Backend::Gpu(crate::gpu::GpuWindow::new(&gl)?, gl)));
    match gpu {
        Ok(b) => {
            log::info!("rendering with OpenGL ES");
            b
        }
        Err(e) => {
            log::warn!("GPU rendering unavailable ({e}); rendering in software");
            software()
        }
    }
}

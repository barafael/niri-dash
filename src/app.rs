//! The daemon: Wayland event loop, Slint UI, the system's senders and the control socket, glued
//! together. A standalone app (`niri-dash app <widget>`) is the same, with one widget in a normal
//! window.

use crate::layout::{Kind, Layout, Loaded, RendererChoice};
use crate::shell::{Mode, Shell};
use crate::{
    Clipboard as UiClipboard, Control, Dash, Power as UiPower, Shell as UiShell, Tray as UiTray,
    WidgetInst, named_socket_path, socket_path, theme,
};
use config::watch_config;
use niri_dash_system::{self as sys, Capabilities, Event, Feed, IconSrc, Senders, Started, Tell};
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::{
    ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode,
    VecModel,
};
use smithay_client_toolkit::reexports::calloop::{EventLoop, channel};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::{Connection, globals::registry_queue_init};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

mod a11y;
mod audio;
mod clipboard;
mod clock;
mod color_picker;
mod config;
mod devices;
mod edit;
mod enums;
mod feeds;
mod hold;
mod nav;
mod probe;

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
    }
}
mod look;
mod mascots;
mod night_light;
mod notes;
mod notifications;
mod power;
mod privacy;
pub mod snapshot;
pub mod stress;
#[doc(hidden)]
pub mod testing;
mod timer;

/// Starting the dashboard (or a snapshot) failed.
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
    #[error("connecting to the Wayland display: {0}")]
    Wayland(#[from] smithay_client_toolkit::reexports::client::ConnectError),
    #[error("wayland registry: {0}")]
    Registry(#[from] smithay_client_toolkit::reexports::client::globals::GlobalError),
    #[error("event loop: {0}")]
    EventLoop(#[from] smithay_client_toolkit::reexports::calloop::Error),
    /// Slint couldn't set up the platform or the UI.
    #[error("{0}")]
    Ui(String),
    #[error(transparent)]
    Shell(#[from] crate::shell::ShellError),
    #[error(transparent)]
    Gpu(#[from] crate::gpu::GpuError),
    #[error("creating {}: {source}", path.display())]
    Create {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("writing the PNG: {0}")]
    Png(#[from] png::EncodingError),
    #[error("reading {}: {source}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The stress run found the dashboard in a state it shouldn't be in.
    #[error("stress run failed {0}")]
    Stress(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

struct LayerPlatform {
    window: Rc<dyn WindowAdapter>,
}

impl Platform for LayerPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }
}

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

/// Replace the model's rows, touching only those that changed (each change repaints).
fn set_if_changed<T: Clone + 'static>(
    m: &VecModel<T>,
    rows: Vec<T>,
    same: impl Fn(&T, &T) -> bool,
) {
    if m.row_count() != rows.len() {
        m.set_vec(rows);
        return;
    }
    for (i, r) in rows.into_iter().enumerate() {
        if !m.row_data(i).is_some_and(|old| same(&old, &r)) {
            m.set_row_data(i, r);
        }
    }
}

/// How long a slider the user dragged keeps its value against what the source reports, which
/// lags behind.
const TOUCH_HOLD: Duration = Duration::from_millis(1500);

/// Whether something the user touched at `since` is still held.
fn held(since: Instant) -> bool {
    since.elapsed() < TOUCH_HOLD
}

/// How many images `App::image` keeps by path.
const IMAGE_CACHE: usize = 96;
/// Largest side of a raster image kept for display. Album art in a tall media widget is the
/// largest (a few hundred logical px); a full-size cover or screenshot is megabytes of pixels.
const IMAGE_MAX: u32 = 512;

/// `img`, scaled down to fit `IMAGE_MAX` if it's a large raster image (SVGs stay as they are).
fn small_image(img: Image) -> Image {
    let size = img.size();
    if size.width.max(size.height) <= IMAGE_MAX {
        return img;
    }
    let Some(buf) = img.to_rgba8() else {
        return img;
    };
    rgba_image(
        buf.width(),
        buf.height(),
        buf.as_bytes().to_vec(),
        IMAGE_MAX,
    )
}

/// An image of RGBA pixels, scaled down to fit `max` on its longer side.
fn rgba_image(width: u32, height: u32, rgba: Vec<u8>, max: u32) -> Image {
    let (w, h, px) = sys::clipboard::shrink(width, height, rgba, max);
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&px, w, h))
}

struct App {
    ui: slint::Weak<Dash>,
    /// Set in a standalone app: the one widget it shows.
    solo: Option<Kind>,
    /// Where to send the system's commands.
    tx: Senders,
    /// What the system can do here.
    caps: Capabilities,
    /// Whether the dashboard is up, as last told to the system.
    visible: Cell<bool>,
    layout: RefCell<Layout>,
    widgets: Rc<VecModel<WidgetInst>>,
    /// The Columns and Rows steppers of edit mode.
    grid_controls: Rc<VecModel<crate::SettingRow>>,
    /// Icons, album art and notification images by path, with when each was last used: art and
    /// notification images come and go with every track and screenshot, so the least recently
    /// used are dropped past `IMAGE_CACHE`.
    images: RefCell<HashMap<PathBuf, (Option<Image>, u64)>>,
    image_uses: Cell<u64>,
    /// Controls from the UI, run by the event loop, which has the shell at hand.
    pending: RefCell<Vec<Control>>,
    locale: sys::Locale,
    /// When the brightness slider was last dragged.
    brightness_touched: Cell<Option<Instant>>,
    /// The tray items as last reported, and the address of the one whose menu is open.
    tray_items: RefCell<Vec<sys::tray::Item>>,
    menu_address: RefCell<String>,
    toast_timer: Timer,
    /// Drives the blend between two themes.
    look_timer: Rc<Timer>,
    /// Installed font families for the Style tab (looked up on first use).
    fonts: RefCell<Option<Vec<String>>>,
    /// The surface is up but the entrance animation hasn't started yet.
    reveal_pending: Cell<bool>,
    /// When the exit animation ends and the surface goes away.
    hide_at: Cell<Option<Instant>>,
    /// While dragging: the widget being dragged (0 for a new one) and where others would go.
    preview: RefCell<Option<(u32, crate::layout::Reflow)>>,
    /// A press that may turn into a long press.
    hold: hold::Hold,
    /// The hint labels last handed to `Nav.hint-chars`, to set it only when they change.
    hints: RefCell<Vec<String>>,
    /// The window of each Claude session, by pid (niri only).
    claude_windows: RefCell<HashMap<i32, u64>>,
    /// config.kdl as we last wrote it, to ignore our own writes when watching the file.
    last_written: RefCell<String>,
    /// config.kdl doesn't parse, so edits aren't saved over it.
    config_broken: Cell<bool>,
    /// Files the watcher checks for changes (config.kdl and the active theme).
    watched: Arc<Mutex<Vec<PathBuf>>>,
    /// What the daemon keeps across restarts (do not disturb, the timer, picked colors).
    state: RefCell<crate::state::State>,
    clock: clock::ClockState,
    timers: timer::Timers,
    notes: notes::Notes,
    picker: color_picker::Picker,
    privacy: privacy::Privacy,
    night: night_light::NightLight,
    mascots: mascots::Mascots,
    clipboard: clipboard::History,
    mixer: audio::Mixer,
}

impl App {
    fn ui(&self) -> Dash {
        self.ui.upgrade().expect("ui alive")
    }

    fn image(&self, src: &IconSrc) -> Option<Image> {
        match src {
            IconSrc::None => None,
            IconSrc::Path(p) => {
                let now = self.image_uses.get() + 1;
                self.image_uses.set(now);
                let mut images = self.images.borrow_mut();
                let entry = images.entry(p.clone()).or_insert_with(|| {
                    let img = Image::load_from_path(p)
                        .map_err(|e| log::debug!("{}: {e:?}", p.display()))
                        .ok()
                        .map(small_image);
                    (img, now)
                });
                entry.1 = now;
                let img = entry.0.clone();
                if images.len() > IMAGE_CACHE {
                    // Drop the older half in one go rather than one entry per call.
                    let mut ages: Vec<u64> = images.values().map(|(_, t)| *t).collect();
                    ages.sort_unstable();
                    let cut = ages[ages.len() / 2];
                    images.retain(|_, (_, t)| *t >= cut);
                }
                img
            }
            IconSrc::Rgba {
                width,
                height,
                data,
            } => Some(rgba_image(
                *width,
                *height,
                data.as_ref().clone(),
                IMAGE_MAX,
            )),
        }
    }

    fn toast(&self, text: &str) {
        let ui = self.ui();
        ui.set_toast(text.into());
        ui.set_toast_shown(true);
        let ui = self.ui.clone();
        let secs = (3 + text.len() / 40).min(10) as u64;
        self.toast_timer.start(
            TimerMode::SingleShot,
            Duration::from_secs(secs),
            move || {
                if let Some(ui) = ui.upgrade() {
                    ui.set_toast_shown(false);
                }
            },
        );
    }

    /// Change the saved state and write it out.
    fn update_state(&self, f: impl FnOnce(&mut crate::state::State)) {
        let mut s = self.state.borrow_mut();
        f(&mut s);
        crate::state::save(&s);
    }

    /// Called every loop iteration: starts the entrance once the surface is configured, removes
    /// the surface when the exit animation is over, and picks a color once the dashboard is out of
    /// the way.
    fn tick(&self, shell: &mut Shell) {
        // Start the entrance only once the first (still invisible) frame is on screen: that frame
        // is the expensive one (buffers, full repaint), and the animation shouldn't run while it
        // renders.
        if self.reveal_pending.get() && shell.is_configured() && shell.presented > 0 {
            self.reveal_pending.set(false);
            self.ui().set_shown(true);
        }
        if let Some(t) = self.hide_at.get()
            && Instant::now() >= t
        {
            self.hide_at.set(None);
            shell.hide();
            self.set_visible(false);
            self.set_awake(false);
        }
        self.tick_pick(shell);
    }

    /// Tell the system whether the dashboard is up: sources only watch and poll while it is.
    fn set_visible(&self, on: bool) {
        self.visible.set(on);
        self.tx.host.tell(sys::host::Command::Visible(on));
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

    fn is_open(&self, shell: &Shell) -> bool {
        shell.is_visible() && self.hide_at.get().is_none()
    }

    fn show(&self, shell: &mut Shell, edit: bool) {
        if self.hide_at.take().is_some() {
            // Shown again during the exit animation: just turn around.
            self.ui().set_shown(true);
        } else if !shell.is_visible() {
            self.come_up();
            let ui = self.ui();
            ui.set_shown(false);
            // Lay out for the output we are about to appear on before the first frame, so
            // nothing animates from a stale size. (An app window gets its size from niri.)
            if self.solo.is_none() {
                self.prepare_for_output(shell);
            }
            shell.show();
            self.reveal_pending.set(true);
            self.set_visible(true);
            self.set_awake(true);
        } else if self.solo.is_some() {
            // Already open: bring the window to the front.
            self.tx
                .windows
                .tell(sys::windows::Command::FocusPid(std::process::id() as i32));
        }
        if edit && self.solo.is_none() {
            self.set_editing(true);
        }
    }

    /// The UI's part of coming up: the clock ticking, every widget back on its first page, no
    /// widget entered (a standalone app in its one widget).
    fn come_up(&self) {
        self.start_clock();
        self.ui().invoke_reset();
        self.reset_nav();
    }

    /// The UI's part of going away: out of edit mode, menus shut, no widget entered.
    fn go_down(&self) {
        if self.ui().get_editing() {
            self.set_editing(false);
        }
        self.ui().global::<UiTray>().set_menu_open(false);
        self.reset_nav();
        self.hold.cancel();
    }

    fn hide(&self, shell: &mut Shell) {
        self.go_down();
        if !shell.is_visible() {
            // Already gone (e.g. the compositor closed it).
            self.ui().set_shown(false);
            self.set_visible(false);
            self.set_awake(false);
            return;
        }
        if self.hide_at.get().is_some() {
            return;
        }
        let motion = self.ui().global::<crate::Theme>().get_motion();
        self.ui().set_shown(false);
        self.reveal_pending.set(false);
        // The exit animations in app.slint and frame.slint take 70 ms (times the motion); the
        // surface goes once their last frame is drawn.
        self.hide_at.set(Some(
            Instant::now() + Duration::from_millis((80.0 * motion) as u64),
        ));
    }

    /// The dashboard came up or went away: notes, timers and mascots only run while seen.
    fn set_awake(&self, on: bool) {
        self.notes_awake(on);
        self.timers_awake(on);
        self.mascots_awake(on);
    }

    fn control(&self, shell: &mut Shell, c: Control) {
        match c {
            Control::Show => self.show(shell, false),
            Control::Edit => self.show(shell, true),
            Control::Hide => self.hide(shell),
            Control::Toggle => {
                if self.is_open(shell) {
                    self.hide(shell)
                } else {
                    self.show(shell, false)
                }
            }
            Control::Quit => {
                if let Some(kind) = self.solo {
                    let _ = std::fs::remove_file(named_socket_path(kind.name()));
                }
                std::process::exit(0);
            }
            Control::Inhibit => self.tx.power.tell(sys::power::Command::ToggleInhibit),
            Control::Dnd => self.toggle_dnd(),
        }
    }

    /// What the control socket or the config watcher asked for.
    fn handle_input(&self, input: Input) -> Option<Control> {
        match input {
            Input::Control(c) => return Some(c),
            Input::ConfigChanged => self.reload_config(),
            Input::NightLight(cmd) => self.night_light(cmd, true),
            Input::MascotSay(say) => self.mascot_say(say),
        }
        None
    }

    /// What the system reports.
    fn handle_event(&self, event: Event) -> Option<Control> {
        match event {
            Event::Toast(t) => self.toast(&t),
            Event::Perf(sys::perf::Event::Snapshot(s)) => self.show_perf(s),
            Event::Claude(sys::claude::Event::Usage(u)) => self.show_claude_usage(*u),
            Event::Claude(sys::claude::Event::Sessions(s)) => self.show_claude(s),
            Event::Windows(sys::windows::Event::State(s)) => self.show_windows(s),
            Event::Keyboard(sys::keyboard::Event::Layouts(l)) => self.show_keyboard_layouts(l),
            Event::Wifi(sys::wifi::Event::State(s)) => self.show_wifi(*s),
            Event::Bluetooth(sys::bluetooth::Event::State(s)) => self.show_bluetooth(*s),
            Event::Brightness(sys::brightness::Event::Level(l)) => self.show_brightness(l),
            Event::Tray(sys::tray::Event::Items(items)) => self.show_tray(items),
            Event::Tray(sys::tray::Event::Menu(m)) => self.show_tray_menu(m),
            Event::Weather(sys::weather::Event::State(w)) => self.show_weather(w),
            Event::Media(sys::media::Event::State(m)) => self.show_media(m),
            Event::Power(sys::power::Event::State(s)) => self.show_power(s),
            Event::Power(sys::power::Event::Inhibited(on)) => {
                self.ui().global::<UiPower>().set_inhibited(on)
            }
            Event::Power(sys::power::Event::SleepBlockers(list)) => self.show_blockers(list),
            Event::Audio(sys::audio::Event::State(s)) => self.show_audio(s),
            Event::Notifications(sys::notifications::Event::State(s)) => self.show_notifications(s),
            Event::Clipboard(sys::clipboard::Event::Copied(clip)) => self.add_clip(clip),
            Event::Privacy(sys::privacy::Event::Casts(casts)) => self.set_casts(casts),
            Event::Privacy(sys::privacy::Event::Usage(usage)) => self.set_privacy_usage(usage),
            Event::NightLight(sys::night_light::Event::Kwin(s)) => self.show_kwin_night_light(s),
            Event::Drives(sys::drives::Event::State(s)) => self.show_drives(s),
            Event::Phones(sys::phones::Event::State(s)) => self.show_phones(s),
            Event::Color(sys::color::Event::Picked(rgb)) => return self.color_picked(rgb),
            Event::Mascot(sys::mascot::Event::Output { id, stdout }) => {
                self.mascot_output(id, stdout)
            }
        }
        None
    }
}

/// What reaches the dashboard from outside the system: the control socket and the config
/// watcher.
enum Input {
    Control(Control),
    /// config.kdl or the theme file changed on disk.
    ConfigChanged,
    /// `niri-dash night-light …`, or a standalone night-light app's change.
    NightLight(sys::night_light::Cmd),
    /// A line for the mascots, from `niri-dash say`.
    MascotSay(crate::mascots::Say),
}

fn wire(app: &Rc<App>, ui: &Dash) {
    let a = app.clone();
    ui.on_close_requested(move || a.pending.borrow_mut().push(Control::Hide));
    let a = app.clone();
    ui.global::<UiShell>()
        .on_hide(move || a.pending.borrow_mut().push(Control::Hide));
    edit::wire(app, ui);
    nav::wire(app, ui);
    look::wire(app, ui);
    clock::wire(app, ui);
    feeds::wire(app, ui);
    power::wire(app, ui);
    audio::wire(app, ui);
    notifications::wire(app, ui);
    clipboard::wire(app, ui);
    color_picker::wire(app, ui);
    timer::wire(app, ui);
    notes::wire(app, ui);
    privacy::wire(app, ui);
    night_light::wire(app, ui);
    devices::wire(app, ui);
    mascots::wire(app, ui);
}

fn control_server(listener: UnixListener, tx: channel::Sender<Input>) {
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

/// The dashboard on `system`. `visible`: it is up already (a snapshot renders without `show`).
fn build(
    ui: &Dash,
    system: Started,
    input: channel::Sender<Input>,
    solo: Option<Kind>,
    config: Loaded,
    visible: bool,
) -> Rc<App> {
    let Started { senders, caps } = system;
    theme::install_bundled();
    ui.set_solo(solo.is_some());
    let desktop = ui.global::<crate::Desktop>();
    desktop.set_niri(caps.niri());
    desktop.set_plasma(caps.kwin());
    if !caps.has_windows() {
        ui.global::<crate::Windows>().set_available(false);
    }

    let Loaded {
        layout,
        warnings,
        broken,
        text,
    } = config;
    let locale = sys::locale();
    let widgets = Rc::new(VecModel::<WidgetInst>::default());
    ui.set_widgets(ModelRc::from(widgets.clone()));
    let grid_controls = Rc::new(VecModel::<crate::SettingRow>::default());
    ui.set_grid_controls(ModelRc::from(grid_controls.clone()));
    let state = crate::state::load();
    let app = Rc::new(App {
        ui: ui.as_weak(),
        solo,
        tx: senders,
        caps,
        visible: Cell::new(visible),
        layout: RefCell::new(layout),
        widgets,
        grid_controls,
        images: Default::default(),
        image_uses: Cell::new(0),
        pending: Default::default(),
        locale,
        brightness_touched: Cell::new(None),
        tray_items: Default::default(),
        menu_address: Default::default(),
        toast_timer: Timer::default(),
        look_timer: Default::default(),
        fonts: Default::default(),
        reveal_pending: Cell::new(false),
        hide_at: Cell::new(None),
        preview: Default::default(),
        hold: Default::default(),
        hints: Default::default(),
        claude_windows: Default::default(),
        last_written: RefCell::new(text),
        config_broken: Cell::new(broken),
        watched: Default::default(),
        mixer: Default::default(),
        clock: Default::default(),
        timers: timer::Timers::restored(&state),
        notes: Default::default(),
        picker: Default::default(),
        privacy: Default::default(),
        night: Default::default(),
        mascots: Default::default(),
        clipboard: Default::default(),
        state: RefCell::new(state),
    });
    wire(&app, ui);
    app.apply_settings();
    if !warnings.is_empty() {
        for w in &warnings {
            log::warn!("{w}");
        }
        app.toast(&warnings.join("; "));
    }
    watch_config(app.watched.clone(), input);
    if visible {
        app.set_awake(true);
    }
    app.sync_grid();
    app.sync_widgets();
    app.tick_clock(true);
    app
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
/// Also decides which compositor this is, before anything asks. `NIRI_DASH_MOCK=1` (for
/// development) runs on the mock system's made-up data instead.
fn start_system(
    events: channel::Sender<Event>,
    solo: Option<Kind>,
    wayland: Option<Connection>,
) -> Result<Started> {
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
fn set_platform(window: Rc<dyn WindowAdapter>) -> Result<()> {
    slint::platform::set_platform(Box::new(LayerPlatform { window }))
        .map_err(|e| Error::Ui(format!("slint platform: {e}")))
}

/// The dashboard UI, shown on the platform's window.
fn new_ui() -> Result<Dash> {
    let ui = Dash::new().map_err(|e| Error::Ui(format!("creating UI: {e}")))?;
    ui.show().map_err(|e| Error::Ui(e.to_string()))?;
    Ok(ui)
}

/// The renderer `NIRI_DASH_RENDERER` asks for, overriding the config (for development).
fn renderer_override() -> Option<RendererChoice> {
    std::env::var("NIRI_DASH_RENDERER")
        .ok()
        .and_then(|r| r.parse().ok())
}

/// `niri-dash stress` on screen: the whole dashboard in a normal window, on the mock system, with
/// no control socket. Ends when the keys run out or the window is closed.
fn stress_window(mut run: stress::Run) -> Result<stress::Monkey> {
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
    let (events_tx, events) = channel::channel::<Event>();
    let (input_tx, inputs) = channel::channel::<Input>();
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

    shell.watch_pointer(hold::watcher(&app));
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
        let timeout = stress
            .as_ref()
            .map_or(timeout, |r| r.pace.wait().min(timeout));
        event_loop.dispatch(Some(timeout).filter(|t| *t != Duration::MAX), &mut shell)?;
        slint::platform::update_timers_and_animations();
        app.keep_keyboard();
        let pending: Vec<Control> = app.pending.borrow_mut().drain(..).collect();
        for c in pending {
            route(&app, &mut shell, c);
        }
        app.tick(&mut shell);
        if let Some(run) = stress.as_deref_mut() {
            if shell.closed_by_compositor {
                return Ok(());
            }
            if shell.is_configured() && run.pace.due() {
                if run.ops.is_some_and(|n| run.monkey.done() >= n) {
                    return Ok(());
                }
                run.monkey
                    .step(&app, window.window())
                    .map_err(|f| Error::Stress(f.to_string()))?;
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
            shell.set_cursor(enums::cursor_from_ui(ui.get_cursor()));
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

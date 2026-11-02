//! The daemon: Wayland event loop, Slint UI, data sources and the control socket, glued together.
//! A standalone app (`niri-dash app <widget>`) is the same, with one widget in a normal window.

use crate::layout::{Kind, Layout, Loaded, RendererChoice};
use crate::shell::{Mode, Shell};
use crate::sources::{
    self, Ctx, IconSrc, Msg, audio::Audio, bluetooth::Bluetooth, brightness::Brightness,
    inhibit::Inhibit, kdeconnect::KdeConnect, media::Media, niri, notifications::Notifications,
    tray, udisks::Drives, weather::Weather, wifi::Wifi,
};
use crate::{
    Clipboard as UiClipboard, Control, Dash, Power as UiPower, Shell as UiShell, Tray as UiTray,
    WidgetInst, named_socket_path, socket_path, theme,
};
use config::watch_config;
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

mod audio;
mod clipboard;
mod clock;
mod color_picker;
mod config;
mod devices;
mod edit;
mod enums;
mod feeds;
mod look;
mod mascots;
mod night_light;
mod notes;
mod notifications;
mod power;
mod privacy;
pub mod snapshot;
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
    let (w, h, px) = sources::clipboard::shrink(width, height, rgba, max);
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&px, w, h))
}

struct App {
    ui: slint::Weak<Dash>,
    /// Set in a standalone app: the one widget it shows.
    solo: Option<Kind>,
    ctx: Ctx,
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
    locale: sources::Locale,
    /// When the brightness slider was last dragged.
    brightness_touched: Cell<Option<Instant>>,
    /// The tray items as last reported, and the address of the one whose menu is open.
    tray_items: RefCell<Vec<tray::Item>>,
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
    /// The niri window of each Claude session, by pid.
    claude_windows: RefCell<HashMap<i32, u64>>,
    /// config.kdl as we last wrote it, to ignore our own writes when watching the file.
    last_written: RefCell<String>,
    /// config.kdl doesn't parse, so edits aren't saved over it.
    config_broken: Cell<bool>,
    /// Files the watcher checks for changes (config.kdl and the active theme).
    watched: Arc<Mutex<Vec<PathBuf>>>,
    weather: Weather,
    wifi: Wifi,
    bluetooth: Bluetooth,
    audio: Audio,
    brightness: Brightness,
    notifications: Notifications,
    tray: tray::Tray,
    media: Media,
    inhibit: Inhibit,
    drives: Drives,
    phones: KdeConnect,
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
    /// the surface when the exit animation is over, and does what needs the shell (a copied
    /// clipboard entry, a color pick once the dashboard is out of the way).
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
            self.ctx.set_visible(false);
            self.set_awake(false);
        }
        self.push_pending_clip(shell);
        self.tick_pick(shell);
        self.tick_night_light(shell);
    }

    fn prepare_for_output(&self, shell: &mut Shell) {
        if let Ok(niri_ipc::Response::FocusedOutput(Some(out))) =
            niri::request(niri_ipc::Request::FocusedOutput)
            && let Some(l) = out.logical
        {
            shell.prepare(l.width, l.height, l.scale);
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
            self.start_clock();
            let ui = self.ui();
            ui.invoke_reset();
            ui.set_shown(false);
            // Lay out for the output we are about to appear on before the first frame, so
            // nothing animates from a stale size. (An app window gets its size from niri.)
            if self.solo.is_none() {
                self.prepare_for_output(shell);
            }
            shell.show();
            self.reveal_pending.set(true);
            self.ctx.set_visible(true);
            self.set_awake(true);
        } else if self.solo.is_some() {
            // Already open: bring the window to the front.
            niri::focus_pid(&self.ctx, std::process::id() as i32);
        }
        if edit && self.solo.is_none() {
            self.set_editing(true);
        }
    }

    fn hide(&self, shell: &mut Shell) {
        if self.ui().get_editing() {
            self.set_editing(false);
        }
        self.ui().global::<UiTray>().set_menu_open(false);
        if !shell.is_visible() {
            // Already gone (e.g. the compositor closed it).
            self.ui().set_shown(false);
            self.ctx.set_visible(false);
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
            Control::Inhibit => self.inhibit.toggle(),
            Control::Dnd => self.toggle_dnd(),
        }
    }

    fn handle(&self, msg: Msg) -> Option<Control> {
        match msg {
            Msg::Control(c) => return Some(c),
            Msg::Toast(t) => self.toast(&t),
            Msg::ConfigChanged => self.reload_config(),
            Msg::Perf(s) => self.show_perf(s),
            Msg::ClaudeUsage(u) => self.show_claude_usage(u),
            Msg::Claude(s) => self.show_claude(s),
            Msg::Windows(s) => self.show_windows(s),
            Msg::KeyboardLayouts(l) => self.show_keyboard_layouts(l),
            Msg::Wifi(s) => self.show_wifi(s),
            Msg::Bluetooth(s) => self.show_bluetooth(s),
            Msg::Brightness(level) => self.show_brightness(level),
            Msg::Tray(items) => self.show_tray(items),
            Msg::TrayMenu(m) => self.show_tray_menu(m),
            Msg::Weather(w) => self.show_weather(w),
            Msg::Media(m) => self.show_media(m),
            Msg::Power(s) => self.show_power(s),
            Msg::Inhibited(on) => self.ui().global::<UiPower>().set_inhibited(on),
            Msg::SleepBlockers(list) => self.show_blockers(list),
            Msg::Audio(s) => self.show_audio(s),
            Msg::Notifications(s) => self.show_notifications(s),
            Msg::Clipboard(clip) => self.add_clip(clip),
            Msg::Casts(casts) => self.set_casts(casts),
            Msg::Privacy(usage) => self.set_privacy_usage(usage),
            Msg::NightLight(cmd) => self.night_light(cmd, true),
            Msg::Drives(s) => self.show_drives(s),
            Msg::Phones(s) => self.show_phones(s),
            Msg::ColorPicked(rgb) => return self.color_picked(rgb),
            Msg::MascotSay(say) => self.mascot_say(say),
            Msg::MascotAnswer(id, out) => self.mascot_answer(id, out),
        }
        None
    }
}

fn wire(app: &Rc<App>, ui: &Dash) {
    let a = app.clone();
    ui.on_close_requested(move || a.pending.borrow_mut().push(Control::Hide));
    let a = app.clone();
    ui.global::<UiShell>()
        .on_hide(move || a.pending.borrow_mut().push(Control::Hide));
    edit::wire(app, ui);
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

fn control_server(listener: UnixListener, tx: channel::Sender<Msg>) {
    std::thread::Builder::new()
        .name("control".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut line = String::new();
                let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                if BufReader::new(&stream).read_line(&mut line).is_err() {
                    continue;
                }
                if let Some(cmd) = crate::gamma::Cmd::parse_wire(line.trim()) {
                    let _ = tx.send(Msg::NightLight(cmd));
                    continue;
                }
                if let Some(say) = crate::mascots::Say::parse_wire(line.trim()) {
                    match say {
                        Ok(say) => {
                            let _ = tx.send(Msg::MascotSay(say));
                        }
                        Err(e) => log::warn!("bad say command: {e}"),
                    }
                    continue;
                }
                let Ok(c) = line.trim().parse::<Control>() else {
                    log::warn!("unknown control command {:?}", line.trim());
                    continue;
                };
                let _ = tx.send(Msg::Control(c));
            }
        })
        .expect("spawning control thread");
}

/// A data source a widget reads from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Feed {
    Perf,
    Niri,
    Power,
    Claude,
    ClaudeUsage,
    Weather,
    Wifi,
    Bluetooth,
    Audio,
    Brightness,
    Notifications,
    Tray,
    Media,
    Inhibit,
    Privacy,
    Drives,
    Phone,
}

/// The sources a standalone app of `kind` has to run. Clock and calendar tick on their own, the
/// clipboard comes from the Wayland side.
fn feeds(kind: Kind) -> &'static [Feed] {
    match kind {
        Kind::Clock | Kind::Calendar | Kind::Clipboard | Kind::Session => &[],
        Kind::SystemMonitor | Kind::Memory | Kind::Disks | Kind::Network | Kind::Gpu => {
            &[Feed::Perf]
        }
        Kind::Windows => &[Feed::Niri],
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
        Kind::Privacy => &[Feed::Niri, Feed::Privacy],
        Kind::Notes => &[],
        Kind::Timer => &[Feed::Notifications],
        Kind::ColorPicker => &[],
        Kind::KeyboardLayout => &[Feed::Niri],
        // A standalone night-light app asks the daemon, which holds the gamma control.
        Kind::NightLight => &[],
        Kind::RemovableDrives => &[Feed::Drives],
        Kind::Phone => &[Feed::Phone],
    }
}

/// Whether a standalone app keeps running when its window closes, so the next `niri-dash app`
/// shows it again with its state. The clipboard does: its history is what it is for.
fn resident(kind: Kind) -> bool {
    kind == Kind::Clipboard
}

fn build(ui: &Dash, ctx: Ctx, solo: Option<Kind>, config: Loaded) -> Rc<App> {
    let wants = |f: Feed| solo.is_none_or(|k| feeds(k).contains(&f));
    // Sources a standalone app doesn't show get a dormant context: they exist but stay idle.
    let feed_ctx = |f: Feed| ctx.live_if(wants(f));
    sources::perf::start(feed_ctx(Feed::Perf));
    niri::start(feed_ctx(Feed::Niri));
    sources::power::start(feed_ctx(Feed::Power));
    sources::claude::start(feed_ctx(Feed::Claude));
    sources::claude_usage::start(feed_ctx(Feed::ClaudeUsage));
    sources::privacy::start(feed_ctx(Feed::Privacy));
    theme::install_bundled();
    ui.set_solo(solo.is_some());

    let Loaded {
        layout,
        warnings,
        broken,
        text,
    } = config;
    let locale = sources::locale();
    let widgets = Rc::new(VecModel::<WidgetInst>::default());
    ui.set_widgets(ModelRc::from(widgets.clone()));
    let grid_controls = Rc::new(VecModel::<crate::SettingRow>::default());
    ui.set_grid_controls(ModelRc::from(grid_controls.clone()));
    let state = crate::state::load();
    let app = Rc::new(App {
        ui: ui.as_weak(),
        solo,
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
        claude_windows: Default::default(),
        last_written: RefCell::new(text),
        config_broken: Cell::new(broken),
        watched: Default::default(),
        weather: Weather::start(feed_ctx(Feed::Weather), locale.chrono),
        wifi: Wifi::start(feed_ctx(Feed::Wifi)),
        bluetooth: Bluetooth::start(feed_ctx(Feed::Bluetooth)),
        audio: Audio::start(feed_ctx(Feed::Audio)),
        brightness: Brightness::start(feed_ctx(Feed::Brightness)),
        notifications: Notifications::start(feed_ctx(Feed::Notifications)),
        tray: tray::Tray::start(feed_ctx(Feed::Tray)),
        media: Media::start(feed_ctx(Feed::Media)),
        inhibit: Inhibit::start(feed_ctx(Feed::Inhibit)),
        drives: Drives::start(feed_ctx(Feed::Drives)),
        phones: KdeConnect::start(feed_ctx(Feed::Phone)),
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
        ctx: ctx.clone(),
    });
    wire(&app, ui);
    app.apply_settings();
    if !warnings.is_empty() {
        for w in &warnings {
            log::warn!("{w}");
        }
        app.toast(&warnings.join("; "));
    }
    watch_config(app.watched.clone(), ctx);
    if app.ctx.visible() {
        // Snapshots render without `show`.
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
    serve(listener, Mode::Dashboard, None, initial, Layout::load())
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
    let result = serve(listener, mode, Some(kind), Some(Control::Show), config);
    let _ = std::fs::remove_file(&path);
    result
}

/// The runtime the sources run on.
fn runtime() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("niri-dash-io")
        .enable_all()
        .build()?)
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

/// The event loop shared by the dashboard and the standalone apps. Returns when a (non-resident)
/// app's window closes; the dashboard runs until it is told to quit.
fn serve(
    listener: UnixListener,
    mode: Mode,
    solo: Option<Kind>,
    initial: Option<Control>,
    config: Loaded,
) -> Result<()> {
    let rt = runtime()?;
    let conn = Connection::connect_to_env()?;
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

    let (tx, rx) = channel::channel::<Msg>();
    let ctx = Ctx::new(tx.clone(), rt.handle().clone());
    shell.clip_tx = Some(tx.clone());
    let app = build(&ui, ctx, solo, config);
    ui.global::<UiClipboard>()
        .set_available(shell.has_clipboard());

    let a = app.clone();
    event_loop
        .handle()
        .insert_source(rx, move |ev, _, shell| {
            if let channel::Event::Msg(m) = ev
                && let Some(c) = a.handle(m)
            {
                a.control(shell, c);
            }
        })
        .map_err(|e| Error::Ui(format!("message source: {e}")))?;
    control_server(listener, tx);

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
        event_loop.dispatch(Some(timeout).filter(|t| *t != Duration::MAX), &mut shell)?;
        slint::platform::update_timers_and_animations();
        let pending: Vec<Control> = app.pending.borrow_mut().drain(..).collect();
        for c in pending {
            app.control(&mut shell, c);
        }
        app.tick(&mut shell);
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

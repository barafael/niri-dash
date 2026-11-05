//! The dashboard: the Slint UI and the system's senders and events, glued together, wherever it
//! appears ([`Surface`]): on Wayland ([`wayland`]) or in a browser (`crate::web`). A standalone app
//! (`niri-dash app <widget>`) is the same, with one widget in a normal window.

use crate::layout::{Kind, Layout, Loaded};
use crate::{Control, Dash, Power as UiPower, Shell as UiShell, Tray as UiTray, WidgetInst, theme};
use niri_dash_system::{self as sys, Capabilities, Event, IconSrc, Senders, Started, Tell};
use slint::{
    ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode,
    VecModel,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use web_time::Instant;

mod a11y;
mod arrange;
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
#[cfg(not(target_arch = "wasm32"))]
mod probe;

mod look;
mod mascots;
mod night_light;
mod notes;
mod notifications;
mod pomodoro;
mod power;
mod privacy;
mod sleeper;
#[cfg(not(target_arch = "wasm32"))]
pub mod snapshot;
#[cfg(not(target_arch = "wasm32"))]
pub mod stress;
#[cfg(not(target_arch = "wasm32"))]
#[doc(hidden)]
pub mod testing;
mod timer;
#[cfg(not(target_arch = "wasm32"))]
mod wayland;
#[cfg(target_arch = "wasm32")]
pub mod web;

#[cfg(not(target_arch = "wasm32"))]
use wayland::{Input, renderer_override, set_platform, start_system, stress_window};
#[cfg(not(target_arch = "wasm32"))]
pub use wayland::{run, run_app};

/// A channel into a calloop event loop: the sink to send to, and the source to insert in the loop.
/// The loop wakes when something is sent.
#[cfg(not(target_arch = "wasm32"))]
fn loop_channel<T: Send + 'static>() -> (
    sys::Sink<T>,
    smithay_client_toolkit::reexports::calloop::channel::Channel<T>,
) {
    let (tx, rx) = smithay_client_toolkit::reexports::calloop::channel::channel();
    (
        sys::Sink::new(move |value| tx.send(value).map_err(|_| sys::Closed)),
        rx,
    )
}

/// Where the dashboard appears: a layer surface or a window on Wayland, a canvas in a browser. It
/// comes and goes; the UI stays alive in between.
trait Surface {
    /// Whether it is there, also if it isn't ready to be drawn on yet.
    fn is_visible(&self) -> bool;
    /// Whether it is ready to be drawn on.
    fn is_configured(&self) -> bool;
    /// How many frames it has shown since it came up.
    fn presented(&self) -> u32;
    fn show(&mut self);
    fn hide(&mut self);
    /// About to `show`: get the sizes right, if that takes doing first.
    fn prepare(&mut self, _: &App) {}
}

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
    #[cfg(not(target_arch = "wasm32"))]
    #[error("connecting to the Wayland display: {0}")]
    Wayland(#[from] smithay_client_toolkit::reexports::client::ConnectError),
    #[cfg(not(target_arch = "wasm32"))]
    #[error("wayland registry: {0}")]
    Registry(#[from] smithay_client_toolkit::reexports::client::globals::GlobalError),
    #[cfg(not(target_arch = "wasm32"))]
    #[error("event loop: {0}")]
    EventLoop(#[from] smithay_client_toolkit::reexports::calloop::Error),
    /// Slint couldn't set up the platform or the UI.
    #[error("{0}")]
    Ui(String),
    #[cfg(not(target_arch = "wasm32"))]
    #[error(transparent)]
    Shell(#[from] crate::shell::ShellError),
    #[cfg(not(target_arch = "wasm32"))]
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
    pomo: pomodoro::Pomo,
    notes: notes::Notes,
    picker: color_picker::Picker,
    privacy: privacy::Privacy,
    night: night_light::NightLight,
    mascots: mascots::Mascots,
    sleeper: sleeper::Sleeper,
    clipboard: clipboard::History,
    arrange: arrange::Arrange,
    mixer: audio::Mixer,
    /// The media player's cover as last shown, and its image.
    media_art: RefCell<Option<(IconSrc, Option<Image>)>>,
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
    fn tick(&self, shell: &mut impl Surface) {
        // Start the entrance only once the first (still invisible) frame is on screen: that frame
        // is the expensive one (buffers, full repaint), and the animation shouldn't run while it
        // renders.
        if self.reveal_pending.get() && shell.is_configured() && shell.presented() > 0 {
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

    fn is_open(&self, shell: &impl Surface) -> bool {
        shell.is_visible() && self.hide_at.get().is_none()
    }

    fn show(&self, shell: &mut impl Surface, edit: bool) {
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
                shell.prepare(self);
            }
            shell.show();
            self.reveal_pending.set(true);
            self.set_visible(true);
            self.set_awake(true);
        } else if self.solo.is_some() {
            // Already open: bring the window to the front.
            self.tx
                .windows
                .tell(sys::windows::Command::FocusPid(crate::own_pid() as i32));
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

    fn hide(&self, shell: &mut impl Surface) {
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
        self.pomodoro_awake(on);
        self.mascots_awake(on);
    }

    fn control(&self, shell: &mut impl Surface, c: Control) {
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
            // A page isn't left by asking it to.
            #[cfg(target_arch = "wasm32")]
            Control::Quit => {}
            #[cfg(not(target_arch = "wasm32"))]
            Control::Quit => {
                if let Some(kind) = self.solo {
                    let _ = std::fs::remove_file(crate::named_socket_path(kind.name()));
                }
                std::process::exit(0);
            }
            Control::Inhibit => self.tx.power.tell(sys::power::Command::ToggleInhibit),
            Control::Dnd => self.toggle_dnd(),
        }
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
            Event::Sleep(e) => self.sleep_event(e),
        }
        None
    }
}

fn wire(app: &Rc<App>, ui: &Dash) {
    let a = app.clone();
    ui.on_close_requested(move || a.pending.borrow_mut().push(Control::Hide));
    let a = app.clone();
    // Clicking a window or a notification takes you to it and puts the dashboard away. In the demo
    // there is nothing to go to (it only says so), and putting the dashboard away would look like
    // the page reloading.
    ui.global::<UiShell>().on_hide(move || {
        if !a.caps.simulated {
            a.pending.borrow_mut().push(Control::Hide);
        }
    });
    edit::wire(app, ui);
    arrange::wire(app, ui);
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
    pomodoro::wire(app, ui);
    notes::wire(app, ui);
    privacy::wire(app, ui);
    night_light::wire(app, ui);
    devices::wire(app, ui);
    mascots::wire(app, ui);
    sleeper::wire(app, ui);
}

/// The dashboard on `system`. `visible`: it is up already (a snapshot renders without `show`).
fn build(
    ui: &Dash,
    system: Started,
    #[cfg(not(target_arch = "wasm32"))] input: sys::Sink<Input>,
    solo: Option<Kind>,
    config: Loaded,
    visible: bool,
) -> Rc<App> {
    let Started { senders, caps } = system;
    theme::install_bundled();
    ui.set_solo(solo.is_some());
    let desktop = ui.global::<crate::Desktop>();
    desktop.set_screencast(caps.screencast);
    desktop.set_niri(caps.compositor == sys::Compositor::Niri);
    desktop.set_plasma(caps.compositor == sys::Compositor::Kwin);
    desktop.set_unavailable(caps.unavailable().into());
    if !caps.windows {
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
    let arrange = arrange::Arrange::new(layout.autolayout.clone());
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
        media_art: Default::default(),
        clock: Default::default(),
        timers: timer::Timers::restored(&state),
        pomo: pomodoro::Pomo::restored(&state),
        notes: Default::default(),
        picker: Default::default(),
        privacy: Default::default(),
        night: Default::default(),
        mascots: Default::default(),
        sleeper: Default::default(),
        clipboard: Default::default(),
        arrange,
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
    #[cfg(not(target_arch = "wasm32"))]
    config::watch_config(app.watched.clone(), input);
    if visible {
        app.set_awake(true);
    }
    app.sync_grid();
    app.sync_widgets();
    app.tick_clock(true);
    app
}

/// The dashboard UI, shown on the platform's window.
fn new_ui() -> Result<Dash> {
    let ui = Dash::new().map_err(|e| Error::Ui(format!("creating UI: {e}")))?;
    ui.show().map_err(|e| Error::Ui(e.to_string()))?;
    Ok(ui)
}

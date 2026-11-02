//! The daemon: Wayland event loop, Slint UI, data sources and the control socket, glued together.
//! A standalone app (`niri-dash app <widget>`) is the same, with one widget in a normal window.

use crate::layout::{Kind, Layout, Placed, Shape as LShape};
use crate::shell::{Mode, Shell};
use crate::sources::{
    self, Ctx, IconSrc, Msg,
    audio::Audio,
    bluetooth::Bluetooth,
    brightness::Brightness,
    clipboard::{self, Clip},
    inhibit::Inhibit,
    media::Media,
    niri,
    notifications::Notifications,
    power, session, tray,
    weather::Weather,
    wifi::Wifi,
};
use crate::theme;
use crate::{Control, named_socket_path, socket_path};
use chrono::{Datelike, Local, Timelike};
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::{
    ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer,
    TimerMode, VecModel,
};
use smithay_client_toolkit::reexports::calloop::{EventLoop, channel};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::{Connection, globals::registry_queue_init};
use smithay_client_toolkit::seat::pointer::CursorIcon;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use strum::IntoEnumIterator;

use crate::{
    Audio as UiAudio, Bluetooth as UiBluetooth, BtDevice, CalDay, Calendar, Claude as UiClaude,
    ClaudeSession, ClipItem, Clipboard as UiClipboard, Clock, Dash, Media as UiMedia, MenuEntry,
    Notif, Notifications as UiNotifications, PaletteItem, Perf, Power as UiPower,
    Session as UiSession, Shape, Shell as UiShell, Tray as UiTray, TrayItem, Weather as UiWeather,
    WeatherDay, WidgetInst, WidgetKind, Wifi as UiWifi, WifiNet, WinItem, Windows as UiWindows,
};

mod extra;
use extra::Extra;

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

fn kind_to_ui(k: Kind) -> WidgetKind {
    match k {
        Kind::Clock => WidgetKind::Clock,
        Kind::Calendar => WidgetKind::Calendar,
        Kind::Perf => WidgetKind::Perf,
        Kind::Windows => WidgetKind::Windows,
        Kind::Wifi => WidgetKind::Wifi,
        Kind::Bluetooth => WidgetKind::Bluetooth,
        Kind::Battery => WidgetKind::Battery,
        Kind::Audio => WidgetKind::Audio,
        Kind::Brightness => WidgetKind::Brightness,
        Kind::Notifications => WidgetKind::Notifications,
        Kind::Tray => WidgetKind::Tray,
        Kind::Claude => WidgetKind::Claude,
        Kind::Clipboard => WidgetKind::Clipboard,
        Kind::Weather => WidgetKind::Weather,
        Kind::Memory => WidgetKind::Memory,
        Kind::Disks => WidgetKind::Disks,
        Kind::Network => WidgetKind::Network,
        Kind::Gpu => WidgetKind::Gpu,
        Kind::ClaudeUsage => WidgetKind::ClaudeUsage,
        Kind::Media => WidgetKind::Media,
        Kind::Session => WidgetKind::Session,
        Kind::Mascot => WidgetKind::Mascot,
        Kind::Privacy => WidgetKind::Privacy,
        Kind::Notes => WidgetKind::Notes,
        Kind::Timer => WidgetKind::Timer,
        Kind::ColorPicker => WidgetKind::ColorPicker,
        Kind::KeyboardLayout => WidgetKind::KeyboardLayout,
    }
}

/// The Style tab's keys: the theme's style variables, mirrored as a Slint enum.
const STYLE_KEYS: [(theme::StyleVar, crate::StyleKey); 14] = [
    (theme::StyleVar::Radius, crate::StyleKey::Radius),
    (theme::StyleVar::RadiusSmall, crate::StyleKey::RadiusSmall),
    (theme::StyleVar::Gap, crate::StyleKey::Gap),
    (theme::StyleVar::Padding, crate::StyleKey::Padding),
    (theme::StyleVar::BorderWidth, crate::StyleKey::BorderWidth),
    (theme::StyleVar::FontFamily, crate::StyleKey::FontFamily),
    (theme::StyleVar::FontSmall, crate::StyleKey::FontSmall),
    (theme::StyleVar::FontBody, crate::StyleKey::FontBody),
    (theme::StyleVar::FontTitle, crate::StyleKey::FontTitle),
    (
        theme::StyleVar::FontWeightDisplay,
        crate::StyleKey::FontWeightDisplay,
    ),
    (theme::StyleVar::Motion, crate::StyleKey::Motion),
    (theme::StyleVar::Glass, crate::StyleKey::Glass),
    (
        theme::StyleVar::BackdropOpacity,
        crate::StyleKey::BackdropOpacity,
    ),
    (theme::StyleVar::CardOpacity, crate::StyleKey::CardOpacity),
];

fn style_key(v: theme::StyleVar) -> crate::StyleKey {
    STYLE_KEYS
        .iter()
        .find(|(r, _)| *r == v)
        .map(|(_, k)| *k)
        .expect("every style variable has a key")
}

fn style_var(k: crate::StyleKey) -> theme::StyleVar {
    STYLE_KEYS
        .iter()
        .find(|(_, u)| *u == k)
        .map(|(r, _)| *r)
        .expect("every key is a style variable")
}

fn profile_to_ui(p: power::Profile) -> crate::PowerProfile {
    match p {
        power::Profile::PowerSaver => crate::PowerProfile::PowerSaver,
        power::Profile::Balanced => crate::PowerProfile::Balanced,
        power::Profile::Performance => crate::PowerProfile::Performance,
    }
}

fn profile_from_ui(p: crate::PowerProfile) -> power::Profile {
    match p {
        crate::PowerProfile::PowerSaver => power::Profile::PowerSaver,
        crate::PowerProfile::Balanced => power::Profile::Balanced,
        crate::PowerProfile::Performance => power::Profile::Performance,
    }
}

fn weather_icon(i: sources::weather::Icon) -> crate::WeatherIcon {
    use sources::weather::Icon;
    match i {
        Icon::Sun => crate::WeatherIcon::Sun,
        Icon::Moon => crate::WeatherIcon::Moon,
        Icon::CloudSun => crate::WeatherIcon::CloudSun,
        Icon::CloudMoon => crate::WeatherIcon::CloudMoon,
        Icon::Cloud => crate::WeatherIcon::Cloud,
        Icon::Fog => crate::WeatherIcon::Fog,
        Icon::Rain => crate::WeatherIcon::Rain,
        Icon::Snow => crate::WeatherIcon::Snow,
        Icon::Storm => crate::WeatherIcon::Storm,
    }
}

fn kind_from_ui(k: WidgetKind) -> Kind {
    Kind::iter()
        .find(|&x| kind_to_ui(x) == k)
        .unwrap_or(Kind::Clock)
}

/// The per-widget settings as the UI reads them, defaults filled in.
fn to_opts(p: &Placed, us: bool) -> crate::WidgetOpts {
    use crate::widget_settings::{Opt, get_bool, get_int, get_str};
    let v = &p.settings;
    let b = |o| get_bool(v, o, us);
    crate::WidgetOpts {
        hours12: get_int(v, Opt::Hours, us) == 12,
        seconds: b(Opt::Seconds),
        date: b(Opt::Date),
        sunday_first: get_str(v, Opt::FirstDay, us) == "sunday",
        week_numbers: b(Opt::WeekNumbers),
        history: get_int(v, Opt::History, us) as i32,
        cores: b(Opt::Cores),
        top_apps: get_int(v, Opt::TopApps, us) as i32,
        hide_system: b(Opt::HideSystem),
        lock: b(Opt::Lock),
        suspend: b(Opt::Suspend),
        logout: b(Opt::Logout),
        restart: b(Opt::Restart),
        poweroff: b(Opt::PowerOff),
        confirm: b(Opt::Confirm),
        stopwatch: get_str(v, Opt::Mode, us) == "stopwatch",
        character: character_to_ui(character(p, us)),
    }
}

/// A mascot widget's character.
fn character(p: &Placed, us: bool) -> crate::mascots::Character {
    use crate::widget_settings::{Opt, get_str};
    get_str(&p.settings, Opt::Character, us)
        .parse()
        .expect("the setting only takes character names")
}

fn character_to_ui(c: crate::mascots::Character) -> crate::Character {
    use crate::mascots::Character as C;
    match c {
        C::Schraubi => crate::Character::Schraubi,
        C::Zolli => crate::Character::Zolli,
        C::Verschnitti => crate::Character::Verschnitti,
    }
}

fn to_inst(p: &Placed, us: bool) -> WidgetInst {
    WidgetInst {
        has_settings: !crate::widget_settings::for_kind(p.kind).is_empty(),
        opts: to_opts(p, us),
        can_round: p.kind.can_be_round(),
        id: p.id as i32,
        kind: kind_to_ui(p.kind),
        shape: if p.shape == LShape::Round {
            Shape::Round
        } else {
            Shape::Rect
        },
        col: p.col,
        row: p.row,
        w: p.w,
        h: p.h,
    }
}

fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}

/// A clipboard history entry, with what the UI shows for it.
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
    let (w, h, px) = clipboard::shrink(
        buf.width(),
        buf.height(),
        buf.as_bytes().to_vec(),
        IMAGE_MAX,
    );
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&px, w, h))
}

/// Largest side of a clipboard thumbnail in pixels; the list shows them at most ~100 px (logical)
/// wide, so this covers scale 2.
const CLIP_THUMB: u32 = 240;

struct ClipEntry {
    clip: Clip,
    text: String,
    /// The image's full size, and a thumbnail: a screenshot decoded at full size is 16 MB, and
    /// the list only shows it about 100 px wide. The PNG itself stays in `clip` for pasting.
    image: Option<(u32, u32, Image)>,
    time: chrono::DateTime<Local>,
}

struct App {
    ui: slint::Weak<Dash>,
    /// Set in a standalone app: the one widget it shows.
    solo: Option<Kind>,
    ctx: Ctx,
    layout: RefCell<Layout>,
    widgets: Rc<VecModel<WidgetInst>>,
    /// Icons, album art and notification images by path, with when each was last used: art and
    /// notification images come and go with every track and screenshot, so the least recently
    /// used are dropped past `IMAGE_CACHE`.
    images: RefCell<HashMap<PathBuf, (Option<Image>, u64)>>,
    image_uses: Cell<u64>,
    pending: RefCell<Vec<Control>>,
    month_offset: Cell<i32>,
    last_day: Cell<u32>,
    locale: (chrono::Locale, bool),
    audio_touched: Cell<Option<Instant>>,
    brightness_touched: Cell<Option<Instant>>,
    tray_items: RefCell<Vec<tray::Item>>,
    menu_address: RefCell<String>,
    toast_timer: Timer,
    clock_timer: Timer,
    /// Drives the blend between two themes.
    look_timer: Rc<Timer>,
    /// Installed font families for the Style tab (looked up on first use).
    fonts: RefCell<Option<Vec<String>>>,
    /// The surface is up but the entrance animation hasn't started yet.
    reveal_pending: Cell<bool>,
    /// When the exit animation ends and the surface goes away.
    hide_at: Cell<Option<Instant>>,
    clips: RefCell<Vec<ClipEntry>>,
    /// While dragging: the widget being dragged (0 for a new one) and where others would go.
    preview: RefCell<Option<(u32, crate::layout::Reflow)>>,
    /// A clipboard entry to put back on the clipboard (needs the Wayland side).
    pending_clip: RefCell<Option<Clip>>,
    claude_windows: RefCell<HashMap<i32, u64>>,
    /// config.kdl as we last wrote it, to ignore our own writes when watching the file.
    last_written: RefCell<String>,
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
    /// The mascot widgets and what each says, mirrored into `Mascots.lines` by widget id.
    mascots: RefCell<crate::mascots::Board>,
    mascot_lines: Rc<VecModel<crate::MascotLine>>,
    mascot_timer: Timer,
    /// State for the newer widgets (see `extra.rs`).
    extra: Extra,
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
            } => {
                let (w, h, px) =
                    clipboard::shrink(*width, *height, data.as_ref().clone(), IMAGE_MAX);
                Some(Image::from_rgba8(
                    SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&px, w, h),
                ))
            }
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

    // ---- layout ----

    fn sync_widgets(&self) {
        self.sync_mascots();
        self.sync_notes(false);
        if let Some(kind) = self.solo {
            let inst = to_inst(&self.layout.borrow().solo_widget(kind), self.locale.1);
            self.ui().set_solo_inst(inst);
            return;
        }
        self.ui()
            .set_hidden_widgets(self.layout.borrow().hidden_count() as i32);
        let preview = self.preview.borrow();
        let us = self.locale.1;
        let new: Vec<WidgetInst> = self
            .layout
            .borrow()
            .visible()
            .into_iter()
            .map(|p| {
                let mut w = to_inst(p, us);
                if let Some((dragged, moves)) = preview.as_ref()
                    && p.id != *dragged
                    && let Some(&(_, r)) = moves.iter().find(|(id, _)| *id == p.id)
                {
                    (w.col, w.row, w.w, w.h) = (r.col, r.row, r.w, r.h);
                }
                w
            })
            .collect();
        drop(preview);
        let same_ids = self.widgets.row_count() == new.len()
            && self.widgets.iter().zip(&new).all(|(a, b)| a.id == b.id);
        if same_ids {
            for (i, w) in new.into_iter().enumerate() {
                if self.widgets.row_data(i).as_ref() != Some(&w) {
                    self.widgets.set_row_data(i, w);
                }
            }
        } else {
            self.widgets.set_vec(new);
        }
    }

    fn layout_changed(&self) {
        self.sync_widgets();
        self.save_layout();
    }

    fn set_preview(&self, p: Option<(u32, crate::layout::Reflow)>) {
        let changed = *self.preview.borrow() != p;
        *self.preview.borrow_mut() = p;
        if changed {
            self.sync_widgets();
        }
    }

    /// The main direction of a move, used to prefer pushing others that way.
    fn direction(&self, id: u32, col: i32, row: i32) -> (i32, i32) {
        let Some(p) = self.layout.borrow().get(id).cloned() else {
            return (0, 1);
        };
        let (dc, dr) = (col - p.col, row - p.row);
        if dc.abs() > dr.abs() {
            (dc.signum(), 0)
        } else if dr != 0 {
            (0, dr.signum())
        } else {
            (0, 1)
        }
    }

    fn preview_move(&self, id: u32, col: i32, row: i32) -> bool {
        let prefer = self.direction(id, col, row);
        let result = {
            let l = self.layout.borrow();
            l.get(id).and_then(|p| {
                l.reflow(
                    id,
                    crate::layout::Rect {
                        col,
                        row,
                        w: p.w,
                        h: p.h,
                    },
                    prefer,
                )
            })
        };
        let ok = result.is_some();
        self.set_preview(result.map(|m| (id, m)));
        ok
    }

    fn preview_resize(&self, id: u32, w: i32, h: i32) -> bool {
        let result = {
            let l = self.layout.borrow();
            l.get(id).and_then(|p| {
                let (w, h) = l.clamp_size(p, w, h);
                let prefer = if h > p.h { (0, 1) } else { (1, 0) };
                l.reflow(
                    id,
                    crate::layout::Rect {
                        col: p.col,
                        row: p.row,
                        w,
                        h,
                    },
                    prefer,
                )
            })
        };
        let ok = result.is_some();
        self.set_preview(result.map(|m| (id, m)));
        ok
    }

    fn preview_add(&self, kind: Kind, col: i32, row: i32) -> bool {
        let (w, h) = kind.default_size();
        let result = self
            .layout
            .borrow()
            .reflow(0, crate::layout::Rect { col, row, w, h }, (0, 1));
        let ok = result.is_some();
        self.set_preview(result.map(|m| (0, m)));
        ok
    }

    /// Fill the Look tab: every color scheme with a preview of its colors, every style with a
    /// preview of its corners.
    fn refresh_looks(&self) {
        let (cur_c, cur_s) = {
            let l = self.layout.borrow();
            (l.colorscheme.clone(), l.style.clone())
        };
        let mut schemes = theme::list(&theme::colorschemes_dir());
        if !schemes.contains(&cur_c) {
            schemes.push(cur_c.clone());
        }
        let schemes: Vec<crate::SchemeItem> = schemes
            .into_iter()
            .map(|name| {
                let text = theme::colorscheme_text(&name).unwrap_or_default();
                let (look, _) = theme::resolve(&text, theme::DEFAULT_STYLE);
                crate::SchemeItem {
                    current: name == cur_c,
                    description: theme::description(&text).into(),
                    backdrop: look.color(theme::ColorRole::Backdrop),
                    card: look.color(theme::ColorRole::Card),
                    accent: look.color(theme::ColorRole::Accent),
                    text: look.color(theme::ColorRole::Text),
                    good: look.color(theme::ColorRole::Good),
                    bad: look.color(theme::ColorRole::Bad),
                    name: name.into(),
                }
            })
            .collect();
        let mut styles = theme::list(&theme::styles_dir());
        if !styles.contains(&cur_s) {
            styles.push(cur_s.clone());
        }
        let styles: Vec<crate::StyleItem> = styles
            .into_iter()
            .map(|name| {
                let text = theme::style_text(&name).unwrap_or_default();
                let (look, _) = theme::resolve(theme::DEFAULT_COLORSCHEME, &text);
                crate::StyleItem {
                    current: name == cur_s,
                    description: theme::description(&text).into(),
                    radius: look.style.radius,
                    gap: look.style.gap,
                    motion: look.style.motion,
                    name: name.into(),
                }
            })
            .collect();
        let ui = self.ui();
        ui.set_colorschemes(model(schemes));
        ui.set_styles(model(styles));
        self.refresh_style_vars();
    }

    fn fonts(&self) -> Vec<String> {
        self.fonts
            .borrow_mut()
            .get_or_insert_with(theme::font_choices)
            .clone()
    }

    /// The Style tab's rows: every variable with its current value (preset + adjustments).
    fn refresh_style_vars(&self) {
        let (look, _) = self.current_look();
        let over = self.layout.borrow().style_overrides.clone();
        let overridden = |var: theme::StyleVar| over.iter().any(|(k, _)| k == var.as_ref());
        let num = |var: theme::StyleVar| look.style.get(var).as_f32().unwrap_or(0.0);
        let mut rows: Vec<crate::StyleRow> = vec![crate::StyleRow {
            key: style_key(theme::StyleVar::Glass),
            kind: crate::RowKind::Toggle,
            label: "Frosted glass".into(),
            value: if look.style.glass { "on" } else { "off" }.into(),
            on: look.style.glass,
            overridden: overridden(theme::StyleVar::Glass),
            can_dec: false,
            can_inc: false,
        }];
        rows.extend(theme::KNOBS.iter().map(|k| {
            let v = num(k.var);
            crate::StyleRow {
                key: style_key(k.var),
                kind: crate::RowKind::Stepper,
                label: k.label.into(),
                value: theme::knob_text(k.var, v).into(),
                on: false,
                overridden: overridden(k.var),
                can_dec: theme::step(k, v, -1) != v,
                can_inc: theme::step(k, v, 1) != v,
            }
        }));
        let font = look.style.font_family.to_string();
        let fonts = self.fonts();
        let fi = fonts.iter().position(|f| *f == font);
        rows.push(crate::StyleRow {
            key: style_key(theme::StyleVar::FontFamily),
            kind: crate::RowKind::Stepper,
            label: "Font".into(),
            value: if font.is_empty() {
                "System".into()
            } else {
                font.as_str().into()
            },
            on: false,
            overridden: overridden(theme::StyleVar::FontFamily),
            can_dec: fi.is_none_or(|i| i > 0),
            can_inc: fi.is_none_or(|i| i + 1 < fonts.len()),
        });
        self.ui().set_style_rows(model(rows));
        self.ui().set_style_customized(!over.is_empty());
    }

    fn style_step(&self, var: theme::StyleVar, delta: i32) {
        let (now, _) = self.current_look();
        let (preset, _) = {
            let l = self.layout.borrow();
            theme::load_look(&l.colorscheme, &l.style, &[])
        };
        let value: kdl::KdlValue = match (var, now.style.get(var)) {
            (theme::StyleVar::FontFamily, theme::StyleValue::Text(cur)) => {
                let fonts = self.fonts();
                let i = fonts
                    .iter()
                    .position(|f| *f == cur.as_str())
                    .map(|i| i as i32)
                    .unwrap_or(0);
                let j = (i + delta).clamp(0, fonts.len() as i32 - 1) as usize;
                match fonts.get(j) {
                    Some(f) => kdl::KdlValue::String(f.clone()),
                    None => return,
                }
            }
            (_, theme::StyleValue::Flag(on)) => kdl::KdlValue::Bool(!on),
            (_, cur) => {
                let Some(knob) = theme::KNOBS.iter().find(|k| k.var == var) else {
                    return;
                };
                let Some(cur) = cur.as_f32() else {
                    return;
                };
                let v = theme::step(knob, cur, delta);
                if v.fract() == 0.0
                    && !matches!(
                        var,
                        theme::StyleVar::Motion
                            | theme::StyleVar::BackdropOpacity
                            | theme::StyleVar::CardOpacity
                    )
                {
                    kdl::KdlValue::Integer(v as i128)
                } else {
                    kdl::KdlValue::Float((v as f64 * 100.0).round() / 100.0)
                }
            }
        };
        // A value equal to the preset's drops the adjustment again.
        let mut probe = preset.style.clone();
        let same_as_preset = probe.set(var, &value).is_ok() && probe == preset.style;
        let key = var.as_ref().to_string();
        {
            let mut l = self.layout.borrow_mut();
            match l.style_overrides.iter().position(|(k, _)| *k == key) {
                Some(i) if same_as_preset => {
                    l.style_overrides.remove(i);
                }
                Some(i) => l.style_overrides[i].1 = value,
                None if same_as_preset => {}
                None => l.style_overrides.push((key, value)),
            }
        }
        self.save_layout();
        self.apply_look(true);
        self.refresh_style_vars();
    }

    fn style_reset(&self, var: Option<theme::StyleVar>) {
        {
            let mut l = self.layout.borrow_mut();
            match var {
                None => l.style_overrides.clear(),
                Some(v) => l.style_overrides.retain(|(k, _)| k != v.as_ref()),
            }
        }
        self.save_layout();
        self.apply_look(true);
        self.refresh_style_vars();
    }

    fn save_layout(&self) {
        if self.config_broken.get() {
            self.toast("config.kdl has errors, so edits aren't saved. Fix the file first.");
            return;
        }
        if let Some(text) = self.layout.borrow_mut().save() {
            *self.last_written.borrow_mut() = text;
        }
    }

    // ---- config & theme ----

    /// Load the configured color scheme and style and apply them; with `animate`, blend from
    /// the current look over a third of a second (scaled by the new style's motion).
    fn apply_look(&self, animate: bool) {
        let (target, problems) = self.current_look();
        if let Ok(mut w) = self.watched.lock() {
            let l = self.layout.borrow();
            *w = vec![
                Layout::path(),
                theme::colorscheme_path(&l.colorscheme),
                theme::style_path(&l.style),
            ];
        }
        self.show_look(target, &problems, animate);
    }

    /// The look config.kdl asks for, with notes on anything missing or wrong.
    fn current_look(&self) -> (theme::Look, Vec<String>) {
        let l = self.layout.borrow();
        theme::load_look(&l.colorscheme, &l.style, &l.style_overrides)
    }

    /// Show `target` (blending over to it with `animate`), toasting any `problems` loading it.
    fn show_look(&self, target: theme::Look, problems: &[String], animate: bool) {
        if !problems.is_empty() {
            self.toast(&format!("Theme: {}", problems.join("; ")));
        }
        let ui = self.ui();
        let from = theme::current(&ui);
        let duration = Duration::from_millis((350.0 * target.style.motion) as u64);
        if !animate || duration.is_zero() || from == target {
            self.look_timer.stop();
            theme::set(&ui, &target);
            return;
        }
        // Blend: colors and sizes move together; Slint redraws as the Theme global changes.
        let start = Instant::now();
        let weak = self.ui.clone();
        let timer = Rc::downgrade(&self.look_timer);
        self.look_timer
            .start(TimerMode::Repeated, Duration::from_millis(12), move || {
                let Some(ui) = weak.upgrade() else { return };
                let t = (start.elapsed().as_secs_f32() / duration.as_secs_f32()).min(1.0);
                if t >= 1.0 {
                    theme::set(&ui, &target);
                    if let Some(timer) = timer.upgrade() {
                        timer.stop();
                    }
                    return;
                }
                let eased = if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
                };
                theme::set(&ui, &theme::mix(&from, &target, eased));
            });
    }

    fn apply_settings(&self) {
        let (weather, cell) = {
            let l = self.layout.borrow();
            (l.weather_config(self.locale.1), l.cell_size)
        };
        self.apply_look(self.ctx.visible());
        self.weather.configure(weather);
        self.ui().set_cell_size(cell);
    }

    fn reload_config(&self) {
        let path = Layout::path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        if text == *self.last_written.borrow() {
            // Our own write; only a color scheme or style file may have changed.
            self.apply_look(self.ctx.visible());
            return;
        }
        match Layout::parse(&text) {
            Ok((fresh, warnings)) => {
                self.config_broken.set(false);
                self.layout.borrow_mut().replace(fresh);
                *self.last_written.borrow_mut() = text;
                self.apply_settings();
                self.sync_widgets();
                self.refresh_settings();
                if !warnings.is_empty() {
                    self.toast(&format!("config.kdl: {}", warnings.join("; ")));
                }
            }
            Err(e) => {
                self.config_broken.set(true);
                self.toast(&format!("config.kdl: {e}"));
            }
        }
    }

    fn clip_items(&self) {
        let today = Local::now().date_naive();
        let items: Vec<ClipItem> = self
            .clips
            .borrow()
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let when = if c.time.date_naive() == today {
                    c.time.format("%H:%M")
                } else {
                    c.time.format("%d.%m. %H:%M")
                };
                let image = c.image.as_ref().map(|(_, _, img)| img.clone());
                let info = match &c.image {
                    Some((w, h, _)) => format!("{when} · image {w}×{h}"),
                    None => {
                        let lines = String::from_utf8_lossy(&c.clip.data).lines().count();
                        if lines > 1 {
                            format!("{when} · {lines} lines")
                        } else {
                            when.to_string()
                        }
                    }
                };
                ClipItem {
                    index: i as i32,
                    text: c.text.as_str().into(),
                    is_image: image.is_some(),
                    image: image.unwrap_or_default(),
                    info: info.into(),
                }
            })
            .collect();
        self.ui().global::<UiClipboard>().set_items(model(items));
    }

    fn add_clip(&self, clip: Clip) {
        let mut clips = self.clips.borrow_mut();
        if let Some(pos) = clips.iter().position(|c| c.clip.data == clip.data) {
            let mut e = clips.remove(pos);
            e.time = Local::now();
            clips.insert(0, e);
        } else {
            let (text, image) = if clip.kind == clipboard::ClipKind::Text {
                let t = clipboard::preview(&String::from_utf8_lossy(&clip.data));
                if t.is_empty() {
                    return;
                }
                (t, None)
            } else {
                let Some((w, h, rgba)) = clipboard::decode_png(&clip.data) else {
                    return;
                };
                let (tw, th, small) = clipboard::shrink(w, h, rgba, CLIP_THUMB);
                let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&small, tw, th);
                (String::from("Image"), Some((w, h, Image::from_rgba8(buf))))
            };
            clips.insert(
                0,
                ClipEntry {
                    clip,
                    text,
                    image,
                    time: Local::now(),
                },
            );
            clips.truncate(clipboard::MAX_ENTRIES);
        }
        drop(clips);
        self.clip_items();
    }

    /// Called every loop iteration: starts the entrance once the surface is configured, and
    /// removes the surface when the exit animation is over.
    fn tick_visibility(&self, shell: &mut Shell) {
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
        if let Some(clip) = self.pending_clip.borrow_mut().take() {
            shell.set_clipboard(clip);
        }
        self.tick_extra(shell);
    }

    fn sync_grid(&self) {
        if self.solo.is_some() {
            return;
        }
        let ui = self.ui();
        let (cols, rows) = (ui.get_cols(), ui.get_rows());
        let changed = {
            let l = self.layout.borrow();
            (l.cols, l.rows) != (cols, rows)
        };
        if changed {
            self.layout.borrow_mut().set_grid(cols, rows);
            self.sync_widgets();
        }
    }

    // ---- show / hide ----

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
            self.month_offset.set(0);
            self.tick_clock(true);
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
            self.arm_clock();
        } else if self.solo.is_some() {
            // Already open: bring the window to the front.
            focus_own_window(&self.ctx);
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
        // Matches the exit animation lengths in app.slint / frame.slint.
        self.hide_at.set(Some(
            Instant::now() + Duration::from_millis((80.0 * motion) as u64),
        ));
    }

    fn set_editing(&self, on: bool) {
        let ui = self.ui();
        ui.set_editing(on);
        if on {
            // The sidebar opens with edit mode; fill its theme lists.
            self.refresh_looks();
        }
        if !on {
            self.save_layout();
            ui.set_settings_id(0);
        }
    }

    // ---- widget settings ----

    /// Open a widget's settings in the sidebar (entering edit mode if needed).
    fn configure_widget(&self, id: u32) {
        let ui = self.ui();
        if !ui.get_editing() {
            self.set_editing(true);
        }
        ui.set_settings_id(id as i32);
        ui.set_drawer_tab(crate::DrawerTab::Widget);
        self.refresh_settings();
    }

    /// Rebuild the sidebar rows for the widget whose settings are open.
    fn refresh_settings(&self) {
        use crate::widget_settings::{Ty, for_kind, get};
        let ui = self.ui();
        let id = ui.get_settings_id();
        let us = self.locale.1;
        let layout = self.layout.borrow();
        let Some(p) = (id != 0).then(|| layout.get(id as u32)).flatten() else {
            if id != 0 {
                ui.set_settings_id(0);
                if ui.get_drawer_tab() == crate::DrawerTab::Widget {
                    ui.set_drawer_tab(crate::DrawerTab::Widgets);
                }
            }
            return;
        };
        ui.set_settings_title(p.kind.title().into());
        let rows: Vec<crate::SettingRow> = for_kind(p.kind)
            .iter()
            .enumerate()
            .map(|(i, &opt)| {
                let v = get(&p.settings, opt, us);
                let (kind, placeholder) = match opt.ty() {
                    Ty::Toggle => (crate::RowKind::Toggle, ""),
                    Ty::Steps(_) | Ty::Choice(_) => (crate::RowKind::Stepper, ""),
                    Ty::Text { placeholder } => (crate::RowKind::Text, placeholder),
                };
                crate::SettingRow {
                    index: i as i32,
                    kind,
                    label: opt.label().into(),
                    value: match &v {
                        kdl::KdlValue::String(s) if kind == crate::RowKind::Text => s.clone(),
                        v => opt.value_text(v),
                    }
                    .into(),
                    on: v == kdl::KdlValue::Bool(true),
                    changed: p.settings.iter().any(|(k, _)| k == opt.as_ref()),
                    can_dec: opt.can_step(&v, -1),
                    can_inc: opt.can_step(&v, 1),
                    placeholder: placeholder.into(),
                }
            })
            .collect();
        ui.set_setting_rows(model(rows));
    }

    /// Change setting number `index` of the open widget with `f(current value)`; `None` resets it.
    fn change_setting(
        &self,
        index: i32,
        f: impl FnOnce(crate::widget_settings::Opt, &kdl::KdlValue) -> Option<kdl::KdlValue>,
    ) {
        use crate::widget_settings::{for_kind, get, set};
        let id = self.ui().get_settings_id() as u32;
        let us = self.locale.1;
        let kind = {
            let mut layout = self.layout.borrow_mut();
            let Some(kind) = layout.get(id).map(|p| p.kind) else {
                return;
            };
            let Some(&opt) = for_kind(kind).get(index as usize) else {
                return;
            };
            let Some(values) = layout.settings_mut(id) else {
                return;
            };
            let cur = get(values, opt, us);
            let new = f(opt, &cur).unwrap_or_else(|| opt.default(us));
            set(values, opt, new, us);
            kind
        };
        self.layout_changed();
        self.refresh_settings();
        if kind == Kind::Weather {
            self.apply_settings();
        }
        if kind == Kind::Mascot {
            self.sync_mascots();
        }
    }

    // ---- mascots ----

    /// Match the mascots to the mascot widgets on the dashboard (or the standalone app's).
    fn sync_mascots(&self) {
        use crate::mascots::Spec;
        use crate::widget_settings::{Opt, get_int, get_str};
        let us = self.locale.1;
        let layout = self.layout.borrow();
        let solo = self.solo.map(|k| layout.solo_widget(k));
        let placed: Vec<&Placed> = match &solo {
            Some(p) => vec![p],
            None => layout.visible(),
        };
        let specs = placed
            .into_iter()
            .filter(|p| p.kind == Kind::Mascot)
            .map(|p| {
                let v = &p.settings;
                Spec {
                    id: p.id,
                    character: character(p, us),
                    wildness: get_int(v, Opt::Wildness, us) as u8,
                    interval: Duration::from_secs(get_int(v, Opt::Interval, us) as u64),
                    command: get_str(v, Opt::Command, us),
                }
            })
            .collect();
        drop(layout);
        let todo = self.mascots.borrow_mut().sync(specs, Instant::now());
        self.mascot_todo(todo);
    }

    /// Run the commands the mascots asked for, then show what changed.
    fn mascot_todo(&self, todo: Vec<crate::mascots::Todo>) {
        for t in todo {
            if let crate::mascots::Todo::Run { id, command, name } = t {
                run_mascot_command(&self.ctx, id, command, name);
            }
        }
        self.push_mascots();
    }

    /// Mirror the board into `Mascots.lines`, indexed by widget id.
    fn push_mascots(&self) {
        let mut board = self.mascots.borrow_mut();
        if !board.take_dirty() {
            return;
        }
        let len = board.ids().max().map_or(0, |m| m as usize + 1);
        let mut rows = vec![crate::MascotLine::default(); len];
        for id in board.ids() {
            if let Some(v) = board.view(id) {
                rows[id as usize] = crate::MascotLine {
                    text: v.text.into(),
                    outfit: outfit_to_ui(v.outfit),
                    seq: v.seq,
                    queued: v.queued as i32,
                };
            }
        }
        drop(board);
        let model = &self.mascot_lines;
        if model.row_count() == rows.len() {
            for (i, row) in rows.into_iter().enumerate() {
                if model.row_data(i).as_ref() != Some(&row) {
                    model.set_row_data(i, row);
                }
            }
        } else {
            model.set_vec(rows);
        }
    }

    /// The dashboard came up or went away: the mascots pop up with something new, and their
    /// once-a-second tick only runs while they can be seen.
    fn set_awake(&self, on: bool) {
        self.timers_awake(on);
        self.ui().global::<crate::Mascots>().set_awake(on);
        if on {
            let todo = self.mascots.borrow_mut().wake(Instant::now());
            self.mascot_todo(todo);
            self.mascot_timer.restart();
        } else {
            self.mascot_timer.stop();
        }
    }

    fn mascot_tick(&self) {
        let todo = self.mascots.borrow_mut().tick(Instant::now());
        self.mascot_todo(todo);
    }

    fn mascot_say(&self, say: crate::mascots::Say) {
        let result = self.mascots.borrow_mut().say(&say, Instant::now());
        match result {
            Ok(_) => self.push_mascots(),
            Err(e) => {
                log::info!("mascots: {e}; showing it as a toast");
                self.toast(&say.text);
            }
        }
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

    // ---- clock & calendar ----

    /// Arm the clock's single-shot timer for just after the next full second. It re-arms itself
    /// while the dashboard is up and lapses when hidden: nothing reads the clock then, and `show`
    /// ticks it right away.
    fn arm_clock(&self) {
        let next = 1_005 - Local::now().timestamp_subsec_millis().min(999) as u64;
        self.clock_timer.set_interval(Duration::from_millis(next));
        self.clock_timer.restart();
    }

    fn tick_clock(&self, force_calendar: bool) {
        let now = Local::now();
        let ui = self.ui();
        let c = ui.global::<Clock>();
        let (loc, us) = self.locale;
        c.set_time(now.format("%H:%M").to_string().into());
        c.set_seconds(now.format("%S").to_string().into());
        c.set_time12(now.format("%-I:%M").to_string().into());
        c.set_ampm(now.format("%p").to_string().into());
        let date_fmt = if us {
            "%A, %B %-d, %Y"
        } else {
            "%A, %-d. %B %Y"
        };
        c.set_date(now.format_localized(date_fmt, loc).to_string().into());
        c.set_weekday(now.format_localized("%a", loc).to_string().into());
        c.set_month(now.format_localized("%B", loc).to_string().into());
        c.set_day(now.day() as i32);
        let (h, m, s) = (now.hour() as f32, now.minute() as f32, now.second() as f32);
        c.set_hour(h % 12.0 + m / 60.0);
        c.set_minute(m + s / 60.0);
        c.set_second(s);
        self.tick_zones();
        if force_calendar || self.last_day.get() != now.day() {
            self.last_day.set(now.day());
            self.build_calendar();
        }
    }

    fn build_calendar(&self) {
        let today = Local::now().date_naive();
        let off = self.month_offset.get();
        let months = today.year() * 12 + today.month0() as i32 + off;
        let (y, m) = (months.div_euclid(12), months.rem_euclid(12) as u32 + 1);
        let first = chrono::NaiveDate::from_ymd_opt(y, m, 1).expect("valid month");
        let (loc, _) = self.locale;
        // Six weeks from the Monday (or Sunday) on or before the 1st.
        let grid = |sunday: bool| {
            let back = if sunday {
                first.weekday().num_days_from_sunday()
            } else {
                first.weekday().num_days_from_monday()
            };
            let start = first - chrono::Days::new(back as u64);
            let days: Vec<CalDay> = (0..42)
                .map(|i| {
                    let d = start + chrono::Days::new(i);
                    CalDay {
                        day: d.day() as i32,
                        in_month: d.month() == m,
                        today: d == today,
                    }
                })
                .collect();
            let names: Vec<SharedString> = (0..7)
                .map(|i| {
                    let d = start + chrono::Days::new(i);
                    let s = d.format_localized("%a", loc).to_string();
                    s.chars().take(2).collect::<String>().into()
                })
                .collect();
            // ISO week of each row's Monday (the 2nd column when weeks start on Sunday).
            let weeks: Vec<i32> = (0..6)
                .map(|r| {
                    let monday = start + chrono::Days::new(r * 7 + sunday as u64);
                    monday.iso_week().week() as i32
                })
                .collect();
            (days, names, weeks)
        };
        let ui = self.ui();
        let cal = ui.global::<Calendar>();
        cal.set_title(first.format_localized("%B %Y", loc).to_string().into());
        let (days, names, weeks) = grid(false);
        cal.set_weekdays(model(names));
        cal.set_days(model(days));
        cal.set_weeks(model(weeks));
        let (days, names, weeks) = grid(true);
        cal.set_weekdays_sun(model(names));
        cal.set_days_sun(model(days));
        cal.set_weeks_sun(model(weeks));
        cal.set_showing_current(off == 0);
    }

    // ---- messages from sources ----

    fn handle(&self, msg: Msg) -> Option<Control> {
        let ui = self.ui();
        match msg {
            Msg::Control(c) => return Some(c),
            Msg::Toast(t) => self.toast(&t),
            m @ (Msg::KeyboardLayouts(_)
            | Msg::Casts(_)
            | Msg::ColorPicked(_)
            | Msg::SleepBlockers(_)
            | Msg::Privacy(_)) => {
                return self.handle_extra(m);
            }
            Msg::MascotSay(say) => self.mascot_say(say),
            Msg::MascotAnswer(id, out) => {
                self.mascots.borrow_mut().answer(id, out, Instant::now());
                self.push_mascots();
            }
            Msg::Perf(s) => {
                let s = *s;
                let p = ui.global::<Perf>();
                p.set_cpu(s.cpu);
                p.set_mem(s.mem);
                p.set_cpu_history(model(s.cpu_history));
                p.set_mem_history(model(s.mem_history));
                p.set_cpu_text(s.cpu_text.into());
                p.set_mem_text(s.mem_text.into());
                p.set_temp_text(s.temp_text.into());
                p.set_net_text(s.net_text.into());
                p.set_load_text(s.load_text.into());
                p.set_cores(model(s.cores));
                p.set_mem_used_text(s.mem_used_text.into());
                p.set_mem_avail_text(s.mem_avail_text.into());
                p.set_swap(s.swap);
                p.set_swap_text(s.swap_text.into());
                p.set_top_mem(model(
                    s.top_mem
                        .into_iter()
                        .map(|(n, v)| crate::NameValue {
                            name: n.into(),
                            value: v.into(),
                        })
                        .collect(),
                ));
                p.set_net_rx_history(model(s.net_rx_history));
                p.set_net_tx_history(model(s.net_tx_history));
                p.set_net_rx_text(s.net_rx_text.into());
                p.set_net_tx_text(s.net_tx_text.into());
                p.set_net_peak_text(s.net_peak_text.into());
                p.set_net_iface(s.net_iface.into());
                p.set_net_addr(s.net_addr.into());
                p.set_net_rx_frac(s.net_rx_frac);
                p.set_net_tx_frac(s.net_tx_frac);
                let row = |d: &sources::perf::DiskRow| crate::DiskRow {
                    mount: d.mount.as_str().into(),
                    used: d.used,
                    text: d.text.as_str().into(),
                };
                p.set_user_disks(model(
                    s.disks.iter().filter(|d| !d.system).map(row).collect(),
                ));
                p.set_disks(model(s.disks.iter().map(row).collect()));
                p.set_disk_history(model(s.disk_history));
                p.set_disk_read_text(s.disk_read_text.into());
                p.set_disk_write_text(s.disk_write_text.into());
                p.set_root_used(s.root_used);
                p.set_has_gpu(s.gpu.is_some());
                if let Some(g) = s.gpu {
                    p.set_gpu_name(g.name.into());
                    p.set_gpu_busy(g.busy);
                    p.set_gpu_history(model(g.history));
                    p.set_gpu_vram(g.vram);
                    p.set_gpu_vram_text(g.vram_text.into());
                    p.set_gpu_temp_text(g.temp_text.into());
                    p.set_gpu_power_text(g.power_text.into());
                }
            }
            Msg::ClaudeUsage(u) => {
                let c = ui.global::<crate::ClaudeUsage>();
                c.set_plan(u.plan.into());
                c.set_window_active(u.window_active);
                c.set_window_tokens(u.window_tokens.into());
                c.set_window_count(u.window_count.into());
                c.set_window_cached(u.window_cached.into());
                c.set_window_messages(u.window_messages as i32);
                c.set_window_left(u.window_left.into());
                c.set_window_progress(u.window_progress);
                c.set_window_resets(u.window_resets.into());
                c.set_burn(u.burn.into());
                c.set_today(u.today.into());
                c.set_week(u.week.into());
                c.set_hourly(model(u.hourly));
                c.set_models(model(
                    u.models
                        .into_iter()
                        .map(|(n, sh)| crate::ModelShare {
                            name: n.into(),
                            share: sh,
                        })
                        .collect(),
                ));
            }
            Msg::Windows(s) => {
                let w = ui.global::<UiWindows>();
                let rows: Vec<WinItem> = s
                    .rows
                    .iter()
                    .map(|r| {
                        let icon = self.image(&r.icon);
                        WinItem {
                            id: r.id as i32,
                            header: r.header,
                            title: r.title.as_str().into(),
                            app: r.app.as_str().into(),
                            has_icon: icon.is_some(),
                            icon: icon.unwrap_or_default(),
                            focused: r.focused,
                            urgent: r.urgent,
                        }
                    })
                    .collect();
                w.set_items(model(rows));
                w.set_count(s.count as i32);
                w.set_available(s.connected);
                let icon = s.focused_icon.as_ref().and_then(|i| self.image(i));
                w.set_has_focused_icon(icon.is_some());
                w.set_focused_icon(icon.unwrap_or_default());
            }
            Msg::Wifi(s) => {
                let w = ui.global::<UiWifi>();
                w.set_available(s.available);
                w.set_enabled(s.enabled);
                w.set_ssid(s.ssid.into());
                w.set_signal(s.signal);
                w.set_status(s.activity.to_string().into());
                w.set_busy(s.activity.busy());
                w.set_networks(model(
                    s.networks
                        .into_iter()
                        .map(|n| WifiNet {
                            ssid: n.ssid.into(),
                            signal: n.signal,
                            secure: n.secure,
                            active: n.active,
                            known: n.known,
                        })
                        .collect(),
                ));
            }
            Msg::Bluetooth(s) => {
                let b = ui.global::<UiBluetooth>();
                b.set_available(s.available);
                b.set_powered(s.powered);
                b.set_connected_count(s.devices.iter().filter(|d| d.connected).count() as i32);
                b.set_devices(model(
                    s.devices
                        .into_iter()
                        .map(|d| BtDevice {
                            address: d.address.into(),
                            name: d.name.into(),
                            connected: d.connected,
                            battery: d.battery,
                            busy: d.busy,
                        })
                        .collect(),
                ));
            }
            Msg::Inhibited(on) => ui.global::<UiPower>().set_inhibited(on),
            Msg::Power(s) => {
                let p = ui.global::<UiPower>();
                p.set_has_battery(s.has_battery);
                p.set_percent(s.percent);
                p.set_charging(s.charging);
                p.set_status_text(s.status_text.into());
                p.set_power_text(s.power_text.into());
                p.set_has_profile(s.profile.is_some());
                p.set_profile(
                    s.profile
                        .map(profile_to_ui)
                        .unwrap_or(crate::PowerProfile::Balanced),
                );
                p.set_profiles(model(s.profiles.into_iter().map(profile_to_ui).collect()));
                p.set_peripherals(model(
                    s.peripherals
                        .into_iter()
                        .map(|d| crate::Peripheral {
                            kind: extra::device_kind(d.kind),
                            name: d.name.into(),
                            percent: d.percent,
                        })
                        .collect(),
                ));
            }
            Msg::Audio(s) => {
                let a = ui.global::<UiAudio>();
                let touched = self
                    .audio_touched
                    .get()
                    .is_some_and(|t| t.elapsed() < Duration::from_millis(1500));
                if !touched {
                    a.set_volume(s.volume);
                    if let Some((v, _)) = s.mic {
                        a.set_mic_volume(v);
                    }
                }
                a.set_muted(s.muted);
                a.set_mic_muted(s.mic.is_none_or(|(_, m)| m));
                a.set_sink_name(s.sink_name.as_str().into());
                self.sync_mixer(&s);
            }
            Msg::Brightness(level) => {
                let b = ui.global::<crate::Brightness>();
                b.set_available(level.is_some());
                let touched = self
                    .brightness_touched
                    .get()
                    .is_some_and(|t| t.elapsed() < Duration::from_millis(1500));
                if let Some(l) = level
                    && !touched
                {
                    b.set_level(l);
                }
            }
            Msg::Notifications(s) => {
                let n = ui.global::<UiNotifications>();
                n.set_server(s.server);
                self.sync_dnd(s.dnd, s.missed);
                let today = Local::now().date_naive();
                n.set_items(model(
                    s.items
                        .iter()
                        .map(|x| {
                            let icon = self.image(&x.icon);
                            Notif {
                                id: x.id as i32,
                                app: x.app.as_str().into(),
                                summary: x.summary.as_str().into(),
                                body: x.body.as_str().into(),
                                time: if x.time.date_naive() == today {
                                    x.time.format("%H:%M")
                                } else {
                                    x.time.format("%d.%m. %H:%M")
                                }
                                .to_string()
                                .into(),
                                has_icon: icon.is_some(),
                                icon: icon.unwrap_or_default(),
                                has_default: x.has_default(),
                                urgent: x.urgent,
                            }
                        })
                        .collect(),
                ));
            }
            Msg::Tray(items) => {
                let t = ui.global::<UiTray>();
                t.set_items(model(
                    items
                        .iter()
                        .map(|i| {
                            let icon = self.image(&i.icon);
                            TrayItem {
                                id: i.address.as_str().into(),
                                title: i.title.as_str().into(),
                                has_icon: icon.is_some(),
                                icon: icon.unwrap_or_default(),
                                has_menu: i.has_menu,
                            }
                        })
                        .collect(),
                ));
                *self.tray_items.borrow_mut() = items;
            }
            Msg::TrayMenu(m) => {
                let t = ui.global::<UiTray>();
                if m.entries.is_empty() {
                    self.toast("This tray item has an empty menu");
                    return None;
                }
                *self.menu_address.borrow_mut() = m.address;
                t.set_menu_title(m.title.into());
                t.set_menu_x(m.x);
                t.set_menu_y(m.y);
                t.set_menu(model(
                    m.entries
                        .into_iter()
                        .map(|e| MenuEntry {
                            id: e.id,
                            label: e.label.into(),
                            depth: e.depth,
                            separator: e.separator,
                            enabled: e.enabled,
                            toggle: match e.toggle {
                                tray::Toggle::None => crate::MenuToggle::None,
                                tray::Toggle::Check => crate::MenuToggle::Check,
                                tray::Toggle::Radio => crate::MenuToggle::Radio,
                            },
                            checked: e.checked,
                            header: e.header,
                        })
                        .collect(),
                ));
                t.set_menu_open(true);
            }
            Msg::ConfigChanged => self.reload_config(),
            Msg::Clipboard(clip) => self.add_clip(clip),
            Msg::Claude(st) => {
                let c = ui.global::<UiClaude>();
                let mut wins = self.claude_windows.borrow_mut();
                wins.clear();
                c.set_busy(st.busy as i32);
                c.set_sessions(model(
                    st.sessions
                        .iter()
                        .map(|s| {
                            if let Some(w) = s.window {
                                wins.insert(s.pid, w);
                            }
                            ClaudeSession {
                                pid: s.pid,
                                title: s.title.as_str().into(),
                                place: s.place.as_str().into(),
                                status: s.status_text.as_str().into(),
                                busy: s.status == sources::claude::Status::Busy,
                                has_window: s.window.is_some(),
                            }
                        })
                        .collect(),
                ));
            }
            Msg::Weather(w) => {
                let u = ui.global::<UiWeather>();
                u.set_error(w.error.as_str().into());
                if !w.error.is_empty() {
                    // Keep showing the last good forecast if there is one.
                    return None;
                }
                u.set_loaded(true);
                u.set_place(w.place.into());
                u.set_temp(w.temp.into());
                u.set_feels(w.feels.into());
                u.set_description(w.description.into());
                u.set_icon(weather_icon(w.icon));
                u.set_details(w.details.into());
                u.set_days(model(
                    w.days
                        .into_iter()
                        .map(|d| WeatherDay {
                            name: d.name.into(),
                            icon: weather_icon(d.icon),
                            max: d.max.into(),
                            min: d.min.into(),
                        })
                        .collect(),
                ));
            }
            Msg::Media(m) => {
                let u = ui.global::<UiMedia>();
                u.set_available(m.is_some());
                let m = m.unwrap_or_default();
                u.set_title(m.title.into());
                u.set_artist(m.artist.into());
                u.set_player(m.player.into());
                u.set_playing(m.playing);
                u.set_progress(m.progress);
                let art = m.art.map(IconSrc::Path).and_then(|a| self.image(&a));
                u.set_has_art(art.is_some());
                u.set_art(art.unwrap_or_default());
            }
        }
        None
    }
}

fn wire(app: &Rc<App>, ui: &Dash) {
    extra::wire(app, ui);
    let a = app.clone();
    ui.on_close_requested(move || a.pending.borrow_mut().push(Control::Hide));
    let a = app.clone();
    ui.global::<UiShell>()
        .on_hide(move || a.pending.borrow_mut().push(Control::Hide));

    // Edit mode.
    let a = app.clone();
    ui.on_set_editing(move |on| a.set_editing(on));
    // Live reflow: dragging shows where everything would go; dropping commits it.
    let a = app.clone();
    ui.on_preview_move(move |id, c, r| a.preview_move(id as u32, c, r));
    let a = app.clone();
    ui.on_preview_resize(move |id, w, h| a.preview_resize(id as u32, w, h));
    let a = app.clone();
    ui.on_preview_add(move |k, c, r| a.preview_add(kind_from_ui(k), c, r));
    let a = app.clone();
    ui.on_end_preview(move || a.set_preview(None));
    let a = app.clone();
    ui.on_move_widget(move |id, c, r| {
        a.set_preview(None);
        let prefer = a.direction(id as u32, c, r);
        if a.layout.borrow_mut().move_reflow(id as u32, c, r, prefer) {
            a.layout_changed();
        } else {
            a.sync_widgets();
            a.toast("No room there");
        }
    });
    let a = app.clone();
    ui.on_resize_widget(move |id, w, h| {
        a.set_preview(None);
        if a.layout.borrow_mut().resize_reflow(id as u32, w, h) {
            a.layout_changed();
        } else {
            a.sync_widgets();
        }
    });
    let a = app.clone();
    ui.on_add_widget_at(move |k, c, r| {
        a.set_preview(None);
        let kind = kind_from_ui(k);
        if a.layout.borrow_mut().add_at(kind, c, r).is_some() {
            a.layout_changed();
        } else {
            a.sync_widgets();
            a.toast("No room there");
        }
    });
    let a = app.clone();
    ui.on_choose_colorscheme(move |name| {
        a.layout.borrow_mut().colorscheme = name.to_string();
        a.save_layout();
        a.apply_look(true);
        a.refresh_looks();
    });
    let a = app.clone();
    ui.on_choose_style(move |name| {
        // Clicking the current preset changes nothing ("Reset all" is for dropping adjustments).
        if a.layout.borrow().style == name.as_str() {
            return;
        }
        {
            // The preset takes over what it changes; other adjustments (often the font, glass,
            // opacity) stay, since no preset has an opinion on them.
            let stext =
                theme::style_text(&name).unwrap_or_else(|| theme::DEFAULT_STYLE.to_string());
            let taken = theme::preset_changes(&stext);
            let mut l = a.layout.borrow_mut();
            l.style = name.to_string();
            l.style_overrides.retain(|(k, _)| {
                // Unknown names (typos in a hand-edited file) stay for the user to see.
                k.parse::<theme::StyleVar>()
                    .map_or(true, |v| !taken.contains(&v))
            });
        }
        a.save_layout();
        a.apply_look(true);
        a.refresh_looks();
    });
    let a = app.clone();
    ui.on_style_step(move |key, delta| a.style_step(style_var(key), delta));
    let a = app.clone();
    ui.on_style_reset(move |key| a.style_reset(Some(style_var(key))));
    let a = app.clone();
    ui.on_style_reset_all(move || a.style_reset(None));
    ui.on_open_themes_folder(|| {
        let dir = theme::colorschemes_dir();
        let dir = dir.parent().unwrap_or(&dir);
        if let Err(e) = sources::spawn_detached("xdg-open", &[&dir.to_string_lossy()]) {
            log::warn!("{e}");
        }
    });
    let a = app.clone();
    ui.on_toggle_shape(move |id| {
        if a.layout.borrow_mut().toggle_shape(id as u32) {
            a.layout_changed();
        } else {
            a.toast("No room to make this widget round here");
        }
    });
    let a = app.clone();
    ui.on_remove_widget(move |id| {
        a.layout.borrow_mut().remove(id as u32);
        a.layout_changed();
        // Closes the settings tab if they were this widget's.
        a.refresh_settings();
    });
    let a = app.clone();
    ui.on_configure_widget(move |id| a.configure_widget(id as u32));
    let a = app.clone();
    ui.on_setting_step(move |index, delta| {
        a.change_setting(index, |opt, cur| Some(opt.step(cur, delta)))
    });
    let a = app.clone();
    ui.on_setting_text(move |index, text| {
        let text = text.trim().to_string();
        a.change_setting(index, |_, _| {
            (!text.is_empty()).then_some(kdl::KdlValue::String(text))
        })
    });
    let a = app.clone();
    ui.on_setting_reset(move |index| a.change_setting(index, |_, _| None));
    let a = app.clone();
    ui.on_add_widget(move |k| {
        let kind = kind_from_ui(k);
        if a.layout.borrow_mut().add(kind).is_some() {
            a.layout_changed();
            a.toast(&format!("Added {}", kind.title()));
        } else {
            a.toast("No free space. Remove or shrink a widget first.");
        }
    });
    let a = app.clone();
    ui.on_reset_layout(move || {
        a.layout.borrow_mut().reset();
        a.layout_changed();
    });
    ui.set_palette(model(
        Kind::iter()
            .collect::<Vec<_>>()
            .iter()
            .map(|k| PaletteItem {
                kind: kind_to_ui(*k),
                title: k.title().into(),
                description: k.description().into(),
                w: k.default_size().0,
                h: k.default_size().1,
            })
            .collect(),
    ));

    // Calendar.
    let a = app.clone();
    ui.global::<Calendar>().on_shift(move |d| {
        a.month_offset
            .set(if d == 0 { 0 } else { a.month_offset.get() + d });
        a.build_calendar();
    });

    // Windows.
    let w = ui.global::<UiWindows>();
    let a = app.clone();
    w.on_focus(move |id| niri::focus(&a.ctx, id as u64));
    let a = app.clone();
    w.on_close(move |id| niri::close(&a.ctx, id as u64));

    // Wi-Fi.
    let w = ui.global::<UiWifi>();
    let a = app.clone();
    w.on_toggle(move || a.wifi.toggle());
    let a = app.clone();
    w.on_rescan(move || a.wifi.rescan());
    let a = app.clone();
    w.on_connect(move |ssid, pw| a.wifi.connect(ssid.into(), pw.into()));
    let a = app.clone();
    w.on_disconnect(move || a.wifi.disconnect());

    // Bluetooth.
    let b = ui.global::<UiBluetooth>();
    let a = app.clone();
    b.on_toggle(move || a.bluetooth.toggle());
    let a = app.clone();
    b.on_connect(move |addr| a.bluetooth.connect(addr.into()));
    let a = app.clone();
    b.on_disconnect(move |addr| a.bluetooth.disconnect(addr.into()));

    // Power, audio, brightness.
    let a = app.clone();
    ui.global::<UiPower>()
        .on_set_profile(move |p| power::set_profile(&a.ctx, profile_from_ui(p)));
    let a = app.clone();
    ui.global::<UiPower>()
        .on_toggle_inhibit(move || a.inhibit.toggle());
    let au = ui.global::<UiAudio>();
    let a = app.clone();
    au.on_set_volume(move |v| {
        a.audio_touched.set(Some(Instant::now()));
        a.audio.set_volume(v);
    });
    let a = app.clone();
    au.on_set_mic_volume(move |v| {
        a.audio_touched.set(Some(Instant::now()));
        a.audio.set_mic_volume(v);
    });
    let a = app.clone();
    au.on_toggle_mute(move || a.audio.toggle_mute());
    let a = app.clone();
    au.on_toggle_mic_mute(move || a.audio.toggle_mic_mute());
    let a = app.clone();
    ui.global::<crate::Brightness>().on_set(move |v| {
        a.brightness_touched.set(Some(Instant::now()));
        a.brightness.set(v);
        if let Some(ui) = a.ui.upgrade() {
            ui.global::<crate::Brightness>().set_level(v);
        }
    });

    // Notifications.
    let n = ui.global::<UiNotifications>();
    let a = app.clone();
    n.on_dismiss(move |id| a.notifications.dismiss(id as u32));
    let a = app.clone();
    n.on_clear(move || a.notifications.clear());
    let a = app.clone();
    n.on_invoke(move |id| a.notifications.invoke(id as u32));

    // Tray.
    let t = ui.global::<UiTray>();
    let a = app.clone();
    t.on_activate(move |id, x, y| {
        let item = a
            .tray_items
            .borrow()
            .iter()
            .find(|i| i.address == id.as_str())
            .cloned();
        match item {
            Some(i) if i.item_is_menu && i.has_menu => a.tray.open_menu(i.address, x, y),
            Some(i) => {
                a.tray.activate(i.address, x as i32, y as i32);
                a.pending.borrow_mut().push(Control::Hide);
            }
            None => {}
        }
    });
    let a = app.clone();
    t.on_open_menu(move |id, x, y| a.tray.open_menu(id.into(), x, y));
    let a = app.clone();
    t.on_menu_activate(move |id| {
        let address = a.menu_address.borrow().clone();
        a.tray.menu_item(address, id);
        if let Some(ui) = a.ui.upgrade() {
            ui.global::<UiTray>().set_menu_open(false);
        }
        a.pending.borrow_mut().push(Control::Hide);
    });
    let a = app.clone();
    t.on_menu_close(move || {
        if let Some(ui) = a.ui.upgrade() {
            ui.global::<UiTray>().set_menu_open(false);
        }
    });

    // Claude sessions.
    let a = app.clone();
    ui.global::<UiClaude>().on_focus(move |pid| {
        let win = a.claude_windows.borrow().get(&pid).copied();
        if let Some(id) = win {
            niri::focus(&a.ctx, id);
        }
    });

    // Clipboard.
    let cb = ui.global::<UiClipboard>();
    let a = app.clone();
    cb.on_copy(move |i| {
        let clip = a.clips.borrow().get(i as usize).map(|c| c.clip.clone());
        if let Some(clip) = clip {
            *a.pending_clip.borrow_mut() = Some(clip);
            a.toast("Copied");
        }
    });
    let a = app.clone();
    cb.on_remove(move |i| {
        let mut clips = a.clips.borrow_mut();
        if (i as usize) < clips.len() {
            clips.remove(i as usize);
        }
        drop(clips);
        a.clip_items();
    });
    let a = app.clone();
    cb.on_clear(move || {
        a.clips.borrow_mut().clear();
        a.clip_items();
    });

    // Media.
    let m = ui.global::<UiMedia>();
    let a = app.clone();
    m.on_play_pause(move || a.media.play_pause());
    let a = app.clone();
    m.on_next(move || a.media.next());
    let a = app.clone();
    m.on_previous(move || a.media.previous());

    // Session.
    let s = ui.global::<UiSession>();
    let a = app.clone();
    s.on_lock(move || {
        a.pending.borrow_mut().push(Control::Hide);
        session::lock(&a.ctx);
    });
    let a = app.clone();
    s.on_suspend(move || {
        a.pending.borrow_mut().push(Control::Hide);
        session::suspend(&a.ctx);
    });
    let a = app.clone();
    s.on_logout(move || {
        a.pending.borrow_mut().push(Control::Hide);
        session::logout(&a.ctx);
    });
    let a = app.clone();
    s.on_reboot(move || session::reboot(&a.ctx));
    let a = app.clone();
    s.on_poweroff(move || session::poweroff(&a.ctx));
}

fn outfit_to_ui(o: crate::mascots::Outfit) -> crate::Outfit {
    use crate::mascots::Outfit as O;
    match o {
        O::Plain => crate::Outfit::Plain,
        O::Helmet => crate::Outfit::Helmet,
        O::Vest => crate::Outfit::Vest,
        O::Beard => crate::Outfit::Beard,
        O::Shades => crate::Outfit::Shades,
        O::Tophat => crate::Outfit::Tophat,
        O::Party => crate::Outfit::Party,
    }
}

/// Run a mascot widget's `command` (with `NIRI_DASH_MASCOT` set to its name) and send back what
/// it printed. A command that fails, hangs or prints nothing gets a remark of the mascot's own.
fn run_mascot_command(ctx: &Ctx, id: u32, command: String, name: &'static str) {
    let ctx2 = ctx.clone();
    ctx.rt.spawn(async move {
        let run = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&command)
            .env("NIRI_DASH_MASCOT", name)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output();
        let out = match tokio::time::timeout(Duration::from_secs(10), run).await {
            Ok(Ok(o)) if o.status.success() => {
                crate::mascots::from_command_output(&String::from_utf8_lossy(&o.stdout))
            }
            Ok(Ok(o)) => {
                log::warn!("mascot command {command:?} failed: {}", o.status);
                None
            }
            Ok(Err(e)) => {
                log::warn!("mascot command {command:?}: {e}");
                None
            }
            Err(_) => {
                log::warn!("mascot command {command:?} took over 10 s");
                None
            }
        };
        ctx2.send(Msg::MascotAnswer(id, out));
    });
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

/// Watch config.kdl and the theme files: inotify on the directories holding them (after following
/// symlinks, so a symlinked dotfile counts as its target). The list of files changes with the
/// theme, which goes through config.kdl, so the watches are re-synced after every event.
fn watch_config(paths: Arc<Mutex<Vec<PathBuf>>>, ctx: Ctx) {
    use notify::{RecursiveMode, Watcher};
    use std::collections::HashSet;
    fn resolve(p: &std::path::Path) -> PathBuf {
        std::fs::canonicalize(p).unwrap_or_else(|_| {
            match (
                p.parent().and_then(|d| std::fs::canonicalize(d).ok()),
                p.file_name(),
            ) {
                (Some(d), Some(n)) => d.join(n),
                _ => p.to_path_buf(),
            }
        })
    }
    std::thread::Builder::new()
        .name("config-watch".into())
        .spawn(move || {
            let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
            let mut watcher = match notify::recommended_watcher(tx) {
                Ok(w) => w,
                Err(e) => {
                    log::warn!("not watching the config for changes: {e}");
                    return;
                }
            };
            let mut dirs: HashSet<PathBuf> = HashSet::new();
            loop {
                let list = paths.lock().map(|p| p.clone()).unwrap_or_default();
                let targets: Vec<PathBuf> = list.iter().map(|p| resolve(p)).collect();
                let want: HashSet<PathBuf> = targets
                    .iter()
                    .filter_map(|t| t.parent().map(|d| d.to_path_buf()))
                    .collect();
                for d in dirs.difference(&want) {
                    let _ = watcher.unwatch(d);
                }
                for d in want.difference(&dirs) {
                    if let Err(e) = watcher.watch(d, RecursiveMode::NonRecursive) {
                        log::debug!("watching {}: {e}", d.display());
                    }
                }
                dirs = want;
                let hits = |ev: notify::Result<notify::Event>| {
                    ev.is_ok_and(|e| {
                        !e.kind.is_access() && e.paths.iter().any(|p| targets.contains(p))
                    })
                };
                let Ok(ev) = rx.recv() else { return };
                let mut changed = hits(ev);
                // Editors save in several steps; report the burst once.
                std::thread::sleep(Duration::from_millis(50));
                while let Ok(ev) = rx.try_recv() {
                    changed |= hits(ev);
                }
                if changed {
                    ctx.send(Msg::ConfigChanged);
                }
            }
        })
        .expect("spawning config watcher");
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
}

/// The sources a standalone app of `kind` has to run. Clock and calendar tick on their own, the
/// clipboard comes from the Wayland side.
fn feeds(kind: Kind) -> &'static [Feed] {
    match kind {
        Kind::Clock | Kind::Calendar | Kind::Clipboard | Kind::Session => &[],
        Kind::Perf | Kind::Memory | Kind::Disks | Kind::Network | Kind::Gpu => &[Feed::Perf],
        Kind::Windows => &[Feed::Niri],
        Kind::Claude => &[Feed::Claude],
        Kind::ClaudeUsage => &[Feed::ClaudeUsage],
        Kind::Wifi => &[Feed::Wifi],
        Kind::Bluetooth => &[Feed::Bluetooth],
        Kind::Battery => &[Feed::Power, Feed::Inhibit],
        Kind::Audio => &[Feed::Audio],
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
    }
}

/// Whether a standalone app keeps running when its window closes, so the next `niri-dash app`
/// shows it again with its state. The clipboard does: its history is what it is for.
pub fn resident(kind: Kind) -> bool {
    kind == Kind::Clipboard
}

fn build(ui: &Dash, ctx: Ctx, solo: Option<Kind>) -> Rc<App> {
    let wants = |f: Feed| solo.is_none_or(|k| feeds(k).contains(&f));
    // Sources a standalone app doesn't show get a dormant context: they exist but stay idle.
    let feed_ctx = |f: Feed| ctx.live_if(wants(f));
    sources::perf::start(feed_ctx(Feed::Perf));
    niri::start(feed_ctx(Feed::Niri));
    power::start(feed_ctx(Feed::Power));
    sources::claude::start(feed_ctx(Feed::Claude));
    sources::claude_usage::start(feed_ctx(Feed::ClaudeUsage));
    sources::privacy::start(feed_ctx(Feed::Privacy));
    theme::install_bundled();
    ui.set_solo(solo.is_some());

    let (layout, warnings) = Layout::load();
    let locale = sources::locale();
    let widgets = Rc::new(VecModel::<WidgetInst>::default());
    ui.set_widgets(ModelRc::from(widgets.clone()));
    let config_broken = warnings.iter().any(|w| w.starts_with("config.kdl:"));
    let app = Rc::new(App {
        ui: ui.as_weak(),
        solo,
        layout: RefCell::new(layout),
        widgets,
        images: Default::default(),
        image_uses: Cell::new(0),
        pending: Default::default(),
        month_offset: Cell::new(0),
        last_day: Cell::new(0),
        locale,
        audio_touched: Cell::new(None),
        brightness_touched: Cell::new(None),
        tray_items: Default::default(),
        menu_address: Default::default(),
        toast_timer: Timer::default(),
        clock_timer: Timer::default(),
        look_timer: Default::default(),
        fonts: Default::default(),
        reveal_pending: Cell::new(false),
        hide_at: Cell::new(None),
        clips: Default::default(),
        preview: Default::default(),
        pending_clip: Default::default(),
        claude_windows: Default::default(),
        last_written: RefCell::new(std::fs::read_to_string(Layout::path()).unwrap_or_default()),
        config_broken: Cell::new(config_broken),
        watched: Default::default(),
        weather: Weather::start(feed_ctx(Feed::Weather), locale.0),
        wifi: Wifi::start(feed_ctx(Feed::Wifi)),
        bluetooth: Bluetooth::start(feed_ctx(Feed::Bluetooth)),
        audio: Audio::start(feed_ctx(Feed::Audio)),
        brightness: Brightness::start(feed_ctx(Feed::Brightness)),
        notifications: Notifications::start(feed_ctx(Feed::Notifications)),
        tray: tray::Tray::start(feed_ctx(Feed::Tray)),
        media: Media::start(feed_ctx(Feed::Media)),
        inhibit: Inhibit::start(feed_ctx(Feed::Inhibit)),
        mascots: Default::default(),
        mascot_lines: Rc::new(VecModel::default()),
        mascot_timer: Timer::default(),
        extra: Extra::new(solo),
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
    ui.global::<crate::Mascots>()
        .set_lines(ModelRc::from(app.mascot_lines.clone()));
    let a = Rc::downgrade(&app);
    app.mascot_timer
        .start(TimerMode::Repeated, Duration::from_secs(1), move || {
            if let Some(a) = a.upgrade() {
                a.mascot_tick();
            }
        });
    app.mascot_timer.stop();
    let a = Rc::downgrade(&app);
    ui.global::<crate::Mascots>().on_next(move |id| {
        if let Some(a) = a.upgrade() {
            let todo = a.mascots.borrow_mut().next(id as u32, Instant::now());
            a.mascot_todo(vec![todo]);
        }
    });
    watch_config(app.watched.clone(), ctx);
    if app.ctx.visible() {
        // Snapshots render without `show`.
        app.set_awake(true);
    }
    app.sync_grid();
    app.sync_widgets();
    app.tick_clock(true);
    let a = Rc::downgrade(&app);
    app.clock_timer
        .start(TimerMode::SingleShot, Duration::from_secs(1), move || {
            if let Some(a) = a.upgrade()
                && a.ctx.visible()
            {
                a.tick_clock(false);
                a.arm_clock();
            }
        });
    app
}

/// Render the dashboard offscreen into a PNG, with live data, without touching the screen.
pub fn snapshot(
    out: &std::path::Path,
    width: u32,
    height: u32,
    scale: f32,
    wait: Duration,
    edit: bool,
    solo: Option<Kind>,
) -> Result<()> {
    use slint::platform::WindowEvent;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let incremental = std::env::var_os("NIRI_DASH_INCREMENTAL").is_some();
    // NIRI_DASH_RENDERER=gpu: draw with FemtoVG/OpenGL ES into an offscreen texture instead.
    let gpu = match std::env::var("NIRI_DASH_RENDERER")
        .ok()
        .and_then(|r| r.parse().ok())
    {
        Some(crate::layout::RendererChoice::Gpu) => {
            let gl = crate::gpu::Gl::offscreen()?;
            Some((crate::gpu::GpuWindow::new(&gl)?, gl))
        }
        _ => None,
    };
    let soft = MinimalSoftwareWindow::new(if incremental {
        RepaintBufferType::ReusedBuffer
    } else {
        RepaintBufferType::NewBuffer
    });
    let adapter: Rc<dyn WindowAdapter> = match &gpu {
        Some((w, _)) => w.clone(),
        None => soft.clone(),
    };
    let window = adapter.window();
    slint::platform::set_platform(Box::new(LayerPlatform {
        window: adapter.clone(),
    }))
    .map_err(|e| Error::Ui(format!("slint platform: {e}")))?;
    // Draws a frame into `pixels` if anything changed (or always with `full`).
    let render_into =
        |window: &slint::Window,
         pixels: &mut [slint::platform::software_renderer::PremultipliedRgbaColor],
         stride: usize,
         full: bool|
         -> Option<()> {
            let Some((g, _)) = &gpu else {
                return crate::shell::render_into(&soft, pixels, stride, full).map(|_| ());
            };
            if !full && !g.needs_redraw() {
                return None;
            }
            g.mark_drawn();
            let shot = window
                .take_snapshot()
                .map_err(|e| log::warn!("GPU snapshot: {e}"))
                .ok()?;
            for (d, s) in pixels.iter_mut().zip(shot.as_slice()) {
                *d = slint::platform::software_renderer::PremultipliedRgbaColor {
                    red: s.r,
                    green: s.g,
                    blue: s.b,
                    alpha: s.a,
                };
            }
            Some(())
        };
    let ui = Dash::new().map_err(|e| Error::Ui(format!("creating UI: {e}")))?;
    ui.show().map_err(|e| Error::Ui(e.to_string()))?;
    let (pw, ph) = (
        (width as f32 * scale).round() as u32,
        (height as f32 * scale).round() as u32,
    );
    window.dispatch_event(WindowEvent::ScaleFactorChanged {
        scale_factor: scale,
    });
    window.set_size(slint::PhysicalSize::new(pw, ph));
    let mut event_loop: EventLoop<'static, ()> = EventLoop::try_new()?;
    let (tx, rx) = channel::channel::<Msg>();
    let ctx = Ctx::new(tx, rt.handle().clone());
    ctx.set_visible(true);
    let app = build(&ui, ctx.clone(), solo);
    let a = app.clone();
    event_loop
        .handle()
        .insert_source(rx, move |ev, _, _| {
            if let channel::Event::Msg(m) = ev {
                let _ = a.handle(m);
            }
        })
        .map_err(|e| Error::Ui(e.to_string()))?;
    // NIRI_DASH_COLORSCHEME / NIRI_DASH_STYLE preview a theme without touching config.kdl.
    let (cs, st) = (
        std::env::var("NIRI_DASH_COLORSCHEME").ok(),
        std::env::var("NIRI_DASH_STYLE").ok(),
    );
    if cs.is_some() || st.is_some() {
        let (look, problems) = {
            let l = app.layout.borrow();
            theme::load_look(
                cs.as_deref().unwrap_or(&l.colorscheme),
                st.as_deref().unwrap_or(&l.style),
                &l.style_overrides,
            )
        };
        app.show_look(look, &problems, false);
    }
    if edit {
        app.set_editing(true);
        // NIRI_DASH_DRAWER=1 also opens the widget list.
        // NIRI_DASH_TAB=colors|style opens that sidebar tab.
        match std::env::var("NIRI_DASH_TAB").as_deref() {
            Ok("colors") => ui.set_drawer_tab(crate::DrawerTab::Colors),
            Ok("style") => ui.set_drawer_tab(crate::DrawerTab::Style),
            _ => {}
        }
        // NIRI_DASH_CONFIGURE=n opens the settings of the n-th widget in the file.
        if let Some(id) = std::env::var("NIRI_DASH_CONFIGURE")
            .ok()
            .and_then(|n| n.parse::<usize>().ok())
            .and_then(|n| app.layout.borrow().widgets.get(n).map(|w| w.id))
        {
            app.configure_widget(id);
        }
    }
    let end = Instant::now() + wait;
    let mut first = true;
    let mut pixels = vec![
        slint::platform::software_renderer::PremultipliedRgbaColor::default();
        (pw * ph) as usize
    ];
    while Instant::now() < end {
        event_loop.dispatch(Some(Duration::from_millis(50)), &mut ())?;
        slint::platform::update_timers_and_animations();
        if first {
            // Let layouts settle once before the grid size is read.
            first = false;
            ctx.refresh();
        }
        app.sync_grid();
        if incremental {
            render_into(window, &mut pixels, pw as usize, false);
        }
    }
    // NIRI_DASH_EVENTS="down:x,y;move:x,y;up:x,y;dbl:x,y;wait:ms": scripted pointer input,
    // rendered incrementally with animations running in between.
    if let Ok(script) = std::env::var("NIRI_DASH_EVENTS") {
        use slint::platform::{PointerEventButton as B, WindowEvent as E};
        let pump = |ms: u64, pixels: &mut Vec<_>| {
            let t = Instant::now();
            while t.elapsed() < Duration::from_millis(ms) {
                slint::platform::update_timers_and_animations();
                render_into(window, pixels, pw as usize, false);
                std::thread::sleep(Duration::from_millis(8));
            }
        };
        for step in script.split(';').filter(|s| !s.is_empty()) {
            let (verb, arg) = step.split_once(':').unwrap_or((step, ""));
            if verb == "wait" {
                pump(arg.parse().unwrap_or(300), &mut pixels);
                continue;
            }
            let (x, y) = arg
                .split_once(',')
                .map(|(a, b)| (a.parse().unwrap_or(0.0), b.parse().unwrap_or(0.0)))
                .unwrap_or((0.0, 0.0));
            let position = slint::LogicalPosition::new(x, y);
            let ev = |e: E| window.dispatch_event(e);
            match verb {
                "down" => ev(E::PointerPressed {
                    position,
                    button: B::Left,
                }),
                "up" => ev(E::PointerReleased {
                    position,
                    button: B::Left,
                }),
                "move" => ev(E::PointerMoved { position }),
                "dbl" => {
                    ev(E::PointerMoved { position });
                    for _ in 0..2 {
                        ev(E::PointerPressed {
                            position,
                            button: B::Left,
                        });
                        ev(E::PointerReleased {
                            position,
                            button: B::Left,
                        });
                    }
                }
                _ => log::warn!("unknown event {verb}"),
            }
            pump(40, &mut pixels);
        }
        pump(400, &mut pixels);
    }
    // NIRI_DASH_HOVER=x,y: move the pointer there and render that update incrementally.
    if let Some((x, y)) = std::env::var("NIRI_DASH_HOVER").ok().and_then(|v| {
        let (a, b) = v.split_once(',')?;
        Some((a.parse::<f32>().ok()?, b.parse::<f32>().ok()?))
    }) {
        window.dispatch_event(slint::platform::WindowEvent::PointerMoved {
            position: slint::LogicalPosition::new(x, y),
        });
        slint::platform::update_timers_and_animations();
        render_into(window, &mut pixels, pw as usize, false);
    }
    // NIRI_DASH_BENCH=1: time the frames of the open animation (incremental rendering).
    if std::env::var_os("NIRI_DASH_BENCH").is_some() {
        ui.set_shown(false);
        let t = Instant::now();
        while t.elapsed() < Duration::from_millis(300) {
            slint::platform::update_timers_and_animations();
            render_into(window, &mut pixels, pw as usize, false);
        }
        ui.set_shown(true);
        let start = Instant::now();
        let mut times = Vec::new();
        while start.elapsed() < Duration::from_millis(900) {
            slint::platform::update_timers_and_animations();
            let f = Instant::now();
            if render_into(window, &mut pixels, pw as usize, false).is_some() {
                times.push(f.elapsed().as_secs_f64() * 1000.0);
            }
        }
        times.sort_by(|a, b| a.total_cmp(b));
        let n = times.len().max(1);
        println!(
            "frames {} median {:.1} ms p90 {:.1} ms max {:.1} ms",
            times.len(),
            times[n / 2],
            times[n * 9 / 10],
            times[n - 1]
        );
    }
    // Like hiding and showing again: a fresh buffer, then a full frame.
    // NIRI_DASH_KEEP=1 keeps the incrementally rendered frame instead.
    let keep = incremental && std::env::var_os("NIRI_DASH_KEEP").is_some();
    if !keep {
        pixels.fill(Default::default());
    }
    // NIRI_DASH_NO_FULL=1 reproduces the old bug (partial frame after re-show).
    if !keep {
        render_into(
            window,
            &mut pixels,
            pw as usize,
            std::env::var_os("NIRI_DASH_NO_FULL").is_none(),
        );
    }
    let file = std::fs::File::create(out).map_err(|source| Error::Create {
        path: out.to_path_buf(),
        source,
    })?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), pw, ph);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header()?;
    let data: Vec<u8> = pixels
        .iter()
        .flat_map(|p| [p.red, p.green, p.blue, p.alpha])
        .collect();
    w.write_image_data(&data)?;
    Ok(())
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
    serve(listener, Mode::Dashboard, None, initial)
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
    let layout = Layout::load().0;
    let placed = layout.solo_widget(kind);
    let px = |cells: i32| (cells.max(1) as f32 * layout.cell_size).round() as u32;
    let (min_w, min_h) = kind.min_size();
    let mode = Mode::App {
        app_id: crate::desktop::app_id(kind),
        title: crate::desktop::name(kind),
        size: (px(placed.w), px(placed.h)),
        min_size: (px(min_w), px(min_h)),
    };
    log::info!("{} running; control socket {}", kind.name(), path.display());
    let result = serve(listener, mode, Some(kind), Some(Control::Show));
    let _ = std::fs::remove_file(&path);
    result
}

/// Bring this app's window to the front (niri knows it by our pid).
fn focus_own_window(ctx: &Ctx) {
    let pid = std::process::id() as i32;
    ctx.act("Focus window", async move {
        tokio::task::spawn_blocking(move || {
            let niri_ipc::Response::Windows(windows) = niri::request(niri_ipc::Request::Windows)?
            else {
                return Ok(());
            };
            match windows.iter().find(|w| w.pid == Some(pid)) {
                Some(w) => niri::action(niri_ipc::Action::FocusWindow { id: w.id }),
                None => Ok(()),
            }
        })
        .await?
    });
}

/// The event loop shared by the dashboard and the standalone apps. Returns when a (non-resident)
/// app's window closes; the dashboard runs until it is told to quit.
fn serve(
    listener: UnixListener,
    mode: Mode,
    solo: Option<Kind>,
    initial: Option<Control>,
) -> Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("niri-dash-io")
        .enable_all()
        .build()?;

    let conn = Connection::connect_to_env()?;
    let (globals, queue) = registry_queue_init::<Shell>(&conn)?;
    let mut event_loop: EventLoop<'static, Shell> = EventLoop::try_new()?;
    let backend = choose_backend(&conn);
    let window = backend.adapter();
    slint::platform::set_platform(Box::new(LayerPlatform {
        window: window.clone(),
    }))
    .map_err(|e| Error::Ui(format!("slint platform: {e}")))?;
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

    let ui = Dash::new().map_err(|e| Error::Ui(format!("creating UI: {e}")))?;
    ui.show().map_err(|e| Error::Ui(e.to_string()))?;

    let (tx, rx) = channel::channel::<Msg>();
    let ctx = Ctx::new(tx.clone(), rt.handle().clone());
    shell.clip_tx = Some(tx.clone());
    let app = build(&ui, ctx, solo);
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
        app.tick_visibility(&mut shell);
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
            shell.set_cursor(match ui.get_cursor() {
                crate::PointerShape::Move => CursorIcon::Move,
                crate::PointerShape::Resize => CursorIcon::NwseResize,
                crate::PointerShape::Normal => CursorIcon::Default,
            });
            if bench {
                window.window().request_redraw();
            }
            shell.render_if_needed();
        }
        let _ = conn.flush();
    }
}

/// The renderer from the config (`renderer`), or `NIRI_DASH_RENDERER`. `auto` prefers the GPU and
/// falls back to software when EGL/OpenGL ES can't be set up.
fn choose_backend(conn: &Connection) -> crate::shell::Backend {
    use crate::layout::RendererChoice;
    use crate::shell::Backend;
    let choice = std::env::var("NIRI_DASH_RENDERER")
        .ok()
        .and_then(|r| r.parse().ok())
        .unwrap_or_else(|| crate::layout::Layout::load().0.renderer);
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

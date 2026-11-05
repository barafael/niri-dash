//! Widget placement on the dashboard grid, declared in `~/.config/niri-dash/config.kdl`.
//!
//! The file is the source of truth. Edit mode writes changes back into it in place, so comments
//! and formatting the user wrote survive; widgets the user edits by hand are picked up live.

use kdl::{KdlDocument, KdlEntry, KdlEntryFormat, KdlNode, KdlNodeFormat, KdlValue};
use std::path::PathBuf;
use std::str::FromStr;
use strum::{AsRefStr, EnumIter, EnumString, IntoEnumIterator, IntoStaticStr};

/// A widget type. Its name ([`Kind::name`]) is what config.kdl uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, EnumString, AsRefStr, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum Kind {
    Clock,
    Calendar,
    SystemMonitor,
    Windows,
    ClaudeSessions,
    Wifi,
    Bluetooth,
    Battery,
    Volume,
    Brightness,
    Notifications,
    Tray,
    Media,
    Session,
    Clipboard,
    Weather,
    Memory,
    Disks,
    Network,
    Gpu,
    ClaudeUsage,
    Mascot,
    KeyboardLayout,
    ColorPicker,
    Timer,
    Stopwatch,
    Notes,
    Privacy,
    NightLight,
    RemovableDrives,
    Phone,
    SleepTimer,
}

impl Kind {
    /// The name used in config.kdl.
    pub fn name(self) -> &'static str {
        self.into()
    }

    pub fn from_str_opt(name: &str) -> Option<Kind> {
        Kind::from_str(name).ok()
    }

    /// What this kind of widget is: one entry per kind, so a new kind doesn't build until it is
    /// described here.
    pub fn info(self) -> &'static KindInfo {
        use crate::widget_settings::Opt::*;
        match self {
            Kind::Clock => &KindInfo {
                title: "Clock",
                description: "Digital, or analog when round",
                default_size: (4, 2),
                min_size: (2, 1),
                can_be_round: true,
                starts_round: false,
                settings: &[Hours, Seconds, Date, Zones],
                icon: "preferences-system-time",
            },
            Kind::Calendar => &KindInfo {
                title: "Calendar",
                description: "Month view",
                default_size: (4, 4),
                min_size: (3, 3),
                can_be_round: true,
                starts_round: false,
                settings: &[FirstDay, WeekNumbers],
                icon: "x-office-calendar",
            },
            Kind::SystemMonitor => &KindInfo {
                title: "System monitor",
                description: "CPU, memory, network, temperature",
                default_size: (4, 3),
                min_size: (2, 2),
                can_be_round: true,
                starts_round: false,
                settings: &[History, Cores],
                icon: "utilities-system-monitor",
            },
            Kind::Windows => &KindInfo {
                title: "Windows",
                description: "Open windows by workspace; click to focus",
                default_size: (5, 5),
                min_size: (3, 2),
                can_be_round: false,
                starts_round: false,
                settings: &[],
                icon: "preferences-system-windows",
            },
            Kind::ClaudeSessions => &KindInfo {
                title: "Claude sessions",
                description: "Running Claude Code sessions; click to focus",
                default_size: (4, 3),
                min_size: (3, 2),
                can_be_round: true,
                starts_round: false,
                settings: &[],
                icon: "utilities-terminal",
            },
            Kind::Wifi => &KindInfo {
                title: "Wi-Fi",
                description: "Networks, connect, radio on/off",
                default_size: (3, 4),
                min_size: (2, 2),
                can_be_round: true,
                starts_round: false,
                settings: &[],
                icon: "network-wireless",
            },
            Kind::Bluetooth => &KindInfo {
                title: "Bluetooth",
                description: "Devices, connect, adapter on/off",
                default_size: (3, 3),
                min_size: (2, 2),
                can_be_round: true,
                starts_round: false,
                settings: &[],
                icon: "bluetooth",
            },
            Kind::Battery => &KindInfo {
                title: "Battery & power",
                description: "Charge, power profile; device batteries and sleep blockers when bigger",
                default_size: (3, 2),
                min_size: (2, 1),
                can_be_round: true,
                starts_round: false,
                settings: &[],
                icon: "battery",
            },
            Kind::Volume => &KindInfo {
                title: "Volume",
                description: "Speaker and microphone; outputs and apps when bigger",
                default_size: (3, 3),
                min_size: (2, 1),
                can_be_round: true,
                starts_round: false,
                settings: &[],
                icon: "audio-volume-high",
            },
            Kind::Brightness => &KindInfo {
                title: "Brightness",
                description: "Screen backlight",
                default_size: (3, 1),
                min_size: (2, 1),
                can_be_round: true,
                starts_round: false,
                settings: &[],
                icon: "video-display",
            },
            Kind::Notifications => &KindInfo {
                title: "Notifications",
                description: "Notification history",
                default_size: (4, 5),
                min_size: (3, 2),
                can_be_round: false,
                starts_round: false,
                settings: &[],
                icon: "preferences-system-notifications",
            },
            Kind::Tray => &KindInfo {
                title: "Tray",
                description: "Status icons and their menus",
                default_size: (4, 1),
                min_size: (1, 1),
                can_be_round: false,
                starts_round: false,
                settings: &[],
                icon: "preferences-desktop",
            },
            Kind::Media => &KindInfo {
                title: "Media player",
                description: "Now playing (MPRIS)",
                default_size: (5, 2),
                min_size: (3, 1),
                can_be_round: false,
                starts_round: false,
                settings: &[],
                icon: "multimedia-player",
            },
            Kind::Session => &KindInfo {
                title: "Session",
                description: "Lock, suspend, log out, power off",
                default_size: (4, 1),
                min_size: (3, 1),
                can_be_round: false,
                starts_round: false,
                settings: &[Lock, Suspend, Logout, Restart, PowerOff, Confirm],
                icon: "system-shutdown",
            },
            Kind::Clipboard => &KindInfo {
                title: "Clipboard history",
                description: "Records what you copy while it's on the dashboard, in memory only",
                default_size: (4, 4),
                min_size: (3, 2),
                can_be_round: false,
                starts_round: false,
                settings: &[],
                icon: "edit-paste",
            },
            Kind::Weather => &KindInfo {
                title: "Weather",
                description: "Now and the next days (Open-Meteo)",
                default_size: (4, 2),
                min_size: (2, 1),
                can_be_round: true,
                starts_round: false,
                settings: &[Location, Units],
                icon: "weather-few-clouds",
            },
            Kind::Memory => &KindInfo {
                title: "Memory",
                description: "RAM and swap, history, biggest apps",
                default_size: (4, 3),
                min_size: (2, 2),
                can_be_round: true,
                starts_round: false,
                settings: &[History, TopApps],
                icon: "utilities-system-monitor",
            },
            Kind::Disks => &KindInfo {
                title: "Disks",
                description: "Space per filesystem, read and write",
                default_size: (4, 2),
                min_size: (3, 1),
                can_be_round: true,
                starts_round: false,
                settings: &[HideSystem],
                icon: "drive-harddisk",
            },
            Kind::Network => &KindInfo {
                title: "Network",
                description: "Download and upload, history",
                default_size: (4, 2),
                min_size: (2, 1),
                can_be_round: true,
                starts_round: false,
                settings: &[History],
                icon: "network-transmit-receive",
            },
            Kind::Gpu => &KindInfo {
                title: "GPU",
                description: "GPU load, video memory, temperature",
                default_size: (4, 2),
                min_size: (2, 1),
                can_be_round: true,
                starts_round: false,
                settings: &[History],
                icon: "video-display",
            },
            Kind::ClaudeUsage => &KindInfo {
                title: "Claude usage",
                description: "Plan, 5-hour window, tokens today",
                default_size: (4, 3),
                min_size: (3, 2),
                can_be_round: true,
                starts_round: false,
                settings: &[],
                icon: "utilities-system-monitor",
            },
            Kind::Mascot => &KindInfo {
                title: "Mascot",
                description: "Schraubi, Zolli or Verschnitti; says what you tell it",
                default_size: (4, 2),
                min_size: (3, 2),
                can_be_round: false,
                starts_round: false,
                settings: &[Character, Wildness, Interval, Command, Cowfile],
                icon: "face-smile",
            },
            Kind::KeyboardLayout => &KindInfo {
                title: "Keyboard layout",
                description: "Current layout; click to switch, bigger shows all",
                default_size: (2, 1),
                min_size: (1, 1),
                can_be_round: true,
                starts_round: false,
                settings: &[],
                icon: "input-keyboard",
            },
            Kind::ColorPicker => &KindInfo {
                title: "Color picker",
                description: "Pick a pixel anywhere; hex copied, recent colors",
                default_size: (3, 2),
                min_size: (2, 1),
                can_be_round: true,
                starts_round: false,
                settings: &[],
                icon: "color-select",
            },
            Kind::Timer => &KindInfo {
                title: "Timer",
                description: "Countdown with a ring; scroll to set, presets when bigger",
                default_size: (3, 2),
                min_size: (2, 1),
                can_be_round: true,
                starts_round: true,
                settings: &[],
                icon: "alarm-symbolic",
            },
            Kind::Stopwatch => &KindInfo {
                title: "Stopwatch",
                description: "Laps; a seconds dial when round, the lap list when taller",
                default_size: (3, 3),
                min_size: (2, 1),
                can_be_round: true,
                starts_round: true,
                settings: &[],
                icon: "chronometer",
            },
            Kind::Notes => &KindInfo {
                title: "Sticky note",
                description: "A note that keeps what you type; name it to have several",
                default_size: (3, 3),
                min_size: (2, 1),
                can_be_round: false,
                starts_round: false,
                settings: &[Note],
                icon: "accessories-text-editor",
            },
            Kind::Privacy => &KindInfo {
                title: "Privacy",
                description: "Who uses the microphone, camera or screen; stop screen sharing",
                default_size: (3, 2),
                min_size: (2, 1),
                can_be_round: true,
                starts_round: false,
                settings: &[],
                icon: "security-high",
            },
            Kind::NightLight => &KindInfo {
                title: "Night light",
                description: "Warmer screen colors for the evening",
                default_size: (3, 1),
                min_size: (2, 1),
                can_be_round: true,
                starts_round: false,
                settings: &[],
                icon: "weather-clear-night",
            },
            Kind::RemovableDrives => &KindInfo {
                title: "Removable drives",
                description: "USB drives and SD cards: mount, open, eject",
                default_size: (3, 2),
                min_size: (2, 1),
                can_be_round: false,
                starts_round: false,
                settings: &[],
                icon: "drive-removable-media",
            },
            Kind::Phone => &KindInfo {
                title: "Phone",
                description: "KDE Connect: battery, ring to find it, send files",
                default_size: (3, 2),
                min_size: (2, 1),
                can_be_round: true,
                starts_round: false,
                settings: &[],
                icon: "smartphone",
            },
            Kind::SleepTimer => &KindInfo {
                title: "Sleep timer",
                description: "Count down to suspend, hibernate or power off (like sleepy)",
                default_size: (4, 3),
                min_size: (3, 2),
                can_be_round: true,
                starts_round: false,
                settings: &[SleepWhen, SleepScript, DryRun, KeepAwake],
                icon: "system-suspend",
            },
        }
    }
}

/// The fixed facts about a kind of widget.
#[derive(Debug)]
pub struct KindInfo {
    /// The name in the widget picker, the settings sidebar and the desktop entry.
    pub title: &'static str,
    /// The line under the title in the widget picker, and the desktop entry's comment.
    pub description: &'static str,
    /// (w, h) in cells of a new rectangular instance.
    pub default_size: (i32, i32),
    /// The smallest (w, h) a rectangular instance can be resized to.
    pub min_size: (i32, i32),
    /// Whether a round version makes sense (lists don't fit in a circle).
    pub can_be_round: bool,
    /// Whether a new instance is round (at [`ROUND_SIZE`]), and one in config.kdl without a
    /// `shape` too.
    pub starts_round: bool,
    /// Its settings ([`crate::widget_settings`]), in the order the sidebar shows them.
    pub settings: &'static [crate::widget_settings::Opt],
    /// A themed icon name from the freedesktop icon naming spec (or one most themes have), for
    /// the desktop entry.
    pub icon: &'static str,
}

/// The size of a round widget that doesn't give one.
pub const ROUND_SIZE: i32 = 2;

impl KindInfo {
    /// The shape a new instance (or one in config.kdl without a `shape`) has.
    pub fn new_shape(&self) -> Shape {
        if self.starts_round {
            Shape::Round
        } else {
            Shape::Rect
        }
    }

    /// The (w, h) of a new instance.
    pub fn new_size(&self) -> (i32, i32) {
        match self.new_shape() {
            Shape::Rect => self.default_size,
            Shape::Round => (ROUND_SIZE, ROUND_SIZE),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, EnumString, AsRefStr)]
#[strum(serialize_all = "kebab-case")]
pub enum Shape {
    #[default]
    Rect,
    Round,
}

impl Shape {
    /// A size this shape can have: round widgets are square, as big as the larger side.
    fn fit(self, w: i32, h: i32) -> (i32, i32) {
        match self {
            Shape::Rect => (w, h),
            Shape::Round => (w.max(h), w.max(h)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Placed {
    pub id: u32,
    pub kind: Kind,
    pub shape: Shape,
    pub col: i32,
    pub row: i32,
    pub w: i32,
    pub h: i32,
    /// Settings from the node's children (see `widget_settings`).
    pub settings: crate::widget_settings::Values,
    /// Index of the node this widget came from in the file's `layout` block.
    node: Option<usize>,
}

impl Placed {
    pub fn rect(&self) -> Rect {
        Rect {
            col: self.col,
            row: self.row,
            w: self.w,
            h: self.h,
        }
    }

    /// The smallest size it may be resized to; round widgets go down to a single cell.
    pub fn min_size(&self) -> (i32, i32) {
        match self.shape {
            Shape::Rect => self.kind.info().min_size,
            Shape::Round => (1, 1),
        }
    }
}

pub use niri_dash_system::weather::WeatherConfig;

/// How the dashboard is drawn. `Auto` uses the GPU when EGL works and falls back to software.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, EnumString, AsRefStr)]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum RendererChoice {
    #[default]
    Auto,
    Gpu,
    Software,
}

/// The config as loaded from disk.
pub struct Loaded {
    pub layout: Layout,
    /// What is wrong with the file: unknown widgets, bad values, or why it doesn't parse.
    pub warnings: Vec<String>,
    /// The file doesn't parse. The default layout stands in, and the file is left alone.
    pub broken: bool,
    /// The file as it was read (or written).
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Layout {
    pub widgets: Vec<Placed>,
    /// The theme: a color scheme and a style, each a name (a file in colorschemes/ or styles/)
    /// or a path.
    pub colorscheme: String,
    pub style: String,
    /// Per-variable adjustments on top of the style, from the theme node's children:
    /// `theme colorscheme="…" style="…" { radius 12 }`.
    pub style_overrides: Vec<(String, KdlValue)>,
    pub weather: WeatherConfig,
    /// Takes effect on restart.
    pub renderer: RendererChoice,
    /// The `grid columns=… rows=…` the file sets, if any; without it the screen decides
    /// ([`Layout::set_screen_grid`]).
    pub grid: Option<(i32, i32)>,
    /// The grid in use: the explicit one, or what fits the screen.
    pub cols: i32,
    pub rows: i32,
    /// The parsed file, kept so saving only touches what changed.
    doc: KdlDocument,
    next_id: u32,
}

const HEADER: &str = r#"// niri-dash layout.
//
// The dashboard is a grid of square cells. `grid columns=16 rows=9` sets how many; the cells get
// as big as fits the screen. Without a `grid` line there are as many 100 px cells as fit. Every
// widget sits at a column and row (counting from 0) and spans `width` x `height` cells. Round
// widgets are square: give them `size=N` and `shape="round"`. Widgets that don't fit a smaller
// screen, or overlap one listed earlier, are hidden rather than moved. Changing the grid moves the
// widgets that no longer fit. Edit mode won't make it too small for them; a grid set too small here
// removes the ones there is no room for.
//
// Edit mode (the pencil, or right-click the background) writes changes back into this file and
// keeps your comments. Changes you save here show up right away.
//
"#;

/// The header's list of widget names, wrapped like the rest of it.
fn widget_list() -> String {
    const WIDTH: usize = 99;
    let mut out = String::new();
    let mut line = String::from("// Widgets:");
    for name in Kind::iter().map(Kind::name) {
        if line.len() + 1 + name.len() > WIDTH {
            out.push_str(&line);
            out.push('\n');
            line = String::from("//         ");
        }
        line.push(' ');
        line.push_str(name);
    }
    out.push_str(&line);
    out.push('\n');
    out
}

const DEFAULT: &str = include_str!("../assets/config.kdl");

/// The layout to start from: a browser's window can be a phone's, so it goes by the page's shape.
fn default_layout() -> &'static str {
    #[cfg(target_arch = "wasm32")]
    if crate::web::is_portrait() {
        return include_str!("../assets/config-portrait.kdl");
    }
    DEFAULT
}

/// A KDL number, integer or not.
pub(crate) fn number(v: &KdlValue) -> Option<f64> {
    match v {
        KdlValue::Integer(i) => Some(*i as f64),
        KdlValue::Float(f) => Some(*f),
        _ => None,
    }
}

/// A property as a whole number (rounded); `None` if missing, not a number or out of range.
fn int(node: &KdlNode, key: &str) -> Option<i32> {
    let n = number(node.get(key)?)?.round();
    (f64::from(i32::MIN)..=f64::from(i32::MAX))
        .contains(&n)
        .then_some(n as i32)
}

fn remove_prop(node: &mut KdlNode, key: &str) {
    node.entries_mut()
        .retain(|e| e.name().is_none_or(|n| n.value() != key));
}

/// Set a property, keeping the entry's position and spacing if it exists.
fn set_prop(node: &mut KdlNode, key: &str, value: KdlValue) {
    match node.entry_mut(key) {
        Some(e) => set_entry_value(e, value),
        None => node.push(new_entry(Some(key), value)),
    }
}

/// Change an entry's value. kdl keeps the original text of a value, so that is updated too;
/// an unchanged value keeps its text as written.
fn set_entry_value(e: &mut KdlEntry, value: KdlValue) {
    if *e.value() == value {
        return;
    }
    let repr = value_repr(&value);
    e.set_value(value);
    match e.format_mut() {
        Some(f) => f.value_repr = repr,
        None => e.autoformat(),
    }
}

/// A new entry after a space: a property `key=value`, or an argument without a key.
fn new_entry(key: Option<&str>, value: KdlValue) -> KdlEntry {
    let repr = value_repr(&value);
    let mut e = match key {
        Some(key) => KdlEntry::new_prop(key, value),
        None => KdlEntry::new(value),
    };
    e.set_format(KdlEntryFormat {
        value_repr: repr,
        leading: " ".into(),
        ..Default::default()
    });
    e
}

fn value_repr(v: &KdlValue) -> String {
    match v {
        KdlValue::String(s) => format!("{s:?}"),
        KdlValue::Integer(i) => i.to_string(),
        KdlValue::Float(f) => {
            let s = format!("{f}");
            if s.contains('.') { s } else { format!("{s}.0") }
        }
        KdlValue::Bool(b) => {
            if *b {
                "#true".into()
            } else {
                "#false".into()
            }
        }
        KdlValue::Null => "#null".into(),
    }
}

fn set_int(node: &mut KdlNode, key: &str, value: i32) {
    set_prop(node, key, KdlValue::Integer(value.into()));
}

/// Parse a KDL file; a syntax error names its line.
pub(crate) fn parse_doc(text: &str) -> Result<KdlDocument, String> {
    KdlDocument::parse(text).map_err(|e| {
        let detail = e.diagnostics.first().map(|d| {
            let line = text[..d.span.offset().min(text.len())]
                .lines()
                .count()
                .max(1);
            format!(
                "line {line}: {}",
                d.message
                    .clone()
                    .or(d.label.clone())
                    .unwrap_or_else(|| "syntax error".into())
            )
        });
        detail.unwrap_or_else(|| e.to_string())
    })
}

impl Layout {
    /// Where config.kdl is. Color schemes and styles live in the same directory.
    pub fn path() -> PathBuf {
        // For development: point at a scratch config instead of the real one.
        if let Some(p) = std::env::var_os("NIRI_DASH_CONFIG") {
            return PathBuf::from(p);
        }
        crate::files::config_home().join("niri-dash/config.kdl")
    }

    pub fn default_text() -> String {
        format!("{HEADER}{}\n{}", widget_list(), default_layout())
    }

    /// Parse a config. Unknown widgets and bad values are skipped with a warning.
    pub fn parse(text: &str) -> Result<(Self, Vec<String>), String> {
        let doc = parse_doc(text)?;
        let mut warnings = Vec::new();
        let grid = grid_of(&doc, &mut warnings);
        if doc.get("cell-size").is_some() {
            warnings.push(
                "cell-size is no longer read: set the number of cells with \
                 `grid columns=… rows=…` instead"
                    .into(),
            );
        }
        let theme = theme_of(&doc, &mut warnings);
        let mut weather = WeatherConfig::default();
        if let Some(node) = doc.get("weather") {
            weather.location = node
                .get("location")
                .and_then(|v| v.as_string())
                .map(str::to_string);
            let num = |k: &str| node.get(k).and_then(number);
            weather.coords = num("latitude").zip(num("longitude"));
            weather.fahrenheit =
                node.get("units").and_then(|v| v.as_string()) == Some("fahrenheit");
        }
        let renderer = match doc.get_arg("renderer").map(|v| v.as_string()) {
            None => RendererChoice::Auto,
            Some(Some(r)) if let Ok(r) = r.parse() => r,
            Some(_) => {
                warnings.push(r#"renderer must be "auto", "gpu" or "software""#.into());
                RendererChoice::Auto
            }
        };
        let mut widgets = Vec::new();
        let mut next_id = 1;
        if let Some(children) = doc.get("layout").and_then(|n| n.children()) {
            for (i, node) in children.nodes().iter().enumerate() {
                let name = node.name().value();
                let Some(kind) = Kind::from_str_opt(name) else {
                    warnings.push(format!("unknown widget `{name}`"));
                    continue;
                };
                let shape = match node
                    .get("shape")
                    .and_then(|v| v.as_string())
                    .map(Shape::from_str)
                {
                    None => kind.info().new_shape(),
                    Some(Ok(Shape::Round)) if !kind.info().can_be_round => {
                        warnings.push(format!("{name} can't be round"));
                        Shape::Rect
                    }
                    Some(Ok(shape)) => shape,
                    Some(Err(_)) => {
                        warnings.push(format!("{name}: unknown shape (use \"rect\" or \"round\")"));
                        Shape::Rect
                    }
                };
                let (dw, dh) = kind.info().default_size;
                let size = int(node, "size");
                let w = int(node, "width")
                    .or(size)
                    .unwrap_or(if shape == Shape::Round {
                        ROUND_SIZE
                    } else {
                        dw
                    })
                    .max(1);
                let h = int(node, "height")
                    .or(size)
                    .unwrap_or(if shape == Shape::Round {
                        ROUND_SIZE
                    } else {
                        dh
                    })
                    .max(1);
                let (w, h) = shape.fit(w, h);
                let (Some(col), Some(row)) = (int(node, "col"), int(node, "row")) else {
                    warnings.push(format!("{name}: needs col= and row="));
                    continue;
                };
                let (settings, bad) = child_values(node);
                warnings.extend(
                    bad.into_iter()
                        .map(|k| format!("{name} {{ {k} }}: expected a value")),
                );
                warnings.extend(crate::widget_settings::check(kind, &settings));
                widgets.push(Placed {
                    id: next_id,
                    kind,
                    shape,
                    col,
                    row,
                    w,
                    h,
                    settings,
                    node: Some(i),
                });
                next_id += 1;
            }
        } else {
            warnings.push("no `layout { … }` block".into());
        }
        let layout = Layout {
            widgets,
            colorscheme: theme.colorscheme,
            style: theme.style,
            style_overrides: theme.overrides,
            weather,
            renderer,
            grid,
            cols: grid.map_or(16, |g| g.0),
            rows: grid.map_or(9, |g| g.1),
            doc,
            next_id,
        };
        let hidden: Vec<String> = layout
            .widgets
            .iter()
            .filter(|w| w.col < 0 || w.row < 0)
            .map(|w| format!("{}: negative position", w.kind.name()))
            .collect();
        warnings.extend(hidden);
        Ok((layout, warnings))
    }

    /// Load the config, writing the default one first if there is none.
    pub fn load() -> Loaded {
        Self::load_from_disk(true)
    }

    /// Load the config; without one, the default config, which isn't written.
    pub fn read() -> Loaded {
        Self::load_from_disk(false)
    }

    fn load_from_disk(create: bool) -> Loaded {
        let path = Self::path();
        let text = match crate::files::read(&path) {
            Ok(t) => t,
            Err(_) => {
                let t = Self::default_text();
                if create && let Err(e) = crate::files::write_atomic(&path, &t) {
                    log::warn!("writing {}: {e}", path.display());
                }
                t
            }
        };
        let (layout, warnings, broken) = match Self::parse(&text) {
            Ok((l, w)) => (l, w, false),
            Err(e) => {
                let (l, _) = Self::parse(&Self::default_text()).expect("default config parses");
                (l, vec![format!("config.kdl: {e}")], true)
            }
        };
        Loaded {
            layout,
            warnings,
            broken,
            text,
        }
    }

    /// The config text for the current layout: the original document with positions updated,
    /// removed widgets dropped and new ones appended. Everything else is kept as written. The
    /// layout then refers to this text, so the next save works against what is on disk.
    pub fn render(&mut self) -> String {
        let mut doc = self.doc.clone();
        // The theme node is rewritten only where it changed; style overrides are its children,
        // keeping any the user formatted by hand.
        let have = theme_of(&doc, &mut Vec::new());
        if (&have.colorscheme, &have.style) != (&self.colorscheme, &self.style) {
            let node = theme_node(&mut doc);
            set_prop(node, "colorscheme", self.colorscheme.clone().into());
            set_prop(node, "style", self.style.clone().into());
        }
        if have.overrides != self.style_overrides {
            write_children(theme_node(&mut doc), &self.style_overrides);
        }
        if grid_of(&doc, &mut Vec::new()) != self.grid {
            write_grid(&mut doc, self.grid);
        }
        if doc.get("layout").is_none() {
            doc.nodes_mut().push(KdlNode::new("layout"));
        }
        let layout = doc.get_mut("layout").expect("just ensured");
        let old: Vec<KdlNode> = layout
            .children()
            .map(|c| c.nodes().to_vec())
            .unwrap_or_default();
        let mut nodes = Vec::with_capacity(old.len() + 2);
        let mut new_index = vec![None; self.widgets.len()];
        for (i, node) in old.into_iter().enumerate() {
            match self.widgets.iter().position(|w| w.node == Some(i)) {
                Some(wi) => {
                    let mut node = node;
                    write_widget(&mut node, &self.widgets[wi]);
                    new_index[wi] = Some(nodes.len());
                    nodes.push(node);
                }
                // A widget that was removed in edit mode: drop its node.
                None if Kind::from_str_opt(node.name().value()).is_some()
                    && node_was_widget(&node) => {}
                // Anything else (unknown names, broken entries) stays as the user wrote it.
                None => nodes.push(node),
            }
        }
        for (wi, w) in self.widgets.iter().enumerate() {
            if w.node.is_none() {
                let mut node = KdlNode::new(w.kind.name());
                node.set_format(KdlNodeFormat {
                    leading: "    ".into(),
                    terminator: "\n".into(),
                    ..Default::default()
                });
                write_widget(&mut node, w);
                new_index[wi] = Some(nodes.len());
                nodes.push(node);
            }
        }
        let fresh_block = layout.children().is_none();
        let children = layout.ensure_children();
        let newline_before =
            fresh_block || children.format().is_some_and(|f| f.leading.ends_with('\n'));
        one_per_line(&mut nodes, newline_before, "    ");
        *children.nodes_mut() = nodes;
        if fresh_block {
            layout.autoformat();
        }
        for (w, idx) in self.widgets.iter_mut().zip(new_index) {
            w.node = idx;
        }
        let text = doc.to_string();
        // Re-parse so later saves work against exactly what is on disk.
        if let Ok(d) = KdlDocument::parse(&text) {
            self.doc = d;
        }
        text
    }

    /// Write the config; returns what was written.
    pub fn save(&mut self) -> Option<String> {
        let path = Self::path();
        let text = self.render();
        match crate::files::write_atomic(&path, &text) {
            Ok(()) => Some(text),
            Err(e) => {
                log::warn!("saving {}: {e}", path.display());
                None
            }
        }
    }

    /// Replace the widgets with the ones from a freshly parsed file, keeping ids stable for
    /// widgets that didn't change so the UI doesn't rebuild them. If the file sets a different
    /// grid, the widgets are fitted into it ([`Layout::refit`]).
    pub fn replace(&mut self, mut other: Layout) -> Refit {
        let mut used = vec![false; self.widgets.len()];
        let mut next = self.next_id;
        for w in &mut other.widgets {
            let same = self
                .widgets
                .iter()
                .enumerate()
                .find(|(i, o)| !used[*i] && o.kind == w.kind && o.col == w.col && o.row == w.row)
                .or_else(|| {
                    self.widgets
                        .iter()
                        .enumerate()
                        .find(|(i, o)| !used[*i] && o.kind == w.kind)
                });
            match same {
                Some((i, o)) => {
                    used[i] = true;
                    w.id = o.id;
                }
                None => {
                    w.id = next;
                    next += 1;
                }
            }
        }
        other.next_id = next.max(other.widgets.iter().map(|w| w.id + 1).max().unwrap_or(1));
        let regrid = other.grid.is_some() && other.grid != self.grid;
        if other.grid.is_none() {
            (other.cols, other.rows) = (self.cols, self.rows);
        }
        *self = other;
        if regrid {
            self.refit()
        } else {
            Refit::default()
        }
    }

    /// Put the default widgets back. On an explicit grid they are fitted into it.
    pub fn reset(&mut self) -> Refit {
        let (fresh, _) = Self::parse(&Self::default_text()).expect("default config parses");
        // Keep the user's file around it (comments, the grid); only the widgets are reset.
        self.widgets.clear();
        for mut w in fresh.widgets {
            w.node = None;
            w.id = self.next_id;
            self.next_id += 1;
            self.widgets.push(w);
        }
        if self.grid.is_some() {
            self.refit()
        } else {
            Refit::default()
        }
    }

    /// Set the grid size that fits the current screen, for the automatic grid (an explicit one
    /// stays). This never moves widgets: ones that don't fit are hidden here and come back on a
    /// larger screen, so a transient size can't damage the saved layout.
    pub fn set_screen_grid(&mut self, cols: i32, rows: i32) {
        if self.grid.is_none() {
            self.cols = cols.max(1);
            self.rows = rows.max(1);
        }
    }

    /// Use a grid of `cols` x `rows` (clamped to [`COLUMNS`] and [`ROWS`]), or go back to the
    /// automatic one with `None`. An explicit grid is fitted into ([`Layout::refit`]); the
    /// automatic one only hides what doesn't fit, as [`Layout::set_screen_grid`] does.
    pub fn set_grid(&mut self, grid: Option<(i32, i32)>) -> Refit {
        let grid = grid.map(|(c, r)| {
            (
                c.clamp(*COLUMNS.start(), *COLUMNS.end()),
                r.clamp(*ROWS.start(), *ROWS.end()),
            )
        });
        self.grid = grid;
        match grid {
            Some((c, r)) => {
                (self.cols, self.rows) = (c, r);
                self.refit()
            }
            None => Refit::default(),
        }
    }

    /// Widgets shown on the current grid: inside it and not overlapping an earlier one.
    pub fn visible(&self) -> Vec<&Placed> {
        let mut shown: Vec<&Placed> = Vec::new();
        for w in &self.widgets {
            let r = w.rect();
            if self.in_bounds(&r) && shown.iter().all(|o| !o.rect().overlaps(&r)) {
                shown.push(w);
            }
        }
        shown
    }

    pub fn hidden_count(&self) -> usize {
        self.widgets.len() - self.visible().len()
    }

    /// The widget a standalone app shows: the first of that kind in the file (for its shape and
    /// settings), or a default one.
    pub fn solo_widget(&self, kind: Kind) -> Placed {
        self.widgets
            .iter()
            .find(|p| p.kind == kind)
            .cloned()
            .unwrap_or_else(|| {
                let (w, h) = kind.info().new_size();
                Placed {
                    id: 0,
                    kind,
                    shape: kind.info().new_shape(),
                    col: 0,
                    row: 0,
                    w,
                    h,
                    settings: Default::default(),
                    node: None,
                }
            })
    }

    pub fn get(&self, id: u32) -> Option<&Placed> {
        self.widgets.iter().find(|w| w.id == id)
    }

    fn get_mut(&mut self, id: u32) -> Option<&mut Placed> {
        self.widgets.iter_mut().find(|w| w.id == id)
    }

    pub fn can_place(&self, id: u32, col: i32, row: i32, w: i32, h: i32) -> bool {
        let r = Rect { col, row, w, h };
        w >= 1
            && h >= 1
            && self.in_bounds(&r)
            && self
                .visible()
                .into_iter()
                .filter(|o| o.id != id)
                .all(|o| !o.rect().overlaps(&r))
    }

    fn free_spot(&self, id: u32, w: i32, h: i32) -> Option<(i32, i32)> {
        (0..=self.rows - h)
            .flat_map(|r| (0..=self.cols - w).map(move |c| (c, r)))
            .find(|&(c, r)| self.can_place(id, c, r, w, h))
    }

    /// Round widgets are square; switching tries to keep the footprint and falls back to the
    /// largest square that fits.
    pub fn toggle_shape(&mut self, id: u32) -> bool {
        let Some(p) = self.get(id).cloned() else {
            return false;
        };
        match p.shape {
            Shape::Rect if !p.kind.info().can_be_round => false,
            Shape::Rect => {
                for d in (1..=p.w.max(p.h)).rev() {
                    if self.can_place(id, p.col, p.row, d, d) {
                        let q = self.get_mut(id).expect("checked");
                        (q.shape, q.w, q.h) = (Shape::Round, d, d);
                        return true;
                    }
                }
                false
            }
            Shape::Round => {
                // Never smaller than the rectangle's minimum: the default size (as wide as the
                // circle was, if wider), else the minimum; where it is if there's room, else
                // pushing the others aside. Without room it stays round.
                let info = p.kind.info();
                let (mw, mh) = info.min_size;
                let (dw, dh) = info.default_size;
                let fit = |w: i32, h: i32| (w.min(self.cols), h.min(self.rows));
                let mut sizes = vec![fit(dw.max(p.w), dh), fit(dw, dh), fit(mw, mh)];
                sizes.dedup();
                sizes.retain(|&(w, h)| w >= mw && h >= mh);
                let at = |w: i32, h: i32| Rect {
                    col: p.col.min(self.cols - w).max(0),
                    row: p.row.min(self.rows - h).max(0),
                    w,
                    h,
                };
                let free = sizes.iter().find(|&&(w, h)| {
                    let r = at(w, h);
                    self.can_place(id, r.col, r.row, w, h)
                });
                let target = match free {
                    Some(&(w, h)) => Some(at(w, h)),
                    None => sizes.iter().map(|&(w, h)| at(w, h)).find(|&r| {
                        let mut trial = self.clone();
                        trial.place_reflow(id, r, (0, 1))
                    }),
                };
                let Some(target) = target else {
                    return false;
                };
                if !self.place_reflow(id, target, (0, 1)) {
                    return false;
                }
                self.get_mut(id).expect("checked").shape = Shape::Rect;
                true
            }
        }
    }

    /// Where the weather comes from: the first weather widget's settings, over the top-level
    /// `weather` node (the one place to give coordinates).
    pub fn weather_config(&self, us: bool) -> WeatherConfig {
        use crate::widget_settings::{Opt, get_str};
        let mut cfg = self.weather.clone();
        let no_global = cfg == WeatherConfig::default();
        if let Some(w) = self.widgets.iter().find(|w| w.kind == Kind::Weather) {
            let set = |o: Opt| w.settings.iter().any(|(k, _)| k == o.as_ref());
            if set(Opt::Location) {
                cfg.location = Some(get_str(&w.settings, Opt::Location, us));
                cfg.coords = None;
            }
            if set(Opt::Units) || no_global {
                cfg.fahrenheit = get_str(&w.settings, Opt::Units, us) == "fahrenheit";
            }
        }
        cfg
    }

    pub fn settings_mut(&mut self, id: u32) -> Option<&mut crate::widget_settings::Values> {
        self.get_mut(id).map(|w| &mut w.settings)
    }

    pub fn remove(&mut self, id: u32) {
        self.widgets.retain(|w| w.id != id);
    }

    /// Add a widget at the first free spot, shrinking towards its minimum size if needed.
    pub fn add(&mut self, kind: Kind) -> Option<u32> {
        let id = self.next_id;
        let shape = kind.info().new_shape();
        let (dw, dh) = kind.info().new_size();
        let (mw, mh) = match shape {
            Shape::Rect => kind.info().min_size,
            Shape::Round => (1, 1),
        };
        let mut sizes = vec![(dw, dh)];
        let (mut w, mut h) = (dw, dh);
        while w > mw || h > mh {
            if shape == Shape::Round {
                (w, h) = (w - 1, h - 1);
                sizes.push((w, h));
                continue;
            }
            if w > mw && w >= h {
                w -= 1;
            } else if h > mh {
                h -= 1;
            } else {
                w -= 1;
            }
            sizes.push((w, h));
        }
        for (w, h) in sizes {
            if let Some((col, row)) = self.free_spot(id, w, h) {
                self.widgets.push(Placed {
                    id,
                    kind,
                    shape,
                    col,
                    row,
                    w,
                    h,
                    settings: Vec::new(),
                    node: None,
                });
                self.next_id += 1;
                return Some(id);
            }
        }
        None
    }
}

/// What the `theme colorscheme="…" style="…" { var value … }` node picks; defaults for what it
/// leaves out.
struct ThemeLine {
    colorscheme: String,
    style: String,
    overrides: Vec<(String, KdlValue)>,
}

fn theme_of(doc: &KdlDocument, warnings: &mut Vec<String>) -> ThemeLine {
    let node = doc.get("theme");
    let name = |key: &str| {
        node.and_then(|n| n.get(key))
            .and_then(KdlValue::as_string)
            .filter(|s| !s.is_empty())
            .unwrap_or("default")
            .to_string()
    };
    let (overrides, bad) = node.map(child_values).unwrap_or_default();
    warnings.extend(
        bad.into_iter()
            .map(|k| format!("theme {{ {k} }}: expected a value")),
    );
    ThemeLine {
        colorscheme: name("colorscheme"),
        style: name("style"),
        overrides,
    }
}

/// The document's theme node, added at the top if there is none.
fn theme_node(doc: &mut KdlDocument) -> &mut KdlNode {
    if doc.get("theme").is_none() {
        let mut n = KdlNode::new("theme");
        n.autoformat();
        doc.nodes_mut().insert(0, n);
    }
    doc.get_mut("theme").expect("just ensured")
}

/// How many columns and rows an explicit grid may have.
pub const COLUMNS: std::ops::RangeInclusive<i32> = 4..=48;
pub const ROWS: std::ops::RangeInclusive<i32> = 3..=32;
/// The cell size of the automatic grid (no `grid` node), in logical pixels. The UI has its own
/// copy (`auto-cell` in app.slint).
pub const AUTO_CELL: f32 = 100.0;

/// What the `grid columns=C rows=R` node sets, clamped to [`COLUMNS`] and [`ROWS`]; `None`
/// (the screen decides) without one.
fn grid_of(doc: &KdlDocument, warnings: &mut Vec<String>) -> Option<(i32, i32)> {
    let node = doc.get("grid")?;
    let (Some(c), Some(r)) = (int(node, "columns"), int(node, "rows")) else {
        warnings.push("grid needs columns= and rows= (whole numbers)".into());
        return None;
    };
    let mut clamp = |v: i32, range: std::ops::RangeInclusive<i32>, what: &str| {
        if !range.contains(&v) {
            warnings.push(format!(
                "grid {what} must be {} to {}",
                range.start(),
                range.end()
            ));
        }
        v.clamp(*range.start(), *range.end())
    };
    Some((clamp(c, COLUMNS, "columns"), clamp(r, ROWS, "rows")))
}

/// Write the grid into the document: update the `grid` node, add one (in place of an old
/// `cell-size` line, or after the theme), or remove it for the automatic grid. A removed node's
/// comments stay, on the node after it.
fn write_grid(doc: &mut KdlDocument, grid: Option<(i32, i32)>) {
    let nodes = doc.nodes_mut();
    let at = nodes.iter().position(|n| n.name().value() == "grid");
    match (grid, at) {
        (Some((c, r)), Some(i)) => {
            set_int(&mut nodes[i], "columns", c);
            set_int(&mut nodes[i], "rows", r);
        }
        (Some((c, r)), None) => {
            let mut node = KdlNode::new("grid");
            node.push(new_entry(Some("columns"), KdlValue::Integer(c.into())));
            node.push(new_entry(Some("rows"), KdlValue::Integer(r.into())));
            let mut format = KdlNodeFormat {
                terminator: "\n".into(),
                ..Default::default()
            };
            if let Some(i) = nodes.iter().position(|n| n.name().value() == "cell-size") {
                format.leading = nodes[i]
                    .format()
                    .map(|f| f.leading.clone())
                    .unwrap_or_default();
                node.set_format(format);
                nodes[i] = node;
            } else {
                let i = nodes
                    .iter()
                    .position(|n| n.name().value() == "theme")
                    .map_or(0, |i| i + 1);
                // After a blank line, unless it opens the file.
                format.leading = if i == 0 { String::new() } else { "\n".into() };
                node.set_format(format);
                nodes.insert(i, node);
            }
        }
        (None, Some(i)) => {
            let gone = nodes.remove(i);
            let leading = gone.format().map(|f| f.leading.clone()).unwrap_or_default();
            if !leading.trim().is_empty()
                && let Some(next) = nodes.get_mut(i)
            {
                let mut f = next.format().cloned().unwrap_or_default();
                f.leading = leading + &f.leading;
                next.set_format(f);
            }
        }
        (None, None) => {}
    }
}

/// `name value` children of a node, plus the names of children without a single value.
fn child_values(node: &KdlNode) -> (Vec<(String, KdlValue)>, Vec<String>) {
    let mut values = Vec::new();
    let mut bad = Vec::new();
    for child in node.children().map(|c| c.nodes()).unwrap_or_default() {
        let name = child.name().value().to_string();
        match child.entries() {
            [e] if e.name().is_none() => values.push((name, e.value().clone())),
            _ => bad.push(name),
        }
    }
    (values, bad)
}

/// A node's line break may have been stored with a neighbour that is now gone (removed, or all
/// replaced as by a layout reset): make sure every node still starts on its own line.
/// `newline_before`: the text before the first node already ends a line.
fn one_per_line(nodes: &mut [KdlNode], mut newline_before: bool, indent: &str) {
    for n in nodes {
        let mut f = n.format().cloned().unwrap_or_default();
        if !newline_before && !f.leading.contains('\n') {
            f.leading = format!("\n{}", f.leading.trim_start_matches([' ', '\t']));
            f.leading.push_str(indent);
            n.set_format(f.clone());
        }
        newline_before = f.terminator.contains('\n');
    }
}

/// Make the node's children exactly `items` (`name value` each), keeping the formatting and
/// comments of children that stay. No items: no children block at all.
fn write_children(node: &mut KdlNode, items: &[(String, KdlValue)]) {
    if items.is_empty() {
        node.clear_children();
        if let Some(f) = node.format_mut() {
            f.before_children = String::new();
        }
        return;
    }
    // Children are indented one level deeper than the node.
    let indent = node
        .format()
        .map(|f| f.leading.rsplit('\n').next().unwrap_or("").to_string())
        .unwrap_or_default()
        + "    ";
    let fresh_block = node.children().is_none();
    let children = node.ensure_children();
    let old: Vec<KdlNode> = children.nodes().to_vec();
    let mut nodes = Vec::new();
    for (key, value) in items {
        match old.iter().find(|n| n.name().value() == key) {
            Some(n) => {
                let mut n = n.clone();
                if let Some(e) = n.entry_mut(0) {
                    set_entry_value(e, value.clone());
                }
                nodes.push(n);
            }
            None => {
                let mut n = KdlNode::new(key.as_str());
                n.push(new_entry(None, value.clone()));
                n.set_format(KdlNodeFormat {
                    leading: indent.clone(),
                    terminator: "\n".into(),
                    ..Default::default()
                });
                nodes.push(n);
            }
        }
    }
    // (A fresh block gets its leading newline below.)
    let newline_before =
        fresh_block || children.format().is_some_and(|f| f.leading.ends_with('\n'));
    one_per_line(&mut nodes, newline_before, &indent);
    *children.nodes_mut() = nodes;
    if fresh_block {
        if let Some(f) = node.format_mut() {
            f.before_children = " ".into();
        }
        let closing = indent[..indent.len() - 4].to_string();
        if let Some(c) = node.children_mut().as_mut() {
            let mut f = c.format().cloned().unwrap_or_default();
            f.leading = "\n".into();
            f.trailing = closing;
            c.set_format(f);
        }
    }
}

/// Whether a node looks like a widget we'd have parsed (so dropping it means it was removed).
fn node_was_widget(node: &KdlNode) -> bool {
    int(node, "col").is_some() && int(node, "row").is_some()
}

fn write_widget(node: &mut KdlNode, w: &Placed) {
    if child_values(node).0 != w.settings {
        write_children(node, &w.settings);
    }
    set_int(node, "col", w.col);
    set_int(node, "row", w.row);
    match w.shape {
        Shape::Round => {
            remove_prop(node, "width");
            remove_prop(node, "height");
            set_int(node, "size", w.w);
            set_prop(node, "shape", Shape::Round.as_ref().into());
        }
        Shape::Rect => {
            remove_prop(node, "size");
            if w.kind.info().starts_round {
                // Without it, the widget would come back round.
                set_prop(node, "shape", Shape::Rect.as_ref().into());
            } else {
                remove_prop(node, "shape");
            }
            set_int(node, "width", w.w);
            set_int(node, "height", w.h);
        }
    }
}

/// A widget rectangle in grid cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub col: i32,
    pub row: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn overlaps(&self, o: &Rect) -> bool {
        self.col < o.col + o.w
            && o.col < self.col + self.w
            && self.row < o.row + o.h
            && o.row < self.row + self.h
    }

    /// Whether it lies within a grid of `cols` x `rows` cells.
    fn inside(&self, cols: i32, rows: i32) -> bool {
        self.col >= 0 && self.row >= 0 && self.col + self.w <= cols && self.row + self.h <= rows
    }
}

/// A widget the packer may move, and how small it may get.
#[derive(Debug, Clone, Copy)]
struct Item {
    id: u32,
    orig: Rect,
    min_w: i32,
    min_h: i32,
    round: bool,
}

impl Item {
    /// Sizes to try, the original first, then smaller ones by decreasing area.
    fn sizes(&self) -> Vec<(i32, i32)> {
        let mut v: Vec<(i32, i32)> = if self.round {
            (self.min_w..=self.orig.w).rev().map(|d| (d, d)).collect()
        } else {
            (self.min_w..=self.orig.w)
                .flat_map(|w| (self.min_h..=self.orig.h).map(move |h| (w, h)))
                .collect()
        };
        v.sort_by_key(|(w, h)| (-(w * h), (w - self.orig.w).abs() + (h - self.orig.h).abs()));
        v
    }
}

fn merge(others: &[(u32, Rect)], placed: Vec<(u32, Rect)>) -> Vec<(u32, Rect)> {
    let mut pos = others.to_vec();
    for (id, r) in placed {
        if let Some(p) = pos.iter_mut().find(|(i, _)| *i == id) {
            p.1 = r;
        }
    }
    pos
}

/// Which grid cells are taken; used by the packing searches.
struct Occupancy {
    cols: i32,
    rows: i32,
    cells: Vec<bool>,
}

impl Occupancy {
    fn new(cols: i32, rows: i32) -> Self {
        Self {
            cols,
            rows,
            cells: vec![false; (cols * rows).max(0) as usize],
        }
    }

    fn set(&mut self, r: &Rect, v: bool) {
        for row in r.row.max(0)..(r.row + r.h).min(self.rows) {
            for col in r.col.max(0)..(r.col + r.w).min(self.cols) {
                self.cells[(row * self.cols + col) as usize] = v;
            }
        }
    }

    fn fits(&self, r: &Rect) -> bool {
        r.inside(self.cols, self.rows)
            && (r.row..r.row + r.h).all(|row| {
                (r.col..r.col + r.w).all(|col| !self.cells[(row * self.cols + col) as usize])
            })
    }

    fn free(&self) -> i32 {
        self.cells.iter().filter(|c| !**c).count() as i32
    }

    /// The free `w` x `h` spot closest to `near` (a top-left corner, which may lie outside the
    /// grid); the first in reading order among equally close ones.
    fn nearest(&self, w: i32, h: i32, near: (i32, i32)) -> Option<Rect> {
        (0..=self.rows - h)
            .flat_map(|row| (0..=self.cols - w).map(move |col| Rect { col, row, w, h }))
            .filter(|r| self.fits(r))
            .min_by_key(|r| (r.col - near.0).abs() + (r.row - near.1).abs())
    }

    /// Place every item somewhere free, biggest first, each as close to its old spot as possible;
    /// backtracks within `budget` steps.
    fn pack(&mut self, items: &[(u32, Rect)], budget: &mut u32) -> Option<Vec<(u32, Rect)>> {
        let mut items = items.to_vec();
        items.sort_by_key(|(_, r)| -(r.w * r.h));
        let mut out = Vec::with_capacity(items.len());
        self.pack_from(&items, 0, &mut out, budget).then_some(out)
    }

    fn pack_from(
        &mut self,
        items: &[(u32, Rect)],
        i: usize,
        out: &mut Vec<(u32, Rect)>,
        budget: &mut u32,
    ) -> bool {
        let Some(&(id, orig)) = items.get(i) else {
            return true;
        };
        let rest: i32 = items[i..].iter().map(|(_, r)| r.w * r.h).sum();
        if rest > self.free() {
            return false;
        }
        let mut spots: Vec<Rect> = (0..=self.rows - orig.h)
            .flat_map(|row| (0..=self.cols - orig.w).map(move |col| Rect { col, row, ..orig }))
            .filter(|r| self.fits(r))
            .collect();
        spots.sort_by_key(|r| (r.col - orig.col).abs() + (r.row - orig.row).abs());
        for r in spots.into_iter().take(24) {
            if *budget == 0 {
                return false;
            }
            *budget -= 1;
            self.set(&r, true);
            out.push((id, r));
            if self.pack_from(items, i + 1, out, budget) {
                return true;
            }
            out.pop();
            self.set(&r, false);
        }
        false
    }

    /// Like `pack`, but items may shrink (down to their minimum) when they don't fit otherwise.
    fn pack_shrinking(
        &mut self,
        items: &[Item],
        budget: &mut u32,
        deadline: web_time::Instant,
    ) -> Option<Vec<(u32, Rect)>> {
        let mut items = items.to_vec();
        items.sort_by_key(|m| -(m.orig.w * m.orig.h));
        let mut out = Vec::with_capacity(items.len());
        self.shrink_from(&items, 0, &mut out, budget, deadline)
            .then_some(out)
    }

    fn shrink_from(
        &mut self,
        items: &[Item],
        i: usize,
        out: &mut Vec<(u32, Rect)>,
        budget: &mut u32,
        deadline: web_time::Instant,
    ) -> bool {
        let Some(m) = items.get(i) else { return true };
        let rest_min: i32 = items[i..].iter().map(|m| m.min_w * m.min_h).sum();
        if rest_min > self.free() {
            return false;
        }
        for (w, h) in m.sizes() {
            let orig = m.orig;
            let mut spots: Vec<Rect> = (0..=self.rows - h)
                .flat_map(|row| (0..=self.cols - w).map(move |col| Rect { col, row, w, h }))
                .filter(|r| self.fits(r))
                .collect();
            spots.sort_by_key(|r| (r.col - orig.col).abs() + (r.row - orig.row).abs());
            for r in spots.into_iter().take(8) {
                if *budget == 0 || web_time::Instant::now() > deadline {
                    return false;
                }
                *budget -= 1;
                self.set(&r, true);
                out.push((m.id, r));
                if self.shrink_from(items, i + 1, out, budget, deadline) {
                    return true;
                }
                out.pop();
                self.set(&r, false);
            }
        }
        false
    }

    /// Fill every free cell exactly with the items (their total area equals the free area).
    /// The first free cell in reading order must be some item's top-left corner, which keeps
    /// the search small; items closest to that cell are tried first.
    fn tile(&mut self, items: &[(u32, Rect)], budget: &mut u32) -> Option<Vec<(u32, Rect)>> {
        let mut used = vec![false; items.len()];
        let mut out = Vec::with_capacity(items.len());
        self.tile_from(items, &mut used, &mut out, budget)
            .then_some(out)
    }

    fn tile_from(
        &mut self,
        items: &[(u32, Rect)],
        used: &mut [bool],
        out: &mut Vec<(u32, Rect)>,
        budget: &mut u32,
    ) -> bool {
        let Some(first) = self.cells.iter().position(|c| !*c) else {
            return used.iter().all(|u| *u);
        };
        let (col, row) = (first as i32 % self.cols, first as i32 / self.cols);
        let mut cand: Vec<usize> = (0..items.len()).filter(|&i| !used[i]).collect();
        cand.sort_by_key(|&i| (items[i].1.col - col).abs() + (items[i].1.row - row).abs());
        let mut tried: Vec<(i32, i32)> = Vec::new();
        for i in cand {
            let (id, orig) = items[i];
            if tried.contains(&(orig.w, orig.h)) {
                continue; // same size as one that already failed here
            }
            tried.push((orig.w, orig.h));
            let r = Rect { col, row, ..orig };
            if !self.fits(&r) {
                continue;
            }
            if *budget == 0 {
                return false;
            }
            *budget -= 1;
            self.set(&r, true);
            used[i] = true;
            out.push((id, r));
            if self.tile_from(items, used, out, budget) {
                return true;
            }
            out.pop();
            used[i] = false;
            self.set(&r, false);
        }
        false
    }
}

/// New rectangles (position, and size if it had to shrink) for the widgets a placement moves.
pub type Reflow = Vec<(u32, Rect)>;

/// What fitting the widgets into a changed grid did ([`Layout::refit`]).
#[derive(Debug, Default)]
pub struct Refit {
    /// How many widgets moved (or shrank).
    pub moved: usize,
    /// The widgets there was no room for, removed from the layout, in reading order of where
    /// they were.
    pub evicted: Vec<Placed>,
}

impl Refit {
    /// Whether the layout changed, so it needs saving.
    pub fn changed(&self) -> bool {
        self.moved > 0 || !self.evicted.is_empty()
    }

    /// The toast that tells about removed widgets, if any were.
    pub fn message(&self) -> Option<String> {
        (!self.evicted.is_empty()).then(|| {
            let names: Vec<&str> = self.evicted.iter().map(|p| p.kind.info().title).collect();
            format!("Removed {}: no room", names.join(", "))
        })
    }
}

impl Layout {
    fn others(&self, id: u32) -> Vec<(u32, Rect)> {
        self.visible()
            .into_iter()
            .filter(|w| w.id != id)
            .map(|w| (w.id, w.rect()))
            .collect()
    }

    fn in_bounds(&self, r: &Rect) -> bool {
        r.inside(self.cols, self.rows)
    }

    /// Make room for widget `id` (0 for one being added) at `target`, moving others as little
    /// as possible. Modeled on Android's Launcher3 reorder: first try pushing every widget in
    /// the way in one direction (cascading into whatever it hits), for each of the four
    /// directions; then try moving each one to its nearest free spot. The solution that moves
    /// the fewest cells wins; `prefer` (a direction, e.g. the drag direction) breaks ties.
    pub fn reflow(&self, id: u32, target: Rect, prefer: (i32, i32)) -> Option<Reflow> {
        if !self.in_bounds(&target) {
            return None;
        }
        let others = self.others(id);
        if others.iter().all(|(_, r)| !r.overlaps(&target)) {
            return Some(Vec::new());
        }
        let mut dirs = [(0, 1), (1, 0), (-1, 0), (0, -1)];
        dirs.sort_by_key(|d| if *d == prefer { 0 } else { 1 });
        let mut candidates: Vec<Vec<(u32, Rect)>> = dirs
            .iter()
            .filter_map(|d| self.push(&others, target, *d))
            .collect();
        candidates.extend(self.nearest_free(&others, target));
        if candidates.is_empty() {
            // Crowded grid: nothing slides into place; search for a proper re-pack.
            candidates.extend(self.repack(&others, target));
        }
        let orig = |id: u32| others.iter().find(|(o, _)| *o == id).map(|(_, r)| *r);
        // Cells moved, plus a stiff price for every cell a widget had to give up.
        let cost = |sol: &Vec<(u32, Rect)>| -> i32 {
            sol.iter()
                .map(|(i, r)| {
                    orig(*i)
                        .map(|o| {
                            (o.col - r.col).abs()
                                + (o.row - r.row).abs()
                                + 4 * (o.w * o.h - r.w * r.h)
                        })
                        .unwrap_or(0)
                })
                .sum()
        };
        // min_by_key keeps the first of equal costs, i.e. the preferred direction.
        let best = candidates.into_iter().min_by_key(cost)?;
        Some(
            best.into_iter()
                .filter(|(i, r)| orig(*i) != Some(*r))
                .collect(),
        )
    }

    /// The general case, for widgets of any size and full grids: free the widgets in the way,
    /// and if they can't be re-placed, also the next-nearest ones. Each set is re-packed by a
    /// bounded search that keeps widgets close to where they were; on a full grid it first tries
    /// an exact re-tiling, then lets displaced widgets shrink towards their minimum size.
    fn repack(&self, others: &[(u32, Rect)], target: Rect) -> Option<Vec<(u32, Rect)>> {
        let deadline = web_time::Instant::now() + std::time::Duration::from_millis(25);
        let center = |r: &Rect| (2 * r.col + r.w, 2 * r.row + r.h);
        let tc = center(&target);
        let mut order: Vec<usize> = (0..others.len()).collect();
        order.sort_by_key(|&i| {
            let r = &others[i].1;
            let c = center(r);
            (
                !r.overlaps(&target),
                (c.0 - tc.0).abs() + (c.1 - tc.1).abs(),
            )
        });
        let first = others
            .iter()
            .filter(|(_, r)| r.overlaps(&target))
            .count()
            .max(1);
        let mut ks: Vec<usize> = [0, 1, 2, 4, 7, 11]
            .iter()
            .map(|d| first + d)
            .filter(|k| *k < others.len())
            .collect();
        ks.push(others.len());
        let total = self.cols * self.rows;
        for k in ks {
            if web_time::Instant::now() > deadline {
                break;
            }
            let movable: Vec<Item> = order[..k]
                .iter()
                .map(|&i| {
                    let (id, r) = others[i];
                    let (mw, mh) = self.get(id).map_or((r.w, r.h), Placed::min_size);
                    Item {
                        id,
                        orig: r,
                        min_w: mw.min(r.w),
                        min_h: mh.min(r.h),
                        round: self.get(id).is_some_and(|p| p.shape == Shape::Round),
                    }
                })
                .collect();
            let blocked: Vec<Rect> = std::iter::once(target)
                .chain(order[k..].iter().map(|&i| others[i].1))
                .collect();
            let free = total - blocked.iter().map(|r| r.w * r.h).sum::<i32>();
            let need: i32 = movable.iter().map(|m| m.orig.w * m.orig.h).sum();
            let need_min: i32 = movable.iter().map(|m| m.min_w * m.min_h).sum();
            if free < need_min {
                continue;
            }
            let mut occ = Occupancy::new(self.cols, self.rows);
            for b in &blocked {
                occ.set(b, true);
            }
            let rects: Vec<(u32, Rect)> = movable.iter().map(|m| (m.id, m.orig)).collect();
            let mut budget = 1500;
            if free == need
                && let Some(placed) = occ.tile(&rects, &mut budget)
            {
                return Some(merge(others, placed));
            }
            if free >= need {
                let mut budget = 2000;
                if let Some(placed) = occ.pack(&rects, &mut budget) {
                    return Some(merge(others, placed));
                }
            }
            let mut budget = 3000;
            if let Some(placed) = occ.pack_shrinking(&movable, &mut budget, deadline) {
                return Some(self.regrow(target, merge(others, placed)));
            }
        }
        None
    }

    /// After shrinking, let widgets grow back towards their original size into free space
    /// around them (never into anything else).
    fn regrow(&self, target: Rect, mut pos: Vec<(u32, Rect)>) -> Vec<(u32, Rect)> {
        loop {
            let mut grew = false;
            for i in 0..pos.len() {
                let (id, cur) = pos[i];
                let Some(p) = self.get(id) else { continue };
                if cur.w * cur.h >= p.w * p.h {
                    continue;
                }
                let mut occ = Occupancy::new(self.cols, self.rows);
                occ.set(&target, true);
                for (j, (_, r)) in pos.iter().enumerate() {
                    if j != i {
                        occ.set(r, true);
                    }
                }
                let item = Item {
                    id,
                    orig: Rect {
                        w: p.w,
                        h: p.h,
                        ..cur
                    },
                    min_w: cur.w,
                    min_h: cur.h,
                    round: p.shape == Shape::Round,
                };
                // Bigger sizes first; any placement that still covers where it is now.
                let better = item
                    .sizes()
                    .into_iter()
                    .filter(|(w, h)| w * h > cur.w * cur.h)
                    .find_map(|(w, h)| {
                        ((cur.row + cur.h - h).max(0)..=cur.row)
                            .flat_map(|row| {
                                ((cur.col + cur.w - w).max(0)..=cur.col).map(move |col| Rect {
                                    col,
                                    row,
                                    w,
                                    h,
                                })
                            })
                            .find(|r| occ.fits(r))
                    });
                if let Some(r) = better {
                    pos[i].1 = r;
                    grew = true;
                }
            }
            if !grew {
                return pos;
            }
        }
    }

    /// Push everything overlapping `target` in direction `d`; pushed widgets push what they hit.
    fn push(
        &self,
        others: &[(u32, Rect)],
        target: Rect,
        d: (i32, i32),
    ) -> Option<Vec<(u32, Rect)>> {
        let mut pos: Vec<(u32, Rect)> = others.to_vec();
        let mut fixed: Vec<Rect> = vec![target];
        let mut settled = vec![false; pos.len()];
        let mut queue = vec![target];
        while let Some(obstacle) = queue.pop() {
            for i in 0..pos.len() {
                if settled[i] || !pos[i].1.overlaps(&obstacle) {
                    continue;
                }
                let mut r = pos[i].1;
                // Step past every fixed rect it overlaps, along the push direction.
                while let Some(f) = fixed.iter().find(|f| f.overlaps(&r)) {
                    match d {
                        (0, 1) => r.row = f.row + f.h,
                        (0, -1) => r.row = f.row - r.h,
                        (1, 0) => r.col = f.col + f.w,
                        _ => r.col = f.col - r.w,
                    }
                    if !self.in_bounds(&r) {
                        return None;
                    }
                }
                pos[i].1 = r;
                settled[i] = true;
                fixed.push(r);
                queue.push(r);
            }
        }
        Some(pos)
    }

    /// Move each widget overlapping `target` to the free spot closest to where it was.
    fn nearest_free(&self, others: &[(u32, Rect)], target: Rect) -> Option<Vec<(u32, Rect)>> {
        let mut pos: Vec<(u32, Rect)> = others.to_vec();
        let mut displaced: Vec<usize> = (0..pos.len())
            .filter(|&i| pos[i].1.overlaps(&target))
            .collect();
        // Big ones first: they are the hardest to fit.
        displaced.sort_by_key(|&i| -(pos[i].1.w * pos[i].1.h));
        // Everything that stays put, plus the newcomer, plus displaced widgets once re-placed.
        let mut blocked: Vec<Rect> = std::iter::once(target)
            .chain(
                pos.iter()
                    .enumerate()
                    .filter(|(j, _)| !displaced.contains(j))
                    .map(|(_, (_, r))| *r),
            )
            .collect();
        for &i in &displaced {
            let orig = pos[i].1;
            let mut best: Option<(i32, Rect)> = None;
            for row in 0..=self.rows - orig.h {
                for col in 0..=self.cols - orig.w {
                    let r = Rect { col, row, ..orig };
                    if blocked.iter().any(|b| b.overlaps(&r)) {
                        continue;
                    }
                    let dist = (col - orig.col).abs() + (row - orig.row).abs();
                    if best.is_none_or(|(d, _)| dist < d) {
                        best = Some((dist, r));
                    }
                }
            }
            let r = best?.1;
            pos[i].1 = r;
            blocked.push(r);
        }
        Some(pos)
    }

    /// Make every widget show on the current grid, after it changed. Widgets that do show stay
    /// where they are. The others (outside the grid, or under one listed earlier) move into the
    /// free space: first all together by the bounded packing search (which may shrink them
    /// towards their minimum size, then lets them grow back); if that finds nothing, one by one
    /// in reading order of where they were, each to the free spot nearest its old one at the
    /// biggest size that fits anywhere. Widgets that don't fit even at their minimum size are
    /// removed.
    pub fn refit(&mut self) -> Refit {
        let shown: Vec<u32> = self.visible().iter().map(|w| w.id).collect();
        let mut occ = Occupancy::new(self.cols, self.rows);
        let mut misfits = Vec::new();
        for p in &self.widgets {
            if shown.contains(&p.id) {
                occ.set(&p.rect(), true);
            } else {
                let (mw, mh) = p.min_size();
                misfits.push(Item {
                    id: p.id,
                    orig: p.rect(),
                    min_w: mw.min(p.w),
                    min_h: mh.min(p.h),
                    round: p.shape == Shape::Round,
                });
            }
        }
        if misfits.is_empty() {
            return Refit::default();
        }
        misfits.sort_by_key(|m| (m.orig.row, m.orig.col));
        let deadline = web_time::Instant::now() + std::time::Duration::from_millis(50);
        let mut budget = 5000;
        let mut evicted = Vec::new();
        let moves = match occ.pack_shrinking(&misfits, &mut budget, deadline) {
            Some(placed) => {
                let all = self
                    .widgets
                    .iter()
                    .map(|p| (p.id, p.rect()))
                    .collect::<Vec<_>>();
                let none = Rect {
                    col: 0,
                    row: 0,
                    w: 0,
                    h: 0,
                };
                let mut pos = self.regrow(none, merge(&all, placed));
                pos.retain(|(id, _)| !shown.contains(id));
                pos
            }
            None => {
                let mut moves = Vec::new();
                for m in &misfits {
                    let near = (m.orig.col, m.orig.row);
                    match m
                        .sizes()
                        .into_iter()
                        .find_map(|(w, h)| occ.nearest(w, h, near))
                    {
                        Some(r) => {
                            occ.set(&r, true);
                            moves.push((m.id, r));
                        }
                        None => evicted.push(m.id),
                    }
                }
                moves
            }
        };
        self.apply_reflow(&moves);
        let evicted: Vec<Placed> = evicted
            .iter()
            .filter_map(|id| self.get(*id).cloned())
            .collect();
        self.widgets
            .retain(|w| evicted.iter().all(|e| e.id != w.id));
        Refit {
            moved: moves.len(),
            evicted,
        }
    }

    /// Apply a reflow computed by [`Layout::reflow`].
    pub fn apply_reflow(&mut self, moves: &Reflow) {
        for &(id, r) in moves {
            if let Some(p) = self.get_mut(id) {
                (p.col, p.row, p.w, p.h) = (r.col, r.row, r.w, r.h);
            }
        }
    }

    /// Move a widget, pushing others aside.
    pub fn move_reflow(&mut self, id: u32, col: i32, row: i32, prefer: (i32, i32)) -> bool {
        let Some(p) = self.get(id) else { return false };
        let target = Rect {
            col,
            row,
            ..p.rect()
        };
        let Some(moves) = self.reflow(id, target, prefer) else {
            return false;
        };
        self.apply_reflow(&moves);
        let p = self.get_mut(id).expect("checked");
        (p.col, p.row) = (col, row);
        true
    }

    /// Move and resize a widget at once (from the keyboard), pushing others aside; false if
    /// there is no room.
    pub fn place_reflow(&mut self, id: u32, target: Rect, prefer: (i32, i32)) -> bool {
        let Some(moves) = self.reflow(id, target, prefer) else {
            return false;
        };
        self.apply_reflow(&moves);
        let Some(p) = self.get_mut(id) else {
            return false;
        };
        (p.col, p.row, p.w, p.h) = (target.col, target.row, target.w, target.h);
        true
    }

    /// What a resize of widget `id` to `target` comes to: where it ends up (at least its minimum
    /// size, square if round, with the sides the resize left alone staying put) and what makes
    /// room, pushed the way the widget grew. None if there is no room.
    pub fn plan_resize(&self, id: u32, target: Rect) -> Option<(Rect, Reflow)> {
        let p = self.get(id)?;
        let from = p.rect();
        let (w, h) = self.clamp_size(p, target.w, target.h);
        let to = Rect {
            col: if target.col == from.col {
                from.col
            } else {
                target.col + target.w - w
            },
            row: if target.row == from.row {
                from.row
            } else {
                target.row + target.h - h
            },
            w,
            h,
        };
        let prefer = if to.row + to.h > from.row + from.h {
            (0, 1)
        } else if to.row < from.row {
            (0, -1)
        } else if to.col < from.col {
            (-1, 0)
        } else {
            (1, 0)
        };
        Some((to, self.reflow(id, to, prefer)?))
    }

    /// Resize a widget to `target`, pushing others aside; false if there is no room.
    pub fn resize_reflow(&mut self, id: u32, target: Rect) -> bool {
        let Some((to, moves)) = self.plan_resize(id, target) else {
            return false;
        };
        self.apply_reflow(&moves);
        let p = self.get_mut(id).expect("checked");
        (p.col, p.row, p.w, p.h) = (to.col, to.row, to.w, to.h);
        true
    }

    /// The size a resize of `p` to `w` x `h` gives: at least its minimum, square if round.
    pub fn clamp_size(&self, p: &Placed, w: i32, h: i32) -> (i32, i32) {
        let (min_w, min_h) = p.min_size();
        p.shape.fit(w.max(min_w), h.max(min_h))
    }

    /// Add a widget of its default size at a spot, pushing others aside.
    pub fn add_at(&mut self, kind: Kind, col: i32, row: i32) -> Option<u32> {
        let (w, h) = kind.info().new_size();
        let target = Rect { col, row, w, h };
        let moves = self.reflow(0, target, (0, 1))?;
        self.apply_reflow(&moves);
        let id = self.next_id;
        self.next_id += 1;
        self.widgets.push(Placed {
            id,
            kind,
            shape: kind.info().new_shape(),
            col,
            row,
            w,
            h,
            settings: Vec::new(),
            node: None,
        });
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Layout {
        let (mut l, warnings) = Layout::parse(&Layout::default_text()).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        l.set_screen_grid(16, 9);
        l
    }

    fn id_of(l: &Layout, kind: Kind) -> u32 {
        l.widgets.iter().find(|w| w.kind == kind).unwrap().id
    }

    #[test]
    fn default_layout_has_no_overlaps() {
        let l = grid();
        assert_eq!(l.visible().len(), l.widgets.len());
        for a in l.visible() {
            assert!(l.can_place(a.id, a.col, a.row, a.w, a.h), "{a:?} overlaps");
        }
    }

    #[test]
    fn can_place_rejects_overlap_and_accepts_free() {
        let mut l = grid();
        let clock = id_of(&l, Kind::Clock);
        assert!(!l.can_place(clock, 4, 0, 4, 2), "onto windows widget");
        l.remove(id_of(&l, Kind::Windows));
        assert!(l.can_place(clock, 4, 0, 4, 2));
        let before = l.widgets.iter().map(Placed::rect).collect::<Vec<_>>();
        assert!(l.move_reflow(clock, 4, 0, (0, 1)));
        let moved: Vec<_> = l
            .widgets
            .iter()
            .filter(|w| w.id != clock)
            .map(Placed::rect)
            .filter(|r| !before.contains(r))
            .collect();
        assert!(moved.is_empty(), "nothing else moves: {moved:?}");
    }

    #[test]
    fn round_is_square() {
        let mut l = grid();
        let clock = id_of(&l, Kind::Clock);
        l.remove(id_of(&l, Kind::Windows));
        assert!(l.toggle_shape(clock));
        let p = l.get(clock).unwrap();
        assert_eq!((p.shape, p.w, p.h), (Shape::Round, 2, 2));
        assert_eq!(l.clamp_size(p, 3, 2), (3, 3));
        assert_eq!(
            l.clamp_size(p, 0, 0),
            (1, 1),
            "round ones go down to a cell"
        );
        assert!(
            !l.can_place(clock, p.col, p.row, 3, 3),
            "3x3 would overlap the weather widget"
        );
        l.remove(id_of(&l, Kind::Weather));
        let target = Rect {
            w: 3,
            h: 2,
            ..l.get(clock).unwrap().rect()
        };
        assert!(l.resize_reflow(clock, target));
        let p = l.get(clock).unwrap();
        assert_eq!((p.w, p.h), (3, 3));
    }

    #[test]
    fn resizing_from_the_left_and_top_keeps_the_far_sides() {
        let mut l = Layout::parse(
            "grid columns=16 rows=9\nlayout {\n    clock col=4 row=3 width=4 height=3\n}\n",
        )
        .unwrap()
        .0;
        l.set_screen_grid(16, 9);
        let clock = id_of(&l, Kind::Clock);
        let (min_w, min_h) = l.get(clock).unwrap().min_size();
        // Out to the top left: the bottom right corner stays.
        let out = Rect {
            col: 2,
            row: 1,
            w: 6,
            h: 5,
        };
        assert!(l.resize_reflow(clock, out));
        assert_eq!(l.get(clock).unwrap().rect(), out);
        // Far below its minimum: it stops there, the bottom right corner still in place.
        let tiny = Rect {
            col: 7,
            row: 5,
            w: 1,
            h: 1,
        };
        assert!(l.resize_reflow(clock, tiny));
        let r = l.get(clock).unwrap().rect();
        assert_eq!((r.w, r.h), (min_w, min_h));
        assert_eq!((r.col + r.w, r.row + r.h), (8, 6));
        // Past the grid's edge: refused.
        let past = Rect { col: -1, ..r };
        assert!(!l.resize_reflow(clock, past));
    }

    #[test]
    fn smaller_grid_hides_without_moving() {
        let mut l = grid();
        let before: Vec<_> = l
            .widgets
            .iter()
            .map(|w| (w.id, w.col, w.row, w.w, w.h))
            .collect();
        l.set_screen_grid(4, 3);
        assert!(l.hidden_count() > 0);
        l.set_screen_grid(16, 9);
        let after: Vec<_> = l
            .widgets
            .iter()
            .map(|w| (w.id, w.col, w.row, w.w, w.h))
            .collect();
        assert_eq!(before, after);
        assert_eq!(l.hidden_count(), 0);
    }

    #[test]
    fn overlapping_widget_from_file_is_hidden() {
        let (mut l, _) = Layout::parse("layout {\n clock col=0 row=0 width=4 height=2\n calendar col=1 row=1 width=4 height=4\n}").unwrap();
        l.set_screen_grid(16, 9);
        assert_eq!(l.visible().len(), 1);
        assert_eq!(l.hidden_count(), 1);
    }

    #[test]
    fn round_back_to_rect_keeps_the_minimum() {
        // A calendar shrunk to one round cell comes back at least 3x3, pushing the clock aside.
        let text = "grid columns=8 rows=6\nlayout {\n    calendar col=0 row=0 size=1 shape=\"round\"\n    clock col=1 row=0 width=4 height=2\n}\n";
        let (mut l, _) = Layout::parse(text).unwrap();
        let cal = id_of(&l, Kind::Calendar);
        assert!(l.toggle_shape(cal));
        let p = l.get(cal).unwrap();
        assert_eq!(p.shape, Shape::Rect);
        assert!(p.w >= 3 && p.h >= 3, "{}x{}", p.w, p.h);
        assert_eq!(l.hidden_count(), 0);
        assert!(l.visible().len() == 2);
    }

    #[test]
    fn round_stays_round_without_room() {
        // A 4x3 grid full of other widgets: no room for a 3x3 calendar.
        let text = "grid columns=4 rows=3\nlayout {\n    calendar col=0 row=0 size=1 shape=\"round\"\n    clock col=1 row=0 width=3 height=2\n    session col=0 row=2 width=4 height=1\n    brightness col=0 row=1 size=1 shape=\"round\"\n}\n";
        let (mut l, _) = Layout::parse(text).unwrap();
        let cal = id_of(&l, Kind::Calendar);
        assert!(!l.toggle_shape(cal));
        let p = l.get(cal).unwrap();
        assert_eq!((p.shape, p.w, p.h), (Shape::Round, 1, 1));
    }

    #[test]
    fn edits_keep_comments_and_formatting() {
        let text = "// my dash\ngrid columns=16 rows=9 // laptop\n\nlayout {\n    // the clock\n    clock    col=0 row=0 width=4 height=2\n    weird-thing col=1\n    calendar col=0 row=2 width=4 height=4 // month\n    tray col=5 row=0 width=2 height=1\n}\n";
        let (mut l, warnings) = Layout::parse(text).unwrap();
        assert_eq!(warnings, vec!["unknown widget `weird-thing`"]);
        assert_eq!(l.grid, Some((16, 9)));
        let clock = id_of(&l, Kind::Clock);
        assert!(l.can_place(clock, 6, 3, 4, 2));
        assert!(l.move_reflow(clock, 6, 3, (0, 1)));
        l.remove(id_of(&l, Kind::Tray));
        let cal = id_of(&l, Kind::Calendar);
        assert!(l.toggle_shape(cal));
        l.add(Kind::Session).unwrap();
        let out = l.render();
        assert!(out.contains("// my dash"), "{out}");
        assert!(out.contains("grid columns=16 rows=9 // laptop"), "{out}");
        assert!(out.contains("// the clock"), "{out}");
        assert!(
            out.contains("clock    col=6 row=3 width=4 height=2"),
            "{out}"
        );
        assert!(out.contains("weird-thing col=1"), "{out}");
        assert!(out.contains("// month"), "{out}");
        assert!(!out.contains("tray"), "{out}");
        assert!(
            out.contains("calendar col=0 row=2 size=4 shape=round")
                || out.contains("calendar col=0 row=2 size=4 shape=\"round\""),
            "{out}"
        );
        assert!(out.contains("session col="), "{out}");
        // And it parses back to the same widgets.
        let (back, _) = Layout::parse(&out).unwrap();
        assert_eq!(back.widgets.len(), 3);
        // A second save with no changes is byte-identical.
        let mut again = back.clone();
        assert_eq!(again.render(), out);
    }

    #[test]
    fn widget_names_and_round_size_parse() {
        for kind in Kind::iter() {
            assert_eq!(Kind::from_str_opt(kind.name()), Some(kind));
        }
        // The names are what users' config.kdl files say: renaming a variant must not change one.
        let names: Vec<&str> = Kind::iter().map(Kind::name).collect();
        assert_eq!(
            names,
            [
                "clock",
                "calendar",
                "system-monitor",
                "windows",
                "claude-sessions",
                "wifi",
                "bluetooth",
                "battery",
                "volume",
                "brightness",
                "notifications",
                "tray",
                "media",
                "session",
                "clipboard",
                "weather",
                "memory",
                "disks",
                "network",
                "gpu",
                "claude-usage",
                "mascot",
                "keyboard-layout",
                "color-picker",
                "timer",
                "stopwatch",
                "notes",
                "privacy",
                "night-light",
                "removable-drives",
                "phone",
                "sleep-timer",
            ]
        );
        assert_eq!(Kind::from_str_opt("perf"), None);
        let (l, warnings) = Layout::parse("layout {\n  system-monitor col=0 row=0 width=4 height=3\n  volume col=4 row=0 size=2 shape=\"round\"\n}").unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(l.widgets[0].kind, Kind::SystemMonitor);
        assert_eq!(
            (
                l.widgets[1].kind,
                l.widgets[1].shape,
                l.widgets[1].w,
                l.widgets[1].h
            ),
            (Kind::Volume, Shape::Round, 2, 2)
        );
    }

    /// A clipboard widget is what turns the recording of the clipboard on, so that's the user's
    /// choice to make, not a default.
    #[test]
    fn default_layout_has_no_clipboard_history() {
        assert!(!grid().widgets.iter().any(|w| w.kind == Kind::Clipboard));
    }

    #[test]
    fn default_header_lists_every_widget() {
        let text = Layout::default_text();
        let list: Vec<&str> = text
            .lines()
            .skip_while(|l| !l.starts_with("// Widgets:"))
            .take_while(|l| l.starts_with("//") && *l != "//")
            .collect();
        assert!(list.iter().all(|l| l.len() <= 99), "{list:?}");
        let names: Vec<&str> = list
            .iter()
            .flat_map(|l| {
                l.trim_start_matches("// Widgets:")
                    .trim_start_matches("//")
                    .split_whitespace()
            })
            .collect();
        let all: Vec<&str> = Kind::iter().map(Kind::name).collect();
        assert_eq!(names, all);
    }

    #[test]
    fn style_overrides_without_a_theme_node() {
        let (mut l, _) = Layout::parse("layout {\n}\n").unwrap();
        l.style_overrides = vec![("radius".into(), KdlValue::Integer(8))];
        let out = l.render();
        let (back, w) = Layout::parse(&out).unwrap();
        assert!(w.is_empty(), "{w:?}\n{out}");
        assert_eq!(back.style_overrides, l.style_overrides, "{out}");
    }

    #[test]
    fn syntax_error_reports_line() {
        let err = Layout::parse("layout {\n  clock col=0 row=0\n  calendar col=\"\n}").unwrap_err();
        assert!(err.starts_with("line "), "{err}");
    }

    #[test]
    fn reflow_pushes_aside() {
        let mut l = grid();
        // The default layout fills 16x9 exactly; leave room to push into.
        l.set_screen_grid(16, 12);
        let clock = id_of(&l, Kind::Clock);
        // Onto the windows widget: the others make room.
        assert!(l.move_reflow(clock, 4, 0, (1, 0)));
        assert_eq!(
            l.visible().len(),
            l.widgets.len(),
            "nothing pushed off screen"
        );
        for a in l.visible() {
            assert!(l.can_place(a.id, a.col, a.row, a.w, a.h), "{a:?} overlaps");
        }
    }

    #[test]
    fn reflow_into_a_free_gap_moves_nothing() {
        let mut l = grid();
        l.remove(id_of(&l, Kind::Windows));
        let moves = l
            .reflow(
                0,
                Rect {
                    col: 4,
                    row: 0,
                    w: 5,
                    h: 3,
                },
                (0, 1),
            )
            .unwrap();
        assert!(moves.is_empty());
    }

    #[test]
    fn reflow_fails_when_full() {
        let l = grid();
        // The grid is completely full; a 16x9 newcomer can't fit anywhere.
        assert!(
            l.reflow(
                0,
                Rect {
                    col: 0,
                    row: 0,
                    w: 16,
                    h: 9
                },
                (0, 1)
            )
            .is_none()
        );
    }

    #[test]
    fn timers_start_round() {
        for kind in Kind::iter().filter(|k| k.info().starts_round) {
            assert!(kind.info().can_be_round, "{kind:?}");
        }
        let (mut l, _) = Layout::parse("layout {\n}\n").unwrap();
        l.set_screen_grid(16, 9);
        let timer = l.add(Kind::Timer).unwrap();
        let p = l.get(timer).unwrap();
        assert_eq!((p.shape, p.w, p.h), (Shape::Round, ROUND_SIZE, ROUND_SIZE));
        let sw = l.add_at(Kind::Stopwatch, 4, 0).unwrap();
        assert_eq!(l.get(sw).unwrap().shape, Shape::Round);
        // Made rectangular, a stopwatch says so in the file, or it would come back round.
        assert!(l.toggle_shape(sw));
        let out = l.render();
        assert!(
            out.lines()
                .any(|l| l.contains("stopwatch") && l.contains("shape=\"rect\"")),
            "{out}"
        );
        let (l, warnings) = Layout::parse(&out).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let shapes: Vec<_> = l.widgets.iter().map(|w| (w.kind, w.shape)).collect();
        assert_eq!(
            shapes,
            [(Kind::Timer, Shape::Round), (Kind::Stopwatch, Shape::Rect)]
        );
        // Without a shape, a timer in the file is round.
        let (l, _) = Layout::parse("layout {\n  timer col=0 row=0\n}\n").unwrap();
        assert_eq!(
            (l.widgets[0].shape, l.widgets[0].w),
            (Shape::Round, ROUND_SIZE)
        );
    }

    #[test]
    fn add_at_makes_room() {
        let mut l = grid();
        l.remove(id_of(&l, Kind::Media));
        l.remove(id_of(&l, Kind::Tray));
        // Without media and tray, 5x3 cells are free at (4,6): room for a 4x2 weather.
        let id = l.add_at(Kind::Weather, 4, 6).unwrap();
        assert!(l.get(id).is_some());
        for a in l.visible() {
            assert!(l.can_place(a.id, a.col, a.row, a.w, a.h), "{a:?} overlaps");
        }
    }

    #[test]
    fn theme_is_written_back() {
        let (mut l, _) =
            Layout::parse("// hi\ntheme colorscheme=\"light\"\nlayout {\n}\n").unwrap();
        assert_eq!(
            (l.colorscheme.as_str(), l.style.as_str()),
            ("light", "default")
        );
        l.style = "compact".into();
        let out = l.render();
        assert!(
            out.contains("theme colorscheme=\"light\" style=\"compact\""),
            "{out}"
        );
        assert!(out.contains("// hi"), "{out}");
        let (back, _) = Layout::parse(&out).unwrap();
        assert_eq!(
            (back.colorscheme.as_str(), back.style.as_str()),
            ("light", "compact")
        );
    }

    #[test]
    fn full_grid_moves_any_size() {
        // The default layout fills the 16x9 grid exactly: only a re-pack can make room.
        let mut l = grid();
        assert_eq!(l.widgets.iter().map(|w| w.w * w.h).sum::<i32>(), 16 * 9);
        let cases = [
            (Kind::Clock, 4, 0),
            (Kind::Windows, 0, 0),
            (Kind::Wifi, 0, 6),
            (Kind::Calendar, 9, 3),
            (Kind::Volume, 0, 0),
        ];
        for (kind, col, row) in cases {
            let id = id_of(&l, kind);
            let t = web_time::Instant::now();
            assert!(
                l.move_reflow(id, col, row, (0, 1)),
                "{kind:?} to {col},{row}"
            );
            assert!(
                t.elapsed() < std::time::Duration::from_millis(500),
                "{kind:?} took {:?}",
                t.elapsed()
            );
            assert_eq!(
                l.visible().len(),
                l.widgets.len(),
                "all still on screen after {kind:?}"
            );
            for a in l.visible() {
                assert!(
                    l.can_place(a.id, a.col, a.row, a.w, a.h),
                    "{a:?} overlaps after {kind:?}"
                );
            }
            let p = l.get(id).unwrap();
            assert_eq!((p.col, p.row), (col, row));
        }
    }

    #[test]
    fn tiling_search_fills_exactly() {
        let mut occ = Occupancy::new(4, 2);
        let items = [
            (
                1,
                Rect {
                    col: 0,
                    row: 0,
                    w: 2,
                    h: 2,
                },
            ),
            (
                2,
                Rect {
                    col: 3,
                    row: 0,
                    w: 1,
                    h: 2,
                },
            ),
            (
                3,
                Rect {
                    col: 0,
                    row: 0,
                    w: 1,
                    h: 2,
                },
            ),
        ];
        let mut budget = 100;
        let placed = occ.tile(&items, &mut budget).unwrap();
        assert_eq!(placed.len(), 3);
        assert_eq!(occ.free(), 0);
    }

    /// Experiment (run with --ignored --nocapture): every widget to every position in the full
    /// default grid; prints how often a re-flow is found and how long it takes.
    #[test]
    #[ignore]
    fn reflow_coverage() {
        let base = grid();
        let (mut ok, mut total, mut worst) = (0, 0, std::time::Duration::ZERO);
        let (mut shrunk, mut lost) = (0, 0);
        let mut fails = Vec::new();
        for w in base.widgets.clone() {
            for row in 0..=base.rows - w.h {
                for col in 0..=base.cols - w.w {
                    let t = web_time::Instant::now();
                    let sol = base.reflow(
                        w.id,
                        Rect {
                            col,
                            row,
                            w: w.w,
                            h: w.h,
                        },
                        (0, 1),
                    );
                    let found = sol.is_some();
                    let loss: i32 = sol
                        .iter()
                        .flatten()
                        .map(|(i, r)| base.get(*i).map(|p| p.w * p.h - r.w * r.h).unwrap_or(0))
                        .sum();
                    if loss > 0 {
                        shrunk += 1;
                        lost += loss;
                    }
                    worst = worst.max(t.elapsed());
                    total += 1;
                    if found {
                        ok += 1;
                    } else {
                        fails.push((w.kind, col, row));
                    }
                }
            }
        }
        println!("reflow found for {ok}/{total} moves; slowest {worst:?}");
        println!(
            "{shrunk} needed shrinking, {:.1} cells lost on average when they did",
            lost as f32 / shrunk.max(1) as f32
        );
        println!("some failures: {:?}", &fails[..fails.len().min(12)]);
    }

    #[test]
    fn style_overrides_round_trip() {
        let (mut l, w) =
            Layout::parse("theme colorscheme=\"nord\" style=\"soft\"\nlayout {\n}\n").unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert!(l.style_overrides.is_empty());
        l.style_overrides = vec![
            ("radius".into(), KdlValue::Integer(12)),
            ("motion".into(), KdlValue::Float(0.6)),
        ];
        let out = l.render();
        let (mut back, w) = Layout::parse(&out).unwrap();
        assert!(w.is_empty(), "{w:?}\n{out}");
        assert_eq!(back.style_overrides, l.style_overrides, "{out}");
        assert!(out.contains("radius 12"), "{out}");
        // Changing one keeps the other line untouched; clearing removes the block.
        back.style_overrides[0].1 = KdlValue::Integer(16);
        let out2 = back.render();
        assert!(
            out2.contains("radius 16") && out2.contains("motion 0.6"),
            "{out2}"
        );
        let (mut back2, _) = Layout::parse(&out2).unwrap();
        back2.style_overrides.clear();
        let out3 = back2.render();
        assert!(!out3.contains('{') || !out3.contains("radius"), "{out3}");
        assert!(
            out3.contains("theme colorscheme=\"nord\" style=\"soft\""),
            "{out3}"
        );
        let (b3, w3) = Layout::parse(&out3).unwrap();
        assert!(w3.is_empty() && b3.style_overrides.is_empty(), "{out3}");
    }

    #[test]
    fn widget_settings_round_trip() {
        use crate::widget_settings::{Opt, get_bool, get_int, set};
        let text = "layout {\n    // my clock\n    clock col=0 row=0 width=4 height=2\n    calendar col=4 row=0 width=4 height=4 {\n        // Mondays!\n        week-numbers #true\n    }\n}\n";
        let (mut l, w) = Layout::parse(text).unwrap();
        assert!(w.is_empty(), "{w:?}");
        let cal = l.widgets[1].id;
        assert!(get_bool(&l.widgets[1].settings, Opt::WeekNumbers, false));
        let clock = l.widgets[0].id;
        let v = l.settings_mut(clock).unwrap();
        set(v, Opt::Hours, KdlValue::Integer(12), false);
        set(v, Opt::Seconds, KdlValue::Bool(false), false);
        let out = l.render();
        assert!(
            out.contains("    clock col=0 row=0 width=4 height=2 {\n        hours 12\n        seconds #false\n    }\n"),
            "{out}"
        );
        // Untouched widget keeps its block and comment exactly.
        assert!(
            out.contains("        // Mondays!\n        week-numbers #true\n    }"),
            "{out}"
        );
        let (mut back, w) = Layout::parse(&out).unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(get_int(&back.widgets[0].settings, Opt::Hours, false), 12);
        // Back to defaults: the block goes away.
        let v = back.settings_mut(back.widgets[0].id).unwrap();
        set(v, Opt::Hours, KdlValue::Integer(24), false);
        set(v, Opt::Seconds, KdlValue::Bool(true), false);
        let out2 = back.render();
        assert!(
            out2.contains("    clock col=0 row=0 width=4 height=2\n"),
            "{out2}"
        );
        assert!(back.get(cal).is_some());
        // Dropping the first of two lines keeps the second on its own line.
        let (mut l, _) = Layout::parse("layout {\n    clock col=0 row=0 {\n        seconds #false\n        date #false\n    }\n}\n").unwrap();
        let id = l.widgets[0].id;
        set(
            l.settings_mut(id).unwrap(),
            Opt::Seconds,
            KdlValue::Bool(true),
            false,
        );
        let out3 = l.render();
        assert!(out3.contains(" {\n        date #false\n    }\n"), "{out3}");
        // Unknown settings warn but survive.
        let (_, w) =
            Layout::parse("layout {\n    clock col=0 row=0 { colour \"red\" }\n}\n").unwrap();
        assert_eq!(w.len(), 1, "{w:?}");
    }

    #[test]
    fn reset_keeps_one_widget_per_line() {
        let text = "theme colorscheme=\"nord\" style=\"soft\"\nlayout {\n    clock col=0 row=0 width=4 height=2\n}\n";
        let (mut l, _) = Layout::parse(text).unwrap();
        l.reset();
        let out = l.render();
        assert!(out.contains("layout {\n"), "{out}");
        for w in &l.widgets {
            assert!(
                out.lines()
                    .any(|line| line.trim_start().starts_with(w.kind.name())),
                "{} not on its own line:\n{out}",
                w.kind.name()
            );
        }
        let (back, warnings) = Layout::parse(&out).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(back.widgets.len(), l.widgets.len());
    }

    #[test]
    fn removing_the_first_widget_keeps_line_breaks() {
        let text = "layout {\n    clock col=0 row=0 width=4 height=2\n    calendar col=4 row=0 width=4 height=4\n}\n";
        let (mut l, _) = Layout::parse(text).unwrap();
        let clock = l.widgets.iter().find(|w| w.kind == Kind::Clock).unwrap().id;
        l.remove(clock);
        let out = l.render();
        assert!(out.contains("layout {\n    calendar"), "{out}");
    }

    #[test]
    fn font_override_round_trips() {
        let (mut l, _) =
            Layout::parse("theme colorscheme=\"nord\" style=\"sharp\"\nlayout {\n}\n").unwrap();
        l.style_overrides = vec![
            (
                "font-family".into(),
                KdlValue::String("JetBrains Mono".into()),
            ),
            ("glass".into(), KdlValue::Bool(true)),
            ("card-opacity".into(), KdlValue::Float(0.85)),
        ];
        let out = l.render();
        let (back, w) = Layout::parse(&out).unwrap();
        assert!(w.is_empty(), "{w:?}\n{out}");
        assert_eq!(back.style_overrides, l.style_overrides, "{out}");
    }

    fn rects(l: &Layout) -> Vec<(u32, Rect)> {
        l.widgets.iter().map(|w| (w.id, w.rect())).collect()
    }

    /// Every widget shows: inside the grid and on top of no other.
    fn all_shown(l: &Layout) {
        assert_eq!(l.visible().len(), l.widgets.len(), "{:?}", rects(l));
    }

    #[test]
    fn grid_parses() {
        let (l, w) = Layout::parse("grid columns=8 rows=5\nlayout {\n}\n").unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert_eq!((l.grid, l.cols, l.rows), (Some((8, 5)), 8, 5));

        let (l, _) = Layout::parse("layout {\n}\n").unwrap();
        assert_eq!(l.grid, None, "no grid node: the screen decides");

        let (l, w) = Layout::parse("grid columns=100 rows=1\nlayout {\n}\n").unwrap();
        assert_eq!(l.grid, Some((*COLUMNS.end(), *ROWS.start())));
        assert_eq!(
            w,
            vec!["grid columns must be 4 to 48", "grid rows must be 3 to 32"]
        );

        let (l, w) = Layout::parse("grid columns=8\nlayout {\n}\n").unwrap();
        assert_eq!(l.grid, None);
        assert_eq!(w, vec!["grid needs columns= and rows= (whole numbers)"]);
    }

    #[test]
    fn cell_size_is_gone() {
        let (l, w) = Layout::parse("cell-size 90\nlayout {\n}\n").unwrap();
        assert_eq!(l.grid, None);
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("`grid columns=… rows=…`"), "{w:?}");
    }

    #[test]
    fn misfit_shrinks_to_fit() {
        let text = "layout {\n    clock col=0 row=0 width=4 height=4\n    calendar col=10 row=0 width=5 height=5\n}\n";
        let (mut l, _) = Layout::parse(text).unwrap();
        l.set_screen_grid(16, 9);
        let clock = l.get(id_of(&l, Kind::Clock)).unwrap().rect();
        let refit = l.set_grid(Some((8, 4)));
        assert!(refit.evicted.is_empty(), "{refit:?}");
        assert_eq!(refit.moved, 1);
        all_shown(&l);
        assert_eq!(l.get(id_of(&l, Kind::Clock)).unwrap().rect(), clock);
        let cal = l.get(id_of(&l, Kind::Calendar)).unwrap();
        assert_eq!((cal.col, cal.row, cal.w, cal.h), (4, 0, 4, 4));
    }

    #[test]
    fn eviction_goes_by_reading_order() {
        // Room for one of the two, even at their minimum sizes; the first in reading order of
        // where they were gets it.
        let evicted = |text: &str| {
            let (mut l, _) = Layout::parse(text).unwrap();
            l.set_screen_grid(16, 9);
            let refit = l.set_grid(Some((8, 4)));
            all_shown(&l);
            assert_eq!(refit.moved, 1);
            assert_eq!(refit.evicted.len(), 1);
            assert_eq!(l.widgets.len(), 2);
            (refit.evicted[0].kind, refit.message().unwrap())
        };
        assert_eq!(
            evicted(
                "layout {\n    clock col=0 row=0 width=4 height=4\n    wifi col=0 row=5 width=3 height=4\n    calendar col=8 row=0 width=4 height=4\n}\n"
            ),
            (Kind::Wifi, "Removed Wi-Fi: no room".into())
        );
        assert_eq!(
            evicted(
                "layout {\n    clock col=0 row=0 width=4 height=4\n    wifi col=8 row=0 width=3 height=4\n    calendar col=0 row=5 width=4 height=4\n}\n"
            ),
            (Kind::Calendar, "Removed Calendar: no room".into())
        );
    }

    #[test]
    fn smaller_grid_fits_or_evicts() {
        let mut l = grid();
        let before = rects(&l);
        let refit = l.set_grid(Some((8, 5)));
        all_shown(&l);
        assert!(!refit.evicted.is_empty());
        assert_eq!(l.widgets.len() + refit.evicted.len(), before.len());
        // What showed on the small grid already stays put.
        for (id, r) in before {
            if r.inside(8, 5) {
                assert_eq!(l.get(id).map(Placed::rect), Some(r));
            }
        }
        // Evicted widgets are gone from the file too.
        let out = l.render();
        let (back, _) = Layout::parse(&out).unwrap();
        assert_eq!(back.widgets.len(), l.widgets.len(), "{out}");
        assert_eq!(back.grid, Some((8, 5)));
    }

    #[test]
    fn growing_the_grid_moves_nothing() {
        let mut l = grid();
        let before = rects(&l);
        let refit = l.set_grid(Some((20, 12)));
        assert!(!refit.changed(), "{refit:?}");
        assert_eq!(rects(&l), before);
        l.set_grid(Some((8, 5)));
        let small = rects(&l);
        assert!(!l.set_grid(Some((9, 5))).changed());
        assert!(!l.set_grid(Some((9, 7))).changed());
        assert_eq!(rects(&l), small);
    }

    #[test]
    fn automatic_grid_hides_instead() {
        let mut l = grid();
        l.set_grid(Some((16, 9)));
        let before = rects(&l);
        // Back to automatic, then a small screen: hidden, not moved or removed.
        assert!(!l.set_grid(None).changed());
        l.set_screen_grid(8, 5);
        assert!(l.hidden_count() > 0);
        assert_eq!(rects(&l), before);
    }

    #[test]
    fn reload_with_another_grid_refits() {
        let mut l = grid();
        let (same, _) = Layout::parse(&Layout::default_text()).unwrap();
        assert!(!l.replace(same).changed(), "same file, nothing to fit");
        let text = Layout::default_text().replace("layout {", "grid columns=8 rows=5\nlayout {");
        let (fresh, _) = Layout::parse(&text).unwrap();
        let refit = l.replace(fresh);
        assert!(!refit.evicted.is_empty());
        assert_eq!((l.cols, l.rows), (8, 5));
        all_shown(&l);
        // Reading it again changes nothing more.
        let (again, _) = Layout::parse(&l.render()).unwrap();
        assert!(!l.replace(again).changed());
    }

    #[test]
    fn grid_round_trips_keeping_comments() {
        // An existing node is updated in place, with its comments.
        let text = "// my dash\n// small screen\ngrid columns=16 rows=9 // laptop\n\nlayout {\n    clock col=0 row=0 width=4 height=2\n}\n";
        let (mut l, _) = Layout::parse(text).unwrap();
        assert_eq!(l.render(), text, "unchanged: written as it was");
        l.set_grid(Some((10, 6)));
        let out = l.render();
        assert_eq!(
            out,
            text.replace("columns=16 rows=9", "columns=10 rows=6"),
            "{out}"
        );
        let (mut back, _) = Layout::parse(&out).unwrap();
        assert_eq!(back.grid, Some((10, 6)));
        assert_eq!(back.render(), out);
        // Back to automatic: the node goes, the comments above it stay.
        l.set_grid(None);
        let out = l.render();
        assert!(!out.contains("grid"), "{out}");
        assert!(out.contains("// my dash\n// small screen\n"), "{out}");
        assert_eq!(Layout::parse(&out).unwrap().0.grid, None);

        // Without a node, one is added after the theme.
        let mut l = grid();
        l.set_grid(Some((12, 7)));
        let out = l.render();
        assert!(
            out.contains("style=\"default\"\n\ngrid columns=12 rows=7\n\n"),
            "{out}"
        );
        let (mut back, w) = Layout::parse(&out).unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(back.grid, Some((12, 7)));
        assert_eq!(back.render(), out);

        // An old cell-size line is replaced, keeping its comment.
        let (mut l, _) =
            Layout::parse("// size\ncell-size 90\nlayout {\n    clock col=0 row=0\n}\n").unwrap();
        l.set_grid(Some((12, 7)));
        let out = l.render();
        assert!(
            out.starts_with("// size\ngrid columns=12 rows=7\nlayout {"),
            "{out}"
        );
        assert!(Layout::parse(&out).unwrap().1.is_empty());
    }
}

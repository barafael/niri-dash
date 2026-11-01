//! The look of the dashboard. A **theme** is a **color scheme** (`colorschemes/*.kdl`: colors
//! only) paired with a **style** (`styles/*.kdl`: corners, spacing, fonts, animation speed). Any
//! scheme goes with any style; `config.kdl` picks them with
//! `theme colorscheme="gruvbox-dark" style="soft"`.
//!
//! Both are resolved into a [`Look`]: every value the Slint `Theme` global holds. Switching
//! themes blends the current look into the new one.

use crate::{Dash, Theme as UiTheme};
use kdl::{KdlDocument, KdlValue};
use slint::{Color, ComponentHandle, SharedString};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use strum::{AsRefStr, EnumCount, EnumIter, EnumString, IntoEnumIterator};

/// Bundled color schemes and styles, written to the config dir on first start.
pub const COLORSCHEMES: &[(&str, &str)] = &[
    (
        "default",
        include_str!("../assets/colorschemes/default.kdl"),
    ),
    ("light", include_str!("../assets/colorschemes/light.kdl")),
    (
        "gruvbox-dark",
        include_str!("../assets/colorschemes/gruvbox-dark.kdl"),
    ),
    (
        "gruvbox-light",
        include_str!("../assets/colorschemes/gruvbox-light.kdl"),
    ),
    (
        "catppuccin-mocha",
        include_str!("../assets/colorschemes/catppuccin-mocha.kdl"),
    ),
    (
        "catppuccin-latte",
        include_str!("../assets/colorschemes/catppuccin-latte.kdl"),
    ),
    ("nord", include_str!("../assets/colorschemes/nord.kdl")),
    (
        "rose-pine",
        include_str!("../assets/colorschemes/rose-pine.kdl"),
    ),
    (
        "dracula",
        include_str!("../assets/colorschemes/dracula.kdl"),
    ),
    (
        "solarized-dark",
        include_str!("../assets/colorschemes/solarized-dark.kdl"),
    ),
    (
        "everforest",
        include_str!("../assets/colorschemes/everforest.kdl"),
    ),
];

pub const STYLES: &[(&str, &str)] = &[
    ("default", include_str!("../assets/styles/default.kdl")),
    ("compact", include_str!("../assets/styles/compact.kdl")),
    ("soft", include_str!("../assets/styles/soft.kdl")),
    ("sharp", include_str!("../assets/styles/sharp.kdl")),
    ("snappy", include_str!("../assets/styles/snappy.kdl")),
    ("still", include_str!("../assets/styles/still.kdl")),
];

pub const DEFAULT_COLORSCHEME: &str = COLORSCHEMES[0].1;
pub const DEFAULT_STYLE: &str = STYLES[0].1;

/// A role a color plays in the UI. The kebab-case name is the key in color scheme files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, EnumString, AsRefStr, EnumCount)]
#[strum(serialize_all = "kebab-case")]
pub enum ColorRole {
    Backdrop,
    Card,
    CardRaised,
    Panel,
    Hover,
    Pressed,
    Border,
    Subtle,
    Shadow,
    Text,
    Dim,
    Faint,
    Accent,
    AccentSoft,
    OnAccent,
    Good,
    Warn,
    Bad,
    Track,
    Knob,
}

/// A style variable. The kebab-case name is the key in style files and config.kdl.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, EnumString, AsRefStr, EnumCount)]
#[strum(serialize_all = "kebab-case")]
pub enum StyleVar {
    Radius,
    RadiusSmall,
    Gap,
    Padding,
    BorderWidth,
    FontFamily,
    FontSmall,
    FontBody,
    FontTitle,
    FontWeightDisplay,
    Motion,
    /// Blur what's behind the dashboard (frosted glass), where the compositor supports it.
    Glass,
    /// How strongly the screen behind is dimmed, 0..1 (multiplies the backdrop color's alpha).
    BackdropOpacity,
    /// How opaque widgets are, 0..1 (multiplies the card color's alpha).
    CardOpacity,
}

/// The value of one style variable.
#[derive(Debug, Clone, PartialEq)]
pub enum StyleValue {
    Num(f32),
    Int(i32),
    Text(SharedString),
    Flag(bool),
}

/// Every style variable, typed.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub radius: f32,
    pub radius_small: f32,
    pub gap: f32,
    pub padding: f32,
    pub border_width: f32,
    pub font_family: SharedString,
    pub font_small: f32,
    pub font_body: f32,
    pub font_title: f32,
    pub font_weight_display: i32,
    pub motion: f32,
    pub glass: bool,
    pub backdrop_opacity: f32,
    pub card_opacity: f32,
}

impl Style {
    pub fn get(&self, var: StyleVar) -> StyleValue {
        use StyleValue::*;
        match var {
            StyleVar::Radius => Num(self.radius),
            StyleVar::RadiusSmall => Num(self.radius_small),
            StyleVar::Gap => Num(self.gap),
            StyleVar::Padding => Num(self.padding),
            StyleVar::BorderWidth => Num(self.border_width),
            StyleVar::FontFamily => Text(self.font_family.clone()),
            StyleVar::FontSmall => Num(self.font_small),
            StyleVar::FontBody => Num(self.font_body),
            StyleVar::FontTitle => Num(self.font_title),
            StyleVar::FontWeightDisplay => Int(self.font_weight_display),
            StyleVar::Motion => Num(self.motion),
            StyleVar::Glass => Flag(self.glass),
            StyleVar::BackdropOpacity => Num(self.backdrop_opacity),
            StyleVar::CardOpacity => Num(self.card_opacity),
        }
    }

    /// Set a variable from a KDL value, clamped to a sensible range. Errors describe what was
    /// expected.
    pub fn set(&mut self, var: StyleVar, value: &KdlValue) -> Result<(), &'static str> {
        let num = match value {
            KdlValue::Integer(i) => Some(*i as f32),
            KdlValue::Float(f) => Some(*f as f32),
            _ => None,
        };
        let need = |n: Option<f32>| n.ok_or("expected a number");
        match var {
            StyleVar::Radius => self.radius = need(num)?.max(0.0),
            StyleVar::RadiusSmall => self.radius_small = need(num)?.max(0.0),
            StyleVar::Gap => self.gap = need(num)?.clamp(0.0, 40.0),
            StyleVar::Padding => self.padding = need(num)?.clamp(0.0, 60.0),
            StyleVar::BorderWidth => self.border_width = need(num)?.clamp(0.0, 8.0),
            StyleVar::FontFamily => {
                self.font_family = value.as_string().ok_or("expected a string")?.into()
            }
            StyleVar::FontSmall => self.font_small = need(num)?.clamp(6.0, 40.0),
            StyleVar::FontBody => self.font_body = need(num)?.clamp(6.0, 40.0),
            StyleVar::FontTitle => self.font_title = need(num)?.clamp(6.0, 60.0),
            StyleVar::FontWeightDisplay => {
                self.font_weight_display = (need(num)? as i32).clamp(100, 900)
            }
            StyleVar::Motion => self.motion = need(num)?.clamp(0.0, 5.0),
            StyleVar::Glass => self.glass = value.as_bool().ok_or("expected #true or #false")?,
            StyleVar::BackdropOpacity => self.backdrop_opacity = need(num)?.clamp(0.0, 1.0),
            StyleVar::CardOpacity => self.card_opacity = need(num)?.clamp(0.0, 1.0),
        }
        Ok(())
    }

    fn mix(&self, b: &Style, t: f32) -> Style {
        let l = |x: f32, y: f32| x + (y - x) * t;
        let early = t < 0.5;
        Style {
            radius: l(self.radius, b.radius),
            radius_small: l(self.radius_small, b.radius_small),
            gap: l(self.gap, b.gap),
            padding: l(self.padding, b.padding),
            border_width: l(self.border_width, b.border_width),
            font_family: if early {
                self.font_family.clone()
            } else {
                b.font_family.clone()
            },
            font_small: l(self.font_small, b.font_small),
            font_body: l(self.font_body, b.font_body),
            font_title: l(self.font_title, b.font_title),
            font_weight_display: if early {
                self.font_weight_display
            } else {
                b.font_weight_display
            },
            motion: b.motion,
            glass: b.glass,
            backdrop_opacity: l(self.backdrop_opacity, b.backdrop_opacity),
            card_opacity: l(self.card_opacity, b.card_opacity),
        }
    }
}

/// Everything the Slint `Theme` global holds: one color per role, and the style.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    pub colors: [Color; ColorRole::COUNT],
    pub style: Style,
}

impl Look {
    pub fn color(&self, role: ColorRole) -> Color {
        self.colors[role as usize]
    }
}

pub fn current(ui: &Dash) -> Look {
    let t = ui.global::<UiTheme>();
    let colors = ColorRole::iter()
        .map(|r| match r {
            ColorRole::Backdrop => t.get_backdrop(),
            ColorRole::Card => t.get_card(),
            ColorRole::CardRaised => t.get_card_raised(),
            ColorRole::Panel => t.get_panel(),
            ColorRole::Hover => t.get_hover(),
            ColorRole::Pressed => t.get_pressed(),
            ColorRole::Border => t.get_border(),
            ColorRole::Subtle => t.get_subtle(),
            ColorRole::Shadow => t.get_shadow(),
            ColorRole::Text => t.get_text(),
            ColorRole::Dim => t.get_dim(),
            ColorRole::Faint => t.get_faint(),
            ColorRole::Accent => t.get_accent(),
            ColorRole::AccentSoft => t.get_accent_soft(),
            ColorRole::OnAccent => t.get_on_accent(),
            ColorRole::Good => t.get_good(),
            ColorRole::Warn => t.get_warn(),
            ColorRole::Bad => t.get_bad(),
            ColorRole::Track => t.get_track(),
            ColorRole::Knob => t.get_knob(),
        })
        .collect::<Vec<_>>()
        .try_into()
        .expect("one color per role");
    let style = Style {
        radius: t.get_radius(),
        radius_small: t.get_radius_small(),
        gap: t.get_gap(),
        padding: t.get_pad(),
        border_width: t.get_border_width(),
        font_family: t.get_font_family(),
        font_small: t.get_font_small(),
        font_body: t.get_font_body(),
        font_title: t.get_font_title(),
        font_weight_display: t.get_font_weight_display(),
        motion: t.get_motion(),
        glass: t.get_glass(),
        backdrop_opacity: t.get_backdrop_opacity(),
        card_opacity: t.get_card_opacity(),
    };
    Look { colors, style }
}

pub fn set(ui: &Dash, look: &Look) {
    let t = ui.global::<UiTheme>();
    for role in ColorRole::iter() {
        let c = look.color(role);
        match role {
            ColorRole::Backdrop => t.set_backdrop(c),
            ColorRole::Card => t.set_card(c),
            ColorRole::CardRaised => t.set_card_raised(c),
            ColorRole::Panel => t.set_panel(c),
            ColorRole::Hover => t.set_hover(c),
            ColorRole::Pressed => t.set_pressed(c),
            ColorRole::Border => t.set_border(c),
            ColorRole::Subtle => t.set_subtle(c),
            ColorRole::Shadow => t.set_shadow(c),
            ColorRole::Text => t.set_text(c),
            ColorRole::Dim => t.set_dim(c),
            ColorRole::Faint => t.set_faint(c),
            ColorRole::Accent => t.set_accent(c),
            ColorRole::AccentSoft => t.set_accent_soft(c),
            ColorRole::OnAccent => t.set_on_accent(c),
            ColorRole::Good => t.set_good(c),
            ColorRole::Warn => t.set_warn(c),
            ColorRole::Bad => t.set_bad(c),
            ColorRole::Track => t.set_track(c),
            ColorRole::Knob => t.set_knob(c),
        }
    }
    let s = &look.style;
    t.set_radius(s.radius);
    t.set_radius_small(s.radius_small);
    t.set_gap(s.gap);
    t.set_pad(s.padding);
    t.set_border_width(s.border_width);
    t.set_font_family(s.font_family.clone());
    t.set_font_small(s.font_small);
    t.set_font_body(s.font_body);
    t.set_font_title(s.font_title);
    t.set_font_weight_display(s.font_weight_display);
    t.set_motion(s.motion);
    t.set_glass(s.glass);
    t.set_backdrop_opacity(s.backdrop_opacity);
    t.set_card_opacity(s.card_opacity);
}

/// Blend two looks: colors and sizes in between, everything else switches halfway.
pub fn mix(a: &Look, b: &Look, t: f32) -> Look {
    let channel = |x: u8, y: u8| {
        (x as f32 + (y as f32 - x as f32) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    let colors = std::array::from_fn(|i| {
        let (ca, cb) = (a.colors[i], b.colors[i]);
        Color::from_argb_u8(
            channel(ca.alpha(), cb.alpha()),
            channel(ca.red(), cb.red()),
            channel(ca.green(), cb.green()),
            channel(ca.blue(), cb.blue()),
        )
    });
    Look {
        colors,
        style: a.style.mix(&b.style, t),
    }
}

pub fn parse_color(s: &str) -> Option<Color> {
    let h = s.strip_prefix('#')?;
    let hex = |i: usize, n: usize| u8::from_str_radix(h.get(i..i + n)?, 16).ok();
    let (r, g, b, a) = match h.len() {
        3 => (hex(0, 1)? * 17, hex(1, 1)? * 17, hex(2, 1)? * 17, 255),
        6 => (hex(0, 2)?, hex(2, 2)?, hex(4, 2)?, 255),
        8 => (hex(0, 2)?, hex(2, 2)?, hex(4, 2)?, hex(6, 2)?),
        _ => return None,
    };
    Some(Color::from_argb_u8(a, r, g, b))
}

fn colors_of(doc: &KdlDocument, look: &mut Look, warnings: &mut Vec<String>) {
    let Some(colors) = doc.get("colors").and_then(|n| n.children()) else {
        return;
    };
    for node in colors.nodes() {
        let key = node.name().value();
        let Ok(role) = ColorRole::from_str(key) else {
            warnings.push(format!("colors.{key}: unknown color"));
            continue;
        };
        match node
            .entries()
            .first()
            .and_then(|e| e.value().as_string())
            .and_then(parse_color)
        {
            Some(c) => look.colors[role as usize] = c,
            None => warnings.push(format!("colors.{key}: expected a color like \"#rrggbb\"")),
        }
    }
}

/// Apply `key value` pairs to a style.
fn apply_style<'a>(
    style: &mut Style,
    pairs: impl Iterator<Item = (&'a str, &'a KdlValue)>,
    warnings: &mut Vec<String>,
) {
    for (key, value) in pairs {
        match StyleVar::from_str(key) {
            Ok(var) => {
                if let Err(e) = style.set(var, value) {
                    warnings.push(format!("style.{key}: {e}"));
                }
            }
            Err(_) => warnings.push(format!("style.{key}: unknown variable")),
        }
    }
}

fn style_of(doc: &KdlDocument, look: &mut Look, warnings: &mut Vec<String>) {
    let Some(block) = doc.get("style").and_then(|n| n.children()) else {
        return;
    };
    let pairs = block
        .nodes()
        .iter()
        .filter_map(|n| Some((n.name().value(), n.entries().first()?.value())));
    apply_style(&mut look.style, pairs, warnings);
}

/// The built-in defaults: the default color scheme and style.
fn base() -> Look {
    let mut look = Look {
        colors: [Color::default(); ColorRole::COUNT],
        style: Style {
            radius: 20.0,
            radius_small: 12.0,
            gap: 7.0,
            padding: 16.0,
            border_width: 1.0,
            font_family: SharedString::new(),
            font_small: 12.0,
            font_body: 14.0,
            font_title: 15.0,
            font_weight_display: 300,
            motion: 1.0,
            glass: false,
            backdrop_opacity: 1.0,
            card_opacity: 1.0,
        },
    };
    let mut ignored = Vec::new();
    colors_of(
        &KdlDocument::parse(DEFAULT_COLORSCHEME).expect("default color scheme parses"),
        &mut look,
        &mut ignored,
    );
    style_of(
        &KdlDocument::parse(DEFAULT_STYLE).expect("default style parses"),
        &mut look,
        &mut ignored,
    );
    look
}

/// The look for a color scheme and a style (file contents). Starts from the defaults so partial
/// files work. A file with a syntax error is reported and skipped.
pub fn resolve(colorscheme: &str, style: &str) -> (Look, Vec<String>) {
    resolve_with(colorscheme, style, &[])
}

/// The variables a style preset changes from the built-in defaults. Choosing that preset drops
/// adjustments to these; adjustments to anything else (often the font, glass, opacity) are kept.
pub fn preset_changes(style: &str) -> Vec<StyleVar> {
    let defaults = base().style;
    let (preset, _) = resolve(DEFAULT_COLORSCHEME, style);
    StyleVar::iter()
        .filter(|&v| preset.style.get(v) != defaults.get(v))
        .collect()
}

/// Like `resolve`, with per-variable adjustments on top of the style (from config.kdl).
pub fn resolve_with(
    colorscheme: &str,
    style: &str,
    overrides: &[(String, KdlValue)],
) -> (Look, Vec<String>) {
    let mut look = base();
    let mut warnings = Vec::new();
    match KdlDocument::parse(colorscheme) {
        Ok(doc) => colors_of(&doc, &mut look, &mut warnings),
        Err(e) => warnings.push(format!("color scheme: {e}")),
    }
    match KdlDocument::parse(style) {
        Ok(doc) => style_of(&doc, &mut look, &mut warnings),
        Err(e) => warnings.push(format!("style: {e}")),
    }
    let mut w = Vec::new();
    apply_style(
        &mut look.style,
        overrides.iter().map(|(k, v)| (k.as_str(), v)),
        &mut w,
    );
    warnings.extend(
        w.into_iter()
            .map(|w| format!("config.kdl theme {{ }}: {w}")),
    );
    (look, warnings)
}

/// One adjustable style variable: the steps the + and - buttons go through, and a label.
pub struct Knob {
    pub var: StyleVar,
    pub label: &'static str,
    pub steps: &'static [f32],
}

pub const KNOBS: &[Knob] = &[
    Knob {
        var: StyleVar::Radius,
        label: "Corner radius",
        steps: &[
            0., 2., 4., 6., 8., 10., 12., 14., 16., 18., 20., 24., 28., 32., 40.,
        ],
    },
    Knob {
        var: StyleVar::RadiusSmall,
        label: "Inner corner radius",
        steps: &[0., 2., 4., 6., 8., 10., 12., 14., 16., 20.],
    },
    Knob {
        var: StyleVar::Gap,
        label: "Widget spacing",
        steps: &[0., 2., 3., 4., 5., 6., 7., 8., 9., 10., 12., 14., 16.],
    },
    Knob {
        var: StyleVar::Padding,
        label: "Widget padding",
        steps: &[6., 8., 10., 12., 14., 16., 18., 20., 24., 28.],
    },
    Knob {
        var: StyleVar::BorderWidth,
        label: "Border width",
        steps: &[0., 1., 2., 3.],
    },
    Knob {
        var: StyleVar::FontSmall,
        label: "Small text",
        steps: &[9., 10., 11., 12., 13., 14., 15., 16.],
    },
    Knob {
        var: StyleVar::FontBody,
        label: "Body text",
        steps: &[10., 11., 12., 13., 14., 15., 16., 17., 18., 20.],
    },
    Knob {
        var: StyleVar::FontTitle,
        label: "Titles",
        steps: &[11., 12., 13., 14., 15., 16., 17., 18., 20., 22., 24.],
    },
    Knob {
        var: StyleVar::FontWeightDisplay,
        label: "Big numbers weight",
        steps: &[100., 200., 300., 400., 500., 600., 700.],
    },
    Knob {
        var: StyleVar::Motion,
        label: "Animations",
        steps: &[0., 0.35, 0.6, 1.0, 1.4, 2.0],
    },
    Knob {
        var: StyleVar::BackdropOpacity,
        label: "Screen dimming",
        steps: &[0., 0.2, 0.4, 0.55, 0.7, 0.85, 1.0],
    },
    Knob {
        var: StyleVar::CardOpacity,
        label: "Widget opacity",
        steps: &[0.4, 0.55, 0.7, 0.8, 0.9, 1.0],
    },
];

/// Font families offered for `font-family`, of those installed ("" is the system default).
pub fn font_choices() -> Vec<String> {
    const CANDIDATES: [&str; 13] = [
        "Inter",
        "Noto Sans",
        "Roboto",
        "Open Sans",
        "Source Sans 3",
        "Cantarell",
        "Fira Sans",
        "IBM Plex Sans",
        "Ubuntu",
        "DejaVu Sans",
        "JetBrains Mono",
        "Fira Code",
        "Hack",
    ];
    let installed = std::process::Command::new("fc-list")
        .args([":", "family"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let have: Vec<&str> = installed
        .lines()
        .flat_map(|l| l.split(','))
        .map(str::trim)
        .collect();
    std::iter::once(String::new())
        .chain(
            CANDIDATES
                .iter()
                .filter(|c| have.contains(c))
                .map(|c| c.to_string()),
        )
        .collect()
}

/// How a value shows in the Style tab.
pub fn knob_text(var: StyleVar, v: f32) -> String {
    match var {
        StyleVar::Motion => match v {
            x if x <= 0.0 => "off".into(),
            x if x < 0.5 => "very fast".into(),
            x if x < 0.9 => "fast".into(),
            x if x < 1.2 => "normal".into(),
            x if x < 1.7 => "slow".into(),
            _ => "slower".into(),
        },
        StyleVar::FontWeightDisplay => format!("{v:.0}"),
        StyleVar::BackdropOpacity | StyleVar::CardOpacity => format!("{:.0} %", v * 100.0),
        _ => format!("{v:.0} px"),
    }
}

/// The step `delta` steps away from `current` (snapping to the nearest step first).
pub fn step(knob: &Knob, current: f32, delta: i32) -> f32 {
    let nearest = knob
        .steps
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - current).abs().total_cmp(&(b.1 - current).abs()))
        .map(|(i, _)| i as i32)
        .unwrap_or(0);
    let i = (nearest + delta).clamp(0, knob.steps.len() as i32 - 1);
    knob.steps[i as usize]
}

fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("niri-dash")
}

pub fn colorschemes_dir() -> PathBuf {
    config_dir().join("colorschemes")
}

pub fn styles_dir() -> PathBuf {
    config_dir().join("styles")
}

fn resolve_path(dir: &Path, name: &str) -> PathBuf {
    if let Some(rest) = name.strip_prefix("~/") {
        return dirs::home_dir().unwrap_or_default().join(rest);
    }
    if name.contains('/') {
        return PathBuf::from(name);
    }
    dir.join(format!("{name}.kdl"))
}

pub fn colorscheme_path(name: &str) -> PathBuf {
    resolve_path(&colorschemes_dir(), name)
}

pub fn style_path(name: &str) -> PathBuf {
    resolve_path(&styles_dir(), name)
}

/// The text of a color scheme or style: its file, else the bundled copy.
pub fn load_text(path: &Path, bundled: &[(&str, &str)], name: &str) -> Option<String> {
    std::fs::read_to_string(path).ok().or_else(|| {
        bundled
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, t)| t.to_string())
    })
}

/// Write the bundled color schemes and styles if missing (never overwrites user edits).
pub fn install_bundled() {
    for (dir, set) in [(colorschemes_dir(), COLORSCHEMES), (styles_dir(), STYLES)] {
        let _ = std::fs::create_dir_all(&dir);
        for (name, text) in set {
            let p = dir.join(format!("{name}.kdl"));
            if !p.exists()
                && let Err(e) = std::fs::write(&p, text)
            {
                log::warn!("writing {}: {e}", p.display());
            }
        }
    }
}

/// Names of the color schemes or styles in a directory, "default" first.
pub fn list(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let p = e.path();
                    if p.extension().is_some_and(|x| x == "kdl") {
                        p.file_stem().map(|s| s.to_string_lossy().into_owned())
                    } else {
                        None
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort_by(|a, b| (a != "default").cmp(&(b != "default")).then(a.cmp(b)));
    names
}

/// A short description from a file's first comment line.
pub fn description(text: &str) -> String {
    let first = text
        .lines()
        .next()
        .unwrap_or("")
        .trim_start_matches('/')
        .trim();
    if let Some((_, rest)) = first.split_once("\". ") {
        return rest.trim_end_matches('.').to_string();
    }
    if let Some((_, rest)) = first.split_once("\" (") {
        return rest.split(')').next().unwrap_or("").to_string();
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors() {
        assert_eq!(
            parse_color("#ff0000"),
            Some(Color::from_argb_u8(255, 255, 0, 0))
        );
        assert_eq!(
            parse_color("#00ff0080"),
            Some(Color::from_argb_u8(0x80, 0, 255, 0))
        );
        assert_eq!(
            parse_color("#fff"),
            Some(Color::from_argb_u8(255, 255, 255, 255))
        );
        assert_eq!(parse_color("red"), None);
    }

    #[test]
    fn enum_names_match_files() {
        assert_eq!(ColorRole::CardRaised.as_ref(), "card-raised");
        assert_eq!(
            StyleVar::from_str("font-weight-display"),
            Ok(StyleVar::FontWeightDisplay)
        );
        assert!(StyleVar::from_str("nope").is_err());
    }

    #[test]
    fn bundled_files_are_complete_and_clean() {
        for (name, text) in COLORSCHEMES {
            let (_, warnings) = resolve(text, DEFAULT_STYLE);
            assert!(warnings.is_empty(), "{name}: {warnings:?}");
            let doc = KdlDocument::parse(text).unwrap();
            assert_eq!(
                doc.get("colors").unwrap().children().unwrap().nodes().len(),
                ColorRole::COUNT,
                "{name} lists every color"
            );
        }
        for (name, text) in STYLES {
            let (_, warnings) = resolve(DEFAULT_COLORSCHEME, text);
            assert!(warnings.is_empty(), "{name}: {warnings:?}");
            assert!(
                *name == "default" || !description(text).is_empty(),
                "{name} has a description"
            );
        }
    }

    #[test]
    fn scheme_and_style_are_independent() {
        let gruvbox = COLORSCHEMES
            .iter()
            .find(|(n, _)| *n == "gruvbox-dark")
            .unwrap()
            .1;
        let sharp = STYLES.iter().find(|(n, _)| *n == "sharp").unwrap().1;
        let (a, _) = resolve(gruvbox, sharp);
        let (b, _) = resolve(gruvbox, DEFAULT_STYLE);
        let (c, _) = resolve(DEFAULT_COLORSCHEME, sharp);
        assert_eq!(a.colors, b.colors, "the style doesn't touch colors");
        assert_eq!(a.style, c.style, "the color scheme doesn't touch the style");
    }

    #[test]
    fn mixing() {
        let (a, _) = resolve(DEFAULT_COLORSCHEME, DEFAULT_STYLE);
        let mut b = a.clone();
        b.colors[ColorRole::Card as usize] = Color::from_argb_u8(255, 200, 100, 0);
        b.style.radius = 40.0;
        let mut a2 = a.clone();
        a2.colors[ColorRole::Card as usize] = Color::from_argb_u8(255, 0, 0, 0);
        let m = mix(&a2, &b, 0.5);
        assert_eq!(
            m.color(ColorRole::Card),
            Color::from_argb_u8(255, 100, 50, 0)
        );
        assert_eq!(m.style.radius, 30.0);
        assert_eq!(mix(&a2, &b, 1.0), b);
    }

    #[test]
    fn stepping_and_overrides() {
        let k = KNOBS.iter().find(|k| k.var == StyleVar::Radius).unwrap();
        assert_eq!(step(k, 20.0, 1), 24.0);
        assert_eq!(step(k, 21.0, -1), 18.0, "snaps to 20 first");
        assert_eq!(step(k, 40.0, 1), 40.0, "clamps");
        let over = [
            ("radius".to_string(), KdlValue::Integer(4)),
            ("glass".to_string(), KdlValue::Bool(true)),
        ];
        let (look, w) = resolve_with(DEFAULT_COLORSCHEME, DEFAULT_STYLE, &over);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(look.style.radius, 4.0);
        assert!(look.style.glass);
        let (_, w) = resolve_with(
            DEFAULT_COLORSCHEME,
            DEFAULT_STYLE,
            &[("bogus".into(), KdlValue::Integer(1))],
        );
        assert_eq!(w.len(), 1);
        let (_, w) = resolve_with(
            DEFAULT_COLORSCHEME,
            DEFAULT_STYLE,
            &[("glass".into(), KdlValue::Integer(1))],
        );
        assert_eq!(w.len(), 1, "glass wants a boolean");
    }

    #[test]
    fn descriptions() {
        assert_eq!(
            description("// niri-dash style: \"compact\". Tighter: small corners.\n"),
            "Tighter: small corners"
        );
        assert_eq!(
            description("// niri-dash color scheme: \"nord\" (Nord). See default.kdl\n"),
            "Nord"
        );
    }

    #[test]
    fn presets_only_take_over_what_they_change() {
        let sharp = STYLES.iter().find(|(n, _)| *n == "sharp").unwrap().1;
        let taken = preset_changes(sharp);
        assert!(taken.contains(&StyleVar::Radius), "{taken:?}");
        assert!(!taken.contains(&StyleVar::FontFamily), "{taken:?}");
        assert!(!taken.contains(&StyleVar::Glass), "{taken:?}");
        // The default preset spells out every variable, all at the built-in values.
        assert!(preset_changes(DEFAULT_STYLE).is_empty());
    }
}

//! The look: a color scheme and a style (the same KDL files the Slint version reads), turned into
//! CSS custom properties, and the stylesheet that uses them on top of Blender's own theme.
//!
//! The scheme named `blender` (the default here) is Blender's dark theme, so a dashboard that
//! doesn't pick a scheme looks like Blender; every other scheme recolors the same parts.

use kdl::{KdlDocument, KdlValue};
use std::path::PathBuf;

/// The color schemes that come with niri-dash, by name.
pub const COLORSCHEMES: &[(&str, &str)] = &[
    // "default" in config.kdl: the Slint version's default scheme is the one this version
    // doesn't use, so the same file reads well in both.
    ("blender", BLENDER),
    ("light", include_str!("../assets/colorschemes/light.kdl")),
    (
        "high-contrast-dark",
        include_str!("../assets/colorschemes/high-contrast-dark.kdl"),
    ),
    (
        "high-contrast-light",
        include_str!("../assets/colorschemes/high-contrast-light.kdl"),
    ),
    (
        "ayu-dark",
        include_str!("../assets/colorschemes/ayu-dark.kdl"),
    ),
    (
        "catppuccin-latte",
        include_str!("../assets/colorschemes/catppuccin-latte.kdl"),
    ),
    (
        "catppuccin-mocha",
        include_str!("../assets/colorschemes/catppuccin-mocha.kdl"),
    ),
    (
        "dracula",
        include_str!("../assets/colorschemes/dracula.kdl"),
    ),
    (
        "everforest",
        include_str!("../assets/colorschemes/everforest.kdl"),
    ),
    (
        "everforest-light",
        include_str!("../assets/colorschemes/everforest-light.kdl"),
    ),
    (
        "gruvbox-dark",
        include_str!("../assets/colorschemes/gruvbox-dark.kdl"),
    ),
    (
        "gruvbox-light",
        include_str!("../assets/colorschemes/gruvbox-light.kdl"),
    ),
    (
        "kanagawa",
        include_str!("../assets/colorschemes/kanagawa.kdl"),
    ),
    (
        "monokai",
        include_str!("../assets/colorschemes/monokai.kdl"),
    ),
    ("nord", include_str!("../assets/colorschemes/nord.kdl")),
    (
        "one-dark",
        include_str!("../assets/colorschemes/one-dark.kdl"),
    ),
    (
        "rose-pine",
        include_str!("../assets/colorschemes/rose-pine.kdl"),
    ),
    (
        "rose-pine-dawn",
        include_str!("../assets/colorschemes/rose-pine-dawn.kdl"),
    ),
    (
        "rose-pine-moon",
        include_str!("../assets/colorschemes/rose-pine-moon.kdl"),
    ),
    (
        "solarized-dark",
        include_str!("../assets/colorschemes/solarized-dark.kdl"),
    ),
    (
        "solarized-light",
        include_str!("../assets/colorschemes/solarized-light.kdl"),
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

/// Blender 4's default dark theme, in the roles a scheme has.
const BLENDER: &str = r##"
colors {
    backdrop    "#161616e0"
    card        "#303030"
    card-raised "#3d3d3d"
    panel       "#282828"
    hover       "#ffffff14"
    pressed     "#ffffff26"
    border      "#1d1d1d"
    subtle      "#ffffff0a"
    shadow      "#00000080"
    text        "#e6e6e6"
    dim         "#a5a5a5"
    faint       "#6e6e6e"
    accent      "#4772b3"
    accent-soft "#4772b340"
    on-accent   "#ffffff"
    good        "#6ba84f"
    warn        "#e5a50a"
    bad         "#d9534f"
    track       "#1d1d1d"
    knob        "#e6e6e6"
}
"##;

/// The color roles, in the order schemes list them: each becomes `--name` in CSS.
pub const ROLES: &[&str] = &[
    "backdrop",
    "card",
    "card-raised",
    "panel",
    "hover",
    "pressed",
    "border",
    "subtle",
    "shadow",
    "text",
    "dim",
    "faint",
    "accent",
    "accent-soft",
    "on-accent",
    "good",
    "warn",
    "bad",
    "track",
    "knob",
];

/// What a style sets, as the dashboard uses it.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    pub radius: f32,
    pub gap: f32,
    pub padding: f32,
    pub font_family: String,
    pub font_small: f32,
    pub font_body: f32,
    pub font_title: f32,
    pub display_weight: f32,
    pub backdrop_opacity: f32,
    pub card_opacity: f32,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            radius: 8.0,
            gap: 5.0,
            padding: 10.0,
            font_family: String::new(),
            font_small: 11.0,
            font_body: 13.0,
            font_title: 13.0,
            display_weight: 300.0,
            backdrop_opacity: 1.0,
            card_opacity: 1.0,
        }
    }
}

/// A color scheme and a style, resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    /// `(role, "#rrggbbaa")` for every role in [`ROLES`].
    pub colors: Vec<(&'static str, String)>,
    pub style: Style,
    /// Whether the scheme is a light one (Blender's widgets follow).
    pub light: bool,
}

impl Look {
    pub fn color(&self, role: &str) -> &str {
        self.colors
            .iter()
            .find(|(r, _)| *r == role)
            .map_or("#ff00ff", |(_, c)| c.as_str())
    }

    /// The custom properties, as a CSS declaration block.
    pub fn vars(&self) -> String {
        let mut out = String::new();
        for (role, c) in &self.colors {
            out.push_str(&format!("--{role}: {c}; "));
        }
        let s = &self.style;
        // Blender's widgets are small; the style's sizes are for Slint's roomier ones, so they are
        // scaled down to sit with them.
        out.push_str(&format!(
            "--radius: {}px; --gap: {}px; --pad: {}px; --font-small: {}px; --font-body: {}px; \
             --font-title: {}px; --display-weight: {};",
            (s.radius * 0.4).round(),
            s.gap,
            (s.padding * 0.7).round(),
            s.font_small,
            s.font_body,
            s.font_title,
            s.display_weight,
        ));
        if !s.font_family.is_empty() {
            out.push_str(&format!(" font-family: \"{}\";", s.font_family));
        }
        out
    }
}

/// The scheme's text, from config's dir (a name or path) or the bundled ones.
fn scheme_text(name: &str) -> Option<String> {
    if let Some(text) = user_file("colorschemes", name) {
        return Some(text);
    }
    COLORSCHEMES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| t.to_string())
}

fn style_text(name: &str) -> Option<String> {
    if let Some(text) = user_file("styles", name) {
        return Some(text);
    }
    STYLES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| t.to_string())
}

/// A scheme or style the user keeps next to config.kdl, or at a path of its own.
fn user_file(dir: &str, name: &str) -> Option<String> {
    let path = if name.contains('/') {
        PathBuf::from(name)
    } else {
        crate::layout::Layout::path()
            .parent()?
            .join(dir)
            .join(format!("{name}.kdl"))
    };
    crate::files::read(&path).ok()
}

/// "#rgb", "#rrggbb" or "#rrggbbaa" as "#rrggbbaa", or None.
pub fn parse_color(s: &str) -> Option<String> {
    let hex = s.trim().strip_prefix('#')?;
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match hex.len() {
        3 => Some(format!(
            "#{}ff",
            hex.chars().flat_map(|c| [c, c]).collect::<String>()
        )),
        6 => Some(format!("#{hex}ff")),
        8 => Some(format!("#{hex}")),
        _ => None,
    }
}

/// The luminance of "#rrggbb…", 0..1.
fn luminance(c: &str) -> f32 {
    let byte = |i: usize| u8::from_str_radix(&c[i..i + 2], 16).unwrap_or(0) as f32 / 255.0;
    0.2126 * byte(1) + 0.7152 * byte(3) + 0.0722 * byte(5)
}

/// Scale a "#rrggbbaa" color's alpha by `k`.
fn fade(c: &str, k: f32) -> String {
    let a = u8::from_str_radix(&c[7..9], 16).unwrap_or(255) as f32;
    format!("{}{:02x}", &c[..7], (a * k.clamp(0.0, 1.0)).round() as u8)
}

fn number(v: &KdlValue) -> Option<f32> {
    match v {
        KdlValue::Integer(i) => Some(*i as f32),
        KdlValue::Float(f) => Some(*f as f32),
        _ => None,
    }
}

/// Resolve the scheme and style named in the config. Anything missing or wrong falls back to the
/// default, and is reported.
pub fn resolve(scheme: &str, style: &str, overrides: &[(String, KdlValue)]) -> (Look, Vec<String>) {
    let mut warnings = Vec::new();
    // An empty or "default" scheme is Blender's here: that is the point of this version.
    let scheme = if scheme.is_empty() || scheme == "default" {
        "blender"
    } else {
        scheme
    };
    let base: KdlDocument = BLENDER.parse().expect("the Blender scheme parses");
    let mut colors: Vec<(&'static str, String)> = ROLES
        .iter()
        .map(|r| {
            let v = base
                .get("colors")
                .and_then(|n| n.children())
                .and_then(|c| c.get_arg(r))
                .and_then(|v| v.as_string())
                .and_then(parse_color)
                .unwrap_or_else(|| "#ff00ffff".into());
            (*r, v)
        })
        .collect();
    match scheme_text(scheme).map(|t| t.parse::<KdlDocument>()) {
        Some(Ok(doc)) => {
            if let Some(children) = doc.get("colors").and_then(|n| n.children()) {
                for node in children.nodes() {
                    let role = node.name().value();
                    let Some(slot) = colors.iter_mut().find(|(r, _)| *r == role) else {
                        warnings.push(format!("{scheme}: unknown color {role}"));
                        continue;
                    };
                    match node
                        .get(0)
                        .and_then(|v| v.as_string())
                        .and_then(parse_color)
                    {
                        Some(c) => slot.1 = c,
                        None => warnings.push(format!("{scheme}: {role} is not a color")),
                    }
                }
            }
        }
        Some(Err(e)) => warnings.push(format!("{scheme}: {e}")),
        None => warnings.push(format!("no color scheme {scheme}")),
    }
    let mut st = Style::default();
    let mut apply = |key: &str, v: &KdlValue| match key {
        "radius" => st.radius = number(v).unwrap_or(st.radius),
        "gap" => st.gap = number(v).unwrap_or(st.gap),
        "padding" => st.padding = number(v).unwrap_or(st.padding),
        "font-family" => st.font_family = v.as_string().unwrap_or_default().to_string(),
        "font-small" => st.font_small = number(v).unwrap_or(st.font_small),
        "font-body" => st.font_body = number(v).unwrap_or(st.font_body),
        "font-title" => st.font_title = number(v).unwrap_or(st.font_title),
        "font-weight-display" => st.display_weight = number(v).unwrap_or(st.display_weight),
        "backdrop-opacity" => st.backdrop_opacity = number(v).unwrap_or(1.0),
        "card-opacity" => st.card_opacity = number(v).unwrap_or(1.0),
        _ => {}
    };
    match style_text(style).map(|t| t.parse::<KdlDocument>()) {
        Some(Ok(doc)) => {
            if let Some(children) = doc.get("style").and_then(|n| n.children()) {
                for node in children.nodes() {
                    if let Some(v) = node.get(0) {
                        apply(node.name().value(), v);
                    }
                }
            }
        }
        Some(Err(e)) => warnings.push(format!("{style}: {e}")),
        None => warnings.push(format!("no style {style}")),
    }
    for (k, v) in overrides {
        apply(k, v);
    }
    for (role, c) in &mut colors {
        match *role {
            "backdrop" => *c = fade(c, st.backdrop_opacity),
            "card" => *c = fade(c, st.card_opacity),
            _ => {}
        }
    }
    let light = luminance(&colors[1].1) > 0.5;
    (
        Look {
            colors,
            style: st,
            light,
        },
        warnings,
    )
}

/// The dashboard's stylesheet. Colors and sizes come from the custom properties [`Look::vars`]
/// sets on the root; Blender's own widgets (`Button`, `NumberSlider` …) keep their look.
pub const CSS: &str = r#"
html, body { height: 100%; }
body { margin: 0; color: var(--text); font-size: var(--font-body); }
.dash { position: fixed; left: 0; top: 0; right: 0; bottom: 0; background: var(--backdrop); overflow: hidden; }
.solo { position: fixed; left: 0; top: 0; right: 0; bottom: 0; background: var(--card); overflow: hidden; display: flex; flex-direction: column; }

/* A widget is a Blender panel: a header strip with its name, and the body under it. */
.card { position: absolute; background: var(--card); border: 1px solid var(--border);
        border-radius: var(--radius); display: flex; flex-direction: column; overflow: hidden; }
.card.round { border-radius: 9999px; }
.card.entered { border: 2px solid var(--accent); }
.card.picked { border: 2px solid var(--warn); }
.card.pinned > .head { color: var(--warn); }
.card > .head { background: var(--panel); padding: 2px 8px; font-size: var(--font-small);
                color: var(--dim); display: flex; gap: 6px; align-items: center; flex-shrink: 0;
                border-bottom: 1px solid var(--border); }
.card > .head .grow { flex: 1; }
.card > .body { padding: var(--pad); flex: 1; display: flex; flex-direction: column;
                gap: 5px; overflow: hidden; min-height: 0; }
.card.round > .body { align-items: center; justify-content: center; text-align: center; }
.solo > .body { padding: 12px; flex: 1; display: flex; flex-direction: column; gap: 6px; overflow: hidden; min-height: 0; }

.row { display: flex; align-items: center; gap: 6px; min-width: 0; }
.col { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
.grow { flex: 1; min-width: 0; }
.wrap { flex-wrap: wrap; }
.center { align-items: center; justify-content: center; text-align: center; }
.spread { justify-content: space-between; }
.scroll { overflow-y: auto; min-height: 0; flex: 1; }
.dim { color: var(--dim); }
.faint { color: var(--faint); }
.small { font-size: var(--font-small); }
.title { font-size: var(--font-title); font-weight: bold; }
.ellipsis { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.display { font-size: 44px; font-weight: var(--display-weight); line-height: 1.1; }
.display.s { font-size: 28px; }
.display.xs { font-size: 20px; }
.good { color: var(--good); }
.warn { color: var(--warn); }
.bad { color: var(--bad); }
.accent { color: var(--accent); }
.mono { font-family: monospace; }
.empty { color: var(--faint); font-style: italic; padding: 4px 0; }

/* A list row: hover like Blender's outliner, the keyboard's cursor in the accent color. */
.item { display: flex; align-items: center; gap: 6px; padding: 2px 6px; border-radius: 3px; min-height: 20px; }
.item:hover { background: var(--hover); }
.item.cursor { background: var(--accent-soft); outline: 1px solid var(--accent); }
.item.on { color: var(--accent); }
.item.header { color: var(--faint); font-size: var(--font-small); padding-top: 4px; }
.cursor { outline: 1px solid var(--accent); background: var(--accent-soft); border-radius: 3px; }
.ring { display: flex; border: 2px solid var(--accent); border-radius: 5px; padding: 1px; }
.ring > * { flex: 1; }
.icon { width: 16px; height: 16px; flex-shrink: 0; }
.icon.l { width: 32px; height: 32px; }
.art { width: 64px; height: 64px; border-radius: 3px; flex-shrink: 0; }
.dot { width: 8px; height: 8px; border-radius: 4px; background: var(--faint); flex-shrink: 0; }
.dot.on { background: var(--good); }
.dot.busy { background: var(--warn); }
.badge { background: var(--accent); color: var(--on-accent); border-radius: 8px; padding: 0 6px; font-size: var(--font-small); }

/* A level: Blender's progress bar look, drawn by hand so it can take any color. */
.meter { height: 6px; background: var(--track); border-radius: 3px; overflow: hidden; flex-shrink: 0; }
.meter > .fill { height: 100%; background: var(--accent); }
.meter.good > .fill { background: var(--good); }
.meter.warn > .fill { background: var(--warn); }
.meter.bad > .fill { background: var(--bad); }
.wide { width: 100%; }

/* The calendar */
.cal { display: grid; grid-template-columns: repeat(7, 1fr); gap: 1px; text-align: center; font-size: var(--font-small); }
.cal.weeks { grid-template-columns: repeat(8, 1fr); }
.cal > .d { padding: 1px 0; border-radius: 3px; }
.cal > .h { color: var(--faint); }
.cal > .other { color: var(--faint); }
.cal > .today { background: var(--accent); color: var(--on-accent); }
.cal > .sel { outline: 1px solid var(--accent); }
.cal > .wk { color: var(--faint); font-style: italic; }

/* The hint labels `f` puts on the widgets, Blender's warning yellow. */
.hint { position: absolute; background: var(--warn); color: #000; padding: 1px 7px; border-radius: 3px;
        font-weight: bold; font-family: monospace; font-size: 15px; border: 1px solid #000; }
.hint .typed { color: #00000060; }

/* The top bar and the quick tips: Blender's header and status bar. */
.topbar { position: absolute; left: 0; right: 0; top: 0; height: 30px; background: var(--panel);
          border-bottom: 1px solid var(--border); display: flex; align-items: center; gap: 8px; padding: 0 10px; }
.topbar .name { font-weight: bold; }
.tips { position: absolute; left: 0; right: 0; bottom: 0; z-index: 32; background: var(--panel); border-top: 1px solid var(--border);
        display: flex; flex-direction: column; padding: 3px 10px; gap: 2px; font-size: var(--font-small); color: var(--dim); }
.tips .line { display: flex; gap: 14px; flex-wrap: wrap; }
.tip { display: flex; gap: 5px; align-items: center; }
.cap { background: var(--card-raised); border: 1px solid var(--border); border-radius: 3px; padding: 0 5px;
       color: var(--text); font-family: monospace; }
.toast { position: absolute; left: 50%; bottom: 70px; transform: translateX(-50%); background: var(--card-raised);
         border: 1px solid var(--border); border-radius: 4px; padding: 6px 14px; max-width: 60%; }

/* Edit mode */
.gridcell { position: absolute; border: 1px dashed var(--subtle); }
.veil { position: absolute; left: 0; top: 0; right: 0; bottom: 0; background: #00000090; display: flex;
        flex-direction: column; align-items: center; justify-content: center; gap: 4px; color: #fff; }
.handle { position: absolute; background: var(--warn); }
.ghost { position: absolute; border: 2px dashed var(--accent); background: var(--accent-soft); border-radius: var(--radius); }
.ghost.bad { border-color: var(--bad); background: #d9534f30; }
.sidebar { position: absolute; top: 30px; right: 0; bottom: 0; width: 300px; background: var(--panel);
           border-left: 1px solid var(--border); display: flex; flex-direction: column; }
.tabs { display: flex; gap: 2px; padding: 4px; border-bottom: 1px solid var(--border); }
.tab { padding: 2px 10px; border-radius: 3px; color: var(--dim); }
.tab:hover { background: var(--hover); }
.tab.on { background: var(--accent); color: var(--on-accent); }
.pane { flex: 1; overflow-y: auto; padding: 8px; display: flex; flex-direction: column; gap: 4px; min-height: 0; }
.pane > *, .scroll > * { flex-shrink: 0; }
.group { color: var(--faint); font-size: var(--font-small); margin-top: 6px; }
.swatches { display: flex; gap: 2px; }
.swatch { width: 10px; height: 10px; border-radius: 2px; border: 1px solid #0006; }
.settings { position: absolute; background: var(--panel); border: 1px solid var(--border); border-radius: 6px;
            padding: 8px; display: flex; flex-direction: column; gap: 4px; width: 280px; }
.menu { position: absolute; background: var(--card-raised); border: 1px solid var(--border); border-radius: 4px;
        padding: 3px; display: flex; flex-direction: column; min-width: 180px; }
.expanded-shade { position: absolute; left: 0; top: 0; right: 0; bottom: 0; background: #000000a0; z-index: 30; }
"#;

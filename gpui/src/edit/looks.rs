//! The Colors and Style tabs: the schemes and presets on offer, each with a small preview.

use crate::dash::Dash;
use crate::theme::{self, Color, ColorRole};
use niri_dash_system::{Tell, host};

/// A color scheme in miniature.
#[derive(Debug, Clone)]
pub struct Scheme {
    pub name: String,
    pub description: String,
    pub current: bool,
    pub backdrop: Color,
    pub card: Color,
    pub accent: Color,
    pub text: Color,
    pub good: Color,
    pub bad: Color,
}

/// A style preset: the corners and gap it has, drawn small.
#[derive(Debug, Clone)]
pub struct StyleCard {
    pub name: String,
    pub description: String,
    pub current: bool,
    pub radius: f32,
    pub gap: f32,
    pub motion: f32,
}

impl Dash {
    /// Fill the Colors and Style tabs: every color scheme with a preview of its colors, every
    /// style with a preview of its corners.
    pub fn refresh_looks(&mut self) {
        let (cur_c, cur_s) = (self.layout.colorscheme.clone(), self.layout.style.clone());
        let mut schemes = theme::list(&theme::colorschemes_dir());
        if !schemes.contains(&cur_c) {
            schemes.push(cur_c.clone());
        }
        self.edit.schemes = schemes
            .into_iter()
            .map(|name| {
                let text = theme::colorscheme_text(&name).unwrap_or_default();
                let (look, _) = theme::resolve(&text, theme::DEFAULT_STYLE);
                Scheme {
                    current: name == cur_c,
                    description: theme::description(&text),
                    backdrop: look.color(ColorRole::Backdrop),
                    card: look.color(ColorRole::Card),
                    accent: look.color(ColorRole::Accent),
                    text: look.color(ColorRole::Text),
                    good: look.color(ColorRole::Good),
                    bad: look.color(ColorRole::Bad),
                    name,
                }
            })
            .collect();
        let mut styles = theme::list(&theme::styles_dir());
        if !styles.contains(&cur_s) {
            styles.push(cur_s.clone());
        }
        self.edit.styles = styles
            .into_iter()
            .map(|name| {
                let text = theme::style_text(&name).unwrap_or_default();
                let (look, _) = theme::resolve(theme::DEFAULT_COLORSCHEME, &text);
                StyleCard {
                    current: name == cur_s,
                    description: theme::description(&text),
                    radius: look.style.radius,
                    gap: look.style.gap,
                    motion: look.style.motion,
                    name,
                }
            })
            .collect();
        self.refresh_style_vars();
    }

    /// Open the folder the color schemes and styles live in (their parent).
    pub fn open_themes_folder(&mut self) {
        let dir = theme::colorschemes_dir();
        let dir = dir.parent().map_or(dir.clone(), |d| d.to_path_buf());
        self.tx.host.tell(host::Command::Open(dir));
    }
}

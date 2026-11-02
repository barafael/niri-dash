//! The color picker widget: picking a pixel through the compositor (niri or KWin), and the
//! recently picked colors.

use super::{App, model};
use crate::shell::Shell;
use crate::sources::Msg;
use crate::{Control, Dash};
use slint::ComponentHandle;
use std::cell::Cell;
use std::rc::Rc;

/// How many picked colors are kept.
const COLORS: usize = 8;

#[derive(Default)]
pub(super) struct Picker {
    /// Waiting for the dashboard to get out of the way before picking a color.
    pending: Cell<bool>,
    /// The dashboard was open when the pick started, so it comes back afterwards.
    reshow: Cell<bool>,
}

impl App {
    pub(super) fn color_picked(&self, rgb: Option<[f64; 3]>) -> Option<Control> {
        if let Some(rgb) = rgb {
            let hex = hex(rgb.map(to_u8));
            self.update_state(|s| {
                s.colors.retain(|c| *c != hex);
                s.colors.insert(0, hex.clone());
                s.colors.truncate(COLORS);
            });
            self.push_colors();
            self.copy_text(&hex);
        }
        if self.picker.reshow.take() {
            return Some(Control::Show);
        }
        None
    }

    /// Called every loop iteration: pick once the dashboard is out of the way.
    pub(super) fn tick_pick(&self, shell: &mut Shell) {
        if self.picker.pending.get() && !shell.is_visible() {
            self.picker.pending.set(false);
            self.run_pick();
        }
    }

    fn start_pick(&self) {
        if self.solo.is_some() {
            // A normal window doesn't cover the screen: pick right away.
            self.run_pick();
            return;
        }
        // The dashboard covers everything: hide it, pick once it's gone, then come back.
        self.picker.reshow.set(true);
        self.picker.pending.set(true);
        self.pending.borrow_mut().push(Control::Hide);
    }

    fn run_pick(&self) {
        let ctx = self.ctx.clone();
        self.ctx.spawn(async move {
            let rgb = match crate::compositor::pick_color(&ctx).await {
                Ok(rgb) => rgb,
                Err(e) => {
                    log::warn!("pick color: {e}");
                    ctx.toast(format!("Pick color: {e}"));
                    None
                }
            };
            ctx.send(Msg::ColorPicked(rgb));
        });
    }

    fn push_colors(&self) {
        let colors: Vec<crate::PickedColor> = self
            .state
            .borrow()
            .colors
            .iter()
            .filter_map(|h| parse_hex(h))
            .map(picked)
            .collect();
        self.ui()
            .global::<crate::ColorPicker>()
            .set_colors(model(colors));
    }
}

fn to_u8(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// "#rrggbb".
fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([b(0)?, b(2)?, b(4)?])
}

/// Hue (degrees), saturation and lightness (percent).
fn hsl([r, g, b]: [u8; 3]) -> (i32, i32, i32) {
    let (r, g, b) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0, 0, (l * 100.0).round() as i32);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (
        h.round() as i32 % 360,
        (s * 100.0).round() as i32,
        (l * 100.0).round() as i32,
    )
}

fn picked(c: [u8; 3]) -> crate::PickedColor {
    let (h, s, l) = hsl(c);
    crate::PickedColor {
        color: slint::Color::from_rgb_u8(c[0], c[1], c[2]),
        hex: hex(c).into(),
        rgb: format!("rgb({}, {}, {})", c[0], c[1], c[2]).into(),
        hsl: format!("hsl({h}, {s}%, {l}%)").into(),
    }
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    let cp = ui.global::<crate::ColorPicker>();
    let a = app.clone();
    cp.on_pick(move || a.start_pick());
    let a = app.clone();
    cp.on_copy(move |text| a.copy_text(&text));
    app.push_colors();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_formats() {
        assert_eq!(hex([1.0, 0.5, 0.0].map(to_u8)), "#ff8000");
        assert_eq!(parse_hex("#ff8000"), Some([255, 128, 0]));
        assert_eq!(parse_hex("ff8000"), None);
        assert_eq!(hsl([255, 0, 0]), (0, 100, 50));
        assert_eq!(hsl([0, 0, 255]), (240, 100, 50));
        assert_eq!(hsl([128, 128, 128]), (0, 0, 50));
    }
}

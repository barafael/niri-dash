//! The look: applying the configured color scheme and style, blending from one look to the next,
//! and the changes edit mode makes to them (a scheme, a style, a style variable).

use crate::dash::Dash;
use crate::layout::Layout;
use crate::theme::{self, StyleValue, StyleVar};
use kdl::KdlValue;
use std::time::{Duration, Instant};

/// A look being blended into another.
pub struct Blend {
    pub from: theme::Look,
    pub to: theme::Look,
    pub started: Instant,
    pub duration: Duration,
}

/// Make the text input and selection colors those of the look: inputs take them from the
/// component library's own theme, which knows nothing of ours. Called once per frame by the
/// dashboard's draw (never by a widget: two writers with different ideas would undo each other
/// forever), and it touches the global only when something differs, as a write notifies every
/// observer of the theme.
pub fn sync_component_theme(tk: &crate::kit::Tk, cx: &mut gpui::App) {
    use gpui_kit::component::Theme;
    // The caret is the text's colour, as in the Slint version.
    let want = (tk.text(), tk.faint(), tk.accent_soft(), tk.text());
    let have = {
        let c = &Theme::global(cx).colors;
        (c.foreground, c.muted_foreground, c.selection, c.caret)
    };
    if have == want {
        return;
    }
    Theme::update(cx, |theme| {
        theme.colors.foreground = want.0;
        theme.colors.muted_foreground = want.1;
        theme.colors.selection = want.2;
        theme.colors.caret = want.3;
    });
}

impl Dash {
    /// The look config.kdl asks for, with notes on anything missing or wrong.
    pub fn configured_look(&self) -> (theme::Look, Vec<String>) {
        let l = &self.layout;
        theme::load_look(&l.colorscheme, &l.style, &l.style_overrides)
    }

    /// Load the configured color scheme and style and apply them; with `animate`, blend from
    /// the current look over a third of a second (scaled by the new style's motion).
    pub fn apply_look(&mut self, animate: bool) {
        let (target, problems) = self.configured_look();
        self.watch_files();
        self.show_look(target, &problems, animate);
    }

    /// Tell the config watcher which files matter now: config.kdl and the files of the color
    /// scheme and the style it picks.
    pub fn watch_files(&self) {
        if let Some(watch) = &self.watch {
            let l = &self.layout;
            let _ = watch.send(vec![
                Layout::path(),
                theme::colorscheme_path(&l.colorscheme),
                theme::style_path(&l.style),
            ]);
        }
    }

    /// Show `target` (blending over to it with `animate`), toasting any `problems` loading it.
    pub fn show_look(&mut self, target: theme::Look, problems: &[String], animate: bool) {
        if !problems.is_empty() {
            self.toast(&format!("Theme: {}", problems.join("; ")));
        }
        let duration = Duration::from_millis((350.0 * target.style.motion) as u64);
        if !animate || duration.is_zero() || self.look == target {
            self.blend = None;
            self.look = target;
            return;
        }
        self.blend = Some(Blend {
            from: self.look.clone(),
            to: target,
            started: Instant::now(),
            duration,
        });
    }

    /// Where the blend has got to; true while it goes on, which asks for another frame.
    pub fn advance_look(&mut self) -> bool {
        let Some(b) = &self.blend else {
            return false;
        };
        let t = (b.started.elapsed().as_secs_f32() / b.duration.as_secs_f32()).min(1.0);
        if t >= 1.0 {
            self.look = b.to.clone();
            self.blend = None;
            return false;
        }
        let eased = if t < 0.5 {
            4.0 * t * t * t
        } else {
            1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
        };
        self.look = theme::mix(&b.from, &b.to, eased);
        true
    }

    pub fn choose_colorscheme(&mut self, name: &str) {
        self.layout.colorscheme = name.to_string();
        self.save_layout();
        self.apply_look(true);
        self.refresh_looks();
    }

    pub fn choose_style(&mut self, name: &str) {
        // Clicking the current preset changes nothing ("Reset all" is for dropping adjustments).
        if self.layout.style == name {
            return;
        }
        // The preset takes over what it changes; other adjustments (often the font, glass,
        // opacity) stay, since no preset has an opinion on them.
        let stext = theme::style_text(name).unwrap_or_else(|| theme::DEFAULT_STYLE.to_string());
        let taken = theme::preset_changes(&stext);
        self.layout.style = name.to_string();
        self.layout.style_overrides.retain(|(k, _)| {
            // Unknown names (typos in a hand-edited file) stay for the user to see.
            k.parse::<StyleVar>().map_or(true, |v| !taken.contains(&v))
        });
        self.save_layout();
        self.apply_look(true);
        self.refresh_looks();
    }

    /// Step style variable `var` up or down (`delta` is +1 or -1), or flip a switch.
    pub fn style_step(&mut self, var: StyleVar, delta: i32) {
        let (now, _) = self.configured_look();
        let (preset, _) = theme::load_look(&self.layout.colorscheme, &self.layout.style, &[]);
        let value: KdlValue = match (var, now.style.get(var)) {
            (StyleVar::FontFamily, StyleValue::Text(cur)) => {
                let fonts = self.fonts();
                let i = fonts
                    .iter()
                    .position(|f| *f == cur.as_ref())
                    .map_or(0, |i| i as i32);
                let j = (i + delta).clamp(0, fonts.len() as i32 - 1) as usize;
                match fonts.get(j) {
                    Some(f) => KdlValue::String(f.clone()),
                    None => return,
                }
            }
            (_, StyleValue::Flag(on)) => KdlValue::Bool(!on),
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
                        StyleVar::Motion | StyleVar::BackdropOpacity | StyleVar::CardOpacity
                    )
                {
                    KdlValue::Integer(v as i128)
                } else {
                    KdlValue::Float((v as f64 * 100.0).round() / 100.0)
                }
            }
        };
        // A value equal to the preset's drops the adjustment again.
        let mut probe = preset.style.clone();
        let same_as_preset = probe.set(var, &value).is_ok() && probe == preset.style;
        let key = var.as_ref().to_string();
        let over = &mut self.layout.style_overrides;
        match over.iter().position(|(k, _)| *k == key) {
            Some(i) if same_as_preset => {
                over.remove(i);
            }
            Some(i) => over[i].1 = value,
            None if same_as_preset => {}
            None => over.push((key, value)),
        }
        self.save_layout();
        self.apply_look(true);
        self.refresh_style_vars();
    }

    /// Drop the adjustment of one style variable, or all of them.
    pub fn style_reset(&mut self, var: Option<StyleVar>) {
        match var {
            None => self.layout.style_overrides.clear(),
            Some(v) => self.layout.style_overrides.retain(|(k, _)| k != v.as_ref()),
        }
        self.save_layout();
        self.apply_look(true);
        self.refresh_style_vars();
    }

    /// The font families the Style tab offers (looked up on first use).
    pub fn fonts(&mut self) -> Vec<String> {
        self.fonts.get_or_insert_with(theme::font_choices).clone()
    }
}

//! The theme: applying the configured look, and the Colors and Style tabs.

use super::enums::{style_var_from_ui, style_var_to_ui};
use super::{App, model};
use crate::layout::Layout;
use crate::{Dash, theme};
use niri_dash_system::{Tell, host};
use slint::TimerMode;
use std::rc::Rc;
use std::time::Duration;
use web_time::Instant;

impl App {
    /// Fill the Look tab: every color scheme with a preview of its colors, every style with a
    /// preview of its corners.
    pub(super) fn refresh_looks(&self) {
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
            key: style_var_to_ui(theme::StyleVar::Glass),
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
                key: style_var_to_ui(k.var),
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
            key: style_var_to_ui(theme::StyleVar::FontFamily),
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

    /// Load the configured color scheme and style and apply them; with `animate`, blend from
    /// the current look over a third of a second (scaled by the new style's motion).
    pub(super) fn apply_look(&self, animate: bool) {
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
    pub(super) fn show_look(&self, target: theme::Look, problems: &[String], animate: bool) {
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
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
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
    ui.on_style_step(move |key, delta| a.style_step(style_var_from_ui(key), delta));
    let a = app.clone();
    ui.on_style_reset(move |key| a.style_reset(Some(style_var_from_ui(key))));
    let a = app.clone();
    ui.on_style_reset_all(move || a.style_reset(None));
    let a = app.clone();
    ui.on_open_themes_folder(move || {
        let dir = theme::colorschemes_dir();
        let dir = dir.parent().map_or(dir.clone(), |d| d.to_path_buf());
        a.tx.host.tell(host::Command::Open(dir));
    });
}

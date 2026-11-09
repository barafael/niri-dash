//! The settings of the widget being worked on (the card in the middle of the screen), the rows of
//! the Style tab, and the card's text fields.

use crate::dash::Dash;
use crate::layout::Kind;
use crate::theme::{self, StyleVar};
use gpui::{App, AppContext as _, Context, Entity, Focusable, Window};
use gpui_kit::component::input::InputState;
use kdl::KdlValue;
use niri_dash_system::{Tell, weather};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Stepper,
    Toggle,
    Text,
}

/// One setting of the widget open in the card (or of the grid, or of Arrange): `index` is its
/// position in the list the owner keeps.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingRow {
    pub index: usize,
    pub kind: RowKind,
    pub label: String,
    pub value: String,
    pub on: bool,
    /// Set to something other than the default.
    pub changed: bool,
    pub can_dec: bool,
    pub can_inc: bool,
    pub placeholder: String,
}

/// One row of the Style tab.
#[derive(Debug, Clone, PartialEq)]
pub struct StyleRow {
    pub var: StyleVar,
    pub kind: RowKind,
    pub label: String,
    pub value: String,
    pub on: bool,
    pub overridden: bool,
    pub can_dec: bool,
    pub can_inc: bool,
}

/// What the keyboard is asked to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Want {
    /// Back to the dashboard's own keys.
    Dash,
    /// Into the text field of the card's row `n`.
    Field(usize),
}

/// The card's text fields. Their keys are theirs while they have the keyboard; the dashboard
/// only watches for the ways out.
#[derive(Default)]
pub struct Fields {
    states: HashMap<(u32, usize), Entity<InputState>>,
    /// A field has the keyboard (as of the last key).
    pub focused: bool,
    /// The row whose field has it.
    pub row: Option<usize>,
    pub want: Option<Want>,
}

impl Fields {
    /// The keyboard goes back to the dashboard.
    pub fn leave(&mut self) {
        if self.focused || self.want.is_some() {
            self.want = Some(Want::Dash);
        }
        self.focused = false;
        self.row = None;
    }

    /// The keyboard goes into the field of row `n`.
    pub fn enter(&mut self, n: usize) {
        self.want = Some(Want::Field(n));
    }

    pub fn state(&self, id: u32, row: usize) -> Option<&Entity<InputState>> {
        self.states.get(&(id, row))
    }

    /// Whether one of the fields has the keyboard now.
    pub fn has_focus(&self, window: &Window, cx: &App) -> bool {
        self.states
            .values()
            .any(|s| s.focus_handle(cx).is_focused(window))
    }
}

impl Dash {
    /// The kind of widget being worked on.
    pub fn picked_kind(&self) -> Option<Kind> {
        self.layout.get(self.edit.settings_id).map(|p| p.kind)
    }

    /// The name of the widget being worked on.
    pub fn settings_title(&self) -> String {
        self.picked_kind()
            .map(|k| k.info().title.to_string())
            .unwrap_or_default()
    }

    /// The widget being worked on is pinned.
    pub fn settings_pinned(&self) -> bool {
        self.edit.settings_id != 0 && self.layout.is_pinned(self.edit.settings_id)
    }

    /// The rows of the card: the settings of the widget being worked on, defaults filled in.
    pub fn setting_rows(&self) -> Vec<SettingRow> {
        use crate::widget_settings::{Ty, get};
        let us = self.locale.us;
        let Some(p) = (self.edit.settings_id != 0)
            .then(|| self.layout.get(self.edit.settings_id))
            .flatten()
        else {
            return Vec::new();
        };
        p.kind
            .info()
            .settings
            .iter()
            .enumerate()
            .map(|(i, &opt)| {
                let v = get(&p.settings, opt, us);
                let (kind, placeholder) = match opt.ty() {
                    Ty::Toggle => (RowKind::Toggle, ""),
                    Ty::Steps(_) | Ty::Choice(_) => (RowKind::Stepper, ""),
                    Ty::Text { placeholder } => (RowKind::Text, placeholder),
                };
                SettingRow {
                    index: i,
                    kind,
                    label: opt.label().to_string(),
                    value: match &v {
                        KdlValue::String(s) if kind == RowKind::Text => s.clone(),
                        v => opt.value_text(v),
                    },
                    on: v == KdlValue::Bool(true),
                    changed: p.settings.iter().any(|(k, _)| k == opt.as_ref()),
                    can_dec: opt.can_step(&v, -1),
                    can_inc: opt.can_step(&v, 1),
                    placeholder: placeholder.to_string(),
                }
            })
            .collect()
    }

    /// The settings card is open: the widget being worked on has settings to show.
    pub fn cfg_open(&self) -> bool {
        self.editing
            && self.edit.settings_id != 0
            && self
                .layout
                .get(self.edit.settings_id)
                .is_some_and(|p| !p.kind.info().settings.is_empty())
    }

    /// Work on widget `id` (0: let go): the cursor goes to the card's first row.
    pub fn set_settings_id(&mut self, id: u32) {
        if self.edit.settings_id != id {
            self.edit.settings_id = id;
            // Other rows: start at the top, and a text field of the one that went lets go.
            self.edit.side_cursor = 0;
            self.edit.size_note.clear();
            self.edit.field.leave();
        }
    }

    /// Open a widget's settings card, entering edit mode if needed.
    pub fn configure_widget(&mut self, id: u32, cx: &mut Context<Self>) {
        if !self.editing {
            self.set_editing(true, cx);
        }
        // A widget that is gone has no card.
        let id = if self.layout.get(id).is_some() { id } else { 0 };
        self.set_settings_id(id);
    }

    /// `s` in a widget: its settings, or a word that it has none.
    pub fn open_settings(&mut self, id: u32, cx: &mut Context<Self>) {
        let (title, any) = {
            let Some(p) = self.layout.get(id) else {
                return;
            };
            let info = p.kind.info();
            (info.title, !info.settings.is_empty())
        };
        if any {
            self.configure_widget(id, cx);
        } else {
            self.toast(&format!("{title} has no settings"));
        }
    }

    /// The card closes when its widget is gone.
    pub fn tidy_settings(&mut self) {
        if self.edit.settings_id != 0 && self.layout.get(self.edit.settings_id).is_none() {
            self.set_settings_id(0);
        }
    }

    /// Change setting number `index` of the open widget with `f(current value)`; `None` resets it.
    pub fn change_setting(
        &mut self,
        index: usize,
        f: impl FnOnce(crate::widget_settings::Opt, &KdlValue) -> Option<KdlValue>,
    ) {
        use crate::widget_settings::{get, set};
        let id = self.edit.settings_id;
        let us = self.locale.us;
        let kind = {
            let Some(kind) = self.layout.get(id).map(|p| p.kind) else {
                return;
            };
            let Some(&opt) = kind.info().settings.get(index) else {
                return;
            };
            let Some(values) = self.layout.settings_mut(id) else {
                return;
            };
            let cur = get(values, opt, us);
            let new = f(opt, &cur).unwrap_or_else(|| opt.default(us));
            set(values, opt, new, us);
            kind
        };
        self.layout_changed();
        if kind == Kind::Weather {
            let config = self.layout.weather_config(self.locale.us);
            self.tx.weather.tell(weather::Command::Configure(config));
        }
    }

    pub fn setting_step(&mut self, index: usize, delta: i32) {
        self.change_setting(index, |opt, cur| Some(opt.step(cur, delta)));
    }

    pub fn setting_reset(&mut self, index: usize) {
        self.change_setting(index, |_, _| None);
    }

    /// A text setting was typed in and accepted.
    pub fn setting_text(&mut self, index: usize, text: &str) {
        let text = text.trim().to_string();
        let mut complaint = None;
        self.change_setting(index, |opt, _| {
            use crate::widget_settings::Opt;
            match opt {
                Opt::Zones => complaint = crate::zones::complaint(&text),
                Opt::Cowfile => complaint = crate::cowfile::complaint(&text),
                _ => {}
            }
            (!text.is_empty()).then_some(KdlValue::String(text))
        });
        // Kept as typed, to fix; the clock leaves the unknown ones out.
        if let Some(c) = complaint {
            self.toast(&c);
        }
    }

    // ---- the card's text fields ---------------------------------------------------------------

    /// Make the text fields of the card's rows, and show each the value the layout has (a field
    /// that lost the keyboard without Enter drops what was typed).
    pub fn prepare_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.cfg_open() {
            return;
        }
        let id = self.edit.settings_id;
        for row in self.setting_rows() {
            if row.kind != RowKind::Text {
                continue;
            }
            let state = self
                .edit
                .field
                .states
                .entry((id, row.index))
                .or_insert_with(|| {
                    let placeholder = row.placeholder.clone();
                    cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
                })
                .clone();
            let focused = state.focus_handle(cx).is_focused(window);
            if !focused && state.read(cx).value().as_ref() != row.value {
                let value = row.value.clone();
                state.update(cx, |s, cx| s.set_value(value, window, cx));
            }
        }
    }

    /// Put the keyboard where the last key asked for it. Called by whoever has the window: the
    /// key and click listeners, and the draw.
    pub fn sync_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.edit.field.want.take() {
            None => {}
            Some(Want::Dash) => {
                window.focus(&self.focus, cx);
                self.edit.field.focused = false;
            }
            Some(Want::Field(n)) => {
                let id = self.edit.settings_id;
                if let Some(state) = self.edit.field.states.get(&(id, n)).cloned() {
                    state.update(cx, |s, cx| s.focus(window, cx));
                    self.edit.field.focused = true;
                    self.edit.field.row = Some(n);
                }
            }
        }
    }

    /// Whether a text field has the keyboard, as the window sees it.
    pub fn note_field_focus(&mut self, window: &Window, cx: &App) {
        let on = self.edit.field.has_focus(window, cx);
        self.edit.field.focused = on;
        if !on {
            self.edit.field.row = None;
        }
    }

    // ---- the Style tab's rows -----------------------------------------------------------------

    /// The Style tab's rows: every variable with its current value (preset + adjustments).
    pub fn refresh_style_vars(&mut self) {
        let (look, _) = self.configured_look();
        let over = self.layout.style_overrides.clone();
        let overridden = |var: StyleVar| over.iter().any(|(k, _)| k == var.as_ref());
        let num = |var: StyleVar| look.style.get(var).as_f32().unwrap_or(0.0);
        let mut rows: Vec<StyleRow> = vec![StyleRow {
            var: StyleVar::Glass,
            kind: RowKind::Toggle,
            label: "Frosted glass".into(),
            value: if look.style.glass { "on" } else { "off" }.into(),
            on: look.style.glass,
            overridden: overridden(StyleVar::Glass),
            can_dec: false,
            can_inc: false,
        }];
        rows.extend(theme::KNOBS.iter().map(|k| {
            let v = num(k.var);
            StyleRow {
                var: k.var,
                kind: RowKind::Stepper,
                label: k.label.into(),
                value: theme::knob_text(k.var, v),
                on: false,
                overridden: overridden(k.var),
                can_dec: theme::step(k, v, -1) != v,
                can_inc: theme::step(k, v, 1) != v,
            }
        }));
        let font = look.style.font_family.to_string();
        let fonts = self.fonts();
        let fi = fonts.iter().position(|f| *f == font);
        rows.push(StyleRow {
            var: StyleVar::FontFamily,
            kind: RowKind::Stepper,
            label: "Font".into(),
            value: if font.is_empty() {
                "System".into()
            } else {
                font.clone()
            },
            on: false,
            overridden: overridden(StyleVar::FontFamily),
            can_dec: fi.is_none_or(|i| i > 0),
            can_inc: fi.is_none_or(|i| i + 1 < fonts.len()),
        });
        self.edit.style_rows = rows;
        self.edit.style_customized = !over.is_empty();
    }
}

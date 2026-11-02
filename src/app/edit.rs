//! Edit mode: moving, resizing, adding and removing widgets, and the widget settings tab.

use super::enums::{character_to_ui, kind_from_ui, kind_to_ui, shape_to_ui};
use super::{App, model, set_if_changed};
use crate::layout::{Kind, Placed};
use crate::{Dash, PaletteItem, WidgetInst};
use slint::Model;
use std::rc::Rc;
use strum::IntoEnumIterator;

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
pub(super) fn character(p: &Placed, us: bool) -> crate::mascots::Character {
    use crate::widget_settings::{Opt, get_str};
    get_str(&p.settings, Opt::Character, us)
        .parse()
        .expect("the setting only takes character names")
}

fn to_inst(p: &Placed, us: bool) -> WidgetInst {
    WidgetInst {
        has_settings: !crate::widget_settings::for_kind(p.kind).is_empty(),
        opts: to_opts(p, us),
        can_round: p.kind.can_be_round(),
        id: p.id as i32,
        kind: kind_to_ui(p.kind),
        shape: shape_to_ui(p.shape),
        col: p.col,
        row: p.row,
        w: p.w,
        h: p.h,
    }
}

impl App {
    pub(super) fn sync_widgets(&self) {
        self.sync_mascots();
        self.sync_notes(false);
        if let Some(kind) = self.solo {
            let inst = to_inst(&self.layout.borrow().solo_widget(kind), self.locale.us);
            self.ui().set_solo_inst(inst);
            return;
        }
        self.ui()
            .set_hidden_widgets(self.layout.borrow().hidden_count() as i32);
        let preview = self.preview.borrow();
        let us = self.locale.us;
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
            set_if_changed(&self.widgets, new, |a, b| a == b);
        } else {
            self.widgets.set_vec(new);
        }
    }

    /// The widgets of `kind` on show: the standalone app's own, or the visible ones on the
    /// dashboard.
    pub(super) fn placed_of(&self, kind: Kind) -> Vec<Placed> {
        let layout = self.layout.borrow();
        match self.solo {
            Some(k) if k == kind => vec![layout.solo_widget(kind)],
            Some(_) => Vec::new(),
            None => layout
                .visible()
                .into_iter()
                .filter(|p| p.kind == kind)
                .cloned()
                .collect(),
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

    pub(super) fn sync_grid(&self) {
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

    pub(super) fn set_editing(&self, on: bool) {
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

    /// Open a widget's settings in the sidebar (entering edit mode if needed).
    pub(super) fn configure_widget(&self, id: u32) {
        let ui = self.ui();
        if !ui.get_editing() {
            self.set_editing(true);
        }
        ui.set_settings_id(id as i32);
        ui.set_drawer_tab(crate::DrawerTab::Widget);
        self.refresh_settings();
    }

    /// Rebuild the sidebar rows for the widget whose settings are open.
    pub(super) fn refresh_settings(&self) {
        use crate::widget_settings::{Ty, for_kind, get};
        let ui = self.ui();
        let id = ui.get_settings_id();
        let us = self.locale.us;
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
        let us = self.locale.us;
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
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
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
}

//! Edit mode: moving, resizing, adding and removing widgets, and the widget settings tab.

use super::enums::{character_to_ui, kind_from_ui, kind_to_ui, shape_to_ui};
use super::{App, model, set_if_changed};
use crate::layout::{Kind, Placed};
use crate::nav::Dir;
use crate::{Dash, PaletteItem, WidgetInst};
use niri_dash_system::{Capabilities, Compositor, Tell, weather};
use slint::{ComponentHandle, Model};
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
        character: character_to_ui(character(p, us)),
        cowfile: get_str(v, Opt::Cowfile, us).into(),
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
        has_settings: !p.kind.info().settings.is_empty(),
        opts: to_opts(p, us),
        can_round: p.kind.info().can_be_round,
        id: p.id as i32,
        kind: kind_to_ui(p.kind),
        title: p.kind.info().title.into(),
        shape: shape_to_ui(p.shape),
        col: p.col,
        row: p.row,
        w: p.w,
        h: p.h,
        min_w: p.min_size().0,
        min_h: p.min_size().1,
        hint: Default::default(),
    }
}

impl App {
    pub(super) fn sync_widgets(&self) {
        self.sync_mascots();
        self.sync_sleep();
        self.sync_notes(false);
        self.sync_history();
        if let Some(kind) = self.solo {
            let inst = to_inst(&self.layout.borrow().solo_widget(kind), self.locale.us);
            self.ui().set_solo_inst(inst);
            return;
        }
        self.ui()
            .set_hidden_widgets(self.layout.borrow().hidden_count() as i32);
        let preview = self.preview.borrow();
        let us = self.locale.us;
        let mut new: Vec<WidgetInst> = self
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
        let ids: Vec<(u32, Kind)> = new
            .iter()
            .map(|w| (w.id as u32, kind_from_ui(w.kind)))
            .collect();
        let hints = crate::nav::assign_hints(&ids);
        if *self.hints.borrow() != hints {
            let chars = hints.iter().map(|h| crate::HintChars {
                chars: model(h.chars().map(|c| c.to_string().into()).collect()),
            });
            self.ui()
                .global::<crate::Nav>()
                .set_hint_chars(model(chars.collect()));
            *self.hints.borrow_mut() = hints.clone();
        }
        for (w, hint) in new.iter_mut().zip(hints) {
            w.hint = hint.into();
        }
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

    /// Put widget `id` at `target` from the keyboard, pushing others aside (`prefer`: the way
    /// it went).
    fn place_by_key(&self, id: u32, target: crate::layout::Rect, prefer: (i32, i32)) {
        self.set_preview(None);
        let placed = self.layout.borrow_mut().place_reflow(id, target, prefer);
        if placed {
            self.layout_changed();
        } else {
            self.toast("No room there");
        }
    }

    /// Ctrl+Shift+h/j/k/l: move widget `id` one cell that way.
    fn nudge(&self, id: u32, dir: Dir) {
        let (dc, dr) = dir.step();
        let Some(r) = self.layout.borrow().get(id).map(Placed::rect) else {
            return;
        };
        let target = crate::layout::Rect {
            col: r.col + dc,
            row: r.row + dr,
            ..r
        };
        self.place_by_key(id, target, (dc, dr));
    }

    /// The keyboard's sizing mode: push widget `id`'s `edge` out a cell, or pull it in.
    fn reshape(&self, id: u32, edge: Dir, grow: bool) {
        let target = {
            let l = self.layout.borrow();
            let Some(p) = l.get(id) else { return };
            crate::nav::resized(
                p.rect(),
                edge,
                grow,
                p.shape == crate::layout::Shape::Round,
                p.min_size(),
            )
        };
        // At its smallest, shrinking does nothing.
        if let Some(target) = target {
            self.place_by_key(id, target, edge.step());
        }
    }

    /// The main direction of a move, used to prefer pushing others that way.
    fn direction(&self, id: u32, col: i32, row: i32) -> (i32, i32) {
        let Some((from_col, from_row)) = self.layout.borrow().get(id).map(|p| (p.col, p.row))
        else {
            return (0, 1);
        };
        let (dc, dr) = (col - from_col, row - from_row);
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

    fn preview_resize(&self, id: u32, target: crate::layout::Rect) -> bool {
        let result = self.layout.borrow().plan_resize(id, target);
        let ok = result.is_some();
        self.set_preview(result.map(|(_, moves)| (id, moves)));
        ok
    }

    fn preview_add(&self, kind: Kind, col: i32, row: i32) -> bool {
        let (w, h) = kind.info().default_size;
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
            self.layout.borrow_mut().set_screen_grid(cols, rows);
            self.sync_widgets();
            self.refresh_grid_controls();
        }
    }

    /// Tell the UI about an explicit grid (0 x 0: the automatic one).
    pub(super) fn apply_grid(&self) {
        let (cols, rows) = self.layout.borrow().grid.unwrap_or((0, 0));
        let ui = self.ui();
        ui.set_grid_cols(cols);
        ui.set_grid_rows(rows);
        self.sync_grid();
        self.refresh_grid_controls();
    }

    /// The Columns and Rows steppers in the Widgets tab.
    fn refresh_grid_controls(&self) {
        use crate::layout::{COLUMNS, ROWS};
        let l = self.layout.borrow();
        let row =
            |index, label: &str, n: i32, range: &std::ops::RangeInclusive<i32>| crate::SettingRow {
                index,
                kind: crate::RowKind::Stepper,
                label: label.into(),
                value: n.to_string().into(),
                on: false,
                changed: l.grid.is_some(),
                can_dec: n > *range.start(),
                can_inc: n < *range.end(),
                placeholder: Default::default(),
            };
        let rows = vec![
            row(0, "Columns", l.cols, &COLUMNS),
            row(1, "Rows", l.rows, &ROWS),
        ];
        drop(l);
        set_if_changed(&self.grid_controls, rows, |a, b| a == b);
    }

    /// Change the grid from edit mode: `Some` makes it explicit, fitting the widgets into it;
    /// `None` goes back to the automatic one.
    fn change_grid(&self, grid: Option<(i32, i32)>) {
        let refit = self.layout.borrow_mut().set_grid(grid);
        self.apply_grid();
        self.layout_changed();
        self.refresh_settings();
        if let Some(m) = refit.message() {
            self.toast(&m);
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

    /// Open a widget's settings card (entering edit mode if needed).
    pub(super) fn configure_widget(&self, id: u32) {
        let ui = self.ui();
        if !ui.get_editing() {
            self.set_editing(true);
        }
        ui.set_settings_id(id as i32);
        self.refresh_settings();
    }

    /// `s` in a widget: open its settings, or say it has none.
    fn open_settings(&self, id: u32) {
        let (title, any) = {
            let layout = self.layout.borrow();
            let Some(p) = layout.get(id) else { return };
            let info = p.kind.info();
            (info.title, !info.settings.is_empty())
        };
        if any {
            self.configure_widget(id);
        } else {
            self.toast(&format!("{title} has no settings"));
        }
    }

    /// Rebuild the rows of the card for the widget whose settings are open.
    pub(super) fn refresh_settings(&self) {
        use crate::widget_settings::{Ty, get};
        let ui = self.ui();
        let id = ui.get_settings_id();
        let us = self.locale.us;
        let layout = self.layout.borrow();
        let Some(p) = (id != 0).then(|| layout.get(id as u32)).flatten() else {
            if id != 0 {
                ui.set_settings_id(0);
            }
            return;
        };
        ui.set_settings_title(p.kind.info().title.into());
        let rows: Vec<crate::SettingRow> = p
            .kind
            .info()
            .settings
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
        use crate::widget_settings::{get, set};
        let id = self.ui().get_settings_id() as u32;
        let us = self.locale.us;
        let kind = {
            let mut layout = self.layout.borrow_mut();
            let Some(kind) = layout.get(id).map(|p| p.kind) else {
                return;
            };
            let Some(&opt) = kind.info().settings.get(index as usize) else {
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
            let config = self.layout.borrow().weather_config(self.locale.us);
            self.tx.weather.tell(weather::Command::Configure(config));
        }
    }
}

/// How a widget's description reads here, where the compositor lacks what it needs (None: as
/// usual), and whether it can't work here at all.
fn caveat(caps: Capabilities, kind: Kind) -> Option<(&'static str, bool)> {
    use Compositor::*;
    match (caps.compositor, kind) {
        (Niri, _) => None,
        (_, Kind::Windows) => Some(("Needs niri: open windows by workspace", true)),
        (_, Kind::ClaudeSessions) => Some(("Running Claude Code sessions", false)),
        (_, Kind::Privacy) => Some((
            "Who uses the microphone or camera (screen sharing needs niri)",
            false,
        )),
        (Kwin, Kind::Notifications) => Some(("Pause Plasma's notification pop-ups", false)),
        (Kwin, Kind::NightLight) => Some(("Pause or resume Plasma's night light", false)),
        (Other, Kind::KeyboardLayout | Kind::ColorPicker) => Some(("Needs niri or Plasma", true)),
        _ => None,
    }
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    let a = app.clone();
    ui.on_set_editing(move |on| a.set_editing(on));
    // Live reflow: dragging shows where everything would go; dropping commits it.
    let a = app.clone();
    ui.on_preview_move(move |id, c, r| a.preview_move(id as u32, c, r));
    let a = app.clone();
    ui.on_preview_resize(move |id, col, row, w, h| {
        a.preview_resize(id as u32, crate::layout::Rect { col, row, w, h })
    });
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
    ui.on_nudge_widget(move |id, dir| {
        if let Some(dir) = Dir::from_index(dir) {
            a.nudge(id as u32, dir);
        }
    });
    let a = app.clone();
    ui.on_reshape_widget(move |id, edge, grow| {
        if let Some(edge) = Dir::from_index(edge) {
            a.reshape(id as u32, edge, grow);
        }
    });
    let a = app.clone();
    ui.on_resize_widget(move |id, col, row, w, h| {
        a.set_preview(None);
        let target = crate::layout::Rect { col, row, w, h };
        if a.layout.borrow_mut().resize_reflow(id as u32, target) {
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
            let round = a
                .layout
                .borrow()
                .get(id as u32)
                .is_some_and(|p| p.shape == crate::layout::Shape::Round);
            a.toast(if round {
                "No room to make this widget rectangular here"
            } else {
                "No room to make this widget round here"
            });
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
    ui.on_open_settings(move |id| a.open_settings(id as u32));
    let a = app.clone();
    ui.on_setting_step(move |index, delta| {
        a.change_setting(index, |opt, cur| Some(opt.step(cur, delta)))
    });
    let a = app.clone();
    ui.on_setting_text(move |index, text| {
        let text = text.trim().to_string();
        let mut complaint = None;
        a.change_setting(index, |opt, _| {
            use crate::widget_settings::Opt;
            match opt {
                Opt::Zones => complaint = crate::zones::complaint(&text),
                Opt::Cowfile => complaint = crate::cowfile::complaint(&text),
                _ => {}
            }
            (!text.is_empty()).then_some(kdl::KdlValue::String(text))
        });
        // Kept as typed, to fix; the clock leaves the unknown ones out.
        if let Some(c) = complaint {
            a.toast(&c);
        }
    });
    let a = app.clone();
    ui.on_setting_reset(move |index| a.change_setting(index, |_, _| None));
    let a = app.clone();
    ui.on_add_widget(move |k| {
        let kind = kind_from_ui(k);
        if a.layout.borrow_mut().add(kind).is_some() {
            a.layout_changed();
            a.toast(&format!("Added {}", kind.info().title));
        } else {
            a.toast("No free space. Remove or shrink a widget first.");
        }
    });
    let a = app.clone();
    ui.on_reset_layout(move || {
        let refit = a.layout.borrow_mut().reset();
        a.layout_changed();
        a.refresh_settings();
        if let Some(m) = refit.message() {
            a.toast(&m);
        }
    });
    // The grid steppers: index 0 is the columns, 1 the rows. The first step makes the grid
    // explicit, starting from what fits the screen.
    // A step that would leave no room for some widget is refused: the stepper shakes and says
    // which ones, and nothing changes. Edit mode never deletes widgets on its own.
    let a = app.clone();
    ui.on_grid_step(move |index, delta| {
        let (cols, rows) = {
            let l = a.layout.borrow();
            (l.cols, l.rows)
        };
        let grid = match index {
            0 => (cols + delta, rows),
            _ => (cols, rows + delta),
        };
        let shake = || {
            let ui = a.ui();
            if index == 0 {
                ui.set_grid_refusals_cols(ui.get_grid_refusals_cols() + 1);
            } else {
                ui.set_grid_refusals_rows(ui.get_grid_refusals_rows() + 1);
            }
        };
        // Past the steppers' ends (the keyboard can try; the buttons are greyed out).
        if !crate::layout::COLUMNS.contains(&grid.0) || !crate::layout::ROWS.contains(&grid.1) {
            shake();
            return;
        }
        let lost = {
            let mut trial = a.layout.borrow().clone();
            trial.set_grid(Some(grid)).evicted
        };
        if !lost.is_empty() {
            let mut names: Vec<&str> = lost.iter().map(|p| p.kind.info().title).collect();
            names.dedup();
            shake();
            a.toast(&format!(
                "No room for {} on a {}\u{d7}{} grid. Shrink or remove widgets first.",
                names.join(", "),
                grid.0,
                grid.1
            ));
            return;
        }
        a.change_grid(Some(grid));
    });
    let a = app.clone();
    ui.on_grid_auto(move || a.change_grid(None));
    // Widgets that can't work under this compositor go last, saying what they need.
    let mut kinds: Vec<Kind> = Kind::iter().collect();
    let caps = app.caps;
    kinds.sort_by_key(|&k| caveat(caps, k).is_some_and(|(_, out)| out));
    ui.set_palette(model(
        kinds
            .into_iter()
            .map(|k| {
                let info = k.info();
                let caveat = caveat(caps, k).map(|(text, _)| text);
                PaletteItem {
                    kind: kind_to_ui(k),
                    title: info.title.into(),
                    description: caveat.unwrap_or(info.description).into(),
                    w: info.new_size().0,
                    h: info.new_size().1,
                }
            })
            .collect(),
    ));
}

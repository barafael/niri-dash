//! What the edit mode tests (`tests/edit.rs`) look at beyond the keys and the mouse: the layout as
//! the dashboard holds it, and whether it agrees with what the views show, what config.kdl says,
//! and the rules a layout keeps (inside the grid, no overlaps, round widgets square, sizes at
//! least the minimum, settings the sidebar can show); and the pointer as a person uses it.

use super::{DrawerTab, RowKind, SettingRow};
use crate::dash::{Dash, NavMode, NavOp};
use crate::layout::{Kind, Layout, Shape};
use crate::testing::Harness;
use gpui::{Modifiers, MouseButton, MouseDownEvent, Pixels, Point, point, px};
use std::collections::HashSet;
use std::time::Duration;
use strum::IntoEnumIterator;

/// One widget of the layout, visible or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub id: u32,
    /// The name config.kdl uses.
    pub kind: String,
    pub round: bool,
    pub pinned: bool,
    pub col: i32,
    pub row: i32,
    pub w: i32,
    pub h: i32,
    /// Shown on the current grid.
    pub visible: bool,
    /// As stored: (name, value as KDL).
    pub settings: Vec<(String, String)>,
}

/// A tile without its id, to compare with one read back from the file.
type ShapeKey = (String, bool, bool, i32, i32, i32, i32, Vec<(String, String)>);

impl Tile {
    /// Where it is and what it is, without the id (which a reload hands out anew).
    fn shape_key(&self) -> ShapeKey {
        (
            self.kind.clone(),
            self.round,
            self.pinned,
            self.col,
            self.row,
            self.w,
            self.h,
            self.settings.clone(),
        )
    }
}

/// Every widget kind's config name, in the palette's order.
pub fn kind_names() -> Vec<&'static str> {
    Kind::iter().map(Kind::name).collect()
}

/// The setting labels a kind has, in the sidebar's order.
pub fn setting_labels(kind: &str) -> Vec<&'static str> {
    Kind::from_str_opt(kind)
        .map(|k| k.info().settings.iter().map(|o| o.label()).collect())
        .unwrap_or_default()
}

/// The (w, h) a kind may not shrink below, rectangular.
pub fn min_size(kind: &str) -> (i32, i32) {
    Kind::from_str_opt(kind).map_or((1, 1), |k| k.info().min_size)
}

/// The name of a kind as the palette and the toolbar say it ("System monitor").
pub fn title(kind: &str) -> &'static str {
    Kind::from_str_opt(kind).map_or("", |k| k.info().title)
}

pub fn can_be_round(kind: &str) -> bool {
    Kind::from_str_opt(kind).is_some_and(|k| k.info().can_be_round)
}

fn tiles_of(layout: &Layout) -> Vec<Tile> {
    let shown: HashSet<u32> = layout.visible().iter().map(|p| p.id).collect();
    layout
        .widgets
        .iter()
        .map(|p| Tile {
            id: p.id,
            kind: p.kind.name().to_string(),
            round: p.shape == Shape::Round,
            pinned: p.pinned,
            col: p.col,
            row: p.row,
            w: p.w,
            h: p.h,
            visible: shown.contains(&p.id),
            settings: p
                .settings
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect(),
        })
        .collect()
}

fn pt(x: f32, y: f32) -> Point<Pixels> {
    point(px(x), px(y))
}

impl Harness {
    /// Press `button` at (x, y), in logical pixels.
    pub fn down(&mut self, x: f32, y: f32, button: MouseButton) {
        self.down_n(x, y, button, 1);
    }

    /// The `count`th press of a series (2 is a double click).
    pub fn down_n(&mut self, x: f32, y: f32, button: MouseButton, count: usize) {
        let mut vcx = self.vcx();
        vcx.simulate_mouse_move(pt(x, y), None, Modifiers::default());
        vcx.simulate_event(MouseDownEvent {
            position: pt(x, y),
            modifiers: Modifiers::default(),
            button,
            click_count: count,
            first_mouse: false,
        });
        drop(vcx);
        self.settle();
    }

    pub fn up(&mut self, x: f32, y: f32, button: MouseButton) {
        let mut vcx = self.vcx();
        vcx.simulate_mouse_up(pt(x, y), button, Modifiers::default());
        drop(vcx);
        self.settle();
    }

    /// The pointer moves with no button down.
    pub fn move_to(&mut self, x: f32, y: f32) {
        let mut vcx = self.vcx();
        vcx.simulate_mouse_move(pt(x, y), None, Modifiers::default());
        drop(vcx);
        self.settle();
    }

    /// The pointer moves with the left button held.
    pub fn drag_to(&mut self, x: f32, y: f32) {
        let mut vcx = self.vcx();
        vcx.simulate_mouse_move(pt(x, y), Some(MouseButton::Left), Modifiers::default());
        drop(vcx);
        self.settle();
    }

    /// Press at `from`, move to `to` in `steps` even steps, release there.
    pub fn drag(&mut self, from: (f32, f32), to: (f32, f32), steps: u32) {
        self.down(from.0, from.1, MouseButton::Left);
        for i in 1..=steps.max(1) {
            let t = i as f32 / steps.max(1) as f32;
            self.drag_to(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
        }
        self.up(to.0, to.1, MouseButton::Left);
    }

    /// Two quick clicks.
    pub fn double_click(&mut self, x: f32, y: f32) {
        for n in 1..=2 {
            self.down_n(x, y, MouseButton::Left, n);
            self.up(x, y, MouseButton::Left);
        }
    }

    /// Time passes, a few milliseconds at a time: timers (a long press) go off.
    pub fn wait_ms(&mut self, ms: u64) {
        self.cx.executor().advance_clock(Duration::from_millis(ms));
        self.settle();
    }

    /// The window position of grid cell (col, row)'s top left corner.
    pub fn cell_origin(&self, col: f32, row: f32) -> (f32, f32) {
        let g = self.read(|d| d.geometry(d.size.0, d.size.1));
        (g.x + col * g.cell, g.y + row * g.cell)
    }

    /// The side of a cell, in pixels.
    pub fn cell(&self) -> f32 {
        self.read(|d| d.geometry(d.size.0, d.size.1).cell)
    }

    /// The gap between cards.
    pub fn gap(&self) -> f32 {
        self.read(|d| d.look.style.gap)
    }

    /// The layout as the dashboard holds it, every widget in file order.
    pub fn tiles(&self) -> Vec<Tile> {
        self.read(|d| tiles_of(&d.layout))
    }

    /// The tile of widget `id`.
    pub fn tile(&self, id: u32) -> Option<Tile> {
        self.tiles().into_iter().find(|t| t.id == id)
    }

    /// The id of the first widget of `kind`.
    pub fn id_of(&self, kind: &str) -> u32 {
        self.tiles()
            .iter()
            .find(|t| t.kind == kind)
            .unwrap_or_else(|| panic!("no {kind} in the layout"))
            .id
    }

    /// The widgets as the views draw them (a drag's preview applied): id, col, row, w, h.
    pub fn shown(&self) -> Vec<(u32, i32, i32, i32, i32)> {
        self.read(|d| {
            d.tiles
                .iter()
                .map(|t| (t.id(), t.placed.col, t.placed.row, t.placed.w, t.placed.h))
                .collect()
        })
    }

    /// The grid in use, and whether config.kdl sets it (rather than the screen).
    pub fn grid(&self) -> (i32, i32, bool) {
        self.read(|d| (d.layout.cols, d.layout.rows, d.layout.grid.is_some()))
    }

    /// The text config.kdl has now.
    pub fn config_text(&self) -> String {
        std::fs::read_to_string(Layout::path()).unwrap_or_default()
    }

    /// Write `text` as config.kdl, as if the dashboard had started from it.
    pub fn seed_config(text: &str) {
        crate::files::write_atomic(&Layout::path(), text).expect("writing config.kdl");
    }

    /// The toast that is up (empty if none).
    pub fn toast_text(&self) -> String {
        self.read(|d| d.toast.as_ref().map(|t| t.text.clone()).unwrap_or_default())
    }

    /// The hint label of the first widget of `kind`.
    pub fn hint_of(&self, kind: &str) -> String {
        self.read(|d| {
            d.tiles
                .iter()
                .find(|t| t.kind().name() == kind)
                .map(|t| t.hint.clone())
        })
        .unwrap_or_else(|| panic!("no label for {kind}"))
    }

    /// Type the hint label of the first widget of `kind`.
    pub fn type_label(&mut self, kind: &str) {
        let label = self.hint_of(kind);
        for c in label.chars() {
            self.key(&c.to_string());
        }
    }

    /// The card's rows (the settings of the widget being worked on).
    pub fn setting_rows(&self) -> Vec<SettingRow> {
        self.read(Dash::setting_rows)
    }

    /// The Arrange tab's rows.
    pub fn arrange_rows(&self) -> Vec<SettingRow> {
        self.read(Dash::arrange_rows)
    }

    /// The Columns and Rows steppers.
    pub fn grid_controls(&self) -> Vec<SettingRow> {
        self.read(|d| d.grid_controls().to_vec())
    }

    pub fn drawer_tab(&self) -> DrawerTab {
        self.read(|d| d.edit.tab)
    }

    pub fn set_drawer_tab(&mut self, tab: DrawerTab) {
        self.update(|d, cx| {
            d.set_tab(tab);
            cx.notify();
        });
    }

    pub fn editing(&self) -> bool {
        self.read(|d| d.editing)
    }

    pub fn settings_id(&self) -> u32 {
        self.read(|d| d.edit.settings_id)
    }

    pub fn adjust_id(&self) -> u32 {
        self.read(|d| d.edit.adjust_id)
    }

    pub fn nav_mode(&self) -> NavMode {
        self.read(|d| d.nav.mode)
    }

    /// The keyboard's move or resize mode (Ctrl+M, Ctrl+R).
    pub fn nav_op(&self) -> NavOp {
        self.read(|d| d.op())
    }

    /// The quick tips along the bottom, as "keys what-they-do".
    pub fn tips(&self) -> Vec<String> {
        self.read(|d| {
            d.tips()
                .iter()
                .map(|k| format!("{} {}", k.key, k.does))
                .collect()
        })
    }

    /// Everything that disagrees or breaks a rule, empty if all is well. Call when no drag is
    /// under way (a drag shows a preview that differs from the layout on purpose).
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.read(|d| {
            let layout = &d.layout;
            let tiles = tiles_of(layout);
            let (cols, rows) = (layout.cols, layout.rows);

            // The rules of a layout.
            let mut seen = HashSet::new();
            for t in &tiles {
                if !seen.insert(t.id) {
                    out.push(format!("two widgets with id {}", t.id));
                }
                if t.round && t.w != t.h {
                    out.push(format!("round {} #{} is {}x{}", t.kind, t.id, t.w, t.h));
                }
                if t.round && !can_be_round(&t.kind) {
                    out.push(format!("{} #{} is round but can't be", t.kind, t.id));
                }
                let (mw, mh) = if t.round { (1, 1) } else { min_size(&t.kind) };
                if t.w < mw || t.h < mh {
                    out.push(format!(
                        "{} #{} is {}x{}, below its minimum {mw}x{mh}",
                        t.kind, t.id, t.w, t.h
                    ));
                }
                if t.col < 0 || t.row < 0 {
                    out.push(format!("{} #{} at negative {},{}", t.kind, t.id, t.col, t.row));
                }
                if t.visible && (t.col + t.w > cols || t.row + t.h > rows) {
                    out.push(format!(
                        "{} #{} shown past the {cols}x{rows} grid at {},{} {}x{}",
                        t.kind, t.id, t.col, t.row, t.w, t.h
                    ));
                }
                if let Some(kind) = Kind::from_str_opt(&t.kind) {
                    let p = layout.get(t.id).expect("the tile's widget");
                    for w in crate::widget_settings::check(kind, &p.settings) {
                        out.push(format!("#{}: {w}", t.id));
                    }
                }
            }
            if !(crate::layout::COLUMNS.contains(&cols) && crate::layout::ROWS.contains(&rows)) {
                out.push(format!("grid {cols}x{rows} is outside the steppers' range"));
            }

            // The views show the visible widgets, where the layout has them.
            if d.edit.preview.is_none() {
                let shown: Vec<_> = d
                    .tiles
                    .iter()
                    .map(|t| (t.id(), t.placed.col, t.placed.row, t.placed.w, t.placed.h))
                    .collect();
                let want: Vec<_> = layout
                    .visible()
                    .iter()
                    .map(|p| (p.id, p.col, p.row, p.w, p.h))
                    .collect();
                if shown != want {
                    out.push(format!("the views show {shown:?}, the layout has {want:?}"));
                }
            }
            if d.hidden_widgets != layout.hidden_count() {
                out.push(format!(
                    "the views say {} hidden, the layout has {}",
                    d.hidden_widgets,
                    layout.hidden_count()
                ));
            }
            let g = d.geometry(d.size.0, d.size.1);
            if (g.cols, g.rows) != (cols, rows) {
                out.push(format!(
                    "the grid on screen is {}x{}, the layout's {cols}x{rows}",
                    g.cols, g.rows
                ));
            }

            // Hint labels: one per shown widget, none the start of another.
            let labels: Vec<&str> = d.tiles.iter().map(|t| t.hint.as_str()).collect();
            for (i, a) in labels.iter().enumerate() {
                if a.is_empty() {
                    out.push(format!("widget {i} has no hint label"));
                }
                for (j, b) in labels.iter().enumerate() {
                    if i != j && b.starts_with(a) {
                        out.push(format!("hint {a:?} is the start of {b:?}"));
                    }
                }
            }
            if d.nav.mode == NavMode::Widget
                && d.solo.is_none()
                && !layout.visible().iter().any(|p| p.id == d.nav.active_id)
            {
                out.push(format!("entered widget #{} isn't on screen", d.nav.active_id));
            }

            // The sidebar's settings are the open widget's.
            let open = d.edit.settings_id;
            if open != 0 {
                match layout.get(open) {
                    None => out.push(format!("settings open for #{open}, which is gone")),
                    Some(p) => {
                        let rows = d.setting_rows();
                        let want = p.kind.info().settings.len();
                        if rows.len() != want {
                            out.push(format!(
                                "{} settings shown for {} #{open}, which has {want}",
                                rows.len(),
                                p.kind.name()
                            ));
                        }
                    }
                }
            }
        });

        // config.kdl says the same, and reads back without warnings.
        let (broken, mine, grid, cols, rows) = self.read(|d| {
            (
                d.config_broken,
                tiles_of(&d.layout),
                d.layout.grid,
                d.layout.cols,
                d.layout.rows,
            )
        });
        if !broken {
            let text = self.config_text();
            match Layout::parse(&text) {
                Err(e) => out.push(format!("config.kdl doesn't parse: {e}")),
                Ok((mut disk, warnings)) => {
                    for w in warnings {
                        out.push(format!("config.kdl warns: {w}"));
                    }
                    disk.set_screen_grid(cols, rows);
                    let mine: Vec<_> = mine.iter().map(Tile::shape_key).collect();
                    let theirs: Vec<_> = tiles_of(&disk).iter().map(Tile::shape_key).collect();
                    if mine != theirs {
                        out.push(format!(
                            "config.kdl differs from the dashboard:\n  file: {theirs:?}\n  dash: {mine:?}"
                        ));
                    }
                    if disk.grid != grid {
                        out.push(format!(
                            "config.kdl's grid is {:?}, the dashboard's {grid:?}",
                            disk.grid
                        ));
                    }
                }
            }
        }
        out
    }

    // ---- what the views' callbacks do, for the tests to call -----------------------------------

    /// Click the element a view named `name` (a button's name as a screen reader says it).
    pub fn click_named(&mut self, name: &str) {
        let name: &'static str = Box::leak(name.to_string().into_boxed_str());
        self.click_on(name);
    }

    /// The bounds of the element a view called `name`.
    pub fn bounds_of_named(&self, name: &str) -> Option<gpui::Bounds<Pixels>> {
        let name: &'static str = Box::leak(name.to_string().into_boxed_str());
        self.bounds_of(name)
    }

    /// Whether an element called `name` is on screen.
    pub fn has_named(&self, name: &str) -> bool {
        let name: &'static str = Box::leak(name.to_string().into_boxed_str());
        self.has(name)
    }

    /// Let things glide again (the harness has them snap to their ends).
    pub fn animate(&mut self, on: bool) {
        self.update(|d, _| d.edit.anim.snap = !on);
    }

    pub fn set_editing(&mut self, on: bool) {
        self.update(|d, cx| d.set_editing(on, cx));
    }

    /// Work on widget `id`'s settings (what `s` in a widget does), entering edit mode if needed.
    pub fn configure(&mut self, id: u32) {
        self.update(|d, cx| d.configure_widget(id, cx));
    }

    pub fn setting_step(&mut self, row: usize, delta: i32) {
        self.update(|d, _| d.setting_step(row, delta));
    }

    pub fn setting_reset(&mut self, row: usize) {
        self.update(|d, _| d.setting_reset(row));
    }

    pub fn setting_text(&mut self, row: usize, text: &str) {
        self.update(|d, _| d.setting_text(row, text));
    }

    pub fn move_widget(&mut self, id: u32, col: i32, row: i32) {
        self.update(|d, _| d.move_widget(id, col, row));
    }

    pub fn resize_widget(&mut self, id: u32, col: i32, row: i32, w: i32, h: i32) {
        self.update(|d, _| d.resize_widget(id, col, row, w, h));
    }

    pub fn toggle_shape(&mut self, id: u32) {
        self.update(|d, _| d.toggle_shape(id));
    }

    pub fn toggle_pin(&mut self, id: u32) {
        self.update(|d, _| d.toggle_pin(id));
    }

    pub fn remove_widget(&mut self, id: u32) {
        self.update(|d, _| d.remove_widget(id));
    }

    pub fn add_widget(&mut self, kind: &str) {
        let kind = Kind::from_str_opt(kind).expect("a widget kind");
        self.update(|d, _| d.add_widget(kind));
    }

    pub fn add_widget_at(&mut self, kind: &str, col: i32, row: i32) {
        let kind = Kind::from_str_opt(kind).expect("a widget kind");
        self.update(|d, _| d.add_widget_at(kind, col, row));
    }

    pub fn grid_step(&mut self, index: usize, delta: i32) {
        self.update(|d, _| d.grid_step(index, delta));
    }

    pub fn grid_auto(&mut self) {
        self.update(|d, _| d.grid_auto());
    }

    pub fn reset_layout(&mut self) {
        self.update(|d, _| d.reset_layout());
    }

    /// How often each grid stepper (columns, rows) has refused a step.
    pub fn refusals(&self, index: usize) -> u32 {
        self.read(|d| d.edit.refusals[index])
    }

    /// Where the pointer would put widget `id`'s top left corner (a drag over): whether there is
    /// room.
    pub fn preview_move(&mut self, id: u32, col: i32, row: i32) -> bool {
        self.update(|d, _| d.preview_move(id, col, row))
    }

    pub fn preview_resize(&mut self, id: u32, col: i32, row: i32, w: i32, h: i32) -> bool {
        self.update(|d, _| d.preview_resize(id, crate::layout::Rect { col, row, w, h }))
    }

    pub fn end_preview(&mut self) {
        self.update(|d, _| d.set_preview(None));
    }

    pub fn arrange_now(&mut self) {
        self.update(|d, _| d.arrange_now());
    }

    pub fn arrange_shuffle(&mut self) {
        self.update(|d, _| d.arrange_shuffle());
    }

    pub fn arrange_undo(&mut self) {
        self.update(|d, _| d.arrange_undo());
    }

    pub fn arrange_step(&mut self, index: usize, delta: i32) {
        self.update(|d, _| d.arrange_step(index, delta));
    }

    pub fn arrange_reset(&mut self, index: usize) {
        self.update(|d, _| d.arrange_reset(index));
    }

    pub fn arrange_copy(&mut self) {
        self.update(|d, _| d.arrange_copy());
    }

    pub fn arrange_result(&self) -> String {
        self.read(|d| d.arrange_result())
    }

    pub fn arrange_can_undo(&self) -> bool {
        self.read(|d| d.arrange_can_undo())
    }

    /// The status line's note of how big sizing made the widget.
    pub fn size_note(&self) -> String {
        self.read(|d| d.edit.size_note.clone())
    }

    /// The color schemes of the Colors tab: (name, current).
    pub fn schemes(&self) -> Vec<(String, bool)> {
        self.read(|d| d.edit.schemes.iter().map(|s| (s.name.clone(), s.current)).collect())
    }

    /// How many presets the Style tab has.
    pub fn style_presets(&self) -> usize {
        self.read(|d| d.edit.styles.len())
    }

    /// The Style tab's rows.
    pub fn style_rows(&self) -> Vec<(String, String, bool)> {
        self.read(|d| {
            d.edit
                .style_rows
                .iter()
                .map(|r| (r.label.clone(), r.value.clone(), r.overridden))
                .collect()
        })
    }

    /// Whether a style row can step: (can decrease, can increase).
    pub fn style_row_can(&self, i: usize) -> (bool, bool) {
        self.read(|d| d.edit.style_rows.get(i).map_or((false, false), |r| (r.can_dec, r.can_inc)))
    }

    pub fn pinned(&self, id: u32) -> Option<bool> {
        self.tile(id).map(|t| t.pinned)
    }

    /// The row labels of a list of rows.
    pub fn labels(rows: &[SettingRow]) -> Vec<String> {
        rows.iter().map(|r| r.label.clone()).collect()
    }

    pub fn row_kind_is_text(row: &SettingRow) -> bool {
        row.kind == RowKind::Text
    }
}

/// What has to hold of edit mode's own state whatever came before (the stress run checks it after
/// every key): the cursor is on a stop that exists, what is open belongs to a widget that is
/// there, a drag has somewhere to go.
pub fn check(d: &Dash) -> Result<(), String> {
    let e = &d.edit;
    let count = d.side_count();
    if count > 0 && e.side_cursor >= count {
        return Err(format!(
            "the sidebar's cursor is on stop {} of {count}",
            e.side_cursor
        ));
    }
    if e.settings_id != 0 && d.layout.get(e.settings_id).is_none() {
        return Err(format!("settings open for #{}, which is gone", e.settings_id));
    }
    if e.settings_id != 0 && !d.editing {
        return Err("a settings card open outside edit mode".into());
    }
    if e.adjust_id != 0 && d.tile(e.adjust_id).is_none() {
        return Err(format!("widget #{} opened for adjusting isn't on screen", e.adjust_id));
    }
    if e.field.focused && !d.cfg_open() {
        return Err("a text field has the keyboard with no settings card open".into());
    }
    if let Some(p) = &e.press
        && d.tile(p.id).is_none()
    {
        return Err(format!("a press on widget #{}, which isn't on screen", p.id));
    }
    if e.preview.is_some() && e.press.is_none() && !e.dragging() {
        return Err("a preview is showing with nothing being dragged".into());
    }
    if !d.visible && (d.editing || e.adjust_id != 0 || e.dragging() || e.press.is_some()) {
        return Err("closed with edit mode's state still up".into());
    }
    if !d.editing && e.settings_id != 0 {
        return Err("a widget is picked outside edit mode".into());
    }
    // The move and resize modes belong to a widget in hand: one entered, or one picked in edit
    // mode. Without one there is no mode.
    if d.nav.op != NavOp::None {
        if d.op_target() == 0 {
            return Err("move or resize mode with no widget in hand".into());
        }
        if !d.visible {
            return Err("closed in move or resize mode".into());
        }
        if d.edit.field.focused {
            return Err("move or resize mode while a text field has the keyboard".into());
        }
    }
    let rows = d.setting_rows();
    if e.settings_id != 0 {
        let want = d.layout.get(e.settings_id).map_or(0, |p| p.kind.info().settings.len());
        if rows.len() != want {
            return Err(format!("{} setting rows for a widget with {want}", rows.len()));
        }
    }
    Ok(())
}

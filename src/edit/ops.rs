//! What edit mode does to the layout: moving, resizing, reshaping, pinning, adding and removing
//! widgets, the grid, and the live preview of a drag. Each ends in a saved config.kdl.

use super::DrawerTab;
use super::settings::{RowKind, SettingRow};
use crate::dash::Dash;
use crate::layout::{COLUMNS, Kind, Placed, Rect, ROWS, Reflow, Shape};
use crate::nav::{self, Dir};

impl Dash {
    /// The layout changed: show it, save it, and what depends on it follows.
    pub fn layout_changed(&mut self) {
        self.sync_widgets();
        self.save_layout();
        self.tidy_edit();
    }

    /// What has to stay in step with the layout and the tabs: the card of a widget that is gone
    /// closes, a widget opened alone that is gone lets go, and the cursor stays on a row.
    pub fn tidy_edit(&mut self) {
        self.tidy_settings();
        if self.edit.adjust_id != 0 && self.tile(self.edit.adjust_id).is_none() {
            self.edit.adjust_id = 0;
        }
        let last = self.side_count().saturating_sub(1);
        if self.edit.side_cursor > last {
            self.edit.side_cursor = last;
        }
    }

    /// Set what the preview of a drag shows (None: nothing).
    pub fn set_preview(&mut self, p: Option<(u32, Reflow)>) {
        let changed = self.edit.preview != p;
        self.edit.preview = p;
        if changed {
            self.sync_widgets();
        }
    }

    /// Why widget `id` can't go to `target`, for the toast.
    fn refusal(&self, id: u32, target: Rect) -> &'static str {
        let l = &self.layout;
        if l.is_pinned(id) {
            "Pinned: unpin it first (p)"
        } else if l.pins(id).iter().any(|r| r.overlaps(&target)) {
            "A pinned widget is in the way"
        } else {
            "No room there"
        }
    }

    /// Put widget `id` at `target` from the keyboard, pushing others aside (`prefer`: the way it
    /// went).
    fn place_by_key(&mut self, id: u32, target: Rect, prefer: (i32, i32)) {
        self.set_preview(None);
        if self.layout.place_reflow(id, target, prefer) {
            self.layout_changed();
        } else {
            let why = self.refusal(id, target);
            self.toast(why);
        }
    }

    /// Ctrl+Shift+h/j/k/l: move widget `id` one cell that way.
    pub fn nudge_widget(&mut self, id: u32, dir: Dir) {
        let (dc, dr) = dir.step();
        let Some(r) = self.layout.get(id).map(Placed::rect) else {
            return;
        };
        let target = Rect {
            col: r.col + dc,
            row: r.row + dr,
            ..r
        };
        self.place_by_key(id, target, (dc, dr));
    }

    /// The keyboard's sizing: `key` (h narrower, l wider, k shorter, j taller) changes widget `id`
    /// by a cell, from its right or bottom side, or from the left or top one when `flip`ped. A
    /// widget that can't grow that way grows the other way instead.
    pub fn size_widget(&mut self, id: u32, key: Dir, flip: bool) {
        let (first, second) = nav::size_edges(key, flip);
        let grow = second.is_some();
        let mut refused = None;
        for edge in std::iter::once(first).chain(second) {
            let target = {
                let Some(p) = self.layout.get(id) else { return };
                nav::resized(p.rect(), edge, grow, p.shape == Shape::Round, p.min_size())
            };
            // At its smallest, shrinking does nothing.
            let Some(target) = target else { return };
            self.set_preview(None);
            if self.layout.place_reflow(id, target, edge.step()) {
                self.layout_changed();
                self.size_note(id);
                return;
            }
            refused.get_or_insert(target);
        }
        if let Some(target) = refused {
            let why = self.refusal(id, target);
            self.toast(why);
        }
    }

    /// Tell the status line how big the widget is now.
    fn size_note(&mut self, id: u32) {
        let note = self.layout.get(id).map(|p| {
            let r = p.rect();
            format!("{} {}×{}", p.kind.info().title, r.w, r.h)
        });
        self.edit.size_note = note.unwrap_or_default();
    }

    /// The main direction of a move, used to prefer pushing others that way.
    fn direction(&self, id: u32, col: i32, row: i32) -> (i32, i32) {
        let Some((from_col, from_row)) = self.layout.get(id).map(|p| (p.col, p.row)) else {
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

    // ---- the live reflow while dragging: others move out of the way; false if there is no room

    pub fn preview_move(&mut self, id: u32, col: i32, row: i32) -> bool {
        let prefer = self.direction(id, col, row);
        let result = self.layout.get(id).and_then(|p| {
            self.layout.reflow(
                id,
                Rect {
                    col,
                    row,
                    w: p.w,
                    h: p.h,
                },
                prefer,
            )
        });
        let ok = result.is_some();
        self.set_preview(result.map(|m| (id, m)));
        ok
    }

    pub fn preview_resize(&mut self, id: u32, target: Rect) -> bool {
        let result = self.layout.plan_resize(id, target);
        let ok = result.is_some();
        self.set_preview(result.map(|(_, moves)| (id, moves)));
        ok
    }

    pub fn preview_add(&mut self, kind: Kind, col: i32, row: i32) -> bool {
        let (w, h) = kind.info().new_size();
        let result = self.layout.reflow(0, Rect { col, row, w, h }, (0, 1));
        let ok = result.is_some();
        self.set_preview(result.map(|m| (0, m)));
        ok
    }

    // ---- what a drop or a button does -------------------------------------------------------

    pub fn move_widget(&mut self, id: u32, col: i32, row: i32) {
        self.set_preview(None);
        let prefer = self.direction(id, col, row);
        if self.layout.move_reflow(id, col, row, prefer) {
            self.layout_changed();
        } else {
            self.sync_widgets();
            let target = self.layout.get(id).map(|p| Rect {
                col,
                row,
                ..p.rect()
            });
            if let Some(target) = target {
                let why = self.refusal(id, target);
                self.toast(why);
            }
        }
    }

    pub fn resize_widget(&mut self, id: u32, col: i32, row: i32, w: i32, h: i32) {
        self.set_preview(None);
        if self.layout.resize_reflow(id, Rect { col, row, w, h }) {
            self.layout_changed();
        } else {
            self.sync_widgets();
        }
    }

    pub fn add_widget_at(&mut self, kind: Kind, col: i32, row: i32) {
        self.set_preview(None);
        if self.layout.add_at(kind, col, row).is_some() {
            self.layout_changed();
        } else {
            self.sync_widgets();
            self.toast("No room there");
        }
    }

    pub fn add_widget(&mut self, kind: Kind) {
        if self.layout.add(kind).is_some() {
            self.layout_changed();
            self.toast(&format!("Added {}", kind.info().title));
        } else {
            self.toast("No free space. Remove or shrink a widget first.");
        }
    }

    pub fn toggle_shape(&mut self, id: u32) {
        if self.layout.toggle_shape(id) {
            self.layout_changed();
        } else if self.layout.is_pinned(id) {
            self.toast("Pinned: unpin it first (p)");
        } else {
            let round = self
                .layout
                .get(id)
                .is_some_and(|p| p.shape == Shape::Round);
            self.toast(if round {
                "No room to make this widget rectangular here"
            } else {
                "No room to make this widget round here"
            });
        }
    }

    /// Pin a widget so that it stays where it is, or let go of it.
    pub fn toggle_pin(&mut self, id: u32) {
        if let Some(pinned) = self.layout.toggle_pin(id) {
            self.layout_changed();
            self.toast(if pinned {
                "Pinned: it stays where it is"
            } else {
                "Unpinned"
            });
        }
    }

    pub fn remove_widget(&mut self, id: u32) {
        self.layout.remove(id);
        if self.edit.adjust_id == id {
            self.edit.adjust_id = 0;
        }
        self.edit.anim.forget(id);
        self.edit.field.forget(id);
        // Closes the card if it was this widget's.
        self.layout_changed();
    }

    /// Put the default widgets back.
    pub fn reset_layout(&mut self) {
        let refit = self.layout.reset();
        self.layout_changed();
        if let Some(m) = refit.message() {
            self.toast(&m);
        }
    }

    // ---- the grid ---------------------------------------------------------------------------

    /// The Columns and Rows steppers in the Widgets tab.
    pub fn grid_controls(&self) -> [SettingRow; 2] {
        let l = &self.layout;
        let row = |index, label: &str, n: i32, range: &std::ops::RangeInclusive<i32>| SettingRow {
            index,
            kind: RowKind::Stepper,
            label: label.into(),
            value: n.to_string(),
            on: false,
            changed: l.grid.is_some(),
            can_dec: n > *range.start(),
            can_inc: n < *range.end(),
            placeholder: String::new(),
        };
        [
            row(0, "Columns", l.cols, &COLUMNS),
            row(1, "Rows", l.rows, &ROWS),
        ]
    }

    /// Change the grid from edit mode: `Some` makes it explicit, fitting the widgets into it;
    /// `None` goes back to the automatic one.
    fn change_grid(&mut self, grid: Option<(i32, i32)>) {
        let refit = self.layout.set_grid(grid);
        self.set_size_forced();
        self.layout_changed();
        if let Some(m) = refit.message() {
            self.toast(&m);
        }
    }

    /// The grid steppers: index 0 is the columns, 1 the rows. The first step makes the grid
    /// explicit, starting from what fits the screen. A step that would leave no room for some
    /// widget is refused: the stepper shakes and says which ones, and nothing changes. Edit mode
    /// never deletes widgets on its own.
    pub fn grid_step(&mut self, index: usize, delta: i32) {
        let (cols, rows) = (self.layout.cols, self.layout.rows);
        let grid = match index {
            0 => (cols + delta, rows),
            _ => (cols, rows + delta),
        };
        // Past the steppers' ends (the keyboard can try; the buttons are greyed out).
        if !COLUMNS.contains(&grid.0) || !ROWS.contains(&grid.1) {
            self.edit.refusals[index.min(1)] += 1;
            return;
        }
        let lost = {
            let mut trial = self.layout.clone();
            trial.set_grid(Some(grid)).evicted
        };
        if !lost.is_empty() {
            let mut names: Vec<&str> = lost.iter().map(|p| p.kind.info().title).collect();
            names.dedup();
            self.edit.refusals[index.min(1)] += 1;
            self.toast(&format!(
                "No room for {} on a {}\u{d7}{} grid. Shrink or remove widgets first.",
                names.join(", "),
                grid.0,
                grid.1
            ));
            return;
        }
        self.change_grid(Some(grid));
    }

    pub fn grid_auto(&mut self) {
        self.change_grid(None);
    }

    // ---- the sidebar's stops --------------------------------------------------------------

    /// How many stops the sidebar's cursor has: with the card open its rows, otherwise those of
    /// the tab. Widgets: the grid's columns and rows, the palette, "Reset" (the layout). Arrange:
    /// "Arrange" and "Shuffle", the settings, "Copy" and "Undo". Colors: the schemes and the
    /// folder button. Style: the presets, "Reset all" (when adjusted), the adjustments and the
    /// folder button.
    pub fn side_count(&self) -> usize {
        if self.cfg_open() {
            return self.setting_rows().len();
        }
        match self.edit.tab {
            DrawerTab::Widgets => 3 + self.edit.palette.len(),
            DrawerTab::Arrange => self.arrange_rows().len() + 4,
            DrawerTab::Colors => self.edit.schemes.len() + 1,
            DrawerTab::Style => self.k_adjust() + self.edit.style_rows.len() + 1,
        }
    }

    /// Where the Style tab's adjustments start among its stops: after the presets and "Reset all".
    pub fn k_adjust(&self) -> usize {
        self.edit.styles.len() + usize::from(self.edit.style_customized)
    }

    /// The keyboard is on stop `i` of `tab` (and edit mode shows it).
    pub fn side_on(&self, tab: DrawerTab, i: usize) -> bool {
        self.editing
            && self.edit.side_keys
            && !self.cfg_open()
            && self.edit.tab == tab
            && self.edit.side_cursor == i
    }

    /// The keyboard is on row `i` of the card.
    pub fn cfg_on(&self, i: usize) -> bool {
        self.cfg_open() && self.edit.side_keys && self.edit.side_cursor == i
    }

    /// Switch the sidebar's tab: the cursor starts at the top, and a text field of the tab that
    /// went lets go of the keyboard.
    pub fn set_tab(&mut self, tab: DrawerTab) {
        if self.edit.tab != tab {
            self.edit.tab = tab;
            self.edit.side_cursor = 0;
            self.edit.field.leave();
        }
    }
}

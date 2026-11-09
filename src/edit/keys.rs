//! The keyboard model of edit mode (`edit-key` in app.slint). The keys are on the widgets or on
//! the sidebar: on the widgets, h j k l size the widget being worked on, Ctrl+h j k l move it and
//! w a s d go to the next; on the sidebar (or the settings card), j k go down the rows, h l step
//! one, Enter presses it, Backspace puts it back, Tab goes on to the next tab.

use super::DrawerTab;
use super::settings::RowKind;
use crate::dash::{Dash, key_dir};
use crate::keys::{Code, Key, Named};
use crate::nav::Dir;
use gpui::Context;

/// A key in edit mode; true if one was used.
pub fn edit_key(d: &mut Dash, key: &Key, cx: &mut Context<Dash>) -> bool {
    // A drag is going on: its pointer is what counts.
    if d.edit.dragging() {
        return false;
    }
    let used = edit_key_inner(d, key, cx);
    d.tidy_edit();
    used
}

/// A key while a text field of the card has the keyboard. Most are the field's own; Tab leaves it
/// (what was typed is dropped) and Enter saves what was typed.
pub fn field_key(d: &mut Dash, key: &Key, cx: &mut Context<Dash>) -> bool {
    if key.bare() && (key.is_named(Named::Tab) || key.is_named(Named::BackTab)) {
        d.edit.field.leave();
        return true;
    }
    if key.bare() && key.is_named(Named::Enter) {
        if let Some(row) = d.edit.field.row
            && let Some(state) = d.edit.field.state(d.edit.settings_id, row)
        {
            let text = state.read(cx).value().to_string();
            d.setting_text(row, &text);
        }
        d.edit.field.leave();
        d.tidy_edit();
        return true;
    }
    false
}

fn edit_key_inner(d: &mut Dash, key: &Key, cx: &mut Context<Dash>) -> bool {
    let target = d.edit.settings_id;
    let plain = key.bare();
    if (key.is('e') || key.is('E')) && key.ctrl {
        d.set_editing(false, cx);
        return true;
    }
    // Ctrl+h j k l (or the arrows): move the widget a cell, the others make room.
    if target != 0
        && key.ctrl
        && !key.alt
        && !key.meta
        && let Some(dir) = key_dir(key)
    {
        d.edit.side_keys = false;
        d.edit.canvas_keys = true;
        d.nudge_widget(target, dir);
        return true;
    }
    // w a s d (and, with the keys on the widgets, the arrows): the focus goes to the next widget
    // that way, or to the first one when none is picked.
    let fd = focus_dir(key);
    let letter = matches!(key.code, Code::Char('w' | 'a' | 's' | 'd'));
    if plain
        && let Some(dir) = fd
        && !key.shift
        && (letter || (target != 0 && !d.edit.side_keys))
    {
        if target == 0 {
            if let Some(first) = d.tiles.first().map(|t| t.id()) {
                d.configure_widget(first, cx);
            }
        } else if let Some(next) = d.neighbor(target, dir) {
            d.configure_widget(next, cx);
        }
        d.edit.side_keys = false;
        d.edit.canvas_keys = true;
        return true;
    }
    // h j k l: size the widget; Shift, or a capital, from the other side.
    if target != 0 && plain && !d.edit.side_keys {
        if let Code::Char(c @ ('h' | 'j' | 'k' | 'l' | 'H' | 'J' | 'K' | 'L')) = key.code
            && let Some(dir) = key_dir(key)
        {
            d.edit.canvas_keys = true;
            d.size_widget(target, dir, key.shift || c.is_ascii_uppercase());
            return true;
        }
        // Enter shows the settings, with the cursor on their first row.
        if key.is_named(Named::Enter) {
            d.edit.canvas_keys = false;
            d.edit.side_keys = true;
            return true;
        }
    }
    if !plain {
        return false;
    }
    if target != 0 && key.is('o') {
        d.toggle_shape(target);
        return true;
    }
    if target != 0 && key.is('p') {
        d.toggle_pin(target);
        return true;
    }
    if target != 0 && (key.is('x') || key.is_named(Named::Delete)) {
        d.remove_widget(target);
        return true;
    }
    if key.is_named(Named::Tab) || key.is_named(Named::BackTab) {
        d.edit.side_keys = true;
        d.edit.canvas_keys = false;
        d.side_tab(if key.is_named(Named::Tab) { 1 } else { -1 });
        return true;
    }
    let down = key.is('j') || key.is_named(Named::Down);
    let up = key.is('k') || key.is_named(Named::Up);
    if down
        || up
        || key.is('g')
        || key.is('G')
        || key.is_named(Named::Home)
        || key.is_named(Named::End)
    {
        let last = d.side_count().saturating_sub(1);
        // The first key shows the cursor where it is.
        if d.edit.side_keys {
            let at = d.edit.side_cursor;
            d.edit.side_cursor = if down {
                last.min(at + 1)
            } else if up {
                at.saturating_sub(1)
            } else if key.is('g') || key.is_named(Named::Home) {
                0
            } else {
                last
            };
        }
        d.edit.side_keys = true;
        return true;
    }
    let left = key.is('h') || key.is_named(Named::Left);
    if left || key.is('l') || key.is_named(Named::Right) {
        d.edit.side_keys = true;
        d.side_step(d.edit.side_cursor, if left { -1 } else { 1 });
        return true;
    }
    if key.is_named(Named::Enter) || key.is(' ') {
        d.edit.side_keys = true;
        d.side_activate(d.edit.side_cursor);
        return true;
    }
    if key.is_named(Named::Backspace) {
        d.edit.side_keys = true;
        d.side_reset(d.edit.side_cursor);
        return true;
    }
    false
}

/// w a s d or the arrows: the direction to the next widget.
fn focus_dir(key: &Key) -> Option<Dir> {
    match key.code {
        Code::Char('a') | Code::Named(Named::Left) => Some(Dir::Left),
        Code::Char('s') | Code::Named(Named::Down) => Some(Dir::Down),
        Code::Char('w') | Code::Named(Named::Up) => Some(Dir::Up),
        Code::Char('d') | Code::Named(Named::Right) => Some(Dir::Right),
        _ => None,
    }
}

impl Dash {
    /// Tab: the next tab of the sidebar (`d` is 1), or the previous (-1). With the settings card
    /// open it has no tabs: Tab steps through its rows.
    fn side_tab(&mut self, d: i32) {
        if self.cfg_open() {
            let n = self.side_count() as i32;
            if n > 0 {
                self.edit.side_cursor = (self.edit.side_cursor as i32 + d + n).rem_euclid(n) as usize;
            }
            return;
        }
        let i = self.edit.tab.index() as i32;
        let j = (i + d + 4).rem_euclid(4) as usize;
        self.set_tab(DrawerTab::ALL[j]);
    }

    /// h/l on stop `i`: step it down or up (a switch flips).
    fn side_step(&mut self, i: usize, d: i32) {
        if self.cfg_open() {
            let rows = self.setting_rows();
            if let Some(row) = rows.get(i).filter(|r| r.kind != RowKind::Text) {
                self.setting_step(row.index, d);
            }
        } else {
            match self.edit.tab {
                DrawerTab::Widgets if i < 2 => self.grid_step(i, d),
                DrawerTab::Arrange if i >= 2 => {
                    if let Some(row) = self.arrange_rows().get(i - 2) {
                        self.arrange_step(row.index, d);
                    }
                }
                DrawerTab::Style if i >= self.k_adjust() => {
                    if let Some(row) = self.edit.style_rows.get(i - self.k_adjust()) {
                        let var = row.var;
                        self.style_step(var, d);
                    }
                }
                _ => {}
            }
        }
    }

    /// Enter or Space on stop `i`.
    pub fn side_activate(&mut self, i: usize) {
        if self.cfg_open() {
            let rows = self.setting_rows();
            if let Some(row) = rows.get(i) {
                if row.kind == RowKind::Text {
                    self.edit.field.enter(i);
                } else {
                    self.setting_step(row.index, 1);
                }
            }
            return;
        }
        match self.edit.tab {
            DrawerTab::Widgets => {
                let n = self.edit.palette.len();
                if i >= 2 && i - 2 < n {
                    let kind = self.edit.palette[i - 2].kind;
                    self.add_widget(kind);
                } else if i == 2 + n {
                    self.reset_layout();
                }
            }
            DrawerTab::Arrange => {
                let rows = self.arrange_rows();
                if i == 0 {
                    self.arrange_now();
                } else if i == 1 {
                    self.arrange_shuffle();
                } else if i - 2 < rows.len() {
                    self.arrange_step(rows[i - 2].index, 1);
                } else if i - 2 == rows.len() {
                    self.arrange_copy();
                } else {
                    self.arrange_undo();
                }
            }
            DrawerTab::Colors => {
                if let Some(s) = self.edit.schemes.get(i) {
                    let name = s.name.clone();
                    self.choose_colorscheme(&name);
                } else {
                    self.open_themes_folder();
                }
            }
            DrawerTab::Style => {
                let k = self.k_adjust();
                if let Some(s) = self.edit.styles.get(i) {
                    let name = s.name.clone();
                    self.choose_style(&name);
                } else if i < k {
                    self.style_reset(None);
                } else if let Some(row) = self.edit.style_rows.get(i - k) {
                    if row.kind == RowKind::Toggle {
                        let var = row.var;
                        self.style_step(var, 1);
                    }
                } else {
                    self.open_themes_folder();
                }
            }
        }
    }

    /// Backspace on stop `i`: back to the default.
    fn side_reset(&mut self, i: usize) {
        if self.cfg_open() {
            let rows = self.setting_rows();
            if let Some(row) = rows.get(i) {
                self.setting_reset(row.index);
            }
            return;
        }
        match self.edit.tab {
            DrawerTab::Widgets if i < 2 => self.grid_auto(),
            DrawerTab::Arrange if i >= 2 => {
                if let Some(row) = self.arrange_rows().get(i - 2) {
                    self.arrange_reset(row.index);
                }
            }
            DrawerTab::Style if i >= self.k_adjust() => {
                if let Some(row) = self.edit.style_rows.get(i - self.k_adjust()) {
                    let var = row.var;
                    self.style_reset(Some(var));
                }
            }
            _ => {}
        }
    }
}

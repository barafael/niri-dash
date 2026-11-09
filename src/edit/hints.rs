//! The shortcuts the top bar shows in edit mode, for what the keys do just now.

use super::DragMode;
use crate::dash::{Dash, NavMode};

/// A keyboard shortcut for the top bar: the keys, and what they do. No keys: a plain note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyHint {
    pub key: String,
    pub does: String,
}

fn hints(list: &[(&str, &str)]) -> Vec<KeyHint> {
    list.iter()
        .map(|(key, does)| KeyHint {
            key: key.to_string(),
            does: does.to_string(),
        })
        .collect()
}

impl Dash {
    /// What the keys do now (`edit-hints` in app.slint).
    pub fn edit_hints(&self) -> Vec<KeyHint> {
        let e = &self.edit;
        if e.drag.mode == DragMode::Carry {
            return hints(&[("Click", "place it"), ("Esc", "cancel")]);
        }
        if e.adjust_id != 0 {
            return hints(&[
                ("Drag", "move"),
                ("Edge or corner", "resize"),
                ("◯ ▢", "shape"),
                ("Esc", "done"),
            ]);
        }
        if !self.editing {
            return Vec::new();
        }
        if self.nav.mode == NavMode::Hints {
            return hints(&[
                ("Type a label", "pick that widget"),
                ("Backspace", "take a letter back"),
                ("f", "cancel"),
            ]);
        }
        if e.field.focused {
            return hints(&[
                ("Enter", "save"),
                ("Esc", "discard"),
                ("Tab", "leave the field"),
            ]);
        }
        if e.settings_id != 0 && self.cfg_open() && e.side_keys {
            return hints(&[
                ("j k", "row"),
                ("h l", "change"),
                ("Enter", "toggle or type"),
                ("Backspace", "reset"),
                ("Tab", "next row"),
                ("w a s d", "back to the widgets"),
                ("Esc", "close"),
            ]);
        }
        if e.side_keys {
            return hints(&[
                ("Tab", "next tab"),
                ("j k", "row"),
                ("h l", "change"),
                ("Enter", "press"),
                ("Backspace", "reset"),
                ("f", "pick a widget"),
                ("w a s d", "to the widgets"),
                ("Esc", "done"),
            ]);
        }
        if e.settings_id != 0 && self.settings_pinned() {
            let title = format!("{} is pinned", self.settings_title());
            let mut out = vec![KeyHint {
                key: String::new(),
                does: title,
            }];
            out.extend(hints(&[
                ("p", "unpin"),
                ("w a s d", "pick another"),
                ("Tab", "settings"),
                ("x", "remove"),
                ("Esc", "let go"),
            ]));
            return out;
        }
        if e.settings_id != 0 {
            let note = if e.size_note.is_empty() {
                self.settings_title()
            } else {
                e.size_note.clone()
            };
            let mut out = vec![KeyHint {
                key: String::new(),
                does: note,
            }];
            out.extend(hints(&[
                ("h j k l", "size"),
                ("Shift", "from the other side"),
                ("Ctrl + h j k l", "move"),
                ("w a s d", "pick another"),
                ("o", "shape"),
                ("p", "pin"),
                ("x", "remove"),
                ("Tab", "settings"),
                ("Esc", "let go"),
            ]));
            return out;
        }
        hints(&[
            ("f", "pick a widget"),
            ("w a s d", "nearest widget"),
            ("Tab", "sidebar"),
            ("Drag", "move"),
            ("Edge", "resize"),
            ("Click", "settings"),
            ("Esc", "done"),
        ])
    }
}

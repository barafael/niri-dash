//! The quick tips along the bottom of the screen: the keys that work right now, in every mode
//! (`tips` in app.slint).

use super::DragMode;
use crate::dash::{Dash, NavMode, NavOp};

/// A keyboard shortcut for the quick tips: the keys, and what they do. No keys: a plain note.
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
    /// What the keys do now (`tips` in app.slint). Nothing in a standalone app.
    pub fn tips(&self) -> Vec<KeyHint> {
        if self.solo.is_some() {
            return Vec::new();
        }
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
        let note = || e.size_note.clone();
        match self.op() {
            NavOp::Move => {
                let mut out = vec![KeyHint {
                    key: String::new(),
                    does: format!("MOVE {}", note()),
                }];
                out.extend(hints(&[
                    ("h j k l", "one cell (arrows too)"),
                    ("Ctrl+R", "resize instead"),
                    ("Esc", "done"),
                ]));
                return out;
            }
            NavOp::Resize => {
                let mut out = vec![KeyHint {
                    key: String::new(),
                    does: format!("RESIZE {}", note()),
                }];
                out.extend(hints(&[
                    ("h j k l", "pull that side out"),
                    ("Shift + h j k l", "pull it in"),
                    ("Ctrl+M", "move instead"),
                    ("Esc", "done"),
                ]));
                return out;
            }
            NavOp::None => {}
        }
        if self.nav.mode == NavMode::Widget && !self.editing {
            return hints(&[
                ("Ctrl+M", "move"),
                ("Ctrl+R", "resize"),
                ("Ctrl + h j k l", "next widget"),
                ("e", "expand"),
                ("s", "settings"),
                ("f", "pick another"),
                ("Ctrl+E", "edit mode"),
                ("Esc", "leave"),
            ]);
        }
        if !self.editing && self.nav.mode != NavMode::Hints {
            return hints(&[
                ("f", "pick a widget"),
                ("Ctrl+E", "edit mode"),
                ("Esc", "close"),
            ]);
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
                ("Ctrl+M", "move"),
                ("Ctrl+R", "resize"),
                ("h j k l", "pick another"),
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

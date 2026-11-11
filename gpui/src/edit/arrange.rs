//! Edit mode's Arrange tab: lay the dashboard out by itself, and the settings of how.

use super::settings::{RowKind, SettingRow};
use crate::autolayout::{self, Knob, Params};
use crate::dash::Dash;
use crate::layout::Rect;
use niri_dash_system::{Tell, clipboard};
use std::sync::Arc;

pub struct Arrange {
    /// The settings being tried; they start as config.kdl's `autolayout` line.
    pub params: Params,
    /// Where the widgets were before the last run, to go back to.
    pub undo: Option<Vec<(u32, Rect)>>,
    /// What the last run made of it, for the line under the buttons.
    pub result: String,
}

impl Arrange {
    pub fn new(params: Params) -> Self {
        Self {
            params,
            undo: None,
            result: String::new(),
        }
    }
}

/// The settings, in the order the row index stands for.
const ALL: [Knob; 19] = [
    Knob::Algo,
    Knob::Order,
    Knob::Seed,
    Knob::Restarts,
    Knob::Jitter,
    Knob::Shrink,
    Knob::Grow,
    Knob::Aspect,
    Knob::Overshoot,
    Knob::Hole,
    Knob::MaxGrow,
    Knob::WeightExponent,
    Knob::CutBias,
    Knob::Affinity,
    Knob::Reach,
    Knob::Placement,
    Knob::Edge,
    Knob::Balance,
    Knob::Effort,
];

fn knob_index(k: Knob) -> usize {
    ALL.iter().position(|a| *a == k).unwrap_or(0)
}

fn knob_from(index: usize) -> Option<Knob> {
    ALL.get(index).copied()
}

impl Dash {
    /// The tab's rows.
    pub fn arrange_rows(&self) -> Vec<SettingRow> {
        let p = &self.edit.arrange.params;
        Knob::of(p.algo)
            .into_iter()
            .map(|k| SettingRow {
                index: knob_index(k),
                kind: RowKind::Stepper,
                label: k.label().into(),
                value: p.text(k),
                on: false,
                changed: !p.is_default(k),
                can_dec: p.can_step(k, -1),
                can_inc: p.can_step(k, 1),
                placeholder: String::new(),
            })
            .collect()
    }

    /// The line under the buttons: what the last run made of it, or what the algorithm does.
    pub fn arrange_result(&self) -> String {
        let a = &self.edit.arrange;
        if a.result.is_empty() {
            a.params.algo.describe().into()
        } else {
            a.result.clone()
        }
    }

    pub fn arrange_can_undo(&self) -> bool {
        self.edit.arrange.undo.is_some()
    }

    /// Lay the widgets out with the current settings.
    pub fn arrange_now(&mut self) {
        let p = self.edit.arrange.params.clone();
        let before: Vec<(u32, Rect)> = self
            .layout
            .widgets
            .iter()
            .map(|w| (w.id, w.rect()))
            .collect();
        let result = autolayout::arrange(&mut self.layout, &p);
        // Running it again keeps the very first layout to go back to, not the last attempt:
        // shuffling through ten layouts and undoing once gets you home.
        self.edit.arrange.undo.get_or_insert(before);
        self.edit.arrange.result = result.summary();
        self.sync_widgets();
        self.save_layout();
        self.tidy_edit();
        if !result.unplaced.is_empty() {
            self.toast(&format!(
                "No room for {} widget{}: they're out of the grid",
                result.unplaced.len(),
                if result.unplaced.len() == 1 { "" } else { "s" }
            ));
        }
    }

    pub fn arrange_undo(&mut self) {
        let Some(before) = self.edit.arrange.undo.take() else {
            return;
        };
        self.layout.set_rects(&before);
        self.edit.arrange.result.clear();
        self.sync_widgets();
        self.save_layout();
        self.tidy_edit();
        self.toast("Back to how it was");
    }

    pub fn arrange_step(&mut self, index: usize, delta: i32) {
        if let Some(k) = knob_from(index) {
            self.edit.arrange.params.step(k, delta);
            self.tidy_edit();
        }
    }

    pub fn arrange_reset(&mut self, index: usize) {
        if let Some(k) = knob_from(index) {
            self.edit.arrange.params.reset(k);
            self.tidy_edit();
        }
    }

    pub fn arrange_shuffle(&mut self) {
        let p = &mut self.edit.arrange.params;
        p.seed = if p.seed >= 999 { 1 } else { p.seed + 1 };
        self.arrange_now();
    }

    pub fn arrange_copy(&mut self) {
        let line = self.edit.arrange.params.to_kdl();
        self.arrange_copy_text(&line, "Copied the settings: paste them into config.kdl");
    }

    /// Put `text` on the clipboard and say `toast`, for text too long or odd to quote.
    fn arrange_copy_text(&mut self, text: &str, toast: &str) {
        let clip = clipboard::Clip {
            kind: clipboard::ClipKind::Text,
            data: Arc::new(text.as_bytes().to_vec()),
        };
        self.tx.clipboard.tell(clipboard::Command::Set(clip));
        self.toast(toast);
    }
}

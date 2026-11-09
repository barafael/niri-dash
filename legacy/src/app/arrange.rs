//! Edit mode's Arrange tab: lay the dashboard out by itself, and the settings of how.

use super::{App, model};
use crate::Dash;
use crate::autolayout::{self, Knob, Params};
use crate::layout::Rect;
use std::cell::RefCell;
use std::rc::Rc;

pub(super) struct Arrange {
    /// The settings being tried; they start as config.kdl's `autolayout` line.
    params: RefCell<Params>,
    /// Where the widgets were before the last run, to go back to.
    undo: RefCell<Option<Vec<(u32, Rect)>>>,
    /// What the last run made of it, for the line under the buttons.
    result: RefCell<String>,
}

impl Arrange {
    pub fn new(params: Params) -> Self {
        Self {
            params: RefCell::new(params),
            undo: RefCell::new(None),
            result: RefCell::new(String::new()),
        }
    }
}

impl App {
    /// The tab's rows and what hangs on them.
    pub(super) fn refresh_arrange(&self) {
        let p = self.arrange.params.borrow().clone();
        let rows: Vec<crate::SettingRow> = Knob::of(p.algo)
            .into_iter()
            .map(|k| crate::SettingRow {
                index: knob_index(k),
                kind: crate::RowKind::Stepper,
                label: k.label().into(),
                value: p.text(k).into(),
                on: false,
                changed: !p.is_default(k),
                can_dec: p.can_step(k, -1),
                can_inc: p.can_step(k, 1),
                placeholder: Default::default(),
            })
            .collect();
        let ui = self.ui();
        ui.set_arrange_rows(model(rows));
        let result = self.arrange.result.borrow();
        ui.set_arrange_result(if result.is_empty() {
            p.algo.describe().into()
        } else {
            result.as_str().into()
        });
        ui.set_arrange_can_undo(self.arrange.undo.borrow().is_some());
    }

    /// Lay the widgets out with the current settings.
    pub(super) fn arrange_now(&self) {
        let p = self.arrange.params.borrow().clone();
        let before: Vec<(u32, Rect)> = self
            .layout
            .borrow()
            .widgets
            .iter()
            .map(|w| (w.id, w.rect()))
            .collect();
        let result = autolayout::arrange(&mut self.layout.borrow_mut(), &p);
        // Running it again keeps the very first layout to go back to, not the last attempt:
        // shuffling through ten layouts and undoing once gets you home.
        self.arrange.undo.borrow_mut().get_or_insert(before);
        *self.arrange.result.borrow_mut() = result.summary();
        self.sync_widgets();
        self.save_layout();
        self.refresh_arrange();
        if !result.unplaced.is_empty() {
            self.toast(&format!(
                "No room for {} widget{}: they're out of the grid",
                result.unplaced.len(),
                if result.unplaced.len() == 1 { "" } else { "s" }
            ));
        }
    }

    pub(super) fn arrange_undo(&self) {
        let Some(before) = self.arrange.undo.borrow_mut().take() else {
            return;
        };
        self.layout.borrow_mut().set_rects(&before);
        self.arrange.result.borrow_mut().clear();
        self.sync_widgets();
        self.save_layout();
        self.refresh_arrange();
        self.toast("Back to how it was");
    }

    fn arrange_step(&self, index: i32, delta: i32) {
        if let Some(k) = knob_from(index) {
            self.arrange.params.borrow_mut().step(k, delta);
            self.refresh_arrange();
        }
    }

    fn arrange_reset(&self, index: i32) {
        if let Some(k) = knob_from(index) {
            self.arrange.params.borrow_mut().reset(k);
            self.refresh_arrange();
        }
    }

    fn arrange_shuffle(&self) {
        {
            let mut p = self.arrange.params.borrow_mut();
            p.seed = if p.seed >= 999 { 1 } else { p.seed + 1 };
        }
        self.arrange_now();
    }

    fn arrange_copy(&self) {
        let line = self.arrange.params.borrow().to_kdl();
        self.copy_text_as(&line, "Copied the settings: paste them into config.kdl");
    }
}

/// The row index the UI hands back stands for a setting.
fn knob_index(k: Knob) -> i32 {
    ALL.iter().position(|a| *a == k).unwrap_or(0) as i32
}

fn knob_from(index: i32) -> Option<Knob> {
    ALL.get(usize::try_from(index).ok()?).copied()
}

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

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    let a = app.clone();
    ui.on_arrange_now(move || a.arrange_now());
    let a = app.clone();
    ui.on_arrange_shuffle(move || a.arrange_shuffle());
    let a = app.clone();
    ui.on_arrange_undo(move || a.arrange_undo());
    let a = app.clone();
    ui.on_arrange_step(move |i, d| a.arrange_step(i, d));
    let a = app.clone();
    ui.on_arrange_reset(move |i| a.arrange_reset(i));
    let a = app.clone();
    ui.on_arrange_copy(move || a.arrange_copy());
    app.refresh_arrange();
}

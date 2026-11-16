//! Keyboard navigation: the `Nav` global's callbacks (hint labels, moving between tiles) and its
//! state across showing and hiding.

use super::App;
use crate::layout::Rect;
use crate::nav::{self, Dir};
use crate::{Dash, KeyBind, KeyHint, Nav, NavMode, Shape, TipRow, WidgetInst};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::rc::Rc;

/// The cell rectangle a tile covers; round tiles are as tall as they are wide.
fn rect(w: &WidgetInst) -> Rect {
    Rect {
        col: w.col,
        row: w.row,
        w: w.w,
        h: if w.shape == Shape::Round { w.w } else { w.h },
    }
}

fn tips(m: &ModelRc<KeyHint>) -> Vec<nav::Tip> {
    m.iter()
        .map(|h| nav::Tip {
            key: h.key.into(),
            does: h.does.into(),
        })
        .collect()
}

fn binds(m: &ModelRc<KeyBind>) -> Vec<nav::Bind> {
    m.iter()
        .map(|b| nav::Bind {
            key: b.key.into(),
            key2: b.key2.into(),
            does: b.does.into(),
            on: b.on,
        })
        .collect()
}

fn hints(tips: &[nav::Tip]) -> ModelRc<KeyHint> {
    let v: Vec<KeyHint> = tips
        .iter()
        .map(|t| KeyHint {
            key: t.key.as_str().into(),
            does: t.does.as_str().into(),
        })
        .collect();
    ModelRc::new(VecModel::from(v))
}

impl App {
    /// Back to no widget entered, as the dashboard goes away. A standalone app is always in
    /// its one widget.
    pub(super) fn reset_nav(&self) {
        let ui = self.ui();
        let n = ui.global::<Nav>();
        n.set_typed("".into());
        n.set_expanded_id(0);
        n.set_widget_tips(hints(&[]));
        n.set_can_scroll(false);
        if self.solo.is_some() {
            n.set_active_id(ui.get_solo_inst().id);
            n.set_mode(NavMode::Widget);
            n.set_tick(n.get_tick() + 1);
        } else {
            n.set_mode(NavMode::Off);
            n.set_active_id(0);
        }
    }
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    let n = ui.global::<Nav>();
    let a = Rc::downgrade(app);
    n.on_pick(move |typed| {
        let Some(a) = a.upgrade() else { return -1 };
        let labels: Vec<String> = a.widgets.iter().map(|w| w.hint.to_string()).collect();
        match nav::pick(&labels, &typed) {
            Ok(Some(i)) => a.widgets.row_data(i).map_or(-1, |w| w.id),
            Ok(None) => 0,
            Err(()) => -1,
        }
    });
    let a = Rc::downgrade(app);
    n.on_neighbor(move |id, dir| {
        let Some(a) = a.upgrade() else { return 0 };
        let Some(dir) = Dir::from_index(dir) else {
            return 0;
        };
        let tiles: Vec<WidgetInst> = a.widgets.iter().collect();
        let Some(from) = tiles.iter().position(|w| w.id == id) else {
            return 0;
        };
        let rects: Vec<Rect> = tiles.iter().map(rect).collect();
        nav::neighbor(&rects, from, dir).map_or(0, |i| tiles[i].id)
    });
    let weak = ui.as_weak();
    n.on_publish(move |head, own, tail| {
        let own = binds(&own);
        debug_assert!(
            !own.iter()
                .any(|b| nav::DASHBOARD_KEYS.contains(&b.key.as_str())
                    || nav::DASHBOARD_KEYS.contains(&b.key2.as_str())),
            "a widget lists a key the dashboard keeps: {own:?}"
        );
        if let Some(ui) = weak.upgrade() {
            let shown = nav::shown(&tips(&head), &own, &tips(&tail));
            // A scope says its keys again whenever anything they depend on is looked at anew;
            // mostly they are what they were, and then nothing that shows them has to redo it.
            let n = ui.global::<Nav>();
            if tips(&n.get_widget_tips()) != shown {
                n.set_widget_tips(hints(&shown));
            }
        }
    });
    n.on_find_bind(|own, text| nav::find_bind(&binds(&own), &text).map_or(-1, |i| i as i32));
    n.on_pack(|all, width| {
        let rows: Vec<TipRow> = nav::pack(&tips(&all), width)
            .iter()
            .map(|row| TipRow { chips: hints(row) })
            .collect();
        ModelRc::new(VecModel::from(rows))
    });
}

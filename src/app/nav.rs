//! Keyboard navigation: the `Nav` global's callbacks (hint labels, moving between tiles) and its
//! state across showing and hiding.

use super::App;
use crate::layout::Rect;
use crate::nav::{self, Dir};
use crate::{Dash, Nav, NavMode, Shape, WidgetInst};
use slint::{ComponentHandle, Model};
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

impl App {
    /// Back to no widget entered, as the dashboard goes away. A standalone app is always in
    /// its one widget.
    pub(super) fn reset_nav(&self) {
        let ui = self.ui();
        let n = ui.global::<Nav>();
        n.set_typed("".into());
        n.set_expanded_id(0);
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
}

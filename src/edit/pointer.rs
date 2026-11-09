//! The mouse in edit mode: dragging a widget by its card, pulling its edges and corners, carrying
//! one picked up with a double click, dragging a new one in from the palette, and the long press
//! that opens a widget alone for adjusting (`legacy/src/app/hold.rs`).
//!
//! The frames only record where a press began; the pointer is watched at the window
//! ([`view::pointer_layer`](super::view::pointer_layer)), so a drag goes on wherever the pointer
//! goes, and the target (the cells a drop would take) is worked out here from where it is.

use crate::dash::{Dash, Geometry};
use crate::layout::{Kind, Rect, Shape};
use gpui::Context;
use std::time::Duration;

/// How long a press has to last to be a long press.
pub const HOLD: Duration = Duration::from_millis(500);
/// How far it may wander in that time, across and down together.
const SLOP: f32 = 10.0;
/// How far a press on a card moves before it is a drag rather than a click.
const DRAGGED: f32 = 4.0;
/// How far a press on a palette entry moves before the widget comes along.
const PULLED: f32 = 8.0;

pub type Point = (f32, f32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DragMode {
    #[default]
    None,
    /// A new widget dragged in from the palette.
    Add,
    /// A widget picked up with a double click.
    Carry,
}

/// A new widget dragged in from the palette, or a picked-up widget following the pointer.
#[derive(Debug, Clone, Default)]
pub struct Drag {
    pub mode: DragMode,
    pub kind: Option<Kind>,
    pub title: &'static str,
    pub id: u32,
    /// Its size in cells.
    pub w: i32,
    pub h: i32,
    /// The pointer, and where in the widget it holds it.
    pub pos: Point,
    pub grab: Point,
    /// Whether a drop here would work.
    pub ok: bool,
    /// The cell the widget is over (and whether that is on the grid), to see when it changes.
    last: Option<(i32, i32, bool)>,
    /// The mouse-up that ends the double click is coming: it doesn't drop.
    skip_up: bool,
}

/// A press on a palette entry, before it is a click or a drag.
#[derive(Debug, Clone, Copy, Default)]
pub struct PalettePress {
    /// Which entry, and where the pointer went down.
    pub pressing: Option<(usize, Point)>,
    /// The press went far enough to take the widget along (so letting go is no click).
    pub dragged_out: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressKind {
    /// The card: a drag moves the widget.
    Move,
    /// An edge or corner: the side (-1 left or top, 1 right or bottom, 0 neither) it moves on
    /// each axis.
    Handle { x: i32, y: i32 },
}

/// A cell rectangle a drag would drop on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub col: i32,
    pub row: i32,
    pub w: i32,
    pub h: i32,
}

/// A press on a widget's card or one of its handles, going on.
#[derive(Debug, Clone)]
pub struct Press {
    pub id: u32,
    pub kind: PressKind,
    pub down: Point,
    pub pos: Point,
    /// Moved far enough to count as a drag rather than a click.
    pub dragged: bool,
    /// Whether the current target has room (after pushing others aside).
    pub t_ok: bool,
    last: Option<Target>,
}

impl Press {
    pub fn new(id: u32, kind: PressKind, at: Point) -> Self {
        Self {
            id,
            kind,
            down: at,
            pos: at,
            dragged: false,
            t_ok: true,
            last: None,
        }
    }

    /// The handle being dragged: the side it moves on each axis.
    pub fn pull(&self) -> (i32, i32) {
        match self.kind {
            PressKind::Move => (0, 0),
            PressKind::Handle { x, y } => (x, y),
        }
    }

    pub fn sizing(&self) -> bool {
        self.pull() != (0, 0)
    }

    pub fn moving(&self) -> bool {
        self.kind == PressKind::Move
    }
}

/// A press held still, waiting to become a long press.
#[derive(Debug, Default)]
pub struct Hold {
    pub at: Option<Point>,
    serial: u64,
}

impl Hold {
    pub fn cancel(&mut self) {
        self.at = None;
        self.serial += 1;
    }
}

/// What a frame needs to know to draw a drag: how far the pointer has pulled it.
#[derive(Debug, Clone, Copy)]
pub struct Pulled {
    /// Pointer offset since the press, for a move (the card follows the pointer).
    pub dx: f32,
    pub dy: f32,
    pub target: Target,
    /// The pointer asks for more than it gets, or there is no room: the handle turns red.
    pub blocked: bool,
}

impl Dash {
    fn grid_geometry(&self) -> Geometry {
        self.geometry(self.size.0, self.size.1)
    }

    /// Where the grid is and what the pointer asks, for the frame of widget `id`.
    pub fn pulled(&self, id: u32) -> Option<Pulled> {
        let p = self.edit.press.as_ref().filter(|p| p.id == id)?;
        let (target, blocked) = self.press_target(p)?;
        let t_ok = p.t_ok;
        let (dx, dy) = if p.moving() {
            (p.pos.0 - p.down.0, p.pos.1 - p.down.1)
        } else {
            (0.0, 0.0)
        };
        Some(Pulled {
            dx,
            dy,
            target,
            blocked: !t_ok || blocked,
        })
    }

    /// What a press on a card or handle asks for: the cells a drop would take, and whether the
    /// pointer asks for more than it gets.
    fn press_target(&self, p: &Press) -> Option<(Target, bool)> {
        let t = self.tile(p.id)?;
        let g = self.grid_geometry();
        let cell = g.cell;
        if cell <= 0.0 {
            return None;
        }
        let pl = &t.placed;
        let round = pl.shape == Shape::Round;
        let cells_h = if round { pl.w } else { pl.h };
        let (px, py) = p.pull();
        let (pulled_x, pulled_y) = (p.pos.0 - p.down.0, p.pos.1 - p.down.1);
        // A pinned widget can't be dragged away.
        let (dx, dy) = if p.moving() && !pl.pinned { (pulled_x, pulled_y) } else { (0.0, 0.0) };
        let (pulled_x, pulled_y) = if p.sizing() { (pulled_x, pulled_y) } else { (0.0, 0.0) };
        // What the pointer asks for. A round widget stays square, whichever way it is pulled.
        let sum = px as f32 * pulled_x + py as f32 * pulled_y;
        let wanted_w = if !round {
            px as f32 * pulled_x
        } else if px != 0 && py != 0 {
            sum / 2.0
        } else {
            sum
        };
        let wanted_h = if round { wanted_w } else { py as f32 * pulled_y };
        // What it gets: down to the minimum size, out to the grid's edge.
        let (min_w, min_h) = pl.min_size();
        let room_w = (if px < 0 { pl.col } else { g.cols - pl.col - pl.w }).max(0) as f32 * cell;
        let room_h = (if py < 0 { pl.row } else { g.rows - pl.row - cells_h }).max(0) as f32 * cell;
        let hi_w = if round { room_w.min(room_h) } else { room_w };
        let lo_w = (min_w - pl.w).min(0) as f32 * cell;
        let stretch_w = wanted_w.clamp(lo_w.min(hi_w), hi_w);
        let lo_h = (min_h - cells_h).min(0) as f32 * cell;
        let stretch_h = if round {
            stretch_w
        } else {
            wanted_h.clamp(lo_h.min(room_h), room_h)
        };
        let t_w = pl.w + (stretch_w / cell).round() as i32;
        let t_h = cells_h + (stretch_h / cell).round() as i32;
        let t_col = if px < 0 {
            pl.col + pl.w - t_w
        } else {
            (((pl.col as f32 * cell + dx) / cell).round() as i32).clamp(0, (g.cols - pl.w).max(0))
        };
        let t_row = if py < 0 {
            pl.row + cells_h - t_h
        } else {
            (((pl.row as f32 * cell + dy) / cell).round() as i32).clamp(0, (g.rows - cells_h).max(0))
        };
        let blocked = wanted_w != stretch_w || wanted_h != stretch_h;
        Some((
            Target {
                col: t_col,
                row: t_row,
                w: t_w,
                h: t_h,
            },
            blocked,
        ))
    }

    /// A press began on the card of widget `id` (a drag moves it) or on one of its handles.
    pub fn press_start(&mut self, id: u32, kind: PressKind, at: Point) {
        let Some(t) = self.tile(id) else { return };
        let pinned = t.placed.pinned;
        // Handles of a pinned widget don't take a press.
        if pinned && matches!(kind, PressKind::Handle { .. }) {
            return;
        }
        if !(self.editing || self.edit.adjust_id == id) || self.edit.dragging() {
            return;
        }
        let mut press = Press::new(id, kind, at);
        press.last = self.press_target(&press).map(|(t, _)| t);
        self.edit.press = Some(press);
    }

    /// The pointer moved, with `pressed` the button held (if any).
    pub fn pointer_moved(&mut self, pos: Point, pressed_left: bool, cx: &mut Context<Self>) {
        let busy = self.edit.hold.at.is_some()
            || self.edit.press.is_some()
            || self.edit.pal_press.pressing.is_some()
            || self.edit.dragging();
        if !busy {
            return;
        }
        // The button went up out of sight (outside the window): it is let go of here. (A carried
        // widget follows the pointer with no button down.)
        if !pressed_left && (!self.edit.dragging() || self.edit.drag.mode == DragMode::Add) {
            if self.edit.dragging() {
                self.finish_drag(false);
                cx.notify();
            } else {
                self.pointer_up(pos, true, cx);
            }
            return;
        }
        if let Some(at) = self.edit.hold.at
            && (pos.0 - at.0).abs() + (pos.1 - at.1).abs() >= SLOP
        {
            self.edit.hold.cancel();
        }
        if self.edit.dragging() {
            self.edit.drag.pos = pos;
            self.update_drag();
        } else if let Some((i, from)) = self.edit.pal_press.pressing
            && (pos.0 - from.0).abs() + (pos.1 - from.1).abs() > PULLED
        {
            self.start_add_drag(i, pos);
        }
        if self.edit.press.is_some() {
            self.press_moved(pos);
        }
        cx.notify();
    }

    /// The pointer moved with a card or handle pressed: its target is worked out, and the others
    /// make room when it changed.
    fn press_moved(&mut self, pos: Point) {
        let Some(mut press) = self.edit.press.take() else {
            return;
        };
        press.pos = pos;
        let pinned = self.tile(press.id).is_some_and(|t| t.placed.pinned);
        // A pinned widget can't be dragged away.
        if press.moving() && !pinned {
            let (dx, dy) = (pos.0 - press.down.0, pos.1 - press.down.1);
            if dx.abs() > DRAGGED || dy.abs() > DRAGGED {
                press.dragged = true;
            }
        }
        let target = self.press_target(&press).map(|(t, _)| t);
        if target != press.last && (press.sizing() || (press.moving() && !pinned)) {
            press.last = target;
            if let Some(t) = target {
                press.t_ok = if press.moving() {
                    self.preview_move(press.id, t.col, t.row)
                } else {
                    self.preview_resize(
                        press.id,
                        Rect {
                            col: t.col,
                            row: t.row,
                            w: t.w,
                            h: t.h,
                        },
                    )
                };
            }
        }
        self.edit.press = Some(press);
    }

    /// A mouse button went up at `pos`.
    pub fn pointer_up(&mut self, pos: Point, left: bool, cx: &mut Context<Self>) {
        self.edit.hold.cancel();
        self.edit.pal_press.pressing = None;
        if self.edit.dragging() {
            if self.edit.drag.skip_up {
                self.edit.drag.skip_up = false;
                return;
            }
            self.edit.drag.pos = pos;
            self.update_drag();
            self.finish_drag(left);
            cx.notify();
            return;
        }
        if let Some(mut press) = self.edit.press.take() {
            if !left {
                // Not the button that pressed: the drag goes on.
                self.edit.press = Some(press);
                return;
            }
            press.pos = pos;
            self.finish_press(press, cx);
            cx.notify();
        }
    }

    /// A card or handle was let go of: the widget goes where the drag asked, if there is room.
    fn finish_press(&mut self, press: Press, cx: &mut Context<Self>) {
        let target = self.press_target(&press).map(|(t, _)| t);
        let id = press.id;
        let Some(pl) = self.tile(id).map(|t| t.placed.clone()) else {
            self.set_preview(None);
            return;
        };
        let cells_h = if pl.shape == Shape::Round { pl.w } else { pl.h };
        let ok = press.t_ok;
        match (press.kind, target) {
            (PressKind::Move, Some(t)) => {
                if ok && (t.col != pl.col || t.row != pl.row) {
                    self.move_widget(id, t.col, t.row);
                } else {
                    self.set_preview(None);
                }
                // Opened on its own the toolbar has the settings: letting go of the long press
                // that opened it, or of a drag, stays a move.
                let has_settings = !pl.kind.info().settings.is_empty();
                if !press.dragged && self.edit.adjust_id != id && has_settings {
                    self.configure_click(id, cx);
                }
            }
            (PressKind::Handle { .. }, Some(t)) => {
                let same = t.col == pl.col && t.row == pl.row && t.w == pl.w && t.h == cells_h;
                if ok && !same {
                    self.resize_widget(id, t.col, t.row, t.w, t.h);
                } else {
                    self.set_preview(None);
                }
            }
            _ => self.set_preview(None),
        }
    }

    /// A click on a widget's settings button (or on its card): its settings open.
    pub fn configure_click(&mut self, id: u32, cx: &mut Context<Self>) {
        self.edit.adjust_id = 0;
        self.edit.canvas_keys = false;
        self.configure_widget(id, cx);
    }

    // ---- carrying and dragging in -----------------------------------------------------------

    /// A double click on an empty part of widget `id`'s card, `at` where the pointer is: the
    /// widget is picked up. A pinned widget isn't.
    pub fn grab_widget(&mut self, id: u32, at: Point) {
        if self.edit.drag.mode != DragMode::None {
            return;
        }
        let Some(t) = self.tile(id) else { return };
        if t.placed.pinned {
            return;
        }
        let (w, h) = (t.placed.w, t.placed.h);
        let g = self.grid_geometry();
        let origin = (
            g.x + t.placed.col as f32 * g.cell,
            g.y + t.placed.row as f32 * g.cell,
        );
        if !self.editing {
            self.edit.adjust_id = id;
        }
        self.edit.press = None;
        self.edit.hold.cancel();
        self.edit.drag = Drag {
            mode: DragMode::Carry,
            id,
            w,
            h,
            pos: at,
            grab: (at.0 - origin.0, at.1 - origin.1),
            ok: true,
            skip_up: true,
            ..Drag::default()
        };
        self.update_drag();
    }

    /// The press on palette entry `i` has gone far enough: its widget comes along.
    fn start_add_drag(&mut self, i: usize, pos: Point) {
        let Some(item) = self.edit.palette.get(i).cloned() else {
            return;
        };
        let g = self.grid_geometry();
        self.edit.drag = Drag {
            mode: DragMode::Add,
            kind: Some(item.kind),
            title: item.title,
            w: item.w,
            h: item.h,
            pos,
            grab: (item.w as f32 * g.cell / 2.0, item.h as f32 * g.cell / 2.0),
            ok: false,
            ..Drag::default()
        };
        self.edit.pal_press.dragged_out = true;
        self.update_drag();
    }

    /// A press began on palette entry `i`.
    pub fn palette_press(&mut self, i: usize, at: Point) {
        if self.edit.dragging() {
            return;
        }
        self.edit.pal_press = PalettePress {
            pressing: Some((i, at)),
            dragged_out: false,
        };
    }

    /// The cell a dragged-in or carried widget is over, and whether the pointer is on the grid.
    pub fn drag_cell(&self) -> (i32, i32, bool) {
        let d = &self.edit.drag;
        let g = self.grid_geometry();
        let over = d.pos.0 >= g.x
            && d.pos.0 < g.x + g.cols as f32 * g.cell
            && d.pos.1 >= g.y
            && d.pos.1 < g.y + g.rows as f32 * g.cell;
        let col = (((d.pos.0 - d.grab.0 - g.x) / g.cell).round() as i32).clamp(0, (g.cols - d.w).max(0));
        let row = (((d.pos.1 - d.grab.1 - g.y) / g.cell).round() as i32).clamp(0, (g.rows - d.h).max(0));
        (col, row, over)
    }

    /// Others make room, or don't, for where the dragged widget is now.
    fn update_drag(&mut self) {
        let (col, row, over) = self.drag_cell();
        if self.edit.drag.last == Some((col, row, over)) {
            return;
        }
        self.edit.drag.last = Some((col, row, over));
        match self.edit.drag.mode {
            DragMode::Add => {
                let kind = self.edit.drag.kind;
                let ok = over && kind.is_some_and(|k| self.preview_add(k, col, row));
                self.edit.drag.ok = ok;
                if !over {
                    self.set_preview(None);
                }
            }
            DragMode::Carry => {
                let id = self.edit.drag.id;
                self.edit.drag.ok = self.preview_move(id, col, row);
            }
            DragMode::None => {}
        }
    }

    /// The drag ends: a drop that has room lands, anything else puts everything back.
    pub fn finish_drag(&mut self, drop: bool) {
        if self.edit.drag.mode == DragMode::None {
            return;
        }
        let (col, row, over) = self.drag_cell();
        let d = std::mem::take(&mut self.edit.drag);
        let ok = d.ok && drop;
        match d.mode {
            DragMode::Add => match d.kind {
                Some(kind) if ok && over => self.add_widget_at(kind, col, row),
                _ => self.set_preview(None),
            },
            DragMode::Carry => {
                if ok {
                    self.move_widget(d.id, col, row);
                } else {
                    self.set_preview(None);
                }
            }
            DragMode::None => {}
        }
    }

    // ---- the long press -----------------------------------------------------------------------

    /// A left press began at `at`: it is a long press if it lasts.
    pub fn hold_start(&mut self, at: Point, cx: &mut Context<Self>) {
        if !self.visible || self.solo.is_some() {
            return;
        }
        let hold = &mut self.edit.hold;
        hold.at = Some(at);
        hold.serial += 1;
        let serial = hold.serial;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HOLD).await;
            this.update(cx, |d, cx| {
                if d.edit.hold.serial == serial {
                    d.held(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// The widget under a point in the window.
    pub fn widget_at(&self, at: Point) -> Option<u32> {
        let g = self.grid_geometry();
        let col = ((at.0 - g.x) / g.cell).floor() as i32;
        let row = ((at.1 - g.y) / g.cell).floor() as i32;
        self.tiles
            .iter()
            .find(|t| {
                let r = t.rect();
                (r.col..r.col + r.w).contains(&col) && (r.row..r.row + r.h).contains(&row)
            })
            .map(|t| t.id())
    }

    /// A press held still on a widget: open it on its own for adjusting.
    fn held(&mut self, cx: &mut Context<Self>) {
        let Some(at) = self.edit.hold.at.take() else {
            return;
        };
        // A blown-up widget is the only one on show, and not where the grid has it.
        let blown_up = self.nav.expanded_id != 0
            && self.nav.mode == crate::dash::NavMode::Widget
            && !self.editing;
        let id = if blown_up {
            Some(self.nav.expanded_id)
        } else {
            self.widget_at(at)
        };
        let Some(id) = id else { return };
        if !self.hold_widget(id, cx) {
            return;
        }
        // Whatever the press was on lets go of it; the same press goes on to move the widget.
        // A blown-up widget shrinks back to its place, away from the pointer: there is nothing
        // under it to go on dragging.
        if !blown_up {
            self.press_start(id, PressKind::Move, at);
        }
        cx.notify();
    }

    /// A press held still on widget `id`: open it on its own for adjusting, whatever the dashboard
    /// was doing: the keyboard's hints, an entered or blown-up widget, a tray menu, another widget
    /// being adjusted all give way. False if it stays as it is: a standalone app, edit mode (every
    /// widget is open already), a drag going on, or this very widget open already.
    pub fn hold_widget(&mut self, id: u32, cx: &mut Context<Self>) -> bool {
        if self.solo.is_some() || self.editing || self.edit.dragging() || self.edit.adjust_id == id {
            return false;
        }
        if self.tile(id).is_none() {
            return false;
        }
        // A menu of a widget gives way.
        crate::widgets::escape(self, cx);
        if self.nav.mode != crate::dash::NavMode::Off || self.nav.expanded_id != 0 {
            self.nav_off();
        }
        self.edit.adjust_id = id;
        self.edit.press = None;
        true
    }
}

//! Drawing edit mode: what goes over and under the frames. The frames' own edit controls are in
//! `frame`, the sidebar in `drawer`, the settings card in `card`, the top bar in `bar`; `parts` are
//! the buttons and rows they are made of.

pub mod bar;
pub mod card;
pub mod drawer;
pub mod frame;
pub mod parts;

use crate::dash::{Dash, Geometry};
use crate::edit::anim::Ease;
use crate::edit::DragMode;
use parts::RowOwner;
use crate::kit::Tk;
use gpui::prelude::*;
use gpui::{
    AnyElement, BorderStyle, Bounds, Context, DispatchPhase, Div, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Window, canvas, div, point, px, size,
};

impl Dash {
    /// What a row of the sidebar's lists does when stepped (`pos` is its position in its list).
    pub fn row_step(&mut self, owner: RowOwner, pos: usize, delta: i32) {
        match owner {
            RowOwner::Grid => self.grid_step(pos, delta),
            RowOwner::Arrange => {
                if let Some(row) = self.arrange_rows().get(pos) {
                    self.arrange_step(row.index, delta);
                }
            }
            RowOwner::Card => {
                if let Some(row) = self.setting_rows().get(pos) {
                    self.setting_step(row.index, delta);
                }
            }
            RowOwner::Style => {
                if let Some(row) = self.edit.style_rows.get(pos) {
                    let var = row.var;
                    self.style_step(var, delta);
                }
            }
        }
    }

    /// A row's reset button.
    pub fn row_reset(&mut self, owner: RowOwner, pos: usize) {
        match owner {
            RowOwner::Grid => self.grid_auto(),
            RowOwner::Arrange => {
                if let Some(row) = self.arrange_rows().get(pos) {
                    self.arrange_reset(row.index);
                }
            }
            RowOwner::Card => {
                if let Some(row) = self.setting_rows().get(pos) {
                    self.setting_reset(row.index);
                }
            }
            RowOwner::Style => {
                if let Some(row) = self.edit.style_rows.get(pos) {
                    let var = row.var;
                    self.style_reset(Some(var));
                }
            }
        }
    }

    /// The lines of the grid, in edit mode: a cell each, faint.
    pub(crate) fn edit_grid(&self, g: &Geometry, tk: &Tk) -> Option<AnyElement> {
        let fade = self
            .edit
            .anim
            .follow(("grid-fade", 0), if self.editing { 1.0 } else { 0.0 }, 160.0, tk.motion(), Ease::Out);
        let fade = fade.max(if self.edit.dragging() { 1.0 } else { 0.0 });
        if fade <= 0.01 {
            return None;
        }
        let (cols, rows, cell, gap) = (g.cols, g.rows, g.cell, tk.gap());
        let (gx, gy) = (g.x, g.y);
        let radius = tk.radius();
        let bg = tk.subtle().opacity(fade);
        let border = tk.border().opacity(fade);
        Some(
            canvas(
                |_, _, _| (),
                move |_, _, window, _| {
                    for i in 0..cols * rows {
                        let (c, r) = (i % cols, i / cols);
                        let bounds = Bounds::new(
                            point(
                                px(gx + c as f32 * cell + gap),
                                px(gy + r as f32 * cell + gap),
                            ),
                            size(px((cell - 2.0 * gap).max(0.0)), px((cell - 2.0 * gap).max(0.0))),
                        );
                        window.paint_quad(gpui::quad(
                            bounds,
                            px(radius),
                            bg,
                            px(1.0),
                            border,
                            BorderStyle::Solid,
                        ));
                    }
                },
            )
            .absolute()
            .size_full()
            .into_any_element(),
        )
    }

    /// Where a dragged-in or carried widget will land.
    pub(crate) fn landing_ghost(&self, g: &Geometry, tk: &Tk) -> Option<AnyElement> {
        let d = &self.edit.drag;
        if d.mode == DragMode::None {
            return None;
        }
        let (col, row, over) = self.drag_cell();
        if d.mode == DragMode::Add && !over {
            return None;
        }
        let (cell, gap, m) = (g.cell, tk.gap(), tk.motion());
        let a = &self.edit.anim;
        let x = a.follow(("ghost-x", 0), col as f32, 90.0, m, Ease::Out);
        let y = a.follow(("ghost-y", 0), row as f32, 90.0, m, Ease::Out);
        let color = if d.ok { tk.good() } else { tk.bad() };
        Some(
            div()
                .absolute()
                .left(px(g.x + x * cell + gap))
                .top(px(g.y + y * cell + gap))
                .w(px((d.w as f32 * cell - 2.0 * gap).max(0.0)))
                .h(px((d.h as f32 * cell - 2.0 * gap).max(0.0)))
                .rounded(px(tk.radius()))
                .border_2()
                .border_color(color)
                .bg(crate::kit::with_alpha(color, 0.12))
                .into_any_element(),
        )
    }

    /// While one widget is opened for adjusting, the others don't react: a click on them (or
    /// anywhere outside) ends adjusting instead of, say, focusing a window and closing.
    pub(crate) fn adjust_catcher(&self, g: &Geometry, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.edit.adjust_id == 0 || self.edit.dragging() {
            return None;
        }
        Some(
            div()
                .id("adjust-catcher")
                .absolute()
                .left(px(g.x))
                .top(px(g.y))
                .w(px(g.cols as f32 * g.cell))
                .h(px(g.rows as f32 * g.cell))
                .occlude()
                .on_click(cx.listener(|d, _, _, cx| {
                    d.edit.adjust_id = 0;
                    cx.notify();
                }))
                .into_any_element(),
        )
    }

    /// Behind an expanded widget the others fade back; a click anywhere puts it back.
    pub(crate) fn expand_shade(&self, g: &Geometry, tk: &Tk, cx: &mut Context<Self>) -> Option<AnyElement> {
        let on = self.expand_on();
        let fade = self
            .edit
            .anim
            .follow(("shade", 0), if on { 1.0 } else { 0.0 }, 200.0, tk.motion(), Ease::Out);
        if !on && fade <= 0.001 {
            return None;
        }
        Some(
            div()
                .id("expand-shade")
                .absolute()
                .left(px(g.x))
                .top(px(g.y))
                .w(px(g.cols as f32 * g.cell))
                .h(px(g.rows as f32 * g.cell))
                .opacity(fade)
                .bg(tk.c(crate::theme::ColorRole::Backdrop).with_alpha(0.75).hsla())
                .occlude()
                .on_click(cx.listener(|d, _, _, cx| {
                    d.nav.expanded_id = 0;
                    cx.notify();
                }))
                .into_any_element(),
        )
    }

    /// Edit mode's buttons sit over the drawer, flush with its edges, and slide in with it.
    pub(crate) fn edit_buttons(&self, tk: &Tk, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.editing {
            return None;
        }
        let (x, _) = self.drawer_x(self.size.0);
        let reset_on = self.side_on(crate::edit::DrawerTab::Widgets, 2 + self.edit.palette.len());
        Some(
            div()
                .absolute()
                .left(px(x))
                .top(px(13.0))
                .w(px(crate::edit::DRAWER_W))
                .h(px(30.0))
                .flex()
                .flex_row()
                .justify_between()
                .child(parts::text_btn(
                    "reset-layout",
                    "Reset layout",
                    Some("Reset"),
                    Some("reset"),
                    false,
                    parts::Look {
                        kfocus: reset_on,
                        ..parts::Look::default()
                    },
                    tk,
                    cx.listener(|d, _, _, cx| {
                        d.reset_layout();
                        cx.notify();
                    }),
                ))
                .child(parts::text_btn(
                    "done-editing",
                    "Done",
                    None,
                    Some("check"),
                    true,
                    parts::Look::default(),
                    tk,
                    cx.listener(|d, _, _, cx| d.set_editing(false, cx)),
                ))
                .into_any_element(),
        )
    }

    /// The widget being dragged in from the drawer, under the pointer.
    pub(crate) fn add_ghost(&self, g: &Geometry, tk: &Tk) -> Option<AnyElement> {
        let d = &self.edit.drag;
        if d.mode != DragMode::Add {
            return None;
        }
        let w = (d.w as f32 * g.cell).clamp(120.0, 260.0);
        Some(
            div()
                .absolute()
                .left(px(d.pos.0 - w / 2.0))
                .top(px(d.pos.1 - 24.0))
                .w(px(w))
                .h(px(48.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(24.0_f32.min(tk.radius_small() + 4.0)))
                .bg(tk.card_raised())
                .border_1()
                .border_color(if d.ok { tk.good() } else { tk.accent() })
                .text_size(px(tk.font_body()))
                .text_color(tk.text())
                .child(format!("{} · {}×{}", d.title, d.w, d.h))
                .into_any_element(),
        )
    }

    /// Watches the pointer for the whole window: a press began on a card or handle goes on
    /// wherever the pointer goes, a carried widget follows it, a long press needs to know it
    /// stayed. The frames only say where a press began.
    pub(crate) fn pointer_layer(&self, cx: &mut Context<Self>) -> Div {
        let dash = cx.entity();
        div().absolute().size_full().child(
            canvas(
                |_, _, _| (),
                move |_, _, window: &mut Window, _| {
                    let d = dash.clone();
                    window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                        if phase != DispatchPhase::Capture {
                            return;
                        }
                        let (pos, left) = (
                            (f32::from(ev.position.x), f32::from(ev.position.y)),
                            ev.pressed_button == Some(MouseButton::Left),
                        );
                        d.update(cx, |d, cx| {
                            d.hover_moved(pos, cx);
                            d.pointer_moved(pos, left, cx);
                        });
                    });
                    let d = dash.clone();
                    window.on_mouse_event(move |ev: &MouseDownEvent, phase, window, cx| {
                        if phase != DispatchPhase::Capture || ev.button != MouseButton::Left {
                            return;
                        }
                        let pos = (f32::from(ev.position.x), f32::from(ev.position.y));
                        // A click takes the keyboard back from a text field of the card; the
                        // field's own press (coming next) gives it to the one clicked.
                        let focus = d.read(cx).edit.field.focused.then(|| d.read(cx).focus.clone());
                        if let Some(focus) = focus {
                            window.focus(&focus, cx);
                        }
                        d.update(cx, |d, cx| d.hold_start(pos, cx));
                    });
                    let d = dash.clone();
                    window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
                        if phase != DispatchPhase::Capture {
                            return;
                        }
                        let pos = (f32::from(ev.position.x), f32::from(ev.position.y));
                        let left = ev.button == MouseButton::Left;
                        d.update(cx, |d, cx| d.pointer_up(pos, left, cx));
                    });
                },
            )
            .absolute()
            .size_full(),
        )
    }

    /// While a card or handle is being pulled the pointer keeps the shape of what it holds,
    /// wherever it goes.
    pub(crate) fn press_cursor(&self) -> Option<Div> {
        let cursor = if let Some(p) = &self.edit.press {
            match p.pull() {
                (0, 0) => div().cursor_move(),
                (0, _) => div().cursor_ns_resize(),
                (_, 0) => div().cursor_ew_resize(),
                (x, y) if x == y => div().cursor_nwse_resize(),
                _ => div().cursor_nesw_resize(),
            }
        } else if self.edit.dragging() {
            div().cursor_move()
        } else {
            return None;
        };
        Some(cursor.absolute().size_full())
    }

    /// The frame under the pointer, in edit mode, for the handles to show.
    fn hover_moved(&mut self, pos: (f32, f32), cx: &mut Context<Self>) {
        let hover = if self.editing || self.edit.adjust_id != 0 {
            self.widget_at(pos)
        } else {
            None
        };
        if hover != self.edit.hover {
            self.edit.hover = hover;
            cx.notify();
        }
    }
}

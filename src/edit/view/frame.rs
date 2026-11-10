//! One placed widget (`WidgetFrame` in frame.slint): its card with the widget inside, and in edit
//! mode the eight handles to pull it by, the toolbar (settings, shape, pin, remove), the landing
//! ghost of a drag, and the lock of a pinned widget. Positions are grid cells; while dragging,
//! the card follows the pointer and a ghost shows where the widget will land, green if it fits.

use super::parts::{Look, eid, icon_btn};
use crate::dash::{Dash, Geometry, NavMode, NavOp, Tile};
use crate::edit::anim::Ease;
use crate::edit::{DragMode, PressKind};
use crate::kit::{self, AriaCursor, Tk};
use crate::theme::ColorRole;
use crate::widgets::{self, Ctx};
use gpui::prelude::*;
use gpui::{
    AnyElement, BoxShadow, Context, Div, MouseButton, MouseDownEvent, Role, Stateful, div, point,
    px,
};

/// The handles: the side each moves on each axis (-1 left or top, 1 right or bottom). Corners last,
/// on top of the edges.
const HANDLES: [(i32, i32); 8] = [
    (0, -1),
    (-1, 0),
    (1, 0),
    (0, 1),
    (-1, -1),
    (1, -1),
    (-1, 1),
    (1, 1),
];

/// The window point of a mouse event.
pub fn at(p: gpui::Point<gpui::Pixels>) -> (f32, f32) {
    (f32::from(p.x), f32::from(p.y))
}

impl Dash {
    /// A widget is blown up in the middle (`e`).
    pub fn expand_on(&self) -> bool {
        self.nav.expanded_id != 0 && self.nav.mode == NavMode::Widget && !self.editing
    }

    /// How far above the others a frame is drawn: 0 normal, 1 while it is in hand, 2 blown up or
    /// shrinking back from it.
    pub fn frame_level(&self, id: u32) -> u8 {
        let e = &self.edit;
        let expanded = self.expand_on() && self.nav.expanded_id == id;
        if expanded || e.anim.value(("fexp", id)) > 0.001 {
            2
        } else if e.press.as_ref().is_some_and(|p| p.id == id)
            || (e.drag.mode == DragMode::Carry && e.drag.id == id)
            || e.adjust_id == id
            || self.op_of(id) != NavOp::None
        {
            1
        } else {
            0
        }
    }

    /// The keyboard's move or resize mode, if it is on widget `id`.
    pub fn op_of(&self, id: u32) -> NavOp {
        if self.op_target() == id {
            self.op()
        } else {
            NavOp::None
        }
    }

    /// What the veil on the widget in hand says besides the mode's name: how big it is, and the
    /// keys.
    fn op_tip(&self, op: NavOp) -> Vec<String> {
        let mut lines = vec![self.edit.size_note.clone()];
        lines.extend(
            match op {
                NavOp::Move => &["h j k l or arrows: one cell", "Ctrl+R resize · Esc done"][..],
                _ => &[
                    "h j k l: grow that side",
                    "Shift: shrink it",
                    "Ctrl+M move · Esc done",
                ][..],
            }
            .iter()
            .map(|l| l.to_string()),
        );
        lines
    }

    /// Move or resize mode: a veil with the mode's name and keys on the card. Slightly see-through,
    /// so the widget stays recognisable under it. Resizing marks the four sides that can be pulled.
    fn op_veil(&self, op: NavOp, round: bool, tk: &Tk) -> Div {
        let arrow = |glyph: &'static str| {
            div()
                .absolute()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(tk.font_small()))
                .text_color(tk.accent())
                .child(glyph)
        };
        let mut veil = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(tk.c(ColorRole::Backdrop).with_alpha(0.85).hsla())
            .debug_selector(|| "mode veil".into())
            .child(
                div()
                    .absolute()
                    .size_full()
                    .rounded(px(if round { 1.0e4 } else { tk.radius() }))
                    .border_2()
                    .border_color(tk.accent()),
            );
        if op == NavOp::Resize {
            veil = veil
                .child(arrow("▲").top_0().left_0().w_full().h(px(18.0)))
                .child(arrow("▼").bottom_0().left_0().w_full().h(px(18.0)))
                .child(arrow("◀").left(px(2.0)).top_0().w(px(18.0)).h_full())
                .child(arrow("▶").right(px(2.0)).top_0().w(px(18.0)).h_full());
        }
        veil.child(
            div()
                .absolute()
                .size_full()
                .p(px(22.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .justify_center()
                .items_center()
                .text_center()
                .child(
                    div()
                        .text_size(px(tk.font_body()))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(tk.accent())
                        .child(if op == NavOp::Move { "MOVE" } else { "RESIZE" }),
                )
                .child(
                    div()
                        .text_size(px(tk.font_small()))
                        .text_color(tk.text())
                        .flex()
                        .flex_col()
                        .items_center()
                        .children(self.op_tip(op).into_iter().map(|l| div().child(l))),
                ),
        )
    }

    /// One placed widget: its card, with the widget inside.
    pub(crate) fn frame(
        &self,
        tile: &Tile,
        g: &Geometry,
        tk: &Tk,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = tile.id();
        let pl = &tile.placed;
        let round = tile.round();
        let (cell, gap) = (g.cell, tk.gap());
        let m = tk.motion();
        let anim = &self.edit.anim;
        let cells_h = if round { pl.w } else { pl.h };
        let press = self.edit.press.as_ref().filter(|p| p.id == id);
        let pulled = self.pulled(id);
        let moving = press.is_some_and(|p| p.moving());
        let sizing = press.is_some_and(|p| p.sizing());
        let (pull_x, pull_y) = press.map_or((0, 0), |p| p.pull());
        let carried = self.edit.drag.mode == DragMode::Carry && self.edit.drag.id == id;
        let lifted = self.edit.adjust_id == id;
        let op = self.op_of(id);
        let editing = self.editing || lifted;
        let selected = self.editing && self.edit.settings_id == id;
        let entered = self.nav.mode == NavMode::Widget && self.nav.active_id == id;
        let expanded = self.expand_on() && self.nav.expanded_id == id;
        let pinned = pl.pinned;
        let hovered = self.edit.hover == Some(id);
        let (t_w, t_h, t_col, t_row) = pulled.map_or((pl.w, cells_h, pl.col, pl.row), |p| {
            (p.target.w, p.target.h, p.target.col, p.target.row)
        });
        let (dx, dy) = pulled.map_or((0.0, 0.0), |p| (p.dx, p.dy));
        let blocked = pulled.is_some_and(|p| p.blocked);
        let t_ok = press.is_none_or(|p| p.t_ok);

        // Where it sits and how big it is, in cells: it glides as the layout changes. Dragging
        // follows the pointer directly; resizing steps from cell to cell, quickly.
        let ms = if moving || carried {
            0.0
        } else if sizing {
            90.0
        } else {
            240.0
        };
        let f = |k: &'static str, v: f32| anim.follow((k, id), v, ms, m, Ease::Soft);
        let (xc, yc) = if carried {
            let d = &self.edit.drag;
            (
                (d.pos.0 - d.grab.0 - g.x) / cell,
                (d.pos.1 - d.grab.1 - g.y) / cell,
            )
        } else {
            (
                (if pull_x < 0 { t_col } else { pl.col }) as f32 + dx / cell,
                (if pull_y < 0 { t_row } else { pl.row }) as f32 + dy / cell,
            )
        };
        let (xc, yc) = (f("fcol", xc), f("frow", yc));
        let (wc, hc) = (f("fw", t_w as f32), f("fh", t_h as f32));
        let (grid_w, grid_h) = (wc * cell, hc * cell);
        let (area_w, area_h) = (g.cols as f32 * cell, g.rows as f32 * cell);
        let e = anim.follow(
            ("fexp", id),
            if expanded { 1.0 } else { 0.0 },
            240.0,
            m,
            Ease::Soft,
        );
        // Blown up it is big, in the middle of the grid, and rectangular even if round.
        let big_w = area_w.min((t_w as f32 * cell).max(area_w * 0.55).max(8.0 * cell));
        let big_h = area_h.min((t_h as f32 * cell).max(area_h * 0.8).max(6.0 * cell));
        let lerp = |a: f32, b: f32| a + (b - a) * e;
        let w = lerp(grid_w, big_w);
        let h = lerp(grid_h, big_h);
        let x = g.x + lerp(xc * cell, (area_w - big_w) / 2.0);
        let y = g.y + lerp(yc * cell, (area_h - big_h) / 2.0);
        let frame_x = lerp(xc * cell, (area_w - big_w) / 2.0);
        let frame_y = lerp(yc * cell, (area_h - big_h) / 2.0);

        // The card: inset by the gap; round widgets are the inscribed circle.
        let shows_round = round && !expanded;
        let (cw, ch) = ((w - 2.0 * gap).max(0.0), (h - 2.0 * gap).max(0.0));
        let diameter = cw.min(ch);
        let appear = anim.entrance((pl.col + pl.row) as f32 * 5.0, 180.0, m, Ease::Soft);
        let (card_w, card_h, left, mut top) = if shows_round {
            (
                diameter,
                diameter,
                (w - diameter) / 2.0,
                (h - diameter) / 2.0,
            )
        } else {
            (cw, ch, gap, gap)
        };
        top += (1.0 - appear) * 28.0;
        let in_hand = moving || sizing || carried;
        let bw = tk.look.style.border_width;
        let border = if sizing && !t_ok {
            tk.bad()
        } else if selected || entered {
            tk.accent()
        } else if editing {
            tk.accent().opacity(if in_hand { 0.9 } else { 0.35 })
        } else {
            tk.border()
        };
        // The border's colour glides to a new one (a widget picked, entered, let go).
        let border: gpui::Hsla = {
            let c = border.to_rgb();
            let ch = |k: &'static str, v: f32| anim.follow((k, id), v, 160.0, m, Ease::Linear);
            gpui::Rgba {
                r: ch("bc-r", c.r),
                g: ch("bc-g", c.g),
                b: ch("bc-b", c.b),
                a: ch("bc-a", c.a),
            }
            .into()
        };
        let bg = if expanded {
            tk.c(ColorRole::Card).with_alpha(1.0)
        } else {
            tk.c(ColorRole::Card).faded(tk.look.style.card_opacity)
        };
        let radius = if shows_round {
            card_w / 2.0
        } else {
            tk.look.style.radius
        };
        let ctx = Ctx::new(self, tk, tile, card_w, card_h);
        let title = pl.kind.info().title;

        // The edit controls fade in and out; the handles show on the widget in hand, as Plasma's do.
        let edit_fade = anim.follow(
            ("efade", id),
            if editing && !moving && !sizing {
                1.0
            } else {
                0.0
            },
            160.0,
            m,
            Ease::Out,
        );
        let handles_fade = anim.follow(
            ("hfade", id),
            if editing && !moving && (sizing || lifted || selected || hovered) {
                1.0
            } else {
                0.0
            },
            160.0,
            m,
            Ease::Out,
        );
        let handles_fade = if sizing { 1.0 } else { handles_fade };

        let mut frame = div()
            .id(("widget", id as usize))
            .role(Role::Group)
            .aria_label(title)
            // A widget without a cursor of its own is what the screen reader is on while it is
            // entered; one with a cursor has a part of it claim the focus instead (GPUI allows one
            // claim at a time: a debug build panics on two, nested or not).
            .aria_cursor(entered && widgets::describe(self).is_none())
            // A screen reader asks for the focus on a part of the widget (GPUI hands it up to
            // here): that enters the widget, as typing its letters does.
            .on_a11y_action(gpui::AccessibleAction::Focus, {
                let dash = cx.entity();
                move |_, _, cx| {
                    dash.update(cx, |d, cx| {
                        if d.visible && !d.editing {
                            d.enter(id);
                            cx.notify();
                        }
                    });
                }
            })
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(w))
            .h(px(h))
            .occlude();

        // Where a dragged widget will land (a resize shows on the widget itself).
        if moving && let Some(p) = pulled {
            let ok = t_ok;
            let color = if ok { tk.good() } else { tk.bad() };
            frame = frame.child(
                div()
                    .absolute()
                    .left(px(p.target.col as f32 * cell - frame_x + gap))
                    .top(px(p.target.row as f32 * cell - frame_y + gap))
                    .w(px((p.target.w as f32 * cell - 2.0 * gap).max(0.0)))
                    .h(px((p.target.h as f32 * cell - 2.0 * gap).max(0.0)))
                    .rounded(px(if round {
                        p.target.w as f32 * cell / 2.0
                    } else {
                        tk.look.style.radius
                    }))
                    .border_2()
                    .border_color(color)
                    .bg(kit::with_alpha(color, 0.12)),
            );
        }

        let mut card = div()
            .id(("card", id as usize))
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(px(card_w))
            .h(px(card_h))
            .rounded(px(radius))
            .bg(bg.hsla())
            .overflow_hidden()
            .opacity(appear * if in_hand { 0.85 } else { 1.0 })
            .debug_selector(move || format!("widget {title}"))
            .when(in_hand || expanded, |c| {
                c.shadow(vec![BoxShadow {
                    color: tk.shadow(),
                    offset: point(px(0.0), px(0.0)),
                    blur_radius: px(24.0),
                    spread_radius: px(0.0),
                    inset: false,
                }])
            })
            // Empty parts of the widget: a double click picks it up.
            .when(!editing && !carried, |c| {
                c.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |d, ev: &MouseDownEvent, _, cx| {
                        if ev.click_count == 2 {
                            d.grab_widget(id, at(ev.position));
                            cx.notify();
                        }
                    }),
                )
            })
            // The border takes no room, as in Slint (a CSS border would push the widget's content
            // in by its width): it is drawn under the content, which starts at the card's edge.
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .rounded(px(radius))
                    .border(px(if selected || entered { bw.max(2.0) } else { bw }))
                    .border_color(border),
            )
            .child(widgets::render(&ctx, cx));

        if op != NavOp::None {
            card = card.child(self.op_veil(op, shows_round, tk));
        }

        // In edit mode the whole card is a drag handle and the content gets no input.
        if editing {
            card = card.child(
                div()
                    .id(("mover", id as usize))
                    .debug_selector(move || format!("move {title}"))
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .occlude()
                    .when(!pinned, |m| m.cursor_move())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |d, ev: &MouseDownEvent, _, cx| {
                            let pos = at(ev.position);
                            if ev.click_count >= 2 {
                                d.grab_widget(id, pos);
                            } else {
                                d.press_start(id, PressKind::Move, pos);
                            }
                            cx.notify();
                        }),
                    ),
            );
        }
        frame = frame.child(card);

        // The resize handles: every edge and corner of the card can be grabbed, each moving just
        // its sides. They reach over the gap between the widgets, and don't overlap any
        // neighbour's.
        if editing && !pinned {
            let band = gap + 8.0;
            let corner = gap + 16.0 + if round { diameter * 0.146 } else { 0.0 };
            let (reach_x, reach_y) = (card_w / 2.0, card_h / 2.0);
            for (n, &(hx, hy)) in HANDLES.iter().enumerate() {
                let on_corner = hx != 0 && hy != 0;
                let hw = if hx == 0 {
                    (w - 2.0 * corner).max(0.0)
                } else if hy == 0 {
                    band
                } else {
                    corner
                };
                let hh = if hy == 0 {
                    (h - 2.0 * corner).max(0.0)
                } else if hx == 0 {
                    band
                } else {
                    corner
                };
                let hl = if hx < 0 {
                    0.0
                } else if hx > 0 {
                    w - hw
                } else {
                    corner
                };
                let ht = if hy < 0 {
                    0.0
                } else if hy > 0 {
                    h - hh
                } else {
                    corner
                };
                let active = press.is_some_and(|p| p.pull() == (hx, hy));
                // The dot: centered on the card's edge or corner, on the circle for a round widget.
                let (rx, ry) = if round {
                    let k = diameter * if on_corner { 0.354 } else { 0.5 };
                    (k, k)
                } else {
                    (reach_x, reach_y)
                };
                let dot_left = w / 2.0 + hx as f32 * rx - 7.0 - hl;
                let dot_top = h / 2.0 + hy as f32 * ry - 7.0 - ht;
                let group = format!("handle-{id}-{n}");
                let dot_bg = if active {
                    if blocked { tk.bad() } else { tk.accent() }
                } else {
                    tk.panel()
                };
                let hover_bg = tk.accent().opacity(0.6);
                let dot_border = if active && blocked {
                    tk.bad()
                } else {
                    tk.accent()
                };
                let handle: Stateful<Div> = div()
                    .id(("handle", (id as usize) * 8 + n))
                    .group(group.clone())
                    .absolute()
                    .left(px(hl))
                    .top(px(ht))
                    .w(px(hw))
                    .h(px(hh))
                    .occlude()
                    .map(|h| match (hx, hy) {
                        (0, _) => h.cursor_ns_resize(),
                        (_, 0) => h.cursor_ew_resize(),
                        _ if hx == hy => h.cursor_nwse_resize(),
                        _ => h.cursor_nesw_resize(),
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |d, ev: &MouseDownEvent, _, cx| {
                            d.press_start(id, PressKind::Handle { x: hx, y: hy }, at(ev.position));
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .when(handles_fade > 0.01, |h| {
                        h.child(
                            div()
                                .absolute()
                                .left(px(dot_left))
                                .top(px(dot_top))
                                .size(px(14.0))
                                .rounded_full()
                                .opacity(handles_fade)
                                .bg(dot_bg)
                                .when(!active, |d| d.group_hover(group, move |s| s.bg(hover_bg)))
                                .border_1()
                                .border_color(dot_border)
                                .shadow(vec![BoxShadow {
                                    color: tk.shadow(),
                                    offset: point(px(0.0), px(0.0)),
                                    blur_radius: px(4.0),
                                    spread_radius: px(0.0),
                                    inset: false,
                                }]),
                        )
                    });
                frame = frame.child(handle);
            }
        }

        // The toolbar: settings, shape toggle, pin and remove, top right of the card, clear of
        // the handle there; on a round widget at the top of the circle, where the corners are free.
        if edit_fade > 0.01 {
            let info = pl.kind.info();
            let (has_settings, can_round) = (!info.settings.is_empty(), info.can_be_round);
            let buttons = usize::from(can_round) + usize::from(has_settings) + 2;
            let tb_w = 2.0 + 32.0 * buttons as f32;
            let (tb_x, tb_y) = if round {
                ((w - tb_w) / 2.0, (h - diameter) / 2.0 + 12.0)
            } else {
                (w - gap - tb_w - 12.0, gap + 12.0)
            };
            let dash = cx.entity();
            let mut bar = div()
                .absolute()
                .left(px(tb_x))
                .top(px(tb_y))
                .w(px(tb_w))
                .h(px(34.0))
                .p(px(2.0))
                .gap(px(2.0))
                .flex()
                .flex_row()
                .rounded(px((17.0_f32).min(tk.radius_small() + 2.0)))
                .bg(tk.panel())
                .border_1()
                .border_color(tk.border())
                .opacity(edit_fade)
                .occlude();
            if has_settings {
                let dash = dash.clone();
                bar = bar.child(icon_btn(
                    eid("settings-for", id as usize),
                    "settings",
                    format!("Settings for {title}"),
                    30.0,
                    if selected { tk.accent() } else { tk.text() },
                    Look::default(),
                    tk,
                    move |_, _, cx| {
                        dash.update(cx, |d, cx| {
                            d.configure_click(id, cx);
                            cx.notify();
                        })
                    },
                ));
            }
            if can_round {
                let dash = dash.clone();
                bar = bar.child(icon_btn(
                    eid("shape", id as usize),
                    if round { "square" } else { "circle" },
                    format!("Make {title} {}", if round { "square" } else { "round" }),
                    30.0,
                    tk.text(),
                    Look {
                        enabled: editing && !pinned,
                        ..Look::default()
                    },
                    tk,
                    move |_, _, cx| {
                        dash.update(cx, |d, cx| {
                            d.toggle_shape(id);
                            cx.notify();
                        })
                    },
                ));
            }
            {
                let dash = dash.clone();
                bar = bar.child(icon_btn(
                    eid("pin", id as usize),
                    "lock",
                    format!("{} {title}", if pinned { "Unpin" } else { "Pin" }),
                    30.0,
                    if pinned { tk.accent() } else { tk.dim() },
                    Look::default(),
                    tk,
                    move |_, _, cx| {
                        dash.update(cx, |d, cx| {
                            d.toggle_pin(id);
                            cx.notify();
                        })
                    },
                ));
            }
            {
                let dash = dash.clone();
                bar = bar.child(icon_btn(
                    eid("remove", id as usize),
                    "close",
                    format!("Remove {title}"),
                    30.0,
                    tk.bad(),
                    Look::default(),
                    tk,
                    move |_, _, cx| {
                        dash.update(cx, |d, cx| {
                            d.remove_widget(id);
                            cx.notify();
                        })
                    },
                ));
            }
            frame = frame.child(bar);

            // A pinned widget wears a lock in edit mode, bottom right (bottom of the circle if
            // round).
            if pinned {
                let (lx, ly) = if round {
                    ((w - 24.0) / 2.0, (h + diameter) / 2.0 - 24.0 - 12.0)
                } else {
                    (w - gap - 24.0 - 12.0, h - gap - 24.0 - 12.0)
                };
                frame = frame.child(
                    div()
                        .absolute()
                        .left(px(lx))
                        .top(px(ly))
                        .size(px(24.0))
                        .rounded_full()
                        .bg(tk.accent())
                        .opacity(edit_fade)
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(kit::icon("lock", 14.0, tk.on_accent())),
                );
            }
        }
        frame.into_any_element()
    }
}

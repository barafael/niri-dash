//! Drawing the dashboard: the backdrop, the top bar, the grid of widget frames, the hint labels
//! and the toast. What goes in a frame is `widgets`'; the small pieces they are made of are
//! `kit`'s.

use crate::dash::{BAR, Dash, DashEvent, Geometry, NavMode, Tile};
use crate::kit::{self, Tk};
use crate::widgets::{self, Ctx};
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, KeyDownEvent, MouseButton, Pixels, Render, Window, div, px,
};

impl Render for Dash {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let vp = window.viewport_size();
        let (width, height) = (f32::from(vp.width), f32::from(vp.height));
        self.set_size(width, height);
        if self.advance_look() {
            window.request_animation_frame();
        }
        let tk = Tk::new(&self.look);
        let root = div()
            .id("niri-dash")
            .key_context("Dash")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .overflow_hidden()
            .text_size(px(tk.look.style.font_body))
            .text_color(tk.text())
            .on_key_down(cx.listener(|dash, ev: &KeyDownEvent, _, cx| {
                let key = crate::keys::Key::from_keystroke(&ev.keystroke);
                if dash.key(&key, cx) {
                    cx.stop_propagation();
                }
            }));
        let root = if tk.look.style.font_family.is_empty() {
            root
        } else {
            root.font_family(tk.look.style.font_family.clone())
        };
        if !self.visible {
            return root;
        }
        if let Some(kind) = self.solo {
            let _ = kind;
            return root
                .bg(tk.card())
                .children(self.tiles.first().map(|t| {
                    let ctx = Ctx::new(self, &tk, t, width, height);
                    widgets::render(&ctx, cx)
                }));
        }
        let g = self.geometry(width, height);
        let backdrop = tk
            .c(crate::theme::ColorRole::Backdrop)
            .faded(tk.look.style.backdrop_opacity);
        let expanded = self.nav.expanded_id;
        root.bg(backdrop.hsla())
            .child(self.backdrop(&tk, cx))
            .child(self.top_bar(&tk, cx))
            .children(
                self.tiles
                    .iter()
                    .filter(|t| t.id() != expanded)
                    .map(|t| self.frame(t, &g, &tk, false, cx)),
            )
            .children(
                self.tiles
                    .iter()
                    .filter(|t| t.id() == expanded)
                    .map(|t| self.frame(t, &g, &tk, true, cx)),
            )
            .children(self.hints(&g, &tk))
            .children(self.toast_view(&tk))
    }
}

impl Dash {
    /// Clicking empty space closes; right click starts edit mode.
    fn backdrop(&self, _tk: &Tk, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("backdrop")
            .absolute()
            .size_full()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|dash, _, _, cx| {
                    if dash.nav.mode == NavMode::Hints {
                        dash.nav_off();
                        cx.notify();
                    } else if !dash.editing {
                        cx.emit(DashEvent::Hide);
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|dash, _, _, cx| {
                    if !dash.editing {
                        dash.set_editing(true, cx);
                    }
                }),
            )
    }

    fn top_bar(&self, tk: &Tk, cx: &mut Context<Self>) -> impl IntoElement {
        let g = self.geometry(self.size.0, self.size.1);
        let right = (self.size.0 - g.x - g.cols as f32 * g.cell).max(0.0) + tk.gap();
        div()
            .absolute()
            .top_0()
            .right(px(right))
            .h(px(BAR))
            .pt(px(6.0))
            .flex()
            .flex_row()
            .gap(px(8.0))
            .when(!self.editing, |bar| {
                bar.child(kit::icon_button(
                    "edit-dashboard",
                    "edit",
                    "Edit dashboard",
                    34.0,
                    tk.dim(),
                    tk,
                    cx.listener(|dash, _, _, cx| dash.set_editing(true, cx)),
                ))
                .child(kit::icon_button(
                    "close-dashboard",
                    "close",
                    "Close dashboard",
                    34.0,
                    tk.dim(),
                    tk,
                    cx.listener(|_, _, _, cx| cx.emit(DashEvent::Hide)),
                ))
            })
            .when(self.editing && self.hidden_widgets > 0, |bar| {
                bar.child(
                    div()
                        .text_color(tk.bad())
                        .child(format!("{} widget(s) don't fit", self.hidden_widgets)),
                )
            })
    }

    /// One placed widget: its card, with the widget inside.
    fn frame(
        &self,
        tile: &Tile,
        g: &Geometry,
        tk: &Tk,
        expanded: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let r = tile.rect();
        let gap = tk.gap();
        let (area_w, area_h) = (g.width, g.height);
        // Where its cell block is; blown up, in the middle of the screen.
        let (x, y, w, h) = if expanded {
            let big_w = area_w
                .min((r.w as f32 * g.cell).max(area_w * 0.55).max(8.0 * g.cell));
            let big_h = area_h
                .min((r.h as f32 * g.cell).max(area_h * 0.8).max(6.0 * g.cell));
            ((area_w - big_w) / 2.0, (area_h - big_h) / 2.0, big_w, big_h)
        } else {
            (
                g.x + r.col as f32 * g.cell,
                g.y + r.row as f32 * g.cell,
                r.w as f32 * g.cell,
                r.h as f32 * g.cell,
            )
        };
        let shows_round = tile.round() && !expanded;
        let (cw, ch) = (w - 2.0 * gap, h - 2.0 * gap);
        let diameter = cw.min(ch);
        let (card_w, card_h, left, top) = if shows_round {
            (diameter, diameter, (w - diameter) / 2.0, (h - diameter) / 2.0)
        } else {
            (cw, ch, gap, gap)
        };
        let entered = self.nav.mode == NavMode::Widget && self.nav.active_id == tile.id();
        let bw = tk.look.style.border_width;
        let border = if entered {
            tk.accent()
        } else if self.editing {
            tk.accent().opacity(0.35)
        } else {
            tk.border()
        };
        let bg = if expanded {
            tk.c(crate::theme::ColorRole::Card).with_alpha(1.0)
        } else {
            tk.c(crate::theme::ColorRole::Card)
                .faded(tk.look.style.card_opacity)
        };
        let ctx = Ctx::new(self, tk, tile, card_w, card_h);
        let label = tile.placed.kind.info().title;
        div()
            .id(("widget", tile.id() as usize))
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(w))
            .h(px(h))
            .occlude()
            .child(
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(card_w))
                    .h(px(card_h))
                    .rounded(px(if shows_round {
                        card_w / 2.0
                    } else {
                        tk.look.style.radius
                    }))
                    .bg(bg.hsla())
                    .border(px(if entered { bw.max(2.0) } else { bw }))
                    .border_color(border)
                    .overflow_hidden()
                    .debug_selector(move || format!("widget {label}"))
                    .child(widgets::render(&ctx, cx)),
            )
            .into_any_element()
    }

    /// The hint labels: on every widget while the hints are up.
    fn hints(&self, g: &Geometry, tk: &Tk) -> Vec<Div> {
        if self.nav.mode != NavMode::Hints {
            return Vec::new();
        }
        let typed = self.nav.typed.to_lowercase();
        self.tiles
            .iter()
            .map(|t| {
                let r = t.rect();
                let cx_ = g.x + (r.col as f32 + r.w as f32 / 2.0) * g.cell;
                let cy_ = g.y + (r.row as f32 + r.h as f32 / 2.0) * g.cell;
                let matches = t.hint.starts_with(&typed);
                let done = if matches { typed.chars().count() } else { 0 };
                let (head, tail): (String, String) = (
                    t.hint.chars().take(done).collect(),
                    t.hint.chars().skip(done).collect(),
                );
                let width = 24.0 + 13.0 * t.hint.chars().count() as f32;
                let label = t.hint.clone();
                div()
                    .absolute()
                    .left(px(cx_ - width / 2.0))
                    .top(px(cy_ - 20.0))
                    .w(px(width))
                    .h(px(40.0))
                    .rounded(px(tk.look.style.radius_small + 4.0))
                    .bg(tk.accent())
                    .border(px(2.0))
                    .border_color(tk.on_accent().opacity(0.3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(tk.look.style.font_title + 3.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(tk.on_accent())
                    .opacity(if matches { 1.0 } else { 0.15 })
                    .debug_selector(move || format!("hint {label}"))
                    .child(div().opacity(0.4).child(head))
                    .child(tail)
            })
            .collect()
    }

    fn toast_view(&self, tk: &Tk) -> Option<Div> {
        let toast = self.toast.as_ref()?;
        Some(
            div()
                .absolute()
                .bottom(px(32.0))
                .w_full()
                .flex()
                .justify_center()
                .child(
                    div()
                        .max_w(px(self.size.0 * 0.6))
                        .px(px(16.0))
                        .py(px(10.0))
                        .rounded(px(tk.look.style.radius_small))
                        .bg(tk.panel())
                        .border(px(1.0))
                        .border_color(tk.border())
                        .child(toast.text.clone()),
                ),
        )
    }
}

/// Pixels as a plain number.
pub fn px_f(p: Pixels) -> f32 {
    f32::from(p)
}

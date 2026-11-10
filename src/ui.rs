//! Drawing the dashboard: the backdrop, the top bar, the grid of widget frames, the hint labels
//! and the toast. What goes in a frame is `widgets`'; the small pieces they are made of are
//! `kit`'s. Edit mode's parts (the frames, the sidebar, the settings card, the top bar) are drawn
//! by `edit::view`.

use crate::dash::{Dash, DashEvent, Geometry, NavMode};
use crate::edit::anim::Ease;
use crate::kit::Tk;
use crate::widgets::{self, Ctx};
use gpui::prelude::*;
use gpui::{Context, Div, KeyDownEvent, MouseButton, Pixels, Render, Window, div, px};

impl Render for Dash {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let vp = window.viewport_size();
        let (width, height) = (f32::from(vp.width), f32::from(vp.height));
        // The drawer's slide, as of this frame, before anything is laid out by it.
        self.edit.sample(self.editing, self.look.style.motion);
        self.set_size(width, height);
        // The size the widgets are drawn at can have changed since the last key (the drawer
        // sliding, a look blending): their cursors stay on stops that exist at it.
        widgets::resized(self);
        if self.advance_look() {
            window.request_animation_frame();
        }
        self.prepare_fields(window, cx);
        self.sync_focus(window, cx);
        self.note_field_focus(window, cx);
        let tk = Tk::new(&self.look);
        crate::look::sync_component_theme(&tk, cx);
        let root = div()
            .id("niri-dash")
            // The node that holds the keyboard for a screen reader; the element under the cursor
            // is its active descendant (`kit::AriaCursor`).
            .role(gpui::Role::Group)
            .key_context("Dash")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .overflow_hidden()
            .text_size(px(tk.look.style.font_body))
            .text_color(tk.text())
            .on_key_down(cx.listener(|dash, ev: &KeyDownEvent, window, cx| {
                dash.note_field_focus(window, cx);
                let key = crate::keys::Key::from_keystroke(&ev.keystroke);
                // A text field has the keyboard (the key came up from it): it keeps every key but
                // Escape.
                dash.typing = window
                    .focused(cx)
                    .is_some_and(|focused| focused != dash.focus);
                if dash.key(&key, cx) {
                    cx.stop_propagation();
                }
                dash.sync_focus(window, cx);
            }));
        // The style's font, or the one the component theme found for the system (GPUI's own
        // ".SystemUIFont" names no installed family on Linux).
        let root = if tk.look.style.font_family.is_empty() {
            root.font_family(gpui_kit::component::ActiveTheme::theme(&**cx).font_family.clone())
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
        let reveal = self.edit.anim.entrance(0.0, 140.0, tk.motion(), Ease::Out);
        let backdrop = tk
            .c(crate::theme::ColorRole::Backdrop)
            .faded(tk.look.style.backdrop_opacity * reveal);
        // The frames in three layers: the rest, the one in hand, the one blown up.
        let mut layer = |level: u8| {
            self.tiles
                .iter()
                .filter(|t| self.frame_level(t.id()) == level)
                .map(|t| self.frame(t, &g, &tk, cx))
                .collect::<Vec<_>>()
        };
        let (below, hand, above) = (layer(0), layer(1), layer(2));
        let root = root
            .bg(backdrop.hsla())
            .child(self.backdrop(&tk, cx))
            .child(self.top_bar(window, &tk, cx))
            .children(self.edit_grid(&g, &tk))
            .children(self.landing_ghost(&g, &tk))
            .children(below)
            .children(self.adjust_catcher(&g, cx))
            .children(hand)
            .children(self.expand_shade(&g, &tk, cx))
            .children(above)
            .children(self.hints(&g, &tk))
            .children(self.edit_buttons(&tk, cx))
            .children(self.drawer(&tk, cx))
            .children(self.card_layer(&tk, cx))
            .children(self.add_ghost(&g, &tk))
            .children(widgets::overlays(self, &tk, cx))
            .children(self.toast_view(&tk))
            .child(self.pointer_layer(cx))
            .children(self.press_cursor());
        // Something is still moving: draw again.
        if self.edit.anim.take_busy() {
            window.request_animation_frame();
        }
        root
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
                    // In edit mode (or with a widget opened for adjusting) clicks never close
                    // the dashboard; Done or Esc does.
                    if dash.nav.mode == NavMode::Hints {
                        dash.nav_off();
                        cx.notify();
                    } else if dash.edit.adjust_id != 0 {
                        dash.edit.adjust_id = 0;
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
                        dash.edit.adjust_id = 0;
                        dash.set_editing(true, cx);
                    }
                }),
            )
    }

    /// The hint labels: on every widget while the hints are up, the others fading back.
    fn hints(&self, g: &Geometry, tk: &Tk) -> Vec<Div> {
        let pop = self.edit.anim.follow(
            ("hints", 0),
            if self.nav.mode == NavMode::Hints { 1.0 } else { 0.0 },
            120.0,
            tk.motion(),
            Ease::Out,
        );
        if self.nav.mode != NavMode::Hints {
            return Vec::new();
        }
        let typed = self.nav.typed.to_lowercase();
        let shade = div()
            .absolute()
            .left(px(g.x))
            .top(px(g.y))
            .w(px(g.cols as f32 * g.cell))
            .h(px(g.rows as f32 * g.cell))
            .bg(tk.c(crate::theme::ColorRole::Backdrop).with_alpha(0.35).hsla());
        std::iter::once(shade)
            .chain(self.tiles.iter().map(|t| {
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
                    .top(px(cy_ - 20.0 + (1.0 - pop) * 6.0))
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
                    .opacity(pop * if matches { 1.0 } else { 0.15 })
                    .debug_selector(move || format!("hint {label}"))
                    .child(div().opacity(0.4).child(head))
                    .child(tail)
            }))
            .collect()
    }

    fn toast_view(&self, tk: &Tk) -> Option<Div> {
        let shown = self.toast.is_some();
        if let Some(t) = &self.toast {
            *self.edit.toast_text.borrow_mut() = t.text.clone();
        }
        let fade = self.edit.anim.follow(
            ("toast", 0),
            if shown { 1.0 } else { 0.0 },
            160.0,
            tk.motion(),
            Ease::Linear,
        );
        if fade <= 0.01 {
            return None;
        }
        let text = self.edit.toast_text.borrow().clone();
        Some(
            div()
                .absolute()
                .top(px(self.size.1 - 70.0 + (1.0 - fade) * 16.0))
                .w_full()
                .flex()
                .justify_center()
                .opacity(fade)
                .child(
                    div()
                        .max_w(px(self.size.0 * 0.6))
                        .h(px(38.0))
                        .px(px(16.0))
                        .flex()
                        .items_center()
                        .rounded(px(19.0_f32.min(tk.look.style.radius_small + 4.0)))
                        .bg(tk.panel())
                        .border(px(1.0))
                        .border_color(tk.border())
                        // An alert a screen reader reads out when it appears.
                        .id("toast")
                        .role(gpui::Role::Alert)
                        .aria_label(text.clone())
                        .debug_selector(|| "toast".into())
                        .child(text),
                ),
        )
    }
}

/// Pixels as a plain number.
pub fn px_f(p: Pixels) -> f32 {
    f32::from(p)
}

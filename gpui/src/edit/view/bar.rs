//! The top bar (what doesn't fit, and the buttons that open edit mode and close the dashboard) and
//! the quick tips along the bottom: the keys that work just now, in every mode.

use super::parts::{Look, icon_btn};
use crate::dash::{BAR, Dash, DashEvent, MARGIN};
use crate::edit::KeyHint;
use crate::edit::anim::Ease;
use crate::kit::Tk;
use gpui::prelude::*;
use gpui::{AnyElement, Context, FontWeight, SharedString, TextRun, Window, div, px};

/// How wide `text` is at `size`, in pixels.
pub(crate) fn text_width(
    window: &Window,
    text: &str,
    size: f32,
    weight: Option<FontWeight>,
) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let mut font = window.text_style().font();
    if let Some(weight) = weight {
        font.weight = weight;
    }
    let run = TextRun {
        len: text.len(),
        font,
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window
        .text_system()
        .shape_line(SharedString::from(text.to_string()), px(size), &[run], None)
        .width
        .into()
}

/// One shortcut as the quick tips show it: the keys on a cap, then what they do.
fn key_chip(hint: &KeyHint, cap_w: f32, tk: &Tk) -> AnyElement {
    div()
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .when(cap_w > 0.0, |d| {
            d.child(
                div()
                    .w(px(cap_w))
                    .h(px(22.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(5.0))
                    .border_1()
                    .border_color(tk.border())
                    .bg(tk.subtle())
                    .text_size(px(tk.font_small()))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(tk.text())
                    .child(hint.key.clone()),
            )
        })
        .child(
            div()
                .text_size(px(tk.font_body()))
                .text_color(if hint.key.is_empty() {
                    tk.accent()
                } else {
                    tk.dim()
                })
                .child(hint.does.clone()),
        )
        .into_any_element()
}

impl Dash {
    pub(crate) fn top_bar(&self, tk: &Tk, cx: &mut Context<Self>) -> AnyElement {
        let g = self.geometry(self.size.0, self.size.1);
        let gap = tk.gap();
        let reveal = self.edit.anim.entrance(0.0, 140.0, tk.motion(), Ease::Out);
        let (left, right) = (g.x + gap, self.size.0 - g.avail_w + g.x + gap);
        let warn = (self.editing && self.hidden_widgets > 0).then(|| {
            format!(
                "{} widget(s) don't fit {}",
                self.hidden_widgets,
                if self.layout.grid.is_some() {
                    "the grid"
                } else {
                    "this screen"
                }
            )
        });
        div()
            .absolute()
            .top(px(-12.0 * (1.0 - reveal)))
            .left(px(0.0))
            .w_full()
            .h(px(BAR))
            .pl(px(left))
            .pr(px(right))
            .pt(px(6.0))
            .gap(px(8.0))
            .flex()
            .flex_row()
            .opacity(reveal)
            .child(div().flex_1().min_w_0().h_full())
            .when_some(warn, |bar, text| {
                bar.child(
                    div()
                        .flex_none()
                        .h_full()
                        .flex()
                        .items_center()
                        .text_size(px(tk.font_body()))
                        .text_color(tk.bad())
                        .child(text),
                )
            })
            .when(!self.editing, |bar| {
                bar.child(icon_btn(
                    "edit-dashboard",
                    "edit",
                    "Edit dashboard",
                    34.0,
                    tk.dim(),
                    Look::default(),
                    tk,
                    cx.listener(|dash, _, _, cx| dash.set_editing(true, cx)),
                ))
                .child(icon_btn(
                    "close-dashboard",
                    "close",
                    "Close dashboard",
                    34.0,
                    tk.dim(),
                    Look::default(),
                    tk,
                    cx.listener(|_, _, _, cx| cx.emit(DashEvent::Hide)),
                ))
            })
            .into_any_element()
    }

    /// Quick tips along the bottom, in the margin under the grid: what the keys do just now. One
    /// that doesn't fit whole is left out.
    pub(crate) fn tips_bar(&self, window: &Window, tk: &Tk) -> Option<AnyElement> {
        let hints = self.tips();
        if hints.is_empty() {
            return None;
        }
        let g = self.geometry(self.size.0, self.size.1);
        let reveal = self.edit.anim.entrance(0.0, 140.0, tk.motion(), Ease::Out);
        let left = g.x + tk.gap();
        let strip_w = g.avail_w - 2.0 * left;
        let mut x = 0.0;
        let chips: Vec<AnyElement> = hints
            .iter()
            .filter_map(|h| {
                let cap = if h.key.is_empty() {
                    0.0
                } else {
                    text_width(window, &h.key, tk.font_small(), Some(FontWeight::SEMIBOLD)) + 12.0
                };
                let w = cap
                    + if cap > 0.0 { 6.0 } else { 0.0 }
                    + text_width(window, &h.does, tk.font_body(), None);
                let fits = x + w <= strip_w;
                x += w + 16.0;
                fits.then(|| key_chip(h, cap, tk))
            })
            .collect();
        Some(
            div()
                .absolute()
                .left(px(left))
                .top(px(self.size.1 - MARGIN))
                .w(px(strip_w.max(0.0)))
                .h(px(MARGIN))
                .opacity(reveal)
                .overflow_hidden()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(16.0))
                .debug_selector(|| "quick tips".into())
                .children(chips)
                .into_any_element(),
        )
    }
}

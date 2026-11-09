//! A widget's settings, in a card in the middle of the screen (over the space the sidebar leaves),
//! with everything else faded back. A click outside, or Escape, closes it. It steps aside while
//! the keys are on the widgets (sizing, moving) or the hints are up, so the widgets can be seen.

use super::parts::{Look, RowOwner, RowSpec, eid, icon_btn, setting_row};
use crate::dash::{Dash, NavMode};
use crate::edit::RowKind;
use crate::edit::anim::Ease;
use crate::kit::Tk;
use gpui::prelude::*;
use gpui::{AnyElement, BoxShadow, Context, FontWeight, Role, div, point, px};

impl Dash {
    /// The card is up: its widget has settings to show and the keys aren't on the widgets.
    pub fn card_up(&self) -> bool {
        self.cfg_open()
            && (!self.edit.canvas_keys || self.edit.side_keys)
            && self.nav.mode != NavMode::Hints
    }

    pub(crate) fn card_layer(&self, tk: &Tk, cx: &mut Context<Self>) -> Option<AnyElement> {
        let up = self.card_up();
        let shift = self
            .edit
            .anim
            .follow(("card", 0), if up { 1.0 } else { 0.0 }, 180.0, tk.motion(), Ease::Soft);
        if shift <= 0.001 {
            return None;
        }
        let (width, height) = self.size;
        let rows = self.setting_rows();
        let id = self.edit.settings_id;
        let title = self.settings_title();
        let (pad, head_h, foot_h) = (18.0, 40.0, 30.0);
        let rows_h: f32 = rows
            .iter()
            .map(|r| if r.kind == RowKind::Text { 76.0 } else { 40.0 })
            .sum::<f32>()
            + 6.0 * rows.len().saturating_sub(1) as f32;
        let body_h = rows_h.min((height - 220.0).max(120.0));
        let card_w = 480.0_f32.min(width - 48.0);
        let card_h = pad * 2.0 + head_h + 8.0 + body_h + 8.0 + foot_h;
        let g = self.geometry(width, height);
        let x = (g.avail_w - card_w) / 2.0;
        let y = (height - card_h) / 2.0 + (1.0 - shift) * 18.0;
        if self.cfg_open() {
            let handle = &self.edit.scroll.card;
            if self.edit.side_keys && self.edit.revealed.get() != Some((None, self.edit.side_cursor, true, id)) {
                handle.scroll_to_item(self.edit.side_cursor);
            }
            self.edit
                .revealed
                .set(Some((None, self.edit.side_cursor, self.edit.side_keys, id)));
        }
        let rows: Vec<AnyElement> = rows.into_iter().enumerate().map(|(i, row)| {
            let field = self.edit.field.state(id, row.index).cloned();
            setting_row(
                RowSpec {
                    owner: RowOwner::Card,
                    pos: i,
                    row,
                    kfocus: self.cfg_on(i),
                    shake: 0.0,
                    live: up,
                    field,
                },
                tk,
                cx,
            )
        }).collect();
        let card = div()
            .id("settings-card")
            .role(Role::TabPanel)
            .aria_label(format!("{title} settings"))
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(card_w))
            .h(px(card_h))
            .p(px(pad))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .rounded(px(tk.radius()))
            .bg(tk.panel())
            .border_1()
            .border_color(tk.border())
            .shadow(vec![BoxShadow {
                color: tk.shadow(),
                offset: point(px(0.0), px(0.0)),
                blur_radius: px(30.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .occlude()
            .child(
                div()
                    .flex_none()
                    .h(px(head_h))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(tk.font_title()))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(tk.text())
                            .child(title),
                    )
                    .child(icon_btn(
                        eid("close-settings", 0),
                        "close",
                        "Close settings",
                        32.0,
                        tk.dim(),
                        Look {
                            enabled: up,
                            ..Look::default()
                        },
                        tk,
                        cx.listener(|d, _, _, cx| {
                            d.set_settings_id(0);
                            cx.notify();
                        }),
                    )),
            )
            .child(
                div()
                    .id("settings-list")
                    .flex_none()
                    .h(px(body_h))
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .overflow_y_scroll()
                    .track_scroll(&self.edit.scroll.card)
                    .children(rows),
            )
            .child(
                div()
                    .flex_none()
                    .h(px(foot_h))
                    .flex()
                    .items_center()
                    .truncate()
                    .text_size(px(tk.font_small()))
                    .text_color(tk.dim())
                    .child("j k h l Enter · Backspace resets · Ctrl+h j k l next widget · Esc closes"),
            );
        Some(
            div()
                .absolute()
                .size_full()
                .opacity(shift)
                .child(
                    div()
                        .id("settings-scrim")
                        .role(Role::Button)
                        .aria_label("Close settings")
                        .debug_selector(|| "settings scrim".into())
                        .absolute()
                        .size_full()
                        .bg(tk.c(crate::theme::ColorRole::Backdrop).with_alpha(0.6).hsla())
                        .occlude()
                        .on_click(cx.listener(|d, _, _, cx| {
                            d.set_settings_id(0);
                            cx.notify();
                        })),
                )
                .child(card)
                .into_any_element(),
        )
    }
}

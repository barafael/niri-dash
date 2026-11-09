//! Claude usage: the current 5-hour window, the models in it, tokens per hour, today and the
//! week. Round: the window's progress as a ring.

use super::charts::{Gauge, Graph, bar, percent};
use super::parts::{head, muted, page, stat};
use crate::dash::Dash;
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, Context, div, px};

pub fn render(ctx: &Ctx, _cx: &mut Context<Dash>) -> AnyElement {
    let m = &ctx.d.widgets.monitors;
    let u = &m.usage;
    let tk = ctx.tk;
    if ctx.round {
        let mut g = Gauge::new("5-hour window", u.window_progress, tk.accent());
        if u.window_active {
            g.big = u.window_count.clone();
            g.caption = format!("tokens · {}", u.window_resets);
        } else {
            g.big = "idle".into();
            g.caption = u.plan.clone();
        }
        return g.element(ctx.w, tk, Vec::new());
    }
    let plan = {
        let text = u.plan.clone();
        let shown = text.clone();
        div()
            .flex_none()
            .h(px(20.0))
            .px(px(7.0))
            .flex()
            .items_center()
            .rounded(px(10.0f32.min(tk.radius_small())))
            .bg(tk.accent_soft())
            .text_size(px(tk.font_small()))
            .text_color(tk.accent())
            .whitespace_nowrap()
            .debug_selector(move || shown)
            .child(text)
    };
    let sessions = if m.sessions == 0 {
        "no sessions".to_string()
    } else {
        format!(
            "{} working · {} waiting",
            m.busy,
            m.sessions.saturating_sub(m.busy)
        )
    };
    let window = if u.window_active {
        format!("5-hour window · {}", u.window_tokens)
    } else {
        "5-hour window".to_string()
    };
    let left = if u.window_active {
        format!("{} · {}", u.window_left, u.window_resets)
    } else {
        u.window_left.clone()
    };
    page(tk)
        .child(head(
            Some(("sparkle", tk.accent())),
            "Claude",
            true,
            vec![
                plan.into_any_element(),
                muted(sessions, tk).flex_none().into_any_element(),
            ],
            tk,
        ))
        // The current 5-hour window: how far along, how much used.
        .child(
            div()
                .w_full()
                .h(px(20.0))
                .flex_none()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(tk.font_body()))
                        .text_color(tk.text())
                        .debug_selector({
                            let shown = window.clone();
                            move || shown
                        })
                        .child(window),
                )
                .child(muted(left, tk).flex_none()),
        )
        .child(bar(
            "claude-window",
            "5-hour window",
            u.window_progress,
            tk.accent(),
            true,
            tk,
        ))
        .when(u.window_active, |p| {
            p.child(stat(
                format!("{} messages · {}", u.window_messages, u.burn),
                u.window_cached.clone(),
                tk.dim(),
                tk,
            ))
        })
        .when(u.window_active && ctx.h > 200.0, |p| {
            p.child(
                div()
                    .w_full()
                    .h(px(18.0))
                    .flex_none()
                    .flex()
                    .flex_row()
                    .gap(px(4.0))
                    .children(u.models.iter().map(|(name, share)| {
                        let text = format!("{name} {}", percent(*share));
                        let shown = text.clone();
                        div()
                            .flex_basis(px(0.0))
                            .flex_grow(share.max(0.15))
                            .min_w_0()
                            .h_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(tk.radius_small().min(4.0)))
                            .bg(tk.card_raised())
                            .text_size(px(tk.font_small()))
                            .text_color(tk.dim())
                            .truncate()
                            .debug_selector(move || shown)
                            .child(text)
                    })),
            )
        })
        .when(ctx.h > 160.0, |p| {
            p.child(
                Graph::new("Claude tokens per hour", &u.hourly, 24, tk.accent())
                    .element("claude-hourly", tk)
                    .flex_1()
                    .min_h(px(24.0)),
            )
        })
        .child(stat(u.today.clone(), u.week.clone(), tk.text(), tk))
        .into_any_element()
}

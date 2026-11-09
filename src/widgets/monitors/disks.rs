//! Disks: what each is filled to, and the read and write activity. Round: the root disk's ring.

use super::charts::{Gauge, Graph, bar, percent};
use super::parts::{head, muted, page, stat};
use crate::dash::Dash;
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, Context, SharedString, div, px};

pub fn render(ctx: &Ctx, _cx: &mut Context<Dash>) -> AnyElement {
    let s = &ctx.d.widgets.monitors.perf;
    let tk = ctx.tk;
    if ctx.round {
        let mut g = Gauge::new("Root disk used", s.root_used, tk.accent());
        g.big = percent(s.root_used);
        g.caption = "/ used".into();
        return g.element(ctx.w, tk, Vec::new());
    }
    let hide_system = ctx.opts.hide_system;
    let tall = ctx.h > 180.0;
    page(tk)
        .child(head(
            None,
            "Disks",
            false,
            vec![
                muted(
                    format!("{}  ·  {}", s.disk_read_text, s.disk_write_text),
                    tk,
                )
                .flex_1()
                .text_right()
                .into_any_element(),
            ],
            tk,
        ))
        .children(
            s.disks
                .iter()
                .filter(|d| !(hide_system && d.system))
                .enumerate()
                .map(|(i, d)| {
                    div()
                        .w_full()
                        .flex_none()
                        .flex()
                        .flex_col()
                        .gap(px(3.0))
                        .child(stat(d.mount.clone(), d.text.clone(), tk.text(), tk))
                        .child(bar(
                            ("disk", i),
                            SharedString::from(format!("{} used", d.mount)),
                            d.used,
                            tk.accent(),
                            true,
                            tk,
                        ))
                }),
        )
        .when(tall, |p| {
            p.child(
                Graph::new("Disk activity over time", &s.disk_history, 90, tk.warn())
                    .element("disk-graph", tk)
                    .flex_1()
                    .min_h(px(24.0)),
            )
        })
        .when(!tall, |p| p.child(div().flex_1()))
        .into_any_element()
}

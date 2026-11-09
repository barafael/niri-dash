//! Network: download and upload, their history on one scale, the interface. Round: two rings
//! against the recent peak.

use super::charts::{Gauge, Graph};
use super::parts::{label, muted, page};
use crate::dash::Dash;
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Hsla, div, px};

pub fn render(ctx: &Ctx, _cx: &mut Context<Dash>) -> AnyElement {
    let s = &ctx.d.widgets.monitors.perf;
    let tk = ctx.tk;
    let down = format!("↓ {}", s.net_rx_text);
    let up = format!("↑ {}", s.net_tx_text);
    if ctx.round {
        // Both rings are relative to the recent peak, so a full one is nothing to warn about.
        let mut g = Gauge::new(
            "Download compared to the recent peak",
            s.net_rx_frac,
            tk.accent(),
        );
        g.inner = Some((
            "Upload compared to the recent peak",
            s.net_tx_frac,
            tk.warn(),
        ));
        g.high_is_bad = false;
        let size = (ctx.w * 0.075).max(10.0);
        let line = |text: String, color: Hsla| {
            let shown = text.clone();
            div()
                .w_full()
                .text_center()
                .text_size(px(size))
                .text_color(color)
                .debug_selector(move || shown)
                .child(text)
                .into_any_element()
        };
        return g.element(
            ctx.w,
            tk,
            vec![line(down, tk.accent()), line(up, tk.warn())],
        );
    }
    let capacity = ctx.opts.history;
    let tall = ctx.h > 110.0;
    let place = if s.net_addr.is_empty() {
        s.net_iface.clone()
    } else {
        format!("{}  {}", s.net_iface, s.net_addr)
    };
    page(tk)
        .child(
            div()
                .w_full()
                .h(px(24.0))
                .flex_none()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(label(down, tk.font_title(), tk.accent()))
                .child(label(up, tk.font_title(), tk.warn()))
                .child(muted(place, tk).flex_1().text_right()),
        )
        .when(tall, |p| {
            p.child(
                div()
                    .relative()
                    .w_full()
                    .flex_1()
                    .min_h(px(24.0))
                    .child(
                        div().absolute().size_full().child(
                            Graph::new(
                                "Download over time",
                                &s.net_rx_history,
                                capacity,
                                tk.accent(),
                            )
                            .element("net-rx-graph", tk)
                            .h_full(),
                        ),
                    )
                    .child(
                        div().absolute().size_full().child(
                            Graph::new("Upload over time", &s.net_tx_history, capacity, tk.warn())
                                .overlay()
                                .element("net-tx-graph", tk)
                                .h_full(),
                        ),
                    ),
            )
        })
        .when(tall, |p| {
            p.child(muted(s.net_peak_text.clone(), tk).flex_none().h(px(18.0)))
        })
        .into_any_element()
}

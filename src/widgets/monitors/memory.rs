//! Memory: RAM and swap, history, the biggest apps. Round: RAM over swap as two rings.

use super::charts::{Gauge, Graph, percent};
use super::parts::{head, label, muted, page, stat};
use crate::dash::Dash;
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, Context, div, px};

pub fn render(ctx: &Ctx, _cx: &mut Context<Dash>) -> AnyElement {
    let s = &ctx.d.widgets.monitors.perf;
    let tk = ctx.tk;
    if ctx.round {
        let mut g = Gauge::new("Memory", s.mem, tk.good());
        g.inner = Some(("Swap", s.swap, tk.warn()));
        g.big = percent(s.mem);
        g.caption = "RAM · swap".into();
        return g.element(ctx.w, tk, Vec::new());
    }
    let used = if s.mem > 0.9 { tk.bad() } else { tk.good() };
    let top_apps = ctx.opts.top_apps.max(0) as usize;
    page(tk)
        .child(head(
            None,
            "Memory",
            false,
            vec![
                muted(s.mem_used_text.clone(), tk)
                    .flex_1()
                    .text_right()
                    .into_any_element(),
                label(percent(s.mem), tk.font_title(), used).into_any_element(),
            ],
            tk,
        ))
        .child(
            Graph::new(
                "Memory use over time",
                &s.mem_history,
                ctx.opts.history,
                tk.good(),
            )
            .element("mem-graph", tk)
            .flex_1()
            .min_h(px(30.0)),
        )
        .child(stat(
            s.mem_avail_text.clone(),
            s.swap_text.clone(),
            if s.swap > 0.5 { tk.warn() } else { tk.dim() },
            tk,
        ))
        .when(top_apps > 0 && ctx.h > 220.0, |p| {
            p.child(
                div().w_full().flex_none().flex().flex_col().children(
                    s.top_mem
                        .iter()
                        .take(top_apps)
                        .map(|(name, value)| stat(name.clone(), value.clone(), tk.text(), tk)),
                ),
            )
        })
        .into_any_element()
}

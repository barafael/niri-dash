//! The system monitor: CPU and memory, each with its history; per-core bars; network and load.
//! Round: CPU over memory as two rings.

use super::charts::{Gauge, Graph, columns, percent};
use super::parts::{label, muted, page};
use crate::dash::Dash;
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, Context, div, px};

pub fn render(ctx: &Ctx, _cx: &mut Context<Dash>) -> AnyElement {
    let s = &ctx.d.widgets.monitors.perf;
    let tk = ctx.tk;
    if ctx.round {
        let mut g = Gauge::new("CPU", s.cpu, tk.accent());
        g.inner = Some(("Memory", s.mem, tk.good()));
        g.big = percent(s.cpu);
        g.caption = format!("CPU · RAM {}", percent(s.mem));
        return g.element(ctx, Vec::new());
    }
    let capacity = ctx.opts.history;
    let line = |title: &'static str, text: &str, extra: Option<&str>| {
        div()
            .w_full()
            .h(px(20.0))
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .child(label(title, tk.font_body(), tk.text()).font_weight(gpui::FontWeight::SEMIBOLD))
            .child(muted(text.to_string(), tk).flex_1())
            .children(extra.map(|t| muted(t.to_string(), tk).flex_none()))
    };
    page(tk)
        .child(line("CPU", &s.cpu_text, Some(&s.temp_text)))
        .child(
            Graph::new("CPU use over time", &s.cpu_history, capacity, tk.accent())
                .element("cpu-graph", tk)
                .flex_1(),
        )
        .when(ctx.opts.cores && ctx.h > 200.0, |p| {
            p.child(columns(&s.cores, 14.0, 2.0, tk))
        })
        .child(line("Memory", &s.mem_text, None))
        .child(
            Graph::new("Memory use over time", &s.mem_history, capacity, tk.good())
                .element("mem-graph", tk)
                .flex_1(),
        )
        .child(
            muted(format!("{}     {}", s.net_text, s.load_text), tk)
                .flex_none()
                .h(px(18.0)),
        )
        .into_any_element()
}

//! GPU: load and video memory. Says so when there is no GPU whose load can be read.
//! Round: load over video memory as two rings.

use super::charts::{Gauge, Graph, bar, percent};
use super::parts::{empty, head, label, muted, page, stat};
use crate::dash::Dash;
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, Context, px};

pub fn render(ctx: &Ctx, _cx: &mut Context<Dash>) -> AnyElement {
    let tk = ctx.tk;
    let Some(g) = ctx.d.widgets.monitors.perf.gpu.as_ref() else {
        return empty("No GPU with readable load (amdgpu)", tk);
    };
    if ctx.round {
        let mut gauge = Gauge::new("GPU load", g.busy, tk.accent());
        gauge.inner = Some(("Video memory", g.vram, tk.warn()));
        gauge.big = percent(g.busy);
        gauge.caption = "GPU · VRAM".into();
        return gauge.element(ctx.w, tk, Vec::new());
    }
    let heat = if g.power_text.is_empty() {
        g.temp_text.clone()
    } else {
        format!("{}  ·  {}", g.temp_text, g.power_text)
    };
    page(tk)
        .child(head(
            None,
            "GPU",
            false,
            vec![
                muted(g.name.clone(), tk)
                    .flex_1()
                    .text_right()
                    .into_any_element(),
                muted(heat, tk).flex_none().into_any_element(),
                label(percent(g.busy), tk.font_title(), tk.accent()).into_any_element(),
            ],
            tk,
        ))
        .when(ctx.h > 110.0, |p| {
            p.child(
                Graph::new(
                    "GPU load over time",
                    &g.history,
                    ctx.opts.history,
                    tk.accent(),
                )
                .element("gpu-graph", tk)
                .flex_1()
                .min_h(px(24.0)),
            )
        })
        .child(stat("Video memory", g.vram_text.clone(), tk.text(), tk))
        .child(bar("gpu-vram", "Video memory", g.vram, tk.warn(), true, tk))
        .into_any_element()
}

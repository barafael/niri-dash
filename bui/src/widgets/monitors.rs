//! System monitor, memory, disks, network, GPU and Claude usage: what the perf feed reports.

use super::{Ctx, chart, empty, line, load_tone, meter, ring};
use crate::widget_settings::Opt;
use blenderui::prelude::*;

/// The last `secs` of a history (one value a second).
fn last(v: &[f32], secs: i64) -> Vec<f32> {
    let n = (secs.max(2) as usize).min(v.len());
    v[v.len() - n..].to_vec()
}

fn waiting() -> Div {
    empty("Measuring…")
}

/// A row with a label, a level and a figure.
fn stat(label: &str, v: f32, text: &str) -> Div {
    div()
        .class("col")
        .child(
            div()
                .class("row small")
                .child(div().class("grow dim").child(label.to_string()))
                .child(div().child(text.to_string())),
        )
        .child(meter(v, load_tone(v)))
}

pub fn system(c: &mut Ctx) -> Div {
    let Some(s) = c.d.data.perf.as_deref() else {
        return waiting();
    };
    if c.round {
        let col = c.d.look.color("accent").to_string();
        return ring(
            s.cpu,
            &col,
            c.d.look.color("track"),
            c.d.look.color("card"),
            div()
                .class("col center")
                .child(div().class("small dim").child("CPU"))
                .child(div().class("title").child(s.cpu_text.clone())),
        );
    }
    let secs = c.opt_int(Opt::History);
    let mut el = div()
        .class("col grow")
        .child(stat("CPU", s.cpu, &s.cpu_text))
        .child(stat("Memory", s.mem, &s.mem_text));
    if c.h > 120.0 {
        el = el.child(chart(&last(&s.cpu_history, secs), c.d.look.color("accent")));
    }
    if c.opt_bool(Opt::Cores) && c.h > 170.0 && !s.cores.is_empty() {
        el = el.child(
            div()
                .class("row")
                .style("height: 24px; align-items: flex-end; gap: 1px")
                .children(s.cores.iter().map(|v| {
                    div().class("grow").style(format!(
                        "height: {:.0}%; min-height: 1px; background: var(--accent)",
                        v.clamp(0.0, 1.0) * 100.0
                    ))
                })),
        );
    }
    el.child(
        div()
            .class("row small dim wrap")
            .child(div().child(s.load_text.clone()))
            .child(div().child(s.temp_text.clone()))
            .child(div().child(s.net_text.clone())),
    )
}

pub fn memory(c: &mut Ctx) -> Div {
    let Some(s) = c.d.data.perf.as_deref() else {
        return waiting();
    };
    if c.round {
        let col = c.d.look.color("accent").to_string();
        return ring(
            s.mem,
            &col,
            c.d.look.color("track"),
            c.d.look.color("card"),
            div()
                .class("col center")
                .child(div().class("small dim").child("RAM"))
                .child(div().class("title").child(s.mem_text.clone())),
        );
    }
    let secs = c.opt_int(Opt::History);
    let top = c.opt_int(Opt::TopApps) as usize;
    let mut el = div()
        .class("col grow")
        .child(stat("Used", s.mem, &s.mem_used_text))
        .child(div().class("small dim").child(s.mem_avail_text.clone()))
        .child(stat("Swap", s.swap, &s.swap_text));
    if c.h > 110.0 {
        el = el.child(chart(&last(&s.mem_history, secs), c.d.look.color("accent")));
    }
    if top > 0 && c.h > 170.0 {
        for (name, v) in s.top_mem.iter().take(top) {
            el = el.child(
                c.read(
                    div()
                        .class("item small")
                        .child(div().class("grow ellipsis").child(name.clone()))
                        .child(div().class("dim").child(v.clone())),
                ),
            );
        }
    }
    el
}

pub fn disks(c: &mut Ctx) -> Div {
    let Some(s) = c.d.data.perf.as_deref() else {
        return waiting();
    };
    if c.round {
        let col = c.d.look.color("accent").to_string();
        return ring(
            s.root_used,
            &col,
            c.d.look.color("track"),
            c.d.look.color("card"),
            div()
                .class("col center")
                .child(div().class("small dim").child("/"))
                .child(
                    div()
                        .class("title")
                        .child(format!("{:.0}%", s.root_used * 100.0)),
                ),
        );
    }
    let hide = c.opt_bool(Opt::HideSystem);
    let mut el = div().class("col grow");
    let disks: Vec<_> = s
        .disks
        .iter()
        .filter(|d| !(hide && d.system))
        .cloned()
        .collect();
    if disks.is_empty() {
        el = el.child(empty("No disks"));
    }
    let mut list = div().class("col scroll");
    for d in disks {
        list = list.child(c.read(stat(&d.mount, d.used, &d.text)));
    }
    el = el.child(list);
    if c.h > 140.0 {
        el = el
            .child(chart(&s.disk_history, c.d.look.color("accent")))
            .child(
                div()
                    .class("row small dim")
                    .child(
                        div()
                            .class("grow")
                            .child(format!("read {}", s.disk_read_text)),
                    )
                    .child(div().child(format!("write {}", s.disk_write_text))),
            );
    }
    el
}

pub fn network(c: &mut Ctx) -> Div {
    let Some(s) = c.d.data.perf.as_deref() else {
        return waiting();
    };
    if c.round {
        return div()
            .class("col center")
            .child(
                div()
                    .class("small accent")
                    .child(format!("↓ {}", s.net_rx_text)),
            )
            .child(
                div()
                    .class("small good")
                    .child(format!("↑ {}", s.net_tx_text)),
            );
    }
    let secs = c.opt_int(Opt::History);
    let mut el = div()
        .class("col grow")
        .child(
            div()
                .class("row")
                .child(
                    div()
                        .class("grow title ellipsis")
                        .child(s.net_iface.clone()),
                )
                .child(div().class("small dim").child(s.net_addr.clone())),
        )
        .child(
            div()
                .class("row")
                .child(
                    div()
                        .class("grow accent")
                        .child(format!("↓ {}", s.net_rx_text)),
                )
                .child(div().class("good").child(format!("↑ {}", s.net_tx_text))),
        );
    if c.h > 90.0 {
        el = el
            .child(chart(
                &last(&s.net_rx_history, secs),
                c.d.look.color("accent"),
            ))
            .child(chart(
                &last(&s.net_tx_history, secs),
                c.d.look.color("good"),
            ));
    }
    el.child(
        div()
            .class("small dim")
            .child(format!("peak {}", s.net_peak_text)),
    )
}

pub fn gpu(c: &mut Ctx) -> Div {
    let Some(s) = c.d.data.perf.as_deref() else {
        return waiting();
    };
    let Some(g) = &s.gpu else {
        return empty("No GPU found");
    };
    if c.round {
        let col = c.d.look.color("accent").to_string();
        return ring(
            g.busy,
            &col,
            c.d.look.color("track"),
            c.d.look.color("card"),
            div()
                .class("col center")
                .child(div().class("small dim").child("GPU"))
                .child(
                    div()
                        .class("title")
                        .child(format!("{:.0}%", g.busy * 100.0)),
                ),
        );
    }
    let secs = c.opt_int(Opt::History);
    let mut el = div()
        .class("col grow")
        .child(line(g.name.clone()).class("title"))
        .child(stat("Busy", g.busy, &format!("{:.0}%", g.busy * 100.0)))
        .child(stat("Video memory", g.vram, &g.vram_text));
    if c.h > 120.0 {
        el = el.child(chart(&last(&g.history, secs), c.d.look.color("accent")));
    }
    el.child(
        div()
            .class("row small dim")
            .child(div().class("grow").child(g.temp_text.clone()))
            .child(div().child(g.power_text.clone())),
    )
}

pub fn claude_usage(c: &mut Ctx) -> Div {
    let Some(u) = c.d.data.usage.as_deref() else {
        return empty("Reading ~/.claude…");
    };
    if c.round {
        let col = c.d.look.color("accent").to_string();
        return ring(
            u.window_progress,
            &col,
            c.d.look.color("track"),
            c.d.look.color("card"),
            div()
                .class("col center")
                .child(div().class("title").child(u.window_count.clone()))
                .child(div().class("small dim").child("tokens")),
        );
    }
    let mut el = div().class("col grow").child(
        div()
            .class("row")
            .child(div().class("grow title").child(if u.plan.is_empty() {
                "Claude".to_string()
            } else {
                u.plan.clone()
            }))
            .child(div().class("small dim").child(u.burn.clone())),
    );
    if u.window_active {
        el = el
            .child(
                div()
                    .class("row small")
                    .child(div().class("grow").child(u.window_tokens.clone()))
                    .child(div().class("dim").child(u.window_left.clone())),
            )
            .child(meter(u.window_progress, load_tone(u.window_progress)))
            .child(div().class("small dim").child(format!(
                "{} messages · {} · resets {}",
                u.window_messages, u.window_cached, u.window_resets
            )));
    } else {
        el = el.child(empty("No session window open"));
    }
    if c.h > 130.0 {
        el = el
            .child(div().class("small dim").child("Last 24 hours"))
            .child(chart(&u.hourly, c.d.look.color("accent")));
    }
    if c.h > 190.0 {
        for (name, share) in &u.models {
            el = el.child(
                div()
                    .class("row small")
                    .child(div().class("grow ellipsis").child(name.clone()))
                    .child(div().class("dim").child(format!("{:.0}%", share * 100.0))),
            );
        }
    }
    el.child(
        div()
            .class("row small dim")
            .child(div().class("grow").child(format!("today {}", u.today)))
            .child(div().child(format!("week {}", u.week))),
    )
}

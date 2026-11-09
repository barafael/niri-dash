//! The clock widget: the time of day, and the date under it.

use crate::widgets::Ctx;
use crate::dash::Dash;
use chrono::{Local, Timelike};
use gpui::prelude::*;
use gpui::{AnyElement, Context, div, px};

/// What the clock shows, as of the last tick.
#[derive(Default)]
pub struct State {
    pub time: String,
    pub seconds: String,
    pub time12: String,
    pub ampm: String,
    pub date: String,
    pub weekday: String,
    pub day: u32,
    /// The hands: hours (0..12), minutes and seconds (0..60), fractions included.
    pub hour: f32,
    pub minute: f32,
    pub second: f32,
}

impl State {
    pub fn come_up(&mut self) {
        self.tick();
    }

    pub fn tick(&mut self) {
        use chrono::Datelike;
        let now = Local::now();
        let loc = niri_dash_system::locale();
        self.time = now.format("%H:%M").to_string();
        self.seconds = now.format("%S").to_string();
        self.time12 = now.format("%-I:%M").to_string();
        self.ampm = now.format("%p").to_string();
        let fmt = if loc.us {
            "%A, %B %-d, %Y"
        } else {
            "%A, %-d. %B %Y"
        };
        self.date = now.format_localized(fmt, loc.chrono).to_string();
        self.weekday = now.format_localized("%a", loc.chrono).to_string();
        self.day = now.day();
        let (h, m, s) = (now.hour() as f32, now.minute() as f32, now.second() as f32);
        self.hour = h % 12.0 + m / 60.0;
        self.minute = m + s / 60.0;
        self.second = s;
    }
}

pub fn render(ctx: &Ctx, _cx: &mut Context<Dash>) -> AnyElement {
    let c = &ctx.d.widgets.time.clock;
    let tk = ctx.tk;
    let (hours12, seconds, date) = (ctx.opts.hours12, ctx.opts.seconds, ctx.opts.date);
    let size = (ctx.h * 0.42).min(ctx.w * 0.24).clamp(22.0, 160.0);
    let small = (ctx.h * 0.16).min(ctx.w * 0.08).clamp(11.0, 48.0);
    div()
        .size_full()
        .p(px(tk.pad()))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(2.0))
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(4.0))
                .child(
                    div()
                        .text_size(px(size))
                        .font_weight(tk.display_weight())
                        .text_color(tk.text())
                        .debug_selector(|| "clock time".into())
                        .child(if hours12 { c.time12.clone() } else { c.time.clone() }),
                )
                .when(seconds || hours12, |row| {
                    row.child(
                        div()
                            .flex()
                            .flex_col()
                            .text_size(px(small))
                            .text_color(tk.faint())
                            .when(seconds, |col| col.child(c.seconds.clone()))
                            .when(hours12, |col| col.child(c.ampm.clone())),
                    )
                }),
        )
        .when(date && ctx.h > 120.0, |col| {
            col.child(
                div()
                    .text_size(px((ctx.h * 0.09).clamp(12.0, 22.0)))
                    .text_color(tk.dim())
                    .child(c.date.clone()),
            )
        })
        .into_any_element()
}

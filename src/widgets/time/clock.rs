//! The clock widget: the time of day, and the date under it, other time zones beside or under
//! it; round, an analog face.

use super::{draw, parts, placed_of};
use crate::dash::Dash;
use crate::layout::Kind;
use crate::widget_settings::{Opt, get_int, get_str};
use crate::widgets::Ctx;
use crate::zones::ZoneTime;
use chrono::{Local, Timelike};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Role, div, px};
use std::collections::BTreeMap;
use crate::kit::AriaState;

/// What the clock shows, as of the last tick.
pub struct State {
    pub time: String,
    pub seconds: String,
    pub time12: String,
    pub ampm: String,
    pub date: String,
    pub weekday: String,
    pub month: String,
    pub day: u32,
    /// The hands: hours (0..12), minutes and seconds (0..60), fractions included.
    pub hour: f32,
    pub minute: f32,
    pub second: f32,
    /// Each clock widget's other time zones, by widget id.
    pub zones: BTreeMap<u32, Vec<ZoneTime>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            time: "00:00".into(),
            seconds: "00".into(),
            time12: "12:00".into(),
            ampm: String::new(),
            date: String::new(),
            weekday: String::new(),
            month: String::new(),
            day: 1,
            hour: 0.0,
            minute: 0.0,
            second: 0.0,
            zones: BTreeMap::new(),
        }
    }
}

/// The time as of now, and each clock's other zones.
pub fn tick(d: &mut Dash) {
    use chrono::Datelike;
    let now = Local::now();
    let loc = niri_dash_system::locale();
    let us = d.locale.us;
    let c = &mut d.widgets.time.clock;
    c.time = now.format("%H:%M").to_string();
    c.seconds = now.format("%S").to_string();
    c.time12 = now.format("%-I:%M").to_string();
    c.ampm = now.format("%p").to_string();
    let fmt = if loc.us {
        "%A, %B %-d, %Y"
    } else {
        "%A, %-d. %B %Y"
    };
    c.date = now.format_localized(fmt, loc.chrono).to_string();
    c.weekday = now.format_localized("%a", loc.chrono).to_string();
    c.month = now.format_localized("%B", loc.chrono).to_string();
    c.day = now.day();
    let (h, m, s) = (now.hour() as f32, now.minute() as f32, now.second() as f32);
    c.hour = h % 12.0 + m / 60.0;
    c.minute = m + s / 60.0;
    c.second = s;
    // The other time zones of every clock that lists some.
    let utc = chrono::Utc::now();
    let mut zones = BTreeMap::new();
    for p in placed_of(d, Kind::Clock) {
        let list = crate::zones::parse(&get_str(&p.settings, Opt::Zones, us));
        if list.is_empty() {
            continue;
        }
        let hours12 = get_int(&p.settings, Opt::Hours, us) == 12;
        zones.insert(p.id, crate::zones::rows(&list, utc, &Local, hours12));
    }
    d.widgets.time.clock.zones = zones;
}

/// A zone's row: where, how far from here, and its time.
fn zone_row(z: &ZoneTime, font: f32, tk: &crate::kit::Tk) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .flex_none()
        .h(px(22.0))
        .items_center()
        .gap(px(8.0))
        .child(
            parts::elided(z.label.clone())
                .flex_1()
                .text_size(px(font))
                .text_color(tk.text()),
        )
        .child(
            div()
                .text_size(px(tk.font_small()))
                .text_color(tk.faint())
                .child(z.delta.clone()),
        )
        .child(
            div()
                .min_w(px(44.0))
                .flex()
                .justify_end()
                .text_size(px(font))
                .text_color(tk.dim())
                .child(z.time.clone()),
        )
}

pub fn render(ctx: &Ctx, _cx: &mut Context<Dash>) -> AnyElement {
    let c = &ctx.d.widgets.time.clock;
    let tk = ctx.tk;
    let (hours12, seconds, date) = (ctx.opts.hours12, ctx.opts.seconds, ctx.opts.date);
    if ctx.round {
        return round(ctx, c);
    }
    let zones: &[ZoneTime] = c.zones.get(&ctx.tile.id()).map_or(&[], Vec::as_slice);
    let n = zones.len() as f32;
    // Other time zones fit under the clock when it's tall, beside it when it's wide and short.
    let below = !zones.is_empty() && ctx.h >= 150.0 + n * 24.0;
    let side = !zones.is_empty() && !below && ctx.w >= 560.0;
    let main_h = if below { ctx.h - n * 24.0 - 8.0 } else { ctx.h };
    let main_w = if side { ctx.w * 0.58 } else { ctx.w };
    let size = (main_h * 0.42).min(main_w * 0.24).clamp(22.0, 160.0);
    let small = (main_h * 0.16).min(main_w * 0.08).clamp(11.0, 48.0);
    div()
        .size_full()
        .p(px(tk.pad()))
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(16.0))
                .child(
                    div()
                        .flex_1()
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
                                        .child(if hours12 {
                                            c.time12.clone()
                                        } else {
                                            c.time.clone()
                                        }),
                                )
                                .when(seconds || hours12, |row| {
                                    row.child(
                                        div()
                                            .flex()
                                            .flex_col()
                                            .text_color(tk.faint())
                                            .when(seconds, |col| {
                                                col.child(
                                                    div()
                                                        .text_size(px(small))
                                                        .child(c.seconds.clone()),
                                                )
                                            })
                                            .when(hours12, |col| {
                                                col.child(div().text_size(px(
                                                    small * if seconds { 0.7 } else { 1.0 },
                                                )).child(c.ampm.clone()))
                                            }),
                                    )
                                }),
                        )
                        .when(date && main_h > 120.0, |col| {
                            col.child(
                                div()
                                    .text_size(px((main_h * 0.09).clamp(12.0, 22.0)))
                                    .text_color(tk.dim())
                                    .child(c.date.clone()),
                            )
                        }),
                )
                .when(side, |row| {
                    row.child(
                        div()
                            .flex()
                            .flex_col()
                            .justify_center()
                            .gap(px(2.0))
                            .w(px(ctx.w * 0.42 - 16.0 - tk.pad()))
                            .children(zones.iter().map(|z| zone_row(z, tk.font_small(), tk))),
                    )
                }),
        )
        .when(below, |col| {
            col.child(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .gap(px(2.0))
                    .children(zones.iter().map(|z| zone_row(z, tk.font_body(), tk))),
            )
        })
        .into_any_element()
}

/// The analog face: ticks, the day, the hands.
fn round(ctx: &Ctx, c: &State) -> AnyElement {
    let tk = ctx.tk;
    let face = ctx.w.min(ctx.h);
    let (faint, text, accent) = (tk.faint(), tk.text(), tk.accent());
    let (hour, minute, second) = (c.hour, c.minute, c.second);
    let seconds = ctx.opts.seconds;
    let value = if ctx.opts.hours12 {
        format!("{} {}", c.time12, c.ampm)
    } else {
        c.time.clone()
    };
    let tick_width = (face * 0.012).max(1.5);
    let dot = (face * 0.045).max(6.0);
    div()
        .id("analog clock")
        .role(Role::Image)
        .aria_label("Clock")
        .aria_state(value)
        .relative()
        .size_full()
        .child(draw::painter(move |b, window| {
            let (x, y) = (f32::from(b.origin.x), f32::from(b.origin.y));
            let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
            // The face is a 100 by 100 box, centered.
            let unit = w.min(h) / 100.0;
            let (ox, oy) = (x + (w - 100.0 * unit) / 2.0, y + (h - 100.0 * unit) / 2.0);
            let at = |px: f32, py: f32| (ox + px * unit, oy + py * unit);
            let mut ticks = draw::Shape::new();
            for i in 0..12 {
                let turns = i as f32 / 12.0;
                // The quarters have longer marks.
                let inner = if i % 3 == 0 { 38.0 } else { 41.0 };
                let (a, b2) = (
                    draw::on_circle(at(50.0, 50.0), inner * unit, turns),
                    draw::on_circle(at(50.0, 50.0), 46.0 * unit, turns),
                );
                ticks.line(a, b2, tick_width);
            }
            ticks.paint(faint, window);
            let hand = |turns: f32, len: f32, tail: f32, width: f32, color, window: &mut gpui::Window| {
                let centre = at(50.0, 50.0);
                let from = draw::on_circle(centre, -tail * unit, turns);
                let to = draw::on_circle(centre, len * unit, turns);
                let mut s = draw::Shape::new();
                s.line(from, to, width);
                s.paint(color, window);
            };
            hand(hour / 12.0, 24.0, 8.0, (face * 0.035).max(3.0), text, window);
            hand(minute / 60.0, 36.0, 8.0, (face * 0.022).max(2.0), text, window);
            if seconds {
                hand(second / 60.0, 40.0, 12.0, (face * 0.01).max(1.0), accent, window);
            }
            let mut centre = draw::Shape::new();
            centre.circle(at(50.0, 50.0), dot / 2.0);
            centre.paint(accent, window);
        }))
        .child(
            div()
                .absolute()
                .top(px(ctx.h * 0.62))
                .w_full()
                .flex()
                .justify_center()
                .text_size(px((face * 0.07).max(10.0)))
                .text_color(tk.faint())
                .child(format!("{} {}", c.weekday, c.day)),
        )
        .into_any_element()
}

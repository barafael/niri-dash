//! The weather widget: what Open-Meteo says, through `niri_dash_system::weather`.

use super::parts::{self, weather_icon};
use crate::dash::Dash;
use crate::kit;
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Role, div, px};
use niri_dash_system::weather;

#[derive(Default)]
pub struct State {
    /// A forecast has arrived at least once.
    pub loaded: bool,
    /// Why there is none, or no new one.
    pub error: String,
    /// The last good forecast.
    pub forecast: Option<weather::State>,
}

impl State {
    pub fn show(&mut self, w: weather::State) {
        self.error = w.error.clone();
        if !w.error.is_empty() {
            // Keep showing the last good forecast if there is one.
            return;
        }
        self.loaded = true;
        self.forecast = Some(w);
    }
}

pub fn check(d: &Dash) -> Result<(), String> {
    let w = &d.widgets.time.weather;
    if w.loaded != w.forecast.is_some() {
        return Err("loaded without a forecast, or the other way round".into());
    }
    Ok(())
}

pub fn render(ctx: &Ctx, _cx: &mut Context<Dash>) -> AnyElement {
    let tk = ctx.tk;
    let w = &ctx.d.widgets.time.weather;
    let Some(f) = w.forecast.as_ref().filter(|_| w.loaded) else {
        return div()
            .size_full()
            .p(px(tk.pad()))
            .child(kit::empty(
                if w.error.is_empty() {
                    "Loading weather…".to_string()
                } else {
                    w.error.clone()
                },
                tk,
            ))
            .into_any_element();
    };
    let icon = weather_icon(f.icon);
    if ctx.round {
        // Round, only the icon says what the weather is like.
        let face = ctx.w;
        return div()
            .id("weather")
            .role(Role::Image)
            .aria_label("Weather")
            .aria_value(f.description.clone())
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(2.0))
            .child(kit::icon(icon, face * 0.3, tk.warn()))
            .child(
                parts::elided(f.temp.clone())
                    .text_size(px(face * 0.2))
                    .font_weight(tk.display_weight())
                    .text_color(tk.text()),
            )
            .child(
                parts::elided(f.place.clone())
                    .text_size(px((face * 0.075).max(10.0)))
                    .text_color(tk.dim()),
            )
            .into_any_element();
    }
    let tall = ctx.h > 150.0;
    let icon_size = 56.0f32.min(ctx.h - 2.0 * tk.pad() - if tall { 66.0 } else { 0.0 });
    div()
        .size_full()
        .p(px(tk.pad()))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(14.0))
                .child(kit::icon(icon, icon_size.max(0.0), tk.warn()))
                .child(
                    div()
                        .flex_none()
                        .text_size(px(44.0f32.min(ctx.h * 0.3)))
                        .font_weight(tk.display_weight())
                        .text_color(tk.text())
                        .child(f.temp.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .child(
                            parts::elided(format!("{} · {}", f.description, f.place))
                                .text_size(px(tk.font_body()))
                                .text_color(tk.text()),
                        )
                        .child(parts::muted(f.feels.clone(), tk))
                        .when(tall, |col| col.child(parts::muted(f.details.clone(), tk))),
                ),
        )
        .when(tall && !f.days.is_empty(), |col| {
            col.child(
                div()
                    .flex()
                    .flex_row()
                    .flex_none()
                    .h(px(58.0))
                    .gap(px(4.0))
                    .children(f.days.iter().enumerate().map(|(i, day)| {
                        div()
                            .id(("day", i))
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap(px(1.0))
                            .rounded(px(tk.radius_small()))
                            .bg(tk.subtle())
                            .text_size(px(tk.font_small()))
                            .child(div().text_color(tk.dim()).child(day.name.clone()))
                            .child(kit::icon(weather_icon(day.icon), 16.0, tk.warn()))
                            .child(
                                div()
                                    .text_color(tk.text())
                                    .child(format!("{} {}", day.max, day.min)),
                            )
                    })),
            )
        })
        .into_any_element()
}

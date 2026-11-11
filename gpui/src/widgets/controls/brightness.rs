//! The brightness widget: the screen's backlight, a slider or a knob. Port of
//! `BrightnessWidget` (`ui/widgets/system.slint`) and `show_brightness` and the `set`
//! callback of `src/app/feeds.rs`.

use super::sliders::{self, Knob, Row, percent, slider_row};
use super::{Fit, digit_level, on, setter};
use crate::dash::Dash;
use crate::keys::Key;
use crate::scope::{Scope, Stops};
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, Context, div, px};
use niri_dash_system::Tell;
use niri_dash_system::brightness::{Command, Event};

#[derive(Default)]
pub struct State {
    pub scope: Scope,
    pub level: f32,
    /// There is a backlight to set.
    pub available: bool,
    /// The tick at which the slider was last dragged.
    pub touched: Option<u64>,
}

pub fn event(d: &mut Dash, event: &Event) {
    let Event::Level(level) = event;
    let c = &mut d.widgets.controls;
    let held = c.held(c.brightness.touched);
    let b = &mut c.brightness;
    b.available = level.is_some();
    if let Some(l) = *level
        && !held
    {
        b.level = l.clamp(0.0, 1.0);
    }
}

/// The user sets the backlight: it shows what was asked for until the source catches up.
fn set(d: &mut Dash, v: f32) {
    let c = &mut d.widgets.controls;
    let v = v.clamp(0.0, 1.0);
    c.brightness.touched = Some(c.ticks);
    c.brightness.level = v;
    d.tx.brightness.tell(Command::Set(v));
}

/// A tap on the knob: dim, or if already dim, full.
fn toggle(d: &mut Dash) {
    let level = d.widgets.controls.brightness.level;
    set(d, if level > 0.6 { 0.3 } else { 1.0 });
}

struct Keys<'a> {
    d: &'a mut Dash,
    round: bool,
}

impl Stops for Keys<'_> {
    fn activate(&mut self, _: usize, _: usize) {
        if self.round {
            toggle(self.d);
        }
    }

    fn adjust(&mut self, _: usize, delta: i32) -> bool {
        let level = self.d.widgets.controls.brightness.level;
        set(self.d, level + delta as f32 * sliders::STEP);
        true
    }

    fn letter(&mut self, key: &Key, _: usize) -> bool {
        match digit_level(key) {
            Some(level) => {
                set(self.d, level);
                true
            }
            None => false,
        }
    }
}

/// h/l change the brightness by 5 %, 1-9 and 0 set 10-100 %.
pub fn key(d: &mut Dash, fit: &Fit, key: &Key) -> bool {
    let mut scope = d.widgets.controls.brightness.scope;
    let taken = scope.press(
        key,
        &mut Keys {
            d,
            round: fit.round,
        },
    );
    d.widgets.controls.brightness.scope = scope;
    taken
}

pub fn describe(b: &State) -> String {
    format!(
        "{}%{}",
        (b.level * 100.0).round(),
        if b.available { "" } else { ", no backlight" }
    )
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let b = &ctx.d.widgets.controls.brightness;
    let tk = ctx.tk;
    let id = ctx.tile.id() as usize;
    if ctx.round {
        return div()
            .size_full()
            .child(sliders::ring_knob(
                Knob {
                    id: ("brightness-knob", id),
                    name: "Brightness".into(),
                    value: b.level,
                    tint: tk.warn(),
                    icon: "sun",
                    label: percent(b.level),
                    dimmed: false,
                    kfocus: ctx.entered,
                    side: ctx.w,
                },
                tk,
                setter(cx, |d, v, _| set(d, v)),
                on(cx, |d, _| toggle(d)),
            ))
            .into_any_element();
    }
    div()
        .size_full()
        .px(px(tk.pad()))
        .flex()
        .items_center()
        .child(slider_row(
            Row {
                id: ("brightness-slider", id * 1000),
                label: "Brightness".into(),
                icon: "sun",
                icon_label: None,
                icon_tint: tk.warn(),
                icon_box: 32.0,
                tint: tk.warn(),
                enabled: b.available,
                value: b.level,
                kfocus: ctx.entered,
                kfocus_icon: false,
            },
            tk,
            setter(cx, |d, v, _| set(d, v)),
            |_, _, _| {},
        ))
        .into_any_element()
}

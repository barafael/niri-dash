//! The battery widget: charge, power profiles, the switch that blocks sleep, the batteries of
//! peripherals, and what blocks sleep. Port of `BatteryWidget` (`legacy/ui/widgets/system.slint`)
//! and `legacy/src/app/power.rs`.

use super::sliders::{ring, switch_view};
use super::{Fit, NoStops, on};
use crate::dash::Dash;
use crate::keys::Key;
use crate::kit::{self, Tk};
use crate::scope::{Scope, Stops};
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, Context, ElementId, Hsla, Role, ScrollHandle, Toggled, div, px};
use niri_dash_system::Tell;
use niri_dash_system::power::{Blocker, Command, DeviceKind, Event, Profile, State as Power};

#[derive(Default)]
pub struct State {
    pub scope: Scope,
    pub power: Power,
    pub inhibited: bool,
    pub blockers: Vec<Blocker>,
    pub scroll: ScrollHandle,
}

impl State {
    /// The profile in force, Balanced while none is known.
    fn profile(&self) -> Profile {
        self.power.profile.unwrap_or(Profile::Balanced)
    }
}

pub fn event(d: &mut Dash, event: &Event) {
    let b = &mut d.widgets.controls.battery;
    match event {
        Event::State(s) => b.power = s.clone(),
        Event::Inhibited(on) => b.inhibited = *on,
        Event::SleepBlockers(list) => b.blockers = list.clone(),
    }
}

/// What the widget has room for.
struct Layout {
    has_profiles: bool,
    show_inhibit: bool,
    show_devices: bool,
    show_blockers: bool,
    /// The ring's diameter, in the rectangular shape.
    ring: f32,
}

impl Layout {
    fn of(f: &Fit, p: &Power) -> Self {
        // In the rect shape the profiles need room for their row next to a usable ring; without
        // it they'd be cut off, with the cursor on them out of sight.
        let has_profiles = !p.profiles.is_empty() && (f.round || f.h - 2.0 * f.pad - 38.0 >= 56.0);
        let below_ring = if has_profiles { 38.0 } else { 0.0 };
        // The sleep switch gets a row only when there's room for it next to a usable ring.
        let show_inhibit = f.h - 2.0 * f.pad - below_ring >= 30.0 + 56.0;
        // Bigger: device batteries (3 rows), then what blocks sleep (4 rows).
        let show_devices = f.h >= 260.0 && !p.peripherals.is_empty();
        let show_blockers = f.h >= 360.0;
        let extra = if show_devices { 34.0 } else { 0.0 } + if show_blockers { 96.0 } else { 0.0 };
        let ring = (f.h - 2.0 * f.pad - below_ring - if show_inhibit { 30.0 } else { 0.0 } - extra)
            .clamp(0.0, 110.0);
        Self {
            has_profiles,
            show_inhibit,
            show_devices,
            show_blockers,
            ring,
        }
    }

    /// The keyboard stops (rectangular): the profiles, then the sleep switch.
    fn count(&self, f: &Fit) -> usize {
        if f.round {
            0
        } else {
            usize::from(self.has_profiles) + usize::from(self.show_inhibit)
        }
    }
}

/// The profile `d` steps away from the current one, wrapping around.
fn profile_step(p: &Power, d: i32) -> Option<Profile> {
    let n = p.profiles.len() as i32;
    if n == 0 {
        return None;
    }
    let i = match p.profile {
        None => 0,
        Some(cur) if p.profiles[0] == cur => 0,
        Some(cur) if n > 1 && p.profiles[1] == cur => 1,
        Some(_) => 2,
    };
    p.profiles.get((i + d).rem_euclid(n) as usize).copied()
}

fn set_profile(d: &mut Dash, p: Profile) {
    d.tx.power.tell(Command::SetProfile(p));
}

fn toggle_inhibit(d: &mut Dash) {
    d.tx.power.tell(Command::ToggleInhibit);
}

/// A tap on the round widget: the next profile of three, saver, balanced, performance.
fn cycle(d: &mut Dash) {
    let b = &d.widgets.controls.battery;
    if b.power.profiles.is_empty() {
        return;
    }
    let next = match b.profile() {
        Profile::PowerSaver => Profile::Balanced,
        Profile::Balanced if b.power.profiles.len() > 2 => Profile::Performance,
        _ => Profile::PowerSaver,
    };
    set_profile(d, next);
}

struct Keys<'a> {
    d: &'a mut Dash,
    round: bool,
    lay: &'a Layout,
}

impl Stops for Keys<'_> {
    fn activate(&mut self, cursor: usize, _: usize) {
        if !self.round && (!self.lay.has_profiles || cursor == 1) {
            toggle_inhibit(self.d);
        } else if self.lay.has_profiles {
            // With none current, the first is the one lit: that's what Enter picks.
            let p = &self.d.widgets.controls.battery.power;
            if let Some(next) = profile_step(p, i32::from(p.profile.is_some())) {
                set_profile(self.d, next);
            }
        }
    }

    fn adjust(&mut self, cursor: usize, delta: i32) -> bool {
        if !self.lay.has_profiles || (!self.round && cursor != 0) {
            return false;
        }
        if let Some(next) = profile_step(&self.d.widgets.controls.battery.power, delta) {
            set_profile(self.d, next);
        }
        true
    }
}

/// h/l (or Enter) change the power profile; j to the sleep switch, Enter flips it.
pub fn key(d: &mut Dash, fit: &Fit, key: &Key) -> bool {
    let lay = Layout::of(fit, &d.widgets.controls.battery.power);
    let mut scope = d.widgets.controls.battery.scope;
    scope.set_count(lay.count(fit), &NoStops);
    let taken = scope.press(
        key,
        &mut Keys {
            d,
            round: fit.round,
            lay: &lay,
        },
    );
    d.widgets.controls.battery.scope = scope;
    taken
}

pub fn refit(d: &mut Dash, fit: &Fit) {
    let n = Layout::of(fit, &d.widgets.controls.battery.power).count(fit);
    d.widgets.controls.battery.scope.set_count(n, &NoStops);
}

pub fn describe(b: &State, fit: &Fit) -> String {
    let n = Layout::of(fit, &b.power).count(fit);
    format!("stop {} of {n}, profile {:?}", b.scope.cursor, b.power.profile)
}

fn level_color(p: &Power, tk: &Tk) -> Hsla {
    if p.charging {
        tk.good()
    } else if p.percent <= 15 {
        tk.bad()
    } else if p.percent <= 30 {
        tk.warn()
    } else {
        tk.accent()
    }
}

fn profile_name(p: Profile) -> &'static str {
    match p {
        Profile::PowerSaver => "Saver",
        Profile::Balanced => "Balanced",
        Profile::Performance => "Performance",
    }
}

fn percent_text(p: &Power) -> String {
    if p.has_battery {
        format!("{}%", p.percent)
    } else {
        "On AC power".into()
    }
}

fn spoken(p: &Power) -> String {
    percent_text(p) + if p.charging { ", charging" } else { "" }
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let fit = Fit::of_ctx(ctx);
    let b = &ctx.d.widgets.controls.battery;
    let lay = Layout::of(&fit, &b.power);
    if fit.round {
        round(ctx, &fit, b, cx)
    } else {
        rect(ctx, &fit, b, &lay, cx)
    }
}

fn round(ctx: &Ctx, fit: &Fit, b: &State, cx: &mut Context<Dash>) -> AnyElement {
    let tk = ctx.tk;
    let p = &b.power;
    let color = level_color(p, tk);
    let id = ctx.tile.id() as usize;
    let can = !p.profiles.is_empty();
    div()
        .relative()
        .size_full()
        .child(ring(
            ("battery-ring", id),
            "Battery",
            spoken(p),
            p.percent as f32 / 100.0,
            (fit.w * 0.07).max(6.0),
            color,
            tk,
        ))
        .child(
            div()
                .absolute()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .child(kit::icon(
                    if p.charging { "bolt" } else { "battery" },
                    (fit.w * 0.22).min(32.0),
                    color,
                ))
                .child(
                    div()
                        .text_size(px((fit.w * 0.16).max(13.0)))
                        .text_color(tk.text())
                        .child(if p.has_battery {
                            format!("{}%", p.percent)
                        } else {
                            "AC".into()
                        }),
                )
                .when(b.inhibited, |c| {
                    c.child(
                        div()
                            .id(("battery-blocked", id))
                            .role(Role::Image)
                            .aria_label("Sleep is blocked")
                            .child(kit::icon("coffee", (fit.w * 0.12).min(16.0), tk.accent())),
                    )
                }),
        )
        .child(
            div()
                .id(("battery-profile", id))
                .role(Role::Button)
                .aria_label("Change power profile")
                .debug_selector(|| "Change power profile".into())
                .absolute()
                .size_full()
                .rounded_full()
                .when(can, |t| t.cursor_pointer())
                .on_click(on(cx, |d, _| cycle(d))),
        )
        .into_any_element()
}

fn rect(ctx: &Ctx, fit: &Fit, b: &State, lay: &Layout, cx: &mut Context<Dash>) -> AnyElement {
    let tk = ctx.tk;
    let p = &b.power;
    let id = ctx.tile.id() as usize;
    let color = level_color(p, tk);
    let entered = ctx.entered;
    let cursor = b.scope.cursor.min(lay.count(fit).saturating_sub(1));
    let on_profiles = entered && cursor == 0 && lay.has_profiles;
    let on_inhibit = entered && cursor == usize::from(lay.has_profiles) && lay.show_inhibit;

    let head = div()
        .flex()
        .flex_row()
        .gap(px(12.0))
        .min_h_0()
        .items_start()
        .when(!lay.show_blockers, |r| r.flex_1())
        .child(
            div()
                .relative()
                .flex_none()
                .size(px(lay.ring))
                .child(ring(
                    ("battery-ring", id),
                    "Battery",
                    spoken(p),
                    p.percent as f32 / 100.0,
                    6.0,
                    color,
                    tk,
                ))
                .child(
                    div()
                        .absolute()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(kit::icon(
                            if p.charging { "bolt" } else { "battery" },
                            lay.ring * 0.4,
                            color,
                        )),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .justify_center()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(
                    div()
                        .text_size(px(26.0))
                        .font_weight(tk.display_weight())
                        .text_color(tk.text())
                        .child(percent_text(p)),
                )
                .child(muted(&p.status_text, tk))
                .when(!p.power_text.is_empty(), |c| c.child(muted(&p.power_text, tk))),
        );

    let profiles = lay.has_profiles.then(|| {
        div()
            .flex()
            .flex_row()
            .gap(px(4.0))
            .h(px(30.0))
            .flex_none()
            .children(p.profiles.iter().enumerate().map(|(i, &prof)| {
                let current = p.profile == Some(prof);
                let lit = on_profiles && (current || (i == 0 && p.profile.is_none()));
                let name = profile_name(prof);
                let (hover, accent) = (tk.hover(), tk.accent());
                div()
                    .id(ElementId::from(("battery-profile", id * 8 + i)))
                    .role(Role::RadioButton)
                    .aria_label(name)
                    .aria_toggled(if current { Toggled::True } else { Toggled::False })
                    .aria_selected(lit)
                    .debug_selector(move || name.to_string())
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    .rounded(px(tk.radius_small().min(15.0)))
                    .border(px(2.0))
                    .border_color(if !lit {
                        gpui::transparent_black()
                    } else if current {
                        tk.text()
                    } else {
                        accent
                    })
                    .bg(if current { tk.accent() } else { tk.card_raised() })
                    .when(!current, |s| s.hover(move |h| h.bg(hover)))
                    .cursor_pointer()
                    .text_size(px(tk.font_small()))
                    .text_color(if current { tk.on_accent() } else { tk.text() })
                    .on_click(on(cx, move |d, _| set_profile(d, prof)))
                    .child(div().truncate().child(name))
            }))
    });

    let inhibit = lay.show_inhibit.then(|| {
        let label = "Block sleep and screen locking";
        div()
            .id(("battery-inhibit", id))
            .role(Role::Switch)
            .aria_label(label)
            .aria_toggled(if b.inhibited { Toggled::True } else { Toggled::False })
            .aria_selected(on_inhibit)
            .debug_selector(move || label.to_string())
            .flex_none()
            .h(px(22.0))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .cursor_pointer()
            // The whole row flips the switch, as in Plasma.
            .on_click(on(cx, |d, _| toggle_inhibit(d)))
            .child(kit::icon(
                "coffee",
                16.0,
                if b.inhibited { tk.accent() } else { tk.dim() },
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(tk.font_small()))
                    .text_color(if b.inhibited { tk.text() } else { tk.dim() })
                    .child(label),
            )
            .child(switch_view(b.inhibited, true, on_inhibit, tk))
    });

    let named = fit.w >= 380.0;
    let devices = lay.show_devices.then(|| {
        div()
            .flex()
            .flex_row()
            .gap(px(6.0))
            .h(px(26.0))
            .flex_none()
            .overflow_hidden()
            .children(
                p.peripherals
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| {
                        (*i + 1) as f32 * if named { 150.0 } else { 70.0 } <= fit.w - 2.0 * fit.pad
                    })
                    .map(|(i, dev)| {
                        let tint = if dev.percent <= 20 { tk.warn() } else { tk.text() };
                        let spoken = format!(
                            "{}{}%",
                            if named { String::new() } else { format!("{} ", dev.name) },
                            dev.percent
                        );
                        div()
                            .flex_none()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(5.0))
                            .px(px(8.0))
                            .rounded(px(tk.radius_small().min(13.0)))
                            .bg(tk.card_raised())
                            .child(kit::icon(device_icon(dev.kind), 15.0, tint))
                            .when(named, |c| {
                                c.child(
                                    div()
                                        .max_w(px(80.0))
                                        .truncate()
                                        .text_size(px(tk.font_small()))
                                        .text_color(tk.dim())
                                        .child(dev.name.clone()),
                                )
                            })
                            .child(
                                div()
                                    .id(ElementId::from(("battery-device", id * 8 + i)))
                                    .text_size(px(tk.font_small()))
                                    .text_color(tint)
                                    .aria_label(spoken)
                                    .child(format!("{}%", dev.percent)),
                            )
                    }),
            )
    });

    let blockers = lay.show_blockers.then(|| {
        let rows = b
            .blockers
            .iter()
            .map(|bl| {
                div()
                    .flex()
                    .flex_row()
                    .gap(px(6.0))
                    .h(px(22.0))
                    .flex_none()
                    .items_center()
                    .text_size(px(tk.font_small()))
                    .child(div().flex_none().text_color(tk.text()).child(bl.who.clone()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(tk.faint())
                            .child(bl.why.clone()),
                    )
                    .into_any_element()
            })
            .collect();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(px(2.0))
            .child(muted(
                if b.blockers.is_empty() {
                    "Nothing else blocks sleep"
                } else {
                    "Blocking sleep"
                },
                tk,
            ))
            .child(super::sliders::scroll_list(("battery-blockers", id), &b.scroll, rows, tk))
    });

    div()
        .size_full()
        .p(px(fit.pad))
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(head)
        .children(profiles)
        .children(inhibit)
        .children(devices)
        .children(blockers)
        .into_any_element()
}

fn muted(text: &str, tk: &Tk) -> gpui::Div {
    div()
        .truncate()
        .text_size(px(tk.font_small()))
        .text_color(tk.dim())
        .child(text.to_string())
}

fn device_icon(kind: DeviceKind) -> &'static str {
    match kind {
        DeviceKind::Mouse => "mouse",
        DeviceKind::Keyboard => "keyboard",
        DeviceKind::Headphones => "headphones",
        DeviceKind::Gamepad => "gamepad",
        DeviceKind::Phone => "phone",
        DeviceKind::Other => "battery",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn power(profile: Option<Profile>) -> Power {
        Power {
            profile,
            profiles: vec![Profile::PowerSaver, Profile::Balanced, Profile::Performance],
            ..Power::default()
        }
    }

    #[test]
    fn profiles_step_and_wrap() {
        let p = power(Some(Profile::Balanced));
        assert_eq!(profile_step(&p, 1), Some(Profile::Performance));
        assert_eq!(profile_step(&p, -1), Some(Profile::PowerSaver));
        let p = power(Some(Profile::Performance));
        assert_eq!(profile_step(&p, 1), Some(Profile::PowerSaver));
        let p = power(Some(Profile::PowerSaver));
        assert_eq!(profile_step(&p, -1), Some(Profile::Performance));
    }

    #[test]
    fn with_no_current_profile_the_first_is_the_one_lit() {
        let p = power(None);
        assert_eq!(profile_step(&p, 0), Some(Profile::PowerSaver));
        assert_eq!(profile_step(&Power::default(), 1), None);
    }
}

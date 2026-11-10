//! The session buttons: lock, suspend, log out, restart, power off, the last two asking twice
//! when `confirm` is on. Port of `SessionWidget` and `SessionButton`
//! (`legacy/ui/widgets/system.slint`) and the session callbacks of `legacy/src/app/power.rs`.
//!
//! Locking, suspending and logging out hide the dashboard first, so that it is not what the
//! lock screen shows or the session takes down with it.

use super::{DISARM, Fit, NoStops, on};
use crate::dash::{Dash, DashEvent};
use crate::keys::Key;
use crate::kit::{self, Tk, with_alpha};
use crate::scope::{Scope, Stops};
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, Context, ElementId, Role, div, px};
use niri_dash_system::Tell;
use niri_dash_system::session::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Lock,
    Suspend,
    Logout,
    Restart,
    Poweroff,
}

impl Button {
    fn label(self) -> &'static str {
        match self {
            Button::Lock => "Lock",
            Button::Suspend => "Suspend",
            Button::Logout => "Log out",
            Button::Restart => "Restart",
            Button::Poweroff => "Power off",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Button::Lock => "lock",
            Button::Suspend => "moon",
            Button::Logout => "logout",
            Button::Restart => "restart",
            Button::Poweroff => "power",
        }
    }

    /// Asks twice (when the widget is set to).
    fn danger(self, fit: &Fit) -> bool {
        fit.opts.confirm && matches!(self, Button::Restart | Button::Poweroff)
    }
}

/// A button waiting for its second press.
#[derive(Debug, Clone, Copy)]
pub struct Armed {
    pub button: Button,
    pub since: u64,
}

#[derive(Default)]
pub struct State {
    pub scope: Scope,
    pub armed: Vec<Armed>,
}

impl State {
    fn is_armed(&self, b: Button) -> bool {
        self.armed.iter().any(|a| a.button == b)
    }
}

/// The buttons shown, in order: each is a keyboard stop. The round shape has lock, suspend and
/// power off.
fn buttons(fit: &Fit) -> Vec<Button> {
    let o = &fit.opts;
    [
        (Button::Lock, o.lock),
        (Button::Suspend, o.suspend),
        (Button::Logout, o.logout && !fit.round),
        (Button::Restart, o.restart && !fit.round),
        (Button::Poweroff, o.poweroff),
    ]
    .into_iter()
    .filter_map(|(b, shown)| shown.then_some(b))
    .collect()
}

/// Press a button: one that asks first arms itself, and a second press (within three seconds)
/// does it.
fn press(d: &mut Dash, fit: &Fit, button: Button, cx: &mut Context<Dash>) {
    let s = &mut d.widgets.controls.session;
    if button.danger(fit) && !s.is_armed(button) {
        let since = d.widgets.controls.ticks;
        d.widgets.controls.session.armed.push(Armed { button, since });
        return;
    }
    s.armed.retain(|a| a.button != button);
    let (command, hides) = match button {
        Button::Lock => (Command::Lock, true),
        Button::Suspend => (Command::Suspend, true),
        Button::Logout => (Command::Logout, true),
        Button::Restart => (Command::Reboot, false),
        Button::Poweroff => (Command::Poweroff, false),
    };
    if hides {
        cx.emit(DashEvent::Hide);
    }
    d.tx.session.tell(command);
}

/// Armed buttons that weren't pressed again go back.
pub fn tick(d: &mut Dash) {
    let c = &mut d.widgets.controls;
    let now = c.ticks;
    c.session.armed.retain(|a| now.saturating_sub(a.since) < DISARM);
}

struct Keys<'a, 'b> {
    d: &'a mut Dash,
    fit: &'a Fit,
    buttons: &'a [Button],
    cx: &'a mut Context<'b, Dash>,
    moved: Option<usize>,
}

impl Stops for Keys<'_, '_> {
    fn activate(&mut self, cursor: usize, _: usize) {
        if let Some(&b) = self.buttons.get(cursor) {
            press(self.d, self.fit, b, self.cx);
        }
    }

    fn adjust(&mut self, cursor: usize, delta: i32) -> bool {
        let last = self.buttons.len().saturating_sub(1) as i32;
        self.moved = Some((cursor as i32 + delta).clamp(0, last) as usize);
        true
    }
}

/// h/l (or j/k) over the buttons, Enter presses one (twice for the ones that ask).
pub fn key(d: &mut Dash, fit: &Fit, key: &Key, cx: &mut Context<Dash>) -> bool {
    let list = buttons(fit);
    let mut scope = d.widgets.controls.session.scope;
    scope.set_count(list.len(), &NoStops);
    let mut keys = Keys {
        d,
        fit,
        buttons: &list,
        cx,
        moved: None,
    };
    let taken = scope.press(key, &mut keys);
    if let Some(to) = keys.moved {
        scope.move_to(to, &NoStops);
    }
    d.widgets.controls.session.scope = scope;
    taken
}

pub fn refit(d: &mut Dash, fit: &Fit) {
    let n = buttons(fit).len();
    d.widgets.controls.session.scope.set_count(n, &NoStops);
}

pub fn describe(s: &State, fit: &Fit) -> String {
    let list = buttons(fit);
    let on = list.get(s.scope.cursor).map_or("nothing", |b| b.label());
    format!("on {on}{}", if s.armed.is_empty() { "" } else { ", asking" })
}

/// One session button. `side` is its height, for the icon and for whether there is room for a
/// label.
struct Look<'a> {
    button: Button,
    index: usize,
    id: usize,
    compact: bool,
    circle: bool,
    kfocus: bool,
    armed: bool,
    height: f32,
    tk: &'a Tk,
}

fn button_view(
    look: Look,
    cx: &mut Context<Dash>,
    fit: &Fit,
) -> gpui::Stateful<gpui::Div> {
    let Look {
        button,
        index,
        id,
        compact,
        circle,
        kfocus,
        armed,
        height,
        tk,
    } = look;
    let name = button.label();
    let (hover, pressed, accent, bad) = (tk.hover(), tk.pressed(), tk.accent(), tk.bad());
    let fit = fit.clone();
    div()
        .id(ElementId::from(("session", id * 8 + index)))
        .role(Role::Button)
        .aria_label(name)
        .aria_value(if armed { "Press again to confirm" } else { "" })
        .aria_selected(kfocus)
        .debug_selector(move || name.to_string())
        // The buttons share the tile's width, down to its smallest size, in any font: a label
        // that is too wide for its button is cut short, and doesn't push the last button out.
        .min_w_0()
        .overflow_hidden()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(2.0))
        .rounded(px(if circle { 1000.0 } else { tk.radius_small() }))
        .border(px(2.0))
        .border_color(if kfocus { accent } else { gpui::transparent_black() })
        .bg(if armed {
            with_alpha(bad, 0.25)
        } else if kfocus {
            hover
        } else {
            tk.card_raised()
        })
        .when(!armed, |b| b.hover(move |s| s.bg(hover)).active(move |s| s.bg(pressed)))
        .cursor_pointer()
        .on_mouse_down(gpui::MouseButton::Left, crate::kit::own_press)
        .on_click(on(cx, move |d, cx| press(d, &fit, button, cx)))
        .child(kit::icon(
            button.icon(),
            22.0f32.min(height * 0.45),
            if armed { bad } else { tk.text() },
        ))
        .when(!compact && height > 54.0, |b| {
            b.child(
                div()
                    .max_w_full()
                    .truncate()
                    .text_size(px(tk.font_small()))
                    .text_color(if armed { bad } else { tk.dim() })
                    .child(if armed { "Confirm" } else { name }),
            )
        })
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let fit = Fit::of_ctx(ctx);
    let s = &ctx.d.widgets.controls.session;
    let tk = ctx.tk;
    let id = ctx.tile.id() as usize;
    let list = buttons(&fit);
    let cursor = s.scope.cursor.min(list.len().saturating_sub(1));
    if fit.round {
        let b = fit.w * 0.3;
        let at = |button: Button| list.iter().position(|&x| x == button);
        let place = |button: Button, x: f32, y: f32, cx: &mut Context<Dash>| {
            let index = at(button)?;
            Some(
                button_view(
                    Look {
                        button,
                        index,
                        id,
                        compact: true,
                        circle: true,
                        kfocus: ctx.entered && cursor == index,
                        armed: s.is_armed(button),
                        height: b,
                        tk,
                    },
                    cx,
                    &fit,
                )
                .absolute()
                .left(px(x))
                .top(px(y))
                .size(px(b)),
            )
        };
        return div()
            .relative()
            .size_full()
            .children(place(Button::Lock, (fit.w - b) / 2.0, fit.w * 0.12, cx))
            .children(place(Button::Suspend, fit.w * 0.16, fit.w * 0.5, cx))
            .children(place(Button::Poweroff, fit.w * 0.84 - b, fit.w * 0.5, cx))
            .into_any_element();
    }
    let height = (fit.h - 2.0 * fit.pad_tight).max(0.0);
    div()
        .size_full()
        .p(px(fit.pad_tight))
        .flex()
        .flex_row()
        .gap(px(8.0))
        .children(list.iter().enumerate().map(|(index, &button)| {
            button_view(
                Look {
                    button,
                    index,
                    id,
                    compact: false,
                    circle: false,
                    kfocus: ctx.entered && cursor == index,
                    armed: s.is_armed(button),
                    height,
                    tk,
                },
                cx,
                &fit,
            )
            .flex_1()
        }))
        .into_any_element()
}

//! The small pieces of edit mode's views: buttons with names a screen reader can say, setting
//! rows (a switch, a stepper, a line to type into), the choice cards of the Colors and Style tabs,
//! the tabs. Ports of `common.slint`'s `IconButton` and `TextButton` and of `app.slint`'s
//! `SettingControl`, `ChoiceCard` and `Segment`.

use crate::edit::settings::{RowKind, SettingRow};
use crate::kit::{self, Tk};
use crate::theme::Color;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, Div, ElementId, Entity, FontWeight, Hsla, Role, SharedString,
    Stateful, Toggled, Window, div, px,
};
use gpui_kit::component::input::{Input, InputState};

/// The state a button is drawn in.
#[derive(Debug, Clone, Copy)]
pub struct Look {
    pub enabled: bool,
    /// Switched on (a toggle button, the shape button of a round widget).
    pub active: bool,
    /// Under the keyboard cursor.
    pub kfocus: bool,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            enabled: true,
            active: false,
            kfocus: false,
        }
    }
}

/// A color between two: `weight` of `a`, the rest of `b`.
pub fn mix(a: Color, b: Color, weight: f32) -> Hsla {
    let k = weight.clamp(0.0, 1.0);
    let ch = |x: u8, y: u8| (x as f32 * k + y as f32 * (1.0 - k)).round() as u8;
    Color::from_argb_u8(
        ch(a.alpha(), b.alpha()),
        ch(a.red(), b.red()),
        ch(a.green(), b.green()),
        ch(a.blue(), b.blue()),
    )
    .hsla()
}

/// An id made of a namespace and a position.
pub fn eid(ns: &str, n: usize) -> ElementId {
    ElementId::Name(SharedString::from(format!("{ns}-{n}")))
}

/// A square button with an icon. Its name says what it does.
#[allow(clippy::too_many_arguments)]
pub fn icon_btn(
    id: impl Into<ElementId>,
    icon: &str,
    label: impl Into<SharedString>,
    size: f32,
    tint: Hsla,
    look: Look,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let label: SharedString = label.into();
    let name = label.to_string();
    let (hover, pressed) = (tk.hover(), tk.pressed());
    let bg = if look.active {
        tk.accent_soft()
    } else if look.kfocus {
        hover
    } else {
        gpui::transparent_black()
    };
    let b = div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .aria_selected(look.kfocus)
        .debug_selector(move || name)
        .flex_none()
        .size(px(size))
        .rounded(px((size / 2.0).min(tk.radius_small())))
        .flex()
        .items_center()
        .justify_center()
        .bg(bg)
        .when(look.kfocus, |b| b.border_2().border_color(tk.accent()))
        .child(kit::icon(
            icon,
            size * 0.55,
            if look.active { tk.accent() } else { tint },
        ));
    if look.enabled {
        b.cursor_pointer()
            .hover(move |s| s.bg(hover))
            .active(move |s| s.bg(pressed))
            .on_click(on_click)
    } else {
        b.opacity(0.4)
    }
}

/// A button with a label.
#[allow(clippy::too_many_arguments)]
pub fn text_btn(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    shown: Option<&str>,
    icon: Option<&str>,
    primary: bool,
    look: Look,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let label: SharedString = label.into();
    let name = label.to_string();
    let (hover, pressed) = (tk.hover(), tk.pressed());
    let accent = tk.accent();
    let tint = if primary { tk.on_accent() } else { tk.text() };
    let bg = if primary {
        accent
    } else {
        tk.card_raised()
    };
    let b = div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.clone())
        .aria_selected(look.kfocus)
        .debug_selector(move || name)
        .h(px(30.0))
        .min_w_0()
        .px(px(12.0))
        .flex()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .rounded(px((15.0_f32).min(tk.radius_small())))
        .bg(bg)
        .text_size(px(tk.font_body()))
        .text_color(tint)
        .when(look.kfocus, |b| {
            b.border_2()
                .border_color(if primary { tk.text() } else { accent })
        })
        .children(icon.map(|i| kit::icon(i, 16.0, tint)))
        .child(div().truncate().child(shown.map_or(label, |s| SharedString::from(s.to_string()))));
    if look.enabled {
        b.cursor_pointer()
            .when(!primary, |b| {
                b.hover(move |s| s.bg(hover)).active(move |s| s.bg(pressed))
            })
            .on_click(on_click)
    } else {
        b.opacity(0.45)
    }
}

/// A small heading over a group of rows.
pub fn section_label(text: impl Into<SharedString>, tk: &Tk) -> Div {
    div()
        .text_size(px(tk.font_small()))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tk.faint())
        .truncate()
        .child(text.into())
}

/// What a row of the sidebar does when stepped, reset or typed into: the list it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowOwner {
    Grid,
    Arrange,
    Card,
    Style,
}

/// Everything a setting row is drawn from.
pub struct RowSpec {
    pub owner: RowOwner,
    /// Its position in the owner's list (and the keyboard's stop, apart from an offset).
    pub pos: usize,
    pub row: SettingRow,
    pub kfocus: bool,
    /// 0 at rest, up to 1 halfway through a shake (a step was refused).
    pub shake: f32,
    /// Whether the row can be used at all.
    pub live: bool,
    pub field: Option<Entity<InputState>>,
}

/// The ids and names of a row's parts.
fn ns(owner: RowOwner) -> &'static str {
    match owner {
        RowOwner::Grid => "grid",
        RowOwner::Arrange => "arrange",
        RowOwner::Card => "card",
        RowOwner::Style => "style",
    }
}

/// A setting in the sidebar: a label with a switch, a − value + stepper, or a line to type into
/// below it. Set to something other than the default, it is highlighted and gets a reset button.
pub fn setting_row(
    spec: RowSpec,
    tk: &Tk,
    cx: &mut gpui::Context<crate::dash::Dash>,
) -> AnyElement {
    let RowSpec {
        owner,
        pos,
        row,
        kfocus,
        shake,
        live,
        field,
    } = spec;
    let n = ns(owner);
    let text_row = row.kind == RowKind::Text;
    let shaking = (shake * std::f32::consts::PI).sin();
    let dx = (shake * std::f32::consts::PI * 6.0).sin() * shaking * 7.0;
    let base = if row.changed {
        tk.c(crate::theme::ColorRole::AccentSoft)
    } else {
        tk.c(crate::theme::ColorRole::CardRaised)
    };
    let bg = mix(base, tk.c(crate::theme::ColorRole::Bad), 1.0 - shaking * 0.45);
    let label = row.label.clone();
    let dash = cx.entity();
    let act = {
        let dash = dash.clone();
        move |delta: i32, cx: &mut App| {
            dash.update(cx, |d, cx| {
                d.row_step(owner, pos, delta);
                cx.notify();
            })
        }
    };
    let reset = {
        let dash = dash.clone();
        move |cx: &mut App| {
            dash.update(cx, |d, cx| {
                d.row_reset(owner, pos);
                cx.notify();
            })
        }
    };
    let on = row.on;
    let on_toggle = row.kind == RowKind::Toggle;
    let value_text = row.value.clone();
    let head = div()
        .h(px(40.0))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(4.0))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(tk.font_body()))
                .text_color(tk.text())
                .child(label.clone()),
        )
        .when(row.changed, |h| {
            let reset = reset.clone();
            h.child(icon_btn(
                eid(&format!("{n}-reset"), pos),
                "reset",
                format!("Reset {label}"),
                26.0,
                tk.dim(),
                Look {
                    enabled: live,
                    ..Look::default()
                },
                tk,
                move |_, _, cx| reset(cx),
            ))
        })
        .when(row.kind == RowKind::Toggle, |h| {
            let act = act.clone();
            h.child(switch_btn(
                eid(&format!("{n}-switch"), pos),
                on,
                label.clone(),
                live,
                tk,
                move |_, _, cx| act(1, cx),
            ))
        })
        .when(row.kind == RowKind::Stepper, |h| {
            let (dec, inc) = (act.clone(), act.clone());
            h.child(icon_btn(
                eid(&format!("{n}-dec"), pos),
                "minus",
                format!("Decrease {label}"),
                30.0,
                tk.text(),
                Look {
                    enabled: row.can_dec && live,
                    ..Look::default()
                },
                tk,
                move |_, _, cx| dec(-1, cx),
            ))
            .child(
                div()
                    .w(px(86.0))
                    .flex_none()
                    .truncate()
                    .text_center()
                    .text_size(px(tk.font_body()))
                    .text_color(if row.changed { tk.accent() } else { tk.dim() })
                    .debug_selector({
                        let name = format!("{label} value");
                        move || name
                    })
                    .child(row.value.clone()),
            )
            .child(icon_btn(
                eid(&format!("{n}-inc"), pos),
                "plus",
                format!("Increase {label}"),
                30.0,
                tk.text(),
                Look {
                    enabled: row.can_inc && live,
                    ..Look::default()
                },
                tk,
                move |_, _, cx| inc(1, cx),
            ))
        });
    let inner = div()
        .relative()
        .left(px(dx))
        .pl(px(12.0))
        .pr(px(4.0))
        .when(text_row, |d| d.pb(px(8.0)))
        .flex()
        .flex_col()
        .child(head)
        .when_some(field.filter(|_| text_row), |d, state| {
            d.child(
                div()
                    .pr(px(8.0))
                    .child(text_field(state, &label, live, tk)),
            )
        });
    div()
        .id(eid(&format!("{n}-row"), pos))
        .role(Role::ListItem)
        .aria_label(label.clone())
        .aria_selected(kfocus)
        .aria_value(if on_toggle { String::new() } else { value_text.clone() })
        .debug_selector({
            let name = format!("row {label}");
            move || name
        })
        .h(px(if text_row { 76.0 } else { 40.0 }))
        .flex_none()
        .rounded(px(tk.radius_small()))
        .bg(bg)
        .when(kfocus, |d| d.border_2().border_color(tk.accent()))
        .child(inner)
        .into_any_element()
}

/// An on/off switch with a name a screen reader can say.
pub fn switch_btn(
    id: impl Into<ElementId>,
    on: bool,
    label: impl Into<SharedString>,
    enabled: bool,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let label: SharedString = label.into();
    let name = label.to_string();
    let b = div()
        .id(id)
        .role(Role::Switch)
        .aria_label(label)
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .debug_selector(move || name)
        .flex_none()
        .w(px(40.0))
        .h(px(22.0))
        .rounded(px(11.0_f32.min(tk.radius_small())))
        .bg(if on { tk.accent() } else { tk.track() })
        .relative()
        .child(
            div()
                .absolute()
                .top(px(3.0))
                .left(px(if on { 21.0 } else { 3.0 }))
                .size(px(16.0))
                .rounded(px(8.0_f32.min((tk.radius_small() - 3.0).max(0.0))))
                .bg(if on { tk.on_accent() } else { tk.dim() }),
        );
    if enabled {
        b.cursor_pointer().on_click(on_click)
    } else {
        b.opacity(0.4)
    }
}

/// A one-line text field in the theme's colors.
pub fn text_field(state: Entity<InputState>, label: &str, live: bool, tk: &Tk) -> impl IntoElement {
    let name = format!("{label} field");
    div()
        .h(px(32.0))
        .px(px(tk.pad_tight()))
        .flex()
        .items_center()
        .rounded(px(tk.radius_small()))
        .bg(tk.subtle())
        .border_1()
        .border_color(tk.border())
        .text_size(px(tk.font_body()))
        .text_color(tk.text())
        .when(!live, |d| d.opacity(0.5))
        .debug_selector(move || name)
        .child(
            Input::new(&state)
                .appearance(false)
                .bordered(false)
                .focus_bordered(false)
                .disabled(!live)
                .aria_label(label.to_string()),
        )
}

/// One of the sidebar's choices (a color scheme, a style): a preview, its name and description,
/// and a check mark on the current one.
#[allow(clippy::too_many_arguments)]
pub fn choice_card(
    id: impl Into<ElementId>,
    name: &str,
    description: &str,
    current: bool,
    kfocus: bool,
    height: f32,
    preview: impl IntoElement,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let hover = tk.hover();
    let bg = if current {
        tk.accent_soft()
    } else if kfocus {
        hover
    } else {
        tk.card_raised()
    };
    let label = name.to_string();
    div()
        .id(id)
        .role(Role::RadioButton)
        .aria_label(name.to_string())
        .aria_description(description.to_string())
        .aria_toggled(if current { Toggled::True } else { Toggled::False })
        .aria_selected(kfocus)
        .debug_selector(move || label)
        .h(px(height))
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .p(px(8.0))
        .rounded(px(tk.radius_small()))
        .bg(bg)
        .when(kfocus, |d| d.border_2().border_color(tk.accent()))
        .when(current && !kfocus, |d| d.border_1().border_color(tk.accent()))
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .on_click(on_click)
        .child(div().flex_none().w(px(72.0)).h_full().child(preview))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .justify_center()
                .child(
                    div()
                        .truncate()
                        .text_size(px(tk.font_body()))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(tk.text())
                        .child(name.to_string()),
                )
                .when(!description.is_empty(), |d| {
                    d.child(
                        div()
                            .truncate()
                            .text_size(px(tk.font_small()))
                            .text_color(tk.text())
                            .child(description.to_string()),
                    )
                }),
        )
        .when(current, |d| {
            d.child(kit::icon("check", 18.0, tk.accent()))
        })
        .into_any_element()
}

/// One segment of the sidebar's tab bar.
pub fn segment(
    id: impl Into<ElementId>,
    label: &'static str,
    active: bool,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let hover = tk.hover();
    div()
        .id(id)
        .role(Role::Tab)
        .aria_label(label)
        .aria_selected(active)
        .debug_selector(move || label.to_string())
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(tk.radius_small().min(16.0)))
        .text_size(px(tk.font_body()))
        .font_weight(FontWeight::SEMIBOLD)
        .truncate()
        .cursor_pointer()
        .when(active, |d| d.bg(tk.accent()).text_color(tk.on_accent()))
        .when(!active, |d| {
            d.text_color(tk.dim()).hover(move |s| s.bg(hover))
        })
        .on_click(on_click)
        .child(label)
        .into_any_element()
}

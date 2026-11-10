//! The pieces the tools' widgets share: rows, buttons and a scroll list that show the keyboard
//! cursor (`kfocus`), and a round widget's face. Ports of `common.slint`'s `Row`, `IconButton`,
//! `TextButton`, `ScrollList` and `RoundFace`.
//!
//! The element under the keyboard cursor says so (`aria_cursor`), so what the keyboard is on can
//! be found and read out; see `NavScope` in `legacy/ui/common.slint`.

use crate::kit::{self, Tk};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, Div, ElementId, Hsla, Role, ScrollHandle, SharedString, Stateful,
    Window, div, px, transparent_black,
};
use crate::kit::AriaCursor;
use crate::kit::AriaState;

/// A row of a list: `label` and `value` are what a screen reader says (the tests find it by
/// `label`); `selected` is the look of the current entry, `kfocus` the keyboard cursor. The caller
/// adds the click and the contents.
pub fn row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    value: Option<SharedString>,
    selected: bool,
    kfocus: bool,
    height: f32,
    tk: &Tk,
) -> Stateful<Div> {
    let label: SharedString = label.into();
    let name = label.to_string();
    let (hover, pressed) = (tk.hover(), tk.pressed());
    let bg = if selected {
        tk.accent_soft()
    } else if kfocus {
        hover
    } else {
        transparent_black()
    };
    div()
        .id(id)
        .role(Role::ListItem)
        .aria_label(label)
        .aria_cursor(kfocus)
        .when_some(value, |d, v| d.aria_state(v))
        .debug_selector(move || name)
        .w_full()
        .h(px(height))
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .rounded(px((height / 2.0).min(tk.radius_small())))
        .bg(bg)
        .border(px(2.0))
        .border_color(if kfocus {
            tk.accent()
        } else {
            transparent_black()
        })
        .cursor_pointer()
        .when(!selected && !kfocus, |d| d.hover(move |s| s.bg(hover)))
        .active(move |s| s.bg(pressed))
}

/// A square button with an icon; `fill` is its resting background.
#[allow(clippy::too_many_arguments)]
pub fn icon_button(
    id: impl Into<ElementId>,
    name: &str,
    label: &'static str,
    size: f32,
    tint: Hsla,
    fill: Option<Hsla>,
    kfocus: bool,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (hover, pressed) = (tk.hover(), tk.pressed());
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .aria_cursor(kfocus)
        .debug_selector(move || label.to_string())
        .flex_none()
        .size(px(size))
        .rounded(px((size / 2.0).min(tk.radius_small())))
        .when_some(fill, |d, f| d.bg(f))
        .when(kfocus, |d| d.bg(hover))
        .border(px(2.0))
        .border_color(if kfocus {
            tk.accent()
        } else {
            transparent_black()
        })
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .active(move |s| s.bg(pressed))
        .on_click(on_click)
        .child(kit::icon(name, size * 0.55, tint))
}

/// A button with a label, 30 px high.
pub fn text_button(
    id: impl Into<ElementId>,
    text: &'static str,
    label: &'static str,
    kfocus: bool,
    tk: &Tk,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (hover, pressed) = (tk.hover(), tk.pressed());
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .aria_cursor(kfocus)
        .debug_selector(move || label.to_string())
        .flex_none()
        .h(px(30.0))
        .px(px(12.0))
        .rounded(px(15.0_f32.min(tk.radius_small())))
        .bg(tk.card_raised())
        .border(px(2.0))
        .border_color(if kfocus {
            tk.accent()
        } else {
            transparent_black()
        })
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(tk.font_body()))
        .text_color(tk.text())
        .cursor_pointer()
        .hover(move |s| s.bg(hover))
        .active(move |s| s.bg(pressed))
        .on_click(on_click)
        .child(text)
}

/// A scrollable column with a slim scrollbar that only shows when there is something to scroll.
/// Children are direct (`handle.scroll_to_item(i)` brings the i-th into view).
pub fn scroll_list(
    id: impl Into<ElementId>,
    handle: &ScrollHandle,
    rows: Vec<AnyElement>,
    tk: &Tk,
) -> Div {
    let max = f32::from(handle.max_offset().y);
    let view = f32::from(handle.bounds().size.height);
    let top = -f32::from(handle.offset().y);
    let bar = (max > 1.0 && view > 0.0).then(|| {
        let content = view + max;
        let thumb = (view * view / content).max(24.0).min(view);
        let at = (view - thumb) * (top / max).clamp(0.0, 1.0);
        div()
            .absolute()
            .right(px(1.0))
            .top_0()
            .w(px(4.0))
            .h_full()
            .rounded(px(2.0))
            .bg(tk.subtle())
            .child(
                div()
                    .absolute()
                    .top(px(at))
                    .w_full()
                    .h(px(thumb))
                    .rounded(px(2.0))
                    .bg(tk.faint()),
            )
    });
    div()
        .relative()
        .flex_1()
        .min_h_0()
        .w_full()
        .child(
            div()
                .id(id)
                .size_full()
                .flex()
                .flex_col()
                .overflow_y_scroll()
                .track_scroll(handle)
                .children(rows),
        )
        .children(bar)
}

/// A round widget's face: a big icon over a caption, centred.
pub fn round_face(
    icon: &str,
    tint: Hsla,
    caption: impl Into<SharedString>,
    w: f32,
    tk: &Tk,
) -> AnyElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(2.0))
        .child(kit::icon(icon, w * 0.3, tint))
        .child(
            div()
                .text_size(px((w * 0.075).max(10.0)))
                .text_color(tk.dim())
                .truncate()
                .child(caption.into()),
        )
        .into_any_element()
}

//! The pieces the tools' widgets share: rows, buttons and a scroll list that show the keyboard
//! cursor (`kfocus`), and a round widget's face. Ports of `common.slint`'s `Row`, `IconButton`,
//! `TextButton`, `ScrollList` and `RoundFace`.
//!
//! The element under the keyboard cursor says so (`aria_selected`), so what the keyboard is on can
//! be found and read out; see `NavScope` in `legacy/ui/common.slint`.

use crate::kit::{self, Tk};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, Div, ElementId, Hsla, Role, ScrollHandle, SharedString, Stateful,
    Window, div, px, transparent_black,
};

/// A line of text is as high as Slint draws it (the font's ascent and descent, about 1.36 times
/// its size), not GPUI's 1.6: lines that wrap, and a column of centered texts, come out as tall.
pub const LINE: f32 = 1.362;

/// `inner` with that line height for all the text in it.
pub fn lined(inner: AnyElement) -> AnyElement {
    div()
        .size_full()
        .line_height(gpui::relative(LINE))
        .child(inner)
        .into_any_element()
}

/// The keyboard cursor: a 2 px ring in the accent color over an element of this corner radius,
/// drawn over it (Slint's `border-width`, which takes no room), so that the cursor moving never
/// moves anything inside.
pub fn ring(tk: &Tk, radius: f32) -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .rounded(px(radius))
        .border_2()
        .border_color(tk.accent())
}

/// A widget's header: an icon and a title, 28 px high (`Header` in `common.slint`; `kit::header`
/// is 26).
pub fn header(icon_name: &str, title: impl Into<SharedString>, tk: &Tk) -> Div {
    div()
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .h(px(28.0))
        .child(kit::icon(icon_name, 18.0, tk.dim()))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(tk.font_title()))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(tk.text())
                .child(title.into()),
        )
}

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
    let radius = (height / 2.0).min(tk.radius_small());
    div()
        .id(id)
        .role(Role::ListItem)
        .aria_label(label)
        .aria_selected(kfocus)
        .when_some(value, |d, v| d.aria_value(v))
        .debug_selector(move || name)
        .relative()
        .w_full()
        .h(px(height))
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .rounded(px(radius))
        .bg(bg)
        .when(kfocus, |d| d.child(ring(tk, radius)))
        .cursor_pointer()
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .when(!selected && !kfocus, |d| d.hover(move |s| s.bg(hover)))
        // A selected row stays as it is (Slint: `selected` comes before `pressed`).
        .when(!selected, |d| d.active(move |s| s.bg(pressed)))
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
    let radius = (size / 2.0).min(tk.radius_small());
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .aria_selected(kfocus)
        .debug_selector(move || label.to_string())
        .relative()
        .flex_none()
        .size(px(size))
        .rounded(px(radius))
        .when_some(fill, |d, f| d.bg(f))
        .when(kfocus, |d| d.bg(hover))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .hover(move |s| s.bg(hover))
        .active(move |s| s.bg(pressed))
        .on_click(on_click)
        .child(kit::icon(name, size * 0.55, tint))
        .when(kfocus, |d| d.child(ring(tk, radius)))
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
    let radius = 15.0_f32.min(tk.radius_small());
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .aria_selected(kfocus)
        .debug_selector(move || label.to_string())
        .relative()
        .flex_none()
        .h(px(30.0))
        .px(px(12.0))
        .rounded(px(radius))
        .bg(tk.card_raised())
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(tk.font_body()))
        .text_color(tk.text())
        .cursor_pointer()
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .hover(move |s| s.bg(hover))
        .active(move |s| s.bg(pressed))
        .on_click(on_click)
        .child(text)
        .when(kfocus, |d| d.child(ring(tk, radius)))
}

/// A scrollable column with a slim scrollbar that only shows when there is something to scroll.
/// Children are direct (`handle.scroll_to_item(i)` brings the i-th into view). Port of Slint's
/// `ScrollList`: lists sit in a card's padding, and the scrollbar goes there too, so rows are as
/// wide as the header above them; only a tight padding needs a gutter of its own.
pub fn scroll_list(
    id: impl Into<ElementId>,
    handle: &ScrollHandle,
    rows: Vec<AnyElement>,
    tk: &Tk,
) -> Div {
    let in_padding = tk.pad() >= 8.0;
    let gutter = if in_padding { 0.0 } else { 10.0 };
    let max = f32::from(handle.max_offset().y);
    let view = f32::from(handle.bounds().size.height);
    let top = -f32::from(handle.offset().y);
    let bar = (max > 1.0 && view > 0.0).then(|| {
        let content = view + max;
        let thumb = (view * view / content).max(24.0).min(view);
        let at = (view - thumb) * (top / max).clamp(0.0, 1.0);
        let right = if in_padding {
            -(tk.pad() + 4.0) / 2.0
        } else {
            1.0
        };
        div()
            .absolute()
            .right(px(right))
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
                .absolute()
                .top_0()
                .left_0()
                .bottom_0()
                .right(px(gutter))
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

//! The pieces the monitor widgets share: a label with a value, the title row, muted text.
//! Ports of `Stat` (`ui/widgets/monitor.slint`), `Header`, `Muted` and `Empty`
//! (`ui/common.slint`). Each text names itself to the tests (`debug_selector`).

use crate::kit::{self, Tk};
use gpui::prelude::*;
use gpui::{AnyElement, Div, Hsla, SharedString, div, px};

/// A widget's page: padded, its parts one under the other, 6 px apart.
pub fn page(tk: &Tk) -> Div {
    div()
        .size_full()
        .p(px(tk.pad()))
        .flex()
        .flex_col()
        .gap(px(6.0))
}

/// Secondary text, one line, cut with an ellipsis when it doesn't fit; it takes what is left of
/// the row.
pub fn muted(text: impl Into<SharedString>, tk: &Tk) -> Div {
    let text: SharedString = text.into();
    let shown = text.to_string();
    div()
        .min_w_0()
        .text_size(px(tk.font_small()))
        .text_color(tk.dim())
        .truncate()
        .debug_selector(move || shown)
        .child(text)
}

/// A line of text in a row that keeps its size.
pub fn label(text: impl Into<SharedString>, size: f32, color: Hsla) -> Div {
    let text: SharedString = text.into();
    let shown = text.to_string();
    div()
        .flex_none()
        .whitespace_nowrap()
        .text_size(px(size))
        .text_color(color)
        .debug_selector(move || shown)
        .child(text)
}

/// A label on the left and a value on the right, 20 px high.
pub fn stat(
    name: impl Into<SharedString>,
    value: impl Into<SharedString>,
    value_color: Hsla,
    tk: &Tk,
) -> Div {
    let (name, value): (SharedString, SharedString) = (name.into(), value.into());
    let shown = format!("{name} {value}");
    div()
        .w_full()
        .h(px(20.0))
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .debug_selector(move || shown)
        .child(muted(name, tk).flex_1())
        .child(
            div()
                .flex_none()
                .whitespace_nowrap()
                .text_size(px(tk.font_small()))
                .text_color(value_color)
                .child(value),
        )
}

/// The row at the top of a rectangular widget: an optional icon, the title, and what goes on the
/// right (`right`). With `grow_title` the title takes the room and pushes `right` to the edge;
/// otherwise one of `right` does that itself.
pub fn head(
    icon: Option<(&str, Hsla)>,
    title: &'static str,
    grow_title: bool,
    right: Vec<AnyElement>,
    tk: &Tk,
) -> Div {
    div()
        .w_full()
        .h(px(28.0))
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .when_some(icon, |d, (name, color)| {
            d.child(kit::icon(name, 18.0, color))
        })
        .child(
            div()
                .min_w_0()
                .when(grow_title, |d| d.flex_1())
                .text_size(px(tk.font_title()))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(tk.text())
                .truncate()
                .debug_selector(move || title.to_string())
                .child(title),
        )
        .children(right)
}

/// What a widget says when it has nothing to show, over the whole card.
pub fn empty(text: &'static str, tk: &Tk) -> AnyElement {
    div()
        .size_full()
        .p(px(tk.pad()))
        .flex()
        .items_center()
        .justify_center()
        .text_center()
        .text_size(px(tk.font_body()))
        .text_color(tk.faint())
        .debug_selector(move || text.to_string())
        .child(text)
        .into_any_element()
}

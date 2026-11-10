//! The sidebar: it slides in from the right with edit mode and shrinks the grid to make room. Its
//! tabs are Widgets (the grid's size, the palette to add from), Arrange (lay the dashboard out by
//! itself), Colors (the color schemes) and Style (the presets and every variable of the look).

use super::parts::{
    Look, RowOwner, RowSpec, choice_card, eid, section_label, segment, setting_row, text_btn,
};
use crate::dash::{BAR, Dash};
use crate::edit::anim::Ease;
use crate::edit::{DRAWER_W, DrawerTab, EDGE};
use crate::kit::AriaCursor;
use crate::kit::{self, Tk};
use crate::theme::Color;
use gpui::prelude::*;
use gpui::{
    AnyElement, BoxShadow, Context, Div, FontWeight, MouseButton, MouseDownEvent, Role,
    ScrollHandle, Stateful, div, point, px,
};

impl Dash {
    /// Where the drawer's left edge is, and how far out it is (0 to 1).
    pub fn drawer_x(&self, width: f32) -> (f32, f32) {
        let shift = self.edit.drawer_shift();
        (width + 40.0 - shift * (DRAWER_W + EDGE + 40.0), shift)
    }

    /// Scroll `list` to the row under the keyboard cursor when the cursor moved.
    fn reveal(&self, tab: Option<DrawerTab>, handle: &ScrollHandle, child: usize) {
        if self.cfg_open() {
            return;
        }
        let key = (
            tab,
            self.edit.side_cursor,
            self.edit.side_keys,
            self.edit.settings_id,
        );
        if self.edit.side_keys && self.edit.revealed.get() != Some(key) {
            handle.scroll_to_item(child);
        }
        self.edit.revealed.set(Some(key));
    }

    pub(crate) fn drawer(&self, tk: &Tk, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (x, shift) = self.drawer_x(self.size.0);
        if shift <= 0.001 {
            return None;
        }
        let y = BAR + 8.0;
        let tabs = div()
            .id("drawer-tabs")
            .flex_none()
            .h(px(38.0))
            .p(px(3.0))
            .gap(px(3.0))
            .flex()
            .flex_row()
            .rounded(px(19.0_f32.min(tk.radius_small() + 3.0)))
            .bg(tk.card_raised())
            .role(Role::TabList)
            .aria_label("Sidebar sections")
            .children(DrawerTab::ALL.iter().map(|&tab| {
                segment(
                    eid("tab", tab.index()),
                    tab.label(),
                    self.edit.tab == tab,
                    tk,
                    cx.listener(move |d, _, _, cx| {
                        d.set_tab(tab);
                        cx.notify();
                    }),
                )
            }));
        let list = match self.edit.tab {
            DrawerTab::Widgets => self.widgets_tab(tk, cx),
            DrawerTab::Arrange => self.arrange_tab(tk, cx),
            DrawerTab::Colors => self.colors_tab(tk, cx),
            DrawerTab::Style => self.style_tab(tk, cx),
        };
        Some(
            div()
                .id("drawer")
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(DRAWER_W))
                .h(px((self.size.1 - y - EDGE).max(0.0)))
                .p(px(14.0))
                .flex()
                .flex_col()
                .gap(px(8.0))
                .rounded(px(tk.radius()))
                .bg(tk.panel())
                .border_1()
                .border_color(tk.border())
                .shadow(vec![BoxShadow {
                    color: tk.shadow(),
                    offset: point(px(0.0), px(0.0)),
                    blur_radius: px(30.0),
                    spread_radius: px(0.0),
                    inset: false,
                }])
                .occlude()
                .child(tabs)
                .child(list)
                .into_any_element(),
        )
    }

    /// A scrolling list in the sidebar.
    fn side_list(&self, id: &'static str, handle: &ScrollHandle, gap: f32) -> Stateful<Div> {
        div()
            .id(id)
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(gap))
            .overflow_y_scroll()
            .track_scroll(handle)
    }

    // ---- Widgets ----------------------------------------------------------------------------

    fn widgets_tab(&self, tk: &Tk, cx: &mut Context<Self>) -> AnyElement {
        let kf = |i| self.side_on(DrawerTab::Widgets, i);
        let controls = self.grid_controls();
        let shakes = |i: usize| {
            self.edit.anim.follow(
                ("shake", i as u32),
                (self.edit.refusals[i] % 2) as f32,
                420.0,
                tk.motion(),
                Ease::Linear,
            )
        };
        let n = self.edit.palette.len();
        // Which child of the list holds a stop: the grid's steppers are in the first.
        let cursor = self.edit.side_cursor;
        let child = if cursor < 2 {
            0
        } else {
            (1 + cursor - 2).min(n)
        };
        self.reveal(Some(DrawerTab::Widgets), &self.edit.scroll.widgets, child);
        // How many cells across and down. Changing them fits the widgets in; the reset buttons go
        // back to as many cells as fit the screen.
        let head = div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .pb(px(4.0))
            .child(
                div()
                    .h(px(22.0))
                    .flex()
                    .flex_row()
                    .items_end()
                    .child(section_label("Grid", tk).flex_1())
                    .child(
                        section_label(
                            if self.layout.grid.is_some() {
                                ""
                            } else {
                                "fits the screen"
                            },
                            tk,
                        )
                        .font_weight(FontWeight::NORMAL),
                    ),
            )
            .children(controls.into_iter().enumerate().map(|(i, row)| {
                setting_row(
                    RowSpec {
                        owner: RowOwner::Grid,
                        pos: i,
                        row,
                        kfocus: kf(i),
                        shake: shakes(i),
                        live: true,
                        field: None,
                    },
                    tk,
                    cx,
                )
            }))
            .child(section_label("Add", tk).h(px(28.0)).flex().items_end());
        let entries: Vec<AnyElement> = self
            .edit
            .palette
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let kfocus = kf(2 + i);
                let hover = tk.hover();
                let title = p.title;
                div()
                    .id(eid("palette", i))
                    .flex_none()
                    .h(px(62.0))
                    .py(px(3.0))
                    .child(
                        div()
                            .id(eid("palette-entry", i))
                            .role(Role::Button)
                            .aria_label(title)
                            .aria_description(p.description.clone())
                            .aria_cursor(kfocus)
                            .debug_selector(move || title.to_string())
                            .size_full()
                            .p(px(10.0))
                            .gap(px(10.0))
                            .flex()
                            .flex_row()
                            .rounded(px(tk.radius_small()))
                            .bg(if kfocus { hover } else { tk.card_raised() })
                            .when(kfocus, |d| d.border_2().border_color(tk.accent()))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .active({
                                let pressed = tk.pressed();
                                move |s| s.bg(pressed)
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |d, ev: &MouseDownEvent, _, _| {
                                    d.palette_press(
                                        i,
                                        (f32::from(ev.position.x), f32::from(ev.position.y)),
                                    );
                                }),
                            )
                            .on_click(cx.listener(move |d, _, _, cx| {
                                if !d.edit.pal_press.dragged_out
                                    && let Some(item) = d.edit.palette.get(i)
                                {
                                    let kind = item.kind;
                                    d.add_widget(kind);
                                }
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(36.0))
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(18.0_f32.min(tk.radius_small())))
                                    .bg(tk.accent_soft())
                                    .child(kit::icon("plus", 18.0, tk.accent())),
                            )
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
                                            .child(p.title),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(tk.font_small()))
                                            .text_color(tk.dim())
                                            .child(p.description.clone()),
                                    ),
                            ),
                    )
                    .into_any_element()
            })
            .collect();
        self.side_list("side-widgets", &self.edit.scroll.widgets, 0.0)
            .child(head)
            .children(entries)
            .into_any_element()
    }

    // ---- Arrange ----------------------------------------------------------------------------

    fn arrange_tab(&self, tk: &Tk, cx: &mut Context<Self>) -> AnyElement {
        let kf = |i| self.side_on(DrawerTab::Arrange, i);
        let rows = self.arrange_rows();
        let n = rows.len();
        let cursor = self.edit.side_cursor;
        let child = match cursor {
            0 | 1 => 0,
            c if c < 2 + n => c,
            _ => 2 + n,
        };
        self.reveal(Some(DrawerTab::Arrange), &self.edit.scroll.arrange, child);
        let can_undo = self.arrange_can_undo();
        let top = div()
            .flex_none()
            .h(px(34.0))
            .flex()
            .flex_row()
            .gap(px(6.0))
            .child(
                text_btn(
                    "arrange-now",
                    "Arrange",
                    None,
                    Some("sparkle"),
                    true,
                    Look {
                        kfocus: kf(0),
                        ..Look::default()
                    },
                    tk,
                    cx.listener(|d, _, _, cx| {
                        d.arrange_now();
                        cx.notify();
                    }),
                )
                .flex_1()
                .h(px(34.0)),
            )
            .child(
                text_btn(
                    "arrange-shuffle",
                    "Shuffle",
                    None,
                    Some("refresh"),
                    false,
                    Look {
                        kfocus: kf(1),
                        ..Look::default()
                    },
                    tk,
                    cx.listener(|d, _, _, cx| {
                        d.arrange_shuffle();
                        cx.notify();
                    }),
                )
                .flex_1()
                .h(px(34.0)),
            );
        let result = div()
            .flex_none()
            .h(px(34.0))
            .text_size(px(tk.font_small()))
            .text_color(tk.dim())
            .debug_selector(|| "arrange result".into())
            .child(self.arrange_result());
        let bottom = div()
            .flex_none()
            .h(px(34.0))
            .flex()
            .flex_row()
            .gap(px(6.0))
            .child(
                text_btn(
                    "arrange-copy",
                    "Copy settings",
                    None,
                    Some("clipboard"),
                    false,
                    Look {
                        kfocus: kf(2 + n),
                        ..Look::default()
                    },
                    tk,
                    cx.listener(|d, _, _, cx| {
                        d.arrange_copy();
                        cx.notify();
                    }),
                )
                .flex_1()
                .h(px(34.0)),
            )
            .child(
                text_btn(
                    "arrange-undo",
                    "Undo",
                    None,
                    Some("reset"),
                    false,
                    Look {
                        enabled: can_undo,
                        kfocus: kf(3 + n),
                        ..Look::default()
                    },
                    tk,
                    cx.listener(|d, _, _, cx| {
                        d.arrange_undo();
                        cx.notify();
                    }),
                )
                .flex_1()
                .h(px(34.0)),
            );
        let rows: Vec<AnyElement> = rows
            .into_iter()
            .enumerate()
            .map(|(i, row)| {
                setting_row(
                    RowSpec {
                        owner: RowOwner::Arrange,
                        pos: i,
                        row,
                        kfocus: kf(2 + i),
                        shake: 0.0,
                        live: true,
                        field: None,
                    },
                    tk,
                    cx,
                )
            })
            .collect();
        self.side_list("side-arrange", &self.edit.scroll.arrange, 6.0)
            .child(top)
            .child(result)
            .children(rows)
            .child(bottom)
            .into_any_element()
    }

    // ---- Colors -----------------------------------------------------------------------------

    fn colors_tab(&self, tk: &Tk, cx: &mut Context<Self>) -> AnyElement {
        let n = self.edit.schemes.len();
        self.reveal(
            Some(DrawerTab::Colors),
            &self.edit.scroll.colors,
            self.edit.side_cursor.min(n),
        );
        let cards: Vec<AnyElement> = self
            .edit
            .schemes
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let name = s.name.clone();
                choice_card(
                    eid("scheme", i),
                    &s.name,
                    &s.description,
                    s.current,
                    self.side_on(DrawerTab::Colors, i),
                    56.0,
                    scheme_preview(s, tk),
                    tk,
                    cx.listener(move |d, _, _, cx| {
                        d.choose_colorscheme(&name);
                        cx.notify();
                    }),
                )
            })
            .collect();
        self.side_list("side-colors", &self.edit.scroll.colors, 6.0)
            .children(cards)
            .child(self.folder_button(DrawerTab::Colors, n, tk, cx))
            .into_any_element()
    }

    fn folder_button(
        &self,
        tab: DrawerTab,
        stop: usize,
        tk: &Tk,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .flex_none()
            .child(
                text_btn(
                    "open-config-folder",
                    "Open the config folder",
                    None,
                    None,
                    false,
                    Look {
                        kfocus: self.side_on(tab, stop),
                        ..Look::default()
                    },
                    tk,
                    cx.listener(|d, _, _, cx| {
                        d.open_themes_folder();
                        cx.notify();
                    }),
                )
                .w_full(),
            )
            .into_any_element()
    }

    // ---- Style ------------------------------------------------------------------------------

    fn style_tab(&self, tk: &Tk, cx: &mut Context<Self>) -> AnyElement {
        let kf = |i| self.side_on(DrawerTab::Style, i);
        let styles = self.edit.styles.len();
        let k = self.k_adjust();
        let rows = self.edit.style_rows.len();
        let cursor = self.edit.side_cursor;
        // Preset label, the presets, "Adjust", the rows, the folder button.
        let child = if cursor < styles {
            1 + cursor
        } else if cursor < k {
            styles + 1
        } else {
            styles + 2 + (cursor - k)
        };
        self.reveal(Some(DrawerTab::Style), &self.edit.scroll.style, child);
        let customized = self.edit.style_customized;
        let cards: Vec<AnyElement> = self
            .edit
            .styles
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let name = s.name.clone();
                let description = if s.current && customized {
                    "adjusted below".to_string()
                } else if !s.description.is_empty() {
                    s.description.clone()
                } else if s.motion == 0.0 {
                    "no animations".to_string()
                } else {
                    String::new()
                };
                choice_card(
                    eid("style", i),
                    &s.name,
                    &description,
                    s.current,
                    kf(i),
                    50.0,
                    style_preview(s, tk),
                    tk,
                    cx.listener(move |d, _, _, cx| {
                        d.choose_style(&name);
                        cx.notify();
                    }),
                )
            })
            .collect();
        let adjust = div()
            .flex_none()
            .h(px(34.0))
            .flex()
            .flex_row()
            .items_end()
            .child(section_label("Adjust", tk).flex_1())
            .when(customized, |d| {
                d.child(text_btn(
                    "style-reset-all",
                    "Reset all style adjustments",
                    Some("Reset all"),
                    Some("reset"),
                    false,
                    Look {
                        kfocus: kf(styles),
                        ..Look::default()
                    },
                    tk,
                    cx.listener(|d, _, _, cx| {
                        d.style_reset(None);
                        cx.notify();
                    }),
                ))
            });
        let style_rows: Vec<AnyElement> = self
            .edit
            .style_rows
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, v)| {
                let row = crate::edit::SettingRow {
                    index: i,
                    kind: v.kind,
                    label: v.label,
                    value: v.value,
                    on: v.on,
                    changed: v.overridden,
                    can_dec: v.can_dec,
                    can_inc: v.can_inc,
                    placeholder: String::new(),
                };
                setting_row(
                    RowSpec {
                        owner: RowOwner::Style,
                        pos: i,
                        row,
                        kfocus: kf(k + i),
                        shake: 0.0,
                        live: true,
                        field: None,
                    },
                    tk,
                    cx,
                )
            })
            .collect();
        self.side_list("side-style", &self.edit.scroll.style, 6.0)
            .child(
                section_label("Preset", tk)
                    .flex_none()
                    .h(px(22.0))
                    .flex()
                    .items_end(),
            )
            .children(cards)
            .child(adjust)
            .children(style_rows)
            .child(self.folder_button(DrawerTab::Style, k + rows, tk, cx))
            .into_any_element()
    }
}

/// The scheme in miniature: backdrop, a card, accent, text and status colors.
fn scheme_preview(s: &crate::edit::Scheme, tk: &Tk) -> impl IntoElement {
    let dot = |c: Color, i: usize| {
        div()
            .absolute()
            .left(px(5.0 + i as f32 * 11.0))
            .top(px(5.0))
            .size(px(8.0))
            .rounded_full()
            .bg(c.hsla())
    };
    div()
        .size_full()
        .rounded(px(8.0))
        .bg(s.backdrop.with_alpha(1.0).hsla())
        .border_1()
        .border_color(tk.border())
        .p(px(6.0))
        .child(
            div()
                .size_full()
                .relative()
                .rounded(px(5.0))
                .bg(s.card.with_alpha(1.0).hsla())
                .child(dot(s.accent, 0))
                .child(dot(s.good, 1))
                .child(dot(s.bad, 2))
                .child(
                    div()
                        .absolute()
                        .left(px(5.0))
                        .right(px(5.0))
                        .bottom(px(5.0))
                        .h(px(3.0))
                        .rounded(px(1.5))
                        .bg(s.text.hsla()),
                ),
        )
}

/// Two cards with the style's corner radius and gap, scaled down, drawn in the text color.
fn style_preview(s: &crate::edit::StyleCard, tk: &Tk) -> impl IntoElement {
    let total = 72.0;
    let w = (total - s.gap * 0.6) / 2.0;
    let radius = (w / 2.0).min(s.radius * 0.45);
    let text = tk.text();
    let card = move |i: usize| {
        div()
            .absolute()
            .top(px(0.0))
            .bottom(px(0.0))
            .left(px(i as f32 * (total - w)))
            .w(px(w))
            .rounded(px(radius))
            .bg(text.opacity(0.16))
            .border_1()
            .border_color(text.opacity(0.55))
    };
    div().relative().size_full().child(card(0)).child(card(1))
}

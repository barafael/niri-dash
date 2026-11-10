//! The color picker widget: the last picked color and its values to copy, the pipette, and the
//! recent colors (`ColorPickerWidget` and `CopyText` in `legacy/ui/widgets/tools.slint`, and
//! `legacy/src/app/color_picker.rs`).
//!
//! Picking a pixel goes through the compositor, and the dashboard covers the screen: so it hides
//! first, the pick starts once it is gone (`awake(false)`), and the color that comes back brings
//! the dashboard up again (`Event::Color(Picked)` answers with `Control::Show`).

use super::ui;
use super::{Plain, clipboard};
use crate::Control;
use crate::dash::{Dash, DashEvent};
use crate::keys::Key;
use crate::kit::{self, Tk};
use crate::layout::Kind;
use crate::scope::Stops;
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Hsla, Role, div, px};
use niri_dash_system::{Tell, color};

/// How many picked colors are kept.
const COLORS: usize = 8;

#[derive(Default)]
pub struct State {
    /// Waiting for the dashboard to get out of the way before picking a color.
    pending: bool,
    /// The dashboard was open when the pick started, so it comes back afterwards.
    reshow: bool,
}

/// A picked color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Picked(pub [u8; 3]);

impl Picked {
    /// "#rrggbb".
    pub fn hex(self) -> String {
        let [r, g, b] = self.0;
        format!("#{r:02x}{g:02x}{b:02x}")
    }

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#')?;
        if s.len() != 6 || !s.is_ascii() {
            return None;
        }
        let b = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
        Some(Self([b(0)?, b(2)?, b(4)?]))
    }

    pub fn rgb(self) -> String {
        let [r, g, b] = self.0;
        format!("rgb({r}, {g}, {b})")
    }

    /// Hue (degrees), saturation and lightness (percent).
    pub fn hsl_parts(self) -> (i32, i32, i32) {
        let [r, g, b] = self.0.map(|v| v as f64 / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;
        let d = max - min;
        if d == 0.0 {
            return (0, 0, (l * 100.0).round() as i32);
        }
        let s = d / (1.0 - (2.0 * l - 1.0).abs());
        let h = if max == r {
            60.0 * ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        (
            h.round() as i32 % 360,
            (s * 100.0).round() as i32,
            (l * 100.0).round() as i32,
        )
    }

    pub fn hsl(self) -> String {
        let (h, s, l) = self.hsl_parts();
        format!("hsl({h}, {s}%, {l}%)")
    }

    pub fn color(self) -> Hsla {
        let [r, g, b] = self.0;
        gpui::rgb(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)).into()
    }

    /// HSV's value: how bright the brightest channel is, 0 to 1.
    fn value(self) -> f32 {
        f32::from(*self.0.iter().max().unwrap_or(&0)) / 255.0
    }
}

fn to_u8(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The recent colors, newest first.
fn colors(d: &Dash) -> Vec<Picked> {
    d.state
        .colors
        .iter()
        .filter_map(|h| Picked::parse(h))
        .collect()
}

/// What the widget has room for, in a card of `w` by `h` px with `n` recent colors.
struct Shape {
    round: bool,
    /// Wide enough for rgb() and hsl() beside the swatch.
    details: bool,
    /// Tall enough for the history row.
    history: bool,
    /// Keyboard stops: the values to copy, the pipette, then the history swatches that fit.
    texts: usize,
    swatches: usize,
}

impl Shape {
    fn of(w: f32, h: f32, round: bool, n: usize, pad: f32) -> Self {
        let has = n > 0;
        let details = !round && w >= 380.0;
        let history = !round && h > 150.0 && n > 1;
        let texts = if round || !has {
            0
        } else if details {
            3
        } else {
            1
        };
        let swatches = if history {
            let fit = ((w - 2.0 * pad - 28.0) / 34.0).floor().max(0.0) as usize + 1;
            (n - 1).min(fit)
        } else {
            0
        };
        Self {
            round,
            details,
            history,
            texts,
            swatches,
        }
    }

    fn pipette(&self) -> usize {
        self.texts
    }

    fn count(&self) -> usize {
        if self.round {
            0
        } else {
            self.texts + 1 + self.swatches
        }
    }
}

fn shape_of(d: &Dash, id: u32) -> Shape {
    let inst = d.widgets.tools.insts.get(id);
    Shape::of(
        inst.w,
        inst.h,
        inst.round,
        colors(d).len(),
        d.look.style.padding,
    )
}

// ---- picking ------------------------------------------------------------------------------

/// The pipette was pressed: the dashboard gets out of the way first (a standalone window doesn't
/// cover the screen, and picks right away).
fn start_pick(d: &mut Dash, cx: &mut Context<Dash>) {
    if d.solo.is_some() {
        run_pick(d);
        return;
    }
    let c = &mut d.widgets.tools.color;
    c.reshow = true;
    c.pending = true;
    cx.emit(DashEvent::Hide);
}

fn run_pick(d: &Dash) {
    d.tx.color.tell(color::Command::Pick);
}

/// The dashboard came up or went away; once it is out of the way, a pick that waits for that
/// starts.
pub fn awake(d: &mut Dash, on: bool) {
    if !on && std::mem::take(&mut d.widgets.tools.color.pending) {
        run_pick(d);
    }
}

/// The compositor answers with the color picked (`None`: cancelled or failed): keep it, copy it,
/// and bring the dashboard back if it was in the way.
pub fn picked(d: &mut Dash, rgb: Option<[f64; 3]>) -> Option<Control> {
    if let Some(rgb) = rgb {
        let hex = Picked(rgb.map(to_u8)).hex();
        d.state.colors.retain(|c| *c != hex);
        d.state.colors.insert(0, hex.clone());
        d.state.colors.truncate(COLORS);
        crate::state::save(&d.state);
        resync(d);
        clipboard::copy_text(d, &hex);
    }
    std::mem::take(&mut d.widgets.tools.color.reshow).then_some(Control::Show)
}

/// The colors changed: every widget's cursor stays on a stop that exists.
fn resync(d: &mut Dash) {
    for p in super::placed_of(d, Kind::ColorPicker) {
        let mut inst = d.widgets.tools.insts.get(p.id);
        let count = shape_of(d, p.id).count();
        inst.scope.set_count(count, &Plain);
        d.widgets.tools.insts.set(p.id, inst);
    }
}

// ---- keys -----------------------------------------------------------------------------------

struct Keys<'a, 'b> {
    d: &'a mut Dash,
    cx: &'a mut Context<'b, Dash>,
    shape: Shape,
    colors: Vec<Picked>,
}

impl Stops for Keys<'_, '_> {
    fn activate(&mut self, cursor: usize, _sub: usize) {
        let pipette = self.shape.pipette();
        if self.shape.round || cursor == pipette {
            start_pick(self.d, self.cx);
        } else if cursor < pipette {
            if let Some(last) = self.colors.first() {
                let text = match cursor {
                    0 => last.hex(),
                    1 => last.rgb(),
                    _ => last.hsl(),
                };
                clipboard::copy_text(self.d, &text);
            }
        } else if let Some(c) = self.colors.get(cursor - pipette) {
            clipboard::copy_text(self.d, &c.hex());
        }
    }

    fn letter(&mut self, key: &Key, _cursor: usize) -> bool {
        if key.is('p') {
            start_pick(self.d, self.cx);
            return true;
        }
        false
    }
}

/// j/k over the values, the pipette and the history; Enter copies (or picks); p picks.
pub fn key(d: &mut Dash, id: u32, key: &Key, cx: &mut Context<Dash>) -> bool {
    let mut inst = d.widgets.tools.insts.get(id);
    let colors = colors(d);
    let shape = Shape::of(
        inst.w,
        inst.h,
        inst.round,
        colors.len(),
        d.look.style.padding,
    );
    let mut scope = inst.scope;
    scope.set_count(shape.count(), &Plain);
    let taken = scope.press(
        key,
        &mut Keys {
            d,
            cx,
            shape,
            colors,
        },
    );
    inst.scope = scope;
    d.widgets.tools.insts.set(id, inst);
    taken
}

pub fn describe(d: &Dash, id: u32) -> Option<String> {
    let shape = shape_of(d, id);
    let cursor = d.widgets.tools.insts.get(id).scope.cursor;
    let pipette = shape.pipette();
    Some(match cursor {
        _ if shape.round => "pipette".to_string(),
        c if c < pipette => format!("value {}", c + 1),
        c if c == pipette => "pipette".to_string(),
        c => format!("recent color {}", c - pipette),
    })
}

pub fn check(d: &Dash) -> Result<(), String> {
    for p in super::placed_of(d, Kind::ColorPicker) {
        let count = shape_of(d, p.id).count();
        super::cursor_ok("color picker", d, p.id, count, 1)?;
    }
    if d.state.colors.len() > COLORS {
        return Err(format!("{} picked colors kept", d.state.colors.len()));
    }
    Ok(())
}

// ---- drawing --------------------------------------------------------------------------------

/// A color value that copies itself when clicked.
fn copy_text(
    id: impl Into<gpui::ElementId>,
    text: String,
    size: f32,
    tint: Hsla,
    kfocus: bool,
    tk: &Tk,
    cx: &mut Context<Dash>,
) -> AnyElement {
    let (hover, pressed) = (tk.hover(), tk.pressed());
    let label = format!("Copy {text}");
    let name = label.clone();
    let copied = text.clone();
    let radius = 6.0_f32.min(tk.radius_small());
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .aria_selected(kfocus)
        .debug_selector(move || name)
        .relative()
        .w_full()
        .h(px(24.0))
        .flex_none()
        .px(px(6.0))
        .flex()
        .items_center()
        .rounded(px(radius))
        .when(kfocus, |d| d.bg(hover).child(ui::ring(tk, radius)))
        .cursor_pointer()
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .hover(move |s| s.bg(hover))
        .active(move |s| s.bg(pressed))
        .on_click(cx.listener(move |d, _, _, cx| {
            clipboard::copy_text(d, &copied);
            cx.notify();
        }))
        .text_size(px(size))
        .text_color(tint)
        .child(div().truncate().child(text))
        .into_any_element()
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let d = ctx.d;
    let tk = ctx.tk;
    let id = ctx.tile.id();
    let list = colors(d);
    let shape = Shape::of(ctx.w, ctx.h, ctx.round, list.len(), tk.pad());
    d.widgets
        .tools
        .insts
        .drawn(id, ctx.w, ctx.h, ctx.round, shape.count(), 1);
    let cursor = d.widgets.tools.insts.get(id).scope.cursor;
    let kfocus = |stop: usize| ctx.entered && cursor == stop;
    let last = list.first().copied();
    let swatch = last.map_or(tk.card_raised(), Picked::color);

    if shape.round {
        let dia = ctx.w * 0.52;
        let tint = if last.is_some_and(|c| c.value() > 0.6) {
            Hsla {
                h: 0.0,
                s: 0.0,
                l: 0.0,
                a: 0xb0 as f32 / 255.0,
            }
        } else {
            Hsla {
                h: 0.0,
                s: 0.0,
                l: 1.0,
                a: 0xd0 as f32 / 255.0,
            }
        };
        let caption = last.map_or("Pick".to_string(), Picked::hex);
        return div()
            .id(("color-picker", id))
            .role(Role::Button)
            .aria_label("Pick a color")
            .debug_selector(|| "Pick a color".to_string())
            .size_full()
            .relative()
            .cursor_pointer()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|d, _, _, cx| {
                start_pick(d, cx);
                cx.notify();
            }))
            .child(
                div()
                    .absolute()
                    .left(px((ctx.w - dia) / 2.0))
                    .top(px(ctx.h * 0.42 - dia / 2.0))
                    .size(px(dia))
                    .rounded_full()
                    .bg(swatch)
                    .border(px(2.0))
                    .border_color(tk.border())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(kit::icon("pipette", dia * 0.36, tint)),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .w_full()
                    .top(px(ctx.h * 0.8 - tk.font_small() * ui::LINE / 2.0))
                    .flex()
                    .justify_center()
                    .text_size(px(tk.font_small()))
                    .text_color(tk.dim())
                    .child(caption),
            )
            .into_any_element();
    }

    let box_size = 56.0_f32.min(ctx.h - 2.0 * tk.pad()).max(0.0);
    let mut values = div().flex_1().min_w_0().flex().flex_col().justify_center();
    match last {
        Some(c) => {
            values = values.child(copy_text(
                "copy-hex",
                c.hex(),
                tk.font_title(),
                tk.text(),
                kfocus(0),
                tk,
                cx,
            ));
            if shape.details {
                values = values
                    .child(copy_text(
                        "copy-rgb",
                        c.rgb(),
                        tk.font_small(),
                        tk.dim(),
                        kfocus(1),
                        tk,
                        cx,
                    ))
                    .child(copy_text(
                        "copy-hsl",
                        c.hsl(),
                        tk.font_small(),
                        tk.dim(),
                        kfocus(2),
                        tk,
                        cx,
                    ));
            }
        }
        None => {
            values = values.child(
                kit::muted("Click the pipette, then a pixel", tk)
                    .text_size(px(tk.font_small()))
                    .truncate(),
            );
        }
    }
    let top = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .child(
            div()
                .flex_none()
                .size(px(box_size))
                .rounded(px(tk.radius_small()))
                .bg(swatch)
                .border(px(1.0))
                .border_color(tk.border()),
        )
        .child(values)
        .child(ui::icon_button(
            ("color-picker-pipette", id),
            "pipette",
            "Pick a color",
            36.0,
            tk.text(),
            Some(tk.card_raised()),
            kfocus(shape.pipette()),
            tk,
            cx.listener(|d, _, _, cx| {
                start_pick(d, cx);
                cx.notify();
            }),
        ));
    let pipette = shape.pipette();
    let history = shape.history.then(|| {
        let avail = ctx.w - 2.0 * tk.pad();
        div()
            .flex()
            .flex_row()
            .gap(px(6.0))
            .justify_start()
            .children(
                list.iter()
                    .enumerate()
                    .skip(1)
                    .filter(|(i, _)| (*i as f32 - 1.0) * 34.0 + 28.0 <= avail)
                    .map(|(i, c)| {
                        let hex = c.hex();
                        let focus = kfocus(pipette + i);
                        let label = format!("Copy {hex}");
                        let name = label.clone();
                        let (text, border) = (tk.text(), tk.border());
                        div()
                            .id(("swatch", i))
                            .role(Role::Button)
                            .aria_label(label)
                            .aria_selected(focus)
                            .debug_selector(move || name)
                            .flex_none()
                            .size(px(28.0))
                            .rounded(px(14.0_f32.min(tk.radius_small())))
                            .bg(c.color())
                            .border(px(if focus { 2.0 } else { 1.0 }))
                            .border_color(if focus { tk.accent() } else { border })
                            .cursor_pointer()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .hover(move |s| s.border_2().border_color(text))
                            .on_click(cx.listener(move |d, _, _, cx| {
                                clipboard::copy_text(d, &hex);
                                cx.notify();
                            }))
                    }),
            )
    });
    div()
        .id(("color-picker", id))
        .role(Role::List)
        .aria_label("Color picker")
        .size_full()
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(10.0))
        .p(px(tk.pad()))
        .child(top)
        .children(history)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_formats() {
        assert_eq!(Picked([1.0, 0.5, 0.0].map(to_u8)).hex(), "#ff8000");
        assert_eq!(Picked::parse("#ff8000"), Some(Picked([255, 128, 0])));
        assert_eq!(Picked::parse("ff8000"), None);
        assert_eq!(Picked::parse("#ff80"), None);
        assert_eq!(Picked([255, 0, 0]).hsl_parts(), (0, 100, 50));
        assert_eq!(Picked([0, 0, 255]).hsl_parts(), (240, 100, 50));
        assert_eq!(Picked([128, 128, 128]).hsl_parts(), (0, 0, 50));
        assert_eq!(Picked([255, 128, 0]).rgb(), "rgb(255, 128, 0)");
    }

    #[test]
    fn what_fits() {
        // A wide card shows the values; a tall one the history, as many swatches as fit.
        let s = Shape::of(400.0, 200.0, false, 8, 16.0);
        assert_eq!((s.texts, s.pipette(), s.swatches), (3, 3, 7));
        let s = Shape::of(200.0, 200.0, false, 8, 16.0);
        assert_eq!((s.texts, s.swatches), (1, 5));
        let s = Shape::of(200.0, 100.0, false, 8, 16.0);
        assert_eq!((s.texts, s.swatches, s.count()), (1, 0, 2));
        let s = Shape::of(200.0, 200.0, true, 8, 16.0);
        assert_eq!(s.count(), 0);
        let s = Shape::of(200.0, 100.0, false, 0, 16.0);
        assert_eq!((s.texts, s.count()), (0, 1));
    }
}

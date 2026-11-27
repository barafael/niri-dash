//! The widgets. Each kind has a `build` function that draws it and, while it does, registers what
//! the keyboard can do in it: its *stops* (the rows and buttons `j` `k` walk through, which Enter
//! presses and `h` `l` adjust) and its *binds* (letters of its own, like `d` to dismiss). The
//! mouse and the keyboard use the same registrations, so a widget can't do something with one
//! that it can't do with the other: the key handler runs `build` again and acts on what it got.

use crate::dash::Dash;
use crate::layout::{Kind, Placed, Shape};
use crate::widget_settings::{self as ws, Opt};
use blenderui::prelude::*;
use std::rc::Rc;

mod controls;
mod desk;
mod devices;
mod extras;
mod monitors;
mod net;
mod state;
mod time;

pub use controls::copy_text;
pub use desk::{menu, menu_key, toggle_dnd};
pub use state::State;

/// Something a stop or a key does to the dashboard.
pub type Act = Rc<dyn Fn(&mut Dash, &mut Context<Dash>)>;
/// What a slider stop does with a new value (0..1).
pub type Set = Rc<dyn Fn(&mut Dash, f32, &mut Context<Dash>)>;

/// A place the keyboard's cursor can be in a widget.
#[derive(Clone)]
pub struct Stop {
    /// Enter and Space; None for a slider, or a row only to be read.
    pub act: Option<Act>,
    /// `h` `l` (and the digits) change it.
    pub slide: Option<(f32, Set)>,
    /// What the quick tips call pressing it.
    pub does: &'static str,
    /// What `y` (and Ctrl+C) copies, where there is something.
    pub copy: Option<String>,
}

/// A letter of the widget's own.
#[derive(Clone)]
pub struct Bind {
    pub key: &'static str,
    pub does: String,
    pub act: Act,
}

/// What a widget is drawn with, and what it registers.
pub struct Ctx<'a> {
    pub d: &'a Dash,
    pub p: &'a Placed,
    /// The size of the widget's body, in logical pixels.
    pub w: f32,
    pub h: f32,
    pub round: bool,
    /// The keyboard's cursor, when the widget is entered.
    pub cursor: Option<usize>,
    pub hd: WeakEntity<Dash>,
    pub stops: Vec<Stop>,
    pub binds: Vec<Bind>,
    /// Blown up or standalone ("x"), or in its place on the grid ("s"): the same widget can be
    /// drawn twice at once, and its elements' ids must differ.
    prefix: &'static str,
}

/// An element listener that does `act` to the dashboard.
pub fn run<E: ?Sized + 'static>(
    hd: WeakEntity<Dash>,
    act: Act,
) -> impl Fn(&E, &mut Window, &mut App) + 'static {
    move |_: &E, _: &mut Window, app: &mut App| {
        hd.update(app, |d, cx| {
            act(d, cx);
            d.changed(cx);
        });
    }
}

/// An [`Act`] from a closure.
pub fn act(f: impl Fn(&mut Dash, &mut Context<Dash>) + 'static) -> Act {
    Rc::new(f)
}

impl<'a> Ctx<'a> {
    pub fn new(
        d: &'a Dash,
        p: &'a Placed,
        w: f32,
        h: f32,
        cursor: Option<usize>,
        big: bool,
        hd: WeakEntity<Dash>,
    ) -> Self {
        Self {
            d,
            p,
            w,
            h,
            // Blown up with `e`, or a standalone app: a round widget opens into its full form.
            round: p.shape == Shape::Round && !big,
            cursor,
            hd,
            stops: Vec::new(),
            binds: Vec::new(),
            prefix: if big { "x" } else { "s" },
        }
    }

    pub fn entered(&self) -> bool {
        self.cursor.is_some()
    }

    /// The id of a stop's element, for scrolling to it and typing into it.
    pub fn stop_id(&self, i: usize) -> String {
        stop_id(self.prefix, self.p.id, i)
    }

    /// The last stop registered: its id, and the cursor's frame if it is there.
    fn finish(&self, el: Div, here: bool) -> Div {
        let el = el.id(self.stop_id(self.stops.len() - 1));
        if here { cursor(el) } else { el }
    }

    /// What `y` copies on the stop registered last (a row that also does something).
    pub fn copies(&mut self, text: impl Into<String>) {
        if let Some(s) = self.stops.last_mut() {
            s.copy = Some(text.into());
        }
    }

    /// A row only to be read, whose `text` `y` copies.
    pub fn read_copy(&mut self, el: Div, text: impl Into<String>) -> Div {
        let el = self.read(el);
        if let Some(s) = self.stops.last_mut() {
            s.copy = Some(text.into());
        }
        el
    }

    /// A list that scrolls, keeping its place from one frame to the next.
    pub fn scroll(&self) -> Div {
        div()
            .class("col scroll")
            .id(format!("{}list{}", self.prefix, self.p.id))
    }

    /// Whether the cursor is on the next stop to be registered.
    pub fn here(&self) -> bool {
        self.cursor == Some(self.stops.len())
    }

    /// `el`, pressed by a click and by Enter: the next stop.
    pub fn press(
        &mut self,
        el: Div,
        does: &'static str,
        f: impl Fn(&mut Dash, &mut Context<Dash>) + 'static,
    ) -> Div {
        let here = self.here();
        let a: Act = Rc::new(f);
        self.stops.push(Stop {
            act: Some(a.clone()),
            slide: None,
            does,
            copy: None,
        });
        // An icon or a button says nothing a screen reader can read out: it is named after what
        // it does (a button with words gets those from `button`).
        let el = match el.tag() {
            "img" | "button" if !does.is_empty() => {
                el.attr("aria-label", capital(does)).attr("role", "button")
            }
            // A row: a button named by what it says.
            "div" => el.attr("role", "button"),
            _ => el,
        };
        let el = el.on_click(run::<ClickEvent>(self.hd, a));
        self.finish(el, here)
    }

    /// A Blender button with `label`, as a stop.
    pub fn button(
        &mut self,
        label: impl Into<SharedString>,
        does: &'static str,
        f: impl Fn(&mut Dash, &mut Context<Dash>) + 'static,
    ) -> Div {
        let label: SharedString = label.into();
        let el = self.press(Button().child(label.to_string()), does, f);
        // Words name it; a symbol (‹ ›, −1) doesn't, what it does does.
        let name = if label.chars().filter(|c| c.is_alphabetic()).count() > 1 {
            label.to_string()
        } else {
            capital(does)
        };
        el.attr("aria-label", name)
    }

    /// A row that is only to be read, still a place for the cursor (to scroll to, to copy).
    pub fn read(&mut self, el: Div) -> Div {
        let here = self.here();
        self.stops.push(Stop {
            act: None,
            slide: None,
            does: "",
            copy: None,
        });
        self.finish(el, here)
    }

    /// A text field: Enter (and `i`) type into it, Escape gets out again.
    pub fn field(&mut self, el: Div) -> Div {
        let here = self.here();
        let id = self.stop_id(self.stops.len());
        let focus = id.clone();
        if here {
            let f = focus.clone();
            self.bind("i", "type", move |d, _| d.type_into(f.clone()));
        }
        self.stops.push(Stop {
            act: Some(Rc::new(move |d: &mut Dash, _: &mut Context<Dash>| {
                d.type_into(focus.clone())
            })),
            slide: None,
            does: "type",
            copy: None,
        });
        self.finish(el, here)
    }

    /// Blender's number slider over 0..100 %, as a stop that `h` `l` and the digits change.
    pub fn slider(
        &mut self,
        label: &str,
        key: impl Into<String>,
        value: f32,
        f: impl Fn(&mut Dash, f32, &mut Context<Dash>) + 'static,
    ) -> Div {
        let here = self.here();
        let key: String = key.into();
        let value = self.d.held(&key, value);
        let set: Set = Rc::new(move |d: &mut Dash, v: f32, cx: &mut Context<Dash>| {
            let v = v.clamp(0.0, 1.0);
            d.hold(&key, v);
            f(d, v, cx);
        });
        self.stops.push(Stop {
            act: None,
            slide: Some((value, set.clone())),
            does: "",
            copy: None,
        });
        let hd = self.hd;
        let el = NumberSlider()
            .min(0.0)
            .max(100.0)
            .step(1.0)
            .precision(0)
            .attr("aria-label", label.to_string())
            .value(format!("{:.0}", value * 100.0))
            .class("grow")
            .on_change(move |e: &ChangeEvent, _: &mut Window, app: &mut App| {
                let v = (e.number / 100.0) as f32;
                let set = set.clone();
                hd.update(app, |d, cx| {
                    set(d, v, cx);
                    d.changed(cx);
                });
            });
        self.finish(el, here)
    }

    /// A round control that turns: `j` `k`, `h` `l`, the digits and the scroll wheel change it,
    /// Enter and a click do `f`.
    pub fn knob(
        &mut self,
        label: &str,
        el: Div,
        value: f32,
        set: impl Fn(&mut Dash, f32, &mut Context<Dash>) + 'static,
        does: &'static str,
        f: impl Fn(&mut Dash, &mut Context<Dash>) + 'static,
    ) -> Div {
        let here = self.here();
        // What the user turned it to is kept under its name.
        let key = label.to_lowercase();
        let value = self.d.held(&key, value);
        let held = key.clone();
        let set: Set = Rc::new(move |d: &mut Dash, v: f32, cx: &mut Context<Dash>| {
            let v = v.clamp(0.0, 1.0);
            d.hold(&held, v);
            set(d, v, cx);
        });
        let a: Act = Rc::new(f);
        self.stops.push(Stop {
            act: Some(a.clone()),
            slide: Some((value, set.clone())),
            does,
            copy: None,
        });
        let hd = self.hd;
        let el = el
            .attr("role", "slider")
            .attr("aria-label", label.to_string())
            .attr("aria-valuenow", format!("{:.0}", value * 100.0))
            .attr("aria-valuemin", "0")
            .attr("aria-valuemax", "100");
        let el = el.on_click(run::<ClickEvent>(hd, a)).on_scroll_wheel(
            move |e: &ScrollWheelEvent, _: &mut Window, app: &mut App| {
                let step = if e.delta > 0.0 {
                    0.05
                } else if e.delta < 0.0 {
                    -0.05
                } else {
                    return;
                };
                let set = set.clone();
                hd.update(app, |d, cx| {
                    set(d, d.held(&key, value) + step, cx);
                    d.changed(cx);
                });
                app.stop_propagation();
            },
        );
        self.finish(el, here)
    }

    /// A letter of the widget's own, while it is entered.
    pub fn bind(
        &mut self,
        key: &'static str,
        does: impl Into<String>,
        f: impl Fn(&mut Dash, &mut Context<Dash>) + 'static,
    ) {
        if self.entered() {
            self.binds.push(Bind {
                key,
                does: does.into(),
                act: Rc::new(f),
            });
        }
    }

    /// A setting of this widget.
    pub fn opt_bool(&self, opt: Opt) -> bool {
        ws::get_bool(&self.p.settings, opt, self.d.locale.us)
    }
    pub fn opt_int(&self, opt: Opt) -> i64 {
        ws::get_int(&self.p.settings, opt, self.d.locale.us)
    }
    pub fn opt_str(&self, opt: Opt) -> String {
        ws::get_str(&self.p.settings, opt, self.d.locale.us)
    }

    /// The icon `name` in the color of `role`.
    pub fn icon(&self, name: &str, role: &str) -> Div {
        match crate::icons::icon(name, self.d.look.color(role)) {
            // Decorative unless given a name (`alt`): what it stands for is said next to it.
            Some(pic) => img().class("icon").attr("alt", "").image(pic),
            None => div().class("icon"),
        }
    }

    /// A large icon.
    pub fn icon_l(&self, name: &str, role: &str) -> Div {
        self.icon(name, role).class("l")
    }
}

/// "mute it" → "Mute it".
fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// The id of stop `i` of widget `id`, drawn blown up ("x") or in place ("s").
pub fn stop_id(prefix: &str, id: u32, i: usize) -> String {
    format!("{prefix}{id}-{i}")
}

/// The keyboard's cursor on `el`. Blender's own widgets draw themselves and ignore an outline, so
/// they get a frame around them instead.
fn cursor(el: Div) -> Div {
    match el.tag() {
        "button" | "input" | "select" | "textarea" | "progress" => div().class("ring").child(el),
        _ => el.class("cursor"),
    }
}

/// A level from 0 to 1, as a bar; `tone` is "", "good", "warn" or "bad".
pub fn meter(v: f32, tone: &str) -> Div {
    div().class(format!("meter {tone}")).child(
        div()
            .class("fill")
            .style(format!("width: {:.1}%", v.clamp(0.0, 1.0) * 100.0)),
    )
}

/// The tone a level that is bad when high deserves.
pub fn load_tone(v: f32) -> &'static str {
    if v > 0.9 {
        "bad"
    } else if v > 0.7 {
        "warn"
    } else {
        ""
    }
}

/// A line of text that doesn't wrap.
pub fn line(text: impl Into<String>) -> Div {
    div().class("ellipsis").child(text.into())
}

/// The line a widget shows when there is nothing to show.
pub fn empty(text: impl Into<String>) -> Div {
    div().class("empty").child(text.into())
}

/// A chart of values (0..1), oldest first, filled under the line.
pub fn chart(values: &[f32], color: &str) -> Div {
    let values = values.to_vec();
    let rgb = hex(color);
    canvas(move |b, p| {
        if values.len() < 2 {
            return;
        }
        let (x0, y0, w, h) = (b.origin.x, b.origin.y, b.size.width, b.size.height);
        let step = w / (values.len() - 1) as f32;
        let pts: Vec<(f32, f32)> = values
            .iter()
            .enumerate()
            .map(|(i, v)| {
                (
                    x0 + i as f32 * step,
                    y0 + h - v.clamp(0.0, 1.0) * (h - 2.0) - 1.0,
                )
            })
            .collect();
        let mut poly = pts.clone();
        poly.push((x0 + w, y0 + h));
        poly.push((x0, y0 + h));
        p.polygon(&poly, rgba((rgb << 8) | 0x40));
        p.line(&pts, rgb_color(rgb), 1.5);
    })
    .class("grow")
    .style("min-height: 24px")
}

/// A ring from 0 to 1, as Blender draws a pie, with `text` in the middle.
pub fn ring(v: f32, color: &str, track: &str, card: &str, text: Div) -> Div {
    let (c, t, k) = (hex(color), hex(track), hex(card));
    div()
        .style("position: relative; flex: 1; align-self: stretch; min-height: 0; min-width: 0; display: flex; flex-direction: column")
        .child(
            canvas(move |b, p| {
                let s = b.size.width.min(b.size.height) - 4.0;
                let o = point(b.origin.x + (b.size.width - s) / 2.0, b.origin.y + (b.size.height - s) / 2.0);
                let full = Bounds { origin: o, size: size(s, s) };
                p.oval(full, rgb_color(t), rgb_color(t), 0.0);
                p.arc(full, 90.0, -360.0 * v.clamp(0.0, 1.0), rgb_color(c));
                let inset = s * 0.14;
                let inner = Bounds { origin: point(o.x + inset, o.y + inset), size: size(s - 2.0 * inset, s - 2.0 * inset) };
                p.oval(inner, rgb_color(k), rgb_color(k), 0.0);
            })
            // A flex child: placed absolutely, a canvas is handed the wrong bounds.
            .style("flex: 1; min-height: 0"),
        )
        .child(
            div()
                .style("position: absolute; left: 0; top: 0; right: 0; bottom: 0; display: flex; align-items: center; justify-content: center")
                .child(text),
        )
}

/// "#rrggbb…" as 0xrrggbb.
pub fn hex(c: &str) -> u32 {
    u32::from_str_radix(c.get(1..7).unwrap_or("ff00ff"), 16).unwrap_or(0xff00ff)
}

fn rgb_color(c: u32) -> Rgba {
    rgb(c)
}

/// Draw the widget `p` into `c`. Every kind has one.
pub fn build(c: &mut Ctx) -> Div {
    use Kind::*;
    match c.p.kind {
        Clock => time::clock(c),
        Calendar => time::calendar(c),
        Timer => time::timer(c),
        Stopwatch => time::stopwatch(c),
        Pomodoro => time::pomodoro(c),
        SleepTimer => time::sleep(c),
        SystemMonitor => monitors::system(c),
        Memory => monitors::memory(c),
        Disks => monitors::disks(c),
        Network => monitors::network(c),
        Gpu => monitors::gpu(c),
        ClaudeUsage => monitors::claude_usage(c),
        Battery => controls::battery(c),
        Volume => controls::volume(c),
        Brightness => controls::brightness(c),
        NightLight => controls::night_light(c),
        Session => controls::session(c),
        KeyboardLayout => controls::keyboard(c),
        ColorPicker => controls::color_picker(c),
        Wifi => net::wifi(c),
        Bluetooth => net::bluetooth(c),
        Windows => desk::windows(c),
        ClaudeSessions => desk::claude(c),
        Notifications => desk::notifications(c),
        Tray => desk::tray(c),
        Media => desk::media(c),
        Clipboard => desk::clipboard(c),
        Notes => desk::notes(c),
        Privacy => devices::privacy(c),
        RemovableDrives => devices::drives(c),
        Phone => devices::phone(c),
        Weather => extras::weather(c),
        Mascot => extras::mascot(c),
        About => extras::about(c),
        NiriDashInfo => extras::info(c),
    }
}

/// What the widget's header says next to its name: a count, a state.
pub fn status(d: &Dash, p: &Placed) -> String {
    use Kind::*;
    let data = &d.data;
    match p.kind {
        Windows => format!("{}", data.windows.count),
        Notifications if !data.notifications.items.is_empty() => {
            format!("{}", data.notifications.items.len())
        }
        ClaudeSessions if data.claude.busy > 0 => format!("{} busy", data.claude.busy),
        Wifi => data
            .wifi
            .as_ref()
            .map(|w| niri_dash_system::clean_label(&w.ssid))
            .unwrap_or_default(),
        Battery => data
            .power
            .as_ref()
            .filter(|p| p.has_battery)
            .map(|p| format!("{}%", p.percent))
            .unwrap_or_default(),
        Clipboard if !data.clips.is_empty() => format!("{}", data.clips.len()),
        _ => String::new(),
    }
}

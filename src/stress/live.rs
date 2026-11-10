//! `niri-dash stress` on screen: the same random keys as the headless run, pressed into a real
//! window of the dashboard at a pace you can watch, on the mock system. A second window has a
//! pause button and the pace panel: rate, how many keys, and a ramp up and down.
//!
//! The dashboard is in an ordinary window here, which stays when the dashboard hides itself (the
//! monkey opens it again, as the hotkey would). Closing a window ends the run.

use super::{Failure, Monkey, Rig};
use crate::Control;
use crate::dash::{Dash, DashEvent};
use crate::keys::Key;
use crate::layout::Layout;
use crate::window::{self, Error, Result};
use gpui::{
    AnyWindowHandle, App, AppContext as _, AsyncApp, ClickEvent, Context, Entity, InputEvent,
    IntoElement, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    OperatingSystem, ParentElement, Pixels, Point, Render, ScrollDelta, ScrollEvent, SharedString,
    Styled, TitlebarOptions, TouchPhase, Window, WindowBounds, WindowOptions, div, point,
    prelude::*, px, rgb, size,
};
use niri_dash_system::Event;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

// ---- the pace --------------------------------------------------------------------------------

/// What the pace panel sets: all of it can change while the run goes.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Tuning {
    paused: bool,
    /// Keys a second; 0 for as fast as it goes.
    rate: f64,
    /// Stop after this many, or never.
    total: Option<u64>,
    /// Seconds from a crawl up to `rate` at the start, and back down before the `total` is reached.
    ramp_up: f64,
    ramp_down: f64,
}

/// The values the panel's - and + step through, in order. Infinity stands for "as fast as it goes"
/// and "never stops".
const RATES: &[f64] = &[
    0.5,
    1.0,
    2.0,
    4.0,
    8.0,
    16.0,
    32.0,
    64.0,
    128.0,
    f64::INFINITY,
];
const TOTALS: &[f64] = &[
    100.0,
    250.0,
    500.0,
    1000.0,
    2500.0,
    5000.0,
    10_000.0,
    25_000.0,
    100_000.0,
    f64::INFINITY,
];
const RAMPS: &[f64] = &[0.0, 1.0, 2.0, 5.0, 10.0, 20.0, 30.0, 60.0];
/// The panel's rows.
const ROWS: [&str; 4] = ["Rate", "Keys", "Ramp up", "Ramp down"];

/// The next value of `ladder` above `now` (or below, with a negative `delta`); `now` itself when
/// there is none.
fn stepped(ladder: &[f64], now: f64, delta: i32) -> f64 {
    let found = if delta > 0 {
        ladder.iter().copied().find(|v| *v > now)
    } else {
        ladder.iter().rev().copied().find(|v| *v < now)
    };
    found.unwrap_or(now)
}

impl Tuning {
    /// A panel row's - (negative `delta`) or +. The rows are rate, keys, ramp up and ramp down.
    fn step(&mut self, row: usize, delta: i32) {
        let unbounded = |v: f64| if v > 0.0 { v } else { f64::INFINITY };
        match row {
            0 => {
                let v = stepped(RATES, unbounded(self.rate), delta);
                self.rate = if v.is_finite() { v } else { 0.0 };
            }
            1 => {
                let now = self.total.map_or(f64::INFINITY, |n| n as f64);
                let v = stepped(TOTALS, now, delta);
                self.total = v.is_finite().then_some(v as u64);
            }
            2 => self.ramp_up = stepped(RAMPS, self.ramp_up, delta),
            3 => self.ramp_down = stepped(RAMPS, self.ramp_down, delta),
            _ => {}
        }
    }

    /// The panel's values as text, in the order of its rows.
    fn texts(&self) -> [String; 4] {
        let seconds = |s: f64| {
            if s > 0.0 {
                format!("{s}s")
            } else {
                "off".into()
            }
        };
        [
            if self.rate > 0.0 {
                format!("{}/s", self.rate)
            } else {
                "max".into()
            },
            self.total.map_or("\u{221e}".into(), |n| n.to_string()),
            seconds(self.ramp_up),
            seconds(self.ramp_down),
        ]
    }

    /// Keys a second right now (0: as fast as it goes), `running` seconds into the run with `done`
    /// keys pressed. The rate climbs in a straight line over `ramp_up`; at the end it falls in a
    /// straight line to a crawl, starting when just as many keys are left as that fall will press
    /// (at half the rate on average, over `ramp_down`), so that the last key comes as it ends.
    fn rate_now(&self, running: f64, done: u64) -> f64 {
        if self.rate <= 0.0 {
            return 0.0;
        }
        let crawl = (self.rate * 0.05).max(0.5).min(self.rate);
        let mut share = 1.0_f64;
        if self.ramp_up > 0.0 {
            share = share.min(running / self.ramp_up);
        }
        if let (Some(total), true) = (self.total, self.ramp_down > 0.0) {
            let left = total.saturating_sub(done) as f64;
            share = share.min((left / (self.rate * self.ramp_down / 2.0)).sqrt());
        }
        (self.rate * share).max(crawl)
    }
}

/// The pace of a run: when the next key is due.
struct Pace {
    every: Duration,
    next: Instant,
}

impl Pace {
    fn new(rate: f64) -> Self {
        let mut pace = Self {
            every: Duration::ZERO,
            next: Instant::now(),
        };
        pace.set_rate(rate);
        pace
    }

    /// Keys at `rate` a second from now on (0: as fast as it goes).
    fn set_rate(&mut self, rate: f64) {
        self.every = if rate > 0.0 {
            Duration::from_secs_f64(1.0 / rate)
        } else {
            Duration::ZERO
        };
    }

    /// The next key is due; the one after comes a beat later.
    fn due(&mut self) -> bool {
        let now = Instant::now();
        if now < self.next {
            return false;
        }
        self.next = (self.next + self.every).max(now);
        true
    }
}

// ---- the panel -------------------------------------------------------------------------------

/// The pause button and the pace panel, in a window of their own.
struct Panel {
    tuning: Rc<Cell<Tuning>>,
    status: SharedString,
    open: bool,
}

impl Render for Panel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tuning = self.tuning.get();
        let button = |id: gpui::ElementId, label: String| {
            div()
                .id(id)
                .px_3()
                .py_1()
                .rounded_md()
                .bg(rgb(0x313244))
                .hover(|s| s.bg(rgb(0x45475a)))
                .cursor_pointer()
                .child(label)
        };
        let top = div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                button(
                    "pause".into(),
                    if tuning.paused { "Resume" } else { "Pause" }.into(),
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    let mut t = this.tuning.get();
                    t.paused = !t.paused;
                    this.tuning.set(t);
                    cx.notify();
                })),
            )
            .child(div().flex_1().child(self.status.clone()))
            .child(button("pace".into(), "Pace".into()).on_click(cx.listener(
                |this, _: &ClickEvent, _, cx| {
                    this.open = !this.open;
                    cx.notify();
                },
            )));
        let rows = self.open.then(|| {
            let texts = tuning.texts();
            div()
                .flex()
                .flex_col()
                .gap_1()
                .children(ROWS.iter().enumerate().map(|(row, label)| {
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().w(px(90.0)).child(*label))
                        .child(
                            button(("less", row).into(), "-".into()).on_click(cx.listener(
                                move |this, _: &ClickEvent, _, cx| {
                                    let mut t = this.tuning.get();
                                    t.step(row, -1);
                                    this.tuning.set(t);
                                    cx.notify();
                                },
                            )),
                        )
                        .child(div().w(px(60.0)).child(texts[row].clone()))
                        .child(
                            button(("more", row).into(), "+".into()).on_click(cx.listener(
                                move |this, _: &ClickEvent, _, cx| {
                                    let mut t = this.tuning.get();
                                    t.step(row, 1);
                                    this.tuning.set(t);
                                    cx.notify();
                                },
                            )),
                        )
                }))
        });
        div()
            .size_full()
            .bg(rgb(0x1e1e2e))
            .text_color(rgb(0xcdd6f4))
            .text_sm()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(top)
            .children(rows)
    }
}

// ---- the window ------------------------------------------------------------------------------

/// The monkey's hands on the dashboard's window on screen. Hiding and showing keep the window.
struct Live<'a> {
    app: &'a mut App,
    window: AnyWindowHandle,
    dash: Entity<Dash>,
}

fn at(x: f32, y: f32) -> Point<Pixels> {
    point(px(x), px(y))
}

impl Live<'_> {
    fn send<E: InputEvent>(&mut self, event: E) {
        let _ = self.window.update(self.app, |_, window, cx| {
            window.dispatch_event(event.to_platform_input(), cx);
        });
    }

    fn down(&mut self, x: f32, y: f32, button: MouseButton, count: usize) {
        self.send(MouseMoveEvent {
            position: at(x, y),
            modifiers: Modifiers::default(),
            pressed_button: None,
        });
        self.send(MouseDownEvent {
            position: at(x, y),
            modifiers: Modifiers::default(),
            button,
            click_count: count,
            first_mouse: false,
        });
    }

    fn up(&mut self, x: f32, y: f32, button: MouseButton) {
        self.send(MouseUpEvent {
            position: at(x, y),
            modifiers: Modifiers::default(),
            button,
            click_count: 1,
        });
    }
}

impl Rig for Live<'_> {
    fn read<R>(&self, f: impl FnOnce(&Dash) -> R) -> R {
        f(self.dash.read(self.app))
    }

    fn update<R>(&mut self, f: impl FnOnce(&mut Dash, &mut Context<Dash>) -> R) -> R {
        self.dash.update(self.app, f)
    }

    fn control(&mut self, c: Control) {
        self.dash.update(self.app, |d, cx| d.control(c, cx));
    }

    fn key(&mut self, name: &str) {
        let Some(key) = Key::parse(name) else {
            log::warn!("no key called {name:?}");
            return;
        };
        let Ok(keystroke) = Keystroke::parse_with_os(&key.to_gpui(), OperatingSystem::Linux) else {
            log::warn!("no keystroke for {name:?}");
            return;
        };
        let _ = self.window.update(self.app, |_, window, cx| {
            window.dispatch_keystroke(keystroke, cx);
        });
    }

    fn click(&mut self, x: f32, y: f32) {
        self.click_with(x, y, MouseButton::Left);
    }

    fn click_with(&mut self, x: f32, y: f32, button: MouseButton) {
        self.down(x, y, button, 1);
        self.up(x, y, button);
    }

    fn double_click(&mut self, x: f32, y: f32) {
        for n in 1..=2 {
            self.down(x, y, MouseButton::Left, n);
            self.up(x, y, MouseButton::Left);
        }
    }

    fn move_to(&mut self, x: f32, y: f32) {
        self.send(MouseMoveEvent {
            position: at(x, y),
            modifiers: Modifiers::default(),
            pressed_button: None,
        });
    }

    fn drag(&mut self, from: (f32, f32), to: (f32, f32), steps: u32) {
        self.down(from.0, from.1, MouseButton::Left, 1);
        for i in 1..=steps.max(1) {
            let t = i as f32 / steps.max(1) as f32;
            self.send(MouseMoveEvent {
                position: at(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t),
                modifiers: Modifiers::default(),
                pressed_button: Some(MouseButton::Left),
            });
        }
        self.up(to.0, to.1, MouseButton::Left);
    }

    fn scroll(&mut self, x: f32, y: f32, dy: f32) {
        self.move_to(x, y);
        self.send(ScrollEvent {
            position: at(x, y),
            delta: ScrollDelta::Pixels(point(px(0.0), px(dy))),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
            source: Default::default(),
        });
    }

    fn size(&self) -> (u32, u32) {
        let (w, h) = self.dash.read(self.app).size;
        (w as u32, h as u32)
    }
}

/// A stress run in the app's loop: the keys, their pace and when to stop.
struct Run {
    monkey: Option<Monkey>,
    pace: Pace,
    tuning: Rc<Cell<Tuning>>,
    panel: Entity<Panel>,
    /// What the panel shows of it was last made from this.
    shown: Option<(Tuning, u64, u64)>,
    /// How long the run has gone on, not counting the time it was paused.
    running: Duration,
    last: Option<Instant>,
}

/// What a turn of the loop found.
enum Turn {
    Going,
    Finished(Monkey),
    Failed(Failure),
}

impl Run {
    /// One turn of the loop: show what the panel shows, and press the next key if one is due.
    fn turn(&mut self, rig: &mut Live<'_>) -> Turn {
        let now = Instant::now();
        let tuning = self.tuning.get();
        if let Some(last) = self.last.replace(now)
            && !tuning.paused
        {
            self.running += now.saturating_duration_since(last);
        }
        let monkey = self
            .monkey
            .as_mut()
            .expect("the monkey, until the run ends");
        let done = monkey.done();
        let rate = tuning.rate_now(self.running.as_secs_f64(), done);
        self.show(rig.app, tuning, done, rate);
        if tuning.total.is_some_and(|n| done >= n) {
            return Turn::Finished(self.monkey.take().expect("the monkey"));
        }
        if !tuning.paused {
            self.pace.set_rate(rate);
            if self.pace.due() {
                let monkey = self.monkey.as_mut().expect("the monkey");
                if let Err(failure) = monkey.step(rig) {
                    return Turn::Failed(failure);
                }
            }
        }
        Turn::Going
    }

    /// Update the panel when something it shows has changed.
    fn show(&mut self, app: &mut App, tuning: Tuning, done: u64, rate: f64) {
        let key = (tuning, done, (rate * 10.0).round() as u64);
        if self.shown == Some(key) {
            return;
        }
        self.shown = Some(key);
        let pace = if rate > 0.0 {
            format!("{rate:.1}/s")
        } else {
            "max".into()
        };
        let status = match (tuning.paused, tuning.total) {
            (true, Some(n)) => format!("paused \u{b7} {done} of {n}"),
            (true, None) => format!("paused \u{b7} {done}"),
            (false, Some(n)) => format!("{done} of {n} \u{b7} {pace}"),
            (false, None) => format!("{done} \u{b7} {pace}"),
        };
        self.panel.update(app, |panel, cx| {
            panel.status = status.into();
            cx.notify();
        });
    }
}

/// `niri-dash stress` on screen: the dashboard in a window and the monkey at its keys until the
/// keys are done, a problem turns up, or a window is closed.
pub fn run(monkey: Monkey, args: &super::Args) -> Result<Monkey> {
    let (events_tx, events) = window::channel::<Event>();
    let system = niri_dash_system::mock(events_tx)?;
    let config = Layout::load();
    let tuning = Tuning {
        paused: false,
        rate: args.rate.max(0.0),
        total: args.ops,
        ramp_up: args.ramp_up.max(0.0),
        ramp_down: args.ramp_down.max(0.0),
    };
    let tuning = Rc::new(Cell::new(tuning));
    let outcome: Rc<RefCell<Option<std::result::Result<Monkey, Failure>>>> = Default::default();
    let result = outcome.clone();
    gpui_kit::application()
        .with_assets(crate::assets::Assets)
        .with_quit_mode(gpui::QuitMode::Explicit)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            let dash = cx.new(|cx| Dash::new(system, config, None, cx));
            // A hide or show the dashboard asks of its surface is done in place: the window stays.
            cx.subscribe(&dash, |dash, event: &DashEvent, cx| {
                let control = match event {
                    DashEvent::Hide => Control::Hide,
                    DashEvent::Control(Control::Quit) => return,
                    DashEvent::Control(c) => *c,
                };
                dash.update(cx, |d, cx| d.control(control, cx));
            })
            .detach();
            window::pump_events(&dash, events, cx);
            window::tick(&dash, cx);
            let opened = cx.open_window(
                WindowOptions {
                    titlebar: Some(TitlebarOptions {
                        title: Some("niri-dash stress".into()),
                        ..TitlebarOptions::default()
                    }),
                    window_bounds: Some(WindowBounds::Windowed(gpui::Bounds {
                        origin: point(px(0.0), px(0.0)),
                        size: size(px(1280.0), px(800.0)),
                    })),
                    ..WindowOptions::default()
                },
                {
                    let dash = dash.clone();
                    move |window, cx| {
                        let focus = dash.read(cx).focus.clone();
                        window.focus(&focus, cx);
                        dash.clone()
                    }
                },
            );
            let Ok(handle) = opened else {
                log::error!("opening the window failed");
                cx.quit();
                return;
            };
            let panel_tuning = tuning.clone();
            let panel = cx.new(|_| Panel {
                tuning: panel_tuning,
                status: SharedString::default(),
                open: false,
            });
            let panel_view = panel.clone();
            let _ = cx.open_window(
                WindowOptions {
                    titlebar: Some(TitlebarOptions {
                        title: Some("niri-dash stress: pace".into()),
                        ..TitlebarOptions::default()
                    }),
                    window_bounds: Some(WindowBounds::Windowed(gpui::Bounds {
                        origin: point(px(0.0), px(0.0)),
                        size: size(px(380.0), px(230.0)),
                    })),
                    focus: false,
                    ..WindowOptions::default()
                },
                move |_, _| panel_view,
            );
            // Closing either window ends the run.
            cx.on_window_closed(|cx, _| cx.quit()).detach();
            // The dashboard comes up, as the hotkey would.
            dash.update(cx, |d, cx| d.control(Control::Show, cx));
            let mut run = Run {
                monkey: Some(monkey),
                pace: Pace::new(tuning.get().rate_now(0.0, 0)),
                tuning,
                panel,
                shown: None,
                running: Duration::ZERO,
                last: None,
            };
            let window: AnyWindowHandle = handle.into();
            cx.spawn(async move |cx: &mut AsyncApp| {
                loop {
                    // Often enough for the pause button and the pace to answer at once.
                    cx.background_executor()
                        .timer(Duration::from_millis(5))
                        .await;
                    let turn = cx.update(|app| {
                        let mut rig = Live {
                            app,
                            window,
                            dash: dash.clone(),
                        };
                        run.turn(&mut rig)
                    });
                    match turn {
                        Turn::Going => {}
                        Turn::Finished(monkey) => {
                            *result.borrow_mut() = Some(Ok(monkey));
                            cx.update(|app| app.quit());
                            return;
                        }
                        Turn::Failed(failure) => {
                            *result.borrow_mut() = Some(Err(failure));
                            cx.update(|app| app.quit());
                            return;
                        }
                    }
                }
            })
            .detach();
        });
    let outcome = outcome.borrow_mut().take();
    match outcome {
        Some(Ok(monkey)) => Ok(monkey),
        Some(Err(failure)) => Err(Error::Stress(failure.to_string())),
        // A window was closed: the run ends where it is.
        None => Err(Error::Ui(
            "the window was closed before the run ended".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuning() -> Tuning {
        Tuning {
            paused: false,
            rate: 10.0,
            total: Some(1000),
            ramp_up: 0.0,
            ramp_down: 0.0,
        }
    }

    #[test]
    fn the_rate_is_steady_without_ramps() {
        assert_eq!(tuning().rate_now(0.0, 0), 10.0);
        assert_eq!(tuning().rate_now(100.0, 999), 10.0);
    }

    #[test]
    fn the_rate_climbs_over_the_ramp_up_and_holds() {
        let t = Tuning {
            ramp_up: 10.0,
            ..tuning()
        };
        assert_eq!(t.rate_now(0.0, 0), 0.5);
        assert!((t.rate_now(5.0, 0) - 5.0).abs() < 1e-9);
        assert_eq!(t.rate_now(10.0, 0), 10.0);
        assert_eq!(t.rate_now(60.0, 0), 10.0);
    }

    #[test]
    fn the_rate_falls_over_the_last_keys_and_never_stops() {
        let t = Tuning {
            ramp_down: 10.0,
            ..tuning()
        };
        assert_eq!(t.rate_now(0.0, 900), 10.0);
        assert_eq!(t.rate_now(0.0, 950), 10.0);
        assert!(t.rate_now(0.0, 975) < 10.0);
        // One key left: sqrt(1 / 50) of the rate, a bit over the crawl; none left: the crawl.
        assert!((t.rate_now(0.0, 999) - 10.0 * (1.0_f64 / 50.0).sqrt()).abs() < 1e-9);
        assert_eq!(t.rate_now(0.0, 1000), 0.5);
    }

    #[test]
    fn as_fast_as_it_goes_has_no_ramps() {
        let t = Tuning {
            rate: 0.0,
            ramp_up: 10.0,
            ramp_down: 10.0,
            ..tuning()
        };
        assert_eq!(t.rate_now(0.0, 0), 0.0);
        assert_eq!(t.rate_now(0.0, 999), 0.0);
    }

    #[test]
    fn the_panel_steps_through_the_ladders_and_stops_at_their_ends() {
        let mut t = tuning();
        t.step(0, 1);
        assert_eq!(t.rate, 16.0);
        t.rate = 128.0;
        t.step(0, 1);
        assert_eq!(t.rate, 0.0, "past the top is as fast as it goes");
        t.step(0, 1);
        assert_eq!(t.rate, 0.0);
        t.step(0, -1);
        assert_eq!(t.rate, 128.0);
        t.total = None;
        t.step(1, 1);
        assert_eq!(t.total, None, "never stops is the top");
        t.step(1, -1);
        assert_eq!(t.total, Some(100_000));
        t.step(2, -1);
        assert_eq!(t.ramp_up, 0.0);
        t.step(3, 1);
        t.step(3, 1);
        assert_eq!(t.ramp_down, 2.0);
    }
}

//! `niri-dash snapshot`: the dashboard drawn into a PNG, without a compositor and without
//! touching the screen. A development tool, and what makes the manual's pictures
//! (`book/screenshots.sh`).
//!
//! It is the headless harness's dashboard ([`crate::testing`]) with a real renderer behind it:
//! GPUI's test platform schedules and delivers input deterministically, and its wgpu renderer
//! (the one a window uses, drawing to a texture instead of a surface) rasterizes the frame. It
//! needs a GPU (or a software Vulkan such as lavapipe), but no display. The test platform's
//! windows have a scale factor of 2, so the frame is drawn at twice the size asked for and
//! resized to `--width` times `--scale` pixels.
//!
//! The command line is the Slint version's `niri-dash snapshot` (what it took that GPUI has no
//! use for, the software renderer's buffers, is gone):
//! - `--mock` shows the mock system's made-up data, answered on this thread (so a picture is the
//!   same every time); without it the live system is used, and `--wait` is real time.
//! - Scripted input after the wait: `--events "down:x,y;move:x,y;up:x,y;dbl:x,y;wait:ms;key:K"`.
//! - The config is `$NIRI_DASH_HOME/config/niri-dash/config.kdl`, as for every other command;
//!   without `NIRI_DASH_HOME` a throwaway home with a copy of the real config is used, so the
//!   run never writes over it.

use crate::Control;
use crate::dash::Dash;
use crate::edit::DrawerTab;
use crate::keys::Key;
use crate::layout::{Kind, Layout};
use crate::stress::Sandbox;
use crate::theme;
use crate::window::{Error, Result, channel, start_system};
use gpui::{
    AnyWindowHandle, AppContext as _, Entity, HeadlessAppContext, InputEvent, Keystroke, Modifiers,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, OperatingSystem,
    PlatformHeadlessRenderer, Point, px, size,
};
use niri_dash_system::{Demo, Event, Inboxes, MockSystem, Started, channels};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// A sidebar tab to open in edit mode.
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum Tab {
    Colors,
    Style,
    Arrange,
}

#[derive(clap::Args, Clone)]
pub struct Args {
    /// Where to write the PNG.
    out: PathBuf,
    /// The screen's width in logical pixels.
    #[arg(long, default_value_t = 1706)]
    width: u32,
    #[arg(long, default_value_t = 1066)]
    height: u32,
    /// Pixels per logical pixel in the picture.
    #[arg(long, default_value_t = 1.5)]
    scale: f32,
    /// Seconds of data before the frame is taken (made-up seconds with --mock).
    #[arg(long, default_value_t = 3.0)]
    wait: f32,
    #[arg(long)]
    edit: bool,
    /// Render a standalone app (a widget name) instead of the dashboard.
    #[arg(long, value_enum)]
    app: Option<Kind>,
    /// Open this sidebar tab.
    #[arg(long, value_enum, requires = "edit")]
    tab: Option<Tab>,
    /// Lay the widgets out by themselves (with config.kdl's `autolayout` settings) first.
    #[arg(long, requires = "edit")]
    arrange: bool,
    /// Open the settings of the N-th widget in config.kdl (counting from 0).
    #[arg(long, value_name = "N", requires = "edit")]
    configure: Option<usize>,
    /// Show this color scheme instead of the configured one, without touching config.kdl.
    #[arg(long)]
    colorscheme: Option<String>,
    /// Show this style instead of the configured one, without touching config.kdl.
    #[arg(long)]
    style: Option<String>,
    /// Scripted input after the wait: "down:x,y;move:x,y;up:x,y;dbl:x,y;wait:ms;key:K". K is a
    /// character, or esc, enter, space, tab, backtab, backspace, left, right, up, down, with
    /// ctrl+ / shift+ / alt+ in front for modifiers held.
    #[arg(long, value_name = "SCRIPT")]
    events: Option<String>,
    /// Show the mock system's made-up data instead of the live system's.
    #[arg(long)]
    mock: bool,
}

/// The dashboard in a headless window with a renderer.
struct Shooter {
    cx: HeadlessAppContext,
    dash: Entity<Dash>,
    window: AnyWindowHandle,
    inbound: async_channel::Receiver<Event>,
    /// The mock system and where its commands wait: it answers on this thread, whenever the
    /// dashboard takes a turn.
    mock: Option<(MockSystem, Inboxes)>,
    /// A mouse button is down (a move then is a drag).
    pressed: bool,
}

impl Shooter {
    fn new(args: &Args, solo: Option<Kind>) -> Result<Self> {
        let (events, inbound) = channel::<Event>();
        let (system, mock) = if args.mock {
            let (senders, commands) = channels();
            let started = Started {
                senders,
                caps: MockSystem::CAPS,
            };
            (
                started,
                Some((MockSystem::new(Demo::new(), events.clone()), commands)),
            )
        } else {
            (start_system(events.clone(), solo)?, None)
        };
        let text_system = Arc::new(gpui_wgpu::CosmicTextSystem::new("IBM Plex Sans"));
        let mut cx =
            HeadlessAppContext::with_platform(text_system, Arc::new(crate::assets::Assets), || {
                gpui_wgpu::WgpuHeadlessRenderer::new()
                    .map(|r| Some(Box::new(r) as Box<dyn PlatformHeadlessRenderer>))
            });
        let config = Layout::load();
        let handle = cx
            .open_window(
                size(px(args.width as f32), px(args.height as f32)),
                move |window, cx| {
                    gpui_kit::init(cx);
                    cx.new(|cx| {
                        let mut dash = Dash::new(system, config, solo, cx);
                        // Nothing glides: what was asked for is what is drawn.
                        dash.edit.anim.snap = true;
                        window.focus(&dash.focus, cx);
                        dash
                    })
                },
            )
            .map_err(|e| Error::Ui(format!("no headless renderer: {e:#}")))?;
        let dash = handle.root(&mut cx).map_err(|e| Error::Ui(e.to_string()))?;
        let mut me = Self {
            cx,
            dash,
            window: handle.into(),
            inbound,
            mock,
            pressed: false,
        };
        me.settle();
        Ok(me)
    }

    fn update<R>(&mut self, f: impl FnOnce(&mut Dash, &mut gpui::Context<Dash>) -> R) -> R {
        let r = self.dash.update(&mut self.cx, f);
        self.settle();
        r
    }

    /// The mock system takes the commands that wait for it.
    fn answer_mock(&mut self) {
        if let Some((mock, commands)) = &mut self.mock {
            while let Some(command) = commands.try_next() {
                let kept = mock.handle(command);
                mock.received.push(kept);
            }
        }
    }

    /// Deliver what is pending, answer the mock, and draw.
    fn settle(&mut self) {
        self.answer_mock();
        while let Ok(event) = self.inbound.try_recv() {
            self.dash
                .update(&mut self.cx, |d, cx| d.handle_event(event, cx));
            self.answer_mock();
        }
        self.cx.run_until_parked();
        self.draw();
    }

    fn draw(&mut self) {
        let _ = self.cx.update_window(self.window, |_, window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
    }

    /// Time passes: with the mock system the dashboard's clock is made to run, otherwise it is
    /// the wall clock that does.
    fn pass(&mut self, ms: u64) {
        if self.mock.is_some() {
            let mut left = ms;
            while left > 0 {
                let step = left.min(1000);
                self.cx.advance_clock(Duration::from_millis(step));
                if step == 1000 {
                    self.dash.update(&mut self.cx, |d, cx| d.tick(cx));
                }
                self.settle();
                left -= step;
            }
        } else {
            let end = std::time::Instant::now() + Duration::from_millis(ms);
            while std::time::Instant::now() < end {
                std::thread::sleep(Duration::from_millis(25));
                self.settle();
            }
            self.dash.update(&mut self.cx, |d, cx| d.tick(cx));
            self.settle();
        }
    }

    fn event<E: InputEvent>(&mut self, event: E) {
        let _ = self.cx.update_window(self.window, |_, window, cx| {
            window.dispatch_event(event.to_platform_input(), cx);
        });
        self.settle();
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
        let _ = self.cx.update_window(self.window, |_, window, cx| {
            window.dispatch_keystroke(keystroke, cx);
        });
        self.settle();
    }

    fn pointer(&mut self, verb: &str, at: Point<gpui::Pixels>) {
        let mods = Modifiers::default();
        let left = MouseButton::Left;
        match verb {
            "down" => {
                self.pressed = true;
                self.event(MouseDownEvent {
                    position: at,
                    modifiers: mods,
                    button: left,
                    click_count: 1,
                    first_mouse: false,
                });
            }
            "up" => {
                self.pressed = false;
                self.event(MouseUpEvent {
                    position: at,
                    modifiers: mods,
                    button: left,
                    click_count: 1,
                });
            }
            "move" => self.event(MouseMoveEvent {
                position: at,
                modifiers: mods,
                pressed_button: self.pressed.then_some(left),
            }),
            "dbl" => {
                self.event(MouseMoveEvent {
                    position: at,
                    modifiers: mods,
                    pressed_button: None,
                });
                for n in 1..=2 {
                    self.event(MouseDownEvent {
                        position: at,
                        modifiers: mods,
                        button: left,
                        click_count: n,
                        first_mouse: false,
                    });
                    self.event(MouseUpEvent {
                        position: at,
                        modifiers: mods,
                        button: left,
                        click_count: n,
                    });
                }
            }
            _ => log::warn!("unknown event {verb}"),
        }
    }

    /// Run a script of input steps (see [`Args::events`]).
    fn script(&mut self, script: &str) {
        for step in script.split(';').filter(|s| !s.is_empty()) {
            let (verb, arg) = step.split_once(':').unwrap_or((step, ""));
            match verb {
                "wait" => self.pass(arg.parse().unwrap_or(300)),
                "key" => self.key(arg),
                _ => {
                    let (x, y) = arg
                        .split_once(',')
                        .map(|(a, b)| (a.parse().unwrap_or(0.0), b.parse().unwrap_or(0.0)))
                        .unwrap_or((0.0, 0.0));
                    self.pointer(verb, gpui::point(px(x), px(y)));
                }
            }
            self.pass(40);
        }
        self.pass(400);
    }
}

/// Render the snapshot `args` describe.
pub fn run(args: &Args) -> Result<()> {
    // Never write over the real config and state: a home of their own, with a copy of the config.
    let _sandbox = if std::env::var_os("NIRI_DASH_HOME").is_none() {
        let real = Layout::path();
        let real = real.exists().then_some(real);
        Some(Sandbox::enter(real.as_deref()).map_err(Error::System)?)
    } else {
        None
    };
    let solo = args.app;
    let mut shot = Shooter::new(args, solo)?;
    shot.update(|d, cx| d.control(Control::Show, cx));
    if args.colorscheme.is_some() || args.style.is_some() {
        shot.update(|d, _| {
            let (look, problems) = theme::load_look(
                args.colorscheme.as_deref().unwrap_or(&d.layout.colorscheme),
                args.style.as_deref().unwrap_or(&d.layout.style),
                &d.layout.style_overrides,
            );
            d.show_look(look, &problems, false);
        });
    }
    if args.edit {
        shot.update(|d, cx| d.set_editing(true, cx));
        let tab = match args.tab {
            Some(Tab::Colors) => Some(DrawerTab::Colors),
            Some(Tab::Style) => Some(DrawerTab::Style),
            Some(Tab::Arrange) => Some(DrawerTab::Arrange),
            None => None,
        };
        if let Some(tab) = tab {
            shot.update(|d, _| d.set_tab(tab));
        }
        if let Some(n) = args.configure {
            shot.update(|d, cx| {
                if let Some(id) = d.layout.widgets.get(n).map(|w| w.id) {
                    d.configure_widget(id, cx);
                }
            });
        }
    }
    shot.pass((args.wait * 1000.0) as u64);
    if args.arrange {
        shot.update(|d, _| d.arrange_now());
    }
    if let Some(script) = &args.events {
        shot.script(script);
    }
    shot.draw();
    let image = shot
        .cx
        .capture_screenshot(shot.window)
        .map_err(|e| Error::Ui(format!("taking the picture: {e:#}")))?;
    let (w, h) = (
        (args.width as f32 * args.scale).round() as u32,
        (args.height as f32 * args.scale).round() as u32,
    );
    let image = if image.dimensions() == (w, h) {
        image
    } else {
        image::imageops::resize(&image, w, h, image::imageops::FilterType::Lanczos3)
    };
    image
        .save(&args.out)
        .map_err(|e| Error::Ui(format!("writing {}: {e}", args.out.display())))
}

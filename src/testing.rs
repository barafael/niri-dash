//! Driving the dashboard headlessly, for the tests and the stress run: the real dashboard in a
//! window of GPUI's test platform (no compositor, no GPU), with channels standing in for the
//! system. A test reads what the dashboard sent from the [`Inboxes`], hands it [`Event`]s as the
//! system would, and uses it through keys and clicks, or reads the [`Dash`] itself.
//!
//! The dashboard reads and writes its config and state under the XDG directories, which the
//! test points somewhere disposable ([`Sandbox`]) before the first harness.

use crate::Control;
use crate::dash::{Dash, DashEvent};
use crate::keys::Key;
use crate::layout::{Layout, Loaded};
use gpui::{
    AnyWindowHandle, AppContext as _, Entity, Modifiers, MouseButton, Pixels, Point,
    TestAppContext, VisualTestContext, WindowHandle, point, px, size,
};
use niri_dash_system::{Demo, Event, EventSender, Inboxes, MockSystem, Started, channels};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

pub use crate::dash::NavMode;
pub use crate::edit::probe::{Tile, can_be_round, kind_names, min_size, setting_labels, title};
pub use crate::edit::{DrawerTab, RowKind, SettingRow};
pub use crate::stress::{Failure, Monkey, Sandbox};

mod access;
/// The accessibility tree of the dashboard, and `Harness::a11y*`, `cursor`, `spoken`.
pub use access::{Action, Element, Toggled, Tree, request as a11y_request_for};
pub use gpui::accesskit::Role;

/// The window of a harness, unless it is told another size: a large screen.
pub const SIZE: (u32, u32) = (1706, 1066);

/// The dashboard, built as the daemon builds it, on channels instead of the system.
pub struct Harness {
    pub(crate) cx: TestAppContext,
    pub dash: Entity<Dash>,
    window: WindowHandle<Dash>,
    /// Where the dashboard's commands arrive.
    pub inbox: Inboxes,
    events: EventSender,
    inbound: async_channel::Receiver<Event>,
    /// Controls (show, hide) the dashboard asked for, in answer to what it was given.
    controls: Rc<RefCell<Vec<Control>>>,
    /// The mock system of [`Harness::with_mock`] and where its commands wait. It answers here, on
    /// the harness's own thread, whenever the dashboard takes a turn: on a thread of its own it
    /// would answer when the machine let it, and the same keys wouldn't give the same run.
    mock: Option<(MockSystem, Inboxes)>,
    size: (u32, u32),
}

impl Harness {
    /// The dashboard with the widgets `config` (config.kdl's text) lays out, up and on screen as
    /// far as it can tell, under niri with everything available.
    pub fn new(config: &str) -> Self {
        let (senders, inbox) = channels();
        let system = Started {
            senders,
            caps: MockSystem::CAPS,
        };
        Self::on(config, system, inbox, SIZE)
    }

    /// The dashboard on the mock system, which answers what it is told with made-up data (as
    /// `NIRI_DASH_MOCK=1` shows). [`Harness::inbox`] stays empty: the mock gets the commands.
    pub fn with_mock(config: &str) -> Self {
        Self::with_mock_sized(config, SIZE)
    }

    /// [`Harness::with_mock`] on a window of `size` pixels. A small one has small cells, so
    /// every widget at its minimum has little room.
    pub fn with_mock_sized(config: &str, size: (u32, u32)) -> Self {
        let (senders, commands) = channels();
        let system = Started {
            senders,
            caps: MockSystem::CAPS,
        };
        let mut me = Self::on(config, system, channels().1, size);
        me.mock = Some((MockSystem::new(Demo::new(), me.events.clone()), commands));
        // The mock sends what it has once the dashboard is up.
        me.control(Control::Show);
        me
    }

    fn on(config: &str, system: Started, inbox: Inboxes, size_px: (u32, u32)) -> Self {
        let mut cx = TestAppContext::single();
        let (layout, warnings) = Layout::parse(config).expect("a config that parses");
        let loaded = Loaded {
            layout,
            warnings,
            broken: false,
            text: config.to_string(),
        };
        let (events, inbound) = crate::window::channel::<Event>();
        let window = cx.open_window(
            size(px(size_px.0 as f32), px(size_px.1 as f32)),
            |window, cx| {
                gpui_kit::init(cx);
                let mut dash = Dash::new(system, loaded, None, cx);
                // Nothing glides in a headless run: what a key or click did is what is there.
                dash.edit.anim.snap = true;
                window.focus(&dash.focus, cx);
                dash
            },
        );
        let dash = window.root(&mut cx).expect("the dashboard's window");
        let controls: Rc<RefCell<Vec<Control>>> = Default::default();
        let sink = controls.clone();
        cx.update(|cx| {
            cx.subscribe(&dash, move |_, event: &DashEvent, _| match event {
                DashEvent::Hide => sink.borrow_mut().push(Control::Hide),
                DashEvent::Control(c) => sink.borrow_mut().push(*c),
            })
            .detach();
        });
        let mut me = Self {
            cx,
            dash,
            window,
            inbox,
            events,
            inbound,
            controls,
            mock: None,
            size: size_px,
        };
        me.settle();
        me
    }

    pub(crate) fn vcx(&self) -> VisualTestContext {
        VisualTestContext::from_window(AnyWindowHandle::from(self.window), &self.cx)
    }

    /// Read the dashboard.
    pub fn read<R>(&self, f: impl FnOnce(&Dash) -> R) -> R {
        self.cx.read_entity(&self.dash, |d, _| f(d))
    }

    /// Change the dashboard, as the app itself would; it is drawn again afterwards.
    pub fn update<R>(&mut self, f: impl FnOnce(&mut Dash, &mut gpui::Context<Dash>) -> R) -> R {
        let r = self.dash.update(&mut self.cx, f);
        self.settle();
        r
    }

    /// The window's size in pixels.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// A control (show, hide, …) as the control socket would send it.
    pub fn control(&mut self, c: Control) {
        self.update(|d, cx| d.control(c, cx));
    }

    /// The controls (hide, show, …) the dashboard asked of its surface since the last call, in
    /// order. The surface isn't there, so they only pile up here.
    pub fn controls(&mut self) -> Vec<Control> {
        std::mem::take(&mut *self.controls.borrow_mut())
    }

    /// Hand the dashboard an event, as the system would, and let it show it.
    pub fn send(&mut self, event: impl Into<Event>) {
        let _ = self.events.send(event.into());
        self.settle();
    }

    /// Deliver what is pending, answer the mock, and draw.
    pub fn settle(&mut self) {
        self.answer_mock();
        while let Ok(event) = self.inbound.try_recv() {
            self.dash.update(&mut self.cx, |d, cx| d.handle_event(event, cx));
            self.answer_mock();
        }
        self.cx.run_until_parked();
        self.draw();
    }

    /// Draw what changed since the last frame.
    pub fn draw(&mut self) {
        let mut vcx = self.vcx();
        vcx.update(|window, cx| {
            window.refresh();
            window.draw(cx).clear(cx);
        });
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

    /// What the mock system has been told, in order.
    pub fn received(&self) -> Vec<String> {
        self.mock
            .as_ref()
            .map(|(m, _)| m.received.iter().map(|r| format!("{r:?}")).collect())
            .unwrap_or_default()
    }

    /// Time passes: a second tick per `secs`.
    pub fn advance(&mut self, secs: u64) {
        for _ in 0..secs {
            self.cx.executor().advance_clock(Duration::from_secs(1));
            self.dash.update(&mut self.cx, |d, cx| d.tick(cx));
            self.settle();
        }
    }

    /// Press and release a key, through the window as a keyboard would.
    pub fn press(&mut self, key: &Key) {
        let window = AnyWindowHandle::from(self.window);
        self.cx.simulate_keystrokes(window, &key.to_gpui());
        self.settle();
    }

    /// Press a key by name: `"j"`, `"ctrl+h"`, `"esc"`, `"shift+J"`.
    pub fn key(&mut self, name: &str) {
        let key = Key::parse(name).unwrap_or_else(|| panic!("no key called {name:?}"));
        self.press(&key);
    }

    /// Press several keys, a word of letters, or names separated by spaces.
    pub fn keys(&mut self, names: &str) {
        for name in names.split_whitespace() {
            self.key(name);
        }
    }

    /// Type text, one key per character.
    pub fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.press(&Key::char(c));
        }
    }

    /// Click at a point, in logical pixels.
    pub fn click(&mut self, x: f32, y: f32) {
        self.click_with(x, y, MouseButton::Left);
    }

    pub fn click_with(&mut self, x: f32, y: f32, button: MouseButton) {
        let at: Point<Pixels> = point(px(x), px(y));
        let mut vcx = self.vcx();
        vcx.simulate_mouse_move(at, None, Modifiers::default());
        vcx.simulate_mouse_down(at, button, Modifiers::default());
        vcx.simulate_mouse_up(at, button, Modifiers::default());
        drop(vcx);
        self.settle();
    }

    /// Turn the wheel at a point; `dy` in logical pixels, negative scrolls down.
    pub fn scroll(&mut self, x: f32, y: f32, dy: f32) {
        let at: Point<Pixels> = point(px(x), px(y));
        let mut vcx = self.vcx();
        vcx.simulate_mouse_move(at, None, Modifiers::default());
        vcx.simulate_event(gpui::ScrollEvent {
            position: at,
            delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(dy))),
            modifiers: Modifiers::default(),
            touch_phase: gpui::TouchPhase::Moved,
            source: Default::default(),
        });
        drop(vcx);
        self.settle();
    }

    /// The bounds of the element a view named with `debug_selector`, in window pixels.
    pub fn bounds_of(&self, selector: &'static str) -> Option<gpui::Bounds<Pixels>> {
        let mut vcx = self.vcx();
        vcx.debug_bounds(selector)
    }

    /// Click on the element called `selector`.
    pub fn click_on(&mut self, selector: &'static str) {
        let b = self
            .bounds_of(selector)
            .unwrap_or_else(|| panic!("nothing called {selector:?} on screen"));
        let c = b.center();
        self.click(f32::from(c.x), f32::from(c.y));
    }

    /// Whether an element called `selector` is on screen.
    pub fn has(&self, selector: &'static str) -> bool {
        self.bounds_of(selector).is_some()
    }

    /// Let `monkey` press `ops` keys on the mock system (see [`Harness::with_mock`]). Hiding and
    /// showing keep the window, as on screen. Gives the monkey back for its summary, or what
    /// went wrong.
    pub fn stress(&mut self, monkey: Monkey, ops: u64) -> Result<Monkey, Failure> {
        crate::stress::drive(self, monkey, ops)
    }
}

/// The text of the default config.kdl (what a first start writes).
pub fn default_config() -> String {
    Layout::default_text()
}

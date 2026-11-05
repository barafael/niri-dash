//! Driving the dashboard headlessly, for the UI tests (`tests/ui.rs`): the real dashboard on a
//! window that only exists in memory, with channels standing in for the system. A test reads what
//! the dashboard sent from the [`Inboxes`], hands it [`Event`]s as the system would, and uses the
//! UI through its globals or through key and pointer events.
//!
//! Slint allows one platform per process and its objects stay on the thread that made them, so a
//! test binary runs its scenarios one after another on one thread; each gets a fresh dashboard.
//! The dashboard reads and writes its config and state under the XDG directories, which the test
//! points somewhere disposable before the first harness.

use super::{App, Input, build};
use crate::layout::{Layout, Loaded};
use crate::{Control, Dash};
use niri_dash_system::{Event, Inboxes, MockSystem, Started, channels};
use slint::platform::software_renderer::PremultipliedRgbaColor;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Key, Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, PhysicalSize, SharedString};
use smithay_client_toolkit::reexports::calloop::{EventLoop, channel};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// Windows that only exist in memory.
struct TestPlatform;

thread_local! {
    /// The window made last, the dashboard's, to render it.
    static WINDOW: RefCell<Option<Rc<MinimalSoftwareWindow>>> = const { RefCell::new(None) };
}

impl Platform for TestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
        WINDOW.with(|w| *w.borrow_mut() = Some(window.clone()));
        Ok(window)
    }
}

/// The dashboard, built as the daemon builds it, on channels instead of the system.
pub struct Harness {
    ui: Dash,
    app: Rc<App>,
    /// Where the dashboard's commands arrive.
    pub inbox: Inboxes,
    events: niri_dash_system::EventSender,
    event_loop: EventLoop<'static, Vec<Control>>,
    /// Controls (show, hide) the dashboard asked for, in answer to events.
    controls: Vec<Control>,
    /// The config watcher's end; kept so it has somewhere to send.
    _inputs: channel::Channel<Input>,
    window: Rc<MinimalSoftwareWindow>,
    pixels: Vec<PremultipliedRgbaColor>,
    /// The window's width in pixels.
    width: usize,
    /// How many events the dashboard has been handed.
    delivered: Rc<std::cell::Cell<u64>>,
}

/// The window of a harness, unless it is told another size: a large screen.
const SIZE: (u32, u32) = (1706, 1066);

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
    /// `niri-dash snapshot --mock` shows). [`Harness::inbox`] stays empty: the mock gets the
    /// commands.
    pub fn with_mock(config: &str) -> Self {
        Self::with_mock_sized(config, SIZE)
    }

    /// [`Harness::with_mock`] on a window of `size` pixels. A small one has small cells, so
    /// every widget at its minimum has little room.
    pub fn with_mock_sized(config: &str, size: (u32, u32)) -> Self {
        let (events, events_rx) = super::loop_channel::<Event>();
        let system = niri_dash_system::mock(events).expect("starting the mock system");
        let mut me = Self::on(config, system, channels().1, size);
        me.listen(events_rx);
        // The mock sends what it has once the dashboard is up.
        me.app.set_visible(true);
        me.pump();
        me
    }

    fn on(config: &str, system: Started, inbox: Inboxes, size: (u32, u32)) -> Self {
        // Already set by an earlier harness, which is fine.
        let _ = slint::platform::set_platform(Box::new(TestPlatform));
        let ui = Dash::new().expect("creating the UI");
        ui.show().expect("showing the UI");
        let window = WINDOW
            .with(|w| w.borrow().clone())
            .expect("the dashboard's window");
        ui.window().set_size(PhysicalSize::new(size.0, size.1));
        let (layout, warnings) = Layout::parse(config).expect("a config that parses");
        let loaded = Loaded {
            layout,
            warnings,
            broken: false,
            text: config.to_string(),
        };
        let (events, events_rx) = super::loop_channel::<Event>();
        let (input_tx, inputs) = super::loop_channel::<Input>();
        let app = build(&ui, system, input_tx, None, loaded, true);
        // As on show: the keyboard goes to the dashboard, nothing entered.
        ui.window()
            .dispatch_event(WindowEvent::WindowActiveChanged(true));
        ui.invoke_reset();
        app.reset_nav();
        let mut me = Self {
            ui,
            app,
            inbox,
            events,
            event_loop: EventLoop::try_new().expect("an event loop"),
            controls: Vec::new(),
            _inputs: inputs,
            window,
            pixels: vec![PremultipliedRgbaColor::default(); (size.0 * size.1) as usize],
            width: size.0 as usize,
            delivered: Default::default(),
        };
        me.listen(events_rx);
        me.pump();
        me
    }

    /// Show what arrives on `events`.
    fn listen(&mut self, events: channel::Channel<Event>) {
        let a = self.app.clone();
        let delivered = self.delivered.clone();
        self.event_loop
            .handle()
            .insert_source(events, move |ev, _, controls: &mut Vec<Control>| {
                if let channel::Event::Msg(e) = ev {
                    delivered.set(delivered.get() + 1);
                    if let Some(c) = a.handle_event(e) {
                        controls.push(c);
                    }
                }
            })
            .expect("inserting the event source");
    }

    /// What a screen reader would say about the entered widget ("button \"Rescan\""), `None` with
    /// no widget entered. An error says what doesn't add up (see `a11y.rs`).
    pub fn spoken(&self) -> Result<Option<String>, String> {
        Ok(self.app.spoken()?.map(|s| s.to_string()))
    }

    /// The element labelled `label` with this role ("button", "listitem", …), as a screen reader
    /// would find it.
    pub fn element(&self, label: &str, role: &str) -> Option<Element> {
        self.app.element(label, role)
    }

    /// Everything in the window a screen reader could read out.
    pub fn readable(&self) -> Vec<String> {
        self.app.readable()
    }

    /// Draw what changed since the last frame, as the daemon would.
    pub fn render(&mut self) {
        crate::shell::render_into(&self.window, &mut self.pixels, self.width, false);
    }

    /// One turn of the dashboard's loop in a stress run: what is pending, and the controls it
    /// asks for applied in place.
    fn turn(&mut self) {
        self.pump();
        for c in self.controls() {
            self.app.control_in_place(c);
        }
    }

    /// Turn until nothing has arrived for two turns, laying out as a frame would on each. Frames
    /// are only drawn now and then here; on screen every turn draws one, and what a layout sets off
    /// (a list scrolling to keep the cursor in sight) runs on the turn after. A check should look
    /// at what the screen would show.
    fn quieten(&mut self) {
        let mut seen = self.delivered.get();
        let mut quiet = 0;
        while quiet < 2 {
            self.turn();
            self.app.lay_out();
            if self.delivered.get() == seen {
                quiet += 1;
            } else {
                seen = self.delivered.get();
                quiet = 0;
            }
        }
    }

    /// Let `monkey` press `ops` keys, `rate` a second (0: as fast as it goes), on the mock
    /// system (see [`Harness::with_mock`]). Hiding and showing keep the window, as on screen.
    /// Frames are drawn ten times a second, as a screen would show them, however fast the keys
    /// come. Gives the monkey back for its summary, or what went wrong.
    pub fn stress(
        &mut self,
        mut monkey: super::stress::Monkey,
        ops: u64,
        rate: f64,
    ) -> Result<super::stress::Monkey, super::stress::Failure> {
        let mut pace = super::stress::Pace::new(rate);
        let mut frames = super::stress::Pace::new(10.0);
        while monkey.done() < ops {
            // The dashboard's loop runs on between keys.
            while !pace.due() {
                self.turn();
                std::thread::sleep(pace.wait().min(Duration::from_millis(1)));
            }
            self.quieten();
            monkey.step(&self.app, self.ui.window())?;
            // Answers from the mock system's thread, if they come quickly.
            let until = std::time::Instant::now() + Duration::from_millis(2);
            loop {
                self.turn();
                if std::time::Instant::now() >= until {
                    break;
                }
                std::thread::sleep(Duration::from_micros(200));
            }
            if frames.due() {
                self.render();
            }
        }
        self.render();
        Ok(monkey)
    }

    pub fn ui(&self) -> &Dash {
        &self.ui
    }

    /// The dashboard behind the UI (for `probe.rs`).
    pub(super) fn app(&self) -> &Rc<App> {
        &self.app
    }

    /// Hand the dashboard an event, as the system would, and let it show it.
    pub fn send(&mut self, event: impl Into<Event>) {
        let _ = self.events.send(event.into());
        self.pump();
    }

    /// Deliver what is pending and run Slint's timers and animations once.
    pub fn pump(&mut self) {
        let _ = self
            .event_loop
            .dispatch(Some(Duration::ZERO), &mut self.controls);
        slint::platform::update_timers_and_animations();
        self.app.keep_keyboard();
    }

    /// The controls (hide, show, …) the dashboard asked of its surface since the last call, in
    /// order. The surface isn't there, so they only pile up here.
    pub fn controls(&mut self) -> Vec<Control> {
        let mut all: Vec<Control> = self.app.pending.borrow_mut().drain(..).collect();
        all.append(&mut self.controls);
        all
    }

    /// Press and release a key: a character, or one of Slint's special keys.
    pub fn key(&mut self, key: impl Into<SharedString>) {
        let text = key.into();
        let window = self.ui.window();
        window.dispatch_event(WindowEvent::KeyPressed { text: text.clone() });
        window.dispatch_event(WindowEvent::KeyReleased { text });
        self.pump();
    }

    /// Press and release a special key.
    pub fn special(&mut self, key: Key) {
        self.key(key)
    }

    /// Click at a point, in logical pixels.
    pub fn click(&mut self, x: f32, y: f32) {
        let position = LogicalPosition::new(x, y);
        let button = PointerEventButton::Left;
        let window = self.ui.window();
        window.dispatch_event(WindowEvent::PointerMoved { position });
        window.dispatch_event(WindowEvent::PointerPressed { position, button });
        window.dispatch_event(WindowEvent::PointerReleased { position, button });
        self.pump();
    }
}

pub use super::a11y::{Element, Spoken};
pub use super::probe::{Tile, can_be_round, kind_names, min_size, setting_labels, ui_kind};
pub use super::stress::{Failure, Monkey, Sandbox};

/// The text of the default config.kdl (what a first start writes).
pub fn default_config() -> String {
    Layout::default_text()
}

//! Driving the dashboard headlessly, for the UI tests (`tests/ui.rs`): the real dashboard on a
//! window that only exists in memory, with channels standing in for the system. A test reads what
//! the dashboard sent from the [`Inboxes`], hands it [`Event`]s as the system would, and uses the
//! UI through its globals or through key and pointer events.
//!
//! Slint allows one platform per process and its objects stay on the thread that made them, so a
//! test binary runs its scenarios one after another on one thread; each gets a fresh dashboard.
//! The dashboard reads and writes its config and state under the XDG directories, which the test
//! points somewhere disposable before the first harness.

use super::a11y::bridge::{Request, TreeSink};
use super::{App, Input, build};
use crate::layout::{Layout, Loaded};
use crate::{Control, Dash};
use niri_dash_system::{Demo, Event, Inboxes, MockSystem, Started, channels};
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
    /// When the harness started, for the clock Slint's timers and animations run on.
    static STARTED: std::time::Instant = std::time::Instant::now();
    /// A clock that only moves when told to ([`Harness::stress`]), or None for the real one.
    static CLOCK: std::cell::Cell<Option<Duration>> = const { std::cell::Cell::new(None) };
    /// How far the real clock is behind where a stress run took the other one: time never goes
    /// back.
    static AHEAD: std::cell::Cell<Duration> = const { std::cell::Cell::new(Duration::ZERO) };
}

impl Platform for TestPlatform {
    /// What Slint's timers (a button that disarms after a while) and animations run on. A stress
    /// run moves it by hand, a fixed time per key: its keys come far faster than a person's, and
    /// on the real clock the same keys would find a timer run out or an animation half done
    /// depending on how fast the machine was.
    fn duration_since_start(&self) -> Duration {
        clock_now()
    }

    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
        WINDOW.with(|w| *w.borrow_mut() = Some(window.clone()));
        Ok(window)
    }
}

/// What a screen reader would have been given, as [`Harness::access_events`] returns it.
#[derive(Debug, Clone)]
pub enum AccessEvent {
    /// A tree, or a change to it (always the whole tree, so that ids stay put).
    Update(accesskit::TreeUpdate),
    /// Whether the window has the keyboard.
    WindowFocus(bool),
}

/// A screen reader's sink that only writes down what it gets.
struct Recorder(Rc<RefCell<Vec<AccessEvent>>>);

impl TreeSink for Recorder {
    fn update(&mut self, update: accesskit::TreeUpdate) {
        self.0.borrow_mut().push(AccessEvent::Update(update));
    }

    fn window_focus(&mut self, focused: bool) {
        self.0.borrow_mut().push(AccessEvent::WindowFocus(focused));
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
    /// Where the dashboard's own requests (a screen reader's) are sent.
    input: niri_dash_system::Sink<Input>,
    /// What a screen reader was given since the last look.
    access_log: Rc<RefCell<Vec<AccessEvent>>>,
    window: Rc<MinimalSoftwareWindow>,
    pixels: Vec<PremultipliedRgbaColor>,
    /// The window's width in pixels.
    width: usize,
    /// How many events the dashboard has been handed.
    delivered: Rc<std::cell::Cell<u64>>,
    /// The mock system of [`Harness::with_mock`] and where its commands wait. It answers here, on
    /// the harness's own thread, whenever the dashboard takes a turn: on a thread of its own it
    /// would answer when the machine let it, and the same keys wouldn't give the same run.
    mock: Option<(MockSystem, Inboxes)>,
}

/// The time Slint's timers and animations run on.
fn clock_now() -> Duration {
    CLOCK
        .with(|c| c.get())
        .unwrap_or_else(|| STARTED.with(|s| s.elapsed()) + AHEAD.with(|a| a.get()))
}

/// How long a person takes over a key, on the clock a stress run keeps.
const KEY_TIME: Duration = Duration::from_millis(120);

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
        let (senders, commands) = channels();
        let system = Started {
            senders,
            caps: MockSystem::CAPS,
        };
        let mut me = Self::on(config, system, channels().1, size);
        me.mock = Some((MockSystem::new(Demo::new(), events), commands));
        me.listen(events_rx);
        // The mock sends what it has once the dashboard is up.
        me.app.set_visible(true);
        me.pump();
        me
    }

    /// The dashboard on the simulated system (`NIRI_DASH_SIM=1`): a made-up machine that keeps
    /// moving, as the web version runs on. For soak runs and measuring.
    pub fn with_sim(config: &str, size: (u32, u32)) -> Self {
        let (events, events_rx) = super::loop_channel::<Event>();
        let system = niri_dash_system::simulated(events).expect("the simulated system");
        let mut me = Self::on(config, system, channels().1, size);
        me.listen(events_rx);
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
        let app = build(&ui, system, input_tx.clone(), None, loaded, true);
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
            input: input_tx,
            access_log: Default::default(),
            window,
            pixels: vec![PremultipliedRgbaColor::default(); (size.0 * size.1) as usize],
            width: size.0 as usize,
            delivered: Default::default(),
            mock: None,
        };
        me.listen(events_rx);
        me.listen_inputs(inputs);
        me.pump();
        me
    }

    /// Answer what the dashboard sends itself: here, a screen reader's requests.
    fn listen_inputs(&mut self, inputs: channel::Channel<Input>) {
        let a = self.app.clone();
        self.event_loop
            .handle()
            .insert_source(inputs, move |ev, _, controls: &mut Vec<Control>| {
                if let channel::Event::Msg(i) = ev
                    && let Some(c) = a.handle_input(i)
                {
                    controls.push(c);
                }
            })
            .expect("inserting the input source");
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

    /// A screen reader that only writes down what it is given (see [`Harness::access_events`]),
    /// not yet listening: [`Harness::access_activate`] is it arriving.
    pub fn access_connect(&mut self) {
        self.access_log.borrow_mut().clear();
        self.app
            .access_attach(Box::new(Recorder(self.access_log.clone())));
    }

    /// Publish on the accessibility bus for real, as the daemon does. Only for `tests/atspi.rs`,
    /// which runs under a private session bus of its own: it says so with
    /// `NIRI_DASH_PRIVATE_BUS`, and without it this refuses, so that no test can reach the
    /// session's bus by mistake.
    pub fn access_connect_unix(&mut self) {
        assert!(
            std::env::var_os("NIRI_DASH_PRIVATE_BUS").is_some(),
            "refusing to use the session's accessibility bus from a test"
        );
        self.app
            .access_attach(super::a11y::bridge::unix(self.input.clone()));
    }

    /// The screen reader arrives, asking for the tree.
    pub fn access_activate(&mut self) {
        self.access_send(Request::Activated);
    }

    /// The screen reader leaves.
    pub fn access_deactivate(&mut self) {
        self.access_send(Request::Deactivated);
    }

    /// The screen reader asks an element for something.
    pub fn access_do(&mut self, request: accesskit::ActionRequest) {
        self.access_send(Request::Action(request));
    }

    fn access_send(&mut self, request: Request) {
        let _ = self.input.send(Input::Access(request));
        self.pump();
    }

    /// What the screen reader was given since the last call, in order.
    pub fn access_events(&mut self) -> Vec<AccessEvent> {
        std::mem::take(&mut *self.access_log.borrow_mut())
    }

    /// The dashboard as the tree a screen reader's adapter is given.
    pub fn access_tree(&self) -> accesskit::TreeUpdate {
        self.app.access_tree()
    }

    /// Everything in the window a screen reader could read out.
    pub fn readable(&self) -> Vec<String> {
        self.app.readable()
    }

    /// Draw what changed since the last frame, as the daemon would.
    pub fn render(&mut self) {
        crate::shell::render_into(&self.window, &mut self.pixels, self.width, false);
    }

    /// How many events from the system have reached the dashboard.
    pub fn delivered(&self) -> u64 {
        self.delivered.get()
    }

    /// Draw what changed (or, with `full`, everything), and say what was repainted: the bounding
    /// box (x, y, width, height) and the rectangles it is made of. For measuring.
    #[allow(clippy::type_complexity)]
    pub fn draw(
        &mut self,
        full: bool,
    ) -> Option<((i32, i32, u32, u32), Vec<(i32, i32, u32, u32)>)> {
        use slint::platform::software_renderer::RepaintBufferType;
        if full {
            self.window.request_redraw();
        }
        let mut out = None;
        let (pixels, width) = (&mut self.pixels, self.width);
        self.window.draw_if_needed(|renderer| {
            renderer.set_repaint_buffer_type(if full {
                RepaintBufferType::NewBuffer
            } else {
                RepaintBufferType::ReusedBuffer
            });
            let region = renderer.render(pixels, width);
            let b = (
                region.bounding_box_origin().x,
                region.bounding_box_origin().y,
                region.bounding_box_size().width,
                region.bounding_box_size().height,
            );
            let rects = region
                .iter()
                .map(|(o, s)| (o.x, o.y, s.width, s.height))
                .collect();
            out = Some((b, rects));
        });
        out
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
    /// Frames are drawn ten times a second of keys, as a screen would show them, however fast the
    /// keys come (at full speed, one in 75). Gives the monkey back for its summary, or what went wrong.
    pub fn stress(
        &mut self,
        monkey: super::stress::Monkey,
        ops: u64,
        rate: f64,
    ) -> Result<super::stress::Monkey, super::stress::Failure> {
        let monkey = monkey.instant_looks();
        let mut pace = super::stress::Pace::new(rate);
        // A frame every tenth of a second: by the key, so that the same keys draw the same frames
        // (the clock decides when a frame lands, on a machine that is busy or slow).
        let every = if rate > 0.0 {
            (rate / 10.0).ceil() as u64
        } else {
            75
        };
        // From here on, time passes by the key, not by the clock.
        let start = clock_now();
        CLOCK.with(|c| c.set(Some(start)));
        let result = self.stress_keys(monkey, ops, &mut pace, every);
        let end = CLOCK.with(|c| c.take()).unwrap_or(start);
        AHEAD.with(|a| a.set(end.saturating_sub(STARTED.with(|s| s.elapsed()))));
        result
    }

    fn stress_keys(
        &mut self,
        mut monkey: super::stress::Monkey,
        ops: u64,
        pace: &mut super::stress::Pace,
        every: u64,
    ) -> Result<super::stress::Monkey, super::stress::Failure> {
        while monkey.done() < ops {
            CLOCK.with(|c| c.set(c.get().map(|t| t + KEY_TIME)));
            // The dashboard's loop runs on between keys.
            while !pace.due() {
                self.turn();
                std::thread::sleep(pace.wait().min(Duration::from_millis(1)));
            }
            self.quieten();
            monkey.step(&self.app, self.ui.window())?;
            self.turn();
            if monkey.done().is_multiple_of(every) {
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
        self.answer_mock();
        let _ = self
            .event_loop
            .dispatch(Some(Duration::ZERO), &mut self.controls);
        slint::platform::update_timers_and_animations();
        self.answer_mock();
        self.app.keep_keyboard();
        self.app.access_sync(false);
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

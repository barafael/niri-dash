//! The dashboard in a browser. It is the same dashboard: this file is to a canvas and the page's
//! events what `shell.rs` and `gpu.rs` are to a layer surface and the seat. FemtoVG draws through
//! WebGL, keys and pointers become the same Slint window events, a frame is a requestAnimationFrame,
//! and the system is the simulated one, as a page has no D-Bus and no compositor to ask.
//!
//! The canvas is the page's only content and covers it. What is under it plays the desktop: going
//! away (Escape, a window focused) uncovers it, and any key or click brings the dashboard back, as
//! the hotkey would.

use super::{App, Surface, build, new_ui};
use crate::layout::Layout;
use crate::{Clipboard as UiClipboard, Control, Dash, Nav, NavMode, PointerShape, Theme};
use i_slint_core::input::{BackendMouseEvent, TouchPhase};
use i_slint_core::lengths::LogicalPoint;
use i_slint_core::platform::InternalEvent;
use i_slint_renderer_femtovg::{
    FemtoVGOpenGLRenderer, FemtoVGOpenGLRendererExt, FemtoVGRendererExt,
};
use niri_dash_system as sys;
use slint::platform::{
    Clipboard, Key, Platform, PointerEventButton, Renderer, WindowAdapter, WindowEvent,
};
use slint::{
    ComponentHandle, LogicalPosition, PhysicalSize, PlatformError, SharedString, Window, WindowSize,
};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use wasm_bindgen::convert::FromWasmAbi;
use wasm_bindgen::prelude::*;
use web_sys::{
    AddEventListenerOptions, ClipboardEvent, EventTarget, HtmlCanvasElement, HtmlElement,
    KeyboardEvent, PointerEvent, WheelEvent,
};

/// A Slint window drawn by FemtoVG on a canvas; the browser's counterpart of `GpuWindow`.
struct CanvasWindow {
    window: Window,
    renderer: FemtoVGOpenGLRenderer,
    size: Cell<PhysicalSize>,
    needs_redraw: Cell<bool>,
}

impl CanvasWindow {
    fn new(canvas: HtmlCanvasElement) -> Result<Rc<Self>, PlatformError> {
        let renderer = FemtoVGOpenGLRenderer::new_suspended();
        renderer.set_opengl_context(canvas)?;
        Ok(Rc::new_cyclic(|w: &Weak<Self>| Self {
            window: Window::new(w.clone()),
            renderer,
            size: Default::default(),
            needs_redraw: Cell::new(true),
        }))
    }

    /// Draw if anything changed. Returns whether it did.
    fn draw_if_needed(&self) -> bool {
        if !self.needs_redraw.replace(false) {
            return false;
        }
        if let Err(e) = self.renderer.render() {
            log::warn!("render failed: {e}");
            return false;
        }
        true
    }
}

impl WindowAdapter for CanvasWindow {
    fn window(&self) -> &Window {
        &self.window
    }
    fn renderer(&self) -> &dyn Renderer {
        &self.renderer
    }
    fn size(&self) -> PhysicalSize {
        self.size.get()
    }
    fn set_size(&self, size: WindowSize) {
        let sf = self.window.scale_factor();
        self.size.set(size.to_physical(sf));
        self.window.dispatch_event(WindowEvent::Resized {
            size: size.to_logical(sf),
        });
    }
    fn request_redraw(&self) {
        self.needs_redraw.set(true);
    }
}

/// The one window there is, and the clipboard text widgets copy and paste through. A page can only
/// write the browser's clipboard whenever it likes; reading happens when a paste event brings the
/// text.
struct WebPlatform {
    window: Rc<CanvasWindow>,
    clip: Rc<RefCell<String>>,
}

impl Platform for WebPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }

    fn set_clipboard_text(&self, text: &str, clipboard: Clipboard) {
        if clipboard == Clipboard::DefaultClipboard {
            *self.clip.borrow_mut() = text.to_string();
            if let Some(w) = web_sys::window() {
                let _ = w.navigator().clipboard().write_text(text);
            }
        }
    }

    fn clipboard_text(&self, clipboard: Clipboard) -> Option<String> {
        (clipboard == Clipboard::DefaultClipboard).then(|| self.clip.borrow().clone())
    }
}

/// Where the dashboard appears: the canvas, with the page under it when it is away.
struct Page {
    body: HtmlElement,
    window: Rc<CanvasWindow>,
    visible: bool,
    presented: u32,
}

impl Surface for Page {
    fn is_visible(&self) -> bool {
        self.visible
    }

    fn is_configured(&self) -> bool {
        self.visible
    }

    fn presented(&self) -> u32 {
        self.presented
    }

    fn show(&mut self) {
        self.visible = true;
        self.presented = 0;
        let _ = self.body.class_list().remove_2("away", "loading");
        self.window.request_redraw();
    }

    fn hide(&mut self) {
        self.visible = false;
        let _ = self.body.class_list().add_1("away");
    }
}

/// The size of the grid if there are no widgets to go by.
const DEFAULT_GRID: (i32, i32) = (16, 9);
/// The room around the grid: `margin` and `bar` in ui/app.slint.
const GRID_MARGIN: f64 = 24.0;
const GRID_BAR: f64 = 44.0;
/// How far the dashboard scales to fit the page.
const ZOOM: (f64, f64) = (0.25, 3.0);

/// How long a touchpad has to be still for its gesture to count as over, in milliseconds.
const GESTURE_END: f64 = 140.0;

/// A touchpad's scrolling in progress: where it is, and when the last of it came.
struct Gesture {
    position: LogicalPosition,
    last: f64,
}

/// Everything the page's events and frames work on.
struct Dashboard {
    ui: Dash,
    app: Rc<App>,
    page: RefCell<Page>,
    window: Rc<CanvasWindow>,
    canvas: HtmlCanvasElement,
    clip: Rc<RefCell<String>>,
    /// The keys held that Slint counts as modifiers, to let go of them when the page loses focus.
    modifiers: RefCell<Vec<SharedString>>,
    /// The canvas's size in pixels and how big the dashboard is drawn, as last laid out.
    frame: Cell<(u32, u32, f32)>,
    /// How much of the page's pixels the dashboard's logical pixels take.
    zoom: Cell<f32>,
    cursor: Cell<PointerShape>,
    glass: Cell<bool>,
    gesture: RefCell<Option<Gesture>>,
    /// Sees the pointer on its way to the UI, for long presses.
    hold: Box<dyn Fn(&WindowEvent)>,
}

impl Dashboard {
    /// A pointer event, as far as a press and hold and the UI are concerned.
    fn pointer(&self, event: WindowEvent) {
        (self.hold)(&event);
        self.slint().dispatch_event(event);
    }

    fn route(&self, c: Control) {
        self.app.control(&mut *self.page.borrow_mut(), c);
    }

    fn slint(&self) -> &Window {
        self.window.window()
    }

    /// How much to zoom for all of the layout to fit a page of this size. On a screen the
    /// automatic grid has as many cells as fit and hides widgets beyond them, which suits a
    /// monitor one has picked a layout for. A page is whatever size the visitor's window is, so
    /// the layout stays whole and the dashboard scales to it instead, like a slide.
    fn zoom_for(&self, page_width: f64, page_height: f64) -> f64 {
        let layout = self.app.layout.borrow();
        let extent = |f: fn(&crate::layout::Placed) -> i32| layout.widgets.iter().map(f).max();
        let columns = extent(|p| p.col + p.w).unwrap_or(DEFAULT_GRID.0);
        let rows = extent(|p| p.row + p.h).unwrap_or(DEFAULT_GRID.1);
        let cell = f64::from(crate::layout::AUTO_CELL);
        let width = f64::from(columns) * cell + 2.0 * GRID_MARGIN;
        let height = f64::from(rows) * cell + GRID_BAR + GRID_MARGIN;
        (page_width / width)
            .min(page_height / height)
            .clamp(ZOOM.0, ZOOM.1)
    }

    /// Lay out for the canvas's size, the screen's scale and the layout's extent, if any changed.
    fn fit(&self) {
        let Some(w) = web_sys::window() else { return };
        let (page_width, page_height) = (
            f64::from(self.canvas.client_width()),
            f64::from(self.canvas.client_height()),
        );
        let density = w.device_pixel_ratio();
        let width = (page_width * density).round() as u32;
        let height = (page_height * density).round() as u32;
        let zoom = self.zoom_for(page_width, page_height);
        // Slightly coarse, as tiny changes aren't worth laying everything out again for.
        let scale = ((density * zoom * 256.0).floor() / 256.0) as f32;
        if width == 0
            || height == 0
            || self.frame.replace((width, height, scale)) == (width, height, scale)
        {
            return;
        }
        self.zoom.set(scale / density as f32);
        self.canvas.set_width(width);
        self.canvas.set_height(height);
        self.slint()
            .dispatch_event(WindowEvent::ScaleFactorChanged {
                scale_factor: scale,
            });
        self.slint().set_size(PhysicalSize::new(width, height));
        self.window.request_redraw();
    }

    /// Where an event happened, in the dashboard's logical pixels.
    fn position(&self, ev: &web_sys::MouseEvent) -> LogicalPosition {
        let zoom = self.zoom.get();
        LogicalPosition::new(ev.offset_x() as f32 / zoom, ev.offset_y() as f32 / zoom)
    }

    /// One turn of the event loop, as often as the page repaints.
    fn frame(&self, now: f64) {
        slint::platform::update_timers_and_animations();
        self.app.keep_keyboard();
        self.end_gesture(now);
        let pending: Vec<Control> = self.app.pending.borrow_mut().drain(..).collect();
        for c in pending {
            self.route(c);
        }
        self.app.tick(&mut *self.page.borrow_mut());
        if !self.page.borrow().visible {
            return;
        }
        self.fit();
        self.app.sync_grid();
        self.follow_ui();
        if self.window.draw_if_needed() {
            self.page.borrow_mut().presented += 1;
        }
    }

    /// The pointer shape and frosted glass the UI asks for, as far as the page does those.
    fn follow_ui(&self) {
        let shape = self.ui.get_cursor();
        if self.cursor.replace(shape) != shape {
            let css = match shape {
                PointerShape::Normal => "default",
                PointerShape::Move => "move",
                PointerShape::ResizeEw => "ew-resize",
                PointerShape::ResizeNs => "ns-resize",
                PointerShape::ResizeNwse => "nwse-resize",
                PointerShape::ResizeNesw => "nesw-resize",
            };
            let _ = self.canvas.style().set_property("cursor", css);
        }
        let glass = self.ui.global::<Theme>().get_glass();
        if self.glass.replace(glass) != glass {
            let classes = self.page.borrow().body.class_list();
            let _ = classes.toggle_with_force("glass", glass);
        }
    }

    fn wheel_phase(&self, position: LogicalPosition, dx: f32, dy: f32, phase: TouchPhase) {
        let event = BackendMouseEvent::Wheel {
            position: LogicalPoint::new(position.x, position.y),
            delta_x: dx,
            delta_y: dy,
            phase,
        };
        self.slint()
            .dispatch_event(WindowEvent::internal(InternalEvent::Mouse(event)));
    }

    /// A browser doesn't say when fingers leave the touchpad, so a pause ends the gesture.
    fn end_gesture(&self, now: f64) {
        let over = self
            .gesture
            .borrow()
            .as_ref()
            .filter(|g| now - g.last > GESTURE_END)
            .map(|g| g.position);
        if let Some(position) = over {
            *self.gesture.borrow_mut() = None;
            self.wheel_phase(position, 0.0, 0.0, TouchPhase::Ended);
        }
    }

    /// Scrolling. Slint scrolls content by +delta; a page's wheel events are the opposite way. A
    /// wheel gives big steps, which Slint animates; a touchpad gives many small ones, which it
    /// should follow directly and let glide at the end.
    fn wheel(&self, ev: &WheelEvent) {
        const LINE: f64 = 16.0;
        const PAGE: f64 = 400.0;
        // A wheel's notch is about 100 pixels here and 48 on the desktop.
        const WHEEL: f64 = 0.5;
        let unit = match ev.delta_mode() {
            WheelEvent::DOM_DELTA_LINE => LINE,
            WheelEvent::DOM_DELTA_PAGE => PAGE,
            _ => 1.0,
        };
        let (dx, dy) = (-ev.delta_x() * unit, -ev.delta_y() * unit);
        let position = self.position(ev);
        let touchpad = ev.delta_mode() == WheelEvent::DOM_DELTA_PIXEL
            && dx.abs().max(dy.abs()) < 48.0
            && (dx.fract() != 0.0 || dy.fract() != 0.0 || self.gesture.borrow().is_some());
        if !touchpad {
            self.slint().dispatch_event(WindowEvent::PointerScrolled {
                position,
                delta_x: (dx * WHEEL) as f32,
                delta_y: (dy * WHEEL) as f32,
            });
            return;
        }
        if self.gesture.borrow().is_none() {
            self.wheel_phase(position, 0.0, 0.0, TouchPhase::Started);
        }
        *self.gesture.borrow_mut() = Some(Gesture {
            position,
            last: ev.time_stamp(),
        });
        self.wheel_phase(position, dx as f32, dy as f32, TouchPhase::Moved);
    }

    fn key(&self, ev: &KeyboardEvent, pressed: bool) {
        if ev.meta_key() && ev.key() != "Meta" {
            // The browser's and the desktop's.
            return;
        }
        let Some(text) = key_text(&ev.key(), ev.shift_key()) else {
            return;
        };
        let away = !self.page.borrow().visible;
        if away {
            // As the hotkey would, except that a stray Escape stays away.
            let modifier = matches!(ev.key().as_str(), "Shift" | "Control" | "Alt" | "Meta");
            if pressed && !modifier && ev.key() != "Escape" && !ev.repeat() {
                ev.prevent_default();
                self.route(Control::Show);
            }
            return;
        }
        let ctrl = ev.ctrl_key();
        let lower = ev.key().to_ascii_lowercase();
        // With nothing entered, Ctrl+R is free for the browser, where it reloads. Pasting comes as
        // a paste event, which only happens if the browser gets to see the key.
        let entered = self.ui.global::<Nav>().get_mode() != NavMode::Off;
        if ctrl && ((lower == "r" && !entered) || lower == "v") {
            return;
        }
        // The browser keeps the zoom keys.
        if ctrl && matches!(lower.as_str(), "+" | "-" | "=" | "0") {
            return;
        }
        ev.prevent_default();
        let modifier = matches!(ev.key().as_str(), "Shift" | "Control" | "Alt" | "Meta");
        if modifier {
            let mut held = self.modifiers.borrow_mut();
            held.retain(|m| *m != text);
            if pressed {
                held.push(text.clone());
            }
        }
        self.slint().dispatch_event(if !pressed {
            WindowEvent::KeyReleased { text }
        } else if ev.repeat() {
            WindowEvent::KeyPressRepeated { text }
        } else {
            WindowEvent::KeyPressed { text }
        });
    }

    /// The page lost the keyboard: whatever was held won't be seen let go, so let go of it now.
    fn blur(&self) {
        let held: Vec<SharedString> = self.modifiers.borrow_mut().drain(..).collect();
        for text in held {
            self.slint()
                .dispatch_event(WindowEvent::KeyReleased { text });
        }
        self.slint()
            .dispatch_event(WindowEvent::WindowActiveChanged(false));
    }

    /// Text arrived from the browser's clipboard: put it where widgets paste from, and press the
    /// key that the browser kept for itself.
    fn paste(&self, ev: &ClipboardEvent) {
        let Some(text) = ev.clipboard_data().and_then(|d| d.get_data("text").ok()) else {
            return;
        };
        *self.clip.borrow_mut() = text;
        let v: SharedString = "v".into();
        self.slint()
            .dispatch_event(WindowEvent::KeyPressed { text: v.clone() });
        self.slint()
            .dispatch_event(WindowEvent::KeyReleased { text: v });
    }
}

/// A key as Slint names it, as the shell does for Wayland's; None for keys the browser should keep
/// (function keys, media keys and so on).
fn key_text(key: &str, shift: bool) -> Option<SharedString> {
    let special = match key {
        "Escape" => Key::Escape,
        "Enter" => Key::Return,
        "Backspace" => Key::Backspace,
        "Tab" if shift => Key::Backtab,
        "Tab" => Key::Tab,
        "Delete" => Key::Delete,
        "ArrowLeft" => Key::LeftArrow,
        "ArrowRight" => Key::RightArrow,
        "ArrowUp" => Key::UpArrow,
        "ArrowDown" => Key::DownArrow,
        "Home" => Key::Home,
        "End" => Key::End,
        "PageUp" => Key::PageUp,
        "PageDown" => Key::PageDown,
        "Shift" => Key::Shift,
        "Control" => Key::Control,
        "Alt" => Key::Alt,
        "Meta" => Key::Meta,
        _ => {
            let mut chars = key.chars();
            return match (chars.next(), chars.next()) {
                (Some(c), None) if !c.is_control() => Some(key.into()),
                _ => None,
            };
        }
    };
    Some(special.into())
}

fn button(b: i16) -> PointerEventButton {
    match b {
        0 => PointerEventButton::Left,
        1 => PointerEventButton::Middle,
        2 => PointerEventButton::Right,
        3 => PointerEventButton::Back,
        4 => PointerEventButton::Forward,
        _ => PointerEventButton::Other,
    }
}

/// Call `f` for each `name` event on `target`, for as long as the page lives.
fn listen<E>(target: &EventTarget, name: &str, f: impl FnMut(E) + 'static)
where
    E: FromWasmAbi + 'static,
{
    let closure = Closure::<dyn FnMut(E)>::new(f);
    // Not passive: these handlers keep the browser from scrolling and the like.
    let options = AddEventListenerOptions::new();
    options.set_passive(false);
    let added = target.add_event_listener_with_callback_and_add_event_listener_options(
        name,
        closure.as_ref().unchecked_ref(),
        &options,
    );
    if added.is_err() {
        log::warn!("not listening for {name}");
    }
    closure.forget();
}

fn wire_input(d: &Rc<Dashboard>, window: &web_sys::Window, document: &web_sys::Document) {
    let canvas: &EventTarget = d.canvas.as_ref();

    let a = d.clone();
    listen(window, "keydown", move |ev: KeyboardEvent| a.key(&ev, true));
    let a = d.clone();
    listen(window, "keyup", move |ev: KeyboardEvent| a.key(&ev, false));
    let a = d.clone();
    listen(window, "blur", move |_: web_sys::Event| a.blur());
    let a = d.clone();
    listen(window, "focus", move |_: web_sys::Event| {
        a.slint()
            .dispatch_event(WindowEvent::WindowActiveChanged(true));
    });
    let a = d.clone();
    listen(document, "paste", move |ev: ClipboardEvent| a.paste(&ev));

    let a = d.clone();
    listen(canvas, "pointermove", move |ev: PointerEvent| {
        a.pointer(WindowEvent::PointerMoved {
            position: a.position(&ev),
        });
    });
    let a = d.clone();
    listen(canvas, "pointerdown", move |ev: PointerEvent| {
        // A drag keeps going over the edge of the page.
        let _ = a.canvas.set_pointer_capture(ev.pointer_id());
        let position = a.position(&ev);
        a.pointer(WindowEvent::PointerMoved { position });
        a.pointer(WindowEvent::PointerPressed {
            position,
            button: button(ev.button()),
        });
    });
    let a = d.clone();
    listen(canvas, "pointerup", move |ev: PointerEvent| {
        a.pointer(WindowEvent::PointerReleased {
            position: a.position(&ev),
            button: button(ev.button()),
        });
    });
    let a = d.clone();
    listen(canvas, "pointerleave", move |_: PointerEvent| {
        a.pointer(WindowEvent::PointerExited);
    });
    let a = d.clone();
    listen(canvas, "wheel", move |ev: WheelEvent| {
        ev.prevent_default();
        a.wheel(&ev);
    });
    // A right-click opens edit mode, not the browser's menu.
    listen(canvas, "contextmenu", |ev: web_sys::Event| {
        ev.prevent_default();
    });
    // With the dashboard away, a click anywhere brings it back.
    let a = d.clone();
    listen(document, "pointerdown", move |ev: PointerEvent| {
        let on_link = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
            .is_some_and(|e| e.closest("a").ok().flatten().is_some());
        if !on_link && !a.page.borrow().visible {
            a.route(Control::Show);
        }
    });
}

/// Ask for `frame` on every repaint of the page, for as long as it lives.
fn run_frames(d: Rc<Dashboard>, window: web_sys::Window) {
    type Frame = Closure<dyn FnMut(f64)>;
    let next: Rc<RefCell<Option<Frame>>> = Rc::new(RefCell::new(None));
    let again = next.clone();
    let w = window.clone();
    *next.borrow_mut() = Some(Closure::new(move |now: f64| {
        d.frame(now);
        if let Some(f) = again.borrow().as_ref() {
            let _ = w.request_animation_frame(f.as_ref().unchecked_ref());
        }
    }));
    if let Some(f) = next.borrow().as_ref() {
        let _ = window.request_animation_frame(f.as_ref().unchecked_ref());
    }
}

/// Whether the page is taller than wide, as a phone's is.
pub(crate) fn is_portrait() -> bool {
    let size = |v: Result<JsValue, JsValue>| v.ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
    web_sys::window().is_some_and(|w| size(w.inner_height()) > size(w.inner_width()))
}

fn run() -> Result<(), String> {
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;
    let body = document.body().ok_or("no body")?;
    let canvas: HtmlCanvasElement = document
        .get_element_by_id("canvas")
        .ok_or("no canvas")?
        .dyn_into()
        .map_err(|_| "the canvas isn't one")?;

    let wants_reset = window
        .location()
        .search()
        .is_ok_and(|q| web_sys::UrlSearchParams::new_with_str(&q).is_ok_and(|q| q.has("reset")));
    if wants_reset {
        crate::files::clear();
    }

    let adapter = CanvasWindow::new(canvas.clone()).map_err(|e| e.to_string())?;
    let clip = Rc::new(RefCell::new(String::new()));
    slint::platform::set_platform(Box::new(WebPlatform {
        window: adapter.clone(),
        clip: clip.clone(),
    }))
    .map_err(|e| e.to_string())?;
    let ui = new_ui().map_err(|e| e.to_string())?;

    let (events_tx, mut events) = sys::unbounded_channel::<sys::Event>();
    let system = sys::simulated(events_tx).map_err(|e| e.to_string())?;
    let has_clipboard = system.caps.clipboard;
    let app = build(&ui, system, None, Layout::load(), false);
    ui.global::<UiClipboard>().set_available(has_clipboard);

    let dashboard = Rc::new(Dashboard {
        page: RefCell::new(Page {
            body,
            window: adapter.clone(),
            visible: false,
            presented: 0,
        }),
        window: adapter,
        canvas,
        clip,
        modifiers: Default::default(),
        frame: Cell::new((0, 0, 0.0)),
        zoom: Cell::new(1.0),
        cursor: Cell::new(PointerShape::Normal),
        glass: Cell::new(false),
        gesture: Default::default(),
        hold: super::hold::watcher(&app),
        app,
        ui,
    });
    wire_input(&dashboard, &window, &document);

    let d = dashboard.clone();
    wasm_bindgen_futures::spawn_local(async move {
        while let Some(event) = events.recv().await {
            if let Some(c) = d.app.handle_event(event) {
                d.route(c);
            }
        }
    });

    dashboard.fit();
    dashboard
        .slint()
        .dispatch_event(WindowEvent::WindowActiveChanged(true));
    dashboard.route(Control::Show);
    run_frames(dashboard, window);
    Ok(())
}

/// Bring up the dashboard on the page's canvas. Runs as long as the page.
pub fn start() {
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Info);
    if let Err(e) = run() {
        log::error!("niri-dash: {e}");
        if let Some(body) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.body())
        {
            let _ = body.class_list().add_1("failed");
        }
    }
}

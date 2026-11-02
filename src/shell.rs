//! The Wayland side: a fullscreen wlr-layer-shell surface on the overlay layer that hosts the Slint
//! UI (or, for a standalone app, an ordinary xdg-shell window), plus the clipboard through
//! wlr-data-control. Frames come from one of two renderers: FemtoVG draws with OpenGL ES straight
//! onto the surface and eglSwapBuffers commits it (`gpu`), or Slint's software renderer draws into
//! a pixel buffer that is already in wl_shm's ARGB8888 byte order, so presenting a frame is one
//! memcpy into a shm buffer. Pointer and keyboard input are translated into Slint window events.
//! The surface only exists while the dashboard is shown; the Slint component stays alive so state
//! and history survive hiding.

use crate::sources::Msg;
use crate::sources::clipboard::{self, Clip};
use i_slint_core::input::TouchPhase;
use slint::platform::software_renderer::RepaintBufferType;
use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, TargetPixel,
};
use slint::platform::{Key, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{LogicalPosition, PhysicalSize};
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::LoopHandle,
        client::{
            Connection, Dispatch, Proxy, QueueHandle,
            globals::GlobalList,
            protocol::{
                wl_keyboard, wl_output, wl_pointer, wl_region, wl_seat, wl_shm, wl_surface,
            },
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{
            BTN_LEFT, BTN_MIDDLE, BTN_RIGHT, CursorIcon, PointerEvent, PointerEventKind,
            PointerHandler, ThemeSpec, ThemedPointer,
        },
    },
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        xdg::{
            XdgShell,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{
        Shm, ShmHandler,
        slot::{Buffer, SlotPool},
    },
};
use std::io::{Read, Write};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use wayland_protocols::ext::background_effect::v1::client::{
    ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1,
    ext_background_effect_surface_v1::ExtBackgroundEffectSurfaceV1,
};
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
    wp_fractional_scale_v1::{self, WpFractionalScaleV1},
};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1},
    zwlr_data_control_manager_v1::ZwlrDataControlManagerV1,
    zwlr_data_control_offer_v1::{self, ZwlrDataControlOfferV1},
    zwlr_data_control_source_v1::{self, ZwlrDataControlSourceV1},
};

/// One pixel in wl_shm ARGB8888 layout (little endian: B, G, R, A), premultiplied.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Bgra {
    b: u8,
    g: u8,
    r: u8,
    a: u8,
}

impl TargetPixel for Bgra {
    fn blend(&mut self, c: PremultipliedRgbaColor) {
        let inv = 255 - c.alpha as u16;
        let mix = |dst: u8, src: u8| (src as u16 + (dst as u16 * inv + 127) / 255).min(255) as u8;
        self.r = mix(self.r, c.red);
        self.g = mix(self.g, c.green);
        self.b = mix(self.b, c.blue);
        self.a = mix(self.a, c.alpha);
    }
    fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self { b, g, r, a: 255 }
    }
    fn background() -> Self {
        Self::default()
    }
}

/// What kind of surface `show` creates.
pub enum Mode {
    /// The fullscreen dashboard on the overlay layer.
    Dashboard,
    /// A standalone app in a normal window.
    App {
        app_id: String,
        title: String,
        /// Logical size to use when the compositor leaves it to us.
        size: (u32, u32),
        min_size: (u32, u32),
    },
}

enum Role {
    Layer(LayerSurface),
    Toplevel(Window),
}

impl Role {
    fn wl_surface(&self) -> &wl_surface::WlSurface {
        match self {
            Role::Layer(l) => l.wl_surface(),
            Role::Toplevel(w) => w.wl_surface(),
        }
    }

    fn commit(&self) {
        match self {
            Role::Layer(l) => l.commit(),
            Role::Toplevel(w) => w.commit(),
        }
    }
}

struct Surface {
    role: Role,
    /// Absent without fractional-scale support; the buffer then has an integer scale.
    fractional: Option<WpFractionalScaleV1>,
    viewport: Option<WpViewport>,
    /// Background blur (frosted glass) for this surface, when the compositor offers it.
    effect: Option<ExtBackgroundEffectSurfaceV1>,
    /// Whether the blur region currently covers the surface.
    blurred: bool,
    /// Logical size from the last configure.
    width: u32,
    height: u32,
    configured: bool,
    frame_pending: bool,
}

impl Surface {
    /// Map the next buffer (`scale` times the logical size) onto the surface and ask for a frame
    /// callback; both apply with the next commit.
    fn prepare_frame(&self, qh: &QueueHandle<Shell>, scale: f64) {
        let wl = self.role.wl_surface();
        if let Some(v) = &self.viewport {
            v.set_destination(self.width as i32, self.height as i32);
        } else {
            wl.set_buffer_scale(scale as i32);
        }
        wl.frame(qh, FrameCallbackData(wl.clone()));
    }
}

/// Where the software renderer's frames go: rendered into `pixels`, then copied into whichever
/// shm buffer the compositor isn't reading.
struct ShmTarget {
    window: Rc<MinimalSoftwareWindow>,
    pool: SlotPool,
    buffers: Vec<Buffer>,
    pixels: Vec<Bgra>,
    /// Changed area (x0, y0, x1, y1) not yet presented to the compositor.
    damage: Option<(i32, i32, i32, i32)>,
}

/// How frames reach the surface.
enum Output {
    Shm(ShmTarget),
    /// FemtoVG draws, eglSwapBuffers attaches and commits.
    Egl(Rc<crate::gpu::GpuWindow>, Rc<crate::gpu::Gl>),
}

/// Mime types announced by a clipboard offer.
#[derive(Default)]
pub struct OfferMimes(Mutex<Vec<String>>);

pub struct Shell {
    pub conn: Connection,
    qh: QueueHandle<Shell>,
    data_control: Option<ZwlrDataControlManagerV1>,
    data_device: Option<ZwlrDataControlDeviceV1>,
    /// The offer behind the current selection; destroyed when the next one arrives.
    current_offer: Option<ZwlrDataControlOfferV1>,
    /// Our own clipboard contents while we own the selection.
    own_source: Option<(ZwlrDataControlSourceV1, Clip)>,
    /// Where new clipboard contents are reported.
    pub clip_tx: Option<Sender<Msg>>,
    registry_state: RegistryState,
    seat_state: SeatState,
    output_state: OutputState,
    compositor: CompositorState,
    layer_shell: Option<LayerShell>,
    xdg_shell: Option<XdgShell>,
    mode: Mode,
    shm: Shm,
    fractional_manager: Option<WpFractionalScaleManagerV1>,
    background_effect: Option<ExtBackgroundEffectManagerV1>,
    glass: bool,
    viewporter: Option<WpViewporter>,
    loop_handle: LoopHandle<'static, Shell>,
    surface: Option<Surface>,
    pointer: Option<ThemedPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    cursor: CursorIcon,
    /// A touchpad scroll gesture is in progress (between the first event and axis stop).
    scroll_gesture: bool,
    /// Last scale the compositor asked for; reused for the first frame after showing again.
    scale: f64,
    /// The Slint window, whichever renderer draws it.
    window: Rc<dyn WindowAdapter>,
    output: Output,
    /// Physical size the window was last laid out (and the software frame allocated) for; (0, 0)
    /// after hiding, so the next frame starts afresh.
    frame_size: (u32, u32),
    /// When the last GPU frame was drawn, for `MIN_FRAME_GAP`.
    last_frame: Option<std::time::Instant>,
    /// Frames presented since the surface was created.
    pub presented: u32,
    /// Set when the compositor closed our surface (output went away etc).
    pub closed_by_compositor: bool,
}

/// The compositor lacks something the dashboard needs.
#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    #[error("{0}: {1}")]
    Bind(
        &'static str,
        #[source] smithay_client_toolkit::reexports::client::globals::BindError,
    ),
    #[error("shm pool: {0}")]
    Pool(#[from] smithay_client_toolkit::shm::CreatePoolError),
}

impl Shell {
    pub fn new(
        conn: Connection,
        globals: &GlobalList,
        qh: QueueHandle<Shell>,
        loop_handle: LoopHandle<'static, Shell>,
        backend: Backend,
        mode: Mode,
    ) -> Result<Self, ShellError> {
        let compositor = CompositorState::bind(globals, &qh)
            .map_err(|e| ShellError::Bind("wl_compositor", e))?;
        let (layer_shell, xdg_shell) =
            match mode {
                Mode::Dashboard => (
                    Some(LayerShell::bind(globals, &qh).map_err(|e| {
                        ShellError::Bind("the compositor has no wlr-layer-shell", e)
                    })?),
                    None,
                ),
                Mode::App { .. } => (
                    None,
                    Some(
                        XdgShell::bind(globals, &qh)
                            .map_err(|e| ShellError::Bind("the compositor has no xdg-shell", e))?,
                    ),
                ),
            };
        let shm = Shm::bind(globals, &qh).map_err(|e| ShellError::Bind("wl_shm", e))?;
        let window = backend.adapter();
        let output = match backend {
            Backend::Software(window) => Output::Shm(ShmTarget {
                window,
                pool: SlotPool::new(2560 * 1600 * 4, &shm)?,
                buffers: Vec::new(),
                pixels: Vec::new(),
                damage: None,
            }),
            Backend::Gpu(window, gl) => Output::Egl(window, gl),
        };
        let fractional_manager = globals
            .bind::<WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ())
            .ok();
        let viewporter = globals.bind::<WpViewporter, _, _>(&qh, 1..=1, ()).ok();
        let background_effect = globals
            .bind::<ExtBackgroundEffectManagerV1, _, _>(&qh, 1..=1, ())
            .ok();
        let data_control = globals
            .bind::<ZwlrDataControlManagerV1, _, _>(&qh, 1..=2, ())
            .ok();
        if data_control.is_none() {
            log::warn!("no wlr-data-control; clipboard history is unavailable");
        }
        if fractional_manager.is_none() || viewporter.is_none() {
            log::warn!("no fractional-scale/viewporter; rendering at integer scale");
        }
        Ok(Self {
            conn,
            registry_state: RegistryState::new(globals),
            seat_state: SeatState::new(globals, &qh),
            output_state: OutputState::new(globals, &qh),
            qh,
            compositor,
            layer_shell,
            xdg_shell,
            mode,
            shm,
            fractional_manager,
            background_effect,
            glass: false,
            viewporter,
            data_control,
            data_device: None,
            current_offer: None,
            own_source: None,
            clip_tx: None,
            loop_handle,
            surface: None,
            pointer: None,
            keyboard: None,
            cursor: CursorIcon::Default,
            scroll_gesture: false,
            scale: 1.0,
            window,
            output,
            frame_size: (0, 0),
            presented: 0,
            last_frame: None,
            closed_by_compositor: false,
        })
    }

    /// Size the Slint window for an output before the surface exists, so the first frame is
    /// laid out for the real screen (the compositor's configure then matches).
    pub fn prepare(&mut self, width: u32, height: u32, scale: f64) {
        if width == 0 || height == 0 || scale <= 0.0 {
            return;
        }
        self.scale = scale;
        self.window
            .window()
            .dispatch_event(WindowEvent::ScaleFactorChanged {
                scale_factor: scale as f32,
            });
        self.window.window().set_size(PhysicalSize::new(
            (width as f64 * scale).round() as u32,
            (height as f64 * scale).round() as u32,
        ));
    }

    /// Frosted glass: ask the compositor to blur everything behind the dashboard. Takes effect
    /// with the next frame. Without ext-background-effect support this does nothing.
    pub fn set_glass(&mut self, on: bool) {
        if self.glass != on {
            self.glass = on;
            self.apply_glass();
        }
    }

    fn apply_glass(&mut self) {
        let Some(s) = self.surface.as_mut() else {
            return;
        };
        let Some(effect) = &s.effect else { return };
        if !s.configured || s.blurred == self.glass {
            return;
        }
        if self.glass {
            let region = self.compositor.wl_compositor().create_region(&self.qh, ());
            region.add(0, 0, s.width as i32, s.height as i32);
            effect.set_blur_region(Some(&region));
            region.destroy();
        } else {
            effect.set_blur_region(None);
        }
        s.blurred = self.glass;
        // Applied on the next commit.
        self.window.window().request_redraw();
    }

    /// The compositor gave the surface its (logical) size: draw for it.
    fn configured(&mut self, width: u32, height: u32) {
        if let Some(s) = self.surface.as_mut() {
            s.width = width;
            s.height = height;
            s.configured = true;
            s.frame_pending = false;
            // The size may have changed: the blur region has to follow.
            s.blurred = false;
            self.window.window().request_redraw();
        }
        self.apply_glass();
    }

    /// The surface exists and the compositor has told us its size.
    pub fn is_configured(&self) -> bool {
        self.surface.as_ref().is_some_and(|s| s.configured)
    }

    pub fn is_visible(&self) -> bool {
        self.surface.is_some()
    }

    pub fn show(&mut self) {
        if self.surface.is_some() {
            return;
        }
        let wl_surface = self.compositor.create_surface(&self.qh);
        let fractional = self
            .fractional_manager
            .as_ref()
            .map(|m| m.get_fractional_scale(&wl_surface, &self.qh, ()));
        let viewport = self
            .viewporter
            .as_ref()
            .map(|v| v.get_viewport(&wl_surface, &self.qh, ()));
        let role = match &self.mode {
            Mode::Dashboard => {
                let shell = self.layer_shell.as_ref().expect("bound for the dashboard");
                // No output given: niri puts the surface on the focused output.
                let layer = shell.create_layer_surface(
                    &self.qh,
                    wl_surface,
                    Layer::Overlay,
                    Some("niri-dash"),
                    None,
                );
                layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
                layer.set_exclusive_zone(-1);
                // NIRI_DASH_NO_KEYBOARD=1 is for development: show without grabbing the keyboard.
                layer.set_keyboard_interactivity(
                    if std::env::var_os("NIRI_DASH_NO_KEYBOARD").is_some() {
                        KeyboardInteractivity::None
                    } else {
                        KeyboardInteractivity::Exclusive
                    },
                );
                layer.set_size(0, 0);
                Role::Layer(layer)
            }
            Mode::App {
                app_id,
                title,
                min_size,
                ..
            } => {
                let shell = self.xdg_shell.as_ref().expect("bound for apps");
                let window =
                    shell.create_window(wl_surface, WindowDecorations::RequestServer, &self.qh);
                window.set_app_id(app_id.clone());
                window.set_title(title.clone());
                window.set_min_size(Some(*min_size));
                Role::Toplevel(window)
            }
        };
        role.commit();
        self.presented = 0;
        let effect = self
            .background_effect
            .as_ref()
            .map(|m| m.get_background_effect(role.wl_surface(), &self.qh, ()));
        self.surface = Some(Surface {
            role,
            fractional,
            viewport,
            effect,
            blurred: false,
            width: 0,
            height: 0,
            configured: false,
            frame_pending: false,
        });
        self.closed_by_compositor = false;
    }

    pub fn hide(&mut self) {
        match &mut self.output {
            Output::Egl(_, gl) => gl.detach(),
            // The buffers belonged to the old surface; the pool is reused.
            Output::Shm(t) => t.buffers.clear(),
        }
        if let Some(s) = self.surface.take() {
            if let Some(f) = s.fractional {
                f.destroy();
            }
            if let Some(v) = s.viewport {
                v.destroy();
            }
            if let Some(e) = s.effect {
                e.destroy();
            }
            drop(s.role);
        }
        self.window
            .window()
            .dispatch_event(WindowEvent::PointerExited);
        // Forget the frame so the next show repaints everything.
        self.frame_size = (0, 0);
        let _ = self.conn.flush();
    }

    pub fn set_cursor(&mut self, icon: CursorIcon) {
        if self.cursor == icon {
            return;
        }
        self.cursor = icon;
        if let Some(p) = &self.pointer {
            let _ = p.set_cursor(&self.conn, icon);
        }
    }

    /// How long a pending GPU redraw is held back by `MIN_FRAME_GAP`, if it is. The event loop
    /// sleeps at most this long so the frame isn't late.
    pub fn frame_wait(&self) -> Option<std::time::Duration> {
        match &self.output {
            Output::Egl(window, _) if window.needs_redraw() => self.frame_gap(),
            _ => None,
        }
    }

    /// What is left of `MIN_FRAME_GAP` since the last GPU frame, unless something animates.
    fn frame_gap(&self) -> Option<std::time::Duration> {
        if self.window.window().has_active_animations() {
            return None;
        }
        let since = self.last_frame?.elapsed();
        MIN_FRAME_GAP.checked_sub(since).filter(|d| !d.is_zero())
    }

    /// Render and present a frame if the surface is ready and Slint has something new.
    pub fn render_if_needed(&mut self) {
        let Some(s) = &self.surface else {
            return;
        };
        if !s.configured || s.frame_pending {
            return;
        }
        let scale = if s.fractional.is_some() {
            self.scale
        } else {
            self.scale.round().max(1.0)
        };
        let pw = (s.width as f64 * scale).round() as u32;
        let ph = (s.height as f64 * scale).round() as u32;
        if pw == 0 || ph == 0 {
            return;
        }
        let committed = match self.output {
            Output::Shm(_) => self.render_software(pw, ph, scale),
            Output::Egl(..) => self.render_gpu(pw, ph, scale),
        };
        if committed && let Some(s) = &mut self.surface {
            s.frame_pending = true;
            self.presented += 1;
        }
    }

    /// Lay the Slint window out for a new buffer size or scale, if it changed. Returns whether it
    /// did.
    fn resize_window(&mut self, pw: u32, ph: u32, scale: f64) -> bool {
        if self.frame_size == (pw, ph) {
            return false;
        }
        self.frame_size = (pw, ph);
        let w = self.window.window();
        w.dispatch_event(WindowEvent::ScaleFactorChanged {
            scale_factor: scale as f32,
        });
        w.set_size(PhysicalSize::new(pw, ph));
        w.request_redraw();
        true
    }

    /// Software path: Slint repaints what changed into `pixels`, which are copied into a free shm
    /// buffer and committed with the damage. Returns whether a frame was committed.
    fn render_software(&mut self, pw: u32, ph: u32, scale: f64) -> bool {
        let resized = self.resize_window(pw, ph, scale);
        let (Output::Shm(t), Some(s)) = (&mut self.output, &self.surface) else {
            return false;
        };
        if resized {
            t.pixels = vec![Bgra::default(); (pw * ph) as usize];
            t.buffers.clear();
            t.damage = None;
        }
        // After a resize `pixels` holds no frame yet: render all of it.
        let Some((origin, size)) = render_into(&t.window, &mut t.pixels, pw as usize, resized)
        else {
            return false;
        };
        log::debug!("frame {pw}x{ph} @ {scale}, full {resized}, dirty {origin:?} {size:?}");
        // A frame that could not be presented still changed `pixels`; its damage carries over.
        let (x0, y0) = (origin.x.max(0), origin.y.max(0));
        let (x1, y1) = (x0 + size.width as i32, y0 + size.height as i32);
        t.damage = Some(match t.damage {
            Some((a, b, c, d)) => (a.min(x0), b.min(y0), c.max(x1), d.max(y1)),
            None => (x0, y0, x1, y1),
        });

        let stride = pw as i32 * 4;
        // Find a buffer the compositor is not reading from, or make one.
        let free = t.buffers.iter_mut().position(|b| {
            b.height() == ph as i32 && b.stride() == stride && b.canvas(&mut t.pool).is_some()
        });
        let idx = match free {
            Some(i) => i,
            None if t.buffers.len() >= 3 => {
                // All busy; wait for a release.
                self.window.window().request_redraw();
                return false;
            }
            None => {
                let Ok((buffer, _)) =
                    t.pool
                        .create_buffer(pw as i32, ph as i32, stride, wl_shm::Format::Argb8888)
                else {
                    log::warn!("could not allocate shm buffer");
                    return false;
                };
                t.buffers.push(buffer);
                t.buffers.len() - 1
            }
        };
        let buffer = &mut t.buffers[idx];
        let Some(canvas) = buffer.canvas(&mut t.pool) else {
            self.window.window().request_redraw();
            return false;
        };
        // SAFETY: Bgra is repr(C) of four u8s.
        let src = unsafe {
            std::slice::from_raw_parts(t.pixels.as_ptr() as *const u8, t.pixels.len() * 4)
        };
        canvas[..src.len()].copy_from_slice(src);
        let wl = s.role.wl_surface();
        s.prepare_frame(&self.qh, scale);
        match t.damage.take() {
            Some((x0, y0, x1, y1)) if x1 > x0 && y1 > y0 => {
                wl.damage_buffer(x0, y0, x1 - x0, y1 - y0)
            }
            _ => wl.damage_buffer(0, 0, pw as i32, ph as i32),
        }
        if t.buffers[idx].attach_to(wl).is_err() {
            log::warn!("buffer attach failed");
        }
        s.role.commit();
        true
    }

    /// GPU path: FemtoVG draws the whole frame and eglSwapBuffers attaches and commits it.
    /// Returns whether a frame was committed.
    fn render_gpu(&mut self, pw: u32, ph: u32, scale: f64) -> bool {
        let (Output::Egl(window, gl), Some(s)) = (&self.output, &self.surface) else {
            return false;
        };
        let (window, gl) = (window.clone(), gl.clone());
        if !gl.is_attached() {
            if let Err(e) = gl.attach(s.role.wl_surface(), pw, ph) {
                log::error!("GPU rendering unavailable: {e}");
                return false;
            }
            log::debug!("EGL surface attached, {pw}x{ph} @ {scale}");
            self.frame_size = (0, 0);
        }
        // Resizes the EGL window too (through the renderer).
        self.resize_window(pw, ph, scale);
        // Without an animation running, a redraw is a data update or input like a volume scroll,
        // which arrives faster than it's worth drawing at the display's refresh rate.
        if !window.needs_redraw() || self.frame_gap().is_some() {
            return false;
        }
        let Some(s) = &self.surface else {
            return false;
        };
        // Part of the commit eglSwapBuffers makes.
        s.prepare_frame(&self.qh, scale);
        let t = std::time::Instant::now();
        if !window.draw_if_needed() {
            return false;
        }
        self.last_frame = Some(t);
        log::trace!(
            "GPU frame {pw}x{ph} in {:?}, animating {}",
            t.elapsed(),
            self.window.window().has_active_animations()
        );
        true
    }
}

/// The shortest time between two GPU frames when nothing is animating (60 per second): every
/// frame is a full redraw here and a full recomposite in the compositor.
const MIN_FRAME_GAP: std::time::Duration = std::time::Duration::from_micros(16_600);

/// The Slint window and what draws it.
pub enum Backend {
    Software(Rc<MinimalSoftwareWindow>),
    Gpu(Rc<crate::gpu::GpuWindow>, Rc<crate::gpu::Gl>),
}

impl Backend {
    pub fn adapter(&self) -> Rc<dyn WindowAdapter> {
        match self {
            Backend::Software(w) => w.clone(),
            Backend::Gpu(w, _) => w.clone(),
        }
    }
}

/// Render a frame into `pixels` if Slint has something new. With `full`, the buffer's contents are
/// treated as garbage and everything is repainted; otherwise only what changed since the last
/// render into this same buffer. Returns the repainted area.
pub fn render_into<T: TargetPixel>(
    window: &MinimalSoftwareWindow,
    pixels: &mut [T],
    stride: usize,
    full: bool,
) -> Option<(slint::PhysicalPosition, PhysicalSize)> {
    if full {
        window.request_redraw();
    }
    let mut dirty = None;
    window.draw_if_needed(|renderer| {
        // Switching the buffer type also drops the renderer's partial-rendering cache.
        renderer.set_repaint_buffer_type(if full {
            RepaintBufferType::NewBuffer
        } else {
            RepaintBufferType::ReusedBuffer
        });
        let region = renderer.render(pixels, stride);
        dirty = Some((region.bounding_box_origin(), region.bounding_box_size()));
    });
    dirty
}

/// A scroll event with a gesture phase. Slint's public `WindowEvent` can't carry the phase, so this
/// uses its internal event type (i-slint-core is pinned to the exact version slint uses).
fn wheel_phase(
    window: &slint::Window,
    position: LogicalPosition,
    dx: f32,
    dy: f32,
    phase: TouchPhase,
) {
    let event = i_slint_core::input::BackendMouseEvent::Wheel {
        position: i_slint_core::lengths::LogicalPoint::new(position.x, position.y),
        delta_x: dx,
        delta_y: dy,
        phase,
    };
    window.dispatch_event(WindowEvent::internal(
        i_slint_core::platform::InternalEvent::Mouse(event),
    ));
}

fn button(b: u32) -> PointerEventButton {
    match b {
        BTN_LEFT => PointerEventButton::Left,
        BTN_RIGHT => PointerEventButton::Right,
        BTN_MIDDLE => PointerEventButton::Middle,
        0x116 => PointerEventButton::Back,
        0x115 => PointerEventButton::Forward,
        _ => PointerEventButton::Other,
    }
}

fn key_text(ev: &KeyEvent) -> Option<slint::SharedString> {
    let special = match ev.keysym {
        Keysym::Escape => Some(Key::Escape),
        Keysym::Return | Keysym::KP_Enter => Some(Key::Return),
        Keysym::BackSpace => Some(Key::Backspace),
        Keysym::Tab => Some(Key::Tab),
        Keysym::ISO_Left_Tab => Some(Key::Backtab),
        Keysym::Delete => Some(Key::Delete),
        Keysym::Left => Some(Key::LeftArrow),
        Keysym::Right => Some(Key::RightArrow),
        Keysym::Up => Some(Key::UpArrow),
        Keysym::Down => Some(Key::DownArrow),
        Keysym::Home => Some(Key::Home),
        Keysym::End => Some(Key::End),
        Keysym::Page_Up => Some(Key::PageUp),
        Keysym::Page_Down => Some(Key::PageDown),
        Keysym::Shift_L | Keysym::Shift_R => Some(Key::Shift),
        Keysym::Control_L | Keysym::Control_R => Some(Key::Control),
        Keysym::Alt_L | Keysym::Alt_R => Some(Key::Alt),
        Keysym::Super_L | Keysym::Super_R => Some(Key::Meta),
        _ => None,
    };
    if let Some(k) = special {
        return Some(k.into());
    }
    ev.utf8
        .as_ref()
        .filter(|s| !s.is_empty() && !s.chars().any(char::is_control))
        .map(|s| s.as_str().into())
}

impl CompositorHandler for Shell {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        factor: i32,
    ) {
        // Integer scale fallback when fractional-scale is missing.
        if self
            .surface
            .as_ref()
            .is_some_and(|s| s.fractional.is_none())
        {
            self.scale = factor as f64;
            self.window.window().request_redraw();
        }
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        if let Some(s) = self.surface.as_mut() {
            s.frame_pending = false;
        }
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for Shell {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for Shell {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.hide();
        self.closed_by_compositor = true;
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        self.configured(configure.new_size.0.max(1), configure.new_size.1.max(1));
    }
}

impl WindowHandler for Shell {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.hide();
        self.closed_by_compositor = true;
    }
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &Window,
        configure: WindowConfigure,
        _: u32,
    ) {
        let (Mode::App { size, .. }, Some(s)) = (&self.mode, &self.surface) else {
            return;
        };
        // A dimension the compositor leaves open keeps its current value (or our default).
        let pick = |new: Option<std::num::NonZeroU32>, cur: u32, default: u32| match new {
            Some(v) => v.get(),
            None if cur > 0 => cur,
            None => default,
        };
        let width = pick(configure.new_size.0, s.width, size.0);
        let height = pick(configure.new_size.1, s.height, size.1);
        self.configured(width, height);
    }
}

impl SeatHandler for Shell {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        cap: Capability,
    ) {
        if self.data_device.is_none()
            && let Some(m) = &self.data_control
        {
            self.data_device = Some(m.get_data_device(&seat, qh, ()));
        }
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            let kb = self.seat_state.get_keyboard_with_repeat(
                qh,
                &seat,
                None,
                self.loop_handle.clone(),
                Box::new(|shell: &mut Shell, _, ev| {
                    if let Some(text) = key_text(&ev) {
                        shell
                            .window
                            .window()
                            .dispatch_event(WindowEvent::KeyPressRepeated { text });
                    }
                }),
            );
            match kb {
                Ok(kb) => self.keyboard = Some(kb),
                Err(e) => log::warn!("keyboard: {e}"),
            }
        }
        if cap == Capability::Pointer && self.pointer.is_none() {
            let cursor_surface = self.compositor.create_surface(qh);
            match self.seat_state.get_pointer_with_theme::<_, ()>(
                qh,
                &seat,
                self.shm.wl_shm(),
                cursor_surface,
                ThemeSpec::default(),
            ) {
                Ok(p) => self.pointer = Some(p),
                Err(e) => log::warn!("pointer: {e}"),
            }
        }
    }
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        cap: Capability,
    ) {
        if cap == Capability::Keyboard
            && let Some(kb) = self.keyboard.take()
        {
            kb.release();
        }
        if cap == Capability::Pointer {
            self.pointer = None;
        }
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for Shell {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        self.window
            .window()
            .dispatch_event(WindowEvent::WindowActiveChanged(true));
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
        self.window
            .window()
            .dispatch_event(WindowEvent::WindowActiveChanged(false));
    }
    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        ev: KeyEvent,
    ) {
        if let Some(text) = key_text(&ev) {
            self.window
                .window()
                .dispatch_event(WindowEvent::KeyPressed { text });
        }
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        ev: KeyEvent,
    ) {
        if let Some(text) = key_text(&ev) {
            self.window
                .window()
                .dispatch_event(WindowEvent::KeyPressRepeated { text });
        }
    }
    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        ev: KeyEvent,
    ) {
        // Release events carry no utf8; rebuild the text from the keysym.
        let text = key_text(&ev).or_else(|| {
            ev.keysym
                .key_char()
                .filter(|c| !c.is_control())
                .map(|c| c.to_string().into())
        });
        if let Some(text) = text {
            self.window
                .window()
                .dispatch_event(WindowEvent::KeyReleased { text });
        }
    }
    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
    }
}

impl PointerHandler for Shell {
    fn pointer_frame(
        &mut self,
        conn: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for ev in events {
            let position = LogicalPosition::new(ev.position.0 as f32, ev.position.1 as f32);
            match ev.kind {
                PointerEventKind::Enter { .. } => {
                    if let Some(p) = &self.pointer {
                        let _ = p.set_cursor(conn, self.cursor);
                    }
                    self.window
                        .window()
                        .dispatch_event(WindowEvent::PointerMoved { position });
                }
                PointerEventKind::Leave { .. } => {
                    if self.scroll_gesture {
                        self.scroll_gesture = false;
                        wheel_phase(self.window.window(), position, 0.0, 0.0, TouchPhase::Ended);
                    }
                    self.window
                        .window()
                        .dispatch_event(WindowEvent::PointerExited)
                }
                PointerEventKind::Motion { .. } => self
                    .window
                    .window()
                    .dispatch_event(WindowEvent::PointerMoved { position }),
                PointerEventKind::Press { button: b, .. } => {
                    self.window
                        .window()
                        .dispatch_event(WindowEvent::PointerPressed {
                            position,
                            button: button(b),
                        })
                }
                PointerEventKind::Release { button: b, .. } => {
                    self.window
                        .window()
                        .dispatch_event(WindowEvent::PointerReleased {
                            position,
                            button: button(b),
                        })
                }
                PointerEventKind::Axis {
                    horizontal,
                    vertical,
                    source,
                    ..
                } => {
                    // Slint scrolls content by +delta; wayland's axis is the opposite direction.
                    let unit = |a: &smithay_client_toolkit::seat::pointer::AxisScroll| {
                        if a.value120 != 0 {
                            a.value120 as f32 / 120.0 * 48.0
                        } else {
                            a.absolute as f32
                        }
                    };
                    let (dx, dy) = (-unit(&horizontal), -unit(&vertical));
                    let stop = horizontal.stop || vertical.stop;
                    let continuous = matches!(
                        source,
                        Some(wl_pointer::AxisSource::Finger | wl_pointer::AxisSource::Continuous)
                    );
                    if continuous || stop || self.scroll_gesture {
                        // Touchpad: give Slint the gesture phase so lists follow the fingers and
                        // glide on release, instead of animating every tiny event like a wheel tick.
                        if !self.scroll_gesture && !stop {
                            self.scroll_gesture = true;
                            wheel_phase(
                                self.window.window(),
                                position,
                                0.0,
                                0.0,
                                TouchPhase::Started,
                            );
                        }
                        if dx != 0.0 || dy != 0.0 {
                            wheel_phase(self.window.window(), position, dx, dy, TouchPhase::Moved);
                        }
                        if stop && self.scroll_gesture {
                            self.scroll_gesture = false;
                            wheel_phase(
                                self.window.window(),
                                position,
                                0.0,
                                0.0,
                                TouchPhase::Ended,
                            );
                        }
                    } else {
                        self.window
                            .window()
                            .dispatch_event(WindowEvent::PointerScrolled {
                                position,
                                delta_x: dx,
                                delta_y: dy,
                            });
                    }
                }
            }
        }
    }
}

impl ShmHandler for Shell {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for Shell {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

impl Dispatch<WpFractionalScaleManagerV1, ()> for Shell {
    fn event(
        _: &mut Self,
        _: &WpFractionalScaleManagerV1,
        _: <WpFractionalScaleManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpFractionalScaleV1, ()> for Shell {
    fn event(
        shell: &mut Self,
        _: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            shell.scale = scale as f64 / 120.0;
            shell.window.window().request_redraw();
        }
    }
}

impl Dispatch<ExtBackgroundEffectManagerV1, ()> for Shell {
    fn event(
        _: &mut Self,
        _: &ExtBackgroundEffectManagerV1,
        event: <ExtBackgroundEffectManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        log::debug!("background effect: {event:?}");
    }
}

impl Dispatch<ExtBackgroundEffectSurfaceV1, ()> for Shell {
    fn event(
        _: &mut Self,
        _: &ExtBackgroundEffectSurfaceV1,
        _: <ExtBackgroundEffectSurfaceV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_region::WlRegion, ()> for Shell {
    fn event(
        _: &mut Self,
        _: &wl_region::WlRegion,
        _: wl_region::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpViewporter, ()> for Shell {
    fn event(
        _: &mut Self,
        _: &WpViewporter,
        _: <WpViewporter as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WpViewport, ()> for Shell {
    fn event(
        _: &mut Self,
        _: &WpViewport,
        _: <WpViewport as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Shell {
    /// Put a clipboard history entry back on the clipboard.
    pub fn set_clipboard(&mut self, clip: Clip) {
        let (Some(m), Some(dev)) = (&self.data_control, &self.data_device) else {
            return;
        };
        let source = m.create_data_source(&self.qh, ());
        for mime in clip.kind.mimes() {
            source.offer(mime.to_string());
        }
        dev.set_selection(Some(&source));
        if let Some((old, _)) = self.own_source.replace((source, clip)) {
            old.destroy();
        }
        let _ = self.conn.flush();
    }

    fn receive(&mut self, offer: ZwlrDataControlOfferV1) {
        let mimes = offer
            .data::<OfferMimes>()
            .and_then(|m| m.0.lock().ok().map(|v| v.clone()))
            .unwrap_or_default();
        let (Some((kind, mime)), Some(tx)) = (clipboard::pick(&mimes), self.clip_tx.clone()) else {
            offer.destroy();
            return;
        };
        let Ok((mut reader, writer)) = std::io::pipe() else {
            offer.destroy();
            return;
        };
        offer.receive(mime.to_string(), std::os::fd::AsFd::as_fd(&writer));
        let _ = self.conn.flush();
        drop(writer);
        if let Some(old) = self.current_offer.replace(offer) {
            old.destroy();
        }
        std::thread::spawn(move || {
            let mut data = Vec::new();
            let mut chunk = [0u8; 64 * 1024];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) if data.len() + n <= clipboard::MAX_BYTES => {
                        data.extend_from_slice(&chunk[..n])
                    }
                    Ok(_) => return, // too big to keep
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => return,
                }
            }
            if !data.is_empty() {
                let _ = tx.send(Msg::Clipboard(Clip {
                    kind,
                    data: Arc::new(data),
                }));
            }
        });
    }
}

impl Dispatch<ZwlrDataControlManagerV1, ()> for Shell {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlManagerV1,
        _: <ZwlrDataControlManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for Shell {
    fn event(
        shell: &mut Self,
        _: &ZwlrDataControlDeviceV1,
        event: zwlr_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_data_control_device_v1::Event::Selection { id: Some(offer) } => {
                shell.receive(offer)
            }
            zwlr_data_control_device_v1::Event::Selection { id: None } => {
                if let Some(old) = shell.current_offer.take() {
                    old.destroy();
                }
            }
            zwlr_data_control_device_v1::Event::PrimarySelection { id: Some(offer) } => {
                offer.destroy()
            }
            zwlr_data_control_device_v1::Event::Finished => {
                shell.data_device = None;
            }
            _ => {}
        }
    }

    smithay_client_toolkit::reexports::client::event_created_child!(Shell, ZwlrDataControlDeviceV1, [
        zwlr_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, OfferMimes::default()),
    ]);
}

impl Dispatch<ZwlrDataControlOfferV1, OfferMimes> for Shell {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlOfferV1,
        event: zwlr_data_control_offer_v1::Event,
        mimes: &OfferMimes,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_offer_v1::Event::Offer { mime_type } = event
            && let Ok(mut m) = mimes.0.lock()
        {
            m.push(mime_type);
        }
    }
}

impl Dispatch<ZwlrDataControlSourceV1, ()> for Shell {
    fn event(
        shell: &mut Self,
        source: &ZwlrDataControlSourceV1,
        event: zwlr_data_control_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_data_control_source_v1::Event::Send { fd, .. } => {
                if let Some((_, clip)) = shell.own_source.as_ref().filter(|(s, _)| s == source) {
                    let data = clip.data.clone();
                    std::thread::spawn(move || {
                        let mut f = std::fs::File::from(fd);
                        let _ = f.write_all(&data);
                    });
                }
            }
            zwlr_data_control_source_v1::Event::Cancelled => {
                if shell.own_source.as_ref().is_some_and(|(s, _)| s == source) {
                    shell.own_source = None;
                }
                source.destroy();
            }
            _ => {}
        }
    }
}

delegate_registry!(Shell);
smithay_client_toolkit::delegate_dispatch2!(Shell);

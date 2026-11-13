//! Hardware rendering: Slint's FemtoVG renderer drawing with OpenGL ES through EGL, straight onto
//! the shell's surface (the dashboard's layer surface or an app's window), or offscreen without
//! any display server for snapshots. The EGL context outlives the surface (which only exists while
//! the dashboard is shown); without a surface it is made current surfaceless, so Slint's GPU
//! resources survive hiding and showing.

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, c_void};
use std::num::NonZeroU32;
use std::rc::{Rc, Weak};

use khronos_egl as egl;
use slint::platform::femtovg_renderer::{FemtoVGRenderer, OpenGLInterface};
use slint::platform::{Renderer, WindowAdapter, WindowEvent};
use slint::{PhysicalSize, Window, WindowSize};
use wayland_client::{Connection, Proxy, protocol::wl_surface::WlSurface};
use wayland_egl::WlEglSurface;

type Error = Box<dyn std::error::Error + Send + Sync>;

/// Setting up OpenGL ES through EGL failed.
#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    #[error("loading libEGL: {0}")]
    Load(String),
    #[error("{0}: {1}")]
    Egl(&'static str, #[source] egl::Error),
    #[error("EGL lacks EGL_KHR_surfaceless_context")]
    NoSurfaceless,
    #[error("no RGBA8 EGL config")]
    NoConfig,
    #[error("wl_egl_window: {0}")]
    Window(String),
    #[error("FemtoVG renderer: {0}")]
    Renderer(String),
}

type Result<T, E = GpuError> = std::result::Result<T, E>;

/// EGL display, config and context, plus the window surface while one is attached.
pub struct Gl {
    egl: egl::DynamicInstance<egl::EGL1_5>,
    display: egl::Display,
    config: egl::Config,
    context: egl::Context,
    target: RefCell<Option<Target>>,
}

struct Target {
    // Dropped after `surface` is destroyed (see `detach`). None for a pbuffer.
    native: Option<WlEglSurface>,
    surface: egl::Surface,
}

impl Gl {
    /// A context for drawing onto surfaces of `conn`.
    pub fn new(conn: &Connection) -> Result<Rc<Self>> {
        let display_ptr = conn.backend().display_ptr() as *mut c_void;
        // SAFETY: the pointer is the live wl_display of `conn`, matching the Wayland platform.
        unsafe { Self::with_platform(EGL_PLATFORM_WAYLAND_KHR, display_ptr, egl::WINDOW_BIT) }
    }

    /// A context without any display server, for offscreen rendering (`snapshot`).
    pub fn offscreen() -> Result<Rc<Self>> {
        // SAFETY: the surfaceless platform takes no native display.
        unsafe {
            Self::with_platform(
                EGL_PLATFORM_SURFACELESS_MESA,
                std::ptr::null_mut(),
                egl::PBUFFER_BIT,
            )
        }
        .or_else(|_| unsafe {
            Self::with_platform(EGL_PLATFORM_SURFACELESS_MESA, std::ptr::null_mut(), 0)
        })
    }

    /// # Safety
    /// `native_display` must be valid for `platform`.
    unsafe fn with_platform(
        platform: egl::Enum,
        native_display: *mut c_void,
        surface_type: egl::Int,
    ) -> Result<Rc<Self>> {
        // SAFETY: loads the system libEGL, which implements the EGL ABI.
        let egl = unsafe { egl::DynamicInstance::<egl::EGL1_5>::load_required() }
            .map_err(|e| GpuError::Load(e.to_string()))?;
        // SAFETY: guaranteed by the caller.
        let display =
            unsafe { egl.get_platform_display(platform, native_display, &[egl::ATTRIB_NONE]) }
                .map_err(|e| GpuError::Egl("eglGetPlatformDisplay", e))?;
        egl.initialize(display)
            .map_err(|e| GpuError::Egl("eglInitialize", e))?;
        let extensions = egl
            .query_string(Some(display), egl::EXTENSIONS)
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !extensions.contains("EGL_KHR_surfaceless_context") {
            return Err(GpuError::NoSurfaceless);
        }
        let config = egl
            .choose_first_config(
                display,
                &[
                    egl::SURFACE_TYPE,
                    surface_type,
                    egl::RENDERABLE_TYPE,
                    egl::OPENGL_ES2_BIT,
                    egl::RED_SIZE,
                    8,
                    egl::GREEN_SIZE,
                    8,
                    egl::BLUE_SIZE,
                    8,
                    egl::ALPHA_SIZE,
                    8,
                    egl::STENCIL_SIZE,
                    8,
                    egl::NONE,
                ],
            )
            .map_err(|e| GpuError::Egl("eglChooseConfig", e))?
            .ok_or(GpuError::NoConfig)?;
        egl.bind_api(egl::OPENGL_ES_API)
            .map_err(|e| GpuError::Egl("eglBindAPI", e))?;
        let context = egl
            .create_context(
                display,
                config,
                None,
                &[egl::CONTEXT_MAJOR_VERSION, 3, egl::NONE],
            )
            .or_else(|_| {
                egl.create_context(
                    display,
                    config,
                    None,
                    &[egl::CONTEXT_MAJOR_VERSION, 2, egl::NONE],
                )
            })
            .map_err(|e| GpuError::Egl("eglCreateContext", e))?;
        egl.make_current(display, None, None, Some(context))
            .map_err(|e| GpuError::Egl("eglMakeCurrent (surfaceless)", e))?;
        if let Ok(vendor) = egl.query_string(Some(display), egl::VENDOR) {
            log::info!("EGL {}", vendor.to_string_lossy());
        }
        Ok(Rc::new(Self {
            egl,
            display,
            config,
            context,
            target: RefCell::new(None),
        }))
    }

    /// Render into `surface` from now on, at `width`×`height` buffer pixels.
    pub fn attach(&self, surface: &WlSurface, width: u32, height: u32) -> Result<()> {
        self.detach();
        let native = WlEglSurface::new(surface.id(), width as i32, height as i32)
            .map_err(|e| GpuError::Window(e.to_string()))?;
        // SAFETY: `native` is a wl_egl_window on the same Wayland display as `self.display`.
        let surface = unsafe {
            self.egl.create_window_surface(
                self.display,
                self.config,
                native.ptr() as egl::NativeWindowType,
                None,
            )
        }
        .map_err(|e| GpuError::Egl("eglCreateWindowSurface", e))?;
        self.egl
            .make_current(
                self.display,
                Some(surface),
                Some(surface),
                Some(self.context),
            )
            .map_err(|e| GpuError::Egl("eglMakeCurrent", e))?;
        // Frame pacing is done with our own frame callbacks; never block in eglSwapBuffers.
        let _ = self.egl.swap_interval(self.display, 0);
        *self.target.borrow_mut() = Some(Target {
            native: Some(native),
            surface,
        });
        Ok(())
    }

    /// Render into an offscreen pbuffer of `width`×`height` from now on: frames are drawn as on
    /// screen, without being read back (`snapshot --bench`).
    pub fn attach_pbuffer(&self, width: u32, height: u32) -> Result<()> {
        self.detach();
        let surface = self
            .egl
            .create_pbuffer_surface(
                self.display,
                self.config,
                &[
                    egl::WIDTH,
                    width as egl::Int,
                    egl::HEIGHT,
                    height as egl::Int,
                    egl::NONE,
                ],
            )
            .map_err(|e| GpuError::Egl("eglCreatePbufferSurface", e))?;
        self.egl
            .make_current(
                self.display,
                Some(surface),
                Some(surface),
                Some(self.context),
            )
            .map_err(|e| GpuError::Egl("eglMakeCurrent (pbuffer)", e))?;
        *self.target.borrow_mut() = Some(Target {
            native: None,
            surface,
        });
        Ok(())
    }

    /// Wait until the GPU has done what it was told (`glFinish`), to time a frame.
    pub fn finish(&self) {
        if let Some(f) = self.egl.get_proc_address("glFinish") {
            // glFinish takes no arguments; the context is current on this thread.
            f();
        }
    }

    /// Stop rendering into the surface, before it is destroyed.
    pub fn detach(&self) {
        if let Some(t) = self.target.borrow_mut().take() {
            let _ = self
                .egl
                .make_current(self.display, None, None, Some(self.context));
            let _ = self.egl.destroy_surface(self.display, t.surface);
            drop(t.native);
        }
    }

    pub fn is_attached(&self) -> bool {
        self.target.borrow().is_some()
    }
}

impl Drop for Gl {
    fn drop(&mut self) {
        self.detach();
        let _ = self.egl.make_current(self.display, None, None, None);
        let _ = self.egl.destroy_context(self.display, self.context);
    }
}

const EGL_PLATFORM_WAYLAND_KHR: egl::Enum = 0x31D8;
const EGL_PLATFORM_SURFACELESS_MESA: egl::Enum = 0x31DD;

/// What FemtoVG holds on to; it shares the context with the shell.
struct GlHandle(Rc<Gl>);

// SAFETY: the context is created and only ever used on the UI thread; `get_proc_address` returns
// functions of the current context's API.
unsafe impl OpenGLInterface for GlHandle {
    fn ensure_current(&self) -> Result<(), Error> {
        let gl = &self.0;
        let target = gl.target.borrow();
        let s = target.as_ref().map(|t| t.surface);
        gl.egl.make_current(gl.display, s, s, Some(gl.context))?;
        Ok(())
    }

    fn swap_buffers(&self) -> Result<(), Error> {
        let gl = &self.0;
        if let Some(t) = gl.target.borrow().as_ref() {
            gl.egl.swap_buffers(gl.display, t.surface)?;
        }
        Ok(())
    }

    fn resize(&self, width: NonZeroU32, height: NonZeroU32) -> Result<(), Error> {
        if let Some(native) = self
            .0
            .target
            .borrow()
            .as_ref()
            .and_then(|t| t.native.as_ref())
        {
            native.resize(width.get() as i32, height.get() as i32, 0, 0);
        }
        Ok(())
    }

    fn get_proc_address(&self, name: &CStr) -> *const c_void {
        name.to_str()
            .ok()
            .and_then(|n| self.0.egl.get_proc_address(n))
            .map_or(std::ptr::null(), |f| f as *const c_void)
    }
}

/// A Slint window drawn by FemtoVG; the GPU counterpart of `MinimalSoftwareWindow`.
pub struct GpuWindow {
    window: Window,
    renderer: FemtoVGRenderer,
    size: Cell<PhysicalSize>,
    needs_redraw: Cell<bool>,
}

impl GpuWindow {
    pub fn new(gl: &Rc<Gl>) -> Result<Rc<Self>> {
        let renderer = FemtoVGRenderer::new(GlHandle(gl.clone()))
            .map_err(|e| GpuError::Renderer(e.to_string()))?;
        Ok(Rc::new_cyclic(|w: &Weak<Self>| Self {
            window: Window::new(w.clone()),
            renderer,
            size: Default::default(),
            needs_redraw: Cell::new(true),
        }))
    }

    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw.get()
    }

    /// Forget a pending redraw (after drawing some other way, like a snapshot).
    pub fn mark_drawn(&self) {
        self.needs_redraw.set(false);
    }

    /// Render and swap (which commits the surface) if anything changed. Returns whether it did.
    pub fn draw_if_needed(&self) -> bool {
        if !self.needs_redraw.replace(false) {
            return false;
        }
        if let Err(e) = self.renderer.render() {
            log::warn!("GPU render failed: {e}");
            return false;
        }
        true
    }
}

impl WindowAdapter for GpuWindow {
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

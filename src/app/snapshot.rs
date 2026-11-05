//! `niri-dash snapshot`: render the dashboard offscreen into a PNG, with live data, without
//! touching the screen. A development tool.

use super::{Error, Input, Result, build, new_ui, renderer_override, set_platform, start_system};
use crate::layout::{Kind, Layout, RendererChoice};
use crate::theme;
use niri_dash_system::{Event, Tell, host};
use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType,
};
use slint::platform::{PointerEventButton, WindowAdapter, WindowEvent};
use smithay_client_toolkit::reexports::calloop::{EventLoop, channel};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// A sidebar tab to open in edit mode.
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum Tab {
    Colors,
    Style,
}

#[derive(clap::Args, Clone)]
pub struct Args {
    out: PathBuf,
    #[arg(long, default_value_t = 1706)]
    width: u32,
    #[arg(long, default_value_t = 1066)]
    height: u32,
    #[arg(long, default_value_t = 1.5)]
    scale: f32,
    /// Seconds of live data before the frame is taken.
    #[arg(long, default_value_t = 3.0)]
    wait: f32,
    #[arg(long)]
    edit: bool,
    /// Render a standalone app (a widget name) instead of the dashboard.
    #[arg(long, value_enum)]
    pub app: Option<crate::layout::Kind>,
    /// Open this sidebar tab.
    #[arg(long, value_enum, requires = "edit")]
    tab: Option<Tab>,
    /// Open the settings of the N-th widget in config.kdl (counting from 0).
    #[arg(long, value_name = "N", requires = "edit")]
    configure: Option<usize>,
    /// Show this color scheme instead of the configured one, without touching config.kdl.
    #[arg(long)]
    colorscheme: Option<String>,
    /// Show this style instead of the configured one, without touching config.kdl.
    #[arg(long)]
    style: Option<String>,
    /// Scripted input after the wait, rendered incrementally with animations running in
    /// between: "down:x,y;move:x,y;up:x,y;dbl:x,y;wait:ms;key:K". K is a character, or esc,
    /// enter, space, tab, backtab, backspace, left, right, up, down, with ctrl+ / shift+ / alt+
    /// in front for modifiers held.
    #[arg(long, value_name = "SCRIPT")]
    events: Option<String>,
    /// Render incrementally while waiting, into a reused buffer as the daemon does.
    #[arg(long)]
    incremental: bool,
    /// Keep the incrementally rendered frame instead of rendering a fresh, full one at the end.
    #[arg(long, requires = "incremental")]
    keep: bool,
    /// Time the frames of the open animation and print the figures.
    #[arg(long)]
    bench: bool,
    /// Show the mock system's made-up data instead of the live system's.
    #[arg(long)]
    mock: bool,
}

/// Render the snapshot `args` describe, of the dashboard or of the standalone app `solo`.
pub fn run(args: &Args, solo: Option<Kind>) -> Result<()> {
    let (events_tx, events) = super::loop_channel::<Event>();
    let system = if args.mock {
        niri_dash_system::mock(events_tx)?
    } else {
        start_system(events_tx, solo, None)?
    };
    system.senders.host.tell(host::Command::Visible(true));
    // NIRI_DASH_RENDERER=gpu: draw with FemtoVG/OpenGL ES into an offscreen texture instead.
    let gpu = match renderer_override() {
        Some(RendererChoice::Gpu) => {
            let gl = crate::gpu::Gl::offscreen()?;
            Some((crate::gpu::GpuWindow::new(&gl)?, gl))
        }
        _ => None,
    };
    let soft = MinimalSoftwareWindow::new(if args.incremental {
        RepaintBufferType::ReusedBuffer
    } else {
        RepaintBufferType::NewBuffer
    });
    let adapter: Rc<dyn WindowAdapter> = match &gpu {
        Some((w, _)) => w.clone(),
        None => soft.clone(),
    };
    let window = adapter.window();
    set_platform(adapter.clone())?;
    // Draws a frame into `pixels` if anything changed (or always with `full`).
    let render_into = |pixels: &mut [PremultipliedRgbaColor], stride: usize, full: bool| {
        let Some((g, _)) = &gpu else {
            return crate::shell::render_into(&soft, pixels, stride, full).map(|_| ());
        };
        if !full && !g.needs_redraw() {
            return None;
        }
        g.mark_drawn();
        let shot = window
            .take_snapshot()
            .map_err(|e| log::warn!("GPU snapshot: {e}"))
            .ok()?;
        for (d, s) in pixels.iter_mut().zip(shot.as_slice()) {
            *d = PremultipliedRgbaColor {
                red: s.r,
                green: s.g,
                blue: s.b,
                alpha: s.a,
            };
        }
        Some(())
    };
    let ui = new_ui()?;
    let (pw, ph) = (
        (args.width as f32 * args.scale).round() as u32,
        (args.height as f32 * args.scale).round() as u32,
    );
    window.dispatch_event(WindowEvent::ScaleFactorChanged {
        scale_factor: args.scale,
    });
    window.set_size(slint::PhysicalSize::new(pw, ph));
    let mut event_loop: EventLoop<'static, ()> = EventLoop::try_new()?;
    // Only the config watcher sends inputs: there is no control socket.
    let (input_tx, inputs) = super::loop_channel::<Input>();
    let app = build(&ui, system, input_tx, solo, Layout::load(), true);
    let a = app.clone();
    event_loop
        .handle()
        .insert_source(events, move |ev, _, _| {
            if let channel::Event::Msg(e) = ev {
                let _ = a.handle_event(e);
            }
        })
        .map_err(|e| Error::Ui(e.to_string()))?;
    let a = app.clone();
    event_loop
        .handle()
        .insert_source(inputs, move |ev, _, _| {
            if let channel::Event::Msg(i) = ev {
                let _ = a.handle_input(i);
            }
        })
        .map_err(|e| Error::Ui(e.to_string()))?;
    if args.colorscheme.is_some() || args.style.is_some() {
        let (look, problems) = {
            let l = app.layout.borrow();
            theme::load_look(
                args.colorscheme.as_deref().unwrap_or(&l.colorscheme),
                args.style.as_deref().unwrap_or(&l.style),
                &l.style_overrides,
            )
        };
        app.show_look(look, &problems, false);
    }
    if args.edit {
        app.set_editing(true);
        match args.tab {
            Some(Tab::Colors) => ui.set_drawer_tab(crate::DrawerTab::Colors),
            Some(Tab::Style) => ui.set_drawer_tab(crate::DrawerTab::Style),
            None => {}
        }
        let id = args
            .configure
            .and_then(|n| app.layout.borrow().widgets.get(n).map(|w| w.id));
        if let Some(id) = id {
            app.configure_widget(id);
        }
    }
    let end = Instant::now() + Duration::from_secs_f32(args.wait);
    let mut first = true;
    let mut pixels = vec![PremultipliedRgbaColor::default(); (pw * ph) as usize];
    while Instant::now() < end {
        event_loop.dispatch(Some(Duration::from_millis(50)), &mut ())?;
        slint::platform::update_timers_and_animations();
        if first {
            // Let layouts settle once before the grid size is read.
            first = false;
            app.tx.host.tell(host::Command::Refresh);
        }
        app.sync_grid();
        if args.incremental {
            render_into(&mut pixels, pw as usize, false);
        }
    }
    // As on show: the keyboard goes to the dashboard (or to a standalone app's widget).
    window.dispatch_event(WindowEvent::WindowActiveChanged(true));
    ui.invoke_reset();
    app.reset_nav();
    if let Some(script) = &args.events {
        // Events keep coming in meanwhile, so what the (mock) system answers shows up.
        let mut pump = |ms: u64, pixels: &mut Vec<_>| {
            let t = Instant::now();
            while t.elapsed() < Duration::from_millis(ms) {
                if let Err(e) = event_loop.dispatch(Some(Duration::ZERO), &mut ()) {
                    log::warn!("event loop: {e}");
                }
                slint::platform::update_timers_and_animations();
                render_into(pixels, pw as usize, false);
                std::thread::sleep(Duration::from_millis(8));
            }
        };
        for step in script.split(';').filter(|s| !s.is_empty()) {
            let (verb, arg) = step.split_once(':').unwrap_or((step, ""));
            if verb == "wait" {
                pump(arg.parse().unwrap_or(300), &mut pixels);
                continue;
            }
            if verb == "key" {
                press_key(window, arg);
                pump(40, &mut pixels);
                continue;
            }
            let (x, y) = arg
                .split_once(',')
                .map(|(a, b)| (a.parse().unwrap_or(0.0), b.parse().unwrap_or(0.0)))
                .unwrap_or((0.0, 0.0));
            let position = slint::LogicalPosition::new(x, y);
            let button = PointerEventButton::Left;
            let ev = |e: WindowEvent| window.dispatch_event(e);
            match verb {
                "down" => ev(WindowEvent::PointerPressed { position, button }),
                "up" => ev(WindowEvent::PointerReleased { position, button }),
                "move" => ev(WindowEvent::PointerMoved { position }),
                "dbl" => {
                    ev(WindowEvent::PointerMoved { position });
                    for _ in 0..2 {
                        ev(WindowEvent::PointerPressed { position, button });
                        ev(WindowEvent::PointerReleased { position, button });
                    }
                }
                _ => log::warn!("unknown event {verb}"),
            }
            pump(40, &mut pixels);
        }
        pump(400, &mut pixels);
    }
    if args.bench {
        ui.set_shown(false);
        let t = Instant::now();
        while t.elapsed() < Duration::from_millis(300) {
            slint::platform::update_timers_and_animations();
            render_into(&mut pixels, pw as usize, false);
        }
        ui.set_shown(true);
        let start = Instant::now();
        let mut times = Vec::new();
        while start.elapsed() < Duration::from_millis(900) {
            slint::platform::update_timers_and_animations();
            let f = Instant::now();
            if render_into(&mut pixels, pw as usize, false).is_some() {
                times.push(f.elapsed().as_secs_f64() * 1000.0);
            }
        }
        times.sort_by(|a, b| a.total_cmp(b));
        let n = times.len().max(1);
        println!(
            "frames {} median {:.1} ms p90 {:.1} ms max {:.1} ms",
            times.len(),
            times[n / 2],
            times[n * 9 / 10],
            times[n - 1]
        );
    }
    if !args.keep {
        // Like hiding and showing again: a fresh buffer, then a full frame.
        pixels.fill(Default::default());
        render_into(&mut pixels, pw as usize, true);
    }
    write_png(&args.out, pw, ph, &pixels)
}

/// Press and release `spec` ("f", "ctrl+l", "esc", …) as the platform layer would.
pub(super) fn press_key(window: &slint::Window, spec: &str) {
    use slint::platform::Key;
    let mut rest = spec;
    let mut mods = Vec::new();
    loop {
        let (m, r) = match rest.split_once('+') {
            Some((m, r)) if !r.is_empty() => (m, r),
            _ => break,
        };
        mods.push(match m {
            "ctrl" => Key::Control,
            "shift" => Key::Shift,
            "alt" => Key::Alt,
            _ => break,
        });
        rest = r;
    }
    let text: slint::SharedString = match rest {
        "esc" => Key::Escape.into(),
        "enter" => Key::Return.into(),
        "space" => " ".into(),
        "tab" => Key::Tab.into(),
        "backtab" => Key::Backtab.into(),
        "backspace" => Key::Backspace.into(),
        "left" => Key::LeftArrow.into(),
        "right" => Key::RightArrow.into(),
        "up" => Key::UpArrow.into(),
        "down" => Key::DownArrow.into(),
        other => other.into(),
    };
    let ev = |e: WindowEvent| window.dispatch_event(e);
    for m in &mods {
        ev(WindowEvent::KeyPressed { text: (*m).into() });
    }
    ev(WindowEvent::KeyPressed { text: text.clone() });
    ev(WindowEvent::KeyReleased { text });
    for m in mods.iter().rev() {
        ev(WindowEvent::KeyReleased { text: (*m).into() });
    }
}

fn write_png(
    out: &std::path::Path,
    w: u32,
    h: u32,
    pixels: &[PremultipliedRgbaColor],
) -> Result<()> {
    let file = std::fs::File::create(out).map_err(|source| Error::Create {
        path: out.to_path_buf(),
        source,
    })?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header()?;
    let data: Vec<u8> = pixels
        .iter()
        .flat_map(|p| [p.red, p.green, p.blue, p.alpha])
        .collect();
    writer.write_image_data(&data)?;
    Ok(())
}

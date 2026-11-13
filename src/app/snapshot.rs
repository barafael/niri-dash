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
    Arrange,
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
    /// Time frames and print the figures: the open animation, a still frame, showing the
    /// dashboard again and again, edit mode in and out, and style presets one after another.
    /// `NIRI_DASH_RENDERER=gpu` draws into an offscreen buffer as on screen, without reading it
    /// back. `NIRI_DASH_BENCH_ONLY=open|show|edit|styles` runs one part;
    /// `NIRI_DASH_BENCH_TURNS=1` says when the late turns of edit mode and the presets came.
    /// Choosing presets saves config.kdl: point XDG_CONFIG_HOME at a copy.
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
    // What a frame costs on screen: drawn into a pbuffer, waited for, never read back.
    let bench_frame = |full: bool| -> Option<()> {
        match &gpu {
            Some((g, gl)) => {
                if full {
                    g.window().request_redraw();
                }
                if !g.draw_if_needed() {
                    return None;
                }
                gl.finish();
                Some(())
            }
            None => None,
        }
    };
    let (pw, ph) = (
        (args.width as f32 * args.scale).round() as u32,
        (args.height as f32 * args.scale).round() as u32,
    );
    window.dispatch_event(WindowEvent::ScaleFactorChanged {
        scale_factor: args.scale,
    });
    window.set_size(slint::PhysicalSize::new(pw, ph));
    if args.bench
        && let Some((_, gl)) = &gpu
    {
        gl.attach_pbuffer(pw, ph)?;
    }
    let mut event_loop: EventLoop<'static, ()> = EventLoop::try_new()?;
    // Only the config watcher sends inputs: there is no control socket.
    let (input_tx, inputs) = super::loop_channel::<Input>();
    let app = build(&ui, system, input_tx, solo, Layout::load(), true);
    let a = app.clone();
    // With --bench: how long each system event took to handle, and what it was.
    let slow_events: Rc<std::cell::RefCell<Vec<(f64, String)>>> = Default::default();
    let slow = slow_events.clone();
    let bench = args.bench;
    event_loop
        .handle()
        .insert_source(events, move |ev, _, _| {
            if let channel::Event::Msg(e) = ev {
                if !bench {
                    let _ = a.handle_event(e);
                    return;
                }
                let label: String = format!("{e:?}").chars().take(60).collect();
                let t = Instant::now();
                let _ = a.handle_event(e);
                slow.borrow_mut()
                    .push((t.elapsed().as_secs_f64() * 1000.0, label));
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
            Some(Tab::Arrange) => ui.set_drawer_tab(crate::DrawerTab::Arrange),
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
        app.step_look();
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
    if args.arrange {
        // Once the grid is the screen's.
        app.arrange_now();
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
                app.step_look();
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
        // The GPU's frames as the daemon draws them; the software renderer's into the buffer.
        let frame = |full: bool, pixels: &mut Vec<PremultipliedRgbaColor>| {
            if gpu.is_some() {
                bench_frame(full)
            } else {
                render_into(pixels, pw as usize, full)
            }
        };
        // NIRI_DASH_BENCH_ONLY=open, show or styles: only that part.
        let only = std::env::var("NIRI_DASH_BENCH_ONLY").unwrap_or_default();
        let part = |name: &str| only.is_empty() || only == name;
        const PERIOD: Duration = Duration::from_micros(6060);
        let report = |label: &str, mut times: Vec<f64>| {
            times.sort_by(|a, b| a.total_cmp(b));
            let n = times.len().max(1);
            println!(
                "{label}: frames {} median {:.1} ms p90 {:.1} ms max {:.1} ms",
                times.len(),
                times.get(n / 2).unwrap_or(&0.0),
                times.get(n * 9 / 10).unwrap_or(&0.0),
                times.get(n - 1).unwrap_or(&0.0)
            );
        };
        if part("open") {
            ui.set_shown(false);
            let t = Instant::now();
            while t.elapsed() < Duration::from_millis(300) {
                slint::platform::update_timers_and_animations();
                app.step_look();
                frame(false, &mut pixels);
            }
            ui.set_shown(true);
            let start = Instant::now();
            let mut times = Vec::new();
            while start.elapsed() < Duration::from_millis(900) {
                slint::platform::update_timers_and_animations();
                app.step_look();
                let f = Instant::now();
                if frame(false, &mut pixels).is_some() {
                    times.push(f.elapsed().as_secs_f64() * 1000.0);
                }
            }
            report("open animation", times);
            // The same frame again with nothing moving: what any frame costs, so what the animation
            // adds is the difference.
            let t = Instant::now();
            while t.elapsed() < Duration::from_millis(500) {
                slint::platform::update_timers_and_animations();
                app.step_look();
                frame(false, &mut pixels);
            }
            let mut still = Vec::new();
            for _ in 0..20 {
                let f = Instant::now();
                frame(true, &mut pixels);
                still.push(f.elapsed().as_secs_f64() * 1000.0);
            }
            report("still, full frame", still);
        }
        if part("show") {
            // As the daemon runs: shown and hidden again and again (each show refreshes every
            // source), events handled between frames, one frame per refresh at 165 Hz. A turn of
            // the loop that takes longer than a refresh is a frame shown late.
            let (mut turns, mut late) = (0, 0);
            let (mut handling, mut drawing) = (Vec::new(), Vec::new());
            slow_events.borrow_mut().clear();
            for _ in 0..6 {
                ui.set_shown(false);
                app.tx.host.tell(host::Command::Visible(false));
                let t = Instant::now();
                while t.elapsed() < Duration::from_millis(700) {
                    let _ = event_loop.dispatch(Some(Duration::from_millis(5)), &mut ());
                    slint::platform::update_timers_and_animations();
                    app.step_look();
                    frame(false, &mut pixels);
                }
                app.come_up();
                ui.set_shown(true);
                app.tx.host.tell(host::Command::Visible(true));
                let t = Instant::now();
                while t.elapsed() < Duration::from_millis(1200) {
                    let turn = Instant::now();
                    let _ = event_loop.dispatch(Some(Duration::ZERO), &mut ());
                    let handled = turn.elapsed();
                    slint::platform::update_timers_and_animations();
                    app.step_look();
                    let f = Instant::now();
                    frame(false, &mut pixels);
                    let drawn = f.elapsed();
                    let took = turn.elapsed();
                    turns += 1;
                    if took > PERIOD {
                        late += 1;
                    }
                    handling.push(handled.as_secs_f64() * 1000.0);
                    drawing.push(drawn.as_secs_f64() * 1000.0);
                    std::thread::sleep(PERIOD.saturating_sub(took));
                }
            }
            println!("shown 6 times: {late} of {turns} turns of the loop over a 165 Hz refresh");
            report("  handling events, per turn", handling);
            report("  update + frame, per turn", drawing);
        }
        if part("edit") {
            // Into edit mode and out again: the drawer slides in and the grid makes room.
            let mut turns_all = Vec::new();
            let mut late = 0;
            for i in 0..12 {
                app.set_editing(i % 2 == 0);
                let first = turns_all.len();
                let started = Instant::now();
                let mut moved_late = Vec::new();
                let t = Instant::now();
                while t.elapsed() < Duration::from_millis(500) {
                    let turn = Instant::now();
                    let _ = event_loop.dispatch(Some(Duration::ZERO), &mut ());
                    slint::platform::update_timers_and_animations();
                    app.step_look();
                    frame(false, &mut pixels);
                    let took = turn.elapsed();
                    if took > PERIOD {
                        late += 1;
                        moved_late.push((
                            started.elapsed().as_millis() as u64,
                            (took.as_secs_f64() * 10000.0).round() / 10.0,
                        ));
                    }
                    turns_all.push(took.as_secs_f64() * 1000.0);
                    std::thread::sleep(PERIOD.saturating_sub(took));
                }
                let _ = first;
                if std::env::var_os("NIRI_DASH_BENCH_TURNS").is_some() {
                    // When (ms after the switch) the late turns came, and how long they took.
                    println!(
                        "  {}: late turns {moved_late:?}",
                        if i % 2 == 0 { "in " } else { "out" }
                    );
                }
            }
            println!(
                "edit mode in and out 6 times: {late} of {} turns over a 165 Hz refresh",
                turns_all.len()
            );
            let mut d = turns_all.clone();
            d.sort_by(|a, b| b.total_cmp(a));
            println!(
                "  slowest turns: {:?}",
                d.iter()
                    .take(14)
                    .map(|x| (x * 10.0).round() / 10.0)
                    .collect::<Vec<_>>()
            );
            report("  turn of the loop", turns_all);
        }
        if part("styles") {
            // Switching style presets back and forth, as in the Style tab.
            app.refresh_looks();
            let styles: Vec<String> = slint::Model::iter(&ui.get_styles())
                .map(|s| s.name.to_string())
                .collect();
            let (mut turns, mut late) = (0, 0);
            let mut drawing = Vec::new();
            let mut frames = 0;
            let mut choosing = Vec::new();
            for i in 0..12 {
                let c = Instant::now();
                ui.invoke_choose_style(styles[i % styles.len()].as_str().into());
                choosing.push(c.elapsed().as_secs_f64() * 1000.0);
                let t = Instant::now();
                let mut late_at = Vec::new();
                while t.elapsed() < Duration::from_millis(600) {
                    let turn = Instant::now();
                    let _ = event_loop.dispatch(Some(Duration::ZERO), &mut ());
                    slint::platform::update_timers_and_animations();
                    app.step_look();
                    if frame(false, &mut pixels).is_some() {
                        frames += 1;
                    }
                    let took = turn.elapsed();
                    turns += 1;
                    if took > PERIOD {
                        late += 1;
                        late_at.push((
                            t.elapsed().as_millis() as u64,
                            (took.as_secs_f64() * 10.0 * 1000.0).round() / 10.0,
                        ));
                    }
                    drawing.push(took.as_secs_f64() * 1000.0);
                    std::thread::sleep(PERIOD.saturating_sub(took));
                }
                if std::env::var_os("NIRI_DASH_BENCH_TURNS").is_some() {
                    // When (ms after the click) the late turns came, and how long they took.
                    println!("  {}: late turns {late_at:?}", styles[i % styles.len()]);
                }
            }
            println!(
                "style presets {styles:?} switched 12 times: {late} of {turns} turns over a 165 Hz \
                 refresh, {frames} frames drawn"
            );
            report("  choosing a preset (the click)", choosing);
            {
                let mut d = drawing.clone();
                d.sort_by(|a, b| b.total_cmp(a));
                println!(
                    "  slowest turns: {:?}",
                    d.iter()
                        .take(14)
                        .map(|x| (x * 10.0).round() / 10.0)
                        .collect::<Vec<_>>()
                );
            }
            report("  turn of the loop", drawing);
        }
        let mut slow = slow_events.borrow().clone();
        slow.sort_by(|a, b| b.0.total_cmp(&a.0));
        println!("  slowest events:");
        for (ms, label) in slow.iter().take(12) {
            println!("    {ms:>7.2} ms  {label}");
        }
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

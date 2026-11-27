//! `niri-dash-bui snapshot`: the dashboard drawn offscreen into a PNG, on the mock system (or the
//! simulated one), with keys pressed and clicks made first. For looking at it without a display,
//! and for the tests.

use crate::app::{Shell, with_dash};
use crate::dash::Dash;
use crate::layout::{Kind, Layout};
use crate::view::DashView;
use crate::{Control, app};
use blenderui::prelude::*;
use std::path::PathBuf;
use std::time::Duration;

#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    /// Where the PNG goes.
    #[arg(short, long, default_value = "snapshot.png")]
    pub out: PathBuf,
    #[arg(long, default_value_t = 1600.0)]
    pub width: f32,
    #[arg(long, default_value_t = 900.0)]
    pub height: f32,
    /// The simulated system, which keeps moving, instead of the mock one.
    #[arg(long)]
    pub sim: bool,
    /// Open edit mode.
    #[arg(long)]
    pub edit: bool,
    /// One widget, as `app <widget>` shows it.
    #[arg(long, value_enum)]
    pub app: Option<Kind>,
    /// Keys to press, separated by spaces: `f v j`, `C-e`, `Escape`, `S-Tab`.
    #[arg(long, default_value = "")]
    pub keys: String,
    /// Clicks, as `x,y` separated by spaces.
    #[arg(long, default_value = "")]
    pub clicks: String,
    /// How long to let the system report before drawing, in milliseconds.
    #[arg(long, default_value_t = 400)]
    pub wait: u64,
}

/// The kit's name for a key: "Return", "Escape", "Tab"; a letter stands for itself.
pub fn keysym(k: &str) -> (String, String) {
    match k {
        "Enter" | "Return" => (String::new(), "Return".into()),
        "Escape" | "Esc" => (String::new(), "Escape".into()),
        "Tab" => (String::new(), "Tab".into()),
        "BackSpace" | "Backspace" => (String::new(), "BackSpace".into()),
        "Space" | "space" => (" ".into(), "space".into()),
        "Up" | "Down" | "Left" | "Right" | "Delete" => (String::new(), k.into()),
        c => (c.into(), c.into()),
    }
}

pub fn run(args: &Args) -> Result<(), String> {
    // Never the real system: its agents would register on the session.
    if args.sim {
        unsafe { std::env::set_var("NIRI_DASH_SIM", "1") };
    } else {
        unsafe { std::env::set_var("NIRI_DASH_MOCK", "1") };
    }
    let solo = args.app;
    let mut app = Application::headless().start(|cx| {
        let ui = cx.ui_sender();
        let system =
            app::start_system(app::event_sink(ui.clone()), solo).expect("the mock system starts");
        let config = Layout::read();
        let dash = cx.new(|_| {
            let mut d = Dash::new(system, config, solo);
            d.ui = Some(ui.clone());
            d
        });
        cx.set_global(Shell {
            dash,
            window: None,
            solo,
        });
    });
    std::thread::sleep(Duration::from_millis(args.wait));
    app.run_pending();
    // Show it, in a window of the size asked for.
    with_dash(&mut app, |d, cx| {
        d.control(
            if args.edit {
                Control::Edit
            } else {
                Control::Show
            },
            cx,
        )
    });
    let dash = app.global::<Shell>().dash;
    let opts = app::headless_options(solo, args.width, args.height);
    let win = app
        .open_window(opts, move |_, cx| cx.new(|_| DashView::new(dash)))
        .map_err(|e| e.to_string())?;
    app.global_mut::<Shell>().window = Some(win.id);
    // The Show the dashboard asked for is done: drop it.
    with_dash(&mut app, |d, _| d.ops.clear());
    with_dash(&mut app, |d, _| {
        d.tick();
    });
    app.draw(win.id);
    for k in args.keys.split_whitespace() {
        let (mut ctrl, mut shift) = (false, false);
        let mut rest = k;
        loop {
            if let Some(r) = rest.strip_prefix("C-") {
                ctrl = true;
                rest = r;
            } else if let Some(r) = rest.strip_prefix("S-") {
                shift = true;
                rest = r;
            } else {
                break;
            }
        }
        let (text, sym) = keysym(rest);
        app.simulate_key_with(
            win.id,
            &text,
            &sym,
            ctrl,
            shift || rest.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && rest.len() == 1,
        );
        app.run_pending();
        with_dash(&mut app, |d, _| {
            d.ops.retain(|o| *o != crate::dash::WinOp::Show)
        });
        app.draw(win.id);
    }
    for c in args.clicks.split_whitespace() {
        let Some((x, y)) = c
            .split_once(',')
            .and_then(|(x, y)| Some((x.parse::<f32>().ok()?, y.parse::<f32>().ok()?)))
        else {
            return Err(format!("not a click: {c}"));
        };
        app.simulate_click(win.id, x, y);
        app.run_pending();
        app.draw(win.id);
    }
    app.run_pending();
    let (w, h, px) = app.draw(win.id);
    save(&args.out, w, h, &px)?;
    let delivered = with_dash(&mut app, |d, _| d.data.delivered).unwrap_or(0);
    println!("wrote {} ({w}x{h}, {delivered} events)", args.out.display());
    Ok(())
}

/// 0x00RRGGBB pixels as an RGB PNG.
fn save(path: &std::path::Path, w: usize, h: usize, px: &[u32]) -> Result<(), String> {
    let rgb: Vec<u8> = px
        .iter()
        .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8])
        .collect();
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(&rgb))
        .map_err(|e| e.to_string())
}

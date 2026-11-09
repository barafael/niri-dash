//! `niri-dash snapshot`: the dashboard drawn into a PNG headlessly, on the mock system. Needs a
//! GPU (or a software Vulkan such as lavapipe): where there is none, this says so and passes.
//!
//! A plain program (`harness = false`), as the other tests are.

use clap::Parser;
use niri_dash::snapshot::{Args, run};
use niri_dash::testing::Sandbox;

#[derive(Parser)]
struct Cli {
    #[command(flatten)]
    args: Args,
}

/// Take the snapshot `args` ask for into `out`, or None where there is no renderer.
fn shoot(out: &std::path::Path, args: &[&str]) -> Option<image::RgbaImage> {
    let out_arg = out.to_string_lossy().into_owned();
    let mut argv = vec!["snapshot", "--mock"];
    argv.extend_from_slice(args);
    argv.push(&out_arg);
    let cli = Cli::parse_from(argv);
    if let Err(e) = run(&cli.args) {
        let said = e.to_string();
        if said.contains("no headless renderer") {
            println!("snapshot: skipped, no GPU to draw with ({said})");
            return None;
        }
        panic!("snapshot {args:?}: {said}");
    }
    Some(
        image::open(out)
            .expect("the picture can be read back")
            .to_rgba8(),
    )
}

/// How many different colors a picture has: a blank one has one.
fn colors(image: &image::RgbaImage) -> usize {
    let mut seen = std::collections::HashSet::new();
    for pixel in image.pixels().step_by(7) {
        seen.insert(pixel.0);
    }
    seen.len()
}

fn main() {
    let sandbox = Sandbox::enter(None).expect("a temporary directory for the dashboard's files");
    // SAFETY: nothing else runs yet that could read the environment.
    unsafe { std::env::set_var("LC_ALL", "C") };
    let dir = sandbox.path().to_path_buf();

    // The size is the screen's times the scale, whatever the renderer's own scale is.
    let Some(plain) = shoot(
        &dir.join("plain.png"),
        &[
            "--width", "960", "--height", "540", "--scale", "1", "--wait", "1",
        ],
    ) else {
        return;
    };
    assert_eq!(plain.dimensions(), (960, 540), "the size asked for");
    assert!(
        colors(&plain) > 50,
        "the dashboard is drawn, not a blank picture ({} colors)",
        colors(&plain)
    );
    let big = shoot(
        &dir.join("big.png"),
        &[
            "--width", "400", "--height", "300", "--scale", "2", "--wait", "1",
        ],
    )
    .expect("the renderer was there a moment ago");
    assert_eq!(
        big.dimensions(),
        (800, 600),
        "a scale of 2 doubles the size"
    );

    // Edit mode, a tab, input after the wait: each makes another picture.
    let edit = shoot(
        &dir.join("edit.png"),
        &[
            "--width", "960", "--height", "540", "--scale", "1", "--wait", "1", "--edit", "--tab",
            "colors",
        ],
    )
    .expect("the renderer was there a moment ago");
    assert_ne!(plain.as_raw(), edit.as_raw(), "edit mode draws the sidebar");
    let keyed = shoot(
        &dir.join("keys.png"),
        &[
            "--width", "960", "--height", "540", "--scale", "1", "--wait", "1", "--events", "key:f",
        ],
    )
    .expect("the renderer was there a moment ago");
    assert_ne!(plain.as_raw(), keyed.as_raw(), "f labels the widgets");

    // The same run twice draws the same picture, the clock's seconds aside: the mock answers on
    // this thread, and nothing glides.
    let again = shoot(
        &dir.join("edit2.png"),
        &[
            "--width", "960", "--height", "540", "--scale", "1", "--wait", "1", "--edit", "--tab",
            "colors",
        ],
    )
    .expect("the renderer was there a moment ago");
    let different = edit
        .pixels()
        .zip(again.pixels())
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        different * 200 < (960 * 540),
        "{different} pixels differ between two runs"
    );
    println!("snapshot: ok");
}

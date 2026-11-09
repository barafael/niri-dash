//! What a frame costs and how much it repaints: idle, with the hints up, per system event and per
//! widget. A measuring tool, not a test: it asserts nothing and takes minutes, so `cargo test`
//! leaves it alone (`test = false` in Cargo.toml) and it runs by name, in release mode for
//! numbers that mean something:
//!
//! ```text
//! cargo test --release --test perf_probe             # everything
//! cargo test --release --test perf_probe -- idle     # one of: idle, events, soak, where, kinds
//! ```
//!
//! Headless, on the mock system, with the dashboard's files in a temporary directory.

use niri_dash::testing::{Harness, Sandbox, default_config, kind_names, min_size};
use niri_dash_system::{Demo, bluetooth, claude, notifications, perf, power, tray, wifi, windows};
use std::time::{Duration, Instant};

fn pct(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() as f64 - 1.0) * p) as usize]
}

/// Run `secs` of wall clock on `h`, drawing every `step`; report frames, damage and cost.
fn idle(label: &str, h: &mut Harness, secs: f64, step: Duration, screen: (u32, u32)) {
    let start = Instant::now();
    let (mut frames, mut area, mut times, mut exact) =
        (0u32, Vec::new(), Vec::new(), Vec::<f64>::new());
    let mut shown = 0;
    let mut buckets = vec![0u32; secs.ceil() as usize + 1];
    let mut ticks = 0u32;
    while start.elapsed().as_secs_f64() < secs {
        slint::platform::update_timers_and_animations();
        h.pump();
        let t = Instant::now();
        if let Some(((bx, by, w, hh), rects)) = h.draw(false) {
            exact.push(
                rects.iter().map(|r| f64::from(r.2 * r.3)).sum::<f64>()
                    / f64::from(screen.0 * screen.1)
                    * 100.0,
            );
            if shown < 4 && std::env::var_os("PROBE_RECTS").is_some() {
                shown += 1;
                println!("    frame: bbox ({bx},{by}) {w}x{hh}; rects {:?}", rects);
            }
            frames += 1;
            buckets[start.elapsed().as_secs() as usize] += 1;
            area.push(f64::from(w * hh) / f64::from(screen.0 * screen.1) * 100.0);
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        ticks += 1;
        std::thread::sleep(step);
    }
    println!("    frames per second of the run: {buckets:?}");
    let tot: f64 = area.iter().sum();
    let ex: f64 = exact.iter().sum();
    println!(
        "{label:<28} {:>5.1} frames/s  repaint avg {:>5.1}% of screen  (exact rects {:>5.2}% of screen/s; bbox {:>6.2}%/s)  render p50 {:>5.2}ms p95 {:>5.2}ms  ({ticks} polls)",
        f64::from(frames) / secs,
        if frames > 0 {
            tot / f64::from(frames)
        } else {
            0.0
        },
        ex / secs,
        tot / secs,
        pct(&mut times.clone(), 0.5),
        pct(&mut times.clone(), 0.95),
    );
}

fn full(label: &str, h: &mut Harness, n: u32) {
    let mut times = Vec::new();
    for _ in 0..n {
        slint::platform::update_timers_and_animations();
        let t = Instant::now();
        h.draw(true);
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    println!(
        "{label:<28} full frame p50 {:>6.2}ms p95 {:>6.2}ms max {:>6.2}ms",
        pct(&mut times.clone(), 0.5),
        pct(&mut times.clone(), 0.95),
        pct(&mut times.clone(), 1.0)
    );
}

fn main() {
    let _sb = Sandbox::enter(None).expect("a sandbox");
    unsafe { std::env::set_var("LC_ALL", "C") };
    let mode = std::env::args().nth(1).unwrap_or_else(|| "all".into());
    let big = (1706, 1066);
    let small = (1280, 800);
    if mode == "all" || mode == "idle" {
        for (name, size) in [("large", big), ("small", small)] {
            let mut h = Harness::with_mock_sized(&default_config(), size);
            h.draw(true);
            full(&format!("default config, {name}"), &mut h, 40);
            idle(
                &format!("idle, default, {name}"),
                &mut h,
                12.0,
                Duration::from_millis(16),
                size,
            );
            h.key("f");
            h.pump();
            idle(
                &format!("hints up, {name}"),
                &mut h,
                5.0,
                Duration::from_millis(16),
                size,
            );
        }
    }
    if mode == "all" || mode == "events" {
        println!(
            "\nsystem events into the default layout, large screen: handling time and repaint per event"
        );
        println!("(same = the identical state again; changed = a state that differs each time)");
        let mut h = Harness::with_mock_sized(&default_config(), big);
        h.draw(true);
        let screen = f64::from(big.0 * big.1);
        macro_rules! probe {
            ($name:expr, $same:expr, $changed:expr) => {{
                for (kind, n) in [("same", 200u32), ("changed", 200u32)] {
                    let mut handle = Vec::new();
                    let mut draw = Vec::new();
                    let mut area = 0.0;
                    let mut frames = 0u32;
                    for i in 0..n {
                        let ev = if kind == "same" { $same(i) } else { $changed(i) };
                        let t = Instant::now();
                        h.send(ev);
                        h.pump();
                        handle.push(t.elapsed().as_secs_f64() * 1000.0);
                        slint::platform::update_timers_and_animations();
                        let t = Instant::now();
                        if let Some((_, rects)) = h.draw(false) {
                            frames += 1;
                            area += rects.iter().map(|r| f64::from(r.2 * r.3)).sum::<f64>() / screen * 100.0;
                            draw.push(t.elapsed().as_secs_f64() * 1000.0);
                        }
                    }
                    println!(
                        "{:<16} {kind:<8} handle p50 {:>6.3}ms p95 {:>6.3}ms | frames drawn {frames:>3}/{n}, avg repaint {:>5.2}% of screen, draw p50 {:>6.2}ms",
                        $name, pct(&mut handle.clone(), 0.5), pct(&mut handle.clone(), 0.95),
                        if frames > 0 { area / f64::from(frames) } else { 0.0 }, pct(&mut draw.clone(), 0.5)
                    );
                }
            }};
        }
        let demo = Demo::new();
        probe!(
            "wifi",
            |_| wifi::Event::State(Box::new(demo.wifi.clone())),
            |i: u32| {
                let mut s = demo.wifi.clone();
                s.ssid = format!("net{i}");
                wifi::Event::State(Box::new(s))
            }
        );
        probe!(
            "bluetooth",
            |_| bluetooth::Event::State(Box::new(demo.bluetooth.clone())),
            |i: u32| {
                let mut s = demo.bluetooth.clone();
                s.powered = i.is_multiple_of(2);
                bluetooth::Event::State(Box::new(s))
            }
        );
        probe!(
            "windows",
            |_| windows::Event::State(demo.windows.clone()),
            |i: u32| {
                let mut s = demo.windows.clone();
                if let Some(w) = s.rows.iter_mut().find(|r| !r.header) {
                    w.title = format!("title {i}");
                }
                windows::Event::State(s)
            }
        );
        probe!(
            "claude",
            |_| claude::Event::Sessions(demo.claude.clone()),
            |i: u32| {
                let mut s = demo.claude.clone();
                if let Some(w) = s.sessions.get_mut(0) {
                    w.title = format!("task {i}");
                }
                claude::Event::Sessions(s)
            }
        );
        probe!(
            "tray",
            |_| tray::Event::Items(demo.tray.clone()),
            |i: u32| {
                let mut s = demo.tray.clone();
                if let Some(w) = s.get_mut(0) {
                    w.title = format!("app {i}");
                }
                tray::Event::Items(s)
            }
        );
        probe!(
            "notifications",
            |_| notifications::Event::State(demo.notifications.clone()),
            |i: u32| {
                let mut s = demo.notifications.clone();
                s.dnd = if i.is_multiple_of(2) {
                    notifications::Dnd::Forever
                } else {
                    notifications::Dnd::Off
                };
                notifications::Event::State(s)
            }
        );
        probe!(
            "power",
            |_| power::Event::State(demo.power.clone()),
            |i: u32| {
                let mut s = demo.power.clone();
                s.percent = (i % 100) as i32;
                power::Event::State(s)
            }
        );
        probe!(
            "perf",
            |_| perf::Event::Snapshot(Box::new(demo.perf.clone())),
            |i: u32| {
                let mut s = demo.perf.clone();
                s.cpu = (i % 100) as f32;
                s.cpu_text = format!("{}%", i % 100);
                perf::Event::Snapshot(Box::new(s))
            }
        );
    }
    if mode == "soak" {
        let secs: u64 = std::env::args()
            .nth(2)
            .and_then(|s| s.parse().ok())
            .unwrap_or(60);
        let size = big;
        let mut h = Harness::with_sim(&default_config(), size);
        h.draw(true);
        let screen = f64::from(size.0 * size.1);
        let rss = || {
            std::fs::read_to_string("/proc/self/statm")
                .ok()
                .and_then(|s| {
                    s.split_whitespace()
                        .nth(1)
                        .and_then(|n| n.parse::<u64>().ok())
                })
                .unwrap_or(0)
                * 4
                / 1024
        };
        println!(
            "soak on the simulated system, {secs} s, default config, {}x{}",
            size.0, size.1
        );
        println!(
            "{:>5} {:>7} {:>7} {:>9} {:>10} {:>9} {:>8}",
            "sec", "events", "frames", "repaint%", "render ms", "max ms", "rss MB"
        );
        let start = Instant::now();
        let (mut ev0, mut frames, mut area, mut rt, mut rmax) =
            (0u64, 0u32, 0.0f64, 0.0f64, 0.0f64);
        let mut next = 10u64;
        let mut cpu_busy = Duration::ZERO;
        while start.elapsed().as_secs() < secs {
            let t0 = Instant::now();
            slint::platform::update_timers_and_animations();
            h.pump();
            let t = Instant::now();
            if let Some((_, rects)) = h.draw(false) {
                let ms = t.elapsed().as_secs_f64() * 1000.0;
                frames += 1;
                rt += ms;
                rmax = rmax.max(ms);
                area += rects.iter().map(|r| f64::from(r.2 * r.3)).sum::<f64>() / screen * 100.0;
            }
            cpu_busy += t0.elapsed();
            if start.elapsed().as_secs() >= next {
                let ev = h.delivered();
                println!(
                    "{:>5} {:>7} {:>7} {:>9.2} {:>10.2} {:>9.2} {:>8}",
                    next,
                    ev - ev0,
                    frames,
                    area,
                    if frames > 0 {
                        rt / f64::from(frames)
                    } else {
                        0.0
                    },
                    rmax,
                    rss()
                );
                ev0 = ev;
                frames = 0;
                area = 0.0;
                rt = 0.0;
                rmax = 0.0;
                next += 10;
            }
            std::thread::sleep(Duration::from_millis(16));
        }
        println!(
            "busy (pump+draw) {:.2}s of {secs}s = {:.2}% of one core, polling at 60 Hz",
            cpu_busy.as_secs_f64(),
            cpu_busy.as_secs_f64() / secs as f64 * 100.0
        );
    }
    if mode == "where" {
        let size = big;
        let mut h = Harness::with_sim(&default_config(), size);
        h.draw(true);
        let mut hist: std::collections::BTreeMap<(i32, i32, u32, u32), u32> = Default::default();
        let mut frames = 0;
        let start = Instant::now();
        while start.elapsed().as_secs() < 30 {
            slint::platform::update_timers_and_animations();
            h.pump();
            if let Some((_, rects)) = h.draw(false) {
                frames += 1;
                for r in rects {
                    *hist.entry(r).or_default() += 1;
                }
            }
            std::thread::sleep(Duration::from_millis(16));
        }
        let mut v: Vec<_> = hist.into_iter().collect();
        v.sort_by_key(|x| std::cmp::Reverse(x.1));
        println!(
            "{}",
            default_config()
                .lines()
                .filter(|l| l.starts_with("    ") || l.starts_with("grid"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        println!("{frames} frames in 30 s; the most repainted rectangles (x, y, w, h): count");
        for (r, n) in v.iter().take(16) {
            println!("  {:?}: {n}", r);
        }
        // Where the cells are: print the layout's tiles with pixel positions of a 100px grid.
    }
    if mode == "all" || mode == "kinds" {
        println!("\nper widget, alone, large screen (cell {} px):", 100);
        for kind in kind_names() {
            let (w, hh) = min_size(kind);
            let (w, hh) = (w.max(4), hh.max(3));
            let cfg = format!(
                "grid columns=17 rows=10\nlayout {{\n  {kind} col=0 row=0 width={w} height={hh}\n}}\n"
            );
            let mut h = Harness::with_mock_sized(&cfg, big);
            h.draw(true);
            full(&format!("{kind} {w}x{hh}"), &mut h, 20);
            idle(
                &format!("  idle {kind}"),
                &mut h,
                4.0,
                Duration::from_millis(16),
                big,
            );
        }
    }
}

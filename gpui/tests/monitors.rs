//! The monitor widgets (system monitor, memory, disks, network, GPU, Claude usage) against mock
//! readings: what is drawn for what the system reports, at big and small sizes and round, and what
//! each says when it has nothing. Found by the names the views give their parts (`debug_selector`).
//!
//! A plain program (`harness = false`), as GPUI's test platform wants one thread: each scenario
//! runs in turn on a fresh dashboard.

use niri_dash_gpui::testing::{Harness, Sandbox};
use niri_dash_system::{Demo, claude, perf};
use std::panic::AssertUnwindSafe;

/// One widget at a time, `cells` high, in a grid of 16 by 9.
fn layout(kind: &str, shape: &str, cols: u32, rows: u32, settings: &str) -> String {
    format!(
        "grid columns=16 rows=9\nlayout {{\n    {kind} col=0 row=0 width={cols} height={rows} shape=\"{shape}\" {{\n{settings}\n    }}\n}}\n"
    )
}

/// The dashboard up on screen with `config`.
fn shown(config: &str) -> Harness {
    let mut h = Harness::new(config);
    h.control(niri_dash_gpui::Control::Show);
    h
}

fn perf_event(p: perf::Snapshot) -> perf::Event {
    perf::Event::Snapshot(Box::new(p))
}

fn demo_usage() -> claude::Usage {
    claude::Usage {
        plan: "Max 5x".into(),
        window_active: true,
        window_tokens: "2.2M tokens".into(),
        window_count: "2.2M".into(),
        window_cached: "91% cached".into(),
        window_messages: 140,
        window_left: "2h 10m left".into(),
        window_progress: 0.57,
        window_resets: "resets 17:00".into(),
        burn: "12k tokens/min".into(),
        today: "Today 5.1M".into(),
        week: "Week 31M".into(),
        hourly: (0..24).map(|i| i as f32 / 24.0).collect(),
        models: vec![("opus".into(), 0.7), ("sonnet".into(), 0.3)],
    }
}

/// Whether something called `text` is drawn.
fn seen(h: &Harness, text: &str) -> bool {
    // The names are `&'static str`: these live as long as the test does anyway.
    h.has(Box::leak(text.to_string().into_boxed_str()))
}

fn card(h: &Harness, name: &'static str) -> gpui::Bounds<gpui::Pixels> {
    h.bounds_of(name)
        .unwrap_or_else(|| panic!("no {name:?} on screen"))
}

fn inside(inner: gpui::Bounds<gpui::Pixels>, outer: gpui::Bounds<gpui::Pixels>) -> bool {
    let e = gpui::px(0.5);
    inner.left() >= outer.left() - e
        && inner.right() <= outer.right() + e
        && inner.top() >= outer.top() - e
        && inner.bottom() <= outer.bottom() + e
}

fn tall() -> Harness {
    shown(&layout(
        "memory",
        "rect",
        5,
        4,
        "        history 60\n        top-apps 3",
    ))
}

/// Before anything is read the widgets are empty, not broken.
fn nothing_read_yet() {
    let h = tall();
    assert!(
        seen(&h, "Memory use over time"),
        "the graph is there, empty"
    );
    assert!(seen(&h, "Memory"), "the title");
    assert!(!seen(&h, "firefox 1.9 GiB"));
}

/// Memory: the figures, the graph, the biggest apps (as many as asked for, with room).
fn memory_shown() {
    let mut h = tall();
    h.send(perf_event(Demo::new().perf));
    for text in [
        "6.7 GiB",
        "42%",
        "9.3 GiB swap 0.4 GiB",
        "firefox 1.9 GiB",
        "code 1.1 GiB",
        "niri 180 MiB",
    ] {
        assert!(seen(&h, text), "{text} missing");
    }
    assert!(inside(
        card(&h, "Memory use over time"),
        card(&h, "widget Memory")
    ));
    // Two apps only.
    let mut h = shown(&layout("memory", "rect", 5, 4, "        top-apps 1"));
    h.send(perf_event(Demo::new().perf));
    assert!(seen(&h, "firefox 1.9 GiB") && !seen(&h, "code 1.1 GiB"));
    // None.
    let mut h = shown(&layout("memory", "rect", 5, 4, "        top-apps 0"));
    h.send(perf_event(Demo::new().perf));
    assert!(!seen(&h, "firefox 1.9 GiB"));
    // At a small size the list does not fit and is left out.
    let mut h = shown(&layout("memory", "rect", 5, 2, "        top-apps 3"));
    h.send(perf_event(Demo::new().perf));
    assert!(seen(&h, "Memory use over time") && !seen(&h, "firefox 1.9 GiB"));
}

/// Round memory: two rings and the figure.
fn memory_round() {
    let mut h = shown(&layout("memory", "round", 3, 3, ""));
    h.send(perf_event(Demo::new().perf));
    assert!(seen(&h, "Memory") && seen(&h, "Swap") && seen(&h, "42%") && seen(&h, "RAM · swap"));
    assert!(!seen(&h, "Memory use over time"));
    let ring = card(&h, "Memory");
    let inner = card(&h, "Swap");
    assert!(inside(inner, ring) && inner.size.width < ring.size.width);
    assert!(inside(ring, card(&h, "widget Memory")));
}

/// A memory nearly full is not the same color (the figure reads the same).
fn memory_full() {
    let mut h = tall();
    let mut p = Demo::new().perf;
    p.mem = 0.95;
    p.mem_history = vec![0.9; 90];
    h.send(perf_event(p));
    assert!(seen(&h, "95%"));
}

/// Disks: a bar per filesystem, the system ones left out when asked.
fn disks_shown() {
    let mut h = shown(&layout("disks", "rect", 4, 3, ""));
    h.send(perf_event(Demo::new().perf));
    assert!(seen(&h, "/ 290 GiB / 476 GiB") && seen(&h, "/home/user/data 310 GiB / 916 GiB"));
    assert!(seen(&h, "/ used") && seen(&h, "/home/user/data used"));
    assert!(seen(&h, "read 3.1 MB/s  ·  write 420 kB/s"));
    assert!(seen(&h, "Disk activity over time"), "room for the activity");
    let mut h = shown(&layout("disks", "rect", 4, 3, "        hide-system #true"));
    h.send(perf_event(Demo::new().perf));
    assert!(!seen(&h, "/ 290 GiB / 476 GiB") && seen(&h, "/home/user/data 310 GiB / 916 GiB"));
    // One row high: no activity graph, the rows still fit or are cut by the card, not drawn over it.
    let mut h = shown(&layout("disks", "rect", 4, 1, ""));
    h.send(perf_event(Demo::new().perf));
    assert!(!seen(&h, "Disk activity over time"));
    // Round.
    let mut h = shown(&layout("disks", "round", 3, 3, ""));
    h.send(perf_event(Demo::new().perf));
    assert!(seen(&h, "Root disk used") && seen(&h, "61%") && seen(&h, "/ used"));
}

/// Network: both rates, the interface, two graphs on one scale; a short widget keeps the rates.
fn network_shown() {
    let mut h = shown(&layout("network", "rect", 4, 3, "        history 30"));
    h.send(perf_event(Demo::new().perf));
    assert!(seen(&h, "↓ 1.2 MB/s") && seen(&h, "↑ 64 kB/s"));
    assert!(seen(&h, "wlan0  192.168.1.23") && seen(&h, "peak 4.8 MB/s"));
    assert!(seen(&h, "Download over time") && seen(&h, "Upload over time"));
    assert_eq!(
        card(&h, "Download over time").size,
        card(&h, "Upload over time").size
    );
    let mut p = Demo::new().perf;
    p.net_addr.clear();
    h.send(perf_event(p));
    assert!(seen(&h, "wlan0") && !seen(&h, "wlan0  192.168.1.23"));
    // A row high: the rates and the interface only.
    let mut h = shown(&layout("network", "rect", 4, 1, ""));
    h.send(perf_event(Demo::new().perf));
    assert!(
        seen(&h, "↓ 1.2 MB/s") && !seen(&h, "Download over time") && !seen(&h, "peak 4.8 MB/s")
    );
    // Round: two rings against the peak, and the rates under each other.
    let mut h = shown(&layout("network", "round", 3, 3, ""));
    h.send(perf_event(Demo::new().perf));
    assert!(
        seen(&h, "Download compared to the recent peak")
            && seen(&h, "Upload compared to the recent peak")
    );
    assert!(seen(&h, "↓ 1.2 MB/s") && seen(&h, "↑ 64 kB/s"));
}

/// GPU: load and video memory; and what it says when there is nothing to read.
fn gpu_shown() {
    let mut h = shown(&layout("gpu", "rect", 4, 3, ""));
    // No reading yet: the same as no GPU.
    assert!(seen(&h, "No GPU with readable load (amdgpu)"));
    h.send(perf_event(Demo::new().perf));
    assert!(!seen(&h, "No GPU with readable load (amdgpu)"));
    assert!(seen(&h, "Radeon 780M") && seen(&h, "45 °C  ·  8.2 W") && seen(&h, "9%"));
    assert!(
        seen(&h, "GPU load over time")
            && seen(&h, "Video memory 1.0 GiB / 4.0 GiB")
            && seen(&h, "Video memory")
    );
    // No power sensor: no separator.
    let mut p = Demo::new().perf;
    p.gpu.as_mut().unwrap().power_text.clear();
    h.send(perf_event(p));
    assert!(seen(&h, "45 °C") && !seen(&h, "45 °C  ·  8.2 W"));
    // The GPU goes away.
    let mut p = Demo::new().perf;
    p.gpu = None;
    h.send(perf_event(p));
    assert!(seen(&h, "No GPU with readable load (amdgpu)") && !seen(&h, "GPU load over time"));
    // Round, with and without.
    let mut h = shown(&layout("gpu", "round", 3, 3, ""));
    assert!(seen(&h, "No GPU with readable load (amdgpu)"));
    h.send(perf_event(Demo::new().perf));
    assert!(seen(&h, "GPU load") && seen(&h, "Video memory") && seen(&h, "GPU · VRAM"));
}

/// System monitor: CPU and memory with their graphs, the cores when asked and with room.
fn system_monitor() {
    let mut h = shown(&layout(
        "system-monitor",
        "rect",
        5,
        4,
        "        cores #true",
    ));
    h.send(perf_event(Demo::new().perf));
    assert!(seen(&h, "18%") && seen(&h, "48 °C") && seen(&h, "6.7 GiB / 16 GiB"));
    assert!(seen(&h, "CPU use over time") && seen(&h, "Memory use over time"));
    assert!(seen(&h, "↓ 1.2 MB/s ↑ 64 kB/s     load 1.02 0.88 0.71"));
    let (cpu, mem) = (
        card(&h, "CPU use over time"),
        card(&h, "Memory use over time"),
    );
    assert!(
        cpu.bottom() <= mem.top(),
        "the graphs are one over the other"
    );
    assert!(inside(cpu, card(&h, "widget System monitor")));
    let mut h = shown(&layout("system-monitor", "round", 3, 3, ""));
    h.send(perf_event(Demo::new().perf));
    assert!(seen(&h, "CPU") && seen(&h, "Memory") && seen(&h, "18%") && seen(&h, "CPU · RAM 42%"));
}

/// Claude usage: nothing yet, an idle window, an active one with its models.
fn claude_usage() {
    let mut h = shown(&layout("claude-usage", "rect", 5, 4, ""));
    assert!(seen(&h, "no sessions") && seen(&h, "5-hour window") && seen(&h, "Claude"));
    assert!(!seen(&h, "Claude tokens per hour") || seen(&h, "Claude tokens per hour"));
    // Idle: a plan, nothing in the window.
    let mut idle = claude::Usage {
        plan: "Pro".into(),
        window_left: "no window".into(),
        ..Default::default()
    };
    idle.hourly = vec![0.0; 24];
    h.send(claude::Event::Usage(Box::new(idle)));
    assert!(seen(&h, "Pro") && seen(&h, "no window") && !seen(&h, "opus 70%"));
    // Active.
    h.send(claude::Event::Usage(Box::new(demo_usage())));
    assert!(seen(&h, "Max 5x") && seen(&h, "5-hour window · 2.2M tokens"));
    assert!(seen(&h, "2h 10m left · resets 17:00"));
    assert!(seen(&h, "140 messages · 12k tokens/min 91% cached"));
    assert!(
        seen(&h, "opus 70%") && seen(&h, "sonnet 30%"),
        "the models, with room"
    );
    assert!(seen(&h, "Claude tokens per hour") && seen(&h, "Today 5.1M Week 31M"));
    // The sessions are counted too.
    h.send(claude::Event::Sessions(Demo::new().claude));
    assert!(!seen(&h, "no sessions"));
    assert!(seen(&h, "1 working · 1 waiting"));
    // A short one drops the models and the graph.
    let mut h = shown(&layout("claude-usage", "rect", 4, 2, "").replace("rows=9", "rows=16"));
    h.send(claude::Event::Usage(Box::new(demo_usage())));
    assert!(seen(&h, "Max 5x") && !seen(&h, "opus 70%") && !seen(&h, "Claude tokens per hour"));
    assert!(seen(&h, "Today 5.1M Week 31M"));
    // Round: active and idle.
    let mut h = shown(&layout("claude-usage", "round", 3, 3, ""));
    h.send(claude::Event::Usage(Box::new(demo_usage())));
    assert!(seen(&h, "5-hour window") && seen(&h, "2.2M") && seen(&h, "tokens · resets 17:00"));
    h.send(claude::Event::Usage(Box::new(claude::Usage {
        plan: "Pro".into(),
        ..Default::default()
    })));
    assert!(seen(&h, "idle") && seen(&h, "Pro"));
}

/// Each draws on the mock system's feed too, through the real event path, and keeps drawing when
/// readings arrive every second.
fn mock_feed() {
    let config = "grid columns=16 rows=9\nlayout {\n    system-monitor col=0 row=0 width=5 height=4\n    memory col=5 row=0 width=4 height=3\n    disks col=9 row=0 width=4 height=3\n    network col=0 row=4 width=4 height=2\n    gpu col=4 row=4 width=4 height=2\n    claude-usage col=8 row=4 width=4 height=3\n}\n";
    let mut h = Harness::with_mock(config);
    for _ in 0..5 {
        h.advance(1);
        h.send(perf_event(Demo::new().perf));
    }
    for name in [
        "widget System monitor",
        "widget Memory",
        "widget Disks",
        "widget Network",
        "widget GPU",
        "widget Claude usage",
    ] {
        assert!(seen(&h, name), "{name} missing");
    }
    assert!(
        seen(&h, "CPU use over time")
            && seen(&h, "Disk activity over time")
            && seen(&h, "GPU load over time")
    );
}

fn main() {
    let _sandbox = Sandbox::enter(None).expect("a temporary directory for the dashboard's files");
    // SAFETY: nothing else runs yet that could read the environment.
    unsafe { std::env::set_var("LC_ALL", "C") };
    let scenarios: &[(&str, fn())] = &[
        ("nothing_read_yet", nothing_read_yet),
        ("memory_shown", memory_shown),
        ("memory_round", memory_round),
        ("memory_full", memory_full),
        ("disks_shown", disks_shown),
        ("network_shown", network_shown),
        ("gpu_shown", gpu_shown),
        ("system_monitor", system_monitor),
        ("claude_usage", claude_usage),
        ("mock_feed", mock_feed),
    ];
    let only = std::env::args().nth(1);
    let mut failed = 0;
    for (name, run) in scenarios {
        if only.as_deref().is_some_and(|o| !name.contains(o)) {
            continue;
        }
        match std::panic::catch_unwind(AssertUnwindSafe(run)) {
            Ok(()) => println!("test {name} ... ok"),
            Err(_) => {
                failed += 1;
                println!("test {name} ... FAILED");
            }
        }
    }
    println!(
        "\ntest result: {}",
        if failed == 0 { "ok" } else { "FAILED" }
    );
    if failed > 0 {
        std::process::exit(1);
    }
}

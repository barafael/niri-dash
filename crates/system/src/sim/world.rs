//! The parts of the simulated world nobody can change, as plain functions of the time. Which is
//! why a graph has its whole history from the first frame on, and why two pages open at the same
//! moment show the same world.

use super::time::{Scene, by_scene, hash, into, mix, noise, scene, scene_age, slot, wave};
use crate::{claude, perf, power, privacy, weather};
use chrono::{DateTime, Datelike, Days, Local, TimeZone, Timelike};

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
const MIB: f64 = 1024.0 * 1024.0;
const HISTORY: usize = 90;
const CORES: usize = 8;

/// A moment as local time.
pub fn local(t: f64) -> DateTime<Local> {
    Local
        .timestamp_millis_opt((t * 1000.0) as i64)
        .single()
        .unwrap_or_else(Local::now)
}

fn bytes(b: f64) -> String {
    perf::human_bytes(b.max(0.0) as u64)
}

/// CPU load: what the scene asks for, plus the small things always going on.
pub fn cpu(t: f64) -> f64 {
    let base = by_scene(t, 15.0, |s| match s {
        Scene::Coding => 0.15,
        Scene::Building => 0.88,
        Scene::Browsing => 0.24,
        Scene::Call => 0.36,
        Scene::Idle => 0.04,
    });
    let spikes = noise(t, 2.3, 2).powi(4) * 0.3;
    (base + 0.14 * (noise(t, 6.1, 1) - 0.5) + spikes).clamp(0.01, 1.0)
}

fn core(t: f64, i: usize) -> f64 {
    (cpu(t) * (0.45 + 1.1 * noise(t, 4.3, 40 + i as u64))).clamp(0.0, 1.0)
}

fn mem(t: f64) -> f64 {
    let scene_part = by_scene(t, 40.0, |s| match s {
        Scene::Building => 0.16,
        Scene::Call => 0.05,
        Scene::Browsing => 0.06,
        _ => 0.0,
    });
    0.3 + 0.1 * wave(t, 1913.0, 0.2) + 0.05 * noise(t, 240.0, 5) + scene_part
}

/// Bytes per second down and up.
fn net(t: f64) -> (f64, f64) {
    let burst = noise(t, 3.7, 6).powi(3);
    let down = by_scene(t, 8.0, |s| match s {
        Scene::Coding => 90e3,
        Scene::Building => 2.8e6,
        Scene::Browsing => 5.5e6,
        Scene::Call => 420e3,
        Scene::Idle => 6e3,
    });
    let up = by_scene(t, 8.0, |s| match s {
        Scene::Call => 380e3,
        Scene::Browsing => 60e3,
        Scene::Coding => 18e3,
        _ => 6e3,
    });
    (
        2e3 + down * (0.2 + 1.3 * burst),
        1e3 + up * (0.5 + noise(t, 5.3, 7)),
    )
}

fn disk_io(t: f64) -> f64 {
    by_scene(t, 6.0, |s| match s {
        Scene::Building => 0.75,
        Scene::Coding => 0.08,
        _ => 0.04,
    }) * (0.3 + noise(t, 3.1, 8))
}

fn gpu_busy(t: f64) -> f64 {
    let base = by_scene(t, 10.0, |s| match s {
        Scene::Call => 0.42,
        Scene::Browsing => 0.16,
        Scene::Coding => 0.07,
        Scene::Building => 0.05,
        Scene::Idle => 0.02,
    });
    (base + 0.08 * (noise(t, 4.7, 9) - 0.3)).clamp(0.0, 1.0)
}

fn history(now: f64, f: impl Fn(f64) -> f64) -> Vec<f32> {
    (0..HISTORY)
        .map(|i| f(now - (HISTORY - 1 - i) as f64) as f32)
        .collect()
}

/// What the performance widgets show, `online` or not.
pub fn perf(t: f64, online: bool, addr: &str) -> perf::Snapshot {
    let now = t.floor();
    let (cpu_now, mem_now) = (cpu(now), mem(now));
    let total_mem = 32.0 * GIB;
    let used_mem = mem_now * total_mem;
    let swap = 0.04 + 0.08 * wave(now, 2711.0, 0.6);
    let total_swap = 8.0 * GIB;
    let rates = |at: f64| if online { net(at) } else { (0.0, 0.0) };
    let (rx, tx) = rates(now);
    let peak = (0..HISTORY)
        .map(|i| {
            let (d, u) = rates(now - i as f64);
            d.max(u)
        })
        .fold(64e3_f64, f64::max);
    let share = |v: f64| (v / peak).min(1.0);
    // Averages of the last minutes, as the kernel's load figures are.
    let load = |minutes: f64| {
        let n = 12.0;
        CORES as f64
            * (0..n as i32)
                .map(|i| cpu(now - minutes * 5.0 * i as f64))
                .sum::<f64>()
            / n
    };
    let temp = 38.0 + 46.0 * ((0..30).map(|i| cpu(now - i as f64)).sum::<f64>() / 30.0);
    let mut top: Vec<(&str, f64)> = vec![
        ("firefox", 2.1 * GIB * (0.8 + 0.6 * noise(now, 90.0, 11))),
        ("code", 1.3 * GIB * (0.9 + 0.3 * noise(now, 120.0, 12))),
        (
            "rust-analyzer",
            2.4 * GIB * (0.8 + 0.3 * noise(now, 200.0, 13)),
        ),
        ("claude", 420.0 * MIB * (0.8 + 0.5 * noise(now, 60.0, 14))),
        ("niri", 180.0 * MIB * (1.0 + 0.1 * noise(now, 300.0, 15))),
    ];
    if scene(now) == Scene::Building {
        top.push(("rustc", 3.8 * GIB * (0.5 + noise(now, 15.0, 16))));
    }
    top.sort_by(|a, b| b.1.total_cmp(&a.1));
    let io_read = disk_io(now) * 420.0 * MIB;
    let io_write = disk_io(now - 2.0) * 180.0 * MIB;
    let root_used = 0.61 + 0.02 * wave(now, 86_400.0, 0.0);
    let disk = |mount: &str, system, used: f64, total: f64| perf::DiskRow {
        mount: mount.into(),
        system,
        used: used as f32,
        text: format!("{} / {}", bytes(used * total), bytes(total)),
    };
    let vram = 0.2 + 0.25 * gpu_busy(now);
    let busy = gpu_busy(now);
    perf::Snapshot {
        cpu: cpu_now as f32,
        mem: mem_now as f32,
        cpu_history: history(now, cpu),
        mem_history: history(now, mem),
        cpu_text: format!("{:.0}%  ·  {CORES} cores", cpu_now * 100.0),
        mem_text: format!(
            "{} / {}  ·  swap {}",
            bytes(used_mem),
            bytes(total_mem),
            bytes(swap * total_swap)
        ),
        temp_text: format!("{temp:.0} °C"),
        net_text: format!("↓ {}/s  ↑ {}/s", bytes(rx), bytes(tx)),
        load_text: format!("load {:.2} {:.2} {:.2}", load(1.0), load(5.0), load(15.0)),
        cores: (0..CORES).map(|i| core(now, i) as f32).collect(),
        mem_used_text: format!("{} of {}", bytes(used_mem), bytes(total_mem)),
        mem_avail_text: format!("{} available", bytes(total_mem - used_mem)),
        swap: swap as f32,
        swap_text: format!("swap {} / {}", bytes(swap * total_swap), bytes(total_swap)),
        top_mem: top
            .into_iter()
            .take(5)
            .map(|(name, b)| (name.to_string(), bytes(b)))
            .collect(),
        net_rx_history: history(now, |at| share(rates(at).0)),
        net_tx_history: history(now, |at| share(rates(at).1)),
        net_rx_text: format!("{}/s", bytes(rx)),
        net_tx_text: format!("{}/s", bytes(tx)),
        net_peak_text: format!("peak {}/s", bytes(peak)),
        net_iface: "wlan0".into(),
        net_addr: addr.to_string(),
        net_rx_frac: share(rx) as f32,
        net_tx_frac: share(tx) as f32,
        disks: vec![
            disk("/", false, root_used, 476.0 * GIB),
            disk("/boot", true, 0.28, 1.0 * GIB),
            disk(
                "/home/user/data",
                false,
                0.34 + 0.05 * wave(now, 604_800.0, 0.1),
                916.0 * GIB,
            ),
        ],
        disk_history: history(now, |at| disk_io(at).min(1.0)),
        disk_read_text: format!("read {}/s", bytes(io_read)),
        disk_write_text: format!("write {}/s", bytes(io_write)),
        root_used: root_used as f32,
        gpu: Some(perf::Gpu {
            name: "Radeon 780M".into(),
            busy: busy as f32,
            history: history(now, gpu_busy),
            vram: vram as f32,
            vram_text: format!("{} / {}", bytes(vram * 4.0 * GIB), bytes(4.0 * GIB)),
            temp_text: format!("{:.0} °C", 38.0 + 34.0 * busy),
            power_text: format!("{:.1} W", 2.5 + 26.0 * busy),
        }),
    }
}

/// "1h 05min" or "40 min", as the real battery says it.
fn duration_text(minutes: f64) -> String {
    let mins = minutes.round() as i64;
    if mins >= 60 {
        format!("{}h {:02}min", mins / 60, mins % 60)
    } else {
        format!("{mins} min")
    }
}

/// The battery goes down over 95 minutes and is plugged in for 25, with the draw following the CPU.
pub fn battery(t: f64) -> (i32, bool, String, String) {
    const DRAIN: f64 = 95.0 * 60.0;
    const CHARGE: f64 = 25.0 * 60.0;
    const LOW: f64 = 22.0;
    const CAPACITY_WH: f64 = 57.0;
    let at = into(t, DRAIN + CHARGE);
    if at < DRAIN {
        let percent = 100.0 - (100.0 - LOW) * at / DRAIN;
        // Over a few minutes, as the real figure is: it doesn't jump with every spike.
        let window = 180;
        let load = (0..window).map(|i| cpu(t - f64::from(i))).sum::<f64>() / f64::from(window);
        let watts = 5.5 + 21.0 * load + 8.0 * gpu_busy(t);
        (
            percent.round() as i32,
            false,
            format!(
                "{} left",
                duration_text(percent / 100.0 * CAPACITY_WH / watts * 60.0)
            ),
            format!("{watts:.1} W"),
        )
    } else {
        let done = (at - DRAIN) / CHARGE;
        let watts = 58.0 - 26.0 * done + 3.0 * noise(t, 9.0, 21);
        (
            mix(LOW, 100.0, done).round() as i32,
            true,
            format!("Charging · full in {}", duration_text((1.0 - done) * 25.0)),
            format!("{watts:.1} W"),
        )
    }
}

/// The peripherals' batteries, the headphones' among them if they are connected.
pub fn peripherals(t: f64, headphones: Option<i32>) -> Vec<power::Peripheral> {
    let peripheral = |kind, name: &str, percent| power::Peripheral {
        kind,
        name: name.into(),
        percent,
    };
    let mut list = vec![
        peripheral(
            power::DeviceKind::Mouse,
            "MX Master 3S",
            mix(18.0, 100.0, 1.0 - into(t, 90_000.0) / 90_000.0) as i32,
        ),
        peripheral(
            power::DeviceKind::Keyboard,
            "MX Keys",
            mix(40.0, 100.0, 1.0 - into(t, 400_000.0) / 400_000.0) as i32,
        ),
    ];
    if let Some(percent) = headphones {
        list.push(peripheral(
            power::DeviceKind::Headphones,
            "Fairbuds",
            percent,
        ));
    }
    list
}

/// What blocks sleep: the player, while something plays, and the call.
pub fn sleep_blockers(playing: bool, t: f64) -> Vec<power::Blocker> {
    let blocker = |who: &str, why: &str| power::Blocker {
        who: who.into(),
        why: why.into(),
    };
    let mut list = Vec::new();
    if scene(t) == Scene::Call {
        list.push(blocker("Firefox", "Video call in progress"));
    }
    if playing {
        list.push(blocker("Music", "Playing audio"));
    }
    list
}

/// WMO weather codes, in the order the sky goes through them.
const SKY: [i32; 14] = [1, 2, 2, 3, 3, 61, 80, 3, 2, 0, 0, 1, 45, 2];

fn is_day(t: f64) -> bool {
    (7..19).contains(&local(t).hour())
}

/// What the forecast says for a date: a code, a high and a low, the same all day.
fn forecast(ordinal: u64) -> (i32, f64, f64) {
    let code = SKY[(hash(31, ordinal) * SKY.len() as f64) as usize];
    let high = 9.0 + 9.0 * hash(32, ordinal);
    let low = high - 4.0 - 5.0 * hash(33, ordinal);
    (code, high, low)
}

/// Current conditions move through the sky list every 23 minutes; the temperature follows the sun.
pub fn weather(t: f64, place: &str, fahrenheit: bool) -> weather::State {
    let deg = |c: f64| {
        let v = if fahrenheit { c * 9.0 / 5.0 + 32.0 } else { c };
        format!("{v:.0}°")
    };
    let when = local(t);
    let today = when.date_naive();
    let ordinal = today.num_days_from_ce() as u64;
    let (_, high, low) = forecast(ordinal);
    let hour = when.hour() as f64 + when.minute() as f64 / 60.0;
    let sun = ((hour - 9.0) / 24.0 * std::f64::consts::TAU).sin() / 2.0 + 0.5;
    let temp = mix(low, high, sun) + 1.2 * (noise(t, 1700.0, 34) - 0.5);
    let wind = 6.0 + 24.0 * noise(t, 2100.0, 35);
    let code = SKY[slot(t, 23.0 * 60.0, SKY.len())];
    let (icon, description) = weather::describe(code, is_day(t));
    let days = (1..=5)
        .filter_map(|i| {
            let date = today.checked_add_days(Days::new(i))?;
            let (code, high, low) = forecast(ordinal + i);
            Some(weather::Day {
                name: date.format("%a").to_string(),
                icon: weather::describe(code, true).0,
                max: deg(high),
                min: deg(low),
            })
        })
        .collect();
    weather::State {
        place: place.to_string(),
        temp: deg(temp),
        feels: format!("feels like {}", deg(temp - wind / 12.0)),
        description: description.to_string(),
        icon,
        details: format!(
            "{} / {} · {:.0}% humidity · wind {wind:.0} km/h",
            deg(high),
            deg(low),
            45.0 + 45.0 * noise(t, 2600.0, 36)
        ),
        days,
        error: String::new(),
    }
}

fn since_text(seconds: f64) -> String {
    match seconds as i64 {
        0..=59 => "just now".into(),
        s @ 60..=3599 => format!("{} min", s / 60),
        s => format!("{} h {} min", s / 3600, s % 3600 / 60),
    }
}

/// The Claude Code sessions, each working and waiting in a rhythm of its own.
pub fn claude_sessions(t: f64) -> claude::State {
    struct Spec {
        pid: i32,
        window: u64,
        place: &'static str,
        /// Work for this many seconds, then wait this many.
        work: f64,
        wait: f64,
        titles: &'static [&'static str],
    }
    const SPECS: [Spec; 3] = [
        Spec {
            pid: 4101,
            window: 22,
            place: "~/code/niri-dash",
            work: 310.0,
            wait: 95.0,
            titles: &[
                "Simulate the system",
                "Render to a canvas",
                "Deploy to GitHub Pages",
            ],
        },
        Spec {
            pid: 4102,
            window: 23,
            place: "~/code/niri-dash",
            work: 120.0,
            wait: 460.0,
            titles: &["Keyboard navigation", "Fix the hint labels"],
        },
        Spec {
            pid: 4188,
            window: 24,
            place: "~/code/kdl-parser",
            work: 730.0,
            wait: 210.0,
            titles: &["Review the parser", "Better error spans", "Fuzz the lexer"],
        },
    ];
    let sessions: Vec<claude::Session> = SPECS
        .iter()
        .map(|s| {
            let at = into(t, s.work + s.wait);
            let round = (t / (s.work + s.wait))
                .floor()
                .rem_euclid(s.titles.len() as f64) as usize;
            let (status, status_text) = if at < s.work {
                (
                    claude::Status::Busy,
                    format!("working · {}", since_text(at)),
                )
            } else {
                (
                    claude::Status::Idle,
                    format!("waiting for you · {}", since_text(at - s.work)),
                )
            };
            claude::Session {
                pid: s.pid,
                title: s.titles[round].into(),
                place: s.place.into(),
                status,
                status_text,
                window: Some(s.window),
            }
        })
        .collect();
    let busy = sessions
        .iter()
        .filter(|s| s.status == claude::Status::Busy)
        .count();
    claude::State { sessions, busy }
}

fn short_count(n: f64) -> String {
    match n as u64 {
        n @ 0..1_000 => n.to_string(),
        n @ 1_000..1_000_000 => format!("{:.1}k", n as f64 / 1e3),
        n => format!("{:.1}M", n as f64 / 1e6),
    }
}

/// Claude usage: tokens build up over a five-hour window, then it starts again.
pub fn claude_usage(t: f64) -> claude::Usage {
    const WINDOW: f64 = 5.0 * 3600.0;
    let round = (t / WINDOW).floor();
    let age = into(t, WINDOW);
    let progress = age / WINDOW;
    let rate = 0.6 + 0.8 * hash(41, round as u64);
    let tokens = 3.2e6 * rate * progress.powf(0.8);
    let hourly: Vec<f32> = (0..24)
        .map(|i| {
            let hour = (t / 3600.0).floor() as i64 - 23 + i;
            let of_day = local(hour as f64 * 3600.0).hour() as f64;
            let awake = if (8.0..23.0).contains(&of_day) {
                1.0
            } else {
                0.05
            };
            (awake * hash(42, hour as u64)) as f32
        })
        .collect();
    let opus = 0.55 + 0.3 * hash(43, round as u64);
    let left = (WINDOW - age) / 60.0;
    let today_tokens = 1.1e6 + 4.0e6 * local(t).hour() as f64 / 24.0;
    claude::Usage {
        plan: "Max 5×".into(),
        window_active: true,
        window_tokens: format!("{} tokens", short_count(tokens)),
        window_count: short_count(tokens),
        window_cached: format!("{} read from cache", short_count(tokens * 11.0)),
        window_messages: (tokens / 9_000.0) as u32,
        window_left: format!(
            "{} h {:02} min left",
            (left / 60.0) as i32,
            (left as i32) % 60
        ),
        window_progress: progress as f32,
        window_resets: format!("resets {}", local(t + WINDOW - age).format("%H:%M")),
        burn: format!("{} tokens/min", short_count(tokens / (age / 60.0).max(1.0))),
        today: format!(
            "Today {} tokens · {} messages",
            short_count(today_tokens),
            (today_tokens / 9_000.0) as u32
        ),
        week: format!(
            "7 days {} tokens",
            short_count(28e6 + 6e6 * wave(t, 604_800.0, 0.3))
        ),
        hourly,
        models: vec![
            ("Opus 5.5".into(), opus as f32),
            ("Sonnet 5.5".into(), (1.0 - opus) as f32),
        ],
    }
}

/// Who uses the microphone and camera: the call, and a voice memo now and then.
pub fn privacy_usage(t: f64) -> privacy::Usage {
    let call = scene(t) == Scene::Call;
    let mut mic = Vec::new();
    if call {
        mic.push("Firefox".to_string());
    }
    if into(t, 1570.0) < 40.0 {
        mic.push("Sound Recorder".to_string());
    }
    privacy::Usage {
        mic,
        camera: if call {
            vec!["Firefox".to_string()]
        } else {
            Vec::new()
        },
        video_readers: if sharing(t) {
            vec![(88, "Firefox".to_string())]
        } else {
            Vec::new()
        },
    }
}

/// The screen is shared for the second half of each call.
pub fn sharing(t: f64) -> bool {
    scene(t) == Scene::Call && scene_age(t) > 105.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Moments over several hours, a little more than a second apart.
    fn moments() -> impl Iterator<Item = f64> {
        (0..4000).map(|i| 1_800_000_000.0 + i as f64 * 7.3)
    }

    #[test]
    fn perf_figures_stay_in_range() {
        for t in moments() {
            let p = perf(t, true, "192.168.1.23");
            for v in [p.cpu, p.mem, p.swap, p.net_rx_frac, p.net_tx_frac] {
                assert!((0.0..=1.0).contains(&v), "{v} at {t}");
            }
            for h in [&p.cpu_history, &p.net_rx_history, &p.disk_history] {
                assert_eq!(h.len(), HISTORY);
                assert!(h.iter().all(|v| (0.0..=1.0).contains(v)));
            }
            assert_eq!(p.cores.len(), CORES);
            assert!(p.cores.iter().all(|v| (0.0..=1.0).contains(v)));
        }
    }

    #[test]
    fn being_offline_stops_the_traffic() {
        let p = perf(1_800_000_000.0, false, "");
        assert!(p.net_rx_history.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn a_build_shows_on_the_cpu() {
        let during = |wanted| {
            let values: Vec<f64> = moments()
                .filter(|t| scene(*t) == wanted && scene_age(*t) > 20.0)
                .map(cpu)
                .collect();
            values.iter().sum::<f64>() / values.len() as f64
        };
        assert!(during(Scene::Building) > 2.0 * during(Scene::Coding));
    }

    #[test]
    fn the_battery_goes_down_and_up_again() {
        let seen: Vec<(i32, bool)> = (0..7200)
            .map(|i| {
                let (percent, charging, status, power) = battery(i as f64);
                assert!(!status.is_empty() && !power.is_empty());
                (percent, charging)
            })
            .collect();
        assert!(seen.iter().all(|(p, _)| (20..=100).contains(p)));
        for w in seen.windows(2) {
            assert!((w[0].0 - w[1].0).abs() <= 1, "{:?}", w);
            if w[0].1 == w[1].1 {
                assert_eq!(w[0].0 <= w[1].0, w[1].1 || w[0].0 == w[1].0);
            }
        }
        assert!(seen.iter().any(|(_, charging)| *charging));
        assert!(seen.iter().any(|(_, charging)| !*charging));
    }

    #[test]
    fn the_weather_changes_and_its_forecast_holds() {
        let at = |t| weather(t, "Hamburg", false);
        let a = at(1_800_000_000.0);
        assert_eq!(a.days.len(), 5);
        assert_eq!(a.place, "Hamburg");
        let descriptions: std::collections::HashSet<String> = (0..20)
            .map(|i| at(1_800_000_000.0 + i as f64 * 1400.0).description)
            .collect();
        assert!(descriptions.len() > 2, "{descriptions:?}");
        assert!(weather(1_800_000_000.0, "Boston", true).temp.ends_with('°'));
    }

    #[test]
    fn claude_sessions_change_their_minds() {
        let busy: std::collections::HashSet<usize> =
            moments().map(|t| claude_sessions(t).busy).collect();
        assert!(busy.len() > 1);
        let usage = claude_usage(1_800_000_000.0);
        assert_eq!(usage.hourly.len(), 24);
        assert!((0.0..=1.0).contains(&usage.window_progress));
    }

    #[test]
    fn a_call_uses_the_microphone_and_camera() {
        let call = moments().find(|t| sharing(*t)).expect("a shared screen");
        let usage = privacy_usage(call);
        assert!(usage.mic.iter().any(|m| m == "Firefox"));
        assert!(usage.camera.iter().any(|c| c == "Firefox"));
        assert_eq!(usage.video_readers.len(), 1);
    }
}

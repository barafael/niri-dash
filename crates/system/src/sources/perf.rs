//! CPU, memory, network, disks and GPU, sampled once a second. History is kept even while hidden
//! so the graphs are full when the dashboard opens.

use super::{Ctx, sysfs};
pub use crate::perf::{DiskRow, Gpu, Snapshot, human_bytes};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use sysinfo::{
    Components, CpuRefreshKind, Disks, MemoryRefreshKind, Networks, ProcessRefreshKind,
    ProcessesToUpdate, RefreshKind, System,
};

const HISTORY: usize = 90;

fn cpu_temp(components: &Components) -> Option<f32> {
    let preferred = ["tctl", "tdie", "package id 0", "cpu"];
    for key in preferred {
        if let Some(c) = components
            .iter()
            .find(|c| c.label().to_lowercase().contains(key))
        {
            return c.temperature();
        }
    }
    components
        .iter()
        .filter_map(|c| c.temperature())
        .fold(None, |m: Option<f32>, t| Some(m.map_or(t, |m| m.max(t))))
}

/// A GPU readable through sysfs (amdgpu exposes busy %, VRAM and hwmon sensors).
struct GpuDev {
    dev: PathBuf,
    hwmon: Option<PathBuf>,
    name: String,
}

fn find_gpu() -> Option<GpuDev> {
    let mut cards: Vec<PathBuf> = std::fs::read_dir("/sys/class/drm")
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().is_some_and(|n| {
                n.to_string_lossy().starts_with("card") && !n.to_string_lossy().contains('-')
            })
        })
        .collect();
    cards.sort();
    let dev = cards
        .into_iter()
        .map(|c| c.join("device"))
        .find(|d| d.join("gpu_busy_percent").exists())?;
    let hwmon = std::fs::read_dir(dev.join("hwmon"))
        .ok()
        .and_then(|mut rd| rd.next())
        .and_then(|e| e.ok())
        .map(|e| e.path());
    // A readable name from lspci, e.g. "Phoenix1"; falls back to the driver.
    let slot = std::fs::canonicalize(&dev)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
    let name = slot
        .and_then(|s| {
            std::process::Command::new("lspci")
                .args(["-mm", "-s", &s])
                .output()
                .ok()
        })
        .and_then(|o| {
            let line = String::from_utf8_lossy(&o.stdout).into_owned();
            let fields: Vec<&str> = line.split('"').filter(|f| !f.trim().is_empty()).collect();
            fields
                .get(3)
                .map(|d| d.split('[').next().unwrap_or(d).trim().to_string())
        })
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "GPU".into());
    Some(GpuDev { dev, hwmon, name })
}

fn push(q: &mut VecDeque<f32>, v: f32) {
    if q.len() == HISTORY {
        q.pop_front();
    }
    q.push_back(v);
}

/// The peak of a rate history, at least `floor` bytes/s.
fn peak_of(q: &VecDeque<f32>, floor: f32) -> f32 {
    q.iter().copied().fold(floor, f32::max)
}

/// A rate history as fractions of `peak`.
fn scaled(q: &VecDeque<f32>, peak: f32) -> Vec<f32> {
    q.iter().map(|v| v / peak).collect()
}

const VIRTUAL_FS: [&str; 8] = [
    "tmpfs",
    "devtmpfs",
    "overlay",
    "squashfs",
    "ramfs",
    "efivarfs",
    "fuse.portal",
    "nsfs",
];

pub fn start(ctx: Ctx) {
    if ctx.dormant {
        return;
    }
    std::thread::Builder::new()
        .name("perf".into())
        .spawn(move || {
            let mut sys = System::new_with_specifics(
                RefreshKind::nothing()
                    .with_cpu(CpuRefreshKind::nothing().with_cpu_usage())
                    .with_memory(MemoryRefreshKind::everything()),
            );
            let mut nets = Networks::new_with_refreshed_list();
            let mut comps = Components::new_with_refreshed_list();
            let mut disks = Disks::new_with_refreshed_list();
            let gpu = find_gpu();
            let (mut cpu_hist, mut mem_hist) = (VecDeque::new(), VecDeque::new());
            let (mut rx_hist, mut tx_hist, mut io_hist, mut gpu_hist) = (
                VecDeque::new(),
                VecDeque::new(),
                VecDeque::new(),
                VecDeque::new(),
            );
            let mut top_mem: Vec<(String, String)> = Vec::new();
            let mut last = Instant::now();
            let mut tick = 0u64;
            let mut was_visible = false;
            std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
            loop {
                sys.refresh_cpu_usage();
                sys.refresh_memory();
                nets.refresh(true);
                disks.refresh(false);
                let dt = last.elapsed().as_secs_f64().max(0.1);
                last = Instant::now();
                let cpu = sys.global_cpu_usage() / 100.0;
                let mem = sys.used_memory() as f32 / sys.total_memory().max(1) as f32;
                push(&mut cpu_hist, cpu);
                push(&mut mem_hist, mem);
                let physical = |name: &str| {
                    !name.starts_with("lo")
                        && !name.starts_with("docker")
                        && !name.starts_with("veth")
                        && !name.starts_with("br-")
                };
                let (rx, tx) = nets
                    .iter()
                    .filter(|(name, _)| physical(name))
                    .fold((0u64, 0u64), |(r, t), (_, d)| {
                        (r + d.received(), t + d.transmitted())
                    });
                let (rx_rate, tx_rate) = (rx as f64 / dt, tx as f64 / dt);
                push(&mut rx_hist, rx_rate as f32);
                push(&mut tx_hist, tx_rate as f32);
                let real_disks: Vec<&sysinfo::Disk> = {
                    let mut seen = Vec::new();
                    let mut v: Vec<&sysinfo::Disk> = disks
                        .iter()
                        .filter(|d| {
                            !VIRTUAL_FS.contains(&d.file_system().to_string_lossy().as_ref())
                                && d.total_space() > 0
                        })
                        .collect();
                    v.sort_by_key(|d| d.mount_point().as_os_str().len());
                    v.retain(|d| {
                        let key = d.name().to_os_string();
                        if seen.contains(&key) {
                            false
                        } else {
                            seen.push(key);
                            true
                        }
                    });
                    v
                };
                let (read, written) = real_disks.iter().fold((0u64, 0u64), |(r, w), d| {
                    let u = d.usage();
                    (r + u.read_bytes, w + u.written_bytes)
                });
                push(&mut io_hist, ((read + written) as f64 / dt) as f32);
                let gpu_busy = gpu
                    .as_ref()
                    .and_then(|g| sysfs::<u64>(g.dev.join("gpu_busy_percent")))
                    .map(|b| b as f32 / 100.0);
                if let Some(b) = gpu_busy {
                    push(&mut gpu_hist, b);
                }
                // Snapshots are only sent while the dashboard is up; the first one right away when
                // it comes up, with fresh slow-changing data (sensors, the biggest apps).
                let visible = ctx.visible();
                let shown = visible && !was_visible;
                was_visible = visible;
                if visible {
                    if shown || tick.is_multiple_of(5) {
                        comps.refresh(false);
                        // The biggest apps, with their processes added up (browsers have dozens).
                        sys.refresh_processes_specifics(
                            ProcessesToUpdate::All,
                            true,
                            ProcessRefreshKind::nothing().with_memory(),
                        );
                        let mut by_name: HashMap<String, u64> = HashMap::new();
                        // sysinfo lists threads too; their memory is the process's.
                        for p in sys
                            .processes()
                            .values()
                            .filter(|p| p.thread_kind().is_none())
                        {
                            *by_name
                                .entry(p.name().to_string_lossy().into_owned())
                                .or_default() += p.memory();
                        }
                        let mut v: Vec<(String, u64)> = by_name.into_iter().collect();
                        v.sort_by_key(|(_, m)| std::cmp::Reverse(*m));
                        top_mem = v
                            .into_iter()
                            .take(5)
                            .map(|(n, m)| (n, human_bytes(m)))
                            .collect();
                    }
                    let load = System::load_average();
                    // Down and up share one scale: the higher of their peaks.
                    let net_floor = 64.0 * 1024.0;
                    let peak = peak_of(&rx_hist, net_floor).max(peak_of(&tx_hist, net_floor));
                    let io_scaled = scaled(&io_hist, peak_of(&io_hist, 1024.0 * 1024.0));
                    let iface = nets
                        .iter()
                        .filter(|(name, _)| physical(name))
                        .max_by_key(|(_, d)| d.total_received() + d.total_transmitted())
                        .map(|(n, d)| {
                            let addr = d
                                .ip_networks()
                                .iter()
                                .find(|ip| ip.addr.is_ipv4())
                                .or_else(|| d.ip_networks().first())
                                .map(|ip| ip.addr.to_string())
                                .unwrap_or_default();
                            (n.clone(), addr)
                        })
                        .unwrap_or_default();
                    let disk_rows: Vec<DiskRow> = real_disks
                        .iter()
                        .map(|d| {
                            let total = d.total_space();
                            let used = total.saturating_sub(d.available_space());
                            let mount = d.mount_point().to_string_lossy().into_owned();
                            let fs = d.file_system().to_string_lossy();
                            DiskRow {
                                system: ["/boot", "/efi", "/recovery"]
                                    .iter()
                                    .any(|p| mount == *p || mount.starts_with(&format!("{p}/")))
                                    || fs == "vfat",
                                mount,
                                used: used as f32 / total.max(1) as f32,
                                text: format!("{} / {}", human_bytes(used), human_bytes(total)),
                            }
                        })
                        .collect();
                    let root_used = disk_rows
                        .iter()
                        .find(|d| d.mount == "/")
                        .or(disk_rows.first())
                        .map(|d| d.used)
                        .unwrap_or(0.0);
                    let gpu_snap = gpu.as_ref().zip(gpu_busy).map(|(g, busy)| {
                        let vram_used = sysfs::<u64>(g.dev.join("mem_info_vram_used")).unwrap_or(0);
                        let vram_total =
                            sysfs::<u64>(g.dev.join("mem_info_vram_total")).unwrap_or(0);
                        let temp = g
                            .hwmon
                            .as_ref()
                            .and_then(|h| sysfs::<u64>(h.join("temp1_input")));
                        let power = g.hwmon.as_ref().and_then(|h| {
                            sysfs::<u64>(h.join("power1_average"))
                                .or_else(|| sysfs::<u64>(h.join("power1_input")))
                        });
                        Gpu {
                            name: g.name.clone(),
                            busy,
                            history: gpu_hist.iter().copied().collect(),
                            vram: vram_used as f32 / vram_total.max(1) as f32,
                            vram_text: format!(
                                "{} / {}",
                                human_bytes(vram_used),
                                human_bytes(vram_total)
                            ),
                            temp_text: temp
                                .map(|t| format!("{:.0} °C", t as f64 / 1000.0))
                                .unwrap_or_default(),
                            power_text: power
                                .map(|p| format!("{:.1} W", p as f64 / 1e6))
                                .unwrap_or_default(),
                        }
                    });
                    let snap = Snapshot {
                        cpu,
                        mem,
                        cpu_history: cpu_hist.iter().copied().collect(),
                        mem_history: mem_hist.iter().copied().collect(),
                        cpu_text: format!("{:.0}%  ·  {} cores", cpu * 100.0, sys.cpus().len()),
                        mem_text: format!(
                            "{} / {}{}",
                            human_bytes(sys.used_memory()),
                            human_bytes(sys.total_memory()),
                            if sys.used_swap() > 0 {
                                format!("  ·  swap {}", human_bytes(sys.used_swap()))
                            } else {
                                String::new()
                            }
                        ),
                        temp_text: cpu_temp(&comps)
                            .map(|t| format!("{t:.0} °C"))
                            .unwrap_or_default(),
                        net_text: format!(
                            "↓ {}/s  ↑ {}/s",
                            human_bytes(rx_rate as u64),
                            human_bytes(tx_rate as u64)
                        ),
                        load_text: format!(
                            "load {:.2} {:.2} {:.2}",
                            load.one, load.five, load.fifteen
                        ),
                        cores: sys.cpus().iter().map(|c| c.cpu_usage() / 100.0).collect(),
                        mem_used_text: format!(
                            "{} of {}",
                            human_bytes(sys.used_memory()),
                            human_bytes(sys.total_memory())
                        ),
                        mem_avail_text: format!(
                            "{} available",
                            human_bytes(sys.available_memory())
                        ),
                        swap: sys.used_swap() as f32 / sys.total_swap().max(1) as f32,
                        swap_text: format!(
                            "swap {} / {}",
                            human_bytes(sys.used_swap()),
                            human_bytes(sys.total_swap())
                        ),
                        top_mem: top_mem.clone(),
                        net_rx_history: scaled(&rx_hist, peak),
                        net_tx_history: scaled(&tx_hist, peak),
                        net_rx_text: format!("{}/s", human_bytes(rx_rate as u64)),
                        net_tx_text: format!("{}/s", human_bytes(tx_rate as u64)),
                        net_peak_text: format!("peak {}/s", human_bytes(peak as u64)),
                        net_iface: iface.0,
                        net_addr: iface.1,
                        net_rx_frac: (rx_rate as f32 / peak).min(1.0),
                        net_tx_frac: (tx_rate as f32 / peak).min(1.0),
                        disks: disk_rows,
                        disk_history: io_scaled,
                        disk_read_text: format!(
                            "read {}/s",
                            human_bytes((read as f64 / dt) as u64)
                        ),
                        disk_write_text: format!(
                            "write {}/s",
                            human_bytes((written as f64 / dt) as u64)
                        ),
                        root_used,
                        gpu: gpu_snap,
                    };
                    ctx.send(crate::perf::Event::Snapshot(Box::new(snap)));
                }
                tick += 1;
                std::thread::sleep(Duration::from_secs(1));
            }
        })
        .expect("spawning perf thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaling() {
        let q: VecDeque<f32> = [0.0, 50.0, 100.0].into_iter().collect();
        let peak = peak_of(&q, 10.0);
        assert_eq!(peak, 100.0);
        assert_eq!(scaled(&q, peak), vec![0.0, 0.5, 1.0]);
        assert_eq!(peak_of(&[1.0].into_iter().collect(), 10.0), 10.0);
    }

    #[test]
    fn bytes() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(3 * 1024 * 1024 * 1024 / 2), "1.5 GiB");
    }
}

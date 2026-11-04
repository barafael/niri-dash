//! CPU, memory, disks, network and GPU.

#[derive(Debug, Clone, Default)]
pub struct DiskRow {
    pub mount: String,
    /// Boot and firmware partitions, which some people don't care to see.
    pub system: bool,
    pub used: f32,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct Gpu {
    pub name: String,
    pub busy: f32,
    pub history: Vec<f32>,
    pub vram: f32,
    pub vram_text: String,
    pub temp_text: String,
    pub power_text: String,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub cpu: f32,
    pub mem: f32,
    pub cpu_history: Vec<f32>,
    pub mem_history: Vec<f32>,
    pub cpu_text: String,
    pub mem_text: String,
    pub temp_text: String,
    pub net_text: String,
    pub load_text: String,
    pub cores: Vec<f32>,
    // Memory widget.
    pub mem_used_text: String,
    pub mem_avail_text: String,
    pub swap: f32,
    pub swap_text: String,
    pub top_mem: Vec<(String, String)>,
    // Network widget: rates as a fraction of the recent peak, so both lines share a scale.
    pub net_rx_history: Vec<f32>,
    pub net_tx_history: Vec<f32>,
    pub net_rx_text: String,
    pub net_tx_text: String,
    pub net_peak_text: String,
    pub net_iface: String,
    pub net_addr: String,
    pub net_rx_frac: f32,
    pub net_tx_frac: f32,
    // Disks widget.
    pub disks: Vec<DiskRow>,
    pub disk_history: Vec<f32>,
    pub disk_read_text: String,
    pub disk_write_text: String,
    pub root_used: f32,
    // GPU widget (None when there is no readable GPU).
    pub gpu: Option<Gpu>,
}

pub fn human_bytes(b: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = b as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u <= 1 {
        format!("{v:.0} {}", UNITS[u])
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

#[derive(Debug)]
pub enum Event {
    Snapshot(Box<Snapshot>),
}

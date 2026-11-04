//! Night light: warmer screen colors through wlr-gamma-control, which niri implements (on Plasma,
//! the night-light actor drives KWin's own instead). The gamma tables stay in effect while we hold
//! the controls; dropping them (or the daemon exiting) restores the original colors.

use super::Services;
use crate::Event;
use smithay_client_toolkit::reexports::client::{
    Connection, Dispatch, Proxy, QueueHandle, globals::GlobalList, protocol::wl_output::WlOutput,
};
use std::io::{Seek, SeekFrom, Write};
use std::os::fd::AsFd;
use wayland_protocols_wlr::gamma_control::v1::client::{
    zwlr_gamma_control_manager_v1::ZwlrGammaControlManagerV1,
    zwlr_gamma_control_v1::{self, ZwlrGammaControlV1},
};

/// The temperature that leaves colors as they are.
pub const NEUTRAL: u32 = 6500;
/// The warmest setting offered.
pub const WARMEST: u32 = 2500;

/// What `niri-dash night-light` and a standalone night-light app ask the daemon for. On the
/// control socket it is `night-light <arg>`, `arg` as [`Cmd::parse`] reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    Toggle,
    On,
    Off,
    /// On at this temperature.
    Kelvin(u32),
}

impl Cmd {
    const PREFIX: &str = "night-light ";

    /// `toggle`, `on`, `off`, or a temperature like `4500` or `4500K`.
    pub fn parse(arg: &str) -> Option<Self> {
        let arg = arg.trim().to_ascii_lowercase();
        Some(match arg.as_str() {
            "toggle" => Cmd::Toggle,
            "on" => Cmd::On,
            "off" => Cmd::Off,
            k => Cmd::Kelvin(
                k.trim_end_matches('k')
                    .trim()
                    .parse::<u32>()
                    .ok()?
                    .clamp(1000, NEUTRAL),
            ),
        })
    }

    /// The control socket line.
    pub fn to_wire(self) -> String {
        let arg = match self {
            Cmd::Toggle => "toggle".to_string(),
            Cmd::On => "on".to_string(),
            Cmd::Off => "off".to_string(),
            Cmd::Kelvin(k) => k.to_string(),
        };
        format!("{}{arg}", Self::PREFIX)
    }

    /// The command in a control socket line, if it is one.
    pub fn parse_wire(line: &str) -> Option<Self> {
        Self::parse(line.strip_prefix(Self::PREFIX)?)
    }

    /// The new (on, kelvin) from the current ones.
    pub fn apply(self, on: bool, kelvin: u32) -> (bool, u32) {
        match self {
            Cmd::Toggle => (!on, kelvin),
            Cmd::On => (true, kelvin),
            Cmd::Off => (false, kelvin),
            Cmd::Kelvin(k) => (true, k),
        }
    }
}

struct Output {
    output: WlOutput,
    control: Option<ZwlrGammaControlV1>,
    /// Entries per channel, once the compositor has told us.
    size: Option<u32>,
    /// Another program holds this output's gamma.
    failed: bool,
}

#[derive(Default)]
pub struct Gamma {
    manager: Option<ZwlrGammaControlManagerV1>,
    outputs: Vec<Output>,
    /// The temperature wanted; `None` is off.
    want: Option<u32>,
}

impl Gamma {
    /// `kwin`: night light is KWin's own here.
    pub fn bind(globals: &GlobalList, qh: &QueueHandle<Services>, kwin: bool) -> Self {
        let manager = globals
            .bind::<ZwlrGammaControlManagerV1, _, _>(qh, 1..=1, ())
            .ok();
        if manager.is_none() {
            if kwin {
                log::info!("no wlr-gamma-control; night light goes through KWin's own");
            } else {
                log::info!("no wlr-gamma-control; night light is unavailable");
            }
        }
        Self {
            manager,
            ..Default::default()
        }
    }

    pub fn available(&self) -> bool {
        self.manager.is_some()
    }

    pub fn add_output(&mut self, output: WlOutput, qh: &QueueHandle<Services>) {
        self.outputs.push(Output {
            output,
            control: None,
            size: None,
            failed: false,
        });
        self.sync(qh);
    }

    pub fn remove_output(&mut self, output: &WlOutput) {
        self.outputs.retain(|o| {
            if o.output == *output {
                if let Some(c) = &o.control {
                    c.destroy();
                }
                false
            } else {
                true
            }
        });
    }

    /// Turn night light on at `kelvin`, or off with `None`.
    pub fn set(&mut self, want: Option<u32>, qh: &QueueHandle<Services>) {
        let want = want.filter(|&k| k < NEUTRAL);
        if want == self.want {
            return;
        }
        self.want = want;
        if want.is_none() {
            for o in &mut self.outputs {
                if let Some(c) = o.control.take() {
                    c.destroy();
                }
                o.size = None;
                // Try again next time; the other program may be gone.
                o.failed = false;
            }
            return;
        }
        self.sync(qh);
    }

    /// Get controls for outputs that lack one and apply the current temperature.
    fn sync(&mut self, qh: &QueueHandle<Services>) {
        let (Some(manager), Some(kelvin)) = (&self.manager, self.want) else {
            return;
        };
        for o in &mut self.outputs {
            if o.failed {
                continue;
            }
            match (&o.control, o.size) {
                (None, _) => {
                    o.control = Some(manager.get_gamma_control(&o.output, qh, o.output.clone()));
                }
                (Some(c), Some(size)) => apply(c, size, kelvin),
                (Some(_), None) => {} // Applied when the size arrives.
            }
        }
    }

    fn event(&mut self, output: &WlOutput, event: zwlr_gamma_control_v1::Event) -> Option<String> {
        let o = self.outputs.iter_mut().find(|o| o.output == *output)?;
        match event {
            zwlr_gamma_control_v1::Event::GammaSize { size } => {
                o.size = Some(size);
                if let (Some(c), Some(k)) = (&o.control, self.want) {
                    apply(c, size, k);
                }
                None
            }
            zwlr_gamma_control_v1::Event::Failed => {
                if let Some(c) = o.control.take() {
                    c.destroy();
                }
                o.failed = true;
                Some(
                    "Night light: another program controls the screen colors (wlsunset, gammastep?)"
                        .into(),
                )
            }
            _ => None,
        }
    }
}

/// Write the ramps for `kelvin` and hand them to the compositor.
fn apply(control: &ZwlrGammaControlV1, size: u32, kelvin: u32) {
    let table = ramps(size as usize, whitepoint(kelvin));
    match ramp_file(&table) {
        Ok(file) => control.set_gamma(file.as_fd()),
        Err(e) => log::warn!("night light: {e}"),
    }
}

/// An unlinked file in the runtime dir holding the table, positioned at its start.
fn ramp_file(table: &[u16]) -> std::io::Result<std::fs::File> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let path = dir.join(format!("niri-dash-gamma-{}", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)?;
    let _ = std::fs::remove_file(&path);
    let bytes: Vec<u8> = table.iter().flat_map(|v| v.to_ne_bytes()).collect();
    file.write_all(&bytes)?;
    file.seek(SeekFrom::Start(0))?;
    Ok(file)
}

/// Red, green and blue ramps, one after the other, scaled by the white point.
pub fn ramps(size: usize, [r, g, b]: [f64; 3]) -> Vec<u16> {
    let mut out = Vec::with_capacity(size * 3);
    for m in [r, g, b] {
        for i in 0..size {
            let x = if size > 1 {
                i as f64 / (size - 1) as f64
            } else {
                1.0
            };
            out.push(
                (x * m * u16::MAX as f64)
                    .round()
                    .clamp(0.0, u16::MAX as f64) as u16,
            );
        }
    }
    out
}

/// The RGB multipliers that make white look like light of `kelvin` (as wlsunset computes them).
pub fn whitepoint(kelvin: u32) -> [f64; 3] {
    if kelvin >= NEUTRAL {
        return [1.0, 1.0, 1.0];
    }
    let t = kelvin as f64;
    let (x, y) = if t >= 4000.0 {
        illuminant_d(t)
    } else if t >= 2500.0 {
        let (x1, y1) = illuminant_d(t);
        let (x2, y2) = planckian_locus(t);
        let factor = (4000.0 - t) / 1500.0;
        let s = ((std::f64::consts::PI * factor).cos() + 1.0) / 2.0;
        (x1 * s + x2 * (1.0 - s), y1 * s + y2 * (1.0 - s))
    } else {
        planckian_locus(t.max(1667.0))
    };
    let z = 1.0 - x - y;
    let c = |v: f64| v.clamp(0.0, 1.0);
    let gamma = |v: f64| {
        if v <= 0.0031308 {
            12.92 * v
        } else {
            (1.055 * v).powf(1.0 / 2.2) - 0.055
        }
    };
    let rgb = [
        gamma(c(3.2404542 * x - 1.5371385 * y - 0.4985314 * z)),
        gamma(c(-0.9692660 * x + 1.8760108 * y + 0.0415560 * z)),
        gamma(c(0.0556434 * x - 0.2040259 * y + 1.0572252 * z)),
    ];
    let max = rgb[0].max(rgb[1]).max(rgb[2]);
    rgb.map(|v| v / max)
}

fn illuminant_d(t: f64) -> (f64, f64) {
    let x = if t <= 7000.0 {
        0.244063 + 0.09911e3 / t + 2.9678e6 / t.powi(2) - 4.6070e9 / t.powi(3)
    } else {
        0.237040 + 0.24748e3 / t + 1.9018e6 / t.powi(2) - 2.0064e9 / t.powi(3)
    };
    (x, -3.0 * x * x + 2.870 * x - 0.275)
}

fn planckian_locus(t: f64) -> (f64, f64) {
    if t <= 4000.0 {
        let x = -0.2661239e9 / t.powi(3) - 0.2343589e6 / t.powi(2) + 0.8776956e3 / t + 0.179910;
        let y = if t <= 2222.0 {
            -1.1064814 * x.powi(3) - 1.34811020 * x * x + 2.18555832 * x - 0.20219683
        } else {
            -0.9549476 * x.powi(3) - 1.37418593 * x * x + 2.09137015 * x - 0.16748867
        };
        (x, y)
    } else {
        let x = -3.0258469e9 / t.powi(3) + 2.1070379e6 / t.powi(2) + 0.2226347e3 / t + 0.240390;
        (
            x,
            3.0817580 * x.powi(3) - 5.87338670 * x * x + 3.75112997 * x - 0.37001483,
        )
    }
}

impl Dispatch<ZwlrGammaControlManagerV1, ()> for Services {
    fn event(
        _: &mut Self,
        _: &ZwlrGammaControlManagerV1,
        _: <ZwlrGammaControlManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrGammaControlV1, WlOutput> for Services {
    fn event(
        me: &mut Self,
        _: &ZwlrGammaControlV1,
        event: zwlr_gamma_control_v1::Event,
        output: &WlOutput,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let Some(problem) = me.gamma.event(output, event) {
            log::warn!("{problem}");
            let _ = me.events.send(Event::Toast(problem));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitepoints() {
        assert_eq!(whitepoint(6500), [1.0, 1.0, 1.0]);
        // Warmer means less blue, and red stays at full.
        let [r, g, b] = whitepoint(4500);
        assert!((r - 1.0).abs() < 1e-9 && g < 1.0 && b < g, "{r} {g} {b}");
        let [_, g2, b2] = whitepoint(3000);
        assert!(g2 < g && b2 < b);
        // Continuous around the 4000 K seam.
        let [_, ga, ba] = whitepoint(3999);
        let [_, gb, bb] = whitepoint(4000);
        assert!((ga - gb).abs() < 0.01 && (ba - bb).abs() < 0.01);
    }

    #[test]
    fn commands() {
        assert_eq!(Cmd::parse("4500K"), Some(Cmd::Kelvin(4500)));
        assert_eq!(Cmd::parse(" Off "), Some(Cmd::Off));
        assert_eq!(Cmd::parse("warm"), None);
        assert_eq!(
            Cmd::parse_wire(&Cmd::Kelvin(3400).to_wire()),
            Some(Cmd::Kelvin(3400))
        );
        assert_eq!(Cmd::parse_wire("toggle"), None);
        assert_eq!(Cmd::Toggle.apply(true, 4000), (false, 4000));
        assert_eq!(Cmd::Kelvin(3000).apply(false, 4000), (true, 3000));
    }

    #[test]
    fn ramp_layout() {
        let t = ramps(3, [1.0, 0.5, 0.0]);
        assert_eq!(t, [0, 32768, 65535, 0, 16384, 32768, 0, 0, 0]);
    }
}

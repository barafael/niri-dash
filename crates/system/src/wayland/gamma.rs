//! Night light: warmer screen colors through wlr-gamma-control, which niri implements (on Plasma,
//! the night-light actor drives KWin's own instead). The gamma tables stay in effect while we hold
//! the controls; dropping them (or the daemon exiting) restores the original colors.

use super::Services;
use crate::Event;
pub use crate::night_light::{NEUTRAL, whitepoint};
use smithay_client_toolkit::reexports::client::{
    Connection, Dispatch, Proxy, QueueHandle, globals::GlobalList, protocol::wl_output::WlOutput,
};
use std::io::{Seek, SeekFrom, Write};
use std::os::fd::AsFd;
use wayland_protocols_wlr::gamma_control::v1::client::{
    zwlr_gamma_control_manager_v1::ZwlrGammaControlManagerV1,
    zwlr_gamma_control_v1::{self, ZwlrGammaControlV1},
};

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
    use crate::night_light::Cmd;

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

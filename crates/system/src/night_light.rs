//! Night light: our own gamma tables (wlr-gamma-control), or on Plasma, KWin's own night light.

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

/// KWin's night light as the widget shows it. Its schedule and temperatures are set in System
/// Settings; the dashboard can only pause it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NightLightState {
    /// KWin can change the screen colors at all.
    pub available: bool,
    /// Turned on in System Settings.
    pub enabled: bool,
    /// Paused by someone (us included).
    pub inhibited: bool,
    /// We hold a pause.
    pub paused_by_us: bool,
    /// The screen's temperature right now, in kelvin.
    pub temperature: u32,
}

impl NightLightState {
    /// Whether night light follows its schedule (it may still be daytime).
    pub fn on(self) -> bool {
        self.available && self.enabled && !self.inhibited
    }
}

#[derive(Debug)]
pub enum Command {
    /// Warm the screen to this temperature through the gamma tables; None turns it off.
    Gamma(Option<u32>),
    /// Apply `cmd` to KWin's night light; `announce` shows the result on screen.
    Kwin { cmd: Cmd, announce: bool },
}

#[derive(Debug)]
pub enum Event {
    Kwin(NightLightState),
}

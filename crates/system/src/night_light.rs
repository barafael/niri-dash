//! Night light: warmer screen colors. Where the desktop has no night light of its own, the system
//! warms the screen itself (through wlr-gamma-control on Wayland); where it has one (KWin's), the
//! system shows that one and pauses it. The dashboard sends the same commands and gets the same
//! state either way.

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

/// Night light as the widget shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct State {
    /// The screen's colors can be changed here.
    pub available: bool,
    /// Warming the screen, or with a schedule, following it (it may be daytime).
    pub on: bool,
    /// The temperature it is set to, in kelvin; with a schedule, the screen's right now.
    pub kelvin: u32,
    /// The desktop's own night light, on the schedule and warmth set in its settings: the
    /// dashboard can pause and resume it but not set its warmth. None where the system's own.
    pub schedule: Option<Schedule>,
}

/// A desktop's own night light.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Schedule {
    /// Turned on in the desktop's settings.
    pub enabled: bool,
    /// Paused, by us or another program.
    pub paused: bool,
    /// The pause is ours.
    pub paused_by_us: bool,
}

#[derive(Debug)]
pub enum Command {
    /// Do `cmd`. `announce` shows the result on screen (a keybinding has no other feedback);
    /// what gets in the way shows as a toast either way.
    Apply { cmd: Cmd, announce: bool },
    /// How night light was set when the dashboard last ran, to bring back where the setting is
    /// the system's (a desktop's own keeps its own).
    Restore { on: bool, kelvin: u32 },
}

#[derive(Debug)]
pub enum Event {
    State(State),
}

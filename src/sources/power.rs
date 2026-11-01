//! Battery from sysfs, power profiles through `powerprofilesctl`.

use super::{Ctx, Msg, Trigger, run, signals_from};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A power-profiles-daemon profile; the kebab-case name is what powerprofilesctl uses.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::EnumString, strum::AsRefStr,
)]
#[strum(serialize_all = "kebab-case")]
pub enum Profile {
    PowerSaver,
    Balanced,
    Performance,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub has_battery: bool,
    pub percent: i32,
    pub charging: bool,
    pub plugged: bool,
    pub status_text: String,
    pub power_text: String,
    pub profile: Option<Profile>,
    pub profiles: Vec<Profile>,
    /// Batteries of mice, keyboards, headphones and the like.
    pub peripherals: Vec<Peripheral>,
}

/// What kind of device a peripheral battery belongs to (UPower's device type, simplified).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Mouse,
    Keyboard,
    Headphones,
    Gamepad,
    Phone,
    Other,
}

impl DeviceKind {
    /// From UPower's `Type` property.
    pub fn from_upower(t: u32) -> DeviceKind {
        match t {
            5 | 14 => DeviceKind::Mouse,
            6 => DeviceKind::Keyboard,
            17..=19 | 21 => DeviceKind::Headphones,
            12 => DeviceKind::Gamepad,
            8 => DeviceKind::Phone,
            _ => DeviceKind::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Peripheral {
    pub kind: DeviceKind,
    pub name: String,
    pub percent: i32,
}

/// Device batteries from UPower: everything that isn't a power supply of this machine.
async fn peripherals(ctx: &Ctx) -> zbus::Result<Vec<Peripheral>> {
    use std::collections::HashMap;
    use zbus::zvariant::{OwnedObjectPath, OwnedValue};
    let conn = ctx.bus(true).await?;
    let paths: Vec<OwnedObjectPath> = conn
        .call_method(
            Some("org.freedesktop.UPower"),
            "/org/freedesktop/UPower",
            Some("org.freedesktop.UPower"),
            "EnumerateDevices",
            &(),
        )
        .await?
        .body()
        .deserialize()?;
    let mut out = Vec::new();
    for path in paths {
        let props: HashMap<String, OwnedValue> = conn
            .call_method(
                Some("org.freedesktop.UPower"),
                &path,
                Some("org.freedesktop.DBus.Properties"),
                "GetAll",
                &("org.freedesktop.UPower.Device",),
            )
            .await?
            .body()
            .deserialize()?;
        let get_bool = |k: &str| props.get(k).and_then(|v| v.downcast_ref::<bool>().ok());
        let ty = props
            .get("Type")
            .and_then(|v| v.downcast_ref::<u32>().ok())
            .unwrap_or(0);
        // 1 line power, 2 battery, 3 UPS: this machine's own supplies.
        if get_bool("PowerSupply") == Some(true) || matches!(ty, 1..=3) {
            continue;
        }
        if get_bool("IsPresent") == Some(false) {
            continue;
        }
        let percent = props
            .get("Percentage")
            .and_then(|v| v.downcast_ref::<f64>().ok())
            .unwrap_or(0.0);
        if percent <= 0.0 {
            continue;
        }
        let name = props
            .get("Model")
            .and_then(|v| v.downcast_ref::<&str>().ok())
            .map(str::to_string)
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| "Device".into());
        out.push(Peripheral {
            kind: DeviceKind::from_upower(ty),
            name,
            percent: percent.round() as i32,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn read(p: &Path, f: &str) -> Option<String> {
    std::fs::read_to_string(p.join(f))
        .ok()
        .map(|s| s.trim().to_string())
}

fn read_f(p: &Path, f: &str) -> Option<f64> {
    read(p, f)?.parse().ok()
}

fn supplies() -> (Option<PathBuf>, Option<PathBuf>) {
    let mut bat = None;
    let mut ac = None;
    if let Ok(rd) = std::fs::read_dir("/sys/class/power_supply") {
        for e in rd.flatten() {
            let p = e.path();
            match read(&p, "type").as_deref() {
                Some("Battery")
                    if bat.is_none() && read(&p, "scope").as_deref() != Some("Device") =>
                {
                    bat = Some(p)
                }
                Some("Mains") if ac.is_none() => ac = Some(p),
                _ => {}
            }
        }
    }
    (bat, ac)
}

fn fmt_duration(hours: f64) -> String {
    let mins = (hours * 60.0).round() as i64;
    if mins >= 60 {
        format!("{}h {:02}min", mins / 60, mins % 60)
    } else {
        format!("{mins} min")
    }
}

fn battery(state: &mut State) {
    let (bat, ac) = supplies();
    state.plugged = ac.as_deref().and_then(|a| read(a, "online")).as_deref() == Some("1");
    let Some(b) = bat else {
        state.status_text = if state.plugged {
            "On AC power".into()
        } else {
            String::new()
        };
        return;
    };
    state.has_battery = true;
    state.percent = read_f(&b, "capacity").unwrap_or(0.0) as i32;
    let status = read(&b, "status").unwrap_or_default();
    state.charging = status == "Charging";
    // Energy in µWh / power in µW, or charge in µAh / current in µA (times voltage).
    let volts = read_f(&b, "voltage_now").map(|v| v / 1e6);
    let (now, full, rate) = match (
        read_f(&b, "energy_now"),
        read_f(&b, "energy_full"),
        read_f(&b, "power_now"),
    ) {
        (Some(n), Some(f), r) => (Some(n / 1e6), Some(f / 1e6), r.map(|r| r / 1e6)),
        _ => {
            let v = volts.unwrap_or(0.0);
            (
                read_f(&b, "charge_now").map(|c| c / 1e6 * v),
                read_f(&b, "charge_full").map(|c| c / 1e6 * v),
                read_f(&b, "current_now").map(|c| c / 1e6 * v),
            )
        }
    };
    let rate = rate.filter(|r| *r > 0.05);
    state.power_text = rate.map(|r| format!("{r:.1} W")).unwrap_or_default();
    state.status_text = match (status.as_str(), now, full, rate) {
        ("Charging", Some(n), Some(f), Some(r)) => {
            format!("Charging · full in {}", fmt_duration((f - n).max(0.0) / r))
        }
        ("Charging", ..) => "Charging".into(),
        ("Discharging", Some(n), _, Some(r)) => format!("{} left", fmt_duration(n / r)),
        ("Discharging", ..) => "On battery".into(),
        ("Full", ..) => "Fully charged".into(),
        ("Not charging", ..) => {
            if state.plugged {
                "Plugged in, not charging".into()
            } else {
                "Not charging".into()
            }
        }
        (s, ..) => s.to_string(),
    };
}

/// Parse `powerprofilesctl list`: the profiles (low to high) and the active one.
pub fn parse_profiles(list: &str) -> (Vec<Profile>, Option<Profile>) {
    use std::str::FromStr;
    let mut profiles = Vec::new();
    let mut active = None;
    for line in list.lines() {
        // Profile headers are "* performance:" or "  balanced:"; detail lines carry a value.
        let t = line.trim();
        let Some(name) = t.trim_start_matches("* ").strip_suffix(':') else {
            continue;
        };
        let Ok(p) = Profile::from_str(name) else {
            continue;
        };
        if t.starts_with('*') {
            active = Some(p);
        }
        profiles.push(p);
    }
    profiles.sort();
    (profiles, active)
}

pub fn start(ctx: Ctx) {
    let c = ctx.clone();
    // Plugging in, unplugging and charge changes arrive as kernel power_supply events; profile
    // switches as D-Bus signals from power-profiles-daemon.
    let trigger = Trigger::default();
    ctx.on_command_lines(
        "udevadm",
        &["monitor", "--kernel", "--subsystem-match=power_supply"],
        |l| l.starts_with("KERNEL["),
        trigger.clone(),
    );
    ctx.on_dbus_signals(
        true,
        signals_from("net.hadess.PowerProfiles"),
        Duration::from_millis(100),
        trigger.clone(),
    );
    // Devices coming and going, and their charge.
    ctx.on_dbus_signals(
        true,
        signals_from("org.freedesktop.UPower"),
        Duration::from_millis(300),
        trigger.clone(),
    );
    let sent = std::sync::Arc::new(super::Dedup::default());
    ctx.poll_events(trigger, Duration::from_secs(60), move || {
        let (c, sent) = (c.clone(), sent.clone());
        async move {
            let mut s = State::default();
            battery(&mut s);
            if let Ok(list) = run("powerprofilesctl", &["list"]).await {
                (s.profiles, s.profile) = parse_profiles(&list);
            }
            match peripherals(&c).await {
                Ok(p) => s.peripherals = p,
                Err(e) => log::debug!("upower: {e}"),
            }
            if sent.changed(&s) {
                c.send(Msg::Power(s));
            }
        }
    });
}

pub fn set_profile(ctx: &Ctx, profile: Profile) {
    ctx.act("Power profile", async move {
        run("powerprofilesctl", &["set", profile.as_ref()])
            .await
            .map(|_| ())
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles() {
        let out = "* performance:\n    CpuDriver:\tamd_pstate\n    Degraded:   no\n\n  balanced:\n    CpuDriver:\tamd_pstate\n\n  power-saver:\n    CpuDriver:\tamd_pstate\n";
        let (p, a) = parse_profiles(out);
        assert_eq!(
            p,
            vec![Profile::PowerSaver, Profile::Balanced, Profile::Performance]
        );
        assert_eq!(a, Some(Profile::Performance));
    }
}

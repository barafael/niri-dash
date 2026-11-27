//! Battery and power, volume, brightness, night light, session, keyboard layout, color picker.

use super::{Ctx, empty, line, meter, ring};
use crate::dash::Dash;
use crate::widget_settings::Opt;
use blenderui::prelude::*;
use niri_dash_system::{self as sys, audio, night_light, power};
use std::sync::Arc;
use web_time::Instant;

pub fn battery(c: &mut Ctx) -> Div {
    let Some(p) = c.d.data.power.clone() else {
        return empty("Asking UPower…");
    };
    let inhibited = c.d.data.inhibited;
    let tone = if p.percent <= 15 && !p.charging {
        "bad"
    } else if p.charging {
        "good"
    } else {
        ""
    };
    c.bind(
        "i",
        if inhibited {
            "let it sleep"
        } else {
            "stay awake"
        },
        |d, _| d.tell(power::Command::ToggleInhibit),
    );
    if c.round {
        let col =
            c.d.look
                .color(if tone.is_empty() { "accent" } else { tone })
                .to_string();
        let text = if p.has_battery {
            format!("{}%", p.percent)
        } else {
            "AC".into()
        };
        let el = ring(
            p.percent as f32 / 100.0,
            &col,
            c.d.look.color("track"),
            c.d.look.color("card"),
            div().class("title").child(text),
        );
        return c.press(el, "stay awake", |d, _| {
            d.tell(power::Command::ToggleInhibit)
        });
    }
    let mut el = div().class("col grow");
    if p.has_battery {
        el = el
            .child(
                div()
                    .class("row")
                    .child(c.icon_l(
                        if p.charging { "bolt" } else { "battery" },
                        if tone.is_empty() { "text" } else { tone },
                    ))
                    .child(
                        div()
                            .class(format!("display s {tone}"))
                            .child(format!("{}%", p.percent)),
                    )
                    .child(
                        div()
                            .class("col grow small dim")
                            .child(line(p.status_text.clone()))
                            .child(line(p.power_text.clone())),
                    ),
            )
            .child(meter(p.percent as f32 / 100.0, tone));
    } else {
        el = el.child(
            div()
                .class("row")
                .child(c.icon("bolt", "dim"))
                .child(div().class("dim").child("On mains power")),
        );
    }
    if !p.profiles.is_empty() && c.h > 90.0 {
        let mut row = div().class("row");
        for prof in p.profiles.clone() {
            let label = match prof {
                power::Profile::PowerSaver => "Saver",
                power::Profile::Balanced => "Balanced",
                power::Profile::Performance => "Performance",
            };
            let on = p.profile == Some(prof);
            let b = c.button(label, "set the profile", move |d, _| {
                d.tell(power::Command::SetProfile(prof))
            });
            row = row.child(if on {
                b.class("on")
                    .style("background: var(--accent); color: var(--on-accent)")
            } else {
                b
            });
        }
        el = el.child(row);
    }
    el = el.child(
        c.press(
            label()
                .class("row")
                .child(Checkbox().checked(inhibited))
                .child("Keep the machine awake"),
            "stay awake",
            |d, _| d.tell(power::Command::ToggleInhibit),
        ),
    );
    if c.h > 150.0 {
        for per in &p.peripherals {
            let icon = match per.kind {
                power::DeviceKind::Mouse => "mouse",
                power::DeviceKind::Keyboard => "keyboard",
                power::DeviceKind::Headphones => "headphones",
                power::DeviceKind::Gamepad => "gamepad",
                power::DeviceKind::Phone => "phone",
                power::DeviceKind::Other => "battery",
            };
            el = el.child(
                c.read(
                    div()
                        .class("item small")
                        .child(c.icon(icon, "dim"))
                        .child(div().class("grow ellipsis").child(per.name.clone()))
                        .child(div().child(format!("{}%", per.percent))),
                ),
            );
        }
        for b in c.d.data.blockers.clone() {
            el = el.child(
                c.read(
                    div().class("item small dim").child(
                        div()
                            .class("grow ellipsis")
                            .child(format!("{} blocks sleep: {}", b.who, b.why)),
                    ),
                ),
            );
        }
    }
    el
}

pub fn volume(c: &mut Ctx) -> Div {
    let Some(a) = c.d.data.audio.clone() else {
        return empty("Waiting for PipeWire…");
    };
    c.bind("m", if a.muted { "unmute" } else { "mute" }, |d, _| {
        d.tell(audio::Command::ToggleMute)
    });
    if c.round {
        let col =
            c.d.look
                .color(if a.muted { "faint" } else { "accent" })
                .to_string();
        let v = c.d.held("volume", a.volume);
        let el = ring(
            v,
            &col,
            c.d.look.color("track"),
            c.d.look.color("card"),
            div()
                .class("col center")
                .child(c.icon(if a.muted { "volume-mute" } else { "volume" }, "text"))
                .child(div().class("small").child(format!("{:.0}%", v * 100.0))),
        );
        return c.knob(
            "Volume",
            el,
            a.volume,
            |d, v, _| d.tell(audio::Command::SetVolume(v)),
            "mute",
            |d, _| d.tell(audio::Command::ToggleMute),
        );
    }
    let mute = c.press(
        c.icon(
            if a.muted { "volume-mute" } else { "volume" },
            if a.muted { "bad" } else { "text" },
        ),
        "mute",
        |d, _| d.tell(audio::Command::ToggleMute),
    );
    let master = c.slider("Volume", "volume", a.volume, |d, v, _| {
        d.tell(audio::Command::SetVolume(v))
    });
    let mut el = div()
        .class("col grow")
        .child(div().class("row").child(mute).child(master));
    if let Some((mv, mm)) = a.mic {
        let mic = c.press(
            c.icon(
                if mm { "mic-off" } else { "mic" },
                if mm { "bad" } else { "text" },
            ),
            "mute the microphone",
            |d, _| d.tell(audio::Command::ToggleMicMute),
        );
        let s = c.slider("Microphone", "mic", mv, |d, v, _| {
            d.tell(audio::Command::SetMicVolume(v))
        });
        el = el.child(div().class("row").child(mic).child(s));
    }
    if c.h > 90.0 && a.sinks.len() > 1 {
        let hd = c.hd;
        let sinks = a.sinks.clone();
        let ids: Vec<u32> = sinks.iter().map(|s| s.id).collect();
        el = el.child(
            Dropdown()
                .children(
                    sinks
                        .iter()
                        .map(|s| option().selected(s.default).child(s.name.clone())),
                )
                .on_change(move |e: &ChangeEvent, _: &mut Window, app: &mut App| {
                    if let Some(id) = ids.get(e.number as usize).copied() {
                        hd.update(app, |d, _| d.tell(audio::Command::SetDefaultSink(id)));
                    }
                }),
        );
    } else {
        el = el.child(line(a.sink_name.clone()).class("small dim"));
    }
    if c.h > 130.0 {
        for s in a.streams.clone() {
            let id = s.id;
            let icon = match super::super::icons::image(&s.icon) {
                Some(i) => img().class("icon").image(i),
                None => c.icon("volume", "dim"),
            };
            let mute = c.press(icon, "mute it", move |d, _| {
                d.tell(audio::Command::ToggleStreamMute(id))
            });
            let key = format!("stream-{id}");
            let slider = c.slider(&s.app, key, s.volume, move |d, v, _| {
                d.tell(audio::Command::SetStreamVolume(id, v))
            });
            el = el.child(
                div()
                    .class("row small")
                    .child(mute)
                    .child(
                        div()
                            .class("ellipsis")
                            .style("width: 30%")
                            .child(s.app.clone()),
                    )
                    .child(slider),
            );
        }
    }
    el
}

pub fn brightness(c: &mut Ctx) -> Div {
    let Some(level) = c.d.data.brightness else {
        return empty("No backlight to set");
    };
    if c.round {
        let col = c.d.look.color("warn").to_string();
        let v = c.d.held("brightness", level);
        let el = ring(
            v,
            &col,
            c.d.look.color("track"),
            c.d.look.color("card"),
            div()
                .class("col center")
                .child(c.icon("sun", "warn"))
                .child(div().class("small").child(format!("{:.0}%", v * 100.0))),
        );
        return c.knob(
            "Brightness",
            el,
            level,
            |d, v, _| d.tell(sys::brightness::Command::Set(v.max(0.01))),
            "",
            |_, _| {},
        );
    }
    let s = c.slider("Brightness", "brightness", level, |d, v, _| {
        d.tell(sys::brightness::Command::Set(v.max(0.01)))
    });
    div().class("col grow center").child(
        div()
            .class("row wide")
            .child(c.icon("sun", "warn"))
            .child(s),
    )
}

pub fn night_light(c: &mut Ctx) -> Div {
    let Some(n) = c.d.data.night_light else {
        return empty("Night light…");
    };
    if !n.available {
        return empty(format!("Night light {}", c.d.caps.unavailable()));
    }
    let apply = |cmd: night_light::Cmd| {
        move |d: &mut Dash, _: &mut Context<Dash>| {
            d.tell(night_light::Command::Apply {
                cmd,
                announce: false,
            });
            let (on, k) = d
                .data
                .night_light
                .as_ref()
                .map_or((false, 4500), |n| cmd.apply(n.on, n.kelvin));
            d.saved.night_light = crate::state::NightLight { on, kelvin: k };
            d.save_state();
        }
    };
    c.bind(
        "t",
        if n.on { "turn off" } else { "turn on" },
        apply(night_light::Cmd::Toggle),
    );
    let icon = c.icon_l(
        if n.on { "moon" } else { "sun" },
        if n.on { "warn" } else { "dim" },
    );
    if c.round {
        return c.press(
            div()
                .class("col center")
                .child(icon)
                .child(div().class("small").child(format!("{} K", n.kelvin))),
            "turn on or off",
            apply(night_light::Cmd::Toggle),
        );
    }
    let span_k = (night_light::NEUTRAL - night_light::WARMEST) as f32;
    let warmth = (night_light::NEUTRAL - n.kelvin.clamp(night_light::WARMEST, night_light::NEUTRAL))
        as f32
        / span_k;
    let toggle = c.press(
        label()
            .class("row")
            .child(Checkbox().checked(n.on))
            .child(if n.on { "On" } else { "Off" }),
        "turn on or off",
        apply(night_light::Cmd::Toggle),
    );
    let slider = c.slider("Warmth", "kelvin", warmth, move |d, v, _| {
        let k = (night_light::NEUTRAL as f32 - v * span_k).round() as u32;
        d.tell(night_light::Command::Apply {
            cmd: night_light::Cmd::Kelvin(k),
            announce: false,
        });
        d.saved.night_light = crate::state::NightLight {
            on: true,
            kelvin: k,
        };
        d.save_state();
    });
    let mut el = div()
        .class("col grow")
        .child(
            div().class("row").child(icon).child(
                div()
                    .class("col grow")
                    .child(toggle)
                    .child(div().class("small dim").child(format!("{} K", n.kelvin))),
            ),
        )
        .child(
            div()
                .class("row")
                .child(div().class("small dim").child("Warmth"))
                .child(slider),
        );
    if let Some(s) = n.schedule {
        let text = if s.paused {
            "The desktop's schedule is paused"
        } else if s.enabled {
            "Follows the desktop's schedule"
        } else {
            "The desktop's schedule is off"
        };
        el = el.child(div().class("small dim").child(text));
    }
    el
}

pub fn session(c: &mut Ctx) -> Div {
    use sys::session::Command as S;
    /// The command a button stands for (they aren't `Clone`, so they are made when pressed).
    fn command(key: &str) -> S {
        match key {
            "lock" => S::Lock,
            "suspend" => S::Suspend,
            "logout" => S::Logout,
            "restart" => S::Reboot,
            _ => S::Poweroff,
        }
    }
    let confirm = c.opt_bool(Opt::Confirm);
    let waiting =
        c.d.w
            .confirm
            .filter(|(_, t)| t.elapsed().as_secs() < 4)
            .map(|(k, _)| k);
    let all: [(Opt, &'static str, &'static str, &'static str, bool); 5] = [
        (Opt::Lock, "lock", "Lock", "lock", false),
        (Opt::Suspend, "suspend", "Suspend", "moon", false),
        (Opt::Logout, "logout", "Log out", "logout", false),
        (Opt::Restart, "restart", "Restart", "restart", confirm),
        (Opt::PowerOff, "poweroff", "Power off", "power", confirm),
    ];
    let compact = c.w < 220.0 || c.h < 110.0 || c.round;
    let labels = !compact || c.w >= 330.0;
    let mut el = div().class(if compact {
        "row wrap center grow"
    } else {
        "col grow"
    });
    for (opt, key, label, icon, ask) in all {
        if !c.opt_bool(opt) {
            continue;
        }
        let armed = waiting == Some(key);
        let mut body = div()
            .class("row")
            .child(c.icon(icon, if armed { "warn" } else { "text" }));
        if labels {
            body = body.child(if armed {
                format!("{label}? Again")
            } else {
                label.to_string()
            });
        }
        el = el.child(c.press(Button().child(body), label, move |d, _| {
            if ask
                && d.w
                    .confirm
                    .is_none_or(|(k, t)| k != key || t.elapsed().as_secs() >= 4)
            {
                d.w.confirm = Some((key, Instant::now()));
                return;
            }
            d.w.confirm = None;
            d.tell(command(key));
            d.hide();
        }));
    }
    el
}

pub fn keyboard(c: &mut Ctx) -> Div {
    let Some(l) = c.d.data.layouts.clone() else {
        return empty(format!("Keyboard layouts {}", c.d.caps.unavailable()));
    };
    use sys::keyboard::{Command, Switch};
    c.bind("n", "next layout", |d, _| {
        d.tell(Command::Switch(Switch::Next))
    });
    let current = l.layouts.get(l.current).cloned();
    if c.round || c.h < 50.0 {
        let short = current.map(|l| l.short).unwrap_or_default();
        return c.press(
            div()
                .class("col center")
                .child(c.icon("keyboard", "dim"))
                .child(div().class("display xs").child(short)),
            "next layout",
            |d, _| d.tell(Command::Switch(Switch::Next)),
        );
    }
    let mut el = div().class("col grow");
    for (i, lay) in l.layouts.iter().enumerate() {
        let on = i == l.current;
        let row = div()
            .class(if on { "item on" } else { "item" })
            .child(
                div()
                    .class("badge")
                    .style(if on { "" } else { "background: var(--faint)" })
                    .child(lay.short.clone()),
            )
            .child(div().class("grow ellipsis").child(lay.name.clone()));
        el = el.child(c.press(row, "switch to it", move |d, _| {
            d.tell(Command::Switch(Switch::Index(i as u32)))
        }));
    }
    el
}

pub fn color_picker(c: &mut Ctx) -> Div {
    let can = c.d.caps.color_picker;
    let colors = c.d.saved.colors.clone();
    let pick = |d: &mut Dash, _: &mut Context<Dash>| {
        d.tell(sys::color::Command::Pick);
        // The pick happens on the screen behind: get out of the way.
        if d.solo.is_none() {
            d.hide();
        }
    };
    if can {
        c.bind("p", "pick a color", pick);
    }
    let mut el = div().class("col grow");
    let top = div().class("row").child(c.icon("pipette", "text"));
    el = el.child(if can {
        top.child(c.button("Pick a color", "pick", pick))
    } else {
        top.child(
            div()
                .class("dim small")
                .child(format!("Picking {}", c.d.caps.unavailable())),
        )
    });
    if colors.is_empty() {
        el = el.child(empty("Picked colors show here"));
    }
    let mut grid = div().class("row wrap");
    for hex in colors {
        let h = hex.clone();
        let sw = div()
            .class("col center")
            .style("gap: 1px")
            .child(div().style(format!("width: 30px; height: 20px; border-radius: 3px; border: 1px solid var(--border); background: {hex}")))
            .child(div().class("small mono").child(hex.clone()));
        grid = grid.child(c.press(sw, "copy it", move |d, _| copy_text(d, &h)));
    }
    el.child(grid)
}

/// Put `text` on the clipboard.
pub fn copy_text(d: &mut Dash, text: &str) {
    d.tell(sys::clipboard::Command::Set(sys::clipboard::Clip {
        kind: sys::clipboard::ClipKind::Text,
        data: Arc::new(text.as_bytes().to_vec()),
    }));
    d.toast(format!("Copied {text}"));
}

//! Wi-Fi (NetworkManager) and Bluetooth (BlueZ): the radio, what is in range, what is saved or
//! paired, a page of details for one, and the prompts for passwords and pairing codes.

use super::{Ctx, empty, line};
use crate::dash::Dash;
use blenderui::prelude::*;
use niri_dash_system::{bluetooth as bt, clean_label, wifi};

/// Signal bars, as Blender would draw them: four little blocks.
fn bars(n: i32) -> Div {
    div()
        .class("row")
        .style("gap: 1px; align-items: flex-end; height: 12px")
        .children((0..4).map(|i| {
            div().style(format!(
                "width: 3px; height: {}px; background: {}",
                4 + i * 2,
                if i < n { "var(--text)" } else { "var(--track)" }
            ))
        }))
}

fn wifi_bars(signal: i32) -> i32 {
    match signal {
        s if s >= 75 => 4,
        s if s >= 50 => 3,
        s if s >= 25 => 2,
        s if s > 0 => 1,
        _ => 0,
    }
}

/// A text field whose text lives in the dashboard (`get`/`put`).
fn field(
    c: &mut Ctx,
    value: String,
    placeholder: &'static str,
    password: bool,
    put: fn(&mut Dash, String),
) -> Div {
    let hd = c.hd;
    let f = TextField()
        .value(value)
        .placeholder(placeholder)
        .class("grow")
        .on_input(move |e: &ChangeEvent, _: &mut Window, app: &mut App| {
            let v = e.value.clone();
            hd.update(app, |d, _| put(d, v));
        });
    let f = if password {
        f.input_type("password")
    } else {
        f
    };
    c.field(f)
}

pub fn wifi(c: &mut Ctx) -> Div {
    use wifi::Command as W;
    let Some(s) = c.d.data.wifi.as_deref().cloned() else {
        return empty("Asking NetworkManager…");
    };
    if !s.available {
        return empty("No Wi-Fi device");
    }
    c.bind(
        "t",
        if s.enabled { "turn off" } else { "turn on" },
        |d, _| d.tell(W::Toggle),
    );
    c.bind("r", "rescan", |d, _| d.tell(W::Rescan));
    if !s.ssid.is_empty() {
        c.bind("d", "disconnect", |d, _| d.tell(W::Disconnect));
    }
    let ssid = clean_label(&s.ssid);
    if c.round || c.h < 60.0 {
        let el = div()
            .class("col center")
            .child(c.icon_l(
                if s.enabled { "wifi" } else { "wifi-off" },
                if s.ssid.is_empty() { "dim" } else { "text" },
            ))
            .child(div().class("small ellipsis").child(if ssid.is_empty() {
                "Off".to_string()
            } else {
                ssid
            }));
        return c.press(el, "turn on or off", |d, _| d.tell(W::Toggle));
    }
    let head = div()
        .class("row")
        .child(
            c.press(
                div()
                    .class("row")
                    .child(Checkbox().checked(s.enabled))
                    .child("Wi-Fi"),
                "turn on or off",
                |d, _| d.tell(W::Toggle),
            ),
        )
        .child(div().class("grow"))
        .child(c.button(
            if s.scanning { "Scanning…" } else { "Rescan" },
            "rescan",
            |d, _| d.tell(W::Rescan),
        ));
    let mut el = div().class("col grow").child(head);
    if s.blocked {
        el = el.child(
            div()
                .class("warn small")
                .child("Blocked by a hardware switch or rfkill"),
        );
    }
    let status = match &s.activity {
        wifi::Activity::Idle if !s.connecting.is_empty() => {
            format!("Connecting to {}…", s.connecting)
        }
        a => a.to_string(),
    };
    if !status.is_empty() {
        let bad = matches!(s.activity, wifi::Activity::Failed(_));
        el = el.child(
            div()
                .class(if bad { "small bad" } else { "small dim" })
                .child(status),
        );
    }
    if s.link.connectivity == wifi::Connectivity::Portal {
        el = el.child(
            div()
                .class("row")
                .child(
                    div()
                        .class("warn small grow")
                        .child("This network wants you to sign in"),
                )
                .child(c.button("Sign in", "open the login page", |d, _| d.tell(W::SignIn))),
        );
    }
    // A password prompt comes first: something is waiting for it.
    if let Some(p) = s.prompt.clone() {
        let serial = p.serial;
        let mut box_ = div()
            .class("col")
            .style("border: 1px solid var(--accent); border-radius: 4px; padding: 6px");
        box_ = box_.child(
            div()
                .class("title")
                .child(format!("Password for {}", clean_label(&p.ssid))),
        );
        if p.retry {
            box_ = box_.child(
                div()
                    .class("small bad")
                    .child("That didn't work. Try again."),
            );
        }
        if !p.message.is_empty() {
            box_ = box_.child(div().class("small warn").child(p.message.clone()));
        }
        let secret = c.d.w.wifi_secret.clone();
        box_ = box_.child(
            div()
                .class("row")
                .child(field(c, secret, "Password", true, |d, v| {
                    d.w.wifi_secret = v
                })),
        );
        let connect = c.button("Connect", "connect", move |d, _| {
            let answer = wifi::Answer {
                password: std::mem::take(&mut d.w.wifi_secret),
                ..Default::default()
            };
            d.tell(W::Answer { serial, answer });
        });
        let cancel = c.button("Cancel", "cancel", |d, _| {
            d.w.wifi_secret.clear();
            d.tell(W::CancelPrompt);
        });
        el = el.child(box_.child(div().class("row").child(connect).child(cancel)));
    }
    // The details page of one network.
    if let Some(det) = s.detail.clone() {
        let mut page = div().class("col scroll");
        page = page.child(
            div()
                .class("row")
                .child(c.button("‹ Back", "back to the list", |d, _| {
                    d.tell(W::Select { key: String::new() })
                }))
                .child(
                    div()
                        .class("title grow ellipsis")
                        .child(clean_label(&det.ssid)),
                ),
        );
        for (k, v) in wifi::facts(&det, &s.link) {
            page = page.child(
                c.read(
                    div()
                        .class("item small")
                        .child(div().class("dim").style("width: 40%").child(k))
                        .child(div().class("grow ellipsis").child(v)),
                ),
            );
        }
        if let Some(n) = det.network.clone().filter(|n| !n.active) {
            let key = n.key();
            page = page.child(c.button("Connect", "connect", move |d, _| {
                d.tell(W::Connect { key: key.clone() })
            }));
        }
        if let Some(prof) = det.profile.clone() {
            let (uuid, uuid2, uuid3) = (prof.uuid.clone(), prof.uuid.clone(), prof.uuid.clone());
            let auto = prof.autoconnect;
            page = page.child(
                c.press(
                    div()
                        .class("row")
                        .child(Checkbox().checked(auto))
                        .child("Connect by itself"),
                    "connect by itself",
                    move |d, _| {
                        d.tell(W::SetAutoconnect {
                            uuid: uuid.clone(),
                            on: !auto,
                        })
                    },
                ),
            );
            match s.secret.clone().filter(|x| x.uuid == prof.uuid) {
                Some(sec) => {
                    page = page.child(
                        div()
                            .class("row")
                            .child(div().class("dim small").child("Password"))
                            .child(div().class("mono").child(sec.password)),
                    );
                    if let Some(pic) = crate::icons::image(&sec.qr) {
                        page = page.child(img().style("width: 120px; height: 120px").image(pic));
                    }
                    page = page.child(
                        c.button("Hide the password", "hide it", |d, _| d.tell(W::HideSecret)),
                    );
                }
                None => {
                    page = page.child(c.button("Show the password", "show it", move |d, _| {
                        d.tell(W::Reveal {
                            uuid: uuid2.clone(),
                        })
                    }))
                }
            }
            page = page.child(c.button("Forget", "forget it", move |d, _| {
                d.tell(W::Forget {
                    uuid: uuid3.clone(),
                })
            }));
        }
        return el.child(page);
    }
    if !s.enabled {
        return el.child(empty("Wi-Fi is off"));
    }
    let mut list = div().class("col scroll");
    for n in s.networks.clone() {
        let key = n.key();
        let key2 = key.clone();
        let connecting = s.connecting_key == key;
        let mut row = div()
            .class(if n.active { "item on" } else { "item" })
            .child(bars(wifi_bars(n.signal)))
            .child(div().class("grow ellipsis").child(clean_label(&n.ssid)));
        if n.security.needs_secret() {
            row = row.child(c.icon("lock-small", "faint"));
        }
        if connecting {
            row = row.child(div().class("small warn").child("…"));
        } else if n.known() {
            row = row.child(div().class("small faint").child("saved"));
        }
        let row = c.press(
            row,
            if n.active { "details" } else { "connect" },
            move |d, _| {
                let active = d
                    .data
                    .wifi
                    .as_ref()
                    .is_some_and(|w| w.networks.iter().any(|x| x.key() == key && x.active));
                if active {
                    d.tell(W::Select { key: key.clone() });
                } else {
                    d.tell(W::Connect { key: key.clone() });
                }
            },
        );
        if c.entered() && c.stops.len() == c.cursor.unwrap_or(usize::MAX) + 1 {
            c.bind("i", "details", move |d, _| {
                d.tell(W::Select { key: key2.clone() })
            });
        }
        list = list.child(row);
    }
    if s.networks.is_empty() {
        list = list.child(empty(if s.scanning {
            "Looking for networks…"
        } else {
            "No networks in range"
        }));
    }
    let away: Vec<_> = s
        .saved
        .iter()
        .filter(|p| {
            !s.networks
                .iter()
                .any(|n| n.profile.as_ref().is_some_and(|q| q.uuid == p.uuid))
        })
        .cloned()
        .collect();
    if !away.is_empty() && c.h > 160.0 {
        list = list.child(div().class("group").child("Saved, not in range"));
        for p in away {
            let key = p.key();
            let row = div()
                .class("item dim")
                .child(div().class("grow ellipsis").child(clean_label(&p.ssid)))
                .child(div().class("small faint").child(wifi::day(p.timestamp)));
            list = list.child(c.press(row, "details", move |d, _| {
                d.tell(W::Select { key: key.clone() })
            }));
        }
    }
    el.child(list)
}

pub fn bluetooth(c: &mut Ctx) -> Div {
    use bt::Command as B;
    let Some(s) = c.d.data.bluetooth.as_deref().cloned() else {
        return empty("Asking BlueZ…");
    };
    if !s.available {
        return empty("No Bluetooth adapter");
    }
    c.bind(
        "t",
        if s.powered { "turn off" } else { "turn on" },
        |d, _| d.tell(B::Toggle),
    );
    let discovering = s.discovering;
    c.bind(
        "r",
        if discovering {
            "stop looking"
        } else {
            "look for devices"
        },
        move |d, _| d.tell(B::Scan(!discovering)),
    );
    let connected = s.devices.iter().filter(|d| d.connected).count();
    if c.round || c.h < 60.0 {
        let el = div()
            .class("col center")
            .child(c.icon_l(
                "bluetooth",
                if s.powered {
                    if connected > 0 { "accent" } else { "text" }
                } else {
                    "dim"
                },
            ))
            .child(div().class("small").child(if s.powered {
                format!("{connected} connected")
            } else {
                "Off".into()
            }));
        return c.press(el, "turn on or off", |d, _| d.tell(B::Toggle));
    }
    let head = div()
        .class("row")
        .child(
            c.press(
                div()
                    .class("row")
                    .child(Checkbox().checked(s.powered))
                    .child("Bluetooth"),
                "turn on or off",
                |d, _| d.tell(B::Toggle),
            ),
        )
        .child(div().class("grow"))
        .child(c.button(
            if discovering { "Stop" } else { "Look around" },
            "look for devices",
            move |d, _| d.tell(B::Scan(!discovering)),
        ));
    let mut el = div().class("col grow").child(head);
    if s.blocked {
        el = el.child(div().class("warn small").child("Blocked by rfkill"));
    }
    let activity = s.activity.to_string();
    if !activity.is_empty() {
        let bad = matches!(s.activity, bt::Activity::Failed(_));
        el = el.child(
            div()
                .class(if bad { "small bad" } else { "small dim" })
                .child(activity),
        );
    }
    if let Some(p) = s.prompt.clone() {
        let serial = p.serial;
        let (ask, code_needed) = match &p.kind {
            bt::PromptKind::Confirm(n) => (format!("Does {} show {n:06}?", p.name), false),
            bt::PromptKind::EnterPin => (format!("Type the PIN for {}", p.name), true),
            bt::PromptKind::EnterPasskey => (format!("Type the passkey for {}", p.name), true),
            bt::PromptKind::ShowPin(pin) => (format!("Type {pin} on {}", p.name), false),
            bt::PromptKind::ShowPasskey(k, _) => (format!("Type {k:06} on {}", p.name), false),
            bt::PromptKind::Authorize => (format!("Let {} connect?", p.name), false),
        };
        let mut box_ = div()
            .class("col")
            .style("border: 1px solid var(--accent); border-radius: 4px; padding: 6px")
            .child(div().class("title").child(ask));
        if code_needed {
            let text = c.d.w.bt_text.clone();
            box_ = box_.child(
                div()
                    .class("row")
                    .child(field(c, text, "Code", false, |d, v| d.w.bt_text = v)),
            );
        }
        let yes = c.button(
            if code_needed { "Pair" } else { "Yes" },
            "answer",
            move |d, _| {
                let answer = if code_needed {
                    bt::Answer::Code(std::mem::take(&mut d.w.bt_text))
                } else {
                    bt::Answer::Yes
                };
                d.tell(B::Answer { serial, answer });
            },
        );
        let no = c.button("No", "decline", |d, _| d.tell(B::Decline));
        el = el.child(box_.child(div().class("row").child(yes).child(no)));
    }
    if let Some(dev) = s.detail.clone() {
        let mut page = div().class("col scroll");
        page = page.child(
            div()
                .class("row")
                .child(c.button("‹ Back", "back to the list", |d, _| {
                    d.tell(B::Select(String::new()))
                }))
                .child(div().class("title grow ellipsis").child(dev.name.clone())),
        );
        for (k, v) in bt::facts(&dev) {
            page = page.child(
                c.read(
                    div()
                        .class("item small")
                        .child(div().class("dim").style("width: 40%").child(k))
                        .child(div().class("grow ellipsis").child(v)),
                ),
            );
        }
        let p = dev.path.clone();
        let (p1, p2, p3, p4, p5) = (p.clone(), p.clone(), p.clone(), p.clone(), p.clone());
        let (trusted, blocked) = (dev.trusted, dev.blocked);
        page = page.child(if dev.connected {
            c.button("Disconnect", "disconnect", move |d, _| {
                d.tell(B::Disconnect(p1.clone()))
            })
        } else {
            c.button("Connect", "connect", move |d, _| {
                d.tell(B::Connect(p1.clone()))
            })
        });
        page = page
            .child(
                c.press(
                    div()
                        .class("row")
                        .child(Checkbox().checked(trusted))
                        .child("Trusted"),
                    "trust it",
                    move |d, _| d.tell(B::SetTrusted(p2.clone(), !trusted)),
                ),
            )
            .child(
                c.press(
                    div()
                        .class("row")
                        .child(Checkbox().checked(blocked))
                        .child("Blocked"),
                    "block it",
                    move |d, _| d.tell(B::SetBlocked(p3.clone(), !blocked)),
                ),
            );
        let name = c.d.w.bt_text.clone();
        page = page.child(
            div()
                .class("row")
                .child(field(c, name, "New name", false, |d, v| d.w.bt_text = v))
                .child(c.button("Rename", "rename it", move |d, _| {
                    let n = std::mem::take(&mut d.w.bt_text);
                    if !n.trim().is_empty() {
                        d.tell(B::Rename(p4.clone(), n));
                    }
                })),
        );
        page = page.child(c.button("Forget", "forget it", move |d, _| {
            d.tell(B::Forget(p5.clone()))
        }));
        return el.child(page);
    }
    if !s.powered {
        return el.child(empty("Bluetooth is off"));
    }
    let mut list = div().class("col scroll");
    if s.devices.is_empty() {
        list = list.child(empty("Nothing paired yet"));
    }
    for dev in s.devices.clone() {
        let path = dev.path.clone();
        let path2 = dev.path.clone();
        let mut row = div()
            .class(if dev.connected { "item on" } else { "item" })
            .child(div().class(if dev.connected {
                "dot on"
            } else if dev.busy {
                "dot busy"
            } else {
                "dot"
            }))
            .child(div().class("grow ellipsis").child(dev.name.clone()));
        if dev.battery >= 0 {
            row = row.child(div().class("small dim").child(format!("{}%", dev.battery)));
        }
        let connected = dev.connected;
        if c.here() {
            c.bind("i", "details", move |d, _| d.tell(B::Select(path2.clone())));
        }
        list = list.child(c.press(
            row,
            if connected { "disconnect" } else { "connect" },
            move |d, _| {
                d.tell(if connected {
                    B::Disconnect(path.clone())
                } else {
                    B::Connect(path.clone())
                })
            },
        ));
    }
    if discovering || !s.nearby.is_empty() {
        list = list.child(div().class("group").child(if s.unnamed > 0 {
            format!("Nearby (and {} without a name)", s.unnamed)
        } else {
            "Nearby".into()
        }));
        for dev in s.nearby.clone() {
            let path = dev.path.clone();
            let row = div()
                .class("item")
                .child(bars(dev.bars()))
                .child(div().class("grow ellipsis").child(dev.name.clone()))
                .child(div().class("small faint").child("pair"));
            list = list.child(c.press(row, "pair", move |d, _| d.tell(B::Pair(path.clone()))));
        }
    }
    el.child(list)
        .child(line(s.adapter_name.clone()).class("small faint"))
}

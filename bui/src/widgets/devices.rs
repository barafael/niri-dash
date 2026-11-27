//! Privacy (who records, who shares the screen), removable drives and phones.

use super::{Ctx, empty, line};
use blenderui::prelude::*;
use niri_dash_system::{drives, phones, privacy};

pub fn privacy(c: &mut Ctx) -> Div {
    let u = c.d.data.privacy.clone();
    let casts = c.d.data.casts.clone();
    let busy = !u.mic.is_empty() || !u.camera.is_empty() || casts.iter().any(|c| c.active);
    if c.round {
        return div()
            .class("col center")
            .child(c.icon_l(
                if busy { "mic" } else { "check" },
                if busy { "warn" } else { "good" },
            ))
            .child(
                div()
                    .class("small")
                    .child(if busy { "In use" } else { "All quiet" }),
            );
    }
    if !busy {
        return div()
            .class("col center grow")
            .child(c.icon_l("check", "good"))
            .child(div().class("dim").child("Nothing records or shares"));
    }
    let mut el = c.scroll();
    let group = |c: &mut Ctx, el: Div, icon: &str, what: &str, apps: &[String]| {
        let mut el = el;
        if !apps.is_empty() {
            el = el.child(
                div()
                    .class("row")
                    .child(c.icon(icon, "warn"))
                    .child(div().class("title").child(what.to_string())),
            );
            for a in apps {
                el = el.child(
                    c.read_copy(
                        div()
                            .class("item small")
                            .child(line(a.clone()).class("grow")),
                        a.clone(),
                    ),
                );
            }
        }
        el
    };
    el = group(c, el, "mic", "Microphone", &u.mic);
    el = group(c, el, "camera", "Camera", &u.camera);
    if !c.d.caps.screencast && !c.d.caps.simulated {
        el = el.child(div().class("small faint").child(format!(
            "Noticing screen sharing {}",
            c.d.caps.unavailable()
        )));
    }
    let active: Vec<_> = casts.into_iter().filter(|c| c.active).collect();
    if !active.is_empty() {
        el = el.child(
            div()
                .class("row")
                .child(c.icon("screen-share", "warn"))
                .child(div().class("title").child("Screen")),
        );
        for cast in active {
            let sid = cast.session_id;
            let who = cast
                .process
                .clone()
                .or_else(|| {
                    cast.pw_node.and_then(|n| {
                        u.video_readers
                            .iter()
                            .find(|(id, _)| *id == n)
                            .map(|(_, w)| w.clone())
                    })
                })
                .unwrap_or_else(|| "Something".into());
            el = el.child(
                div()
                    .class("item small")
                    .child(
                        div()
                            .class("col grow")
                            .child(line(format!("{who} sees {}", cast.target))),
                    )
                    .child(c.button("Stop", "stop sharing", move |d, _| {
                        d.tell(privacy::Command::StopCast(sid))
                    })),
            );
        }
    }
    el
}

pub fn drives(c: &mut Ctx) -> Div {
    use drives::Command as K;
    let s = c.d.data.drives.clone();
    if !s.available {
        return empty("UDisks isn't running");
    }
    if s.drives.is_empty() {
        return div()
            .class("col center grow")
            .child(c.icon_l("usb", "faint"))
            .child(div().class("dim").child("No removable drives"));
    }
    if c.round {
        return div()
            .class("col center")
            .child(c.icon_l("usb", "text"))
            .child(div().class("small").child(format!("{}", s.drives.len())));
    }
    c.bind("X", "eject all", |d, _| d.tell(K::EjectAll));
    let mut el = c.scroll();
    for dr in s.drives {
        let dpath = dr.path.clone();
        el = el.child(
            div()
                .class("row")
                .child(c.icon("usb", "text"))
                .child(
                    div()
                        .class("col grow")
                        .child(line(dr.name.clone()).class("title"))
                        .child(line(dr.summary()).class("small dim")),
                )
                .child(if dr.ejectable || dr.can_power_off {
                    let p = dpath.clone();
                    c.press(
                        Button().child(c.icon("eject", "text")),
                        "eject",
                        move |d, _| d.tell(K::Eject(p.clone())),
                    )
                } else {
                    div()
                }),
        );
        for v in dr.volumes {
            let path = v.path.clone();
            let p2 = dpath.clone();
            if c.here() {
                c.bind("x", "eject the drive", move |d, _| {
                    d.tell(K::Eject(p2.clone()))
                });
            }
            let mut row = div().class("item small").style("padding-left: 22px").child(
                div().class("grow ellipsis").child(if v.name.is_empty() {
                    "Volume".to_string()
                } else {
                    v.name.clone()
                }),
            );
            if v.locked {
                row = row
                    .child(c.icon("lock-small", "faint"))
                    .child(div().class("faint").child("locked"));
                el = el.child(c.read(row));
                continue;
            }
            match v.mount.clone() {
                Some(m) => {
                    let p = path.clone();
                    if c.here() {
                        c.bind("u", "unmount", move |d, _| d.tell(K::Unmount(p.clone())));
                    }
                    let m2 = m.clone();
                    let p = path.clone();
                    row = row
                        .child(c.press(Button().child("Open"), "open it", move |d, _| {
                            d.tell(K::Open(m2.clone()));
                            d.hide();
                        }))
                        .child(c.press(Button().child("Unmount"), "unmount", move |d, _| {
                            d.tell(K::Unmount(p.clone()))
                        }));
                    let _ = m;
                    el = el.child(row);
                }
                None => {
                    let p = path.clone();
                    el = el.child(row.child(c.press(
                        Button().child("Mount"),
                        "mount",
                        move |d, _| d.tell(K::Mount(p.clone())),
                    )));
                }
            }
        }
    }
    el
}

pub fn phone(c: &mut Ctx) -> Div {
    use phones::Command as P;
    let s = c.d.data.phones.clone();
    if !s.available {
        return empty("KDE Connect isn't running");
    }
    if s.phones.is_empty() {
        return empty("No paired devices");
    }
    if c.round {
        let p = &s.phones[0];
        return div()
            .class("col center")
            .child(c.icon_l("phone", if p.reachable { "text" } else { "faint" }))
            .child(
                div()
                    .class("small")
                    .child(p.charge.map(|c| format!("{c}%")).unwrap_or_default()),
            );
    }
    let mut el = c.scroll();
    for p in s.phones {
        let (id, id2) = (p.id.clone(), p.id.clone());
        let mut row = div()
            .class("row")
            .child(c.icon("phone", if p.reachable { "text" } else { "faint" }))
            .child(
                div()
                    .class("col grow")
                    .child(line(p.name.clone()).class("title"))
                    .child(line(p.status()).class("small dim")),
            );
        if p.reachable && p.can_ring {
            if c.here() {
                let i = id.clone();
                c.bind("r", "ring it", move |d, _| d.tell(P::Ring(i.clone())));
            }
            row = row.child(c.press(
                Button().child(c.icon("ring", "text")),
                "ring it",
                move |d, _| d.tell(P::Ring(id.clone())),
            ));
        }
        if p.reachable && p.can_share {
            row = row.child(c.press(
                Button().child(c.icon("send", "text")),
                "send files",
                move |d, _| {
                    d.tell(P::SendFiles(id2.clone()));
                    d.hide();
                },
            ));
        }
        el = el.child(row);
        if let Some(ch) = p.charge {
            el = el.child(super::meter(
                ch as f32 / 100.0,
                if ch < 15 && !p.charging {
                    "bad"
                } else if p.charging {
                    "good"
                } else {
                    ""
                },
            ));
        }
    }
    el
}

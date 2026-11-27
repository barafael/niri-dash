//! Windows, Claude sessions, notifications, the tray, media, the clipboard and notes.

use super::{Ctx, act, empty, line, meter, run};
use crate::dash::Dash;
use crate::widget_settings::Opt;
use blenderui::prelude::*;
use niri_dash_system::{self as sys, claude, media, notifications as notif, tray, windows};

/// An app's icon from the system, or one of ours.
fn app_icon(c: &Ctx, src: &sys::IconSrc, fallback: &str) -> Div {
    match crate::icons::image(src) {
        Some(pic) => img().class("icon").image(pic),
        None => c.icon(fallback, "dim"),
    }
}

pub fn windows(c: &mut Ctx) -> Div {
    let s = c.d.data.windows.clone();
    if !c.d.caps.windows && !c.d.caps.simulated && !s.connected {
        return empty(format!("Window list {}", c.d.caps.unavailable()));
    }
    if c.round {
        let icon = s
            .focused_icon
            .as_ref()
            .and_then(crate::icons::image)
            .map(|i| img().class("icon l").image(i))
            .unwrap_or_else(|| c.icon_l("window", "text"));
        return div()
            .class("col center")
            .child(icon)
            .child(div().class("small").child(format!("{} windows", s.count)));
    }
    if s.rows.is_empty() {
        return empty("No windows");
    }
    let mut list = c.scroll();
    for r in &s.rows {
        if r.header {
            list = list.child(div().class("item header").child(r.title.clone()));
            continue;
        }
        let id = r.id;
        if c.here() {
            c.bind("d", "close it", move |d, _| {
                d.tell(windows::Command::Close(id))
            });
            c.bind("x", "close it", move |d, _| {
                d.tell(windows::Command::Close(id))
            });
        }
        let mut row = div()
            .class(if r.focused { "item on" } else { "item" })
            .child(app_icon(c, &r.icon, "window"))
            .child(div().class("grow ellipsis").child(r.title.clone()));
        if r.urgent {
            row = row.child(div().class("dot busy"));
        }
        if c.w > 260.0 {
            row = row.child(
                div()
                    .class("small faint ellipsis")
                    .style("max-width: 35%")
                    .child(r.app.clone()),
            );
        }
        list = list.child(c.press(row, "go to it", move |d, _| {
            d.tell(windows::Command::Focus(id));
            d.hide();
        }));
        c.copies(r.title.clone());
    }
    list
}

pub fn claude(c: &mut Ctx) -> Div {
    let s = c.d.data.claude.clone();
    if c.round {
        return div()
            .class("col center")
            .child(c.icon_l("sparkle", if s.busy > 0 { "warn" } else { "text" }))
            .child(div().class("small").child(format!("{} busy", s.busy)));
    }
    if s.sessions.is_empty() {
        return empty("No Claude Code sessions");
    }
    let mut list = c.scroll();
    for sess in &s.sessions {
        let (pid, window) = (sess.pid, sess.window);
        let busy = sess.status == claude::Status::Busy;
        let row = div()
            .class("item")
            .child(div().class(if busy { "dot busy" } else { "dot on" }))
            .child(
                div()
                    .class("col grow")
                    .style("gap: 0")
                    .child(line(sess.title.clone()))
                    .child(line(sess.place.clone()).class("small faint")),
            )
            .child(
                div()
                    .class(if busy { "small warn" } else { "small dim" })
                    .child(sess.status_text.clone()),
            );
        list = list.child(c.press(row, "go to its window", move |d, _| {
            match window {
                Some(w) => d.tell(windows::Command::Focus(w)),
                None => d.tell(windows::Command::FocusPid(pid)),
            }
            d.tell(windows::Command::ShowTab(pid));
            d.hide();
        }));
        c.copies(sess.place.clone());
    }
    list
}

/// Do not disturb on, or off if it is on.
pub fn toggle_dnd(d: &mut Dash) {
    let now = chrono::Local::now();
    let dnd = if d.data.notifications.dnd.active(now) {
        notif::Dnd::Off
    } else {
        notif::Dnd::Forever
    };
    set_dnd(d, dnd);
}

fn set_dnd(d: &mut Dash, dnd: notif::Dnd) {
    d.tell(notif::Command::SetDnd(dnd));
    d.saved.dnd_until = dnd.to_saved();
    d.save_state();
    d.w.dnd_menu = false;
}

pub fn notifications(c: &mut Ctx) -> Div {
    let s = c.d.data.notifications.clone();
    let now = chrono::Local::now();
    let dnd = s.dnd.active(now);
    c.bind("z", "do not disturb", |d, _| d.w.dnd_menu = !d.w.dnd_menu);
    if !s.items.is_empty() {
        c.bind("D", "clear all", |d, _| d.tell(notif::Command::Clear));
    }
    if c.round {
        let el = div()
            .class("col center")
            .child(c.icon_l(
                if dnd { "bell-off" } else { "bell" },
                if dnd { "warn" } else { "text" },
            ))
            .child(div().class("small").child(format!("{}", s.items.len())));
        return c.press(el, "do not disturb", |d, _| toggle_dnd(d));
    }
    let bell = c.press(
        c.icon(
            if dnd { "bell-off" } else { "bell" },
            if dnd { "warn" } else { "text" },
        ),
        "do not disturb",
        |d, _| d.w.dnd_menu = !d.w.dnd_menu,
    );
    let mut head = div()
        .class("row")
        .child(bell)
        .child(div().class("small dim grow").child(match s.dnd {
            notif::Dnd::Off => "Notifications pop up".to_string(),
            notif::Dnd::Forever => "Do not disturb".to_string(),
            notif::Dnd::Until(t) if dnd => format!("Do not disturb until {}", t.format("%H:%M")),
            _ => "Notifications pop up".to_string(),
        }));
    if s.missed > 0 {
        head = head.child(div().class("badge").child(format!("{} missed", s.missed)));
    }
    if !s.items.is_empty() {
        head = head.child(c.button("Clear", "clear all", |d, _| d.tell(notif::Command::Clear)));
    }
    let mut el = div().class("col grow").child(head);
    if c.d.w.dnd_menu {
        let mut menu = div()
            .class("col")
            .style("border: 1px solid var(--border); border-radius: 4px; padding: 4px");
        let options: [(&str, notif::Dnd); 4] = [
            ("Off", notif::Dnd::Off),
            (
                "For an hour",
                notif::Dnd::Until(now + chrono::Duration::hours(1)),
            ),
            (
                "Until tomorrow morning",
                notif::Dnd::Until(notif::next_hour(now, 8)),
            ),
            ("Until turned off", notif::Dnd::Forever),
        ];
        for (label, dnd) in options {
            menu = menu.child(
                c.press(div().class("item").child(label), "pick", move |d, _| {
                    set_dnd(d, dnd)
                }),
            );
        }
        el = el.child(menu);
    }
    if s.items.is_empty() {
        return el.child(empty("No notifications"));
    }
    let mut list = c.scroll();
    for n in s.items.iter() {
        let id = n.id;
        if c.here() {
            c.bind("d", "dismiss", move |d, _| {
                d.tell(notif::Command::Dismiss(id))
            });
        }
        let default = n.has_default();
        let mut text = div().class("col grow").style("gap: 1px").child(
            div()
                .class("row small")
                .child(div().class("grow faint ellipsis").child(n.app.clone()))
                .child(
                    div()
                        .class("faint")
                        .child(n.time.format("%H:%M").to_string()),
                ),
        );
        text =
            text.child(line(n.summary.clone()).class(if n.urgent { "title bad" } else { "title" }));
        if !n.body.is_empty() {
            text = text.child(
                div()
                    .class("small dim")
                    .style("max-height: 3.6em; overflow: hidden")
                    .child(n.body.clone()),
            );
        }
        let hd = c.hd;
        let x = Button()
            .child(c.icon("close", "dim"))
            .style("width: 26px; flex: none")
            .on_click(move |_: &ClickEvent, _: &mut Window, app: &mut App| {
                hd.update(app, |d, cx| {
                    d.tell(notif::Command::Dismiss(id));
                    cx.notify();
                });
                app.stop_propagation();
            });
        let row = div()
            .class("item")
            .style("align-items: flex-start")
            .child(app_icon(c, &n.icon, "bell"))
            .child(text)
            .child(x);
        list = list.child(c.press(
            row,
            if default { "open it" } else { "dismiss" },
            move |d, _| {
                if default {
                    d.tell(notif::Command::Invoke(id));
                    d.hide();
                } else {
                    d.tell(notif::Command::Dismiss(id));
                }
            },
        ));
        c.copies(if n.body.is_empty() {
            n.summary.clone()
        } else {
            format!("{}\n{}", n.summary, n.body)
        });
    }
    el.child(list)
}

pub fn tray(c: &mut Ctx) -> Div {
    let items = c.d.data.tray.clone();
    if items.is_empty() {
        return empty("Nothing in the tray");
    }
    let mut grid = div().class("row wrap");
    for it in items {
        let (a1, a2, a3) = (it.address.clone(), it.address.clone(), it.address.clone());
        let menu_first = it.item_is_menu;
        if c.here() && it.has_menu {
            c.bind("m", "its menu", move |d, _| {
                d.w.menu_cursor = 0;
                d.tell(tray::Command::OpenMenu {
                    address: a3.clone(),
                    x: 0.0,
                    y: 0.0,
                });
            });
        }
        let icon = match crate::icons::image(&it.icon) {
            Some(pic) => img().style("width: 24px; height: 24px").image(pic),
            None => div()
                .class("badge")
                .child(it.title.chars().take(2).collect::<String>()),
        };
        let mut cell = div().class("item").title(it.title.clone()).child(icon);
        if c.w > 260.0 && c.p.w >= 4 {
            cell = cell.child(
                div()
                    .class("small ellipsis")
                    .style("max-width: 110px")
                    .child(it.title.clone()),
            );
        }
        let hd = c.hd;
        if it.has_menu {
            cell =
                cell.on_context_menu(move |e: &ContextMenuEvent, _: &mut Window, app: &mut App| {
                    let (x, y) = (e.position.x.0, e.position.y.0);
                    let a = a2.clone();
                    hd.update(app, |d, cx| {
                        d.w.menu_cursor = 0;
                        d.tell(tray::Command::OpenMenu { address: a, x, y });
                        cx.notify();
                    });
                });
        }
        grid = grid.child(c.press(cell, "activate", move |d, _| {
            if menu_first {
                d.tell(tray::Command::OpenMenu {
                    address: a1.clone(),
                    x: 0.0,
                    y: 0.0,
                });
            } else {
                d.tell(tray::Command::Activate {
                    address: a1.clone(),
                    x: 0,
                    y: 0,
                });
            }
        }));
    }
    grid
}

/// An open tray menu, over everything.
pub fn menu(d: &Dash, hd: WeakEntity<Dash>) -> Option<Div> {
    let m = d.data.menu.as_ref()?;
    let (x, y) = if m.x > 0.0 || m.y > 0.0 {
        (m.x, m.y)
    } else {
        (d.size.0 / 2.0 - 100.0, d.size.1 / 3.0)
    };
    let mut el = div()
        .class("menu")
        .style(format!(
            "left: {}px; top: {}px; z-index: 20",
            x.min(d.size.0 - 220.0).max(0.0),
            y.min(d.size.1 - 200.0).max(0.0)
        ))
        .child(
            div()
                .class("small faint")
                .style("padding: 2px 6px")
                .child(m.title.clone()),
        );
    for (i, e) in m.entries.iter().enumerate() {
        if e.separator {
            el = el.child(div().style("height: 1px; background: var(--border); margin: 2px 0"));
            continue;
        }
        let mark = match e.toggle {
            tray::Toggle::Check if e.checked => "☑ ",
            tray::Toggle::Check => "☐ ",
            tray::Toggle::Radio if e.checked => "◉ ",
            tray::Toggle::Radio => "○ ",
            tray::Toggle::None => "",
        };
        let mut row = div()
            .class(if d.w.menu_cursor == i {
                "item cursor"
            } else {
                "item"
            })
            .style(format!("padding-left: {}px", 6 + e.depth * 12))
            .child(format!("{mark}{}", e.label));
        if e.header || !e.enabled {
            row = row.class("faint");
        } else {
            let (address, id) = (m.address.clone(), e.id);
            row = row.on_click(run::<ClickEvent>(
                hd,
                act(move |d: &mut Dash, _| {
                    d.tell(tray::Command::MenuItem {
                        address: address.clone(),
                        id,
                    });
                    d.data.menu = None;
                }),
            ));
        }
        el = el.child(row);
    }
    // A click outside closes it.
    Some(
        div()
            .style("position: absolute; left: 0; top: 0; right: 0; bottom: 0; z-index: 19")
            .on_click(run::<ClickEvent>(
                hd,
                act(|d: &mut Dash, _| d.data.menu = None),
            ))
            .child(
                el.on_click(|_: &ClickEvent, _: &mut Window, app: &mut App| app.stop_propagation()),
            ),
    )
}

/// Keys while a tray menu is open: j k move, Enter picks, q or Escape closes. True if used.
pub fn menu_key(d: &mut Dash, k: &crate::dash::Key) -> bool {
    let Some(m) = d.data.menu.clone() else {
        return false;
    };
    let pickable: Vec<usize> = m
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.separator && !e.header && e.enabled)
        .map(|(i, _)| i)
        .collect();
    let pos = pickable
        .iter()
        .position(|&i| i == d.w.menu_cursor)
        .unwrap_or(0);
    match (k.name(), k.text()) {
        ("Escape", _) | (_, Some("q")) => d.data.menu = None,
        ("j" | "Down", _) => {
            d.w.menu_cursor = pickable
                .get((pos + 1).min(pickable.len().saturating_sub(1)))
                .copied()
                .unwrap_or(0)
        }
        ("k" | "Up", _) => {
            d.w.menu_cursor = pickable.get(pos.saturating_sub(1)).copied().unwrap_or(0)
        }
        ("Return" | "space", _) => {
            if let Some(e) = m.entries.get(d.w.menu_cursor) {
                d.tell(tray::Command::MenuItem {
                    address: m.address.clone(),
                    id: e.id,
                });
            }
            d.data.menu = None;
        }
        _ => {}
    }
    true
}

pub fn media(c: &mut Ctx) -> Div {
    use media::Command as M;
    let Some(m) = c.d.data.media.clone() else {
        return empty("Nothing playing");
    };
    c.bind("space", if m.playing { "pause" } else { "play" }, |d, _| {
        d.tell(M::PlayPause)
    });
    c.bind("n", "next", |d, _| d.tell(M::Next));
    c.bind("p", "previous", |d, _| d.tell(M::Previous));
    let art = m.art.as_ref().and_then(crate::icons::image);
    if c.round {
        let mut el = div().class("col center");
        if let Some(a) = art {
            el = el.child(
                img()
                    .class("art")
                    .style("border-radius: 9999px; width: 60%; height: 60%")
                    .image(a),
            );
        }
        return c.press(
            el.child(c.icon(if m.playing { "pause" } else { "play" }, "text")),
            "play or pause",
            |d, _| d.tell(M::PlayPause),
        );
    }
    let buttons = div()
        .class("row center")
        .child(c.press(
            Button().child(c.icon("prev", "text")),
            "previous",
            |d, _| d.tell(M::Previous),
        ))
        .child(c.press(
            Button().child(c.icon(if m.playing { "pause" } else { "play" }, "text")),
            "play or pause",
            |d, _| d.tell(M::PlayPause),
        ))
        .child(
            c.press(Button().child(c.icon("next", "text")), "next", |d, _| {
                d.tell(M::Next)
            }),
        );
    let text = div()
        .class("col grow")
        .child(
            line(if m.title.is_empty() {
                m.player.clone()
            } else {
                m.title.clone()
            })
            .class("title"),
        )
        .child(line(m.artist.clone()).class("dim"))
        .child(line(m.player.clone()).class("small faint"));
    let mut top = div().class("row");
    if let Some(a) = art.filter(|_| c.h > 70.0) {
        let side = (c.h * 0.55).clamp(40.0, 160.0);
        top = top.child(
            img()
                .class("art")
                .style(format!("width: {side}px; height: {side}px"))
                .image(a),
        );
    }
    div()
        .class("col grow")
        .child(top.child(text))
        .child(meter(m.progress, ""))
        .child(buttons)
}

pub fn clipboard(c: &mut Ctx) -> Div {
    use sys::clipboard::Command as K;
    if !c.d.caps.clipboard && !c.d.caps.simulated {
        return empty(format!("Clipboard {}", c.d.caps.unavailable()));
    }
    let clips = c.d.data.clips.clone();
    if !clips.is_empty() {
        c.bind("D", "clear the history", |d, _| d.data.clips.clear());
    }
    if c.round {
        return div()
            .class("col center")
            .child(c.icon_l("clipboard", "text"))
            .child(div().class("small").child(format!("{}", clips.len())));
    }
    if clips.is_empty() {
        return empty("What you copy shows here");
    }
    let mut list = c.scroll();
    for (i, clip) in clips.into_iter().enumerate() {
        if c.here() {
            c.bind("d", "remove it", move |d, _| {
                if i < d.data.clips.len() {
                    d.data.clips.remove(i);
                }
            });
        }
        let png = clip.clip.kind == sys::clipboard::ClipKind::Png;
        let mut row = div().class("item");
        if png && let Some((w, h, rgba)) = sys::clipboard::decode_png(&clip.clip.data) {
            let (w, h, rgba) = sys::clipboard::shrink(w, h, rgba, 64);
            row = row.child(
                img()
                    .style("width: 32px; height: 32px")
                    .image_rgba(w, h, rgba),
            );
        }
        row = row.child(line(clip.preview.clone()).class("grow"));
        let data = clip.clip.clone();
        list = list.child(c.press(row, "copy it again", move |d, _| {
            d.tell(K::Set(data.clone()));
            d.toast("Copied");
        }));
    }
    list
}

pub fn notes(c: &mut Ctx) -> Div {
    let name = super::state::note_name(&c.opt_str(Opt::Note));
    let text = c.d.w.note_text(&name);
    let hd = c.hd;
    let n = name.clone();
    let area = TextArea()
        .value(text)
        .placeholder("Write something down")
        .style("width: 100%; flex: 1; min-height: 40px")
        .on_input(move |e: &ChangeEvent, _: &mut Window, app: &mut App| {
            let v = e.value.clone();
            let n = n.clone();
            hd.update(app, |d, _| d.w.edit_note(&n, v));
        });
    div()
        .class("col grow")
        .child(c.field(area))
        .child(div().class("small faint").child(format!("{name}.md")))
}

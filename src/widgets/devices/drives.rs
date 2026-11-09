//! The removable-drives widget: USB sticks and SD cards from UDisks, with mount, unmount, eject
//! and open. One row: a summary and eject; tall: each drive with its volumes. Port of
//! `RemovableDrivesWidget` (`legacy/ui/widgets/devices.slint`) and the drives part of
//! `legacy/src/app/devices.rs`.

use super::parts::{self, Card, Revealed, icon_button, muted, row, text_button};
use crate::dash::{Dash, DashEvent};
use crate::keys::Key;
use crate::kit::{Tk, icon};
use crate::scope::{Scope, Stops};
use crate::widgets::Ctx;
use gpui::prelude::*;
use gpui::{AnyElement, Context, ScrollHandle, SharedString, div, px};
use niri_dash_system::perf::human_bytes;
use niri_dash_system::{Tell, drives};

/// A volume as the widget lists it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Vol {
    /// The block device.
    pub path: String,
    pub name: String,
    pub detail: String,
    /// Where it is mounted; empty if it isn't.
    pub mount: String,
    pub mounted: bool,
    pub locked: bool,
    /// Its place in the flat list of all volumes (the keyboard cursor's stops).
    pub index: usize,
    /// Its drive's path.
    pub drive: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Drv {
    pub path: String,
    pub name: String,
    pub detail: String,
    pub volumes: Vec<Vol>,
}

#[derive(Default)]
pub struct State {
    pub available: bool,
    pub drives: Vec<Drv>,
    /// Every volume of every drive, for the keyboard cursor.
    pub volumes: Vec<Vol>,
    pub scope: Scope,
    list: ScrollHandle,
    revealed: Revealed,
}

/// What the widget can do.
#[derive(Debug, Clone, PartialEq)]
pub enum Do {
    Mount(String),
    Unmount(String),
    Eject(String),
    EjectAll,
    /// Open a mount point in the file manager (which hides the dashboard).
    Open(String),
}

fn volume_detail(v: &drives::Volume) -> String {
    if v.locked {
        "Locked".into()
    } else if let Some(m) = &v.mount {
        m.clone()
    } else if v.size > 0 {
        format!("{} · not mounted", human_bytes(v.size))
    } else {
        "Not mounted".into()
    }
}

impl State {
    fn set(&mut self, s: drives::State) {
        self.available = s.available;
        self.volumes.clear();
        self.drives.clear();
        for drive in &s.drives {
            let mut volumes = Vec::new();
            for v in &drive.volumes {
                let vol = Vol {
                    path: v.path.clone(),
                    name: v.name.clone(),
                    detail: volume_detail(v),
                    mount: v.mount.clone().unwrap_or_default(),
                    mounted: v.mount.is_some(),
                    locked: v.locked,
                    index: self.volumes.len(),
                    drive: drive.path.clone(),
                };
                self.volumes.push(vol.clone());
                volumes.push(vol);
            }
            self.drives.push(Drv {
                path: drive.path.clone(),
                name: drive.name.clone(),
                detail: drive.summary(),
                volumes,
            });
        }
    }

    /// Room for each drive with its volumes (two rows or more).
    pub fn listing(card: Card) -> bool {
        card.h >= 170.0
    }

    /// One drive with one volume you can use: the compact view mounts or opens it too.
    fn single(&self) -> Option<&Vol> {
        match self.drives.as_slice() {
            [d] => match d.volumes.as_slice() {
                [v] if !v.locked => Some(v),
                _ => None,
            },
            _ => None,
        }
    }

    fn empty_text(&self) -> &'static str {
        if self.available {
            "Plug in a USB drive or SD card"
        } else {
            "UDisks isn't running"
        }
    }

    /// What the keys need to know, at the card's size.
    fn keys(&self, card: Card) -> Keys {
        Keys {
            list: Self::listing(card),
            volumes: self.volumes.clone(),
            drives: self.drives.iter().map(|d| d.path.clone()).collect(),
            single: self.single().cloned(),
            out: Vec::new(),
        }
    }

    /// The volumes or the card's size changed: the cursor stays on a stop that exists, and not on
    /// a locked volume.
    pub fn sync(&mut self, card: Card) {
        let keys = self.keys(card);
        let n = if keys.list { self.volumes.len() } else { 0 };
        let mut scope = self.scope;
        scope.set_count(n, &keys);
        scope.settle(&keys);
        self.scope = scope;
    }
}

/// The keys: j/k over the volumes, Enter opens (or mounts) one, u unmounts it, x ejects its
/// drive, X ejects all.
struct Keys {
    list: bool,
    volumes: Vec<Vol>,
    drives: Vec<String>,
    single: Option<Vol>,
    out: Vec<Do>,
}

impl Keys {
    fn eject_one(&mut self) {
        match self.drives.as_slice() {
            [] => {}
            [one] => self.out.push(Do::Eject(one.clone())),
            _ => self.out.push(Do::EjectAll),
        }
    }
}

impl Stops for Keys {
    fn skip(&self, stop: usize) -> bool {
        self.list && self.volumes.get(stop).is_some_and(|v| v.locked)
    }

    fn activate(&mut self, cursor: usize, _sub: usize) {
        let v = if self.list {
            self.volumes.get(cursor)
        } else {
            self.single.as_ref()
        };
        let Some(v) = v else { return };
        if v.mounted {
            self.out.push(Do::Open(v.mount.clone()));
        } else if !v.locked && !v.path.is_empty() {
            self.out.push(Do::Mount(v.path.clone()));
        }
    }

    fn letter(&mut self, key: &Key, cursor: usize) -> bool {
        let at = self.volumes.get(cursor).filter(|_| self.list).cloned();
        if key.is('u')
            && let Some(v) = &at
            && v.mounted
        {
            self.out.push(Do::Unmount(v.path.clone()));
            return true;
        }
        if key.is('x') {
            match &at {
                Some(v) => self.out.push(Do::Eject(v.drive.clone())),
                None => self.eject_one(),
            }
            return true;
        }
        if key.is('X') && !self.drives.is_empty() {
            self.out.push(Do::EjectAll);
            return true;
        }
        false
    }
}

pub fn run(d: &mut Dash, what: Do, cx: &mut Context<Dash>) {
    match what {
        Do::Mount(block) => d.tx.drives.tell(drives::Command::Mount(block)),
        Do::Unmount(block) => d.tx.drives.tell(drives::Command::Unmount(block)),
        Do::Eject(drive) => d.tx.drives.tell(drives::Command::Eject(drive)),
        Do::EjectAll => d.tx.drives.tell(drives::Command::EjectAll),
        Do::Open(mount) => {
            d.tx.drives.tell(drives::Command::Open(mount));
            // The file manager would be behind the dashboard.
            cx.emit(DashEvent::Hide);
        }
    }
    cx.notify();
}

pub fn show(d: &mut Dash, s: drives::State) {
    d.widgets.devices.drives.set(s);
    super::resync(d);
}

/// A key for the entered widget. True if it was taken.
pub fn key(d: &mut Dash, id: u32, key: &Key, cx: &mut Context<Dash>) -> bool {
    let card = d.widgets.devices.seen.of(id);
    let st = &mut d.widgets.devices.drives;
    st.sync(card);
    let mut keys = st.keys(card);
    let mut scope = st.scope;
    let taken = scope.press(key, &mut keys);
    st.scope = scope;
    for what in keys.out {
        run(d, what, cx);
    }
    cx.notify();
    taken
}

pub fn describe(d: &Dash) -> Option<String> {
    let st = &d.widgets.devices.drives;
    match st.volumes.get(st.scope.cursor) {
        Some(v) if st.scope.count > 0 => Some(format!(
            "volume {}/{}: {}",
            st.scope.cursor + 1,
            st.scope.count,
            v.name
        )),
        _ => Some("no stops".into()),
    }
}

pub fn check(d: &Dash) -> Result<(), String> {
    let st = &d.widgets.devices.drives;
    if st.scope.cursor >= st.scope.count.max(1) {
        return Err(format!(
            "the cursor is on stop {} of {}",
            st.scope.cursor, st.scope.count
        ));
    }
    if st.scope.count > st.volumes.len() {
        return Err(format!(
            "{} stops, but {} volumes",
            st.scope.count,
            st.volumes.len()
        ));
    }
    if st.scope.count > 0
        && st.volumes[st.scope.cursor].locked
        && st.volumes.iter().any(|v| !v.locked)
    {
        return Err("the cursor is on a locked volume".into());
    }
    Ok(())
}

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let (d, tk) = (ctx.d, ctx.tk);
    let st = &d.widgets.devices.drives;
    let card = Card::of(ctx);
    if State::listing(card) {
        listing(ctx, st, cx)
    } else {
        compact(ctx, st, tk, cx)
    }
}

fn do_on_click(
    what: Do,
    cx: &mut Context<Dash>,
) -> impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static {
    cx.listener(move |d, _, _, cx| run(d, what.clone(), cx))
}

fn compact(ctx: &Ctx, st: &State, tk: &Tk, cx: &mut Context<Dash>) -> AnyElement {
    let count = st.drives.len();
    let title: SharedString = match count {
        0 => "No drives".into(),
        1 => st.drives[0].name.clone().into(),
        n => format!("{n} drives").into(),
    };
    let sub: SharedString = match count {
        0 => st.empty_text().into(),
        1 => st.drives[0].detail.clone().into(),
        _ => "Eject all to remove them".into(),
    };
    let single = st.single().filter(|_| ctx.w >= 280.0);
    div()
        .size_full()
        .p(px(tk.pad()))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.0))
        .child(icon(
            "usb",
            22.0,
            if count > 0 { tk.text() } else { tk.faint() },
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .justify_center()
                .gap(px(2.0))
                .child(
                    parts::elide(div())
                        .text_size(px(tk.font_body()))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(tk.text())
                        .child(title),
                )
                .child(muted(sub, tk)),
        )
        .when_some(single, |r, v| {
            if v.mounted {
                r.child(icon_button(
                    "drives-open",
                    "folder",
                    "Open folder",
                    32.0,
                    true,
                    tk,
                    do_on_click(Do::Open(v.mount.clone()), cx),
                ))
            } else {
                r.child(text_button(
                    "drives-mount",
                    "Mount",
                    "Mount",
                    None,
                    false,
                    true,
                    false,
                    tk,
                    do_on_click(Do::Mount(v.path.clone()), cx),
                ))
            }
        })
        .when(count > 0, |r| {
            let (label, what) = if count == 1 {
                ("Eject", Do::Eject(st.drives[0].path.clone()))
            } else {
                ("Eject all", Do::EjectAll)
            };
            r.child(icon_button(
                "drives-eject",
                "eject",
                label,
                32.0,
                true,
                tk,
                do_on_click(what, cx),
            ))
        })
        .into_any_element()
}

fn listing(ctx: &Ctx, st: &State, cx: &mut Context<Dash>) -> AnyElement {
    let tk = ctx.tk;
    let count = st.drives.len();
    let focus = ctx.entered.then_some(st.scope.cursor);
    // The scroll list's children, flat: a drive's name, then its volumes; and where each
    // volume's row is among them.
    let mut rows: Vec<AnyElement> = Vec::new();
    let mut child_of = vec![0; st.volumes.len()];
    for (di, drive) in st.drives.iter().enumerate() {
        rows.push(
            div()
                .flex_none()
                .w_full()
                .h(px(30.0))
                .when(drive.volumes.is_empty(), |h| h.mb(px(6.0)))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.0))
                .child(
                    parts::elide(div())
                        .flex_1()
                        .text_size(px(tk.font_body()))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(tk.text())
                        .child(drive.name.clone()),
                )
                .child(icon_button(
                    ("drive-eject", di),
                    "eject",
                    format!("Eject {}", drive.name),
                    28.0,
                    true,
                    tk,
                    do_on_click(Do::Eject(drive.path.clone()), cx),
                ))
                .into_any_element(),
        );
        for (vi, v) in drive.volumes.iter().enumerate() {
            child_of[v.index] = rows.len();
            let last = vi + 1 == drive.volumes.len();
            let what = if v.mounted {
                Do::Open(v.mount.clone())
            } else {
                Do::Mount(v.path.clone())
            };
            let value = if v.locked {
                "locked"
            } else if v.mounted {
                "mounted"
            } else {
                "not mounted"
            };
            let on_click = do_on_click(what, cx);
            rows.push(
                row(
                    ("volume", v.index),
                    v.name.clone(),
                    Some(value.into()),
                    40.0,
                    !v.locked,
                    focus == Some(v.index),
                    tk,
                    Some(Box::new(on_click)),
                )
                .when(last, |r| r.mb(px(6.0)))
                .child(
                    div()
                        .size_full()
                        .pl(px(8.0))
                        .pr(px(4.0))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.0))
                        .child(icon(
                            if v.locked { "lock-small" } else { "folder" },
                            16.0,
                            if v.mounted { tk.accent() } else { tk.dim() },
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .justify_center()
                                .child(
                                    parts::elide(div())
                                        .text_size(px(tk.font_body()))
                                        .text_color(tk.text())
                                        .child(v.name.clone()),
                                )
                                .child(muted(v.detail.clone(), tk)),
                        )
                        .when(v.mounted && ctx.w >= 240.0, |r| {
                            r.child(text_button(
                                ("volume-unmount", v.index),
                                "Unmount",
                                format!("Unmount {}", v.name),
                                None,
                                false,
                                true,
                                false,
                                tk,
                                do_on_click(Do::Unmount(v.path.clone()), cx),
                            ))
                        }),
                )
                .into_any_element(),
            );
        }
    }
    st.revealed.show(
        &st.list,
        focus.filter(|_| count > 0).map(|c| (c, st.volumes.len())),
        focus.and_then(|c| child_of.get(c).copied()).unwrap_or(0),
    );
    let body = if count == 0 {
        parts::empty(st.empty_text(), tk).into_any_element()
    } else {
        parts::scroll_list("drives-list", &st.list, rows, tk)
            .debug_selector(|| "drives list".into())
            .into_any_element()
    };
    let right = if count > 1 {
        vec![
            icon_button(
                "drives-eject-all",
                "eject",
                "Eject all",
                28.0,
                true,
                tk,
                do_on_click(Do::EjectAll, cx),
            )
            .into_any_element(),
        ]
    } else {
        Vec::new()
    };
    div()
        .size_full()
        .p(px(tk.pad()))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(parts::header("usb", "Removable drives", tk, right))
        .child(body)
        .into_any_element()
}

//! The clipboard history: what the system reports, putting an entry back, and the widget
//! (`ClipboardWidget` in `slint/ui/widgets/tools.slint`, `slint/src/app/clipboard.rs`).
//!
//! The history is opt-in: it records only with a clipboard widget in the layout (a hidden one
//! counts, so that a smaller screen doesn't wipe it), or as the standalone clipboard app. Without
//! one nothing copied is read, and what was kept is forgotten. Secrets (a password manager's
//! hint) never reach it; the system leaves them out.

use super::Plain;
use super::ui;
use crate::dash::Dash;
use crate::keys::Key;
use crate::kit;
use crate::layout::Kind;
use crate::scope::{Scope, Stops};
use crate::widgets::Ctx;
use chrono::{DateTime, Local};
use gpui::prelude::*;
use gpui::{AnyElement, Context, Role, ScrollHandle, div, img, px};
use niri_dash_system::Tell;
use niri_dash_system::clipboard::{self, Clip, ClipKind, Command};
use std::sync::Arc;

/// Largest side of a clipboard thumbnail in pixels; the list shows them at most ~100 px (logical)
/// wide, so this covers scale 2.
const THUMB: u32 = 240;

/// A history entry, with what the widget shows for it.
pub struct Entry {
    pub clip: Clip,
    pub text: String,
    /// The image's full size, and a thumbnail: a screenshot decoded at full size is 16 MB, and
    /// the list only shows it about 100 px wide. The PNG itself stays in `clip` for pasting.
    image: Option<(u32, u32, Arc<gpui::RenderImage>)>,
    time: DateTime<Local>,
}

impl Entry {
    /// The line under the text: when, and how big.
    fn info(&self, us: bool) -> String {
        let when = when_text(self.time, us);
        match &self.image {
            Some((w, h, _)) => format!("{when} · image {w}×{h}"),
            None => {
                let lines = String::from_utf8_lossy(&self.clip.data).lines().count();
                if lines > 1 {
                    format!("{when} · {lines} lines")
                } else {
                    when
                }
            }
        }
    }
}

#[derive(Default)]
pub struct State {
    /// Newest first.
    pub entries: Vec<Entry>,
    /// Whether what gets copied is being kept; see [`sync_history`].
    pub recording: bool,
    list: ScrollHandle,
}

/// A time of day as the locale writes it: "9:41 AM" in the US, "9:41" elsewhere.
fn clock_text(t: DateTime<Local>, us: bool) -> String {
    if us {
        t.format("%-I:%M %p").to_string()
    } else {
        t.format("%-H:%M").to_string()
    }
}

/// When something happened: the time of day, with the date before it if that isn't today.
fn when_text(t: DateTime<Local>, us: bool) -> String {
    let time = clock_text(t, us);
    if t.date_naive() == Local::now().date_naive() {
        time
    } else if us {
        format!("{} {time}", t.format("%-m/%-d"))
    } else {
        format!("{} {time}", t.format("%d.%m."))
    }
}

/// An image of RGBA pixels, scaled down to fit `max` on its longer side, as GPUI draws it (BGRA).
fn thumbnail(w: u32, h: u32, rgba: Vec<u8>, max: u32) -> Option<Arc<gpui::RenderImage>> {
    let (w, h, mut px) = clipboard::shrink(w, h, rgba, max);
    for p in px.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    let buf = image::RgbaImage::from_raw(w, h, px)?;
    Some(Arc::new(gpui::RenderImage::new(vec![image::Frame::new(
        buf,
    )])))
}

// ---- what the system reports -----------------------------------------------------------------

/// Clipboard history is opt-in: record only with a clipboard widget in the layout.
pub fn sync_history(d: &mut Dash) {
    let want = match d.solo {
        Some(kind) => kind == Kind::Clipboard,
        None => d.layout.widgets.iter().any(|w| w.kind == Kind::Clipboard),
    };
    let c = &mut d.widgets.tools.clipboard;
    if std::mem::replace(&mut c.recording, want) == want {
        return;
    }
    d.tx.clipboard.tell(Command::Record(want));
    if !want {
        d.widgets.tools.clipboard.entries.clear();
        resync(d);
    }
}

/// Something was copied.
pub fn add(d: &mut Dash, clip: Clip) {
    let c = &mut d.widgets.tools.clipboard;
    // A copy still being read when recording stopped.
    if !c.recording {
        return;
    }
    if let Some(pos) = c.entries.iter().position(|e| e.clip.data == clip.data) {
        let mut e = c.entries.remove(pos);
        e.time = Local::now();
        c.entries.insert(0, e);
    } else {
        let (text, image) = if clip.kind == ClipKind::Text {
            let t = clipboard::preview(&String::from_utf8_lossy(&clip.data));
            if t.is_empty() {
                return;
            }
            (t, None)
        } else {
            let Some((w, h, rgba)) = clipboard::decode_png(&clip.data) else {
                return;
            };
            let Some(thumb) = thumbnail(w, h, rgba, THUMB) else {
                return;
            };
            ("Image".to_string(), Some((w, h, thumb)))
        };
        c.entries.insert(
            0,
            Entry {
                clip,
                text,
                image,
                time: Local::now(),
            },
        );
        c.entries.truncate(clipboard::MAX_ENTRIES);
    }
    resync(d);
}

/// Put `text` on the clipboard, and say so in a toast.
pub fn copy_text(d: &mut Dash, text: &str) {
    copy_text_as(d, text, &format!("Copied {text}"));
}

/// Put `text` on the clipboard and say `toast`, for text too long or odd to quote.
pub fn copy_text_as(d: &mut Dash, text: &str, toast: &str) {
    let clip = Clip {
        kind: ClipKind::Text,
        data: Arc::new(text.as_bytes().to_vec()),
    };
    copy(d, clip, toast);
}

/// Put `clip` on the clipboard and say `toast`.
fn copy(d: &mut Dash, clip: Clip, toast: &str) {
    d.tx.clipboard.tell(Command::Set(clip));
    d.toast(toast);
}

/// Put entry `i` back on the clipboard.
fn copy_entry(d: &mut Dash, i: usize) {
    if let Some(clip) = d
        .widgets
        .tools
        .clipboard
        .entries
        .get(i)
        .map(|e| e.clip.clone())
    {
        copy(d, clip, "Copied");
    }
}

fn remove(d: &mut Dash, i: usize) {
    let c = &mut d.widgets.tools.clipboard;
    if i < c.entries.len() {
        c.entries.remove(i);
    }
    resync(d);
}

fn clear(d: &mut Dash) {
    d.widgets.tools.clipboard.entries.clear();
    resync(d);
}

/// The history changed: every widget's cursor stays on an entry that exists.
fn resync(d: &mut Dash) {
    let count = d.widgets.tools.clipboard.entries.len();
    for p in super::placed_of(d, Kind::Clipboard) {
        let mut inst = d.widgets.tools.insts.get(p.id);
        inst.scope
            .set_count(if inst.round { 0 } else { count }, &Plain);
        d.widgets.tools.insts.set(p.id, inst);
    }
}

// ---- keys -----------------------------------------------------------------------------------

struct Keys<'a>(&'a mut Dash);

impl Stops for Keys<'_> {
    fn activate(&mut self, cursor: usize, sub: usize) {
        if cursor < self.0.widgets.tools.clipboard.entries.len() {
            if sub == 1 {
                remove(self.0, cursor);
            } else {
                copy_entry(self.0, cursor);
            }
        }
    }

    fn letter(&mut self, key: &Key, cursor: usize) -> bool {
        if key.is('d') && cursor < self.0.widgets.tools.clipboard.entries.len() {
            remove(self.0, cursor);
            return true;
        }
        if key.is('D') {
            clear(self.0);
            return true;
        }
        false
    }
}

/// j/k over the entries, Enter copies (l: the remove button), d removes, D clears all.
pub fn key(d: &mut Dash, id: u32, key: &Key) -> bool {
    let mut inst = d.widgets.tools.insts.get(id);
    let count = if inst.round {
        0
    } else {
        d.widgets.tools.clipboard.entries.len()
    };
    let mut scope: Scope = inst.scope;
    scope.set_count(count, &Plain);
    scope.set_subs(2);
    let taken = scope.press(key, &mut Keys(d));
    let count = if inst.round {
        0
    } else {
        d.widgets.tools.clipboard.entries.len()
    };
    scope.set_count(count, &Plain);
    inst.scope = scope;
    d.widgets.tools.insts.set(id, inst);
    d.widgets.tools.clipboard.list.scroll_to_item(scope.cursor);
    taken
}

pub fn describe(d: &Dash, id: u32) -> Option<String> {
    let c = &d.widgets.tools.clipboard;
    let s = d.widgets.tools.insts.get(id).scope;
    Some(if c.entries.is_empty() {
        "no entries".to_string()
    } else {
        format!(
            "entry {} of {}{}",
            s.cursor + 1,
            c.entries.len(),
            if s.part() == 1 { ", remove" } else { "" }
        )
    })
}

pub fn check(d: &Dash) -> Result<(), String> {
    let c = &d.widgets.tools.clipboard;
    let want = match d.solo {
        Some(kind) => kind == Kind::Clipboard,
        None => d.layout.widgets.iter().any(|w| w.kind == Kind::Clipboard),
    };
    if c.recording != want {
        return Err(format!(
            "clipboard recording is {}, the layout wants {want}",
            c.recording
        ));
    }
    if !c.recording && !c.entries.is_empty() {
        return Err("clipboard history kept without a clipboard widget".into());
    }
    if c.entries.len() > clipboard::MAX_ENTRIES {
        return Err(format!("{} clipboard entries kept", c.entries.len()));
    }
    for p in super::placed_of(d, Kind::Clipboard) {
        let inst = d.widgets.tools.insts.get(p.id);
        let count = if inst.round { 0 } else { c.entries.len() };
        super::cursor_ok("clipboard", d, p.id, count, 2)?;
    }
    Ok(())
}

// ---- drawing --------------------------------------------------------------------------------

pub fn render(ctx: &Ctx, cx: &mut Context<Dash>) -> AnyElement {
    let d = ctx.d;
    let tk = ctx.tk;
    let c = &d.widgets.tools.clipboard;
    let id = ctx.tile.id();
    let n = c.entries.len();
    d.widgets.tools.insts.drawn(
        id,
        ctx.w,
        ctx.h,
        ctx.round,
        if ctx.round { 0 } else { n },
        2,
    );
    if ctx.round {
        return ui::round_face(
            "clipboard",
            if n > 0 { tk.accent() } else { tk.faint() },
            format!("{n} items"),
            ctx.w,
            tk,
        );
    }
    let scope = d.widgets.tools.insts.get(id).scope;
    let us = d.locale.us;
    let body: AnyElement = if !d.caps.clipboard {
        kit::empty("The compositor doesn't offer clipboard access", tk)
    } else if n == 0 {
        kit::empty("Nothing copied yet", tk)
    } else {
        let rows = c
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let height = if e.image.is_some() { 72.0 } else { 44.0 };
                let info = e.info(us);
                let value = if i == 0 {
                    format!("current, {info}")
                } else {
                    info.clone()
                };
                let on_row = ctx.entered && scope.cursor == i;
                let row = ui::row(
                    ("clip", i),
                    e.text.clone(),
                    Some(value.into()),
                    i == 0,
                    on_row && scope.part() == 0,
                    height - 3.0,
                    tk,
                )
                .on_click(cx.listener(move |d, _, _, cx| {
                    copy_entry(d, i);
                    cx.notify();
                }))
                .pl(px(10.0))
                .pr(px(4.0))
                .gap(px(10.0))
                .when_some(e.image.as_ref(), |row, (_, _, thumb)| {
                    row.child(
                        img(thumb.clone())
                            .flex_none()
                            .h(px(height - 3.0 - 8.0 - 4.0))
                            .w(px((height - 3.0 - 8.0 - 4.0) * 1.5))
                            .object_fit(gpui::ObjectFit::Contain),
                    )
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .child(
                            div()
                                .text_size(px(tk.font_body()))
                                .text_color(tk.text())
                                .truncate()
                                .child(e.text.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(tk.font_small()))
                                .text_color(tk.dim())
                                .truncate()
                                .child(info),
                        ),
                )
                .child(ui::icon_button(
                    ("clip-remove", i),
                    "close",
                    "Remove from clipboard history",
                    26.0,
                    tk.faint(),
                    None,
                    on_row && scope.part() == 1,
                    tk,
                    cx.listener(move |d, _, _, cx| {
                        remove(d, i);
                        cx.stop_propagation();
                        cx.notify();
                    }),
                ));
                // The row sits in a cell a few pixels taller than itself, as in the Slint list.
                div()
                    .w_full()
                    .h(px(height))
                    .flex_none()
                    .child(row)
                    .into_any_element()
            })
            .collect();
        ui::scroll_list("clip-list", &c.list, rows, tk).into_any_element()
    };
    div()
        .id(("clipboard", id))
        .role(Role::List)
        .aria_label("Clipboard")
        .size_full()
        .flex()
        .flex_col()
        .p(px(tk.pad()))
        .gap(px(6.0))
        .child(
            // As tall as the header, whatever the button's height: in Slint the button hangs
            // from the top of the header.
            div()
                .flex_none()
                .h(px(28.0))
                .flex()
                .flex_row()
                .items_start()
                .child(ui::header("clipboard", "Clipboard", tk).flex_1())
                .when(n > 0, |h| {
                    h.child(ui::text_button(
                        "clip-clear",
                        "Clear",
                        "Clear clipboard history",
                        false,
                        tk,
                        cx.listener(|d, _, _, cx| {
                            clear(d);
                            cx.notify();
                        }),
                    ))
                }),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .w_full()
                .flex()
                .flex_col()
                .child(body),
        )
        .into_any_element()
}

//! The clipboard history: what the shell reports, and putting an entry back.

use super::clock::when_text;
use super::{App, model, rgba_image};
use crate::shell::Shell;
use crate::sources::clipboard::{self, Clip};
use crate::{ClipItem, Clipboard as UiClipboard, Dash};
use chrono::Local;
use slint::{ComponentHandle, Image};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Default)]
pub(super) struct History {
    entries: RefCell<Vec<ClipEntry>>,
    /// An entry to put back on the clipboard (needs the Wayland side).
    pending: RefCell<Option<Clip>>,
}

/// Largest side of a clipboard thumbnail in pixels; the list shows them at most ~100 px (logical)
/// wide, so this covers scale 2.
const CLIP_THUMB: u32 = 240;

struct ClipEntry {
    clip: Clip,
    text: String,
    /// The image's full size, and a thumbnail: a screenshot decoded at full size is 16 MB, and
    /// the list only shows it about 100 px wide. The PNG itself stays in `clip` for pasting.
    image: Option<(u32, u32, Image)>,
    time: chrono::DateTime<Local>,
}

impl App {
    /// Called every loop iteration: hand a copied entry to the shell.
    pub(super) fn push_pending_clip(&self, shell: &mut Shell) {
        if let Some(clip) = self.clipboard.pending.borrow_mut().take() {
            shell.set_clipboard(clip);
        }
    }

    fn clip_items(&self) {
        let us = self.locale.us;
        let items: Vec<ClipItem> = self
            .clipboard
            .entries
            .borrow()
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let when = when_text(c.time, us);
                let image = c.image.as_ref().map(|(_, _, img)| img.clone());
                let info = match &c.image {
                    Some((w, h, _)) => format!("{when} · image {w}×{h}"),
                    None => {
                        let lines = String::from_utf8_lossy(&c.clip.data).lines().count();
                        if lines > 1 {
                            format!("{when} · {lines} lines")
                        } else {
                            when
                        }
                    }
                };
                ClipItem {
                    index: i as i32,
                    text: c.text.as_str().into(),
                    is_image: image.is_some(),
                    image: image.unwrap_or_default(),
                    info: info.into(),
                }
            })
            .collect();
        self.ui().global::<UiClipboard>().set_items(model(items));
    }

    pub(super) fn add_clip(&self, clip: Clip) {
        let mut clips = self.clipboard.entries.borrow_mut();
        if let Some(pos) = clips.iter().position(|c| c.clip.data == clip.data) {
            let mut e = clips.remove(pos);
            e.time = Local::now();
            clips.insert(0, e);
        } else {
            let (text, image) = if clip.kind == clipboard::ClipKind::Text {
                let t = clipboard::preview(&String::from_utf8_lossy(&clip.data));
                if t.is_empty() {
                    return;
                }
                (t, None)
            } else {
                let Some((w, h, rgba)) = clipboard::decode_png(&clip.data) else {
                    return;
                };
                let thumb = rgba_image(w, h, rgba, CLIP_THUMB);
                (String::from("Image"), Some((w, h, thumb)))
            };
            clips.insert(
                0,
                ClipEntry {
                    clip,
                    text,
                    image,
                    time: Local::now(),
                },
            );
            clips.truncate(clipboard::MAX_ENTRIES);
        }
        drop(clips);
        self.clip_items();
    }

    /// Put `text` on the clipboard.
    pub(super) fn copy_text(&self, text: &str) {
        let clip = Clip {
            kind: clipboard::ClipKind::Text,
            data: Arc::new(text.as_bytes().to_vec()),
        };
        self.copy(clip, &format!("Copied {text}"));
    }

    /// Put `clip` on the clipboard (once the event loop gets to it) and say so in a toast.
    fn copy(&self, clip: Clip, toast: &str) {
        *self.clipboard.pending.borrow_mut() = Some(clip);
        self.toast(toast);
    }
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    let cb = ui.global::<UiClipboard>();
    let a = app.clone();
    cb.on_copy(move |i| {
        let clip = a
            .clipboard
            .entries
            .borrow()
            .get(i as usize)
            .map(|c| c.clip.clone());
        if let Some(clip) = clip {
            a.copy(clip, "Copied");
        }
    });
    let a = app.clone();
    cb.on_remove(move |i| {
        let mut clips = a.clipboard.entries.borrow_mut();
        if (i as usize) < clips.len() {
            clips.remove(i as usize);
        }
        drop(clips);
        a.clip_items();
    });
    let a = app.clone();
    cb.on_clear(move || {
        a.clipboard.entries.borrow_mut().clear();
        a.clip_items();
    });
}

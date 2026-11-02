//! The clipboard history: what the shell reports, and putting an entry back.

use super::{App, model};
use crate::sources::clipboard::{self, Clip};
use crate::{ClipItem, Clipboard as UiClipboard, Dash};
use chrono::Local;
use slint::{ComponentHandle, Image, Rgba8Pixel, SharedPixelBuffer};
use std::rc::Rc;
use std::sync::Arc;

/// Largest side of a clipboard thumbnail in pixels; the list shows them at most ~100 px (logical)
/// wide, so this covers scale 2.
const CLIP_THUMB: u32 = 240;

pub(super) struct ClipEntry {
    clip: Clip,
    text: String,
    /// The image's full size, and a thumbnail: a screenshot decoded at full size is 16 MB, and
    /// the list only shows it about 100 px wide. The PNG itself stays in `clip` for pasting.
    image: Option<(u32, u32, Image)>,
    time: chrono::DateTime<Local>,
}

impl App {
    fn clip_items(&self) {
        let today = Local::now().date_naive();
        let items: Vec<ClipItem> = self
            .clips
            .borrow()
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let when = if c.time.date_naive() == today {
                    c.time.format("%H:%M")
                } else {
                    c.time.format("%d.%m. %H:%M")
                };
                let image = c.image.as_ref().map(|(_, _, img)| img.clone());
                let info = match &c.image {
                    Some((w, h, _)) => format!("{when} · image {w}×{h}"),
                    None => {
                        let lines = String::from_utf8_lossy(&c.clip.data).lines().count();
                        if lines > 1 {
                            format!("{when} · {lines} lines")
                        } else {
                            when.to_string()
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
        let mut clips = self.clips.borrow_mut();
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
                let (tw, th, small) = clipboard::shrink(w, h, rgba, CLIP_THUMB);
                let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&small, tw, th);
                (String::from("Image"), Some((w, h, Image::from_rgba8(buf))))
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

    pub(super) fn copy_text(&self, text: &str) {
        *self.pending_clip.borrow_mut() = Some(clipboard::Clip {
            kind: clipboard::ClipKind::Text,
            data: Arc::new(text.as_bytes().to_vec()),
        });
        self.toast(&format!("Copied {text}"));
    }
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    let cb = ui.global::<UiClipboard>();
    let a = app.clone();
    cb.on_copy(move |i| {
        let clip = a.clips.borrow().get(i as usize).map(|c| c.clip.clone());
        if let Some(clip) = clip {
            *a.pending_clip.borrow_mut() = Some(clip);
            a.toast("Copied");
        }
    });
    let a = app.clone();
    cb.on_remove(move |i| {
        let mut clips = a.clips.borrow_mut();
        if (i as usize) < clips.len() {
            clips.remove(i as usize);
        }
        drop(clips);
        a.clip_items();
    });
    let a = app.clone();
    cb.on_clear(move || {
        a.clips.borrow_mut().clear();
        a.clip_items();
    });
}

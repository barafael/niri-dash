//! Clipboard history. The Wayland side (wlr-data-control) lives in `shell.rs`; this holds the
//! entries. History stays in memory only and skips anything a password manager marks secret.

use std::sync::Arc;

pub const MAX_ENTRIES: usize = 50;
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
pub const TEXT_MIMES: [&str; 5] = [
    "text/plain;charset=utf-8",
    "text/plain",
    "UTF8_STRING",
    "STRING",
    "TEXT",
];
pub const IMAGE_MIMES: [&str; 1] = ["image/png"];
/// Set by KeePassXC, KWallet and friends on secrets.
pub const SECRET_HINT: &str = "x-kde-passwordManagerHint";

/// What kind of content a clipboard entry holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipKind {
    Text,
    Png,
}

impl ClipKind {
    /// The mime types to offer when putting an entry of this kind back on the clipboard.
    pub fn mimes(self) -> &'static [&'static str] {
        match self {
            ClipKind::Text => &TEXT_MIMES,
            ClipKind::Png => &IMAGE_MIMES,
        }
    }
}

/// What arrived on the clipboard.
#[derive(Debug, Clone)]
pub struct Clip {
    pub kind: ClipKind,
    pub data: Arc<Vec<u8>>,
}

/// Which offered mime type to read, and as what; None for secrets and unsupported content.
pub fn pick(offered: &[String]) -> Option<(ClipKind, &'static str)> {
    if offered.iter().any(|m| m == SECRET_HINT) {
        return None;
    }
    [ClipKind::Text, ClipKind::Png]
        .into_iter()
        .flat_map(|k| k.mimes().iter().map(move |m| (k, *m)))
        .find(|(_, m)| offered.iter().any(|o| o == m))
}

/// Decode a PNG into RGBA8 for a thumbnail.
pub fn decode_png(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(data));
    dec.set_transformations(
        png::Transformations::normalize_to_color8() | png::Transformations::ALPHA,
    );
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::GrayscaleAlpha => buf
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Rgb => buf
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some((info.width, info.height, rgba))
}

/// Shrink RGBA pixels to fit in `max`×`max` by averaging boxes of source pixels. Images that
/// already fit are returned as they are.
pub fn shrink(w: u32, h: u32, rgba: Vec<u8>, max: u32) -> (u32, u32, Vec<u8>) {
    if w <= max && h <= max {
        return (w, h, rgba);
    }
    let f = (w as f64 / max as f64).max(h as f64 / max as f64);
    let (tw, th) = (
        ((w as f64 / f).round() as u32).max(1),
        ((h as f64 / f).round() as u32).max(1),
    );
    let mut out = Vec::with_capacity((tw * th * 4) as usize);
    for ty in 0..th {
        let (y0, y1) = (ty * h / th, ((ty + 1) * h / th).max(ty * h / th + 1));
        for tx in 0..tw {
            let (x0, x1) = (tx * w / tw, ((tx + 1) * w / tw).max(tx * w / tw + 1));
            let mut sum = [0u32; 4];
            for y in y0..y1 {
                let row = &rgba[((y * w + x0) * 4) as usize..((y * w + x1) * 4) as usize];
                for p in row.as_chunks::<4>().0 {
                    for c in 0..4 {
                        sum[c] += p[c] as u32;
                    }
                }
            }
            let n = (y1 - y0) * (x1 - x0);
            out.extend(sum.map(|s| (s / n) as u8));
        }
    }
    (tw, th, out)
}

/// One-line preview of text: whitespace collapsed, cut at a sensible length.
pub fn preview(text: &str) -> String {
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 300 {
        collapsed.chars().take(300).collect::<String>() + "…"
    } else {
        collapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrink_keeps_aspect_and_averages() {
        // 4×2, left half black, right half white.
        let px = |v: u8| [v, v, v, 255];
        let rgba: Vec<u8> = [px(0), px(0), px(255), px(255)].repeat(2).concat();
        let (w, h, out) = shrink(4, 2, rgba, 2);
        assert_eq!((w, h), (2, 1));
        assert_eq!(out, [px(0), px(255)].concat());
        let (w, h, out) = shrink(1, 1, px(7).to_vec(), 2);
        assert_eq!((w, h, out), (1, 1, px(7).to_vec()));
    }

    #[test]
    fn mime_choice() {
        let m = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            pick(&m(&[
                "text/html",
                "UTF8_STRING",
                "text/plain;charset=utf-8"
            ])),
            Some((ClipKind::Text, "text/plain;charset=utf-8"))
        );
        assert_eq!(
            pick(&m(&["image/png", "text/uri-list"])),
            Some((ClipKind::Png, "image/png"))
        );
        assert_eq!(pick(&m(&["text/plain", SECRET_HINT])), None);
        assert_eq!(pick(&m(&["application/x-foo"])), None);
    }

    #[test]
    fn previews() {
        assert_eq!(preview("  a\n\n b\tc "), "a b c");
    }
}

//! Pictures: window and tray icons, album art, notification images. They arrive as a path or as
//! pixels (`IconSrc`) and are turned into something GPUI draws once, when the event comes, not at
//! every frame. Pictures by path are kept in a small least-recently-used cache (art and
//! notification images come and go with every track and screenshot).

use gpui::prelude::*;
use gpui::{AnyElement, ObjectFit, RenderImage, StyledImage, div, img, px};
use image::{Frame, RgbaImage};
use niri_dash_system::IconSrc;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// How many pictures the cache keeps by path.
const CACHE: usize = 96;
/// The longest side of a raster picture kept for display. Album art in a tall media widget is
/// the largest (a few hundred logical px); a full-size cover or screenshot is megabytes.
const MAX: u32 = 512;

/// A picture ready to draw.
#[derive(Clone)]
pub enum Pic {
    /// Decoded pixels.
    Raster(Arc<RenderImage>),
    /// A vector image, which GPUI loads and draws itself.
    Vector(PathBuf),
}

impl PartialEq for Pic {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Pic::Raster(a), Pic::Raster(b)) => Arc::ptr_eq(a, b),
            (Pic::Vector(a), Pic::Vector(b)) => a == b,
            _ => false,
        }
    }
}

impl std::fmt::Debug for Pic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Pic::Raster(_) => f.write_str("Pic::Raster"),
            Pic::Vector(p) => write!(f, "Pic::Vector({})", p.display()),
        }
    }
}

impl Pic {
    /// A `size` by `size` square showing the picture, the whole of it (`cover`: filling the
    /// square and cropping what sticks out).
    pub fn square(&self, size: f32, cover: bool) -> AnyElement {
        self.boxed(size, size, cover)
    }

    /// A `width` by `height` box showing the picture.
    pub fn boxed(&self, width: f32, height: f32, cover: bool) -> AnyElement {
        let fit = if cover {
            ObjectFit::Cover
        } else {
            ObjectFit::Contain
        };
        let image = match self {
            Pic::Raster(r) => img(r.clone()),
            Pic::Vector(p) => img(p.clone()),
        };
        div()
            .flex_none()
            .w(px(width))
            .h(px(height))
            .overflow_hidden()
            .child(image.size_full().object_fit(fit))
            .into_any_element()
    }
}

/// Pixels (RGBA) as a picture, shrunk to fit `MAX` on the longer side.
fn raster(width: u32, height: u32, rgba: Vec<u8>) -> Option<Pic> {
    if width == 0 || height == 0 || rgba.len() < (width as usize) * (height as usize) * 4 {
        return None;
    }
    let (w, h, mut pixels) = niri_dash_system::clipboard::shrink(width, height, rgba, MAX);
    // GPUI draws BGRA.
    for p in pixels.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    let buffer = RgbaImage::from_raw(w, h, pixels)?;
    Some(Pic::Raster(Arc::new(RenderImage::new(vec![Frame::new(
        buffer,
    )]))))
}

fn load(path: &Path) -> Option<Pic> {
    let bytes = std::fs::read(path)
        .map_err(|e| log::debug!("{}: {e}", path.display()))
        .ok()?;
    match image::load_from_memory(&bytes) {
        Ok(decoded) => {
            let rgba = decoded.to_rgba8();
            raster(rgba.width(), rgba.height(), rgba.into_raw())
        }
        // Not a raster format we know: an SVG, if it is anything, and GPUI draws those.
        Err(_) if looks_like_svg(&bytes) => Some(Pic::Vector(path.to_path_buf())),
        Err(e) => {
            log::debug!("{}: {e}", path.display());
            None
        }
    }
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(1024)];
    head.windows(4).any(|w| w == b"<svg")
}

/// What was made of each path, and when it was last used.
#[derive(Default)]
pub struct Pics {
    by_path: HashMap<PathBuf, (Option<Pic>, u64)>,
    uses: u64,
}

impl Pics {
    /// The picture for `src`, if there is one.
    pub fn get(&mut self, src: &IconSrc) -> Option<Pic> {
        match src {
            IconSrc::None => None,
            IconSrc::Path(p) => {
                self.uses += 1;
                let now = self.uses;
                let entry = self
                    .by_path
                    .entry(p.clone())
                    .or_insert_with(|| (load(p), now));
                entry.1 = now;
                let pic = entry.0.clone();
                if self.by_path.len() > CACHE {
                    // Drop the older half in one go rather than one entry per call.
                    let mut ages: Vec<u64> = self.by_path.values().map(|(_, t)| *t).collect();
                    ages.sort_unstable();
                    let cut = ages[ages.len() / 2];
                    self.by_path.retain(|_, (_, t)| *t >= cut);
                }
                pic
            }
            IconSrc::Rgba {
                width,
                height,
                data,
            } => raster(*width, *height, data.as_ref().clone()),
        }
    }

    /// How many paths are kept.
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.by_path.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(w: u32, h: u32) -> IconSrc {
        IconSrc::Rgba {
            width: w,
            height: h,
            data: Arc::new(vec![200; (w * h * 4) as usize]),
        }
    }

    #[test]
    fn pixels_become_a_picture_and_nothing_stays_nothing() {
        let mut pics = Pics::default();
        assert!(pics.get(&IconSrc::None).is_none());
        assert!(matches!(pics.get(&rgba(4, 2)), Some(Pic::Raster(_))));
        assert!(pics.get(&rgba(0, 0)).is_none());
    }

    #[test]
    fn large_pictures_are_shrunk() {
        let Some(Pic::Raster(r)) = raster(1024, 512, vec![0; 1024 * 512 * 4]) else {
            panic!("a picture");
        };
        let size = r.size(0);
        assert_eq!((size.width.0, size.height.0), (512, 256));
    }

    #[test]
    fn paths_are_cached_and_the_cache_is_bounded() {
        let dir = std::env::temp_dir().join(format!("niri-dash-pics-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(RgbaImage::from_pixel(3, 3, image::Rgba([9, 9, 9, 255])))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let mut pics = Pics::default();
        let first = dir.join("a.png");
        std::fs::write(&first, &png).unwrap();
        let a = pics.get(&IconSrc::Path(first.clone())).expect("a picture");
        assert!(
            pics.get(&IconSrc::Path(first.clone())).unwrap() == a,
            "the same one again"
        );
        for i in 0..CACHE + 20 {
            let p = dir.join(format!("{i}.png"));
            std::fs::write(&p, &png).unwrap();
            pics.get(&IconSrc::Path(p));
        }
        assert!(pics.len() <= CACHE + 1);
        // A path that isn't there is remembered as nothing, not tried again and again.
        assert!(pics.get(&IconSrc::Path(dir.join("nope.png"))).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

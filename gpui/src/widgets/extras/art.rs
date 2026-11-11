//! The mascots' drawings: the original SVG split into aligned layers (`assets/mascots/
//! <name>-<layer>.svg`, all with the same viewBox), so outfits can go on and off, the eyes can
//! blink and the pupils can move. Their own colours in every theme: they are characters, not
//! the dashboard.

use crate::mascots::Character;
use gpui::{App, ImageSource, RenderImage};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

/// What the figure of a character is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Art {
    /// The file prefix, and the lower case name.
    pub file: &'static str,
    /// What the bubble calls it.
    pub name: &'static str,
    /// The viewBox: width, height.
    pub view: (f32, f32),
    /// Between the eyes, as fractions of the width and height.
    pub head: (f32, f32),
    /// Seconds between blinks: each its own rhythm.
    pub blink: f32,
}

impl Art {
    /// Width over height.
    pub fn aspect(&self) -> f32 {
        self.view.0 / self.view.1
    }

    /// One viewBox unit as a fraction of the height.
    pub fn unit(&self) -> f32 {
        1.0 / self.view.1
    }
}

/// The figure for a character (the cow is text: it has none, and gets Schraubi's).
pub fn of(c: Character) -> Art {
    match c {
        Character::Zolli => Art {
            file: "zolli",
            name: "ZOLLI",
            view: (80.0, 140.0),
            head: (39.0 / 80.0, 64.0 / 140.0),
            blink: 6.1,
        },
        Character::Verschnitti => Art {
            file: "verschnitti",
            name: "VERSCHNITTI",
            view: (96.0, 84.0),
            head: (0.5, 38.0 / 84.0),
            blink: 5.3,
        },
        _ => Art {
            file: "schraubi",
            name: "SCHRAUBI",
            view: (64.0, 128.0),
            head: (0.5, 33.0 / 128.0),
            blink: 4.7,
        },
    }
}

/// The layers, back to front (outfit layers are drawn only when worn).
#[cfg(test)]
pub const LAYERS: [&str; 12] = [
    "body", "helmet", "vest", "beard", "brow", "eyes", "closed", "pupils", "shades", "tophat",
    "party", "mouth",
];

pub fn path(art: &Art, layer: &str) -> String {
    format!("mascots/{}-{layer}.svg", art.file)
}

/// The layers drawn at the size they are shown, so they stay sharp however big the widget is
/// (GPUI draws an SVG image at twice its own size, which blurs a figure in a big tile).
#[derive(Default)]
pub struct Raster(RefCell<HashMap<(String, u32), Arc<RenderImage>>>);

impl Raster {
    /// The picture of `path` for a figure `height` pixels high whose viewBox is `view_h` high.
    pub fn source(&self, path: String, height: f32, view_h: f32, cx: &App) -> ImageSource {
        // Whole steps, and always a step above what is needed: sharp, and not redone for every
        // pixel the widget is resized by.
        let steps = (height / view_h).ceil().clamp(1.0, 8.0) as u32;
        let key = (path, steps);
        if let Some(image) = self.0.borrow().get(&key) {
            return ImageSource::Render(image.clone());
        }
        let bytes = gpui::AssetSource::load(&crate::assets::Assets, &key.0)
            .ok()
            .flatten();
        let image = bytes.and_then(|bytes| {
            cx.svg_renderer()
                .render_single_frame(&bytes, steps as f32)
                .ok()
        });
        match image {
            Some(image) => {
                self.0.borrow_mut().insert(key, image.clone());
                ImageSource::Render(image)
            }
            // Not there (it should be): let GPUI say so.
            None => ImageSource::from(key.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::AssetSource;

    #[test]
    fn every_character_has_all_its_layers() {
        for c in [
            Character::Schraubi,
            Character::Zolli,
            Character::Verschnitti,
        ] {
            let art = of(c);
            for layer in LAYERS {
                let file = path(&art, layer);
                assert!(
                    crate::assets::Assets.load(&file).unwrap().is_some(),
                    "{file} is missing"
                );
            }
        }
    }

    #[test]
    fn the_viewbox_is_the_one_the_drawings_have() {
        for c in [
            Character::Schraubi,
            Character::Zolli,
            Character::Verschnitti,
        ] {
            let art = of(c);
            let svg = crate::assets::Assets
                .load(&path(&art, "body"))
                .unwrap()
                .unwrap();
            let svg = String::from_utf8_lossy(&svg);
            let want = format!("viewBox=\"0 0 {} {}\"", art.view.0, art.view.1);
            assert!(
                svg.contains(&want),
                "{}: {want} not in {}",
                art.file,
                &svg[..120]
            );
        }
    }
}

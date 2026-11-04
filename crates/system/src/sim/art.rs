//! Pictures for a world without icon themes or album art: a colored tile with an app's initial,
//! and an abstract cover for a track. Drawn once; the pixels are shared from then on.

use super::time::hash;
use crate::IconSrc;
use std::collections::HashMap;
use std::sync::Arc;

/// The capital letters, five columns of seven dots each (bit 0 at the top).
const FONT: [[u8; 5]; 26] = [
    [0x7e, 0x09, 0x09, 0x09, 0x7e],
    [0x7f, 0x49, 0x49, 0x49, 0x36],
    [0x3e, 0x41, 0x41, 0x41, 0x22],
    [0x7f, 0x41, 0x41, 0x22, 0x1c],
    [0x7f, 0x49, 0x49, 0x49, 0x41],
    [0x7f, 0x09, 0x09, 0x09, 0x01],
    [0x3e, 0x41, 0x49, 0x49, 0x7a],
    [0x7f, 0x08, 0x08, 0x08, 0x7f],
    [0x00, 0x41, 0x7f, 0x41, 0x00],
    [0x20, 0x40, 0x41, 0x3f, 0x01],
    [0x7f, 0x08, 0x14, 0x22, 0x41],
    [0x7f, 0x40, 0x40, 0x40, 0x40],
    [0x7f, 0x02, 0x0c, 0x02, 0x7f],
    [0x7f, 0x04, 0x08, 0x10, 0x7f],
    [0x3e, 0x41, 0x41, 0x41, 0x3e],
    [0x7f, 0x09, 0x09, 0x09, 0x06],
    [0x3e, 0x41, 0x51, 0x21, 0x5e],
    [0x7f, 0x09, 0x19, 0x29, 0x46],
    [0x46, 0x49, 0x49, 0x49, 0x31],
    [0x01, 0x01, 0x7f, 0x01, 0x01],
    [0x3f, 0x40, 0x40, 0x40, 0x3f],
    [0x1f, 0x20, 0x40, 0x20, 0x1f],
    [0x3f, 0x40, 0x38, 0x40, 0x3f],
    [0x63, 0x14, 0x08, 0x14, 0x63],
    [0x07, 0x08, 0x70, 0x08, 0x07],
    [0x61, 0x51, 0x49, 0x45, 0x43],
];

fn name_seed(name: &str) -> u64 {
    name.bytes()
        .fold(7, |h: u64, b| h.wrapping_mul(131).wrapping_add(b.into()))
}

/// Red, green and blue for a hue from 0 to 1.
fn hue(h: f64, saturation: f64, value: f64) -> [f64; 3] {
    let channel = |shift: f64| {
        let k = (shift + h * 6.0).rem_euclid(6.0);
        value - value * saturation * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    [channel(5.0), channel(3.0), channel(1.0)]
}

fn to_byte(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The app icons drawn so far.
#[derive(Default)]
pub struct Icons(HashMap<String, IconSrc>);

impl Icons {
    /// The tile for an app.
    pub fn get(&mut self, app: &str) -> IconSrc {
        self.0
            .entry(app.to_string())
            .or_insert_with(|| tile(app))
            .clone()
    }
}

/// A rounded tile in a color of the app's own, with its initial.
fn tile(app: &str) -> IconSrc {
    const SIZE: usize = 48;
    const RADIUS: f64 = 11.0;
    const DOT: usize = 4;
    let color = hue(hash(11, name_seed(app)), 0.55, 0.78);
    let letter = app
        .bytes()
        .find(u8::is_ascii_alphabetic)
        .map(|b| FONT[(b.to_ascii_uppercase() - b'A') as usize]);
    let (left, top) = ((SIZE - 5 * DOT) / 2, (SIZE - 7 * DOT) / 2);
    let lit = |x: usize, y: usize| {
        let Some(letter) = letter else { return false };
        if x < left || y < top {
            return false;
        }
        let (col, row) = ((x - left) / DOT, (y - top) / DOT);
        col < 5 && row < 7 && letter[col] >> row & 1 == 1
    };
    let mut px = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        for x in 0..SIZE {
            // Distance outside the rounded rectangle, for a soft edge.
            let half = SIZE as f64 / 2.0;
            let edge = |v: usize| ((v as f64 + 0.5 - half).abs() - (half - RADIUS)).max(0.0);
            let outside = edge(x).hypot(edge(y)) - RADIUS;
            let alpha = (0.5 - outside).clamp(0.0, 1.0);
            let shade = 1.08 - 0.22 * y as f64 / SIZE as f64;
            let [r, g, b] = if lit(x, y) {
                [1.0, 1.0, 1.0]
            } else {
                color.map(|c| c * shade)
            };
            px.extend_from_slice(&[to_byte(r), to_byte(g), to_byte(b), to_byte(alpha)]);
        }
    }
    IconSrc::Rgba {
        width: SIZE as u32,
        height: SIZE as u32,
        data: Arc::new(px),
    }
}

/// The pixels of an abstract cover: a diagonal gradient and soft rings, different for each `seed`.
pub fn cover_pixels(seed: u64, width: usize, height: usize) -> Vec<u8> {
    let a = hue(hash(21, seed), 0.6, 0.85);
    let b = hue(hash(22, seed), 0.75, 0.32);
    let (cx, cy) = (hash(23, seed), hash(24, seed));
    let rings = 3.0 + (hash(25, seed) * 5.0).floor();
    let mut px = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let (u, v) = (x as f64 / width as f64, y as f64 / height as f64);
            let d = (u - cx).hypot(v - cy);
            let ring = 0.5 + 0.5 * (d * rings * std::f64::consts::TAU).cos();
            let m = ((u + v) / 2.0 * 0.7 + ring * 0.3).clamp(0.0, 1.0);
            let c = [0, 1, 2].map(|i| to_byte(a[i] * (1.0 - m) + b[i] * m));
            px.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    px
}

/// The cover of a track.
pub fn cover(seed: u64) -> IconSrc {
    const SIZE: usize = 192;
    IconSrc::Rgba {
        width: SIZE as u32,
        height: SIZE as u32,
        data: Arc::new(cover_pixels(seed, SIZE, SIZE)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_are_whole_and_shared() {
        let mut icons = Icons::default();
        let (a, b) = (icons.get("firefox"), icons.get("firefox"));
        let (
            IconSrc::Rgba {
                width,
                height,
                data,
            },
            IconSrc::Rgba { data: same, .. },
        ) = (a, b)
        else {
            panic!("a tile is pixels");
        };
        assert_eq!(data.len(), (width * height * 4) as usize);
        assert!(Arc::ptr_eq(&data, &same));
    }

    #[test]
    fn names_without_letters_still_get_a_tile() {
        assert!(matches!(tile("123"), IconSrc::Rgba { .. }));
    }
}

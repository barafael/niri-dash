//! The mascots' drawings (ui/mascots): one SVG per layer, all with the same viewBox, so an
//! outfit is put on by stacking its layer over the body.

pub const LAYERS: &[(&str, &str)] = &[
    (
        "schraubi-beard",
        include_str!("../ui/mascots/schraubi-beard.svg"),
    ),
    (
        "schraubi-body",
        include_str!("../ui/mascots/schraubi-body.svg"),
    ),
    (
        "schraubi-brow",
        include_str!("../ui/mascots/schraubi-brow.svg"),
    ),
    (
        "schraubi-closed",
        include_str!("../ui/mascots/schraubi-closed.svg"),
    ),
    (
        "schraubi-eyes",
        include_str!("../ui/mascots/schraubi-eyes.svg"),
    ),
    (
        "schraubi-helmet",
        include_str!("../ui/mascots/schraubi-helmet.svg"),
    ),
    (
        "schraubi-mouth",
        include_str!("../ui/mascots/schraubi-mouth.svg"),
    ),
    (
        "schraubi-party",
        include_str!("../ui/mascots/schraubi-party.svg"),
    ),
    (
        "schraubi-pupils",
        include_str!("../ui/mascots/schraubi-pupils.svg"),
    ),
    (
        "schraubi-shades",
        include_str!("../ui/mascots/schraubi-shades.svg"),
    ),
    (
        "schraubi-tophat",
        include_str!("../ui/mascots/schraubi-tophat.svg"),
    ),
    (
        "schraubi-vest",
        include_str!("../ui/mascots/schraubi-vest.svg"),
    ),
    (
        "verschnitti-beard",
        include_str!("../ui/mascots/verschnitti-beard.svg"),
    ),
    (
        "verschnitti-body",
        include_str!("../ui/mascots/verschnitti-body.svg"),
    ),
    (
        "verschnitti-brow",
        include_str!("../ui/mascots/verschnitti-brow.svg"),
    ),
    (
        "verschnitti-closed",
        include_str!("../ui/mascots/verschnitti-closed.svg"),
    ),
    (
        "verschnitti-eyes",
        include_str!("../ui/mascots/verschnitti-eyes.svg"),
    ),
    (
        "verschnitti-helmet",
        include_str!("../ui/mascots/verschnitti-helmet.svg"),
    ),
    (
        "verschnitti-mouth",
        include_str!("../ui/mascots/verschnitti-mouth.svg"),
    ),
    (
        "verschnitti-party",
        include_str!("../ui/mascots/verschnitti-party.svg"),
    ),
    (
        "verschnitti-pupils",
        include_str!("../ui/mascots/verschnitti-pupils.svg"),
    ),
    (
        "verschnitti-shades",
        include_str!("../ui/mascots/verschnitti-shades.svg"),
    ),
    (
        "verschnitti-tophat",
        include_str!("../ui/mascots/verschnitti-tophat.svg"),
    ),
    (
        "verschnitti-vest",
        include_str!("../ui/mascots/verschnitti-vest.svg"),
    ),
    ("zolli-beard", include_str!("../ui/mascots/zolli-beard.svg")),
    ("zolli-body", include_str!("../ui/mascots/zolli-body.svg")),
    ("zolli-brow", include_str!("../ui/mascots/zolli-brow.svg")),
    (
        "zolli-closed",
        include_str!("../ui/mascots/zolli-closed.svg"),
    ),
    ("zolli-eyes", include_str!("../ui/mascots/zolli-eyes.svg")),
    (
        "zolli-helmet",
        include_str!("../ui/mascots/zolli-helmet.svg"),
    ),
    ("zolli-mouth", include_str!("../ui/mascots/zolli-mouth.svg")),
    ("zolli-party", include_str!("../ui/mascots/zolli-party.svg")),
    (
        "zolli-pupils",
        include_str!("../ui/mascots/zolli-pupils.svg"),
    ),
    (
        "zolli-shades",
        include_str!("../ui/mascots/zolli-shades.svg"),
    ),
    (
        "zolli-tophat",
        include_str!("../ui/mascots/zolli-tophat.svg"),
    ),
    ("zolli-vest", include_str!("../ui/mascots/zolli-vest.svg")),
];

use blenderui::Image;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

thread_local! {
    static CACHE: RefCell<HashMap<String, Option<Arc<Image>>>> = RefCell::new(HashMap::new());
}

/// The layer `name` ("zolli-body") of a mascot, decoded at 256 px.
pub fn layer(name: &str) -> Option<Arc<Image>> {
    CACHE.with(|c| {
        c.borrow_mut()
            .entry(name.to_string())
            .or_insert_with(|| {
                let svg = LAYERS.iter().find(|(n, _)| *n == name)?.1;
                // The layers have only a viewBox: give them a size, or they decode tiny.
                let (w, h) = view_box(svg);
                let svg = svg.replacen(
                    "<svg ",
                    &format!("<svg width=\"{}\" height=\"{}\" ", w * 4, h * 4),
                    1,
                );
                Image::decode(svg.as_bytes(), w * 4, h * 4).map(Arc::new)
            })
            .clone()
    })
}

/// The width and height of an SVG's viewBox.
fn view_box(svg: &str) -> (u32, u32) {
    let nums: Vec<u32> = svg
        .split("viewBox=\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .map(|v| {
            v.split_whitespace()
                .filter_map(|n| n.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    match nums.as_slice() {
        [_, _, w, h] => (*w, *h),
        _ => (80, 140),
    }
}

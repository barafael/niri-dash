//! The icons of the Slint version (ui/icons), recolored to the look and decoded once.

use blenderui::Image;
use niri_dash_system::IconSrc;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

const ICONS: &[(&str, &str)] = &[
    ("battery", include_str!("../ui/icons/battery.svg")),
    ("bell-off", include_str!("../ui/icons/bell-off.svg")),
    ("bell", include_str!("../ui/icons/bell.svg")),
    ("bluetooth", include_str!("../ui/icons/bluetooth.svg")),
    ("bolt", include_str!("../ui/icons/bolt.svg")),
    ("camera", include_str!("../ui/icons/camera.svg")),
    ("check", include_str!("../ui/icons/check.svg")),
    ("chevron-left", include_str!("../ui/icons/chevron-left.svg")),
    (
        "chevron-right",
        include_str!("../ui/icons/chevron-right.svg"),
    ),
    ("circle", include_str!("../ui/icons/circle.svg")),
    ("clipboard", include_str!("../ui/icons/clipboard.svg")),
    ("close", include_str!("../ui/icons/close.svg")),
    ("cloud-moon", include_str!("../ui/icons/cloud-moon.svg")),
    ("cloud-sun", include_str!("../ui/icons/cloud-sun.svg")),
    ("cloud", include_str!("../ui/icons/cloud.svg")),
    ("coffee", include_str!("../ui/icons/coffee.svg")),
    ("edit", include_str!("../ui/icons/edit.svg")),
    ("eject", include_str!("../ui/icons/eject.svg")),
    ("flag", include_str!("../ui/icons/flag.svg")),
    ("fog", include_str!("../ui/icons/fog.svg")),
    ("folder", include_str!("../ui/icons/folder.svg")),
    ("gamepad", include_str!("../ui/icons/gamepad.svg")),
    ("globe", include_str!("../ui/icons/globe.svg")),
    ("headphones", include_str!("../ui/icons/headphones.svg")),
    ("keyboard", include_str!("../ui/icons/keyboard.svg")),
    ("lock-small", include_str!("../ui/icons/lock-small.svg")),
    ("lock", include_str!("../ui/icons/lock.svg")),
    ("logout", include_str!("../ui/icons/logout.svg")),
    ("mic-off", include_str!("../ui/icons/mic-off.svg")),
    ("mic", include_str!("../ui/icons/mic.svg")),
    ("minus", include_str!("../ui/icons/minus.svg")),
    ("moon", include_str!("../ui/icons/moon.svg")),
    ("mouse", include_str!("../ui/icons/mouse.svg")),
    ("next", include_str!("../ui/icons/next.svg")),
    ("note", include_str!("../ui/icons/note.svg")),
    ("pause", include_str!("../ui/icons/pause.svg")),
    ("phone", include_str!("../ui/icons/phone.svg")),
    ("pipette", include_str!("../ui/icons/pipette.svg")),
    ("play", include_str!("../ui/icons/play.svg")),
    ("plus", include_str!("../ui/icons/plus.svg")),
    ("power", include_str!("../ui/icons/power.svg")),
    ("prev", include_str!("../ui/icons/prev.svg")),
    ("rain", include_str!("../ui/icons/rain.svg")),
    ("refresh", include_str!("../ui/icons/refresh.svg")),
    ("reset", include_str!("../ui/icons/reset.svg")),
    ("restart", include_str!("../ui/icons/restart.svg")),
    ("ring", include_str!("../ui/icons/ring.svg")),
    ("screen-share", include_str!("../ui/icons/screen-share.svg")),
    ("send", include_str!("../ui/icons/send.svg")),
    ("settings", include_str!("../ui/icons/settings.svg")),
    ("snow", include_str!("../ui/icons/snow.svg")),
    ("sparkle", include_str!("../ui/icons/sparkle.svg")),
    ("square", include_str!("../ui/icons/square.svg")),
    ("storm", include_str!("../ui/icons/storm.svg")),
    ("sun", include_str!("../ui/icons/sun.svg")),
    ("usb", include_str!("../ui/icons/usb.svg")),
    ("volume-mute", include_str!("../ui/icons/volume-mute.svg")),
    ("volume", include_str!("../ui/icons/volume.svg")),
    ("wifi-off", include_str!("../ui/icons/wifi-off.svg")),
    ("wifi", include_str!("../ui/icons/wifi.svg")),
    ("window", include_str!("../ui/icons/window.svg")),
];

/// Decoded images by what they were made from; None for one that wouldn't decode.
type Cache<K> = RefCell<HashMap<K, Option<Arc<Image>>>>;

thread_local! {
    static CACHE: Cache<(String, String)> = RefCell::new(HashMap::new());
    static FILES: Cache<std::path::PathBuf> = RefCell::new(HashMap::new());
}

/// The icon `name` (a file of ui/icons) stroked in `color` ("#rrggbbaa"), drawn at 48 px.
pub fn icon(name: &str, color: &str) -> Option<Arc<Image>> {
    let key = (name.to_string(), color.to_string());
    CACHE.with(|c| {
        c.borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                let svg = ICONS.iter().find(|(n, _)| *n == name)?.1;
                // The kit reads #rrggbb; the alpha goes into the stroke's opacity.
                let (rgb, alpha) = color.split_at(7.min(color.len()));
                let opacity = u8::from_str_radix(alpha, 16).map_or(1.0, |a| a as f32 / 255.0);
                let svg = svg
                    .replace(
                        "stroke=\"#000\"",
                        &format!("stroke=\"{rgb}\" stroke-opacity=\"{opacity}\""),
                    )
                    .replace("fill=\"#000\"", &format!("fill=\"{rgb}\""))
                    .replace("width=\"24\" height=\"24\"", "width=\"48\" height=\"48\"");
                Image::decode(svg.as_bytes(), 48, 48).map(Arc::new)
            })
            .clone()
    })
}

/// An image the system hands over: a file (an app's icon, album art) or pixels.
pub fn image(src: &IconSrc) -> Option<Arc<Image>> {
    match src {
        IconSrc::None => None,
        IconSrc::Path(p) => FILES.with(|f| {
            let mut f = f.borrow_mut();
            if f.len() > 256 {
                f.clear();
            }
            f.entry(p.clone())
                .or_insert_with(|| {
                    let bytes = std::fs::read(p).ok()?;
                    Image::decode(&bytes, 256, 256).map(Arc::new)
                })
                .clone()
        }),
        IconSrc::Rgba {
            width,
            height,
            data,
        } => Image::from_rgba(*width, *height, data).map(Arc::new),
    }
}

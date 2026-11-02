//! The sources', the layout's and the theme's enums as the Slint ones, and back: `x_to_ui` and
//! `x_from_ui`.

use crate::layout::{self, Kind};
use crate::sources::{self, power, tray};
use crate::{WidgetKind, theme};
use smithay_client_toolkit::seat::pointer::CursorIcon;
use strum::IntoEnumIterator;

pub(super) fn kind_to_ui(k: Kind) -> WidgetKind {
    match k {
        Kind::Clock => WidgetKind::Clock,
        Kind::Calendar => WidgetKind::Calendar,
        Kind::SystemMonitor => WidgetKind::SystemMonitor,
        Kind::Windows => WidgetKind::Windows,
        Kind::Wifi => WidgetKind::Wifi,
        Kind::Bluetooth => WidgetKind::Bluetooth,
        Kind::Battery => WidgetKind::Battery,
        Kind::Volume => WidgetKind::Volume,
        Kind::Brightness => WidgetKind::Brightness,
        Kind::Notifications => WidgetKind::Notifications,
        Kind::Tray => WidgetKind::Tray,
        Kind::ClaudeSessions => WidgetKind::ClaudeSessions,
        Kind::Clipboard => WidgetKind::Clipboard,
        Kind::Weather => WidgetKind::Weather,
        Kind::Memory => WidgetKind::Memory,
        Kind::Disks => WidgetKind::Disks,
        Kind::Network => WidgetKind::Network,
        Kind::Gpu => WidgetKind::Gpu,
        Kind::ClaudeUsage => WidgetKind::ClaudeUsage,
        Kind::Media => WidgetKind::Media,
        Kind::Session => WidgetKind::Session,
        Kind::Mascot => WidgetKind::Mascot,
        Kind::Privacy => WidgetKind::Privacy,
        Kind::Notes => WidgetKind::Notes,
        Kind::Timer => WidgetKind::Timer,
        Kind::ColorPicker => WidgetKind::ColorPicker,
        Kind::KeyboardLayout => WidgetKind::KeyboardLayout,
    }
}

pub(super) fn kind_from_ui(k: WidgetKind) -> Kind {
    Kind::iter()
        .find(|&x| kind_to_ui(x) == k)
        .expect("every widget kind is a kind")
}

pub(super) fn shape_to_ui(s: layout::Shape) -> crate::Shape {
    match s {
        layout::Shape::Rect => crate::Shape::Rect,
        layout::Shape::Round => crate::Shape::Round,
    }
}

pub(super) fn toggle_to_ui(t: tray::Toggle) -> crate::MenuToggle {
    match t {
        tray::Toggle::None => crate::MenuToggle::None,
        tray::Toggle::Check => crate::MenuToggle::Check,
        tray::Toggle::Radio => crate::MenuToggle::Radio,
    }
}

/// The pointer the UI asks for, as a cursor shape.
pub(super) fn cursor_from_ui(p: crate::PointerShape) -> CursorIcon {
    match p {
        crate::PointerShape::Normal => CursorIcon::Default,
        crate::PointerShape::Move => CursorIcon::Move,
        crate::PointerShape::Resize => CursorIcon::NwseResize,
    }
}

/// The Style tab's keys: the theme's style variables, mirrored as a Slint enum.
const STYLE_KEYS: [(theme::StyleVar, crate::StyleKey); 14] = [
    (theme::StyleVar::Radius, crate::StyleKey::Radius),
    (theme::StyleVar::RadiusSmall, crate::StyleKey::RadiusSmall),
    (theme::StyleVar::Gap, crate::StyleKey::Gap),
    (theme::StyleVar::Padding, crate::StyleKey::Padding),
    (theme::StyleVar::BorderWidth, crate::StyleKey::BorderWidth),
    (theme::StyleVar::FontFamily, crate::StyleKey::FontFamily),
    (theme::StyleVar::FontSmall, crate::StyleKey::FontSmall),
    (theme::StyleVar::FontBody, crate::StyleKey::FontBody),
    (theme::StyleVar::FontTitle, crate::StyleKey::FontTitle),
    (
        theme::StyleVar::FontWeightDisplay,
        crate::StyleKey::FontWeightDisplay,
    ),
    (theme::StyleVar::Motion, crate::StyleKey::Motion),
    (theme::StyleVar::Glass, crate::StyleKey::Glass),
    (
        theme::StyleVar::BackdropOpacity,
        crate::StyleKey::BackdropOpacity,
    ),
    (theme::StyleVar::CardOpacity, crate::StyleKey::CardOpacity),
];

pub(super) fn style_var_to_ui(v: theme::StyleVar) -> crate::StyleKey {
    STYLE_KEYS
        .iter()
        .find(|(r, _)| *r == v)
        .map(|(_, k)| *k)
        .expect("every style variable has a key")
}

pub(super) fn style_var_from_ui(k: crate::StyleKey) -> theme::StyleVar {
    STYLE_KEYS
        .iter()
        .find(|(_, u)| *u == k)
        .map(|(r, _)| *r)
        .expect("every key is a style variable")
}

pub(super) fn profile_to_ui(p: power::Profile) -> crate::PowerProfile {
    match p {
        power::Profile::PowerSaver => crate::PowerProfile::PowerSaver,
        power::Profile::Balanced => crate::PowerProfile::Balanced,
        power::Profile::Performance => crate::PowerProfile::Performance,
    }
}

pub(super) fn profile_from_ui(p: crate::PowerProfile) -> power::Profile {
    match p {
        crate::PowerProfile::PowerSaver => power::Profile::PowerSaver,
        crate::PowerProfile::Balanced => power::Profile::Balanced,
        crate::PowerProfile::Performance => power::Profile::Performance,
    }
}

pub(super) fn weather_icon_to_ui(i: sources::weather::Icon) -> crate::WeatherIcon {
    use sources::weather::Icon;
    match i {
        Icon::Sun => crate::WeatherIcon::Sun,
        Icon::Moon => crate::WeatherIcon::Moon,
        Icon::CloudSun => crate::WeatherIcon::CloudSun,
        Icon::CloudMoon => crate::WeatherIcon::CloudMoon,
        Icon::Cloud => crate::WeatherIcon::Cloud,
        Icon::Fog => crate::WeatherIcon::Fog,
        Icon::Rain => crate::WeatherIcon::Rain,
        Icon::Snow => crate::WeatherIcon::Snow,
        Icon::Storm => crate::WeatherIcon::Storm,
    }
}

pub(super) fn character_to_ui(c: crate::mascots::Character) -> crate::Character {
    use crate::mascots::Character as C;
    match c {
        C::Schraubi => crate::Character::Schraubi,
        C::Zolli => crate::Character::Zolli,
        C::Verschnitti => crate::Character::Verschnitti,
    }
}

pub(super) fn outfit_to_ui(o: crate::mascots::Outfit) -> crate::Outfit {
    use crate::mascots::Outfit as O;
    match o {
        O::Plain => crate::Outfit::Plain,
        O::Helmet => crate::Outfit::Helmet,
        O::Vest => crate::Outfit::Vest,
        O::Beard => crate::Outfit::Beard,
        O::Shades => crate::Outfit::Shades,
        O::Tophat => crate::Outfit::Tophat,
        O::Party => crate::Outfit::Party,
    }
}

pub(super) fn device_kind_to_ui(k: sources::power::DeviceKind) -> crate::DeviceKind {
    use crate::sources::power::DeviceKind as K;
    match k {
        K::Mouse => crate::DeviceKind::Mouse,
        K::Keyboard => crate::DeviceKind::Keyboard,
        K::Headphones => crate::DeviceKind::Headphones,
        K::Gamepad => crate::DeviceKind::Gamepad,
        K::Phone => crate::DeviceKind::Phone,
        K::Other => crate::DeviceKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The values of `export enum <name> { ... }` in a Slint file.
    fn slint_enum<'a>(src: &'a str, name: &str) -> Vec<&'a str> {
        let head = format!("export enum {name} {{");
        let start = src.find(&head).expect("the enum is declared") + head.len();
        let end = start + src[start..].find('}').expect("the enum is closed");
        src[start..end]
            .split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .collect()
    }

    /// Every kind has a Slint `WidgetKind` value spelled like its config name, Rust maps it to
    /// that value, and frame.slint shows a widget for it.
    #[test]
    fn every_kind_has_a_slint_widget() {
        let data = include_str!("../../ui/data.slint");
        let frame = include_str!("../../ui/frame.slint");
        let values = slint_enum(data, "WidgetKind");
        for kind in Kind::iter() {
            let name = kind.name();
            assert!(
                values.contains(&name),
                "ui/data.slint: WidgetKind has no `{name}`"
            );
            assert_eq!(
                format!("{:?}", kind_to_ui(kind)),
                format!("{kind:?}"),
                "kind_to_ui maps {kind:?} to the wrong WidgetKind"
            );
            let arm = format!("if root.inst.kind == WidgetKind.{name}:");
            assert!(frame.contains(&arm), "ui/frame.slint has no `{arm}` arm");
        }
        assert_eq!(
            values.len(),
            Kind::iter().count(),
            "WidgetKind has values that are no Kind: {values:?}"
        );
    }
}

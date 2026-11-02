//! Mapping the sources' and the layout's enums to the Slint ones.

use crate::layout::Kind;
use crate::sources::{self, power};
use crate::{WidgetKind, theme};
use strum::IntoEnumIterator;

pub(super) fn kind_to_ui(k: Kind) -> WidgetKind {
    match k {
        Kind::Clock => WidgetKind::Clock,
        Kind::Calendar => WidgetKind::Calendar,
        Kind::Perf => WidgetKind::Perf,
        Kind::Windows => WidgetKind::Windows,
        Kind::Wifi => WidgetKind::Wifi,
        Kind::Bluetooth => WidgetKind::Bluetooth,
        Kind::Battery => WidgetKind::Battery,
        Kind::Audio => WidgetKind::Audio,
        Kind::Brightness => WidgetKind::Brightness,
        Kind::Notifications => WidgetKind::Notifications,
        Kind::Tray => WidgetKind::Tray,
        Kind::Claude => WidgetKind::Claude,
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
        .unwrap_or(Kind::Clock)
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

pub(super) fn style_key(v: theme::StyleVar) -> crate::StyleKey {
    STYLE_KEYS
        .iter()
        .find(|(r, _)| *r == v)
        .map(|(_, k)| *k)
        .expect("every style variable has a key")
}

pub(super) fn style_var(k: crate::StyleKey) -> theme::StyleVar {
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

pub(super) fn weather_icon(i: sources::weather::Icon) -> crate::WeatherIcon {
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

pub(super) fn device_kind(k: crate::sources::power::DeviceKind) -> crate::DeviceKind {
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

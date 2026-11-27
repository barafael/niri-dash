//! What the system last reported, applet by applet. The widgets only read this; the system's
//! events are the only thing that writes it ([`Data::apply`]).

use niri_dash_system::{
    self as sys, audio, bluetooth, claude, drives, keyboard, media, night_light, notifications,
    perf, phones, power, privacy, sleep, tray, weather, wifi, windows,
};

/// One clipboard entry, newest first in [`Data::clips`].
#[derive(Debug, Clone)]
pub struct Clip {
    pub clip: sys::clipboard::Clip,
    /// The text, or "Image 640×480".
    pub preview: String,
}

#[derive(Default)]
pub struct Data {
    pub output: Option<sys::host::Output>,
    pub perf: Option<Box<perf::Snapshot>>,
    pub claude: claude::State,
    pub usage: Option<Box<claude::Usage>>,
    pub windows: windows::State,
    /// None: the compositor doesn't say.
    pub layouts: Option<keyboard::Layouts>,
    pub privacy: privacy::Usage,
    pub casts: Vec<privacy::ScreenCast>,
    /// The last color picked from the screen (r, g, b in 0..1), until the next pick.
    pub picked: Option<[f64; 3]>,
    pub wifi: Option<Box<wifi::State>>,
    pub bluetooth: Option<Box<bluetooth::State>>,
    pub audio: Option<audio::State>,
    /// None: no backlight.
    pub brightness: Option<f32>,
    pub notifications: notifications::State,
    pub tray: Vec<tray::Item>,
    pub menu: Option<tray::Menu>,
    pub media: Option<media::State>,
    pub power: Option<power::State>,
    pub inhibited: bool,
    pub blockers: Vec<power::Blocker>,
    pub drives: drives::State,
    pub phones: phones::State,
    pub weather: weather::State,
    pub night_light: Option<night_light::State>,
    pub clips: Vec<Clip>,
    /// How many events have come in, for the log and the tests.
    pub delivered: u64,
}

/// What an event means beyond the data it carries, for the dashboard to act on.
pub enum Followup {
    None,
    Toast(String),
    /// A mascot's command printed something.
    MascotSaid(u32, Option<String>),
    /// A color was picked: keep it.
    Picked([f64; 3]),
    /// The sleep timer's modes, or what came of running one.
    Sleep(sleep::Event),
}

impl Data {
    pub fn apply(&mut self, event: sys::Event) -> Followup {
        use sys::Event as E;
        self.delivered += 1;
        match event {
            E::Host(sys::host::Event::Output(o)) => self.output = o,
            E::Perf(perf::Event::Snapshot(s)) => self.perf = Some(s),
            E::Claude(claude::Event::Sessions(s)) => self.claude = s,
            E::Claude(claude::Event::Usage(u)) => self.usage = Some(u),
            E::Windows(windows::Event::State(s)) => self.windows = s,
            E::Keyboard(keyboard::Event::Layouts(l)) => self.layouts = l,
            E::Privacy(privacy::Event::Usage(u)) => self.privacy = u,
            E::Privacy(privacy::Event::Casts(c)) => self.casts = c,
            E::Color(sys::color::Event::Picked(c)) => {
                if let Some(c) = c {
                    self.picked = Some(c);
                    return Followup::Picked(c);
                }
            }
            E::Wifi(wifi::Event::State(s)) => self.wifi = Some(s),
            E::Bluetooth(bluetooth::Event::State(s)) => self.bluetooth = Some(s),
            E::Audio(audio::Event::State(s)) => self.audio = Some(s),
            E::Brightness(sys::brightness::Event::Level(l)) => self.brightness = l,
            E::Notifications(notifications::Event::State(s)) => self.notifications = s,
            E::Tray(tray::Event::Items(i)) => self.tray = i,
            E::Tray(tray::Event::Menu(m)) => self.menu = Some(m),
            E::Media(media::Event::State(s)) => self.media = s,
            E::Power(power::Event::State(s)) => self.power = Some(s),
            E::Power(power::Event::Inhibited(on)) => self.inhibited = on,
            E::Power(power::Event::SleepBlockers(b)) => self.blockers = b,
            E::Drives(drives::Event::State(s)) => self.drives = s,
            E::Phones(phones::Event::State(s)) => self.phones = s,
            E::Weather(weather::Event::State(s)) => self.weather = s,
            E::NightLight(night_light::Event::State(s)) => self.night_light = Some(s),
            E::Clipboard(sys::clipboard::Event::Copied(clip)) => self.copied(clip),
            E::Mascot(sys::mascot::Event::Output { id, stdout }) => {
                return Followup::MascotSaid(id, stdout);
            }
            E::Sleep(e) => return Followup::Sleep(e),
            E::Toast(t) => return Followup::Toast(t),
        }
        Followup::None
    }

    fn copied(&mut self, clip: sys::clipboard::Clip) {
        let preview = match clip.kind {
            sys::clipboard::ClipKind::Text => {
                sys::clipboard::preview(&String::from_utf8_lossy(&clip.data))
            }
            sys::clipboard::ClipKind::Png => match sys::clipboard::decode_png(&clip.data) {
                Some((w, h, _)) => format!("Image {w}×{h}"),
                None => "Image".into(),
            },
        };
        // The same thing copied again moves to the top.
        self.clips.retain(|c| c.clip.data != clip.data);
        self.clips.insert(0, Clip { clip, preview });
        self.clips.truncate(sys::clipboard::MAX_ENTRIES);
    }
}

//! A system that isn't there but keeps moving: the same commands and events as the real one, with
//! a believable machine behind them. It builds and browses and takes calls, batteries run down,
//! notifications come in, networks drift in and out of range; and everything someone does through
//! the dashboard (a slider, a pairing, a password) works and stays done. This is what the web
//! version runs on, as a browser has no D-Bus and no compositor to ask.
//!
//! Like the real system's actors, [`Sim`] owns its state, answers on channels and ends when its
//! senders are gone. Unlike them it has no outside world: what isn't anyone's to change is a
//! function of the clock ([`world`]), with a period of its own for each thing ([`time`]).

mod art;
mod bluetooth;
mod desk;
mod time;
mod wifi;
mod world;

use crate::{
    Event, EventSender, Inboxes, MockSystem, Received, Started, audio, brightness, channels,
    claude, clipboard, color, drives, host, keyboard, mascot, media, night_light, notifications,
    perf, phones, power, privacy, session, tray, weather, windows,
};
use art::Icons;
use std::time::Duration;

/// How often the world is looked at. Everything is reported only as it changes, most of it once
/// a second at most.
const TICK: Duration = Duration::from_millis(200);

/// Start a simulated system, reporting to `events`: on a thread of its own on the desktop, on the
/// page's event loop in a browser.
#[cfg(not(target_arch = "wasm32"))]
pub fn simulated(events: EventSender) -> std::io::Result<Started> {
    let (senders, inboxes) = channels();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()?;
    std::thread::Builder::new()
        .name("niri-dash-sim".into())
        .spawn(move || rt.block_on(Sim::new(now(), events).event_loop(inboxes)))?;
    Ok(Started {
        senders,
        caps: MockSystem::CAPS,
    })
}

/// Start a simulated system, reporting to `events`: on a thread of its own on the desktop, on the
/// page's event loop in a browser.
#[cfg(target_arch = "wasm32")]
pub fn simulated(events: EventSender) -> std::io::Result<Started> {
    let (senders, inboxes) = channels();
    wasm_bindgen_futures::spawn_local(async move {
        Sim::new(now(), events).event_loop(inboxes).await;
    });
    Ok(Started {
        senders,
        caps: MockSystem::CAPS,
    })
}

/// The time in seconds since the epoch.
fn now() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

#[cfg(not(target_arch = "wasm32"))]
async fn sleep(d: Duration) {
    tokio::time::sleep(d).await;
}

#[cfg(target_arch = "wasm32")]
async fn sleep(d: Duration) {
    gloo_timers::future::sleep(d).await;
}

/// Warm the screen, as the gamma tables would. A browser has a page to tint instead of a screen.
#[cfg(target_arch = "wasm32")]
fn tint(kelvin: Option<u32>) {
    let [r, g, b] = kelvin.map_or([1.0; 3], night_light::whitepoint);
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(root) = document.document_element() else {
        return;
    };
    let color = format!("rgb({:.0} {:.0} {:.0})", r * 255.0, g * 255.0, b * 255.0);
    if let Ok(root) = wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(root) {
        let _ = root.style().set_property("--night-light", &color);
    }
}

/// Only what is shown changes the screen, on the desktop.
#[cfg(not(target_arch = "wasm32"))]
fn tint(_: Option<u32>) {}

/// Put text on the browser's clipboard, the only one there is.
#[cfg(target_arch = "wasm32")]
fn copy(clip: &clipboard::Clip) {
    if clip.kind == clipboard::ClipKind::Text
        && let Ok(text) = std::str::from_utf8(&clip.data)
        && let Some(window) = web_sys::window()
    {
        // A promise nobody needs to wait for; a refusal only means nothing got copied.
        let _ = window.navigator().clipboard().write_text(text);
    }
}

/// There is no clipboard to put it back on; the widget keeps its history.
#[cfg(not(target_arch = "wasm32"))]
fn copy(_: &clipboard::Clip) {}

/// The last thing reported of some kind, to report it only when it changes.
struct Last<T>(Option<T>);

impl<T: PartialEq + Clone> Last<T> {
    /// `new`, if it is news (or `force`d).
    fn news(&mut self, new: T, force: bool) -> Option<T> {
        if !force && self.0.as_ref() == Some(&new) {
            return None;
        }
        self.0 = Some(new.clone());
        Some(new)
    }
}

impl<T> Default for Last<T> {
    fn default() -> Self {
        Self(None)
    }
}

/// What was last reported, per applet.
#[derive(Default)]
struct Reported {
    claude: Last<claude::State>,
    usage: Last<claude::Usage>,
    windows: Last<windows::State>,
    usage_privacy: Last<privacy::Usage>,
    casts: Last<Vec<privacy::ScreenCast>>,
    wifi: Last<crate::wifi::State>,
    bluetooth: Last<crate::bluetooth::State>,
    audio: Last<audio::State>,
    tray: Last<Vec<tray::Item>>,
    media: Last<media::State>,
    power: Last<power::State>,
    blockers: Last<Vec<power::Blocker>>,
    drives: Last<drives::State>,
    phones: Last<phones::State>,
    weather: Last<weather::State>,
}

/// The simulated system.
pub struct Sim {
    events: EventSender,
    /// The time of the last look at the world.
    t: f64,
    visible: bool,
    /// When the world was last reported in full; a second's news is reported once.
    last_second: i64,
    /// Whether a clipboard widget wants what gets copied: as on the desktop, nothing until then.
    recording: bool,
    last_clip: f64,
    reported: Reported,
    icons: Icons,
    wifi: wifi::Wifi,
    bluetooth: bluetooth::Bluetooth,
    windows: desk::Windows,
    casts: desk::Casts,
    notifications: desk::Notifications,
    player: desk::Player,
    mixer: desk::Mixer,
    tray: desk::Tray,
    drives: desk::Drives,
    brightness: f32,
    profile: power::Profile,
    inhibited: bool,
    layouts: keyboard::Layouts,
    place: String,
    fahrenheit: bool,
    picks: u64,
    /// When a picked color comes back.
    picking: Option<f64>,
}

impl Sim {
    /// A system whose world is at the time `t`, reporting to `events`.
    pub fn new(t: f64, events: EventSender) -> Self {
        let mut icons = Icons::default();
        let notifications = desk::Notifications::new(t, &mut icons);
        Self {
            events,
            t,
            visible: false,
            last_second: i64::MIN,
            recording: false,
            last_clip: t,
            reported: Reported::default(),
            wifi: wifi::Wifi::new(t),
            bluetooth: bluetooth::Bluetooth::new(),
            windows: desk::Windows::default(),
            casts: desk::Casts::default(),
            notifications,
            player: desk::Player::new(t),
            mixer: desk::Mixer::new(),
            tray: desk::Tray::default(),
            drives: desk::Drives::new(),
            brightness: 0.7,
            profile: power::Profile::Balanced,
            inhibited: false,
            layouts: keyboard::Layouts {
                layouts: vec![
                    keyboard::Layout {
                        short: "EN".into(),
                        name: "English (US)".into(),
                    },
                    keyboard::Layout {
                        short: "DE".into(),
                        name: "German".into(),
                    },
                ],
                current: 0,
            },
            place: "Hamburg".into(),
            fahrenheit: false,
            picks: 0,
            picking: None,
            icons,
        }
    }

    /// Answer commands and move with the clock until every sender is gone; hands back the state.
    pub async fn event_loop(mut self, mut inbox: Inboxes) -> Self {
        loop {
            tokio::select! {
                cmd = inbox.next() => match cmd {
                    Some(cmd) => {
                        self.t = now();
                        self.handle(cmd);
                    }
                    None => break,
                },
                () = sleep(TICK) => self.tick(now()),
            }
        }
        self
    }

    fn send(&self, event: impl Into<Event>) {
        let _ = self.events.send(event.into());
    }

    fn toast(&self, text: impl Into<String>) {
        let _ = self.events.send(Event::Toast(text.into()));
    }

    /// The world at `t`, as far as it is news. Nothing is reported while the dashboard is away.
    pub fn tick(&mut self, t: f64) {
        self.t = t;
        // These go on while nobody looks, and catch up when someone does.
        self.notifications.advance(t, &mut self.icons);
        self.player.advance(t);
        self.bluetooth.advance(t);
        self.wifi.advance(t);
        if let Some(at) = self.picking
            && t >= at
        {
            self.picking = None;
            self.picks += 1;
            self.send(color::Event::Picked(Some(desk::picked_color(self.picks))));
        }
        if !self.visible {
            return;
        }
        self.report_flows(false);
        let second = t.floor() as i64;
        if second != self.last_second {
            self.last_second = second;
            self.report_world(false);
        }
    }

    /// What can change at any moment, as what someone asked for finishes.
    fn report_flows(&mut self, force: bool) {
        let t = self.t;
        if let Some(s) = self.reported.wifi.news(self.wifi.state(t), force) {
            self.send(crate::wifi::Event::State(Box::new(s)));
        }
        if let Some(s) = self.reported.bluetooth.news(self.bluetooth.state(t), force) {
            self.send(crate::bluetooth::Event::State(Box::new(s)));
        }
    }

    /// What moves with the clock.
    fn report_world(&mut self, force: bool) {
        let t = self.t;
        let addr = if self.wifi.online() {
            "192.168.1.23"
        } else {
            ""
        };
        self.send(perf::Event::Snapshot(Box::new(world::perf(
            t,
            self.wifi.online(),
            addr,
        ))));
        if let Some(s) = self.reported.claude.news(world::claude_sessions(t), force) {
            self.send(claude::Event::Sessions(s));
        }
        if let Some(s) = self.reported.usage.news(world::claude_usage(t), force) {
            self.send(claude::Event::Usage(Box::new(s)));
        }
        let state = self.windows.state(t, &mut self.icons);
        if let Some(s) = self.reported.windows.news(state, force) {
            self.send(windows::Event::State(s));
        }
        if let Some(s) = self
            .reported
            .usage_privacy
            .news(world::privacy_usage(t), force)
        {
            self.send(privacy::Event::Usage(s));
        }
        if let Some(s) = self.reported.casts.news(self.casts.state(t), force) {
            self.send(privacy::Event::Casts(s));
        }
        let playing = self.player.playing(t);
        let headphones = self.bluetooth.headphones();
        let state = self
            .mixer
            .state(t, playing, headphones.is_some(), &mut self.icons);
        if let Some(s) = self.reported.audio.news(state, force) {
            self.send(audio::Event::State(s));
        }
        let state = self.tray.items(t, &mut self.icons);
        if let Some(s) = self.reported.tray.news(state, force) {
            self.send(tray::Event::Items(s));
        }
        let state = self.player.state(t);
        if let Some(s) = self.reported.media.news(state, force) {
            self.send(media::Event::State(Some(s)));
        }
        let state = self.power_state(t, headphones);
        if let Some(s) = self.reported.power.news(state, force) {
            self.send(power::Event::State(s));
        }
        if let Some(s) = self
            .reported
            .blockers
            .news(world::sleep_blockers(playing, t), force)
        {
            self.send(power::Event::SleepBlockers(s));
        }
        if let Some(s) = self.reported.drives.news(self.drives.state(t), force) {
            self.send(drives::Event::State(s));
        }
        if let Some(s) = self.reported.phones.news(desk::phones(t), force) {
            self.send(phones::Event::State(s));
        }
        let state = world::weather(t, &self.place, self.fahrenheit);
        if let Some(s) = self.reported.weather.news(state, force) {
            self.send(weather::Event::State(s));
        }
        if self.recording {
            for clip in desk::copied_between(self.last_clip, t) {
                self.send(clipboard::Event::Copied(clip));
            }
        }
        self.last_clip = t;
    }

    fn power_state(&self, t: f64, headphones: Option<i32>) -> power::State {
        let (percent, charging, status_text, power_text) = world::battery(t);
        power::State {
            has_battery: true,
            percent,
            charging,
            status_text,
            power_text,
            profile: Some(self.profile),
            profiles: vec![
                power::Profile::PowerSaver,
                power::Profile::Balanced,
                power::Profile::Performance,
            ],
            peripherals: world::peripherals(t, headphones),
        }
    }

    /// Everything the dashboard shows, as the real sources send it when it comes up.
    fn report_all(&mut self) {
        self.send(brightness::Event::Level(Some(self.brightness)));
        self.send(keyboard::Event::Layouts(Some(self.layouts.clone())));
        self.send(power::Event::Inhibited(self.inhibited));
        self.send(notifications::Event::State(self.notifications.state()));
        self.report_flows(true);
        self.report_world(true);
    }

    fn handle(&mut self, cmd: Received) {
        use Received as R;
        let t = self.t;
        match cmd {
            R::Host(host::Command::FocusedOutput { reply }) => {
                let _ = reply.send(None);
            }
            R::Host(host::Command::Visible(on)) => {
                self.visible = on;
                if on {
                    self.tick(t);
                    self.report_all();
                }
            }
            R::Host(host::Command::Refresh) => self.report_all(),
            R::Host(host::Command::Open(path)) => {
                self.toast(format!("Would open {}", path.display()));
            }
            R::Host(host::Command::PlaySound(_)) => {}
            R::Windows(c) => self.windows.handle(c, t),
            R::Keyboard(keyboard::Command::Switch(to)) => {
                let n = self.layouts.layouts.len();
                self.layouts.current = match to {
                    keyboard::Switch::Next => (self.layouts.current + 1) % n,
                    keyboard::Switch::Prev => (self.layouts.current + n - 1) % n,
                    keyboard::Switch::Index(i) => (i as usize).min(n - 1),
                };
                self.send(keyboard::Event::Layouts(Some(self.layouts.clone())));
            }
            R::Privacy(privacy::Command::StopCast(_)) => self.casts.stop(t),
            R::Color(color::Command::Pick) => self.picking = Some(t + 0.6),
            R::Wifi(c) => self.wifi.handle(c, t),
            R::Bluetooth(c) => self.bluetooth.handle(c, t),
            R::Audio(c) => self.mixer.handle(c),
            R::Brightness(brightness::Command::Set(v)) => {
                self.brightness = v;
                self.send(brightness::Event::Level(Some(v)));
            }
            R::Notifications(c) => {
                let said = self.notifications.handle(c, t, &mut self.icons);
                if let Some(text) = said {
                    self.toast(text);
                }
                self.send(notifications::Event::State(self.notifications.state()));
            }
            R::Tray(tray::Command::Activate { address, .. }) => {
                self.toast(format!("{} would open now", desk::Tray::name(&address)));
            }
            R::Tray(tray::Command::OpenMenu { address, x, y }) => {
                self.send(tray::Event::Menu(self.tray.menu(&address, x, y)));
            }
            R::Tray(tray::Command::MenuItem { address, id }) => {
                if let Some(text) = self.tray.activate_entry(&address, id) {
                    self.toast(text);
                }
            }
            R::Media(c) => self.player.handle(c),
            R::Power(power::Command::SetProfile(p)) => self.profile = p,
            R::Power(power::Command::ToggleInhibit) => {
                self.inhibited = !self.inhibited;
                self.send(power::Event::Inhibited(self.inhibited));
            }
            R::Session(c) => self.toast(match c {
                session::Command::Lock => "This is only a demo: nothing to lock",
                session::Command::Suspend => "This is only a demo: nothing to suspend",
                session::Command::Logout => "This is only a demo: nobody to log out",
                session::Command::Reboot => "This is only a demo: nothing to reboot",
                session::Command::Poweroff => "This is only a demo: nothing to power off",
            }),
            R::Drives(c) => {
                if let Some(text) = self.drives.handle(c, t) {
                    self.toast(text);
                }
            }
            R::Phones(phones::Command::Ring(id)) => {
                self.toast(format!("{} is ringing", phone_name(&id)));
            }
            R::Phones(phones::Command::SendFiles(id)) => {
                self.toast(format!("Would pick files to send to {}", phone_name(&id)));
            }
            R::Weather(weather::Command::Configure(c)) => {
                if let Some(place) = c.location {
                    self.place = place;
                }
                self.fahrenheit = c.fahrenheit;
            }
            R::NightLight(night_light::Command::Gamma(kelvin)) => tint(kelvin),
            R::NightLight(night_light::Command::Kwin { .. }) => {}
            R::Mascot(mascot::Command::Run { id, .. }) => {
                // Nothing printed: the mascot says something of its own.
                self.send(mascot::Event::Output { id, stdout: None });
            }
            R::Clipboard(clipboard::Command::Set(clip)) => copy(&clip),
            R::Clipboard(clipboard::Command::Record(on)) => {
                if on && !self.recording {
                    // A clipboard that has been in use, from the first look on.
                    for clip in desk::copied_before(t, 5) {
                        self.send(clipboard::Event::Copied(clip));
                    }
                    self.last_clip = t;
                }
                self.recording = on;
            }
        }
        // Whatever that changed shows right away, not at the next second.
        if self.visible {
            self.report_flows(false);
            self.report_world(false);
        }
    }
}

fn phone_name(id: &str) -> &'static str {
    if id == "tablet" { "Tablet" } else { "Pixel" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Tell, bluetooth as bt, wifi as w};
    use smithay_client_toolkit::reexports::calloop::{EventLoop, channel};

    const T: f64 = 1_800_000_000.0;

    /// Everything reported so far.
    fn drain(rx: channel::Channel<Event>) -> Vec<Event> {
        let mut event_loop = EventLoop::<Vec<Event>>::try_new().unwrap();
        event_loop
            .handle()
            .insert_source(rx, |ev, _, seen| {
                if let channel::Event::Msg(e) = ev {
                    seen.push(e);
                }
            })
            .unwrap();
        let mut seen = Vec::new();
        event_loop
            .dispatch(Some(Duration::ZERO), &mut seen)
            .unwrap();
        seen
    }

    fn sim() -> (Sim, channel::Channel<Event>) {
        let (tx, rx) = channel::channel();
        (Sim::new(T, tx), rx)
    }

    #[tokio::test]
    async fn answers_commands_and_ends_with_its_senders() {
        let (sim, rx) = sim();
        let (tx, inbox) = channels();
        tx.host.tell(host::Command::Visible(true));
        tx.brightness.tell(brightness::Command::Set(0.3));
        tx.audio.tell(audio::Command::ToggleMute);
        tx.wifi.tell(w::Command::Toggle);
        tx.power
            .tell(power::Command::SetProfile(power::Profile::Performance));
        drop(tx);
        let sim = sim.event_loop(inbox).await;
        assert_eq!(sim.brightness, 0.3);
        assert_eq!(sim.profile, power::Profile::Performance);
        assert!(!sim.wifi.online());
        let seen = drain(rx);
        assert!(seen.iter().any(|e| matches!(
            e,
            Event::Brightness(brightness::Event::Level(Some(l))) if *l == 0.3
        )));
        assert!(seen.iter().any(|e| matches!(
            e,
            Event::Audio(audio::Event::State(s)) if s.muted
        )));
    }

    #[test]
    fn coming_up_reports_everything_once() {
        let (mut sim, rx) = sim();
        sim.handle(Received::Host(host::Command::Visible(true)));
        let seen = drain(rx);
        let count = |f: fn(&Event) -> bool| seen.iter().filter(|e| f(e)).count();
        for (what, n) in [
            ("perf", count(|e| matches!(e, Event::Perf(_)))),
            ("wifi", count(|e| matches!(e, Event::Wifi(_)))),
            ("bluetooth", count(|e| matches!(e, Event::Bluetooth(_)))),
            ("windows", count(|e| matches!(e, Event::Windows(_)))),
            ("audio", count(|e| matches!(e, Event::Audio(_)))),
            (
                "notifications",
                count(|e| matches!(e, Event::Notifications(_))),
            ),
            ("tray", count(|e| matches!(e, Event::Tray(_)))),
            ("media", count(|e| matches!(e, Event::Media(_)))),
            ("power", count(|e| matches!(e, Event::Power(_)))),
            ("drives", count(|e| matches!(e, Event::Drives(_)))),
            ("phones", count(|e| matches!(e, Event::Phones(_)))),
            ("weather", count(|e| matches!(e, Event::Weather(_)))),
            ("brightness", count(|e| matches!(e, Event::Brightness(_)))),
            ("keyboard", count(|e| matches!(e, Event::Keyboard(_)))),
        ] {
            assert!(n > 0, "{what} never reported");
        }
    }

    #[test]
    fn the_clipboard_is_only_read_when_asked() {
        let clip = |e: &Event| matches!(e, Event::Clipboard(_));
        let (mut sim, rx) = sim();
        sim.handle(Received::Host(host::Command::Visible(true)));
        sim.tick(T + 1000.0);
        assert!(!drain(rx).iter().any(clip));

        let (tx, rx) = channel::channel();
        sim.events = tx;
        sim.t = T + 1000.0;
        sim.handle(Received::Clipboard(clipboard::Command::Record(true)));
        sim.tick(T + 2000.0);
        let seen = drain(rx);
        // What was there when it started, and what came since.
        assert!(seen.iter().filter(|e| clip(e)).count() > 5);

        let (tx, rx) = channel::channel();
        sim.events = tx;
        sim.handle(Received::Clipboard(clipboard::Command::Record(false)));
        sim.tick(T + 3000.0);
        assert!(!drain(rx).iter().any(clip));
    }

    #[test]
    fn nothing_is_reported_while_away() {
        let (mut sim, rx) = sim();
        sim.handle(Received::Host(host::Command::Visible(false)));
        sim.tick(T + 500.0);
        assert!(drain(rx).is_empty());
    }

    #[test]
    fn only_news_is_reported() {
        let (mut sim, rx) = sim();
        sim.handle(Received::Host(host::Command::Visible(true)));
        // The same moment again changes nothing.
        let before = drain(rx);
        assert!(!before.is_empty());
        let (tx, rx) = channel::channel();
        sim.events = tx;
        sim.tick(sim.t);
        assert!(drain(rx).is_empty());
    }

    #[test]
    fn a_long_look_at_the_world() {
        let (mut sim, _rx) = sim();
        sim.handle(Received::Host(host::Command::Visible(true)));
        for i in 0..4000 {
            sim.tick(T + i as f64 * 1.7);
        }
    }

    #[test]
    fn a_pick_comes_back_a_moment_later() {
        let (mut sim, rx) = sim();
        sim.handle(Received::Color(color::Command::Pick));
        sim.tick(T + 0.1);
        sim.tick(T + 1.0);
        let seen = drain(rx);
        let picks = seen
            .iter()
            .filter(|e| matches!(e, Event::Color(color::Event::Picked(Some(_)))))
            .count();
        assert_eq!(picks, 1);
    }

    #[test]
    fn a_session_command_only_says_so() {
        let (mut sim, rx) = sim();
        sim.handle(Received::Session(session::Command::Poweroff));
        assert!(
            drain(rx)
                .iter()
                .any(|e| matches!(e, Event::Toast(t) if t.contains("demo")))
        );
    }

    #[test]
    fn what_pairing_needs_goes_through_the_actor() {
        let (mut sim, rx) = sim();
        sim.handle(Received::Host(host::Command::Visible(true)));
        sim.handle(Received::Bluetooth(bt::Command::Scan(true)));
        sim.tick(T + 15.0);
        let address = sim
            .bluetooth
            .state(T + 15.0)
            .nearby
            .first()
            .expect("a stranger")
            .address
            .clone();
        sim.handle(Received::Bluetooth(bt::Command::Pair(address)));
        sim.tick(T + 17.0);
        assert!(drain(rx).iter().any(|e| matches!(
            e,
            Event::Bluetooth(bt::Event::State(s)) if s.prompt.is_some()
        )));
    }
}

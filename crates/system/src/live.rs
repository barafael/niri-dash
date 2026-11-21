//! The real system: the sources, started on their own runtime, with one actor per applet that
//! turns the applet's commands into what the sources do.

use super::{
    COMMAND_QUEUE, Capabilities, Command, EventSender, Feed, Receiver, Sender, Sink, System, audio,
    bluetooth, brightness, channel, clipboard, color, drives, host, keyboard, mascot, media,
    night_light, notifications, phones, power, privacy, session, sleep, tray, wayland, weather,
    wifi, windows,
};
use crate::compositor;
use crate::sources::{
    self, Ctx, Mailbox, Next, audio::Audio, bluetooth::Bluetooth, brightness::Brightness,
    inhibit::Inhibit, kdeconnect::KdeConnect, media::Media, notifications::Notifications,
    tray::Tray, udisks::Drives, weather::Weather, wifi::Wifi,
};
use smithay_client_toolkit::reexports::client::Connection;
use std::future::Future;
use std::time::Duration;

/// How the live system is started.
pub struct Options {
    /// The feeds to run; None runs all of them (the dashboard).
    pub feeds: Option<&'static [Feed]>,
    /// Use Wayland, on a connection of the system's own (to `WAYLAND_DISPLAY`): for the
    /// clipboard, night light's gamma tables and, on KWin, the window list. false: none of these,
    /// as in an offscreen snapshot.
    pub wayland: bool,
    /// Run night light: warm the screen, or pause the desktop's own. The dashboard does (two
    /// clients can't both hold the gamma tables, and a pause lasts as long as whoever took it); a
    /// standalone app leaves it to the dashboard, and only shows it.
    pub hold_night_light: bool,
}

/// Start the real system on a thread of its own, reporting to `events`. Returns once it is
/// running, knows which compositor this is, and has reported where the dashboard would appear
/// ([`host::Event::Output`]). It stops when every clone of [`System::commands`] is dropped.
pub fn live(events: EventSender, opts: Options) -> std::io::Result<System> {
    let (started_tx, started_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("niri-dash-system".into())
        .spawn(move || {
            let rt = match runtime() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = started_tx.send(Err(e));
                    return;
                }
            };
            let compositor = compositor::detect(rt.handle());
            let ctx = Ctx::new(events.clone(), rt.handle().clone(), compositor);
            // A connection of our own: reading Wayland events on the dashboard's would hand it
            // its own (a surface's size, frame callbacks) without waking its loop.
            let conn = opts.wayland.then(Connection::connect_to_env).and_then(|c| {
                c.map_err(|e| log::warn!("no Wayland for the clipboard and night light: {e}"))
                    .ok()
            });
            let wayland = match conn {
                Some(conn) => {
                    match wayland::start(conn, events, opts.hold_night_light, ctx.is_kwin()) {
                        Ok(w) => Some(w),
                        Err(e) => {
                            let _ = started_tx.send(Err(e));
                            return;
                        }
                    }
                }
                None => None,
            };
            let mut caps = Capabilities::for_compositor(
                compositor,
                wayland.as_ref().is_some_and(|w| w.has_clipboard),
            );
            // KWin's window list is there only if it let us bind it.
            caps.windows |= wayland.as_ref().is_some_and(|w| w.has_windows);
            let ctx = match wayland.as_ref().filter(|w| w.has_windows) {
                Some(w) => ctx.with_kwin_windows(w.windows_by_pid.clone()),
                None => ctx,
            };
            let (commands, inbox) = System::inbox();
            let router = {
                let _in_runtime = rt.enter();
                start(&ctx, opts.feeds, opts.hold_night_light, wayland)
            };
            // Before the dashboard first shows, which lays out for it.
            compositor::report_output(&ctx);
            if started_tx.send(Ok(System { commands, caps })).is_err() {
                return;
            }
            rt.block_on(router.event_loop(inbox));
        })?;
    started_rx
        .recv()
        .unwrap_or_else(|_| Err(std::io::Error::other("the system thread died starting up")))
}

/// The runtime the sources run on.
fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("niri-dash-io")
        .enable_all()
        .build()
}

/// Start every source (those a standalone app doesn't show get a dormant context: they exist but
/// stay idle) and the actors in front of them.
fn start(
    ctx: &Ctx,
    feeds: Option<&'static [Feed]>,
    hold_night_light: bool,
    wayland: Option<wayland::Handles>,
) -> Router {
    let wants = |f: Feed| feeds.is_none_or(|fs| fs.contains(&f));
    let feed = |f: Feed| ctx.live_if(wants(f));
    sources::perf::start(feed(Feed::Perf));
    compositor::start(feed(Feed::Compositor));
    sources::power::start(feed(Feed::Power));
    sources::claude::start(feed(Feed::Claude));
    sources::claude_usage::start(feed(Feed::ClaudeUsage));
    sources::privacy::start(feed(Feed::Privacy));
    let locale = sources::locale();
    let kwin = ctx.is_kwin();
    let base = || ctx.clone();
    Router {
        host: run(Host { ctx: base() }),
        windows: run(WindowsActor {
            ctx: base(),
            kwin: wayland
                .as_ref()
                .filter(|w| w.has_windows)
                .map(|w| w.windows.clone()),
        }),
        keyboard: run(KeyboardActor { ctx: base() }),
        privacy: run(PrivacyActor { ctx: base() }),
        color: run(ColorActor { ctx: base() }),
        wifi: run(Wifi::start(feed(Feed::Wifi))),
        bluetooth: run(Bluetooth::start(feed(Feed::Bluetooth))),
        audio: run(AudioActor(Audio::start(feed(Feed::Audio)))),
        brightness: run(BrightnessActor(Brightness::start(feed(Feed::Brightness)))),
        notifications: run(Notifications::start(feed(Feed::Notifications), base())),
        tray: run(TrayActor(Tray::start(feed(Feed::Tray)))),
        media: run(Media::start(feed(Feed::Media))),
        power: run(PowerActor {
            inhibit: Inhibit::start(feed(Feed::Inhibit)),
            ctx: base(),
        }),
        session: run(SessionActor { ctx: base() }),
        drives: run(Drives::start(feed(Feed::Drives))),
        phones: run(KdeConnect::start(feed(Feed::Phone))),
        weather: run(WeatherActor(Weather::start(
            feed(Feed::Weather),
            locale.chrono,
        ))),
        night_light: run({
            let mut mailbox = Mailbox::default();
            let warmth = if kwin {
                Warmth::Desktop(sources::kde::NightLight::start(
                    feed(Feed::NightLight),
                    &mut mailbox,
                ))
            } else {
                // A standalone app trusts the dashboard to hold the tables.
                let tables = wayland
                    .as_ref()
                    .filter(|w| w.has_gamma)
                    .map(|w| w.gamma.clone());
                Warmth::Ours {
                    available: tables.is_some() || !hold_night_light,
                    tables,
                    on: false,
                    kelvin: night_light::NEUTRAL,
                }
            };
            NightLightActor {
                warmth,
                hold: hold_night_light,
                ctx: base(),
                mailbox,
            }
        }),
        mascot: run(MascotActor { ctx: base() }),
        clipboard: run(ClipboardActor(wayland.map(|w| w.clipboard))),
        sleep: run(SleepActor {
            awake: sources::sleep::Awake::start(base()),
            ctx: base(),
        }),
    }
}

macro_rules! router {
    ($($variant:ident($module:ident)),* $(,)?) => {
        /// The live system as the dashboard sees it: one actor that hands each command to the
        /// applet's own actor. It waits for room there, so a busy applet holds up the others
        /// only once its whole queue is full.
        struct Router {
            $($module: Sender<$module::Command>,)*
        }

        impl Router {
            /// Route commands until the dashboard's sender is gone; the applets' actors end with
            /// it.
            async fn event_loop(self, mut inbox: Receiver<Command>) {
                while let Some(cmd) = inbox.recv().await {
                    let sent = match cmd {
                        $(Command::$variant(c) => self.$module.send(c).await.is_ok(),)*
                    };
                    if !sent {
                        log::debug!("a system actor has stopped");
                    }
                }
            }
        }
    };
}

crate::applets!(router);

/// An actor: its data, and what it does with each command. The loop ends when every sender is
/// gone, and hands the actor back.
pub(crate) trait Actor: Sized + Send + 'static {
    type Command: Send + 'static;

    fn handle(&mut self, cmd: Self::Command);

    fn event_loop(mut self, mut rx: Receiver<Self::Command>) -> impl Future<Output = Self> + Send {
        async move {
            while let Some(cmd) = rx.recv().await {
                self.handle(cmd);
            }
            self
        }
    }
}

/// An actor with tasks of its own, which send it what they find. Commands and findings are handled
/// one at a time, so its data needs no lock; its tasks end when its loop does.
pub(crate) trait Stateful: Sized + Send + 'static {
    type Command: Send + 'static;
    type Inner: Send + 'static;

    fn mailbox(&mut self) -> &mut Mailbox<Self::Inner>;
    fn on_command(&mut self, cmd: Self::Command);
    fn on_inner(&mut self, inner: Self::Inner);
}

impl<T: Stateful> Actor for T {
    type Command = T::Command;

    fn handle(&mut self, cmd: T::Command) {
        self.on_command(cmd);
    }

    async fn event_loop(mut self, mut rx: Receiver<T::Command>) -> Self {
        while let Some(next) = self.mailbox().next(&mut rx).await {
            match next {
                Next::Inner(inner) => self.on_inner(inner),
                Next::Command(cmd) => self.on_command(cmd),
            }
        }
        self
    }
}

/// Run `actor`'s event loop on the current runtime; returns where to send it commands.
fn run<A: Actor>(actor: A) -> Sender<A::Command> {
    let (tx, rx) = channel(COMMAND_QUEUE);
    tokio::spawn(actor.event_loop(rx));
    tx
}

struct Host {
    ctx: Ctx,
}

impl Actor for Host {
    type Command = host::Command;

    fn handle(&mut self, cmd: host::Command) {
        use host::Command::*;
        match cmd {
            Visible(on) => self.ctx.set_visible(on),
            Refresh => self.ctx.refresh(),
            Open(path) => {
                if let Err(e) = sources::spawn_detached("xdg-open", &[&path.to_string_lossy()]) {
                    log::warn!("{e}");
                }
            }
            OpenUrl(url) => {
                if let Err(e) = sources::spawn_detached("xdg-open", &[&url]) {
                    log::warn!("{e}");
                }
            }
            PlaySound(sound) => {
                let path = match sound {
                    host::Sound::Alarm => {
                        "/usr/share/sounds/freedesktop/stereo/alarm-clock-elapsed.oga"
                    }
                };
                if std::path::Path::new(path).exists() {
                    let _ = sources::spawn_detached("pw-play", &[path]);
                }
            }
        }
    }
}

struct WindowsActor {
    ctx: Ctx,
    /// KWin's window list lives on the Wayland thread.
    kwin: Option<Sink<windows::Command>>,
}

impl Actor for WindowsActor {
    type Command = windows::Command;

    fn handle(&mut self, cmd: windows::Command) {
        use windows::Command::*;
        // The terminal's tab is Konsole's to bring up, whoever lists the windows.
        if let ShowTab(pid) = cmd {
            sources::konsole::show_tab(&self.ctx, pid);
            return;
        }
        if let Some(kwin) = &self.kwin {
            let _ = kwin.send(cmd);
            return;
        }
        match cmd {
            Focus(id) => compositor::focus_window(&self.ctx, id),
            FocusPid(pid) => compositor::focus_pid(&self.ctx, pid),
            ShowTab(pid) => sources::konsole::show_tab(&self.ctx, pid),
            Close(id) => compositor::close_window(&self.ctx, id),
        }
    }
}

struct KeyboardActor {
    ctx: Ctx,
}

impl Actor for KeyboardActor {
    type Command = keyboard::Command;

    fn handle(&mut self, cmd: keyboard::Command) {
        let keyboard::Command::Switch(to) = cmd;
        compositor::switch_layout(&self.ctx, to);
    }
}

struct PrivacyActor {
    ctx: Ctx,
}

impl Actor for PrivacyActor {
    type Command = privacy::Command;

    fn handle(&mut self, cmd: privacy::Command) {
        let privacy::Command::StopCast(session) = cmd;
        compositor::stop_cast(&self.ctx, session);
    }
}

struct ColorActor {
    ctx: Ctx,
}

impl Actor for ColorActor {
    type Command = color::Command;

    fn handle(&mut self, cmd: color::Command) {
        let color::Command::Pick = cmd;
        let ctx = self.ctx.clone();
        self.ctx.spawn(async move {
            let rgb = match compositor::pick_color(&ctx).await {
                Ok(rgb) => rgb,
                Err(e) => {
                    log::warn!("pick color: {e}");
                    ctx.toast(format!("Pick color: {e}"));
                    None
                }
            };
            ctx.send(color::Event::Picked(rgb));
        });
    }
}

struct AudioActor(Audio);

impl Actor for AudioActor {
    type Command = audio::Command;

    fn handle(&mut self, cmd: audio::Command) {
        use audio::Command::*;
        let a = &self.0;
        match cmd {
            SetVolume(v) => a.set_volume(v),
            SetMicVolume(v) => a.set_mic_volume(v),
            ToggleMute => a.toggle_mute(),
            ToggleMicMute => a.toggle_mic_mute(),
            SetStreamVolume(id, v) => a.set_stream_volume(id, v),
            ToggleStreamMute(id) => a.toggle_stream_mute(id),
            SetDefaultSink(id) => a.set_default_sink(id),
        }
    }
}

struct BrightnessActor(Brightness);

impl Actor for BrightnessActor {
    type Command = brightness::Command;

    fn handle(&mut self, cmd: brightness::Command) {
        let brightness::Command::Set(v) = cmd;
        self.0.set(v);
    }
}

struct TrayActor(Tray);

impl Actor for TrayActor {
    type Command = tray::Command;

    fn handle(&mut self, cmd: tray::Command) {
        use tray::Command::*;
        match cmd {
            Activate { address, x, y } => self.0.activate(address, x, y),
            OpenMenu { address, x, y } => self.0.open_menu(address, x, y),
            MenuItem { address, id } => self.0.menu_item(address, id),
        }
    }
}

struct PowerActor {
    inhibit: Inhibit,
    ctx: Ctx,
}

impl Actor for PowerActor {
    type Command = power::Command;

    fn handle(&mut self, cmd: power::Command) {
        match cmd {
            power::Command::SetProfile(p) => sources::power::set_profile(&self.ctx, p),
            power::Command::ToggleInhibit => self.inhibit.toggle(),
        }
    }
}

struct SessionActor {
    ctx: Ctx,
}

impl Actor for SessionActor {
    type Command = session::Command;

    fn handle(&mut self, cmd: session::Command) {
        use session::Command::*;
        use sources::session as s;
        match cmd {
            Lock => s::lock(&self.ctx),
            Suspend => s::suspend(&self.ctx),
            Logout => s::logout(&self.ctx),
            Reboot => s::reboot(&self.ctx),
            Poweroff => s::poweroff(&self.ctx),
        }
    }
}

struct WeatherActor(Weather);

impl Actor for WeatherActor {
    type Command = weather::Command;

    fn handle(&mut self, cmd: weather::Command) {
        let weather::Command::Configure(config) = cmd;
        self.0.configure(config);
    }
}

/// Night light, whoever warms the screen: the same commands in, the same state out.
struct NightLightActor {
    warmth: Warmth,
    /// Whether this system runs night light (the dashboard's does), or only shows it (a
    /// standalone app's, whose dashboard runs it).
    hold: bool,
    ctx: Ctx,
    mailbox: Mailbox<sources::kde::NightLightInner>,
}

/// Who warms the screen.
enum Warmth {
    /// The system, through the Wayland side's gamma tables (None without them, or where the
    /// dashboard holds them), as set here.
    Ours {
        tables: Option<Sink<Option<u32>>>,
        available: bool,
        on: bool,
        kelvin: u32,
    },
    /// The desktop, on its own schedule (KWin's); the system pauses it.
    Desktop(sources::kde::NightLight),
}

impl NightLightActor {
    /// Set our own night light to (`on`, `kelvin`) and report it.
    fn set(&mut self, to: (bool, u32)) {
        let Warmth::Ours {
            tables,
            available,
            on,
            kelvin,
        } = &mut self.warmth
        else {
            return;
        };
        (*on, *kelvin) = to;
        if self.hold
            && let Some(tables) = tables
        {
            let _ = tables.send(on.then_some(*kelvin));
        }
        self.ctx.send(night_light::Event::State(night_light::State {
            available: *available,
            on: *on,
            kelvin: *kelvin,
            schedule: None,
        }));
    }
}

impl Stateful for NightLightActor {
    type Command = night_light::Command;
    type Inner = sources::kde::NightLightInner;

    fn mailbox(&mut self) -> &mut Mailbox<Self::Inner> {
        &mut self.mailbox
    }

    fn on_inner(&mut self, inner: Self::Inner) {
        if let Warmth::Desktop(kwin) = &mut self.warmth {
            kwin.on_inner(inner);
        }
    }

    fn on_command(&mut self, cmd: night_light::Command) {
        use night_light::Command::*;
        match (cmd, &self.warmth) {
            (Restore { on, kelvin }, Warmth::Ours { .. }) => self.set((on, kelvin)),
            // The desktop keeps its own, and a pause ended with whoever held it.
            (Restore { .. }, Warmth::Desktop(_)) => {}
            (Apply { cmd, announce }, Warmth::Ours { on, kelvin, .. }) => {
                let (on, kelvin) = cmd.apply(*on, *kelvin);
                self.set((on, kelvin));
                if announce {
                    let text = if on {
                        format!("Night light on · {kelvin} K")
                    } else {
                        "Night light off".to_string()
                    };
                    sources::osd::send(&self.ctx, text, "weather-clear-night".into());
                }
            }
            // Pausing is for the one who holds night light; a standalone app's dashboard does,
            // and this one sees it in what the desktop reports.
            (Apply { cmd, announce }, Warmth::Desktop(kwin)) => {
                if self.hold {
                    kwin.apply(cmd, announce);
                }
            }
        }
    }
}

/// Hands clipboard commands to the Wayland side's event loop; None without Wayland, where what
/// is copied goes nowhere.
struct ClipboardActor(Option<Sink<clipboard::Command>>);

impl Actor for ClipboardActor {
    type Command = clipboard::Command;

    fn handle(&mut self, cmd: clipboard::Command) {
        if let Some(wayland) = &self.0 {
            let _ = wayland.send(cmd);
        }
    }
}

struct MascotActor {
    ctx: Ctx,
}

impl Actor for MascotActor {
    type Command = mascot::Command;

    /// A command that fails or hangs answers None; the mascot then says something of its own.
    fn handle(&mut self, cmd: mascot::Command) {
        let mascot::Command::Run { id, name, command } = cmd;
        let c = self.ctx.clone();
        self.ctx.spawn(async move {
            let env = [("NIRI_DASH_MASCOT", name)];
            let timeout = Some(Duration::from_secs(10));
            let stdout = match sources::run_with("sh", &["-c", &command], &env, timeout).await {
                Ok(stdout) => Some(stdout),
                Err(e) => {
                    log::warn!("mascot command {command:?}: {e}");
                    None
                }
            };
            c.send(mascot::Event::Output { id, stdout });
        });
    }
}

struct SleepActor {
    awake: sources::sleep::Awake,
    ctx: Ctx,
}

impl Actor for SleepActor {
    type Command = sleep::Command;

    fn handle(&mut self, cmd: sleep::Command) {
        match cmd {
            sleep::Command::Detect { script } => sources::sleep::detect(&self.ctx, script),
            sleep::Command::Run { mode, script } => {
                sources::sleep::perform(&self.ctx, mode, script)
            }
            sleep::Command::KeepAwake(on) => self.awake.set(on),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::night_light::{Cmd, Command::*, State};
    use crate::sources::testing::{ctx, reported};

    /// Night light that is the system's own, with gamma tables if given.
    fn ours(ctx: Ctx, hold: bool, tables: Option<Sink<Option<u32>>>) -> NightLightActor {
        NightLightActor {
            warmth: Warmth::Ours {
                available: tables.is_some() || !hold,
                tables,
                on: false,
                kelvin: night_light::NEUTRAL,
            },
            hold,
            ctx,
            mailbox: Mailbox::default(),
        }
    }

    fn states(events: std::sync::mpsc::Receiver<crate::Event>) -> Vec<State> {
        reported(events)
            .into_iter()
            .filter_map(|e| match e {
                crate::Event::NightLight(night_light::Event::State(s)) => Some(s),
                _ => None,
            })
            .collect()
    }

    fn warmed(rx: &mut Receiver<Option<u32>>) -> Vec<Option<u32>> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    /// What was set before a restart comes back, and every command shows in the state reported.
    #[tokio::test]
    async fn our_night_light_warms_the_screen_and_says_how() {
        let (ctx, events) = ctx();
        let (tables, mut rx) = Sink::queue("gamma", 8);
        let mut me = ours(ctx, true, Some(tables));
        me.on_command(Restore {
            on: true,
            kelvin: 4000,
        });
        let quiet = |cmd| Apply {
            cmd,
            announce: false,
        };
        me.on_command(quiet(Cmd::Toggle));
        me.on_command(quiet(Cmd::Kelvin(3000)));
        assert_eq!(warmed(&mut rx), [Some(4000), None, Some(3000)]);
        let seen = states(events);
        let set: Vec<_> = seen.iter().map(|s| (s.on, s.kelvin)).collect();
        assert_eq!(set, [(true, 4000), (false, 4000), (true, 3000)]);
        assert!(seen.iter().all(|s| s.available && s.schedule.is_none()));
    }

    /// A standalone app's system shows what it is told but leaves the screen to the dashboard.
    #[tokio::test]
    async fn only_the_one_holding_night_light_warms_the_screen() {
        let (ctx, events) = ctx();
        let (tables, mut rx) = Sink::queue("gamma", 8);
        let mut me = ours(ctx, false, Some(tables));
        me.on_command(Apply {
            cmd: Cmd::On,
            announce: false,
        });
        assert_eq!(warmed(&mut rx), []);
        let seen = states(events);
        assert_eq!(seen.len(), 1);
        assert!(seen[0].available && seen[0].on);
    }

    /// Without gamma tables, the dashboard's night light says it can't.
    #[tokio::test]
    async fn without_gamma_tables_night_light_is_unavailable() {
        let (ctx, events) = ctx();
        let mut me = ours(ctx, true, None);
        me.on_command(Restore {
            on: true,
            kelvin: 4000,
        });
        assert!(!states(events)[0].available);
    }
}

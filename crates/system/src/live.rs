//! The real system: the sources, started on their own runtime, with one actor per applet that
//! turns the applet's commands into what the sources do.

use super::{
    Capabilities, EventSender, Feed, Senders, Started, audio, bluetooth, brightness, clipboard,
    color, drives, host, keyboard, mascot, media, night_light, notifications, phones, power,
    privacy, session, sleep, tray, wayland, weather, wifi, windows,
};
use crate::compositor;
use crate::sources::{
    self, Ctx, audio::Audio, bluetooth::Bluetooth, brightness::Brightness, inhibit::Inhibit,
    kdeconnect::KdeConnect, media::Media, notifications::Notifications, tray::Tray, udisks::Drives,
    weather::Weather, wifi::Wifi,
};
use smithay_client_toolkit::reexports::calloop::channel::Sender;
use smithay_client_toolkit::reexports::client::Connection;
use std::future::Future;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

/// How the live system is started.
pub struct Options {
    /// The feeds to run; None runs all of them (the dashboard).
    pub feeds: Option<&'static [Feed]>,
    /// The dashboard's Wayland connection, for the clipboard and night light (None: neither, as
    /// in an offscreen snapshot).
    pub wayland: Option<Connection>,
    /// Hold the gamma control for night light (the dashboard does; a standalone app leaves it to
    /// the dashboard).
    pub gamma: bool,
}

/// Start the real system on a thread of its own, reporting to `events`. Returns once it is
/// running and knows which compositor this is. It stops when every sender is dropped.
pub fn live(events: EventSender, opts: Options) -> std::io::Result<Started> {
    let (started_tx, started_rx) = std::sync::mpsc::channel();
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
            let wayland = match opts.wayland {
                Some(conn) => match wayland::start(conn, events, opts.gamma, ctx.is_kwin()) {
                    Ok(w) => Some(w),
                    Err(e) => {
                        let _ = started_tx.send(Err(e));
                        return;
                    }
                },
                None => None,
            };
            let mut caps = Capabilities::for_compositor(
                compositor,
                wayland.as_ref().is_some_and(|w| w.has_clipboard),
                wayland.as_ref().is_some_and(|w| w.has_gamma),
            );
            // KWin's window list is there only if it let us bind it.
            caps.windows |= wayland.as_ref().is_some_and(|w| w.has_windows);
            let (host_tx, host_rx) = unbounded_channel();
            let senders = {
                let _in_runtime = rt.enter();
                start(&ctx, opts.feeds, host_tx, wayland)
            };
            if started_tx.send(Ok(Started { senders, caps })).is_err() {
                return;
            }
            rt.block_on(Host { ctx }.event_loop(host_rx));
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
    host: UnboundedSender<host::Command>,
    wayland: Option<wayland::Handles>,
) -> Senders {
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
    Senders {
        host,
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
        wifi: run(WifiActor(Wifi::start(feed(Feed::Wifi)))),
        bluetooth: run(BluetoothActor(Bluetooth::start(feed(Feed::Bluetooth)))),
        audio: run(AudioActor(Audio::start(feed(Feed::Audio)))),
        brightness: run(BrightnessActor(Brightness::start(feed(Feed::Brightness)))),
        notifications: run(NotificationsActor {
            notifications: Notifications::start(feed(Feed::Notifications)),
            ctx: base(),
        }),
        tray: run(TrayActor(Tray::start(feed(Feed::Tray)))),
        media: run(MediaActor(Media::start(feed(Feed::Media)))),
        power: run(PowerActor {
            inhibit: Inhibit::start(feed(Feed::Inhibit)),
            ctx: base(),
        }),
        session: run(SessionActor { ctx: base() }),
        drives: run(DrivesActor(Drives::start(feed(Feed::Drives)))),
        phones: run(PhonesActor(KdeConnect::start(feed(Feed::Phone)))),
        weather: run(WeatherActor(Weather::start(
            feed(Feed::Weather),
            locale.chrono,
        ))),
        night_light: run(NightLightActor {
            kwin: kwin.then(|| sources::kde::NightLight::start(feed(Feed::NightLight))),
            gamma: wayland.as_ref().map(|w| w.gamma.clone()),
        }),
        mascot: run(MascotActor { ctx: base() }),
        clipboard: run(ClipboardActor(wayland.map(|w| w.clipboard))),
        sleep: run(SleepActor {
            awake: sources::sleep::Awake::start(base()),
            ctx: base(),
        }),
    }
}

/// An actor: its data, and what it does with each command. The loop ends when every sender is
/// gone, and hands the actor back.
trait Actor: Sized + Send + 'static {
    type Command: Send + 'static;

    fn handle(&mut self, cmd: Self::Command);

    fn event_loop(
        mut self,
        mut rx: UnboundedReceiver<Self::Command>,
    ) -> impl Future<Output = Self> + Send {
        async move {
            while let Some(cmd) = rx.recv().await {
                self.handle(cmd);
            }
            self
        }
    }
}

/// Run `actor`'s event loop on the current runtime; returns where to send it commands.
fn run<A: Actor>(actor: A) -> UnboundedSender<A::Command> {
    let (tx, rx) = unbounded_channel();
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
            FocusedOutput { reply } => {
                let compositor = self.ctx.compositor();
                // niri's socket is synchronous.
                self.ctx.runtime().spawn_blocking(move || {
                    let _ = reply.send(compositor::focused_output(compositor));
                });
            }
        }
    }
}

struct WindowsActor {
    ctx: Ctx,
    /// KWin's window list lives on the Wayland thread.
    kwin: Option<Sender<windows::Command>>,
}

impl Actor for WindowsActor {
    type Command = windows::Command;

    fn handle(&mut self, cmd: windows::Command) {
        use windows::Command::*;
        if let Some(kwin) = &self.kwin {
            let _ = kwin.send(cmd);
            return;
        }
        match cmd {
            Focus(id) => compositor::focus_window(&self.ctx, id),
            FocusPid(pid) => compositor::focus_pid(&self.ctx, pid),
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

struct WifiActor(Wifi);

impl Actor for WifiActor {
    type Command = wifi::Command;

    fn handle(&mut self, cmd: wifi::Command) {
        use wifi::Command::*;
        let w = &self.0;
        match cmd {
            Toggle => w.toggle(),
            Rescan => w.rescan(),
            Connect { key } => w.connect(key),
            JoinHidden {
                ssid,
                security,
                answer,
            } => w.join_hidden(ssid, security, answer),
            Answer { serial, answer } => w.answer(serial, answer),
            CancelPrompt => w.cancel_prompt(),
            Disconnect => w.disconnect(),
            Forget { uuid } => w.forget(uuid),
            SetAutoconnect { uuid, on } => w.set_autoconnect(uuid, on),
            Select { key } => w.select(key),
            Reveal { uuid } => w.reveal(uuid),
            HideSecret => w.hide_secret(),
            SignIn => w.sign_in(),
        }
    }
}

struct BluetoothActor(Bluetooth);

impl Actor for BluetoothActor {
    type Command = bluetooth::Command;

    fn handle(&mut self, cmd: bluetooth::Command) {
        use bluetooth::Command::*;
        let b = &self.0;
        match cmd {
            Toggle => b.toggle(),
            Scan(on) => b.scan(on),
            Connect(addr) => b.connect(addr),
            Disconnect(addr) => b.disconnect(addr),
            SetTrusted(addr, on) => b.set_trusted(addr, on),
            SetBlocked(addr, on) => b.set_blocked(addr, on),
            Forget(addr) => b.forget(addr),
            Rename(addr, name) => b.rename(addr, name),
            Pair(addr) => b.pair(addr),
            CancelPairing => b.cancel_pairing(),
            Answer { serial, answer } => b.answer(serial, answer),
            Decline => b.decline(),
            Select(addr) => b.select(addr),
        }
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

struct NotificationsActor {
    notifications: Notifications,
    ctx: Ctx,
}

impl Actor for NotificationsActor {
    type Command = notifications::Command;

    fn handle(&mut self, cmd: notifications::Command) {
        use notifications::Command::*;
        let n = &self.notifications;
        match cmd {
            SetDnd(dnd) => n.set_dnd(dnd),
            Dismiss(id) => n.dismiss(id),
            Clear => n.clear(),
            Invoke(id) => n.invoke(id),
            AddLocal { app, summary, body } => n.add_local(&app, &summary, &body),
            Osd { text, icon } => n.osd().show(&self.ctx, &text, &icon),
        }
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

struct MediaActor(Media);

impl Actor for MediaActor {
    type Command = media::Command;

    fn handle(&mut self, cmd: media::Command) {
        use media::Command::*;
        match cmd {
            PlayPause => self.0.play_pause(),
            Next => self.0.next(),
            Previous => self.0.previous(),
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

struct DrivesActor(Drives);

impl Actor for DrivesActor {
    type Command = drives::Command;

    fn handle(&mut self, cmd: drives::Command) {
        use drives::Command::*;
        match cmd {
            Mount(block) => self.0.mount(block),
            Unmount(block) => self.0.unmount(block),
            Eject(drive) => self.0.eject(drive),
            EjectAll => self.0.eject_all(),
            Open(mount) => self.0.open(mount),
        }
    }
}

struct PhonesActor(KdeConnect);

impl Actor for PhonesActor {
    type Command = phones::Command;

    fn handle(&mut self, cmd: phones::Command) {
        match cmd {
            phones::Command::Ring(id) => self.0.ring(id),
            phones::Command::SendFiles(id) => self.0.send_files(id),
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

struct NightLightActor {
    /// KWin's night light; None where it isn't KWin's.
    kwin: Option<sources::kde::NightLight>,
    /// The Wayland side's gamma tables; None without Wayland.
    gamma: Option<Sender<Option<u32>>>,
}

impl Actor for NightLightActor {
    type Command = night_light::Command;

    fn handle(&mut self, cmd: night_light::Command) {
        match cmd {
            night_light::Command::Gamma(want) => {
                if let Some(gamma) = &self.gamma {
                    let _ = gamma.send(want);
                }
            }
            night_light::Command::Kwin { cmd, announce } => {
                if let Some(kwin) = &self.kwin {
                    kwin.apply(cmd, announce);
                }
            }
        }
    }
}

/// Hands clipboard commands to the Wayland side's event loop; None without Wayland, where what
/// is copied goes nowhere.
struct ClipboardActor(Option<Sender<clipboard::Command>>);

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

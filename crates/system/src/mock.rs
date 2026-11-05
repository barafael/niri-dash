//! A system that isn't there: made-up but believable states for every applet, and an actor that
//! answers commands the way the real system would where the dashboard depends on it (a slider
//! moves, a device connects, a picked color comes back). For screenshots and for trying the
//! dashboard without the services behind it.

use crate::{
    Capabilities, Compositor, Event, EventSender, IconSrc, Inboxes, audio, bluetooth, brightness,
    claude, clipboard, color, drives, host, keyboard, mascot, media, night_light, notifications,
    perf, phones, power, privacy, session, sleep, tray, weather, wifi, windows,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::{Started, channels};

/// Start a mock system on a thread of its own, reporting to `events`.
#[cfg(not(target_arch = "wasm32"))]
pub fn mock(events: EventSender) -> std::io::Result<Started> {
    let (senders, inboxes) = channels();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    std::thread::Builder::new()
        .name("niri-dash-mock".into())
        .spawn(move || rt.block_on(MockSystem::new(Demo::new(), events).event_loop(inboxes)))?;
    Ok(Started {
        senders,
        caps: MockSystem::CAPS,
    })
}

/// A command the mock received, of whichever applet.
#[derive(Debug)]
pub enum Received {
    Host(host::Command),
    Windows(windows::Command),
    Keyboard(keyboard::Command),
    Privacy(privacy::Command),
    Color(color::Command),
    Wifi(wifi::Command),
    Bluetooth(bluetooth::Command),
    Audio(audio::Command),
    Brightness(brightness::Command),
    Notifications(notifications::Command),
    Tray(tray::Command),
    Media(media::Command),
    Power(power::Command),
    Session(session::Command),
    Drives(drives::Command),
    Phones(phones::Command),
    Weather(weather::Command),
    NightLight(night_light::Command),
    Mascot(mascot::Command),
    Clipboard(clipboard::Command),
    Sleep(sleep::Command),
}

impl Inboxes {
    /// The next command of any applet; None when every sender is gone. Each applet's commands
    /// keep their order; between applets there is none.
    pub async fn next(&mut self) -> Option<Received> {
        Some(tokio::select! {
            Some(c) = self.host.recv() => Received::Host(c),
            Some(c) = self.windows.recv() => Received::Windows(c),
            Some(c) = self.keyboard.recv() => Received::Keyboard(c),
            Some(c) = self.privacy.recv() => Received::Privacy(c),
            Some(c) = self.color.recv() => Received::Color(c),
            Some(c) = self.wifi.recv() => Received::Wifi(c),
            Some(c) = self.bluetooth.recv() => Received::Bluetooth(c),
            Some(c) = self.audio.recv() => Received::Audio(c),
            Some(c) = self.brightness.recv() => Received::Brightness(c),
            Some(c) = self.notifications.recv() => Received::Notifications(c),
            Some(c) = self.tray.recv() => Received::Tray(c),
            Some(c) = self.media.recv() => Received::Media(c),
            Some(c) = self.power.recv() => Received::Power(c),
            Some(c) = self.session.recv() => Received::Session(c),
            Some(c) = self.drives.recv() => Received::Drives(c),
            Some(c) = self.phones.recv() => Received::Phones(c),
            Some(c) = self.weather.recv() => Received::Weather(c),
            Some(c) = self.night_light.recv() => Received::NightLight(c),
            Some(c) = self.mascot.recv() => Received::Mascot(c),
            Some(c) = self.clipboard.recv() => Received::Clipboard(c),
            Some(c) = self.sleep.recv() => Received::Sleep(c),
            else => return None,
        })
    }
}

/// The mock: what every applet shows, and every command it got, in order.
pub struct MockSystem {
    pub demo: Demo,
    pub received: Vec<Received>,
    events: EventSender,
}

impl MockSystem {
    /// It runs under niri, with everything there is.
    pub const CAPS: Capabilities = Capabilities::for_compositor(Compositor::Niri, true, true);

    pub fn new(demo: Demo, events: EventSender) -> Self {
        Self {
            demo,
            received: Vec::new(),
            events,
        }
    }

    /// Answer commands until every sender is gone; hands back what it saw.
    pub async fn event_loop(mut self, mut inbox: Inboxes) -> Self {
        while let Some(cmd) = inbox.next().await {
            let cmd = self.handle(cmd);
            self.received.push(cmd);
        }
        self
    }

    fn send(&self, event: impl Into<Event>) {
        let _ = self.events.send(event.into());
    }

    /// Everything the dashboard shows, as the real sources send it when it comes up.
    fn send_all(&self) {
        let d = &self.demo;
        self.send(perf::Event::Snapshot(Box::new(d.perf.clone())));
        self.send(claude::Event::Sessions(d.claude.clone()));
        self.send(windows::Event::State(d.windows.clone()));
        self.send(keyboard::Event::Layouts(Some(d.layouts.clone())));
        self.send(privacy::Event::Usage(d.privacy.clone()));
        self.send(privacy::Event::Casts(Vec::new()));
        self.send(wifi::Event::State(Box::new(d.wifi.clone())));
        self.send(bluetooth::Event::State(Box::new(d.bluetooth.clone())));
        self.send(audio::Event::State(d.audio.clone()));
        self.send(brightness::Event::Level(Some(d.brightness)));
        self.send(notifications::Event::State(d.notifications.clone()));
        self.send(tray::Event::Items(d.tray.clone()));
        self.send(media::Event::State(Some(d.media.clone())));
        self.send(power::Event::State(d.power.clone()));
        self.send(power::Event::Inhibited(d.inhibited));
        self.send(power::Event::SleepBlockers(Vec::new()));
        self.send(drives::Event::State(d.drives.clone()));
        self.send(phones::Event::State(d.phones.clone()));
        self.send(weather::Event::State(d.weather.clone()));
    }

    /// Do what the real system would, as far as the dashboard can tell; returns the command to
    /// keep.
    fn handle(&mut self, cmd: Received) -> Received {
        use Received as R;
        let d = &mut self.demo;
        match cmd {
            R::Host(host::Command::FocusedOutput { reply }) => {
                let _ = reply.send(None);
                // The reply is spent; keep a record of the question.
                return R::Host(host::Command::Refresh);
            }
            R::Host(host::Command::Visible(true) | host::Command::Refresh) => self.send_all(),
            R::Color(color::Command::Pick) => {
                self.send(color::Event::Picked(Some([0.96, 0.62, 0.04])));
            }
            R::Wifi(ref c) => {
                match c {
                    wifi::Command::Toggle => d.wifi.enabled = !d.wifi.enabled,
                    wifi::Command::Connect { key } => {
                        let ssid = key.rsplit_once(':').map_or(key.as_str(), |(_, s)| s);
                        d.wifi.ssid = ssid.to_string();
                        for n in &mut d.wifi.networks {
                            n.active = n.ssid == d.wifi.ssid;
                        }
                    }
                    wifi::Command::Disconnect => {
                        d.wifi.ssid.clear();
                        for n in &mut d.wifi.networks {
                            n.active = false;
                        }
                    }
                    _ => {}
                }
                self.send(wifi::Event::State(Box::new(self.demo.wifi.clone())));
            }
            R::Bluetooth(ref c) => {
                let b = &mut d.bluetooth;
                match c {
                    bluetooth::Command::Toggle => b.powered = !b.powered,
                    bluetooth::Command::Scan(on) => b.discovering = *on,
                    bluetooth::Command::Connect(addr) | bluetooth::Command::Disconnect(addr) => {
                        let on = matches!(c, bluetooth::Command::Connect(_));
                        for dev in b.devices.iter_mut().filter(|dev| dev.address == *addr) {
                            dev.connected = on;
                        }
                    }
                    _ => {}
                }
                self.send(bluetooth::Event::State(Box::new(
                    self.demo.bluetooth.clone(),
                )));
            }
            R::Audio(ref c) => {
                let a = &mut d.audio;
                match *c {
                    audio::Command::SetVolume(v) => a.volume = v,
                    audio::Command::ToggleMute => a.muted = !a.muted,
                    audio::Command::SetMicVolume(v) => {
                        a.mic = a.mic.map(|(_, muted)| (v, muted));
                    }
                    audio::Command::ToggleMicMute => a.mic = a.mic.map(|(v, m)| (v, !m)),
                    _ => {}
                }
                self.send(audio::Event::State(self.demo.audio.clone()));
            }
            R::Brightness(brightness::Command::Set(v)) => {
                d.brightness = v;
                self.send(brightness::Event::Level(Some(v)));
            }
            R::Notifications(ref c) => {
                let n = &mut d.notifications;
                match c {
                    notifications::Command::Dismiss(id) => n.items.retain(|x| x.id != *id),
                    notifications::Command::Clear => n.items.clear(),
                    notifications::Command::SetDnd(dnd) => n.dnd = *dnd,
                    _ => {}
                }
                self.send(notifications::Event::State(self.demo.notifications.clone()));
            }
            R::Media(media::Command::PlayPause) => {
                d.media.playing = !d.media.playing;
                self.send(media::Event::State(Some(self.demo.media.clone())));
            }
            R::Power(ref c) => {
                match *c {
                    power::Command::SetProfile(p) => d.power.profile = Some(p),
                    power::Command::ToggleInhibit => d.inhibited = !d.inhibited,
                }
                self.send(power::Event::State(self.demo.power.clone()));
                self.send(power::Event::Inhibited(self.demo.inhibited));
            }
            R::Drives(ref c) => {
                for v in d.drives.drives.iter_mut().flat_map(|d| &mut d.volumes) {
                    match c {
                        drives::Command::Mount(block) if v.path == *block => {
                            v.mount = Some(format!("/run/media/user/{}", v.name));
                        }
                        drives::Command::Unmount(block) if v.path == *block => v.mount = None,
                        _ => {}
                    }
                }
                self.send(drives::Event::State(self.demo.drives.clone()));
            }
            R::Weather(weather::Command::Configure(_)) => {
                self.send(weather::Event::State(self.demo.weather.clone()));
            }
            R::Mascot(mascot::Command::Run { id, .. }) => {
                // Nothing printed: the mascot says something of its own.
                self.send(mascot::Event::Output { id, stdout: None });
            }
            R::Sleep(sleep::Command::Detect { .. }) => {
                // A laptop that can suspend and hibernate, and a script of its own.
                use sleep::Mode::*;
                self.send(sleep::Event::Modes(vec![
                    Suspend, Hibernate, Poweroff, RunScript,
                ]));
            }
            R::Sleep(sleep::Command::Run { mode, .. }) => {
                // The mock sleeps through nothing: it only says it did.
                self.send(sleep::Event::Done { mode, error: None });
            }
            _ => {}
        }
        cmd
    }
}

/// What every applet shows in the mock.
#[derive(Debug, Clone)]
pub struct Demo {
    pub perf: perf::Snapshot,
    pub claude: claude::State,
    pub windows: windows::State,
    pub layouts: keyboard::Layouts,
    pub privacy: privacy::Usage,
    pub wifi: wifi::State,
    pub bluetooth: bluetooth::State,
    pub audio: audio::State,
    pub brightness: f32,
    pub notifications: notifications::State,
    pub tray: Vec<tray::Item>,
    pub media: media::State,
    pub power: power::State,
    pub inhibited: bool,
    pub drives: drives::State,
    pub phones: phones::State,
    pub weather: weather::State,
}

impl Default for Demo {
    fn default() -> Self {
        Self::new()
    }
}

impl Demo {
    pub fn new() -> Self {
        Self {
            perf: demo_perf(),
            claude: demo_claude(),
            windows: demo_windows(),
            layouts: keyboard::Layouts::from_names(["English (US)", "German"], 0),
            privacy: Default::default(),
            wifi: demo_wifi(),
            bluetooth: demo_bluetooth(),
            audio: demo_audio(),
            brightness: 0.7,
            notifications: demo_notifications(),
            tray: demo_tray(),
            media: media::State {
                bus_name: "org.mpris.MediaPlayer2.demo".into(),
                player: "Music".into(),
                title: "Clair de lune".into(),
                artist: "Claude Debussy".into(),
                playing: true,
                art: None,
                progress: 0.35,
            },
            power: power::State {
                has_battery: true,
                percent: 76,
                charging: false,
                status_text: "3 h 10 min left".into(),
                power_text: "9.4 W".into(),
                profile: Some(power::Profile::Balanced),
                profiles: vec![
                    power::Profile::PowerSaver,
                    power::Profile::Balanced,
                    power::Profile::Performance,
                ],
                peripherals: Vec::new(),
            },
            inhibited: false,
            drives: demo_drives(),
            phones: phones::State {
                available: true,
                phones: vec![phones::Phone {
                    id: "pixel".into(),
                    name: "Pixel".into(),
                    kind: "smartphone".into(),
                    reachable: true,
                    charge: Some(64),
                    charging: false,
                    can_ring: true,
                    can_share: true,
                }],
            },
            weather: demo_weather(),
        }
    }
}

fn history(len: usize, f: impl Fn(f32) -> f32) -> Vec<f32> {
    (0..len).map(|i| f(i as f32).clamp(0.0, 1.0)).collect()
}

fn demo_perf() -> perf::Snapshot {
    let wave = |base: f32, amp: f32| move |i: f32| base + amp * (i / 5.0).sin();
    perf::Snapshot {
        cpu: 0.18,
        mem: 0.42,
        cpu_history: history(60, wave(0.18, 0.08)),
        mem_history: history(60, wave(0.42, 0.01)),
        cpu_text: "18%".into(),
        mem_text: "6.7 GiB / 16 GiB".into(),
        temp_text: "48 °C".into(),
        net_text: "↓ 1.2 MB/s ↑ 64 kB/s".into(),
        load_text: "load 1.02 0.88 0.71".into(),
        cores: vec![0.2, 0.1, 0.35, 0.05, 0.12, 0.4, 0.08, 0.15],
        mem_used_text: "6.7 GiB".into(),
        mem_avail_text: "9.3 GiB".into(),
        swap: 0.05,
        swap_text: "swap 0.4 GiB".into(),
        top_mem: vec![
            ("firefox".into(), "1.9 GiB".into()),
            ("code".into(), "1.1 GiB".into()),
            ("niri".into(), "180 MiB".into()),
        ],
        net_rx_history: history(60, wave(0.4, 0.3)),
        net_tx_history: history(60, wave(0.1, 0.05)),
        net_rx_text: "1.2 MB/s".into(),
        net_tx_text: "64 kB/s".into(),
        net_peak_text: "peak 4.8 MB/s".into(),
        net_iface: "wlan0".into(),
        net_addr: "192.168.1.23".into(),
        net_rx_frac: 0.25,
        net_tx_frac: 0.02,
        disks: vec![
            perf::DiskRow {
                mount: "/".into(),
                system: true,
                used: 0.61,
                text: "290 GiB / 476 GiB".into(),
            },
            perf::DiskRow {
                mount: "/home/user/data".into(),
                system: false,
                used: 0.34,
                text: "310 GiB / 916 GiB".into(),
            },
        ],
        disk_history: history(60, wave(0.1, 0.1)),
        disk_read_text: "read 3.1 MB/s".into(),
        disk_write_text: "write 420 kB/s".into(),
        root_used: 0.61,
        gpu: Some(perf::Gpu {
            name: "Radeon 780M".into(),
            busy: 0.09,
            history: history(60, wave(0.09, 0.05)),
            vram: 0.25,
            vram_text: "1.0 GiB / 4.0 GiB".into(),
            temp_text: "45 °C".into(),
            power_text: "8.2 W".into(),
        }),
    }
}

fn demo_claude() -> claude::State {
    let session = |pid, title: &str, status: claude::Status, text: &str| claude::Session {
        pid,
        title: title.into(),
        place: "~/code/niri-dash".into(),
        status,
        status_text: text.into(),
        window: Some(pid as u64),
    };
    claude::State {
        sessions: vec![
            session(
                4101,
                "Split off the system",
                claude::Status::Busy,
                "working · 12 min",
            ),
            session(
                4102,
                "Keyboard navigation",
                claude::Status::Idle,
                "waiting for you · 3 min",
            ),
        ],
        busy: 1,
    }
}

fn demo_windows() -> windows::State {
    let row = |id, header, title: &str, app: &str, focused| windows::Row {
        id,
        header,
        title: title.into(),
        app: app.into(),
        icon: IconSrc::None,
        focused,
        urgent: false,
    };
    windows::State {
        connected: true,
        rows: vec![
            row(0, true, "1", "", false),
            row(11, false, "niri-dash — Code", "code", true),
            row(12, false, "Mozilla Firefox", "firefox", false),
            row(0, true, "2", "", false),
            row(21, false, "~/code/niri-dash", "Alacritty", false),
        ],
        count: 3,
        focused_icon: None,
    }
}

fn demo_wifi() -> wifi::State {
    let net = |ssid: &str, signal, security, active| wifi::Network {
        ssid: ssid.into(),
        signal,
        security,
        wpa2: security == wifi::Security::Personal,
        bands: 2,
        aps: 1,
        active,
        ..Default::default()
    };
    let mut s = wifi::State::default();
    s.available = true;
    s.enabled = true;
    s.ssid = "Lighthouse".into();
    s.signal = 78;
    s.networks = vec![
        net("Lighthouse", 78, wifi::Security::Personal, true),
        net("Harbour Café", 54, wifi::Security::Open, false),
        net("FRITZ!Box 7590 QX", 31, wifi::Security::Personal, false),
    ];
    s
}

fn demo_bluetooth() -> bluetooth::State {
    let dev = |address: &str, name: &str, kind, connected, battery| bluetooth::Device {
        path: format!("/org/bluez/hci0/dev_{}", address.replace(':', "_")),
        address: address.into(),
        name: name.into(),
        claimed: name.into(),
        named: true,
        kind,
        paired: true,
        trusted: true,
        blocked: false,
        connected,
        legacy: false,
        random_address: false,
        battery,
        rssi: None,
        profiles: Vec::new(),
        busy: false,
    };
    bluetooth::State {
        available: true,
        powered: true,
        adapter: "/org/bluez/hci0".into(),
        adapter_name: "laptop".into(),
        devices: vec![
            dev(
                "AC:80:0A:11:22:33",
                "Fairbuds",
                power::DeviceKind::Headphones,
                true,
                80,
            ),
            dev(
                "F4:73:35:44:55:66",
                "MX Keys",
                power::DeviceKind::Keyboard,
                false,
                -1,
            ),
        ],
        ..Default::default()
    }
}

fn demo_audio() -> audio::State {
    audio::State {
        volume: 0.55,
        muted: false,
        mic: Some((0.7, false)),
        sink_name: "Speakers".into(),
        sinks: Vec::new(),
        streams: Vec::new(),
    }
}

fn demo_notifications() -> notifications::State {
    let at = |minutes| chrono::Local::now() - chrono::Duration::minutes(minutes);
    let note = |id, app: &str, summary: &str, body: &str, minutes| notifications::Notification {
        id,
        app: app.into(),
        summary: summary.into(),
        body: body.into(),
        icon: IconSrc::None,
        actions: Vec::new(),
        urgent: false,
        time: at(minutes),
    };
    notifications::State {
        server: true,
        can_pause: true,
        items: vec![
            note(
                2,
                "Calendar",
                "Stand-up in 10 minutes",
                "Room 3 and online",
                2,
            ),
            note(1, "Mail", "Rafael", "The build is green again.", 25),
        ],
        dnd: notifications::Dnd::Off,
        missed: 0,
    }
}

fn demo_tray() -> Vec<tray::Item> {
    let item = |address: &str, title: &str| tray::Item {
        address: address.into(),
        title: title.into(),
        icon: IconSrc::None,
        has_menu: true,
        item_is_menu: false,
    };
    vec![
        item(":1.40/StatusNotifierItem", "Nextcloud"),
        item(":1.52/StatusNotifierItem", "KeePassXC"),
    ]
}

fn demo_drives() -> drives::State {
    drives::State {
        available: true,
        drives: vec![drives::Drive {
            path: "/org/freedesktop/UDisks2/drives/SanDisk".into(),
            name: "SanDisk Ultra".into(),
            size: 64_000_000_000,
            can_power_off: true,
            ejectable: true,
            volumes: vec![drives::Volume {
                path: "/dev/sda1".into(),
                name: "STICK".into(),
                size: 64_000_000_000,
                mount: Some("/run/media/user/STICK".into()),
                locked: false,
                backing: None,
            }],
        }],
    }
}

fn demo_weather() -> weather::State {
    let day = |name: &str, icon, max: &str, min: &str| weather::Day {
        name: name.into(),
        icon,
        max: max.into(),
        min: min.into(),
    };
    weather::State {
        place: "Hamburg".into(),
        temp: "12°".into(),
        feels: "feels like 9°".into(),
        description: "Partly cloudy".into(),
        icon: weather::Icon::CloudSun,
        details: "14° / 7° · 71% humidity · wind 18 km/h".into(),
        days: vec![
            day("Wed", weather::Icon::Rain, "11°", "6°"),
            day("Thu", weather::Icon::CloudSun, "13°", "7°"),
            day("Fri", weather::Icon::Sun, "15°", "8°"),
            day("Sat", weather::Icon::Cloud, "12°", "6°"),
            day("Sun", weather::Icon::Rain, "10°", "5°"),
        ],
        error: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EventSender, Tell, channels};

    /// Commands queued before the loop runs, then the senders dropped: the loop answers them
    /// all and stops. Each applet's commands keep their order; between applets there is none
    /// (they are separate channels, as with the live system's actors).
    #[tokio::test]
    async fn answers_and_records() {
        let (events_tx, events_rx) = EventSender::channel();
        let (tx, inbox) = channels();
        tx.brightness.tell(brightness::Command::Set(0.3));
        tx.brightness.tell(brightness::Command::Set(0.6));
        tx.audio.tell(audio::Command::ToggleMute);
        tx.color.tell(color::Command::Pick);
        drop(tx);
        let mock = MockSystem::new(Demo::new(), events_tx)
            .event_loop(inbox)
            .await;
        assert_eq!(mock.received.len(), 4);
        assert_eq!(mock.demo.brightness, 0.6);
        assert!(mock.demo.audio.muted);

        let seen: Vec<Event> = events_rx.try_iter().collect();
        let levels: Vec<f32> = seen
            .iter()
            .filter_map(|e| match e {
                Event::Brightness(brightness::Event::Level(l)) => *l,
                _ => None,
            })
            .collect();
        assert_eq!(levels, [0.3, 0.6]);
        assert!(
            seen.iter()
                .any(|e| matches!(e, Event::Audio(audio::Event::State(s)) if s.muted))
        );
        assert!(
            seen.iter()
                .any(|e| matches!(e, Event::Color(color::Event::Picked(Some(_)))))
        );
    }
}

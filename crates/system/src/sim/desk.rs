//! The simulated desk: windows, notifications, the media player, the mixer, the tray, drives,
//! phones and the clipboard. What they show moves with the time; what someone did to them stays
//! done.

use super::art::{self, Icons};
use super::time::{Scene, hash, into, mix, scene, slot};
use super::world::{local, sharing};
use crate::{
    IconSrc, audio, clipboard, drives, media, notifications, phones, privacy, tray, windows,
};
use std::collections::HashMap;
use std::sync::Arc;

/// A window that exists at some point in this world.
struct Win {
    id: u64,
    workspace: usize,
    app: &'static str,
    /// When it is open.
    open: fn(f64) -> bool,
    title: fn(f64) -> String,
}

fn always(_: f64) -> bool {
    true
}

const PAGES: [&str; 6] = [
    "Slint Language Documentation",
    "niri: Configuration Overview",
    "The Rust Programming Language",
    "Open-Meteo: Free Weather API",
    "WebGL2 Fundamentals",
    "Actors with Tokio",
];
const SOURCES: [&str; 5] = ["app.rs", "shell.rs", "layout.rs", "nav.rs", "theme.rs"];

const WINDOWS: [Win; 8] = [
    Win {
        id: 11,
        workspace: 1,
        app: "code",
        open: always,
        title: |t| {
            format!(
                "{} — niri-dash — Code",
                SOURCES[slot(t, 83.0, SOURCES.len())]
            )
        },
    },
    Win {
        id: 12,
        workspace: 1,
        app: "firefox",
        open: always,
        title: |t| format!("{} — Mozilla Firefox", PAGES[slot(t, 67.0, PAGES.len())]),
    },
    Win {
        id: 21,
        workspace: 2,
        app: "Alacritty",
        open: always,
        title: |t| match scene(t) {
            Scene::Building => "cargo build --release".into(),
            _ => "~/code/niri-dash".into(),
        },
    },
    Win {
        id: 22,
        workspace: 2,
        app: "Alacritty",
        open: always,
        title: |_| "claude · niri-dash".into(),
    },
    Win {
        id: 23,
        workspace: 2,
        app: "Alacritty",
        open: always,
        title: |_| "claude · niri-dash (2)".into(),
    },
    Win {
        id: 24,
        workspace: 3,
        app: "Alacritty",
        open: always,
        title: |_| "claude · kdl-parser".into(),
    },
    Win {
        id: 31,
        workspace: 3,
        app: "thunderbird",
        open: always,
        title: |t| format!("Inbox ({}) — Thunderbird", slot(t, 197.0, 7)),
    },
    Win {
        id: 32,
        workspace: 4,
        app: "firefox",
        open: |t| scene(t) == Scene::Call,
        title: |_| "Stand-up — Video call — Mozilla Firefox".into(),
    },
];

/// Which windows get the focus during each scene.
fn usual_focus(s: Scene) -> &'static [u64] {
    match s {
        Scene::Coding => &[11, 11, 22, 21, 11, 12],
        Scene::Building => &[21, 21, 11],
        Scene::Browsing => &[12, 12, 31],
        Scene::Call => &[32, 32, 12],
        Scene::Idle => &[31, 22],
    }
}

/// Windows: their schedule, and what someone focused or closed.
#[derive(Default)]
pub struct Windows {
    /// Focused by hand, until the next time the focus moves by itself.
    focused: Option<(u64, f64)>,
    /// Closed by hand: reopened after a while, as closed apps tend to be.
    closed: HashMap<u64, f64>,
}

impl Windows {
    pub fn handle(&mut self, cmd: windows::Command, t: f64) {
        match cmd {
            windows::Command::Focus(id) => self.focused = Some((id, t + 45.0)),
            windows::Command::FocusPid(_) | windows::Command::ShowTab(_) => {}
            windows::Command::Close(id) => {
                self.closed.insert(id, t + 240.0);
            }
        }
    }

    fn open(&self, w: &Win, t: f64) -> bool {
        (w.open)(t) && self.closed.get(&w.id).is_none_or(|until| t >= *until)
    }

    pub fn state(&self, t: f64, icons: &mut Icons) -> windows::State {
        let open: Vec<&Win> = WINDOWS.iter().filter(|w| self.open(w, t)).collect();
        let by_hand = self
            .focused
            .filter(|(id, until)| t < *until && open.iter().any(|w| w.id == *id))
            .map(|(id, _)| id);
        let usual = usual_focus(scene(t));
        let focus = by_hand.or_else(|| {
            let wanted = usual[slot(t, 37.0, usual.len())];
            open.iter()
                .find(|w| w.id == wanted)
                .or(open.first())
                .map(|w| w.id)
        });
        let mut rows = Vec::new();
        for workspace in 1..=4 {
            let here: Vec<&&Win> = open.iter().filter(|w| w.workspace == workspace).collect();
            if here.is_empty() {
                continue;
            }
            rows.push(windows::Row {
                id: 0,
                header: true,
                title: workspace.to_string(),
                app: String::new(),
                icon: IconSrc::None,
                focused: false,
                urgent: false,
            });
            for w in here {
                rows.push(windows::Row {
                    id: w.id,
                    header: false,
                    title: (w.title)(t),
                    app: w.app.into(),
                    icon: icons.get(w.app),
                    focused: Some(w.id) == focus,
                    urgent: w.id == 31 && Some(w.id) != focus && into(t, 197.0) < 30.0,
                });
            }
        }
        windows::State {
            connected: true,
            count: open.len(),
            focused_icon: open
                .iter()
                .find(|w| Some(w.id) == focus)
                .map(|w| icons.get(w.app)),
            rows,
        }
    }
}

/// What the screen is shared with, unless someone stopped it for the rest of this call.
#[derive(Default)]
pub struct Casts {
    stopped_until: f64,
}

impl Casts {
    pub fn stop(&mut self, t: f64) {
        self.stopped_until = t + 300.0;
    }

    pub fn state(&self, t: f64) -> Vec<privacy::ScreenCast> {
        if !sharing(t) || t < self.stopped_until {
            return Vec::new();
        }
        vec![privacy::ScreenCast {
            session_id: 77,
            target: "eDP-1".into(),
            active: true,
            pid: Some(9021),
            process: Some("firefox".into()),
            pw_node: Some(88),
        }]
    }
}

/// What comes in over the day, one after the other.
const FEED: [(&str, &str, &str, bool); 16] = [
    (
        "Calendar",
        "Stand-up in 10 minutes",
        "Room 3 and online",
        false,
    ),
    (
        "Thunderbird",
        "Lena Vogt",
        "The figures are in the shared folder. Could you check Q3?",
        false,
    ),
    (
        "Build",
        "cargo build finished",
        "niri-dash in 38.2 s, no warnings",
        false,
    ),
    (
        "Firefox",
        "Download finished",
        "niri-v26.04-x86_64.tar.gz",
        false,
    ),
    ("Signal", "Tom", "Lunch at the usual place?", false),
    (
        "System",
        "Update available",
        "14 packages can be updated",
        false,
    ),
    (
        "Nextcloud",
        "3 files synchronized",
        "Documents/Invoices",
        false,
    ),
    (
        "Calendar",
        "Dentist tomorrow",
        "09:30, Hafenstraße 12",
        false,
    ),
    (
        "Thunderbird",
        "GitHub",
        "Your pull request was merged",
        false,
    ),
    ("Battery", "Battery low", "20 % left. Plug in soon.", true),
    ("Signal", "Mira", "Sent a photo", false),
    (
        "Build",
        "cargo test failed",
        "2 of 214 tests failed in tests/ui.rs",
        true,
    ),
    (
        "Backup",
        "Backup complete",
        "42.1 GiB to Attic-NAS in 6 min",
        false,
    ),
    (
        "Firefox",
        "Tab is using a lot of memory",
        "A page is slowing things down",
        false,
    ),
    ("KDE Connect", "Pixel", "Battery at 15 %", false),
    (
        "Claude Code",
        "Claude is waiting for you",
        "~/code/niri-dash",
        false,
    ),
];

const NOTE_EVERY: f64 = 83.0;
/// As many as a history that has been running for a while.
const NOTE_START: u64 = 6;

/// The notification history: it fills by itself and empties by hand.
pub struct Notifications {
    state: notifications::State,
    /// The last of the feed's slots that came in.
    last: u64,
    next_local: u32,
}

impl Notifications {
    pub fn new(t: f64, icons: &mut Icons) -> Self {
        let now = (t / NOTE_EVERY) as u64;
        let mut me = Self {
            state: notifications::State {
                server: true,
                can_pause: true,
                ..Default::default()
            },
            last: now - NOTE_START,
            next_local: 1 << 30,
        };
        me.advance(t, icons);
        me
    }

    fn note(n: u64, icons: &mut Icons) -> notifications::Notification {
        let (app, summary, body, urgent) = FEED[(n % FEED.len() as u64) as usize];
        notifications::Notification {
            id: n as u32,
            app: app.into(),
            summary: summary.into(),
            body: body.into(),
            icon: icons.get(app),
            actions: vec![("default".into(), "Open".into())],
            urgent,
            time: local(n as f64 * NOTE_EVERY),
        }
    }

    /// What came in since the last look. Returns whether anything did.
    pub fn advance(&mut self, t: f64, icons: &mut Icons) -> bool {
        let now = (t / NOTE_EVERY) as u64;
        let fresh = now > self.last;
        for n in self.last + 1..=now {
            self.state.items.insert(0, Self::note(n, icons));
            if self.state.dnd.active(local(t)) {
                self.state.missed += 1;
            }
        }
        self.last = self.last.max(now);
        self.state.items.truncate(40);
        fresh
    }

    pub fn state(&self) -> notifications::State {
        self.state.clone()
    }

    /// Returns a message to show, if the command calls for one.
    pub fn handle(
        &mut self,
        cmd: notifications::Command,
        t: f64,
        icons: &mut Icons,
    ) -> Option<String> {
        use notifications::Command as C;
        match cmd {
            C::SetDnd(dnd) => {
                self.state.dnd = dnd;
                if !dnd.active(local(t)) {
                    self.state.missed = 0;
                }
            }
            C::Dismiss(id) => self.state.items.retain(|n| n.id != id),
            C::Clear => self.state.items.clear(),
            C::Invoke(id) => {
                let app = self.state.items.iter().find(|n| n.id == id)?.app.clone();
                self.state.items.retain(|n| n.id != id);
                return Some(format!("{app} would open now"));
            }
            C::AddLocal { app, summary, body } => {
                self.next_local += 1;
                self.state.items.insert(
                    0,
                    notifications::Notification {
                        id: self.next_local,
                        icon: icons.get(&app),
                        app,
                        summary,
                        body,
                        actions: Vec::new(),
                        urgent: false,
                        time: local(t),
                    },
                );
            }
            C::Osd { text, .. } => return Some(text),
        }
        None
    }
}

/// Public-domain music, with the length of a typical recording in seconds.
const PLAYLIST: [(&str, &str, f64); 8] = [
    ("Clair de lune", "Claude Debussy", 312.0),
    ("Gymnopédie No. 1", "Erik Satie", 186.0),
    ("Prelude in C major", "J. S. Bach", 141.0),
    ("Nocturne Op. 9 No. 2", "Frédéric Chopin", 270.0),
    ("Morning Mood", "Edvard Grieg", 232.0),
    ("Moonlight Sonata", "Ludwig van Beethoven", 339.0),
    ("Ave Maria", "Franz Schubert", 281.0),
    ("Spring: Allegro", "Antonio Vivaldi", 205.0),
];

/// The media player. It moves on by itself, and pauses for calls.
pub struct Player {
    track: usize,
    position: f64,
    /// Paused by hand.
    paused: bool,
    last_seen: f64,
    covers: Vec<Option<IconSrc>>,
}

impl Player {
    pub fn new(t: f64) -> Self {
        let total: f64 = PLAYLIST.iter().map(|p| p.2).sum();
        let mut left = into(t, total);
        let mut track = 0;
        while left >= PLAYLIST[track].2 {
            left -= PLAYLIST[track].2;
            track += 1;
        }
        Self {
            track,
            position: left,
            paused: false,
            last_seen: t,
            covers: vec![None; PLAYLIST.len()],
        }
    }

    pub fn playing(&self, t: f64) -> bool {
        !self.paused && scene(t) != Scene::Call
    }

    pub fn advance(&mut self, t: f64) {
        let passed = (t - self.last_seen).max(0.0);
        self.last_seen = t;
        if self.playing(t) {
            self.position += passed;
        }
        while self.position >= PLAYLIST[self.track].2 {
            self.position -= PLAYLIST[self.track].2;
            self.track = (self.track + 1) % PLAYLIST.len();
        }
    }

    pub fn handle(&mut self, cmd: media::Command) {
        match cmd {
            media::Command::PlayPause => self.paused = !self.paused,
            media::Command::Next => {
                self.track = (self.track + 1) % PLAYLIST.len();
                self.position = 0.0;
            }
            media::Command::Previous => {
                if self.position > 4.0 {
                    self.position = 0.0;
                } else {
                    self.track = (self.track + PLAYLIST.len() - 1) % PLAYLIST.len();
                    self.position = 0.0;
                }
            }
        }
    }

    pub fn state(&mut self, t: f64) -> media::State {
        let (title, artist, length) = PLAYLIST[self.track];
        let cover = self.covers[self.track]
            .get_or_insert_with(|| art::cover(self.track as u64))
            .clone();
        media::State {
            bus_name: "org.mpris.MediaPlayer2.music".into(),
            player: "Music".into(),
            title: title.into(),
            artist: artist.into(),
            playing: self.playing(t),
            art: Some(cover),
            progress: (self.position / length) as f32,
        }
    }
}

const SPEAKERS: u32 = 41;
const HEADPHONES: u32 = 42;
const HDMI: u32 = 43;
const MUSIC: u32 = 61;
const BROWSER: u32 = 62;
const CALL: u32 = 63;

/// The mixer: everything in it is set by hand, but who plays comes and goes.
pub struct Mixer {
    volume: f32,
    muted: bool,
    mic: (f32, bool),
    /// The output picked by hand; otherwise the headphones while connected.
    chosen: Option<u32>,
    streams: HashMap<u32, (f32, bool)>,
}

impl Mixer {
    pub fn new() -> Self {
        Self {
            volume: 0.55,
            muted: false,
            mic: (0.7, false),
            chosen: None,
            streams: HashMap::new(),
        }
    }

    pub fn handle(&mut self, cmd: audio::Command) {
        use audio::Command as C;
        match cmd {
            C::SetVolume(v) => self.volume = v,
            C::ToggleMute => self.muted = !self.muted,
            C::SetMicVolume(v) => self.mic.0 = v,
            C::ToggleMicMute => self.mic.1 = !self.mic.1,
            C::SetStreamVolume(id, v) => self.streams.entry(id).or_insert((0.8, false)).0 = v,
            C::ToggleStreamMute(id) => {
                let s = self.streams.entry(id).or_insert((0.8, false));
                s.1 = !s.1;
            }
            C::SetDefaultSink(id) => self.chosen = Some(id),
        }
    }

    pub fn state(
        &self,
        t: f64,
        playing: bool,
        headphones: bool,
        icons: &mut Icons,
    ) -> audio::State {
        let mut outputs = vec![(SPEAKERS, "Speakers"), (HDMI, "HDMI (LG UltraFine)")];
        if headphones {
            outputs.insert(0, (HEADPHONES, "Fairbuds"));
        }
        let default = self
            .chosen
            .filter(|id| outputs.iter().any(|(o, _)| o == id))
            .unwrap_or(if headphones { HEADPHONES } else { SPEAKERS });
        let stream = |id: u32, app: &str, icons: &mut Icons| {
            let (volume, muted) = self.streams.get(&id).copied().unwrap_or((0.8, false));
            audio::Stream {
                id,
                app: app.into(),
                icon: icons.get(app),
                volume,
                muted,
            }
        };
        let mut streams = Vec::new();
        if playing {
            streams.push(stream(MUSIC, "Music", icons));
        }
        if scene(t) == Scene::Browsing && into(t, 310.0) < 150.0 {
            streams.push(stream(BROWSER, "Firefox", icons));
        }
        if scene(t) == Scene::Call {
            streams.push(stream(CALL, "Video call", icons));
        }
        audio::State {
            volume: self.volume,
            muted: self.muted,
            mic: Some(self.mic),
            sink_name: outputs
                .iter()
                .find(|(id, _)| *id == default)
                .map_or("Speakers", |(_, name)| name)
                .to_string(),
            sinks: outputs
                .iter()
                .map(|(id, name)| audio::Sink {
                    id: *id,
                    name: (*name).into(),
                    default: *id == default,
                })
                .collect(),
            streams,
        }
    }
}

/// A menu entry that can be ticked.
const fn entry(
    id: i32,
    label: &'static str,
    toggle: tray::Toggle,
) -> (i32, &'static str, tray::Toggle) {
    (id, label, toggle)
}

const NEXTCLOUD: &str = ":1.40/StatusNotifierItem";
const KEEPASS: &str = ":1.52/StatusNotifierItem";
const SYNCTHING: &str = ":1.67/StatusNotifierItem";
const SIGNAL: &str = ":1.81/StatusNotifierItem";

/// The system tray and its menus.
#[derive(Default)]
pub struct Tray {
    ticked: HashMap<(String, i32), bool>,
}

impl Tray {
    pub fn items(&self, t: f64, icons: &mut Icons) -> Vec<tray::Item> {
        let item = |address: &str, title: String, icon: IconSrc| tray::Item {
            address: address.into(),
            title,
            icon,
            has_menu: true,
            item_is_menu: false,
        };
        let syncing = into(t, 410.0) < 45.0;
        let mut items = vec![
            item(
                NEXTCLOUD,
                if syncing {
                    format!("Nextcloud — syncing {} files", slot(t, 3.0, 9) + 1)
                } else {
                    "Nextcloud — up to date".into()
                },
                icons.get("Nextcloud"),
            ),
            item(KEEPASS, "KeePassXC".into(), icons.get("KeePassXC")),
            item(SYNCTHING, "Syncthing".into(), icons.get("Syncthing")),
        ];
        // Signal sits in the tray for part of the day.
        if into(t, 1300.0) < 900.0 {
            items.push(item(SIGNAL, "Signal".into(), icons.get("Signal")));
        }
        items
    }

    fn entries(address: &str) -> (&'static str, Vec<(i32, &'static str, tray::Toggle)>) {
        use tray::Toggle::{Check, None as Plain, Radio};
        match address {
            NEXTCLOUD => (
                "Nextcloud",
                vec![
                    entry(1, "Open main dialog", Plain),
                    entry(2, "Pause synchronization", Check),
                    entry(0, "", Plain),
                    entry(3, "Settings…", Plain),
                    entry(4, "Quit Nextcloud", Plain),
                ],
            ),
            KEEPASS => (
                "KeePassXC",
                vec![
                    entry(1, "Toggle window", Plain),
                    entry(2, "Lock databases", Plain),
                    entry(0, "", Plain),
                    entry(3, "Quit", Plain),
                ],
            ),
            SYNCTHING => (
                "Syncthing",
                vec![
                    entry(1, "Open web interface", Plain),
                    entry(0, "", Plain),
                    entry(2, "Sync everything", Radio),
                    entry(3, "Sync on Wi-Fi only", Radio),
                    entry(4, "Pause all folders", Radio),
                ],
            ),
            _ => (
                "Signal",
                vec![
                    entry(1, "Show", Plain),
                    entry(2, "Mute notifications", Check),
                    entry(3, "Quit", Plain),
                ],
            ),
        }
    }

    pub fn menu(&self, address: &str, x: f32, y: f32) -> tray::Menu {
        let (title, entries) = Self::entries(address);
        tray::Menu {
            address: address.into(),
            title: title.into(),
            entries: entries
                .into_iter()
                .map(|(id, label, toggle)| tray::Entry {
                    id,
                    label: label.into(),
                    depth: 0,
                    separator: label.is_empty(),
                    enabled: true,
                    toggle,
                    checked: self
                        .ticked
                        .get(&(address.to_string(), id))
                        .copied()
                        .unwrap_or(false),
                    header: false,
                })
                .collect(),
            x,
            y,
        }
    }

    /// Returns what happened, to say so.
    pub fn activate_entry(&mut self, address: &str, id: i32) -> Option<String> {
        let (title, entries) = Self::entries(address);
        let (_, label, toggle) = entries.into_iter().find(|e| e.0 == id)?;
        match toggle {
            tray::Toggle::None => {}
            tray::Toggle::Check => {
                let on = self
                    .ticked
                    .entry((address.to_string(), id))
                    .or_insert(false);
                *on = !*on;
            }
            tray::Toggle::Radio => {
                self.ticked.retain(|(a, _), _| a != address);
                self.ticked.insert((address.to_string(), id), true);
            }
        }
        Some(format!("{title}: {label}"))
    }

    pub fn name(address: &str) -> &'static str {
        Self::entries(address).0
    }
}

/// A drive plugged in for part of a cycle, unless someone ejected it this time round.
struct Plug {
    drive: drives::Drive,
    cycle: f64,
    plugged: f64,
    ejected_until: f64,
}

/// Removable drives.
pub struct Drives {
    plugs: Vec<Plug>,
}

impl Drives {
    pub fn new() -> Self {
        const GB: u64 = 1_000_000_000;
        let volume = |path: &str, name: &str, size, mount: Option<&str>, locked| drives::Volume {
            path: path.into(),
            name: name.into(),
            size,
            mount: mount.map(str::to_string),
            locked,
            backing: None,
        };
        Self {
            plugs: vec![
                Plug {
                    drive: drives::Drive {
                        path: "/org/freedesktop/UDisks2/drives/SanDisk_Ultra".into(),
                        name: "SanDisk Ultra".into(),
                        size: 64 * GB,
                        can_power_off: true,
                        ejectable: true,
                        volumes: vec![volume(
                            "/dev/sda1",
                            "STICK",
                            64 * GB,
                            Some("/run/media/user/STICK"),
                            false,
                        )],
                    },
                    cycle: 780.0,
                    plugged: 520.0,
                    ejected_until: 0.0,
                },
                Plug {
                    drive: drives::Drive {
                        path: "/org/freedesktop/UDisks2/drives/Seagate_Expansion".into(),
                        name: "Seagate Expansion".into(),
                        size: 2000 * GB,
                        can_power_off: true,
                        ejectable: false,
                        volumes: vec![
                            volume(
                                "/dev/sdb1",
                                "Backup",
                                1500 * GB,
                                Some("/run/media/user/Backup"),
                                false,
                            ),
                            volume("/dev/sdb2", "Private", 500 * GB, None, true),
                        ],
                    },
                    cycle: 1510.0,
                    plugged: 640.0,
                    ejected_until: 0.0,
                },
            ],
        }
    }

    pub fn state(&self, t: f64) -> drives::State {
        drives::State {
            available: true,
            drives: self
                .plugs
                .iter()
                .filter(|p| t >= p.ejected_until && into(t, p.cycle) < p.plugged)
                .map(|p| p.drive.clone())
                .collect(),
        }
    }

    /// Returns a message to show, if the command calls for one.
    pub fn handle(&mut self, cmd: drives::Command, t: f64) -> Option<String> {
        use drives::Command as C;
        let eject = |p: &mut Plug, t: f64| p.ejected_until = t + p.cycle - into(t, p.cycle);
        match cmd {
            C::Mount(block) | C::Unmount(block) if block.is_empty() => {}
            C::Mount(block) => {
                let volumes = self.plugs.iter_mut().flat_map(|p| &mut p.drive.volumes);
                for v in volumes.filter(|v| v.path == block && !v.locked) {
                    v.mount = Some(format!("/run/media/user/{}", v.name));
                }
            }
            C::Unmount(block) => {
                let volumes = self.plugs.iter_mut().flat_map(|p| &mut p.drive.volumes);
                for v in volumes.filter(|v| v.path == block) {
                    v.mount = None;
                }
            }
            C::Eject(path) => {
                for p in self.plugs.iter_mut().filter(|p| p.drive.path == path) {
                    eject(p, t);
                }
                return Some("Safe to remove".into());
            }
            C::EjectAll => {
                for p in &mut self.plugs {
                    eject(p, t);
                }
                return Some("Safe to remove".into());
            }
            C::Open(mount) => return Some(format!("Files would open {mount}")),
        }
        None
    }
}

/// The phones paired through KDE Connect.
pub fn phones(t: f64) -> phones::State {
    let cycle = 5400.0;
    let at = into(t, cycle);
    let charging = at > 4000.0;
    let charge = if charging {
        mix(30.0, 100.0, (at - 4000.0) / 1400.0)
    } else {
        mix(100.0, 30.0, at / 4000.0)
    };
    phones::State {
        available: true,
        phones: vec![
            phones::Phone {
                id: "pixel".into(),
                name: "Pixel".into(),
                kind: "phone".into(),
                reachable: true,
                charge: Some(charge as i32),
                charging,
                can_ring: true,
                can_share: true,
            },
            phones::Phone {
                id: "tablet".into(),
                name: "Tablet".into(),
                kind: "tablet".into(),
                reachable: into(t, 1900.0) < 1100.0,
                charge: Some(40 + (hash(71, (t / 1900.0) as u64) * 55.0) as i32),
                charging: false,
                can_ring: true,
                can_share: true,
            },
        ],
    }
}

/// Things that get copied, in the order they get copied.
const COPIES: [&str; 9] = [
    "cargo install --path . --locked",
    "https://github.com/YaLTeR/niri/wiki/Configuration:-Key-Bindings",
    "Mod+N { spawn \"niri-dash\" \"toggle\"; }",
    "git log --oneline --graph --all",
    "#d79921",
    "impl Actor for Simulation { type Command = Received; }",
    "Hafenstraße 12, 20457 Hamburg",
    "rustup target add wasm32-unknown-unknown",
    "SELECT name, count(*) FROM widgets GROUP BY name;",
];

const COPY_EVERY: f64 = 71.0;

/// A small picture, as a screenshot on the clipboard would be.
fn picture(seed: u64) -> Option<clipboard::Clip> {
    let (width, height) = (160, 100);
    let mut data = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut data, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer
            .write_image_data(&art::cover_pixels(seed, width, height))
            .ok()?;
    }
    Some(clipboard::Clip {
        kind: clipboard::ClipKind::Png,
        data: Arc::new(data),
    })
}

fn text_clip(text: &str) -> clipboard::Clip {
    clipboard::Clip {
        kind: clipboard::ClipKind::Text,
        data: Arc::new(text.as_bytes().to_vec()),
    }
}

/// What was copied in slot `n`.
fn copied(n: u64) -> Option<clipboard::Clip> {
    let i = n % (COPIES.len() as u64 + 1);
    match COPIES.get(i as usize) {
        Some(text) => Some(text_clip(text)),
        None => picture(n),
    }
}

/// The clipboard's history so far when the page opens, oldest first.
pub fn copied_before(t: f64, how_many: u64) -> Vec<clipboard::Clip> {
    let now = (t / COPY_EVERY) as u64;
    (now.saturating_sub(how_many)..now)
        .filter_map(copied)
        .collect()
}

/// What was copied in the time from `since` to `t`.
pub fn copied_between(since: f64, t: f64) -> Vec<clipboard::Clip> {
    let (from, to) = ((since / COPY_EVERY) as u64, (t / COPY_EVERY) as u64);
    (from..to).filter_map(copied).collect()
}

/// The colors the color picker "finds" on a screen that isn't there.
pub fn picked_color(n: u64) -> [f64; 3] {
    [
        hash(72, n) * 0.8 + 0.2,
        hash(73, n) * 0.8 + 0.2,
        hash(74, n) * 0.8 + 0.2,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: f64 = 1_800_000_000.0;

    #[test]
    fn windows_come_with_their_workspaces_and_one_focus() {
        let mut icons = Icons::default();
        let w = Windows::default();
        for i in 0..3000 {
            let s = w.state(T + i as f64 * 3.0, &mut icons);
            assert_eq!(s.rows.iter().filter(|r| r.focused).count(), 1);
            assert_eq!(s.rows.iter().filter(|r| !r.header).count(), s.count);
            assert!(s.rows[0].header);
        }
    }

    #[test]
    fn a_closed_window_is_gone_for_a_while_and_focus_can_be_taken() {
        let mut icons = Icons::default();
        let mut w = Windows::default();
        w.handle(windows::Command::Close(12), T);
        assert!(w.state(T, &mut icons).rows.iter().all(|r| r.id != 12));
        assert!(
            w.state(T + 500.0, &mut icons)
                .rows
                .iter()
                .any(|r| r.id == 12)
        );
        w.handle(windows::Command::Focus(21), T);
        let focused = w.state(T + 1.0, &mut icons);
        assert!(focused.rows.iter().any(|r| r.id == 21 && r.focused));
    }

    #[test]
    fn the_window_titles_move() {
        let mut icons = Icons::default();
        let w = Windows::default();
        let titles: std::collections::HashSet<String> = (0..50)
            .flat_map(|i| {
                w.state(T + i as f64 * 60.0, &mut icons)
                    .rows
                    .into_iter()
                    .map(|r| r.title)
            })
            .collect();
        assert!(titles.len() > 10);
    }

    #[test]
    fn notifications_arrive_in_order_and_stay_dismissed() {
        let mut icons = Icons::default();
        let mut n = Notifications::new(T, &mut icons);
        let start = n.state().items.len();
        assert!(start >= NOTE_START as usize);
        assert!(!n.advance(T, &mut icons));
        assert!(n.advance(T + NOTE_EVERY, &mut icons));
        let items = n.state().items;
        assert_eq!(items.len(), start + 1);
        assert!(items[0].id > items[1].id, "newest first");
        let gone = items[0].id;
        n.handle(notifications::Command::Dismiss(gone), T, &mut icons);
        assert!(n.state().items.iter().all(|i| i.id != gone));
        n.advance(T + NOTE_EVERY, &mut icons);
        assert!(n.state().items.iter().all(|i| i.id != gone));
    }

    #[test]
    fn do_not_disturb_counts_what_it_missed() {
        let mut icons = Icons::default();
        let mut n = Notifications::new(T, &mut icons);
        n.handle(
            notifications::Command::SetDnd(notifications::Dnd::Forever),
            T,
            &mut icons,
        );
        n.advance(T + 2.0 * NOTE_EVERY, &mut icons);
        assert!(n.state().missed >= 1);
        n.handle(
            notifications::Command::SetDnd(notifications::Dnd::Off),
            T,
            &mut icons,
        );
        assert_eq!(n.state().missed, 0);
    }

    #[test]
    fn the_player_moves_on_and_can_be_told_otherwise() {
        let start = 1_000.0_f64;
        let mut p = Player::new(start);
        let first = p.state(start).title;
        // A whole playlist later, it has been through other tracks.
        let mut seen = std::collections::HashSet::from([first.clone()]);
        for i in 1..3000 {
            let t = start + i as f64;
            p.advance(t);
            let s = p.state(t);
            assert!((0.0..=1.0).contains(&s.progress));
            seen.insert(s.title);
        }
        assert!(seen.len() >= 3, "{seen:?}");
        let t = start + 3000.0;
        let title = p.state(t).title;
        p.handle(media::Command::Next);
        assert_ne!(p.state(t).title, title);
        p.handle(media::Command::PlayPause);
        assert!(!p.state(t).playing);
    }

    #[test]
    fn the_headphones_take_over_the_sound_unless_told_otherwise() {
        let mut icons = Icons::default();
        let mut m = Mixer::new();
        let with = m.state(T, true, true, &mut icons);
        assert_eq!(with.sink_name, "Fairbuds");
        assert_eq!(m.state(T, true, false, &mut icons).sink_name, "Speakers");
        m.handle(audio::Command::SetDefaultSink(HDMI));
        assert!(
            m.state(T, true, true, &mut icons)
                .sink_name
                .starts_with("HDMI")
        );
        // Whoever was picked is still the default once what they picked is gone.
        m.handle(audio::Command::SetDefaultSink(HEADPHONES));
        assert_eq!(m.state(T, true, false, &mut icons).sink_name, "Speakers");
    }

    #[test]
    fn the_mixer_keeps_what_it_was_told() {
        let mut icons = Icons::default();
        let mut m = Mixer::new();
        m.handle(audio::Command::SetVolume(0.2));
        m.handle(audio::Command::ToggleMute);
        m.handle(audio::Command::SetStreamVolume(MUSIC, 0.3));
        let s = m.state(T, true, false, &mut icons);
        assert_eq!(s.volume, 0.2);
        assert!(s.muted);
        let music = s.streams.iter().find(|x| x.id == MUSIC).expect("playing");
        assert_eq!(music.volume, 0.3);
    }

    #[test]
    fn tray_menus_remember_their_ticks() {
        let mut t = Tray::default();
        assert!(!t.menu(NEXTCLOUD, 0.0, 0.0).entries[1].checked);
        t.activate_entry(NEXTCLOUD, 2);
        assert!(t.menu(NEXTCLOUD, 0.0, 0.0).entries[1].checked);
        // Radio entries exclude each other.
        t.activate_entry(SYNCTHING, 2);
        t.activate_entry(SYNCTHING, 3);
        let ticked: Vec<i32> = t
            .menu(SYNCTHING, 0.0, 0.0)
            .entries
            .iter()
            .filter(|e| e.checked)
            .map(|e| e.id)
            .collect();
        assert_eq!(ticked, [3]);
    }

    #[test]
    fn a_stick_comes_and_goes_and_can_be_ejected() {
        let mut d = Drives::new();
        let t = 780.0 * 2_000_000.0 + 10.0;
        let stick = |d: &Drives, t| d.state(t).drives.iter().any(|x| x.name == "SanDisk Ultra");
        assert!(stick(&d, t));
        assert!(!stick(&d, t + 600.0));
        d.handle(
            drives::Command::Eject("/org/freedesktop/UDisks2/drives/SanDisk_Ultra".into()),
            t,
        );
        assert!(!stick(&d, t));
        assert!(stick(&d, t + 780.0), "plugged in again next time round");
        d.handle(drives::Command::Unmount("/dev/sda1".into()), t);
        let v = &d.plugs[0].drive.volumes[0];
        assert_eq!(v.mount, None);
    }

    #[test]
    fn clips_are_valid() {
        for n in 0..40 {
            let clip = copied(n).expect("a clip");
            match clip.kind {
                clipboard::ClipKind::Text => {
                    assert!(std::str::from_utf8(&clip.data).is_ok());
                }
                clipboard::ClipKind::Png => {
                    assert!(clipboard::decode_png(&clip.data).is_some());
                }
            }
        }
        assert_eq!(copied_between(0.0, COPY_EVERY * 3.0).len(), 3);
        assert_eq!(copied_before(COPY_EVERY * 10.0, 4).len(), 4);
    }
}

//! Glue for the newer widgets: keyboard layout, clock time zones, color picker, and the rest as
//! they come. Kept out of `app.rs` so that file stays about the dashboard itself.

use super::{App, model};
use crate::layout::Kind;
use crate::shell::Shell;
use crate::sources::notifications::{Dnd, next_hour};
use crate::sources::{Msg, clipboard, niri};
use crate::timer::{Countdown, Run, Stopwatch};
use crate::{Control, Dash};
use slint::{ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Played when the timer goes off, if it's there.
const ALARM_SOUND: &str = "/usr/share/sounds/freedesktop/stereo/alarm-clock-elapsed.oga";

/// How many picked colors are kept.
const COLORS: usize = 8;

pub(super) struct Extra {
    solo: Option<Kind>,
    state: RefCell<crate::state::State>,
    /// Waiting for the dashboard to get out of the way before picking a color.
    pending_pick: Cell<bool>,
    /// The dashboard was open when the pick started, so it comes back afterwards.
    reshow_after_pick: Cell<bool>,
    zones: Rc<VecModel<crate::ZoneList>>,
    osd: crate::sources::osd::Osd,
    countdown: RefCell<Countdown>,
    stopwatch: RefCell<Stopwatch>,
    /// Updates the timer widgets while the dashboard is up and something runs.
    timer_ticker: Rc<Timer>,
    /// Goes off when the countdown ends, shown or not.
    timer_end: Rc<Timer>,
    /// Clears the "Done" a while after the countdown went off.
    timer_done: Rc<Timer>,
    /// Note texts by note name, and the names with unsaved changes.
    notes: RefCell<std::collections::HashMap<String, String>>,
    notes_dirty: RefCell<std::collections::HashSet<String>>,
    note_texts: Rc<VecModel<slint::SharedString>>,
    note_names: Rc<VecModel<slint::SharedString>>,
    notes_save: Rc<Timer>,
    /// Per-app volume rows, and the slider being dragged (its value isn't overwritten).
    streams: Rc<VecModel<crate::AudioStream>>,
    stream_touched: Cell<Option<(u32, std::time::Instant)>>,
    /// Screencasts (from niri) and mic/camera use (from PipeWire), for the privacy widget.
    casts: RefCell<Vec<niri::ScreenCast>>,
    usage: RefCell<crate::sources::privacy::Usage>,
}

impl Extra {
    pub(super) fn new(solo: Option<Kind>) -> Self {
        let state = crate::state::load();
        let mut countdown = Countdown::default();
        if state.timer_secs > 0 {
            countdown.length = Duration::from_secs(state.timer_secs as u64);
        }
        if let Some(end) = state.timer_end {
            let end = UNIX_EPOCH + Duration::from_millis(end.max(0) as u64);
            if end > SystemTime::now() {
                countdown.state = Run::Running { end };
            }
        }
        Self {
            solo,
            countdown: RefCell::new(countdown),
            stopwatch: Default::default(),
            timer_ticker: Default::default(),
            timer_end: Default::default(),
            timer_done: Default::default(),
            notes: Default::default(),
            notes_dirty: Default::default(),
            note_texts: Rc::new(VecModel::default()),
            note_names: Rc::new(VecModel::default()),
            notes_save: Default::default(),
            streams: Rc::new(VecModel::default()),
            stream_touched: Cell::new(None),
            casts: Default::default(),
            usage: Default::default(),
            state: RefCell::new(state),
            pending_pick: Cell::new(false),
            reshow_after_pick: Cell::new(false),
            zones: Rc::new(VecModel::default()),
            osd: Default::default(),
        }
    }

    /// Change the saved state and write it out.
    fn update_state(&self, f: impl FnOnce(&mut crate::state::State)) {
        let mut s = self.state.borrow_mut();
        f(&mut s);
        crate::state::save(&s);
    }
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    use niri_ipc::LayoutSwitchTarget as T;
    let kb = ui.global::<crate::KeyboardLayouts>();
    let a = app.clone();
    kb.on_next(move || niri::switch_layout(&a.ctx, T::Next));
    let a = app.clone();
    kb.on_prev(move || niri::switch_layout(&a.ctx, T::Prev));
    let a = app.clone();
    kb.on_pick(move |i| niri::switch_layout(&a.ctx, T::Index(i.clamp(0, 255) as u8)));

    ui.global::<crate::Clock>()
        .set_zones(ModelRc::from(app.extra.zones.clone()));

    let cp = ui.global::<crate::ColorPicker>();
    let a = app.clone();
    cp.on_pick(move || a.start_pick());
    let a = app.clone();
    cp.on_copy(move |text| a.copy_text(&text));
    app.push_colors();

    let n = ui.global::<crate::Notifications>();
    n.set_morning(clock_text(next_hour(chrono::Local::now(), DND_MORNING), app.locale.1).into());
    let a = app.clone();
    n.on_set_dnd(move |choice| {
        let now = chrono::Local::now();
        let dnd = match choice {
            1 => Dnd::Until(now + chrono::Duration::hours(1)),
            2 => Dnd::Until(next_hour(now, DND_MORNING)),
            3 => Dnd::Forever,
            _ => Dnd::Off,
        };
        a.notifications.set_dnd(dnd);
    });
    let t = ui.global::<crate::Countdown>();
    let a = app.clone();
    t.on_toggle(move || a.countdown(|c, now| c.toggle(now)));
    let a = app.clone();
    t.on_reset(move || a.countdown(|c, _| c.reset()));
    let a = app.clone();
    t.on_adjust(move |m| a.countdown(|c, now| c.adjust(m as i64, now)));
    let a = app.clone();
    t.on_preset(move |m| {
        a.countdown(|c, now| {
            c.length = Duration::from_secs(m.max(1) as u64 * 60);
            c.state = Run::Idle;
            c.toggle(now);
        })
    });
    let a = app.clone();
    t.on_sw_toggle(move || a.stopwatch(|s, now| s.toggle(now)));
    let a = app.clone();
    t.on_sw_lap(move || a.stopwatch(|s, now| s.lap(now)));
    let a = app.clone();
    t.on_sw_reset(move || a.stopwatch(|s, _| s.reset()));
    let w = Rc::downgrade(app);
    app.extra
        .timer_ticker
        .start(TimerMode::Repeated, Duration::from_millis(100), move || {
            if let Some(a) = w.upgrade() {
                a.push_timers();
            }
        });
    app.extra.timer_ticker.stop();
    let w = Rc::downgrade(app);
    app.extra
        .timer_end
        .start(TimerMode::SingleShot, Duration::from_secs(1), move || {
            if let Some(a) = w.upgrade() {
                a.timer_went_off();
            }
        });
    app.extra.timer_end.stop();
    app.arm_timer_end();
    app.push_timers();

    let nt = ui.global::<crate::Notes>();
    nt.set_texts(ModelRc::from(app.extra.note_texts.clone()));
    nt.set_names(ModelRc::from(app.extra.note_names.clone()));
    let a = app.clone();
    nt.on_edited(move |id, text| a.note_edited(id as u32, text.into()));
    let w = Rc::downgrade(app);
    app.extra
        .notes_save
        .start(TimerMode::SingleShot, Duration::from_secs(1), move || {
            if let Some(a) = w.upgrade() {
                a.save_notes();
            }
        });
    app.extra.notes_save.stop();

    let a = app.clone();
    ui.global::<crate::Privacy>()
        .on_stop(move |session| niri::stop_cast(&a.ctx, session as u64));

    let au = ui.global::<crate::Audio>();
    au.set_streams(ModelRc::from(app.extra.streams.clone()));
    let a = app.clone();
    au.on_set_stream_volume(move |id, v| {
        a.extra
            .stream_touched
            .set(Some((id as u32, std::time::Instant::now())));
        a.audio.set_stream_volume(id as u32, v);
    });
    let a = app.clone();
    au.on_toggle_stream_mute(move |id| a.audio.toggle_stream_mute(id as u32));
    let a = app.clone();
    au.on_set_default_sink(move |id| a.audio.set_default_sink(id as u32));

    // Do not disturb survives a restart.
    let saved = Dnd::from_saved(app.extra.state.borrow().dnd_until);
    if saved != Dnd::Off {
        app.notifications.set_dnd(saved);
    }
}

/// "Until tomorrow" ends at this hour.
const DND_MORNING: u32 = 7;

fn clock_text(t: chrono::DateTime<chrono::Local>, us: bool) -> String {
    if us {
        t.format("%-I:%M %p").to_string()
    } else {
        t.format("%-H:%M").to_string()
    }
}

impl App {
    pub(super) fn handle_extra(&self, msg: Msg) -> Option<Control> {
        let ui = self.ui();
        match msg {
            Msg::KeyboardLayouts(l) => {
                let k = ui.global::<crate::KeyboardLayouts>();
                k.set_available(l.is_some());
                let l = l.unwrap_or(niri_ipc::KeyboardLayouts {
                    names: Vec::new(),
                    current_idx: 0,
                });
                k.set_current(l.current_idx as i32);
                k.set_layouts(model(
                    l.names
                        .iter()
                        .map(|n| crate::LayoutItem {
                            short: crate::sources::keyboard::short_label(n).into(),
                            name: n.into(),
                        })
                        .collect(),
                ));
            }
            Msg::Casts(casts) => {
                let started = casts.iter().any(|c| {
                    !self
                        .extra
                        .casts
                        .borrow()
                        .iter()
                        .any(|o| o.session_id == c.session_id)
                });
                if started {
                    self.osd("Screen sharing started", "video-display");
                }
                *self.extra.casts.borrow_mut() = casts;
                self.push_privacy();
            }
            Msg::Privacy(usage) => {
                *self.extra.usage.borrow_mut() = usage;
                self.push_privacy();
            }
            Msg::SleepBlockers(list) => {
                ui.global::<crate::Power>().set_blockers(model(
                    list.into_iter()
                        .map(|b| crate::Blocker {
                            who: b.who.into(),
                            why: b.why.into(),
                            what: b.what.into(),
                        })
                        .collect(),
                ));
            }
            Msg::ColorPicked(rgb) => {
                if let Some(rgb) = rgb {
                    let hex = hex(rgb);
                    self.extra.update_state(|s| {
                        s.colors.retain(|c| *c != hex);
                        s.colors.insert(0, hex.clone());
                        s.colors.truncate(COLORS);
                    });
                    self.push_colors();
                    self.copy_text(&hex);
                }
                if self.extra.reshow_after_pick.take() {
                    return Some(Control::Show);
                }
            }
            _ => {}
        }
        None
    }

    /// Called every loop iteration, after the dashboard's own visibility handling.
    pub(super) fn tick_extra(&self, shell: &mut Shell) {
        if self.extra.pending_pick.get() && !shell.is_visible() {
            self.extra.pending_pick.set(false);
            self.run_pick();
        }
    }

    // ---- do not disturb ----

    /// The notification server's do-not-disturb state: show it, and remember it for restarts.
    pub(super) fn sync_dnd(&self, dnd: Dnd, missed: u32) {
        let ui = self.ui();
        let n = ui.global::<crate::Notifications>();
        let now = chrono::Local::now();
        let us = self.locale.1;
        n.set_dnd(dnd.active(now));
        n.set_missed(missed as i32);
        n.set_dnd_text(
            match dnd {
                Dnd::Off => String::new(),
                Dnd::Forever => "Pop-ups paused".into(),
                Dnd::Until(t) if t.date_naive() == now.date_naive() => {
                    format!("Pop-ups paused until {}", clock_text(t, us))
                }
                Dnd::Until(t) => format!("Pop-ups paused until tomorrow {}", clock_text(t, us)),
            }
            .into(),
        );
        n.set_morning(clock_text(next_hour(now, DND_MORNING), us).into());
        let saved = dnd.to_saved();
        if self.extra.state.borrow().dnd_until != saved {
            self.extra.update_state(|s| s.dnd_until = saved);
        }
    }

    pub(super) fn toggle_dnd(&self) {
        let on = Dnd::from_saved(self.extra.state.borrow().dnd_until).active(chrono::Local::now());
        self.notifications
            .set_dnd(if on { Dnd::Off } else { Dnd::Forever });
        self.osd(
            if on {
                "Notifications back on"
            } else {
                "Do not disturb"
            },
            if on {
                "preferences-system-notifications"
            } else {
                "notifications-disabled"
            },
        );
    }

    fn osd(&self, text: &str, icon: &str) {
        self.extra.osd.show(&self.ctx, text, icon);
    }

    // ---- timer & stopwatch ----

    fn countdown(&self, f: impl FnOnce(&mut Countdown, SystemTime)) {
        f(&mut self.extra.countdown.borrow_mut(), SystemTime::now());
        self.extra.timer_done.stop();
        self.ui().global::<crate::Countdown>().set_done(false);
        let c = self.extra.countdown.borrow().clone();
        self.extra.update_state(|s| {
            s.timer_secs = c.length.as_secs() as u32;
            s.timer_end = match c.state {
                Run::Running { end } => end
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_millis() as i64),
                _ => None,
            };
        });
        self.arm_timer_end();
        self.timers_awake(self.ctx.visible());
    }

    fn stopwatch(&self, f: impl FnOnce(&mut Stopwatch, SystemTime)) {
        f(&mut self.extra.stopwatch.borrow_mut(), SystemTime::now());
        self.timers_awake(self.ctx.visible());
    }

    /// Set the end timer for a running countdown.
    fn arm_timer_end(&self) {
        let c = self.extra.countdown.borrow().clone();
        match c.state {
            Run::Running { .. } => {
                let left = c.left(SystemTime::now());
                self.extra
                    .timer_end
                    .set_interval(left.max(Duration::from_millis(1)));
                self.extra.timer_end.restart();
            }
            _ => self.extra.timer_end.stop(),
        }
    }

    fn timer_went_off(&self) {
        let now = SystemTime::now();
        if !self.extra.countdown.borrow().finished(now) {
            // Woken early (the clock changed): wait for the rest.
            self.arm_timer_end();
            return;
        }
        let length = self.extra.countdown.borrow().length;
        self.countdown(|c, _| c.reset());
        let ui = self.ui();
        ui.global::<crate::Countdown>().set_done(true);
        let text = format!("{} timer is done", crate::timer::format(length, false));
        self.osd(&text, "alarm-symbolic");
        self.notifications.add_local("Timer", &text, "");
        if std::path::Path::new(ALARM_SOUND).exists() {
            let _ = crate::sources::spawn_detached("pw-play", &[ALARM_SOUND]);
        }
        let w = self.ui.clone();
        self.extra
            .timer_done
            .start(TimerMode::SingleShot, Duration::from_secs(60), move || {
                if let Some(ui) = w.upgrade() {
                    ui.global::<crate::Countdown>().set_done(false);
                }
            });
    }

    /// Run the ticker only while it shows anything.
    pub(super) fn timers_awake(&self, visible: bool) {
        // Notes: read them again on show (edited elsewhere?), save right away on hide.
        if visible {
            self.sync_notes(true);
        } else if !self.extra.notes_dirty.borrow().is_empty() {
            self.extra.notes_save.stop();
            self.save_notes();
        }
        let running = matches!(self.extra.countdown.borrow().state, Run::Running { .. })
            || self.extra.stopwatch.borrow().running();
        if visible && running {
            if !self.extra.timer_ticker.running() {
                self.extra.timer_ticker.restart();
            }
        } else {
            self.extra.timer_ticker.stop();
        }
        self.push_timers();
    }

    fn push_timers(&self) {
        let now = SystemTime::now();
        let ui = self.ui();
        let t = ui.global::<crate::Countdown>();
        let c = self.extra.countdown.borrow();
        let text: slint::SharedString = crate::timer::format_left(c.left(now)).into();
        if t.get_text() != text {
            t.set_text(text);
        }
        t.set_fraction(c.fraction(now));
        t.set_running(matches!(c.state, Run::Running { .. }));
        t.set_paused(matches!(c.state, Run::Paused { .. }));
        let s = self.extra.stopwatch.borrow();
        let elapsed = s.elapsed(now);
        t.set_sw_text(crate::timer::format(elapsed, true).into());
        t.set_sw_running(s.running());
        t.set_sw_started(!elapsed.is_zero());
        if t.get_laps().row_count() != s.laps.len() {
            t.set_laps(model(
                s.laps
                    .iter()
                    .map(|l| crate::timer::format(*l, true).into())
                    .collect(),
            ));
        }
    }

    // ---- privacy ----

    fn push_privacy(&self) {
        use crate::{PrivacyItem, PrivacyKind};
        let usage = self.extra.usage.borrow();
        let casts = self.extra.casts.borrow();
        let mut items = Vec::new();
        for app in &usage.mic {
            items.push(PrivacyItem {
                kind: PrivacyKind::Mic,
                app: app.as_str().into(),
                detail: "microphone".into(),
                session: 0,
            });
        }
        for app in &usage.camera {
            items.push(PrivacyItem {
                kind: PrivacyKind::Camera,
                app: app.as_str().into(),
                detail: "camera".into(),
                session: 0,
            });
        }
        for c in casts.iter() {
            let app = c
                .pid
                .and_then(|pid| std::fs::read_to_string(format!("/proc/{pid}/comm")).ok())
                .map(|s| s.trim().to_string())
                .or_else(|| {
                    let node = c.pw_node?;
                    usage
                        .video_readers
                        .iter()
                        .find(|(n, _)| *n == node)
                        .map(|(_, app)| app.clone())
                })
                .unwrap_or_else(|| "An app".into());
            items.push(PrivacyItem {
                kind: PrivacyKind::Screen,
                app: app.into(),
                detail: format!("{}{}", c.target, if c.active { "" } else { " · paused" }).into(),
                session: c.session_id as i32,
            });
        }
        let ui = self.ui();
        let p = ui.global::<crate::Privacy>();
        p.set_mic(!usage.mic.is_empty());
        p.set_camera(!usage.camera.is_empty());
        p.set_screen(!casts.is_empty());
        p.set_items(model(items));
    }

    // ---- per-app volume ----

    pub(super) fn sync_mixer(&self, s: &crate::sources::audio::State) {
        let ui = self.ui();
        let au = ui.global::<crate::Audio>();
        au.set_sinks(model(
            s.sinks
                .iter()
                .map(|k| crate::AudioSink {
                    id: k.id as i32,
                    name: k.name.as_str().into(),
                    default: k.default,
                })
                .collect(),
        ));
        let touched = self
            .extra
            .stream_touched
            .get()
            .filter(|(_, t)| t.elapsed() < Duration::from_millis(1500))
            .map(|(id, _)| id);
        let rows: Vec<crate::AudioStream> = s
            .streams
            .iter()
            .map(|st| {
                let icon = self.image(&st.icon);
                let mut volume = st.volume;
                if touched == Some(st.id)
                    && let Some(old) = self.extra.streams.iter().find(|r| r.id as u32 == st.id)
                {
                    volume = old.volume;
                }
                crate::AudioStream {
                    id: st.id as i32,
                    app: st.app.as_str().into(),
                    has_icon: icon.is_some(),
                    icon: icon.unwrap_or_default(),
                    volume,
                    muted: st.muted,
                }
            })
            .collect();
        set_if_changed(&self.extra.streams, rows, |a, b| a == b);
    }

    // ---- sticky notes ----

    /// The notes widgets and their note names, by widget id.
    fn note_widgets(&self) -> Vec<(u32, String)> {
        use crate::widget_settings::{Opt, get_str};
        let us = self.locale.1;
        let layout = self.layout.borrow();
        let placed: Vec<crate::layout::Placed> = match self.solo {
            Some(Kind::Notes) => vec![layout.solo_widget(Kind::Notes)],
            Some(_) => Vec::new(),
            None => layout
                .visible()
                .into_iter()
                .filter(|p| p.kind == Kind::Notes)
                .cloned()
                .collect(),
        };
        placed
            .into_iter()
            .map(|p| (p.id, note_name(&get_str(&p.settings, Opt::Note, us))))
            .collect()
    }

    /// Fill `Notes.texts` for the notes widgets; `reload` reads unchanged notes from disk again
    /// (they may have been edited elsewhere).
    pub(super) fn sync_notes(&self, reload: bool) {
        let widgets = self.note_widgets();
        if widgets.is_empty() && self.extra.note_texts.row_count() == 0 {
            return;
        }
        {
            let mut notes = self.extra.notes.borrow_mut();
            let dirty = self.extra.notes_dirty.borrow();
            for (_, name) in &widgets {
                if dirty.contains(name) {
                    continue;
                }
                if reload || !notes.contains_key(name) {
                    let text = std::fs::read_to_string(note_path(name)).unwrap_or_default();
                    notes.insert(name.clone(), text);
                }
            }
        }
        let len = widgets
            .iter()
            .map(|(id, _)| *id as usize + 1)
            .max()
            .unwrap_or(0);
        let notes = self.extra.notes.borrow();
        let mut texts = vec![slint::SharedString::new(); len];
        let mut names = vec![slint::SharedString::new(); len];
        for (id, name) in widgets {
            texts[id as usize] = notes.get(&name).cloned().unwrap_or_default().into();
            names[id as usize] = title_case(&name).into();
        }
        set_if_changed(&self.extra.note_texts, texts, |a, b| a == b);
        set_if_changed(&self.extra.note_names, names, |a, b| a == b);
    }

    fn note_edited(&self, id: u32, text: String) {
        let Some(name) = self
            .note_widgets()
            .into_iter()
            .find(|(i, _)| *i == id)
            .map(|(_, n)| n)
        else {
            return;
        };
        self.extra
            .notes
            .borrow_mut()
            .insert(name.clone(), text.clone());
        self.extra.notes_dirty.borrow_mut().insert(name.clone());
        // Other widgets showing the same note follow; the one being typed in keeps its text.
        for (other, n) in self.note_widgets() {
            if other != id && n == name {
                self.extra
                    .note_texts
                    .set_row_data(other as usize, text.as_str().into());
            }
        }
        // Single-shot, restarted by every keystroke: saves once typing pauses.
        self.extra.notes_save.restart();
    }

    fn save_notes(&self) {
        let dirty: Vec<String> = self.extra.notes_dirty.borrow_mut().drain().collect();
        let notes = self.extra.notes.borrow();
        for name in dirty {
            let path = note_path(&name);
            let text = notes.get(&name).map(String::as_str).unwrap_or("");
            let result = (|| {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                let tmp = path.with_extension("md.tmp");
                std::fs::write(&tmp, text)?;
                std::fs::rename(&tmp, &path)
            })();
            if let Err(e) = result {
                log::warn!("saving {}: {e}", path.display());
                self.toast(&format!("Saving note {name}: {e}"));
            }
        }
    }

    // ---- color picker ----

    fn start_pick(&self) {
        if self.extra.solo.is_some() {
            // A normal window doesn't cover the screen: pick right away.
            self.run_pick();
            return;
        }
        // The dashboard covers everything: hide it, pick once it's gone, then come back.
        self.extra.reshow_after_pick.set(true);
        self.extra.pending_pick.set(true);
        self.pending.borrow_mut().push(Control::Hide);
    }

    fn run_pick(&self) {
        let ctx = self.ctx.clone();
        self.ctx.spawn(async move {
            let picked = tokio::task::spawn_blocking(niri::pick_color).await;
            let rgb = match picked {
                Ok(Ok(rgb)) => rgb,
                Ok(Err(e)) => {
                    ctx.toast(format!("Pick color: {e}"));
                    None
                }
                Err(e) => {
                    log::warn!("pick color: {e}");
                    None
                }
            };
            ctx.send(Msg::ColorPicked(rgb));
        });
    }

    fn copy_text(&self, text: &str) {
        *self.pending_clip.borrow_mut() = Some(clipboard::Clip {
            kind: clipboard::ClipKind::Text,
            data: Arc::new(text.as_bytes().to_vec()),
        });
        self.toast(&format!("Copied {text}"));
    }

    fn push_colors(&self) {
        let colors: Vec<crate::PickedColor> = self
            .extra
            .state
            .borrow()
            .colors
            .iter()
            .filter_map(|h| parse_hex(h))
            .map(picked)
            .collect();
        self.ui()
            .global::<crate::ColorPicker>()
            .set_colors(model(colors));
    }

    // ---- clock time zones ----

    /// Refresh `Clock.zones` for every clock widget that lists other zones.
    pub(super) fn tick_zones(&self) {
        use crate::widget_settings::{Opt, get_int, get_str};
        let us = self.locale.1;
        let layout = self.layout.borrow();
        let clocks: Vec<crate::layout::Placed> = match self.solo {
            Some(Kind::Clock) => vec![layout.solo_widget(Kind::Clock)],
            Some(_) => Vec::new(),
            None => layout
                .visible()
                .into_iter()
                .filter(|p| p.kind == Kind::Clock)
                .cloned()
                .collect(),
        };
        drop(layout);
        let now = chrono::Utc::now();
        let mut rows: Vec<crate::ZoneList> = Vec::new();
        for p in clocks {
            let (zones, _) = crate::zones::parse_list(&get_str(&p.settings, Opt::Zones, us));
            if zones.is_empty() {
                continue;
            }
            let hours12 = get_int(&p.settings, Opt::Hours, us) == 12;
            let items: Vec<crate::ZoneTime> =
                crate::zones::rows(&zones, now, &chrono::Local, hours12)
                    .into_iter()
                    .map(|z| crate::ZoneTime {
                        label: z.label.into(),
                        time: z.time.into(),
                        delta: z.delta.into(),
                    })
                    .collect();
            let id = p.id as usize;
            if rows.len() <= id {
                rows.resize(id + 1, crate::ZoneList::default());
            }
            rows[id] = crate::ZoneList {
                items: model(items),
            };
        }
        set_if_changed(&self.extra.zones, rows, |a, b| {
            a.items.row_count() == b.items.row_count()
                && a.items.iter().zip(b.items.iter()).all(|(x, y)| x == y)
        });
    }
}

/// Replace the model's rows, touching only those that changed (each change repaints).
fn set_if_changed<T: Clone + 'static>(
    m: &VecModel<T>,
    rows: Vec<T>,
    same: impl Fn(&T, &T) -> bool,
) {
    if m.row_count() != rows.len() {
        m.set_vec(rows);
        return;
    }
    for (i, r) in rows.into_iter().enumerate() {
        if !m.row_data(i).is_some_and(|old| same(&old, &r)) {
            m.set_row_data(i, r);
        }
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

/// A note name as a file name: letters, digits, '-' and '_'; empty is "notes".
fn note_name(raw: &str) -> String {
    let name: String = raw
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let name = name.trim_matches('-').to_string();
    if name.is_empty() {
        "notes".into()
    } else {
        name
    }
}

fn note_path(name: &str) -> std::path::PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".local/share"))
        .join("niri-dash/notes")
        .join(format!("{name}.md"))
}

fn title_case(name: &str) -> String {
    let mut c = name.chars();
    c.next()
        .map(|f| {
            f.to_uppercase()
                .chain(c)
                .collect::<String>()
                .replace('-', " ")
        })
        .unwrap_or_default()
}

fn to_u8(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn hex(rgb: [f64; 3]) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        to_u8(rgb[0]),
        to_u8(rgb[1]),
        to_u8(rgb[2])
    )
}

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    let b = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([b(0)?, b(2)?, b(4)?])
}

/// Hue (degrees), saturation and lightness (percent).
fn hsl([r, g, b]: [u8; 3]) -> (i32, i32, i32) {
    let (r, g, b) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0, 0, (l * 100.0).round() as i32);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (
        h.round() as i32 % 360,
        (s * 100.0).round() as i32,
        (l * 100.0).round() as i32,
    )
}

fn picked(c: [u8; 3]) -> crate::PickedColor {
    let (h, s, l) = hsl(c);
    crate::PickedColor {
        color: slint::Color::from_rgb_u8(c[0], c[1], c[2]),
        hex: format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]).into(),
        rgb: format!("rgb({}, {}, {})", c[0], c[1], c[2]).into(),
        hsl: format!("hsl({h}, {s}%, {l}%)").into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_names() {
        assert_eq!(note_name(""), "notes");
        assert_eq!(note_name(" Work Stuff! "), "work-stuff");
        assert_eq!(note_name("../etc/passwd"), "etc-passwd");
        assert_eq!(title_case("work-stuff"), "Work stuff");
    }

    #[test]
    fn color_formats() {
        assert_eq!(hex([1.0, 0.5, 0.0]), "#ff8000");
        assert_eq!(parse_hex("#ff8000"), Some([255, 128, 0]));
        assert_eq!(parse_hex("ff8000"), None);
        assert_eq!(hsl([255, 0, 0]), (0, 100, 50));
        assert_eq!(hsl([0, 0, 255]), (240, 100, 50));
        assert_eq!(hsl([128, 128, 128]), (0, 0, 50));
    }
}

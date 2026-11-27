//! The tests: the real dashboard in BUI's headless window, on the mock system run by hand. Keys
//! and clicks go through the window as they would on screen; the dashboard's commands go to the
//! mock, which answers with events, and the tests look at both and at the files written.
//!
//! The kit keeps global state and the files go by environment variables, so the tests take turns
//! ([`LOCK`]).

use crate::app::{Shell, with_dash};
use crate::dash::{Dash, Mode};
use crate::layout::{Kind, Layout};
use crate::view::DashView;
use crate::{Control, app};
use blenderui::prelude::*;
use niri_dash_system::{self as sys, Command, Demo, MockSystem};
use std::path::PathBuf;
use std::sync::Mutex;

static LOCK: Mutex<()> = Mutex::new(());

pub struct Harness {
    pub app: App,
    pub win: WindowId,
    mock: MockSystem,
    commands: std::sync::mpsc::Receiver<Command>,
    events: std::sync::mpsc::Receiver<sys::Event>,
    /// Every command the dashboard sent, in order.
    pub sent: Vec<String>,
    pub dir: PathBuf,
    _turn: std::sync::MutexGuard<'static, ()>,
}

impl Harness {
    /// The dashboard on `config` (None: the default one), shown in a window of 1920 x 1080.
    pub fn new(config: Option<&str>) -> Self {
        let turn = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!(
            "niri-dash-bui-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("config")).expect("a test directory");
        let config_path = dir.join("config/config.kdl");
        std::fs::write(
            &config_path,
            config.map_or_else(Layout::default_text, str::to_string),
        )
        .expect("writing the config");
        // SAFETY: the tests take turns (LOCK), and nothing else reads these meanwhile.
        unsafe {
            std::env::set_var("NIRI_DASH_CONFIG", &config_path);
            std::env::set_var("NIRI_DASH_STATE", dir.join("state.json"));
            std::env::set_var("XDG_DATA_HOME", dir.join("data"));
        }
        let (cmd_sink, commands) = sys::Sink::channel();
        let (ev_sink, events) = sys::Sink::channel();
        let mock = MockSystem::new(Demo::new(), ev_sink);
        let mut app = Application::headless().start(|cx| {
            let system = sys::System {
                commands: cmd_sink,
                caps: MockSystem::CAPS,
            };
            let dash = cx.new(|_| Dash::new(system, Layout::load(), None));
            cx.set_global(Shell {
                dash,
                window: None,
                solo: None,
                a11y: None,
            });
        });
        let dash = app.global::<Shell>().dash;
        let win = app
            .open_window(app::headless_options(None, 1920.0, 1080.0), move |_, cx| {
                cx.new(|_| DashView::new(dash))
            })
            .expect("a headless window")
            .id;
        app.global_mut::<Shell>().window = Some(win);
        let mut h = Self {
            app,
            win,
            mock,
            commands,
            events,
            sent: Vec::new(),
            dir,
            _turn: turn,
        };
        h.control(Control::Show);
        h
    }

    /// Hand the mock what the dashboard asked, and the dashboard what the mock said, until both
    /// are quiet; then draw.
    pub fn pump(&mut self) {
        loop {
            let mut busy = false;
            while let Ok(c) = self.commands.try_recv() {
                self.sent.push(format!("{c:?}"));
                let _ = self.mock.handle(c);
                busy = true;
            }
            let events: Vec<sys::Event> = self.events.try_iter().collect();
            if !events.is_empty() {
                busy = true;
                with_dash(&mut self.app, |d, cx| {
                    for e in events {
                        d.event(e);
                    }
                    cx.notify();
                });
            }
            self.app.run_pending();
            app::apply_focus(&mut self.app);
            // What the dashboard asks of the window: showing is done, hiding is noted.
            with_dash(&mut self.app, |d, _| d.ops.clear());
            if !busy {
                break;
            }
        }
        self.app.draw(self.win);
    }

    /// One of the ticker's ticks, and the redraw it asks for.
    pub fn tick(&mut self) {
        with_dash(&mut self.app, |d, cx| {
            if d.tick() {
                cx.notify();
            }
        });
        self.pump();
    }

    pub fn control(&mut self, c: Control) {
        with_dash(&mut self.app, |d, cx| d.control(c, cx));
        self.pump();
    }

    /// Press keys, as the snapshot command takes them: "f", "C-e", "S-Tab", "Escape", "space".
    pub fn keys(&mut self, keys: &str) {
        for k in keys.split_whitespace() {
            let (mut ctrl, mut shift) = (false, false);
            let mut rest = k;
            loop {
                if let Some(r) = rest.strip_prefix("C-") {
                    ctrl = true;
                    rest = r;
                } else if let Some(r) = rest.strip_prefix("S-") {
                    shift = true;
                    rest = r;
                } else {
                    break;
                }
            }
            let (text, sym) = crate::snapshot::keysym(rest);
            let upper = rest.len() == 1 && rest.chars().all(|c| c.is_ascii_uppercase());
            self.app
                .simulate_key_with(self.win, &text, &sym, ctrl, shift || upper);
            self.pump();
        }
    }

    /// Click the middle of the element with this id.
    pub fn click_id(&mut self, id: &str) {
        let b = self
            .app
            .element_bounds(self.win, id)
            .unwrap_or_else(|| panic!("no element {id}"));
        self.app.simulate_click(
            self.win,
            b.origin.x + b.size.width / 2.0,
            b.origin.y + b.size.height / 2.0,
        );
        self.pump();
    }

    pub fn dash(&self) -> &Dash {
        let dash = self.app.global::<Shell>().dash;
        dash.read(&self.app)
    }

    /// The id of the first widget of a kind.
    pub fn id_of(&mut self, kind: Kind) -> u32 {
        self.dash()
            .layout
            .widgets
            .iter()
            .find(|w| w.kind == kind)
            .map(|w| w.id)
            .unwrap_or_else(|| panic!("no {kind:?}"))
    }

    /// The hint label of the widget `id`.
    pub fn label_of(&mut self, id: u32) -> String {
        self.dash()
            .tiles()
            .into_iter()
            .find(|(p, _)| p.id == id)
            .map(|(_, l)| l)
            .expect("shown")
    }

    /// config.kdl as written, once the writes waiting have been done.
    pub fn config(&self) -> String {
        crate::files::flush();
        std::fs::read_to_string(self.dir.join("config/config.kdl")).expect("the config")
    }

    pub fn took(&self, needle: &str) -> bool {
        self.sent.iter().any(|c| c.contains(needle))
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        crate::files::flush();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dash::Op;

    #[test]
    fn coming_up_tells_the_system_and_fills_the_widgets() {
        let h = Harness::new(None);
        assert!(h.dash().visible);
        assert!(h.took("Visible(true)"));
        assert!(h.dash().data.perf.is_some(), "the mock's data arrived");
        assert!(!h.dash().data.windows.rows.is_empty());
    }

    #[test]
    fn a_label_goes_into_its_widget_and_escape_steps_back() {
        let mut h = Harness::new(None);
        let id = h.id_of(Kind::Calendar);
        let label = h.label_of(id);
        h.keys("f");
        assert_eq!(h.dash().nav.mode, Mode::Hints);
        h.keys(
            &label
                .chars()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(" "),
        );
        assert_eq!(h.dash().nav.mode, Mode::Widget);
        assert_eq!(h.dash().nav.active, id);
        h.keys("Escape");
        assert_eq!(h.dash().nav.mode, Mode::Off);
        assert!(h.dash().visible);
        h.keys("Escape");
        assert!(!h.dash().visible, "a second Escape puts the dashboard away");
        assert!(h.took("Visible(false)"));
    }

    #[test]
    fn a_key_that_matches_no_label_is_not_typed() {
        let mut h = Harness::new(None);
        h.keys("f z");
        assert_eq!(h.dash().nav.typed, "");
        assert_eq!(h.dash().nav.mode, Mode::Hints);
        h.keys("f");
        assert_eq!(h.dash().nav.mode, Mode::Off, "f again cancels");
    }

    #[test]
    fn ctrl_hjkl_walks_between_neighbours() {
        let mut h = Harness::new(None);
        let clock = h.id_of(Kind::Clock);
        let weather = h.id_of(Kind::Weather);
        let windows = h.id_of(Kind::Windows);
        h.keys("C-l");
        assert_eq!(h.dash().nav.active, clock, "the first widget");
        h.keys("C-j");
        assert_eq!(h.dash().nav.active, weather);
        h.keys("C-k C-l");
        assert_eq!(h.dash().nav.active, windows);
    }

    #[test]
    fn the_calendar_moves_by_day_week_and_month() {
        let mut h = Harness::new(None);
        h.keys("C-l C-j C-j");
        let active = h.dash().nav.active;
        assert_eq!(
            h.dash().placed(active).map(|p| p.kind),
            Some(Kind::Calendar)
        );
        let today = chrono::Local::now().date_naive();
        h.keys("l l j");
        assert_eq!(h.dash().w.cal, Some(today + chrono::Duration::days(9)));
        h.keys("t");
        assert_eq!(h.dash().w.cal, None);
    }

    #[test]
    fn a_sliders_keys_set_the_volume() {
        let mut h = Harness::new(None);
        let id = h.id_of(Kind::Volume);
        let label = h.label_of(id);
        h.keys(&format!(
            "f {}",
            label
                .chars()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        ));
        assert_eq!(h.dash().nav.active, id);
        h.keys("3");
        assert!(h.took("SetVolume(0.3"), "{:?}", h.sent);
        h.keys("k");
        assert!(
            h.took("SetVolume(0.35"),
            "a round volume is a knob: k turns it up"
        );
        h.keys("Return");
        assert!(h.took("ToggleMute"));
    }

    #[test]
    fn the_timer_starts_from_the_keyboard_and_remembers() {
        let mut h = Harness::new(Some(
            "layout {\n    timer col=0 row=0 width=3 height=2\n}\n",
        ));
        h.keys("C-l space");
        assert!(matches!(
            h.dash().w.countdown.state,
            crate::timer::Run::Running { .. }
        ));
        assert!(
            h.dash().saved.timer_end.is_some(),
            "kept for the next start"
        );
        h.keys("r");
        assert!(matches!(
            h.dash().w.countdown.state,
            crate::timer::Run::Idle
        ));
    }

    #[test]
    fn edit_mode_removes_a_widget_and_writes_the_file() {
        let mut h = Harness::new(None);
        assert!(h.config().contains("tray"));
        h.keys("C-e");
        assert!(h.dash().edit.is_some());
        let tray = h.id_of(Kind::Tray);
        let label = h.label_of(tray);
        h.keys(&format!(
            "f {}",
            label
                .chars()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        ));
        assert_eq!(h.dash().edit.as_ref().map(|e| e.picked), Some(tray));
        h.keys("x");
        assert!(h.dash().layout.get(tray).is_none());
        let text = h.config();
        assert!(
            !text.lines().any(|l| l.trim_start().starts_with("tray ")),
            "{text}"
        );
        assert!(
            text.contains("// niri-dash layout") || text.contains("layout {"),
            "the rest of the file stays"
        );
        h.keys("Escape");
        assert!(h.dash().edit.is_none());
    }

    #[test]
    fn move_mode_moves_the_widget_and_saves() {
        let mut h = Harness::new(Some(
            "layout {\n    clock col=0 row=0 width=4 height=2\n    weather col=0 row=2 width=4 height=2\n}\n",
        ));
        let clock = h.id_of(Kind::Clock);
        h.keys("C-l C-m");
        assert_eq!(h.dash().nav.op, Some(Op::Move));
        h.keys("l l");
        assert_eq!(h.dash().layout.get(clock).map(|p| p.col), Some(2));
        h.keys("Escape");
        assert_eq!(h.dash().nav.op, None);
        assert!(h.config().contains("clock col=2"), "{}", h.config());
    }

    #[test]
    fn resize_mode_grows_and_shrinks() {
        let mut h = Harness::new(Some(
            "layout {\n    clock col=0 row=0 width=4 height=2\n}\n",
        ));
        let clock = h.id_of(Kind::Clock);
        h.keys("C-l C-r l j");
        let p = h.dash().layout.get(clock).cloned().expect("the clock");
        assert_eq!((p.w, p.h), (5, 3));
        h.keys("S-l");
        let p = h.dash().layout.get(clock).cloned().expect("the clock");
        assert_eq!((p.col, p.w), (1, 4), "shift pulls the opposite side in");
    }

    #[test]
    fn a_click_in_edit_mode_picks_and_opens_the_settings() {
        let mut h = Harness::new(None);
        h.keys("C-e");
        let clock = h.id_of(Kind::Clock);
        h.click_id(&format!("w{clock}"));
        let e = h.dash().edit.clone().expect("editing");
        assert_eq!(e.picked, clock);
        assert!(e.settings);
    }

    #[test]
    fn a_color_scheme_from_the_sidebar_is_saved() {
        let mut h = Harness::new(None);
        h.keys("C-e Tab");
        assert_eq!(
            h.dash().edit.as_ref().map(|e| e.tab),
            Some(crate::edit::Tab::Colors)
        );
        with_dash(&mut h.app, |d, _| {
            d.layout.colorscheme = "nord".into();
            d.relook();
            d.save_layout();
        });
        h.pump();
        assert_eq!(h.dash().look.color("accent"), "#88c0d0ff");
        assert!(
            h.config().contains("colorscheme=\"nord\""),
            "{}",
            h.config()
        );
    }

    #[test]
    fn notification_keys_dismiss_and_clear() {
        let mut h = Harness::new(None);
        let id = h.id_of(Kind::Notifications);
        let label = h.label_of(id);
        h.keys(&format!(
            "f {}",
            label
                .chars()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        ));
        // Past the bell and Clear, onto the first notification.
        h.keys("j j d");
        assert!(h.took("Dismiss("), "{:?}", h.sent);
        h.keys("D");
        assert!(h.took("Clear"));
    }

    #[test]
    fn the_control_socket_s_commands_show_and_hide() {
        let mut h = Harness::new(None);
        h.control(Control::Toggle);
        assert!(!h.dash().visible);
        h.control(Control::Toggle);
        assert!(h.dash().visible);
        h.control(Control::Edit);
        assert!(h.dash().edit.is_some());
        h.control(Control::Hide);
        assert!(!h.dash().visible);
        assert!(h.dash().edit.is_none(), "edit mode ends with the dashboard");
    }

    #[test]
    fn enter_types_into_a_field_and_escape_gets_out() {
        let mut h = Harness::new(Some(
            "layout {\n    sleep-timer col=0 row=0 width=4 height=4\n}\n",
        ));
        h.keys("C-l");
        let stops = h
            .dash()
            .entered_ctx(h.app.global::<Shell>().dash.downgrade())
            .expect("entered")
            .0;
        let field = stops
            .iter()
            .position(|s| s.does == "type")
            .expect("the when field is a stop");
        with_dash(&mut h.app, |d, _| d.nav.cursor = field);
        h.keys("Return");
        assert!(
            h.app.window(h.win).expect("open").text_field_focused(),
            "Enter put the keyboard in the field"
        );
        h.keys("Escape");
        assert!(
            !h.app.window(h.win).expect("open").text_field_focused(),
            "Escape took it out"
        );
        assert_eq!(h.dash().nav.mode, Mode::Widget, "still in the widget");
        h.keys("Escape");
        assert_eq!(
            h.dash().nav.mode,
            Mode::Off,
            "the next one leaves the widget"
        );
    }

    #[test]
    fn a_config_changed_on_disk_is_laid_out_again() {
        let mut h = Harness::new(Some(
            "layout {\n    clock col=0 row=0 width=4 height=2\n}\n",
        ));
        std::fs::write(h.dir.join("config/config.kdl"), "theme colorscheme=\"gruvbox-dark\"\nlayout {\n    clock col=2 row=1 width=4 height=2\n    media col=0 row=4 width=5 height=2\n}\n").expect("writing");
        with_dash(&mut h.app, |d, _| d.reload());
        h.pump();
        let d = h.dash();
        assert_eq!(d.layout.widgets.len(), 2);
        assert_eq!(
            d.layout
                .widgets
                .iter()
                .find(|w| w.kind == Kind::Clock)
                .map(|w| (w.col, w.row)),
            Some((2, 1))
        );
        assert_ne!(
            d.look.color("accent"),
            "#4772b3ff",
            "the scheme changed from Blender's"
        );
    }

    #[test]
    fn a_line_said_to_the_mascot_shows_until_clicked_away() {
        let mut h = Harness::new(Some(
            "layout {\n    mascot col=0 row=0 width=4 height=2\n}\n",
        ));
        let id = h.id_of(Kind::Mascot);
        let say = crate::mascots::Say {
            text: "Build finished".into(),
            to: None,
            outfit: None,
            secs: None,
        };
        with_dash(&mut h.app, |d, _| d.w.say(&say));
        h.pump();
        assert_eq!(
            h.dash().w.board.view(id).map(|v| v.text),
            Some("Build finished".into())
        );
        h.click_id(&format!("w{id}"));
        assert_ne!(
            h.dash().w.board.view(id).map(|v| v.text),
            Some("Build finished".into())
        );
    }

    #[test]
    fn a_held_press_opens_the_widget_and_drags_it() {
        let mut h = Harness::new(Some(
            "layout {\n    clock col=0 row=0 width=4 height=2\n    weather col=0 row=2 width=4 height=2\n}\n",
        ));
        let clock = h.id_of(Kind::Clock);
        h.keys("f");
        let b = h
            .app
            .element_bounds(h.win, &format!("w{clock}"))
            .expect("the clock");
        let (x, y) = (
            b.origin.x + b.size.width / 2.0,
            b.origin.y + b.size.height / 2.0,
        );
        h.app.simulate_press(h.win, x, y);
        h.pump();
        assert!(h.dash().press.is_some());
        std::thread::sleep(crate::dash::HOLD_PRESS + std::time::Duration::from_millis(50));
        h.tick();
        h.pump();
        let e = h.dash().edit.clone().expect("open for adjusting");
        assert!(e.adjust && e.picked == clock);
        assert_eq!(h.dash().nav.mode, Mode::Off, "the labels gave way");
        // The same press goes on: two cells to the right, and let go.
        let cell = h.dash().geometry().cell;
        h.app.simulate_mouse_move(h.win, x + 2.0 * cell, y);
        h.pump();
        h.app.simulate_release(h.win, x + 2.0 * cell, y);
        h.pump();
        assert_eq!(h.dash().layout.get(clock).map(|p| p.col), Some(2));
        assert!(h.config().contains("clock col=2"));
        // The next press is a press of its own: its click counts.
        h.app.simulate_press(h.win, 5.0, 300.0);
        h.pump();
        assert!(!h.dash().swallow_click);
        h.keys("Escape");
        assert!(h.dash().edit.is_none(), "Escape is done with it");
    }

    #[test]
    fn a_press_that_moves_is_no_hold() {
        let mut h = Harness::new(Some(
            "layout {\n    clock col=0 row=0 width=4 height=2\n}\n",
        ));
        let clock = h.id_of(Kind::Clock);
        let b = h
            .app
            .element_bounds(h.win, &format!("w{clock}"))
            .expect("the clock");
        let (x, y) = (b.origin.x + 40.0, b.origin.y + 40.0);
        h.app.simulate_press(h.win, x, y);
        h.app.simulate_mouse_move(h.win, x + 30.0, y);
        h.pump();
        std::thread::sleep(crate::dash::HOLD_PRESS + std::time::Duration::from_millis(50));
        h.tick();
        h.app.simulate_release(h.win, x + 30.0, y);
        h.pump();
        assert!(h.dash().edit.is_none());
    }

    #[test]
    fn y_copies_the_row_under_the_cursor() {
        let mut h = Harness::new(Some(
            "layout {\n    windows col=0 row=0 width=5 height=3\n}\n",
        ));
        h.keys("C-l y");
        assert!(h.took("Set(Clip"), "{:?}", h.sent);
        assert!(
            h.dash()
                .toast
                .as_ref()
                .is_some_and(|(t, _)| t.contains("niri-dash")),
            "{:?}",
            h.dash().toast
        );
    }

    #[test]
    fn moving_the_cursor_scrolls_to_it() {
        let mut h = Harness::new(Some(
            "layout {\n    windows col=0 row=0 width=5 height=3\n}\n",
        ));
        h.keys("C-l j");
        let id = h.id_of(Kind::Windows);
        // The view took the request (and scrolled, as far as its list goes).
        assert!(h.dash().scroll_to.is_none());
        assert!(
            h.app
                .element_bounds(h.win, &crate::widgets::stop_id("s", id, 1))
                .is_some(),
            "stops have ids"
        );
    }

    /// Put the cursor on the entered widget's stop that does `does`, and press it.
    fn press(h: &mut Harness, does: &str) {
        let hd = h.app.global::<Shell>().dash.downgrade();
        let stops = h.dash().entered_ctx(hd).expect("entered").0;
        let i = stops
            .iter()
            .position(|s| s.does == does)
            .unwrap_or_else(|| panic!("no stop that does {does}"));
        with_dash(&mut h.app, |d, _| d.nav.cursor = i);
        h.keys("Return");
    }

    #[test]
    fn a_hidden_network_is_joined_from_its_form() {
        let mut h = Harness::new(Some("layout {\n    wifi col=0 row=0 width=4 height=6\n}\n"));
        h.keys("C-l");
        press(&mut h, "join a hidden network");
        assert!(h.dash().w.wifi.hidden);
        with_dash(&mut h.app, |d, _| {
            d.w.wifi.ssid = "Basement".into();
            d.w.wifi.password = "hunter22".into();
        });
        h.pump();
        press(&mut h, "join it");
        assert!(
            h.took("JoinHidden { ssid: \"Basement\", security: 1"),
            "{:?}",
            h.sent
        );
        assert!(!h.dash().w.wifi.hidden, "the form is done");
    }

    #[test]
    fn a_password_prompt_answers_with_what_was_typed() {
        let mut h = Harness::new(Some("layout {\n    wifi col=0 row=0 width=4 height=6\n}\n"));
        h.keys("C-l");
        // A secured network nothing is saved for asks for its password.
        let key = h
            .dash()
            .data
            .wifi
            .as_ref()
            .and_then(|w| {
                w.networks
                    .iter()
                    .find(|n| !n.known() && n.security.needs_secret())
                    .map(|n| n.key())
            })
            .expect("a new secured network");
        with_dash(&mut h.app, |d, _| {
            d.tell(niri_dash_system::wifi::Command::Connect { key: key.clone() })
        });
        h.pump();
        let serial = h
            .dash()
            .data
            .wifi
            .as_ref()
            .and_then(|w| w.prompt.as_ref())
            .map(|p| p.serial)
            .expect("a prompt");
        with_dash(&mut h.app, |d, _| {
            d.w.wifi.serial = serial;
            d.w.wifi.password = "correct horse".into();
        });
        h.pump();
        press(&mut h, "connect");
        assert!(h.took(&format!("Answer {{ serial: {serial}, answer: Answer {{ identity: \"\", password: \"correct horse\"")), "{:?}", h.sent);
    }

    #[test]
    fn arrange_lays_out_and_undo_puts_back() {
        let mut h = Harness::new(None);
        h.keys("C-e");
        let before: Vec<(u32, crate::layout::Rect)> = h
            .dash()
            .layout
            .widgets
            .iter()
            .map(|w| (w.id, w.rect()))
            .collect();
        with_dash(&mut h.app, |d, _| crate::edit::arrange_now(d, 1));
        h.pump();
        let e = h.dash().edit.clone().expect("editing");
        assert!(e.arranged.is_some() && e.before.is_some());
        assert_eq!(
            e.params.map(|p| p.seed),
            Some(h.dash().layout.autolayout.seed + 1),
            "shuffle takes the next seed"
        );
        let after: Vec<(u32, crate::layout::Rect)> = h
            .dash()
            .layout
            .widgets
            .iter()
            .map(|w| (w.id, w.rect()))
            .collect();
        assert_ne!(before, after, "something moved");
        h.keys("Tab Tab Tab");
        assert_eq!(
            h.dash().edit.as_ref().map(|e| e.tab),
            Some(crate::edit::Tab::Grid)
        );
        with_dash(&mut h.app, |d, _| {
            let before = d
                .edit
                .as_mut()
                .and_then(|e| e.before.take())
                .expect("something to undo");
            d.layout.set_rects(&before);
        });
        h.pump();
        let undone: Vec<(u32, crate::layout::Rect)> = h
            .dash()
            .layout
            .widgets
            .iter()
            .map(|w| (w.id, w.rect()))
            .collect();
        assert_eq!(before, undone);
    }

    #[test]
    fn a_widget_dragged_from_the_sidebar_lands_where_it_is_dropped() {
        let mut h = Harness::new(Some(
            "grid columns=12 rows=8\nlayout {\n    clock col=0 row=0 width=4 height=2\n}\n",
        ));
        h.keys("C-e");
        let row = h
            .app
            .element_bounds(h.win, "add-weather")
            .expect("the weather row");
        let g = h.dash().geometry();
        // To the middle of cells 6..10 x 3..5 (the weather is 4 x 2).
        let (tx, ty) = (g.x + 8.0 * g.cell, g.y + 4.0 * g.cell);
        // As a hand does it: press, a few pixels to start the drag, frames drawn on the way.
        let (sx, sy) = (row.origin.x + 30.0, row.origin.y + 10.0);
        h.app.simulate_press(h.win, sx, sy);
        h.app.simulate_mouse_move(h.win, sx - 10.0, sy);
        h.pump();
        assert!(
            h.dash().edit.as_ref().is_some_and(|e| e.drag.is_some()),
            "carrying it"
        );
        h.app.simulate_mouse_move(h.win, tx, ty);
        h.pump();
        h.app.simulate_release(h.win, tx, ty);
        h.pump();
        let w = h
            .dash()
            .layout
            .widgets
            .iter()
            .find(|w| w.kind == Kind::Weather)
            .cloned()
            .expect("added");
        assert_eq!((w.col, w.row, w.w, w.h), (6, 3, 4, 2));
        assert!(h.config().contains("weather"));
        assert_eq!(h.dash().edit.as_ref().map(|e| e.picked), Some(w.id));
    }

    #[test]
    fn a_screen_reader_gets_the_tree_and_then_what_changed() {
        let mut h = Harness::new(Some(
            "layout {\n    clock col=0 row=0 width=4 height=2\n    volume col=4 row=0 width=3 height=2\n}\n",
        ));
        let tree = h.app.accessibility_tree(h.win);
        let mut sent = std::collections::HashMap::new();
        let first = crate::a11y::diff(&mut sent, &tree, "niri-dash");
        assert!(first.tree.is_some(), "the whole tree, with its root, first");
        assert_eq!(first.nodes.len(), tree.nodes.len());
        let named = |u: &accesskit::TreeUpdate, role: accesskit::Role, name: &str| {
            u.nodes
                .iter()
                .any(|(_, n)| n.role() == role && n.label().is_some_and(|l| l.contains(name)))
        };
        assert!(
            named(&first, accesskit::Role::Button, "Edit"),
            "the top bar's buttons are there"
        );
        assert!(
            first
                .nodes
                .iter()
                .any(|(_, n)| n.role() == accesskit::Role::Slider),
            "the volume slider is a slider"
        );
        let again = crate::a11y::diff(&mut sent, &h.app.accessibility_tree(h.win), "niri-dash");
        assert!(
            again.tree.is_none() && again.nodes.is_empty(),
            "nothing changed, nothing sent"
        );
        h.keys("C-e");
        let edit = crate::a11y::diff(&mut sent, &h.app.accessibility_tree(h.win), "niri-dash");
        assert!(
            named(&edit, accesskit::Role::Button, "Done"),
            "edit mode's Done button came"
        );
    }

    #[test]
    fn a_screen_reader_s_click_does_what_the_mouse_s_does() {
        let mut h = Harness::new(Some(
            "layout {\n    media col=0 row=0 width=5 height=2\n}\n",
        ));
        let tree = h.app.accessibility_tree(h.win);
        let play = tree
            .find(blenderui::a11y::Role::Button, "Play or pause")
            .expect("the play button, named");
        assert!(h.app.accessibility_action(
            h.win,
            play.id,
            blenderui::a11y::Action::Click,
            None,
            0.0
        ));
        h.pump();
        assert!(h.took("PlayPause"), "{:?}", h.sent);
        let edit = h
            .app
            .accessibility_tree(h.win)
            .find(blenderui::a11y::Role::Button, "Edit")
            .map(|n| n.id)
            .expect("Edit");
        h.app
            .accessibility_action(h.win, edit, blenderui::a11y::Action::Click, None, 0.0);
        h.pump();
        assert!(h.dash().edit.is_some());
    }

    #[test]
    fn every_widget_draws_in_every_shape() {
        use strum::IntoEnumIterator;
        let mut text = String::from("layout {\n");
        let (mut col, mut row) = (0, 0);
        for k in Kind::iter() {
            let (w, h) = k.info().default_size;
            if col + w > 30 {
                col = 0;
                row += 6;
            }
            text.push_str(&format!(
                "    {} col={col} row={row} width={w} height={h}\n",
                k.name()
            ));
            col += w;
        }
        text.push_str("}\ngrid columns=30 rows=32\n");
        let mut h = Harness::new(Some(&text));
        assert_eq!(h.dash().layout.visible().len(), Kind::iter().count());
        // Into each one, and through its stops: nothing panics, the cursor stays on a stop.
        for (i, k) in Kind::iter().enumerate() {
            let id = h.id_of(k);
            with_dash(&mut h.app, |d, _| d.enter(id));
            h.keys("j j k");
            let _ = i;
            assert_eq!(h.dash().nav.active, id, "{k:?} kept the keyboard");
        }
        // And round, where they can be.
        with_dash(&mut h.app, |d, _| {
            let ids: Vec<u32> = d
                .layout
                .widgets
                .iter()
                .filter(|w| w.kind.info().can_be_round)
                .map(|w| w.id)
                .collect();
            for id in ids {
                d.layout.toggle_shape(id);
            }
        });
        h.pump();
    }
}

//! The mascot widgets: matching the board to the layout, their commands and their lines.

use super::edit::character;
use super::enums::outfit_to_ui;
use super::{App, set_if_changed};
use crate::Dash;
use crate::layout::Kind;
use niri_dash_system::{Tell, mascot};
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;
use web_time::Instant;

#[derive(Default)]
pub(super) struct Mascots {
    /// The mascot widgets and what each says, mirrored into `lines` by widget id.
    board: RefCell<crate::mascots::Board>,
    lines: Rc<VecModel<crate::MascotLine>>,
    /// Their once-a-second tick, while the dashboard is up.
    timer: Timer,
}

impl App {
    /// The dashboard came up or went away: the mascots pop up with something new, and their
    /// tick only runs while they can be seen.
    pub(super) fn mascots_awake(&self, on: bool) {
        self.ui().global::<crate::Mascots>().set_awake(on);
        if on {
            let todo = self.mascots.board.borrow_mut().wake(Instant::now());
            self.mascot_todo(todo);
            self.mascots.timer.restart();
        } else {
            self.mascots.timer.stop();
        }
    }

    /// Match the mascots to the mascot widgets on the dashboard (or the standalone app's).
    pub(super) fn sync_mascots(&self) {
        use crate::mascots::Spec;
        use crate::widget_settings::{Opt, get_int, get_str};
        let us = self.locale.us;
        let specs = self
            .placed_of(Kind::Mascot)
            .iter()
            .map(|p| {
                let v = &p.settings;
                Spec {
                    id: p.id,
                    character: character(p, us),
                    wildness: get_int(v, Opt::Wildness, us) as u8,
                    interval: Duration::from_secs(get_int(v, Opt::Interval, us) as u64),
                    command: get_str(v, Opt::Command, us),
                }
            })
            .collect();
        let todo = self.mascots.board.borrow_mut().sync(specs, Instant::now());
        self.mascot_todo(todo);
    }

    /// Run the commands the mascots asked for, then show what changed.
    pub(super) fn mascot_todo(&self, todo: Vec<crate::mascots::Todo>) {
        for t in todo {
            if let crate::mascots::Todo::Run { id, command, name } = t {
                self.tx
                    .mascot
                    .tell(mascot::Command::Run { id, name, command });
            }
        }
        self.push_mascots();
    }

    /// Mirror the board into `Mascots.lines`, indexed by widget id.
    fn push_mascots(&self) {
        let mut board = self.mascots.board.borrow_mut();
        if !board.take_dirty() {
            return;
        }
        let len = board.ids().max().map_or(0, |m| m as usize + 1);
        let mut rows = vec![crate::MascotLine::default(); len];
        for id in board.ids() {
            if let Some(v) = board.view(id) {
                rows[id as usize] = crate::MascotLine {
                    text: v.text.into(),
                    outfit: outfit_to_ui(v.outfit),
                    seq: v.seq,
                    queued: v.queued as i32,
                };
            }
        }
        drop(board);
        set_if_changed(&self.mascots.lines, rows, |a, b| a == b);
    }

    fn mascot_tick(&self) {
        let todo = self.mascots.board.borrow_mut().tick(Instant::now());
        self.mascot_todo(todo);
    }

    pub(super) fn mascot_say(&self, say: crate::mascots::Say) {
        let result = self.mascots.board.borrow_mut().say(&say, Instant::now());
        match result {
            Ok(_) => self.push_mascots(),
            Err(e) => {
                log::info!("mascots: {e}; showing it as a toast");
                self.toast(&say.text);
            }
        }
    }

    /// What a mascot widget's command printed (None: it failed); a command that fails, hangs or
    /// prints nothing gets a remark of the mascot's own.
    pub(super) fn mascot_output(&self, id: u32, stdout: Option<String>) {
        let out = stdout.and_then(|s| crate::mascots::from_command_output(&s));
        self.mascots
            .board
            .borrow_mut()
            .answer(id, out, Instant::now());
        self.push_mascots();
    }
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    ui.global::<crate::Mascots>()
        .set_lines(ModelRc::from(app.mascots.lines.clone()));
    // The cows named so far. One that can't be loaded isn't kept, so installing cowsay helps
    // without a restart; until then it is our own cow.
    let cows: RefCell<HashMap<String, Rc<crate::cowfile::Cow>>> = Default::default();
    let cow = Rc::new(move |name: &str| -> Rc<crate::cowfile::Cow> {
        if let Some(cow) = cows.borrow().get(name) {
            return cow.clone();
        }
        match crate::cowfile::load(name) {
            Ok(cow) => {
                let cow = Rc::new(cow);
                cows.borrow_mut().insert(name.to_string(), cow.clone());
                cow
            }
            Err(why) => {
                log::warn!("cow: {why}");
                Rc::new(crate::cowfile::Cow::own())
            }
        }
    });
    // Asked once, and only if a cow shows up.
    let font = std::cell::OnceCell::<slint::SharedString>::new();
    ui.global::<crate::Mascots>().on_cow_font(move || {
        font.get_or_init(|| crate::cowfile::monospace_family().into())
            .clone()
    });
    let c = cow.clone();
    ui.global::<crate::Mascots>()
        .on_cow_art(move |name, eyes, tongue| c(&name).draw(&eyes, &tongue).into());
    ui.global::<crate::Mascots>()
        .on_cow_scene(|art, text, width, height| {
            let scene = crate::mascots::cow_scene(&art, &text, width, height);
            crate::CowScene {
                text: scene.text.into(),
                font: scene.font,
                width: scene.width,
                height: scene.height,
            }
        });
    let a = Rc::downgrade(app);
    app.mascots
        .timer
        .start(TimerMode::Repeated, Duration::from_secs(1), move || {
            if let Some(a) = a.upgrade() {
                a.mascot_tick();
            }
        });
    app.mascots.timer.stop();
    let a = Rc::downgrade(app);
    ui.global::<crate::Mascots>().on_next(move |id| {
        if let Some(a) = a.upgrade() {
            let todo = a.mascots.board.borrow_mut().next(id as u32, Instant::now());
            a.mascot_todo(vec![todo]);
        }
    });
}

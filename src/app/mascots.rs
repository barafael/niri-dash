//! The mascot widgets: matching the board to the layout, their commands and their lines.

use super::App;
use super::edit::character;
use super::enums::outfit_to_ui;
use crate::Dash;
use crate::layout::{Kind, Placed};
use crate::sources::{Ctx, Msg};
use slint::{ComponentHandle, Model, ModelRc, TimerMode};
use std::rc::Rc;
use std::time::{Duration, Instant};

impl App {
    /// Match the mascots to the mascot widgets on the dashboard (or the standalone app's).
    pub(super) fn sync_mascots(&self) {
        use crate::mascots::Spec;
        use crate::widget_settings::{Opt, get_int, get_str};
        let us = self.locale.1;
        let layout = self.layout.borrow();
        let solo = self.solo.map(|k| layout.solo_widget(k));
        let placed: Vec<&Placed> = match &solo {
            Some(p) => vec![p],
            None => layout.visible(),
        };
        let specs = placed
            .into_iter()
            .filter(|p| p.kind == Kind::Mascot)
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
        drop(layout);
        let todo = self.mascots.borrow_mut().sync(specs, Instant::now());
        self.mascot_todo(todo);
    }

    /// Run the commands the mascots asked for, then show what changed.
    pub(super) fn mascot_todo(&self, todo: Vec<crate::mascots::Todo>) {
        for t in todo {
            if let crate::mascots::Todo::Run { id, command, name } = t {
                run_mascot_command(&self.ctx, id, command, name);
            }
        }
        self.push_mascots();
    }

    /// Mirror the board into `Mascots.lines`, indexed by widget id.
    fn push_mascots(&self) {
        let mut board = self.mascots.borrow_mut();
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
        let model = &self.mascot_lines;
        if model.row_count() == rows.len() {
            for (i, row) in rows.into_iter().enumerate() {
                if model.row_data(i).as_ref() != Some(&row) {
                    model.set_row_data(i, row);
                }
            }
        } else {
            model.set_vec(rows);
        }
    }

    fn mascot_tick(&self) {
        let todo = self.mascots.borrow_mut().tick(Instant::now());
        self.mascot_todo(todo);
    }

    pub(super) fn mascot_say(&self, say: crate::mascots::Say) {
        let result = self.mascots.borrow_mut().say(&say, Instant::now());
        match result {
            Ok(_) => self.push_mascots(),
            Err(e) => {
                log::info!("mascots: {e}; showing it as a toast");
                self.toast(&say.text);
            }
        }
    }

    pub(super) fn mascot_answer(
        &self,
        id: u32,
        out: Option<(String, Option<crate::mascots::Outfit>)>,
    ) {
        self.mascots.borrow_mut().answer(id, out, Instant::now());
        self.push_mascots();
    }
}

/// Run a mascot widget's `command` (with `NIRI_DASH_MASCOT` set to its name) and send back what
/// it printed. A command that fails, hangs or prints nothing gets a remark of the mascot's own.
fn run_mascot_command(ctx: &Ctx, id: u32, command: String, name: &'static str) {
    let ctx2 = ctx.clone();
    ctx.rt.spawn(async move {
        let run = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&command)
            .env("NIRI_DASH_MASCOT", name)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output();
        let out = match tokio::time::timeout(Duration::from_secs(10), run).await {
            Ok(Ok(o)) if o.status.success() => {
                crate::mascots::from_command_output(&String::from_utf8_lossy(&o.stdout))
            }
            Ok(Ok(o)) => {
                log::warn!("mascot command {command:?} failed: {}", o.status);
                None
            }
            Ok(Err(e)) => {
                log::warn!("mascot command {command:?}: {e}");
                None
            }
            Err(_) => {
                log::warn!("mascot command {command:?} took over 10 s");
                None
            }
        };
        ctx2.send(Msg::MascotAnswer(id, out));
    });
}

pub(super) fn wire(app: &Rc<App>, ui: &Dash) {
    ui.global::<crate::Mascots>()
        .set_lines(ModelRc::from(app.mascot_lines.clone()));
    let a = Rc::downgrade(&app);
    app.mascot_timer
        .start(TimerMode::Repeated, Duration::from_secs(1), move || {
            if let Some(a) = a.upgrade() {
                a.mascot_tick();
            }
        });
    app.mascot_timer.stop();
    let a = Rc::downgrade(&app);
    ui.global::<crate::Mascots>().on_next(move |id| {
        if let Some(a) = a.upgrade() {
            let todo = a.mascots.borrow_mut().next(id as u32, Instant::now());
            a.mascot_todo(vec![todo]);
        }
    });
}

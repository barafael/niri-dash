//! The dashboard's state and what happens to it: the layout, the look, which widget is entered
//! with the keyboard, edit mode, and what the system reports. It is a GPUI entity ([`Dash`]); the
//! views in `ui` and `widgets` only read it when they are drawn, and every change to it ends in
//! `cx.notify()`.
//!
//! Where the dashboard appears (a layer surface, or a window for a standalone app) is `window`'s
//! business; this asks for it through [`DashEvent`].

use crate::keys::{Code, Key, Named};
use crate::layout::{Kind, Layout, Loaded, Placed, Rect, Shape};
use crate::nav::{self, Dir};
use crate::theme::{self, Look};
use crate::widgets::Widgets;
use crate::{Control, own_pid};
use gpui::{Context, EventEmitter, FocusHandle};
use niri_dash_system::{self as sys, Capabilities, Event, Senders, Started, Tell};
use std::time::{Duration, Instant};

/// The top bar's height, and the dashboard's margins.
pub const MARGIN: f32 = 24.0;
pub const BAR: f32 = 44.0;

/// What the dashboard asks of whoever shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashEvent {
    /// Put the dashboard away (Escape, a click on the background).
    Hide,
    /// A control, as the control socket would send it (quit, say).
    Control(Control),
}

impl EventEmitter<DashEvent> for Dash {}

/// Where the keyboard is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavMode {
    /// Nothing entered.
    #[default]
    Off,
    /// The hint labels are up, and a label is being typed.
    Hints,
    /// A widget is entered: it has the keys.
    Widget,
}

/// What the direction keys do to the widget in hand (Ctrl+M, Ctrl+R): move it, or resize it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavOp {
    #[default]
    None,
    Move,
    Resize,
}

#[derive(Debug, Clone, Default)]
pub struct Nav {
    pub mode: NavMode,
    /// Move or resize mode: the widget in hand (the entered one, or the picked one in edit mode)
    /// keeps the keyboard, and h j k l or the arrows act on it until Escape. Read it through
    /// [`Dash::op`], which lets go of a mode whose widget is gone.
    pub op: NavOp,
    /// The widget the mode was started on.
    pub op_for: u32,
    /// The widget entered (0 = none).
    pub active_id: u32,
    /// The widget blown up in the middle of the dashboard with `e` (0 = none).
    pub expanded_id: u32,
    /// The part of a label typed so far.
    pub typed: String,
}

/// A placed widget as the views draw it: where it is, and its hint label.
#[derive(Debug, Clone)]
pub struct Tile {
    pub placed: Placed,
    pub hint: String,
}

impl Tile {
    pub fn id(&self) -> u32 {
        self.placed.id
    }

    pub fn kind(&self) -> Kind {
        self.placed.kind
    }

    pub fn round(&self) -> bool {
        self.placed.shape == Shape::Round
    }

    /// The cells it covers; a round tile is as tall as it is wide.
    pub fn rect(&self) -> Rect {
        let p = &self.placed;
        Rect {
            col: p.col,
            row: p.row,
            w: p.w,
            h: if p.shape == Shape::Round { p.w } else { p.h },
        }
    }
}

/// How the grid sits in the window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    pub cols: i32,
    pub rows: i32,
    /// The side of a cell, in pixels.
    pub cell: f32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// The width the grid and the cards have: less the drawer, when it is out.
    pub avail_w: f32,
}

/// A line that comes up over the dashboard for a while.
#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub until: Instant,
}

pub struct Dash {
    pub tx: Senders,
    pub caps: Capabilities,
    pub locale: sys::Locale,
    /// Set in a standalone app: the one widget it shows.
    pub solo: Option<Kind>,
    pub layout: Layout,
    pub look: Look,
    /// What the daemon keeps across restarts (do not disturb, the timer, picked colors).
    pub state: crate::state::State,
    pub visible: bool,
    pub editing: bool,
    pub nav: Nav,
    pub tiles: Vec<Tile>,
    pub hidden_widgets: usize,
    pub toast: Option<Toast>,
    pub focus: FocusHandle,
    /// The window's size when the layout was last fitted to it.
    pub size: (f32, f32),
    /// The state of each kind of widget.
    pub widgets: Widgets,
    /// config.kdl as we last wrote it, to ignore our own writes when watching the file.
    pub last_written: String,
    /// config.kdl doesn't parse, so edits aren't saved over it.
    pub config_broken: bool,
    /// How many events the system has delivered (for tests).
    pub delivered: u64,
    /// A text field has the keyboard: set for the key being handled, which then isn't the
    /// dashboard's (Escape apart).
    pub typing: bool,
    /// Tells the config watcher which files to watch.
    pub watch: Option<std::sync::mpsc::Sender<Vec<std::path::PathBuf>>>,
    /// A look being blended into the next.
    pub blend: Option<crate::look::Blend>,
    /// Installed font families for the Style tab (looked up on first use).
    pub fonts: Option<Vec<String>>,
    /// Edit mode: the drawer, the settings card, drags and the keyboard's place in them.
    pub edit: crate::edit::Edit,
}

impl Dash {
    pub fn new(system: Started, config: Loaded, solo: Option<Kind>, cx: &mut Context<Self>) -> Self {
        let Started { senders, caps } = system;
        theme::install_bundled();
        let Loaded {
            layout,
            warnings,
            broken,
            text,
        } = config;
        let (look, look_notes) =
            theme::load_look(&layout.colorscheme, &layout.style, &layout.style_overrides);
        let state = crate::state::load();
        let edit = crate::edit::Edit::new(caps, &layout);
        let mut dash = Self {
            tx: senders,
            caps,
            locale: sys::locale(),
            solo,
            layout,
            look,
            visible: false,
            editing: false,
            nav: Nav::default(),
            tiles: Vec::new(),
            hidden_widgets: 0,
            toast: None,
            focus: cx.focus_handle(),
            size: (0.0, 0.0),
            widgets: Widgets::default(),
            state,
            last_written: text,
            config_broken: broken,
            delivered: 0,
            typing: false,
            watch: None,
            blend: None,
            fonts: None,
            edit,
        };
        for w in warnings.iter().chain(&look_notes) {
            log::warn!("{w}");
        }
        let notes: Vec<String> = warnings.into_iter().chain(look_notes).collect();
        if !notes.is_empty() {
            dash.toast(&notes.join("; "));
        }
        crate::widgets::init(&mut dash);
        dash.sync_widgets();
        dash.reset_nav();
        dash
    }

    // ---- geometry -------------------------------------------------------------------------

    /// How the grid sits in a window of `width` by `height` pixels. With the drawer out the cells
    /// shrink to leave it room; the number of cells stays.
    pub fn geometry(&self, width: f32, height: f32) -> Geometry {
        let (cols, rows) = match self.layout.grid {
            Some((c, r)) => (c, r),
            None => auto_grid(width, height),
        };
        let room = crate::edit::Room::new(width, height, self.look.style.gap, self.edit.drawer_shift());
        let cell = ((room.avail_w - 2.0 * room.side) / cols as f32)
            .min((room.bottom - room.top) / rows as f32);
        Geometry {
            cols,
            rows,
            cell,
            x: (room.avail_w - cols as f32 * cell) / 2.0,
            y: room.top
                + (1.0 - room.shift) * (room.bottom - room.top - rows as f32 * cell) / 2.0,
            width,
            height,
            avail_w: room.avail_w,
        }
    }

    /// The window has this size: the layout adapts to the grid that fits it.
    pub fn set_size(&mut self, width: f32, height: f32) {
        if self.size == (width, height) {
            return;
        }
        self.size = (width, height);
        self.set_size_forced();
    }

    /// Fit the layout to the grid the current size (or the config's grid) gives.
    pub fn set_size_forced(&mut self) {
        let (width, height) = self.size;
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let g = self.geometry(width, height);
        if self.layout.cols != g.cols || self.layout.rows != g.rows {
            self.layout.set_screen_grid(g.cols, g.rows);
            self.sync_widgets();
        }
    }

    // ---- the widgets ----------------------------------------------------------------------

    /// The tiles from the layout, with hint labels.
    pub fn sync_widgets(&mut self) {
        if let Some(kind) = self.solo {
            let placed = self.layout.solo_widget(kind);
            self.tiles = vec![Tile {
                placed,
                hint: String::new(),
            }];
            self.hidden_widgets = 0;
            crate::widgets::layout_changed(self);
            return;
        }
        let mut placed: Vec<Placed> = self.layout.visible().into_iter().cloned().collect();
        crate::edit::previewed(&self.edit.preview, &mut placed);
        let ids: Vec<(u32, Kind)> = placed.iter().map(|p| (p.id, p.kind)).collect();
        let hints = nav::assign_hints(&ids);
        self.tiles = placed
            .into_iter()
            .zip(hints)
            .map(|(placed, hint)| Tile { placed, hint })
            .collect();
        self.hidden_widgets = self.layout.hidden_count();
        crate::widgets::layout_changed(self);
    }

    pub fn tile(&self, id: u32) -> Option<&Tile> {
        self.tiles.iter().find(|t| t.id() == id)
    }

    // ---- toasts ---------------------------------------------------------------------------

    pub fn toast(&mut self, text: &str) {
        let secs = (3 + text.len() / 40).min(10) as u64;
        self.toast = Some(Toast {
            text: text.to_string(),
            until: Instant::now() + Duration::from_secs(secs),
        });
    }

    // ---- showing and hiding ---------------------------------------------------------------

    /// The system is told whether the dashboard is up: sources only watch and poll while it is.
    pub fn set_visible(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.visible == on {
            return;
        }
        self.visible = on;
        if on {
            self.come_up();
        } else {
            self.go_down();
        }
        self.tx.host.tell(sys::host::Command::Visible(on));
        crate::widgets::awake(self, on);
        cx.notify();
    }

    /// The dashboard's part of coming up: every widget back on its first page, no widget entered
    /// (a standalone app is in its one widget).
    fn come_up(&mut self) {
        crate::edit::come_up(self);
        crate::widgets::come_up(self);
        self.reset_nav();
    }

    /// Going away: out of edit mode, menus shut, no widget entered.
    fn go_down(&mut self) {
        self.editing = false;
        crate::edit::go_down(self);
        // Nothing entered or expanded first: the widgets keep their cursors on stops of the
        // sizes they have from now on (nothing is drawn to do it while the dashboard is away).
        self.reset_nav();
        crate::widgets::go_down(self);
    }

    /// Back to no widget entered. A standalone app is always in its one widget.
    pub fn reset_nav(&mut self) {
        self.nav.op = NavOp::None;
        self.nav.typed.clear();
        self.nav.expanded_id = 0;
        match self.solo {
            Some(_) => {
                self.nav.active_id = self.tiles.first().map_or(0, Tile::id);
                self.nav.mode = NavMode::Widget;
            }
            None => {
                self.nav.mode = NavMode::Off;
                self.nav.active_id = 0;
            }
        }
    }

    /// A control, from the control socket, the hotkey or the stress run.
    pub fn control(&mut self, c: Control, cx: &mut Context<Self>) {
        match c {
            Control::Show => self.set_visible(true, cx),
            Control::Edit => {
                self.set_visible(true, cx);
                if self.solo.is_none() {
                    self.set_editing(true, cx);
                }
            }
            Control::Hide => self.set_visible(false, cx),
            Control::Toggle => self.set_visible(!self.visible, cx),
            Control::Quit => cx.emit(DashEvent::Control(Control::Quit)),
            Control::Inhibit => self.tx.power.tell(sys::power::Command::ToggleInhibit),
            Control::Dnd => self.toggle_dnd(cx),
        }
        cx.notify();
    }

    pub fn toggle_dnd(&mut self, cx: &mut Context<Self>) {
        crate::widgets::toggle_dnd(self);
        cx.notify();
    }

    pub fn set_editing(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.editing == on {
            return;
        }
        self.editing = on;
        if on && self.nav.mode != NavMode::Off {
            self.nav.mode = NavMode::Off;
            self.nav.typed.clear();
        }
        self.nav.expanded_id = 0;
        self.nav.op = NavOp::None;
        crate::edit::editing_changed(self, on);
        cx.notify();
    }

    /// A second passed (just after the change), while the dashboard is up.
    pub fn tick(&mut self, cx: &mut Context<Self>) {
        if !self.visible {
            return;
        }
        if self.toast.as_ref().is_some_and(|t| Instant::now() >= t.until) {
            self.toast = None;
        }
        crate::widgets::tick(self);
        cx.notify();
    }

    /// `niri-dash night-light …` arrived on the control socket.
    pub fn night_light_command(&mut self, cmd: sys::night_light::Cmd, cx: &mut Context<Self>) {
        crate::widgets::night_light_command(self, cmd, cx);
    }

    /// `niri-dash say …` arrived on the control socket.
    pub fn mascot_say(&mut self, say: crate::mascots::Say, cx: &mut Context<Self>) {
        crate::widgets::mascot_say(self, say, cx);
    }

    // ---- what the system reports ----------------------------------------------------------

    pub fn handle_event(&mut self, event: Event, cx: &mut Context<Self>) {
        self.delivered += 1;
        if let Some(control) = crate::widgets::event(self, event) {
            // The window (opening it again, say) is the host's to deal with.
            cx.emit(DashEvent::Control(control));
        }
        cx.notify();
    }

    // ---- keys -----------------------------------------------------------------------------

    /// A key press, with the dashboard focused. True if it was taken.
    pub fn key(&mut self, key: &Key, cx: &mut Context<Self>) -> bool {
        let taken = self.key_inner(key, cx);
        self.tidy_op();
        if taken {
            // A key may have changed what a widget is drawn as (expanded, left): its cursor
            // stays on a stop of the new shape.
            crate::widgets::resized(self);
            cx.notify();
        }
        taken
    }

    fn key_inner(&mut self, key: &Key, cx: &mut Context<Self>) -> bool {
        if key.is_named(Named::Escape) {
            self.escape(cx);
            return true;
        }
        // A text field of edit mode has the keyboard: its keys are its own.
        if self.editing && self.edit.field.focused {
            return crate::edit::field_key(self, key, cx);
        }
        // Ctrl+M moves the widget in hand, Ctrl+R resizes it.
        if let Some(op) = op_key(key)
            && self.solo.is_none()
            && !self.edit.dragging()
            && self.edit.adjust_id == 0
            && self.op_target() != 0
        {
            self.op_toggle(op);
            return true;
        }
        if self.op() != NavOp::None && self.solo.is_none() && !self.edit.dragging() && key.bare() {
            // In the mode h j k l (and the arrows) act on the widget in hand, Shift pulling a side
            // in instead of out; the rest of the keys wait for Escape.
            if let Some(d) = key_dir(key) {
                let id = self.op_target();
                if self.nav.op == NavOp::Move {
                    self.nudge_widget(id, d);
                } else {
                    let shrink = key.shift || matches!(key.code, Code::Char(c) if c.is_ascii_uppercase());
                    self.size_widget(id, d, !shrink);
                }
            }
            return true;
        }
        // What a text field types is the field's; Ctrl and Alt keys stay the dashboard's.
        if self.typing && key.bare() {
            return false;
        }
        if self.nav.mode == NavMode::Widget && crate::widgets::capturing(self) {
            return crate::widgets::widget_key(self, key, cx);
        }
        if self.nav.mode == NavMode::Hints {
            return self.hints_key(key, cx);
        }
        if self.solo.is_none()
            && !self.edit.dragging()
            && self.edit.adjust_id == 0
            && key.is('f')
            && key.bare()
        {
            self.nav.typed.clear();
            self.nav.expanded_id = 0;
            self.nav.mode = NavMode::Hints;
            return true;
        }
        if self.editing && self.solo.is_none() {
            return crate::widgets::edit_key(self, key, cx);
        }
        if self.nav.mode == NavMode::Widget && self.solo.is_none() {
            // s opens the widget's settings, in every widget.
            if key.is('s') && key.bare() {
                crate::widgets::open_settings(self, self.nav.active_id, cx);
                return true;
            }
            // e blows the widget up in the middle of the dashboard, and back.
            if key.is('e') && key.bare() {
                self.nav.expanded_id = if self.nav.expanded_id == self.nav.active_id {
                    0
                } else {
                    self.nav.active_id
                };
                return true;
            }
            if let Some(d) = nav_dir(key) {
                if let Some(next) = self.neighbor(self.nav.active_id, d) {
                    self.enter(next);
                }
                return true;
            }
        }
        if self.solo.is_none() && !self.editing && key.is('e') && key.ctrl && !key.alt && !key.meta {
            // From inside a widget: that widget is the one edit mode starts on.
            let entered = (self.nav.mode == NavMode::Widget).then_some(self.nav.active_id);
            self.nav_off();
            self.set_editing(true, cx);
            if let Some(id) = entered {
                crate::widgets::configure(self, id, cx);
            }
            return true;
        }
        if self.nav.mode == NavMode::Widget {
            return crate::widgets::widget_key(self, key, cx);
        }
        false
    }

    /// Escape closes the innermost thing that is open.
    fn escape(&mut self, cx: &mut Context<Self>) {
        // A text field of edit mode, a drag, a mode of the widget in hand, a widget opened alone.
        if crate::edit::escape(self, cx) {
            return;
        }
        if crate::widgets::escape(self, cx) {
            return;
        }
        if self.nav.expanded_id != 0 && self.nav.mode == NavMode::Widget {
            self.nav.expanded_id = 0;
        } else if self.nav.mode != NavMode::Off && self.solo.is_none() {
            self.nav_off();
        } else if self.editing {
            // The settings card closes and the widget is let go, or edit mode ends.
            crate::edit::escape_editing(self, cx);
        } else {
            cx.emit(DashEvent::Hide);
        }
    }

    fn hints_key(&mut self, key: &Key, cx: &mut Context<Self>) -> bool {
        // f hides the hints before anything is typed; Backspace takes back what was.
        if key.is_named(Named::Backspace) && !self.nav.typed.is_empty() {
            self.nav.typed.clear();
            return true;
        }
        if (key.is('f') && self.nav.typed.is_empty()) || key.is_named(Named::Backspace) {
            self.nav_off();
            return true;
        }
        if self.solo.is_none() && key.is('e') && key.ctrl && !key.alt && !key.meta {
            self.nav_off();
            self.set_editing(true, cx);
            return true;
        }
        if key.ctrl || key.alt || key.meta || key.is_named(Named::Shift) {
            return true;
        }
        let Some(c) = key.text() else {
            return true;
        };
        let typed = format!("{}{c}", self.nav.typed);
        let labels: Vec<String> = self.tiles.iter().map(|t| t.hint.clone()).collect();
        match nav::pick(&labels, &typed) {
            Ok(Some(i)) => {
                let id = self.tiles[i].id();
                if self.editing {
                    // In edit mode a label picks the widget to work on: its settings open.
                    self.nav_off();
                    crate::widgets::configure(self, id, cx);
                } else {
                    self.enter(id);
                }
            }
            // Labels are lowercase and `pick` ignores the case; what is kept is what was matched.
            Ok(None) => self.nav.typed = typed.to_lowercase(),
            Err(()) => self.nav_off(),
        }
        true
    }

    /// The widget the keyboard's move and resize modes act on: the one entered, or in edit mode
    /// the picked one (0 = none, and no mode).
    pub fn op_target(&self) -> u32 {
        if self.solo.is_some() {
            0
        } else if self.editing {
            self.edit.settings_id
        } else if self.nav.mode == NavMode::Widget {
            self.nav.active_id
        } else {
            0
        }
    }

    /// The mode in force: none once another widget (or none) is in hand.
    pub fn op(&self) -> NavOp {
        if self.nav.op != NavOp::None && self.op_target() == self.nav.op_for {
            self.nav.op
        } else {
            NavOp::None
        }
    }

    /// Let go of a mode whose widget is not in hand any more.
    pub fn tidy_op(&mut self) {
        if self.op() == NavOp::None {
            self.nav.op = NavOp::None;
            self.nav.op_for = 0;
        }
    }

    /// Ctrl+M / Ctrl+R: start the mode, switch to it from the other one, or end it. The widget
    /// gives up the keyboard while it lasts, so h j k l reach the mode and not the widget.
    fn op_toggle(&mut self, op: NavOp) {
        let id = self.op_target();
        if id == 0 {
            return;
        }
        if self.op() == op {
            self.op_end();
        } else {
            self.size_note(id);
            self.nav.expanded_id = 0;
            self.edit.side_keys = false;
            self.edit.canvas_keys = true;
            self.edit.field.to_dash();
            self.nav.op = op;
            self.nav.op_for = id;
        }
    }

    /// Escape, or the mode's key again: the entered widget takes the keyboard back.
    pub fn op_end(&mut self) {
        self.nav.op = NavOp::None;
        self.nav.op_for = 0;
        self.edit.field.to_dash();
    }

    /// The hints go down, nothing entered.
    pub fn nav_off(&mut self) {
        self.nav.op = NavOp::None;
        self.nav.typed.clear();
        self.nav.expanded_id = 0;
        self.nav.mode = NavMode::Off;
    }

    /// Enter widget `id`: it has the keys now.
    pub fn enter(&mut self, id: u32) {
        self.nav.typed.clear();
        self.nav.expanded_id = 0;
        self.nav.active_id = id;
        self.nav.mode = NavMode::Widget;
        crate::widgets::entered(self, id);
    }

    /// The tile next to `id` in direction `d`.
    pub fn neighbor(&self, id: u32, d: Dir) -> Option<u32> {
        let from = self.tiles.iter().position(|t| t.id() == id)?;
        let rects: Vec<Rect> = self.tiles.iter().map(Tile::rect).collect();
        nav::neighbor(&rects, from, d).map(|i| self.tiles[i].id())
    }

    /// Whether the dashboard has anything entered or open that Escape would close first.
    pub fn place(&self) -> Place {
        if !self.visible {
            Place::Closed
        } else if self.editing {
            Place::Edit
        } else {
            match self.nav.mode {
                NavMode::Off => Place::Off,
                NavMode::Hints => Place::Hints,
                NavMode::Widget => Place::Widget,
            }
        }
    }

    /// Focus the widget of this process in the compositor's window list (standalone app).
    pub fn focus_self(&self) {
        self.tx
            .windows
            .tell(sys::windows::Command::FocusPid(own_pid() as i32));
    }
}

/// Where the dashboard is, as far as the keys are concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Closed,
    Off,
    Hints,
    Widget,
    Edit,
}

/// The automatic grid: as many cells as fit the screen.
pub fn auto_grid(width: f32, height: f32) -> (i32, i32) {
    let cell = crate::layout::AUTO_CELL;
    (
        (((width - 2.0 * MARGIN) / cell).floor() as i32).max(4),
        (((height - BAR - MARGIN) / cell).floor() as i32).max(3),
    )
}

/// h/j/k/l or the arrows, either case: the direction.
pub fn key_dir(key: &Key) -> Option<Dir> {
    match key.code {
        Code::Char('h' | 'H') | Code::Named(Named::Left) => Some(Dir::Left),
        Code::Char('j' | 'J') | Code::Named(Named::Down) => Some(Dir::Down),
        Code::Char('k' | 'K') | Code::Named(Named::Up) => Some(Dir::Up),
        Code::Char('l' | 'L') | Code::Named(Named::Right) => Some(Dir::Right),
        _ => None,
    }
}

/// Ctrl+M (move) or Ctrl+R (resize), without Alt, Meta or Shift.
fn op_key(key: &Key) -> Option<NavOp> {
    if !key.ctrl || key.alt || key.meta || key.shift {
        return None;
    }
    match key.code {
        Code::Char('m') => Some(NavOp::Move),
        Code::Char('r') => Some(NavOp::Resize),
        _ => None,
    }
}

/// Ctrl+h/j/k/l, H/J/K/L or Ctrl+arrows: the direction to the next widget.
fn nav_dir(key: &Key) -> Option<Dir> {
    if key.alt || key.meta {
        return None;
    }
    if key.ctrl {
        return key_dir(key);
    }
    match key.code {
        Code::Char('H') => Some(Dir::Left),
        Code::Char('J') => Some(Dir::Down),
        Code::Char('K') => Some(Dir::Up),
        Code::Char('L') => Some(Dir::Right),
        _ => None,
    }
}

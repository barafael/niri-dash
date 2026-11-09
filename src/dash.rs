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

#[derive(Debug, Clone, Default)]
pub struct Nav {
    pub mode: NavMode,
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
    /// Tells the config watcher which files to watch.
    pub watch: Option<std::sync::mpsc::Sender<Vec<std::path::PathBuf>>>,
    /// A look being blended into the next.
    pub blend: Option<crate::look::Blend>,
    /// Installed font families for the Style tab (looked up on first use).
    pub fonts: Option<Vec<String>>,
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
            watch: None,
            blend: None,
            fonts: None,
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

    /// How the grid sits in a window of `width` by `height` pixels.
    pub fn geometry(&self, width: f32, height: f32) -> Geometry {
        let (cols, rows) = match self.layout.grid {
            Some((c, r)) => (c, r),
            None => auto_grid(width, height),
        };
        let side = MARGIN;
        let (top, bottom) = (BAR, height - MARGIN);
        let cell = ((width - 2.0 * side) / cols as f32).min((bottom - top) / rows as f32);
        Geometry {
            cols,
            rows,
            cell,
            x: (width - cols as f32 * cell) / 2.0,
            y: top + (bottom - top - rows as f32 * cell) / 2.0,
            width,
            height,
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
            return;
        }
        let placed: Vec<Placed> = self.layout.visible().into_iter().cloned().collect();
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
        crate::widgets::come_up(self);
        self.reset_nav();
    }

    /// Going away: out of edit mode, menus shut, no widget entered.
    fn go_down(&mut self) {
        self.editing = false;
        crate::widgets::go_down(self);
        self.reset_nav();
    }

    /// Back to no widget entered. A standalone app is always in its one widget.
    pub fn reset_nav(&mut self) {
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
    pub fn night_light_command(&mut self, _cmd: sys::night_light::Cmd, _cx: &mut Context<Self>) {}

    /// `niri-dash say …` arrived on the control socket.
    pub fn mascot_say(&mut self, _say: crate::mascots::Say, _cx: &mut Context<Self>) {}

    // ---- what the system reports ----------------------------------------------------------

    pub fn handle_event(&mut self, event: Event, cx: &mut Context<Self>) {
        self.delivered += 1;
        if let Some(control) = crate::widgets::event(self, event) {
            self.control(control, cx);
        }
        cx.notify();
    }

    // ---- keys -----------------------------------------------------------------------------

    /// A key press, with the dashboard focused. True if it was taken.
    pub fn key(&mut self, key: &Key, cx: &mut Context<Self>) -> bool {
        let taken = self.key_inner(key, cx);
        if taken {
            cx.notify();
        }
        taken
    }

    fn key_inner(&mut self, key: &Key, cx: &mut Context<Self>) -> bool {
        if key.is_named(Named::Escape) {
            self.escape(cx);
            return true;
        }
        if self.nav.mode == NavMode::Hints {
            return self.hints_key(key, cx);
        }
        if self.solo.is_none() && key.is('f') && key.bare() {
            self.nav.typed.clear();
            self.nav.expanded_id = 0;
            self.nav.mode = NavMode::Hints;
            return true;
        }
        if self.editing && self.solo.is_none() {
            return crate::widgets::edit_key(self, key, cx);
        }
        if self.nav.mode == NavMode::Widget && self.solo.is_none() {
            // Ctrl+Shift+h/j/k/l moves the widget a cell, pushing others aside.
            if key.ctrl && key.shift && !key.alt && !key.meta
                && let Some(d) = key_dir(key)
            {
                self.nav.expanded_id = 0;
                crate::widgets::nudge(self, self.nav.active_id, d, cx);
                return true;
            }
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
        if crate::widgets::escape(self, cx) {
            return;
        }
        if self.nav.expanded_id != 0 && self.nav.mode == NavMode::Widget {
            self.nav.expanded_id = 0;
        } else if self.nav.mode != NavMode::Off && self.solo.is_none() {
            self.nav_off();
        } else if self.editing {
            self.set_editing(false, cx);
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
            Ok(None) => self.nav.typed = typed,
            Err(()) => self.nav_off(),
        }
        true
    }

    /// The hints go down, nothing entered.
    pub fn nav_off(&mut self) {
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

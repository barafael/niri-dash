//! The dashboard's state and everything that happens to it: what the system reports, the
//! control socket, the clock's tick and the keyboard. [`Dash`] is a BUI entity that outlives the
//! window (the window is closed when the dashboard hides); the views only read it, and every
//! change ends in [`Dash::changed`].

use crate::data::{Data, Followup};
use crate::edit::Edit;
use crate::layout::{Kind, Layout, Loaded, Placed, Rect, Shape};
use crate::look::{self, Look};
use crate::nav::{self, Dir};
use crate::widgets::{self, Ctx, Stop};
use crate::{Control, state};
use blenderui::prelude::*;
use niri_dash_system::{self as sys, Capabilities, Command, Tell};
use std::collections::HashMap;
use std::time::Duration;
use web_time::Instant;

/// The top bar and the quick tips, and the margin around the grid.
pub const BAR: f32 = 30.0;
pub const TIPS: f32 = 44.0;
pub const MARGIN: f32 = 14.0;

/// How long a slider the user moved keeps its value against what the source reports, which
/// lags behind.
const HOLD: Duration = Duration::from_millis(1500);

/// What the dashboard asks of the window it is shown in. Done on the next turn of the loop
/// ([`crate::app::apply`]), outside of any listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WinOp {
    Show,
    Hide,
    Quit,
}

/// Where the keyboard is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Off,
    /// The hint labels are up.
    Hints,
    /// A widget is entered.
    Widget,
}

/// What the direction keys do to the widget in hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Move,
    Resize,
}

#[derive(Debug, Clone, Default)]
pub struct Nav {
    pub mode: Mode,
    pub typed: String,
    /// The widget entered, or picked in edit mode (0: none).
    pub active: u32,
    /// The stop the cursor is on in the entered widget.
    pub cursor: usize,
    /// Blown up with `e` (0: none).
    pub expanded: u32,
    pub op: Option<Op>,
}

/// The grid in the window: where it starts and how big a cell is.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Geometry {
    pub x: f32,
    pub y: f32,
    pub cell: f32,
    pub width: f32,
    pub height: f32,
}

impl Geometry {
    /// Where a rectangle of cells is, in pixels: (x, y, w, h), with the gap taken off.
    pub fn place(&self, r: Rect, gap: f32) -> (f32, f32, f32, f32) {
        (
            self.x + r.col as f32 * self.cell + gap,
            self.y + r.row as f32 * self.cell + gap,
            r.w as f32 * self.cell - 2.0 * gap,
            r.h as f32 * self.cell - 2.0 * gap,
        )
    }
}

pub struct Dash {
    pub tx: sys::Sink<Command>,
    pub caps: Capabilities,
    pub locale: sys::Locale,
    /// A standalone app: the one widget it shows.
    pub solo: Option<Placed>,
    pub layout: Layout,
    pub look: Look,
    /// What is kept across restarts (do not disturb, the timer, picked colors).
    pub saved: state::State,
    pub data: Data,
    pub w: widgets::State,
    pub visible: bool,
    pub nav: Nav,
    pub edit: Option<Edit>,
    pub toast: Option<(String, Instant)>,
    /// The window's size, as last drawn.
    pub size: (f32, f32),
    /// Sliders the user moved, and when.
    held: HashMap<&'static str, (f32, Instant)>,
    /// config.kdl as we last wrote or read it, to tell our own writes from the user's.
    pub config_text: String,
    pub config_broken: bool,
    /// What the window is to do next.
    pub ops: Vec<WinOp>,
    /// The text field to type into next ([`crate::app::apply_focus`]).
    pub focus: Option<String>,
    pub ui: Option<UiSender>,
}

impl Dash {
    pub fn new(system: sys::System, config: Loaded, solo: Option<Kind>) -> Self {
        let Loaded {
            layout,
            warnings,
            broken,
            text,
        } = config;
        let (look, notes) =
            look::resolve(&layout.colorscheme, &layout.style, &layout.style_overrides);
        let saved = state::load();
        let locale = sys::locale();
        let solo = solo.map(|k| layout.solo_widget(k));
        let mut d = Self {
            tx: system.commands,
            caps: system.caps,
            locale,
            solo,
            w: widgets::State::new(&saved),
            layout,
            look,
            saved,
            data: Data::default(),
            visible: false,
            nav: Nav::default(),
            edit: None,
            toast: None,
            size: (0.0, 0.0),
            held: HashMap::new(),
            config_text: text,
            config_broken: broken,
            ops: Vec::new(),
            focus: None,
            ui: None,
        };
        for w in warnings.iter().chain(&notes) {
            log::warn!("{w}");
        }
        if let Some(first) = warnings.first() {
            d.toast(first.clone());
        }
        d.configure_system();
        // The widgets' own state follows the layout from the start (a mascot can be told
        // something before the first tick).
        d.tick();
        d
    }

    /// Hand the system what the config says (where the weather is, night light as it was).
    fn configure_system(&mut self) {
        self.settings_changed();
        let nl = self.saved.night_light;
        self.tx.tell(sys::night_light::Command::Restore {
            on: nl.on,
            kelvin: nl.kelvin,
        });
        let dnd = sys::notifications::Dnd::from_saved(self.saved.dnd_until);
        self.tx.tell(sys::notifications::Command::SetDnd(dnd));
    }

    /// The widgets' settings changed: what the system needs to know of them.
    pub fn settings_changed(&mut self) {
        self.tx.tell(sys::weather::Command::Configure(
            self.layout.weather_config(self.locale.us),
        ));
        self.tx.tell(sys::clipboard::Command::Record(
            self.layout
                .widgets
                .iter()
                .any(|w| w.kind == Kind::Clipboard)
                || self
                    .solo
                    .as_ref()
                    .is_some_and(|p| p.kind == Kind::Clipboard),
        ));
    }

    pub fn tell(&self, cmd: impl Into<Command>) {
        self.tx.tell(cmd);
    }

    /// Something changed: draw again.
    pub fn changed(&mut self, cx: &mut Context<Self>) {
        cx.notify();
    }

    /// Ask the window for something, on the next turn of the loop.
    pub fn request(&mut self, op: WinOp) {
        self.ops.push(op);
        if let Some(ui) = &self.ui {
            ui.send(crate::app::apply);
        }
    }

    /// Type into the text field with this id, on the next turn of the loop.
    pub fn type_into(&mut self, id: String) {
        self.focus = Some(id);
        if let Some(ui) = &self.ui {
            ui.send(crate::app::apply_focus);
        }
    }

    pub fn toast(&mut self, text: impl Into<String>) {
        let text = text.into();
        log::info!("toast: {text}");
        let secs = (3 + text.len() / 40).min(10) as u64;
        self.toast = Some((text, Instant::now() + Duration::from_secs(secs)));
    }

    /// The value a slider shows: the user's, while they are at it, then the system's.
    pub fn held(&self, key: &'static str, actual: f32) -> f32 {
        match self.held.get(key) {
            Some((v, at)) if at.elapsed() < HOLD => *v,
            _ => actual,
        }
    }

    pub fn hold(&mut self, key: &'static str, v: f32) {
        self.held.insert(key, (v, Instant::now()));
    }

    pub fn save_state(&mut self) {
        state::save(&self.saved);
    }

    // -- coming and going ---------------------------------------------------------------------

    pub fn control(&mut self, c: Control, cx: &mut Context<Self>) {
        match c {
            Control::Show => self.show(false),
            Control::Edit => self.show(true),
            Control::Hide => self.hide(),
            Control::Toggle => {
                if self.visible {
                    self.hide()
                } else {
                    self.show(false)
                }
            }
            Control::Quit => self.request(WinOp::Quit),
            Control::Inhibit => self.tell(sys::power::Command::ToggleInhibit),
            Control::Dnd => crate::widgets::toggle_dnd(self),
        }
        self.changed(cx);
    }

    pub fn show(&mut self, edit: bool) {
        if !self.visible {
            self.visible = true;
            self.nav = Nav::default();
            if let Some(p) = &self.solo {
                // A standalone app starts inside its widget.
                self.nav.mode = Mode::Widget;
                self.nav.active = p.id;
            }
            self.tell(sys::host::Command::Visible(true));
            self.w.awake(true, &self.tx);
            self.request(WinOp::Show);
        }
        if edit && self.solo.is_none() && self.edit.is_none() {
            self.edit = Some(Edit::new(0));
        }
    }

    pub fn hide(&mut self) {
        if self.solo.is_some() {
            // A standalone app's window is its whole life.
            self.request(WinOp::Quit);
            return;
        }
        if !self.visible {
            return;
        }
        self.visible = false;
        self.edit = None;
        self.nav = Nav::default();
        self.data.menu = None;
        self.tell(sys::host::Command::Visible(false));
        self.w.awake(false, &self.tx);
        self.w.save_notes();
        self.request(WinOp::Hide);
    }

    /// The window went away without being asked (the compositor closed it).
    pub fn window_closed(&mut self) {
        if self.visible {
            self.visible = false;
            self.edit = None;
            self.nav = Nav::default();
            self.tell(sys::host::Command::Visible(false));
            self.w.awake(false, &self.tx);
            self.w.save_notes();
        }
    }

    // -- the system ---------------------------------------------------------------------------

    pub fn event(&mut self, e: sys::Event) {
        match self.data.apply(e) {
            Followup::None => {}
            Followup::Toast(t) => self.toast(t),
            Followup::MascotSaid(id, out) => self.w.mascot_said(id, out),
            Followup::Sleep(e) => self.w.sleep_event(e),
            Followup::Picked(c) => {
                let hex = format!(
                    "#{:02x}{:02x}{:02x}",
                    (c[0] * 255.0).round() as u8,
                    (c[1] * 255.0).round() as u8,
                    (c[2] * 255.0).round() as u8
                );
                self.saved.colors.retain(|x| *x != hex);
                self.saved.colors.insert(0, hex.clone());
                self.saved.colors.truncate(12);
                self.save_state();
                self.toast(format!("Picked {hex}"));
            }
        }
    }

    /// Once in a while (four times a second): timers, the toast, mascots. True if anything on
    /// screen changed.
    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        if self
            .toast
            .as_ref()
            .is_some_and(|(_, until)| Instant::now() >= *until)
        {
            self.toast = None;
            changed = true;
        }
        let tx = self.tx.clone();
        let placed: Vec<Placed> = match &self.solo {
            Some(p) => vec![p.clone()],
            None => self.layout.widgets.clone(),
        };
        let fired = self.w.tick(&tx, &placed, self.visible);
        for text in fired {
            self.toast(text);
            changed = true;
        }
        changed || self.visible
    }

    // -- the layout ---------------------------------------------------------------------------

    /// The widgets on screen, with their hint labels.
    pub fn tiles(&self) -> Vec<(&Placed, String)> {
        if let Some(p) = &self.solo {
            return vec![(p, String::new())];
        }
        let shown = self.layout.visible();
        let ids: Vec<(u32, Kind)> = shown.iter().map(|p| (p.id, p.kind)).collect();
        let labels = nav::assign_hints(&ids);
        shown.into_iter().zip(labels).collect()
    }

    pub fn placed(&self, id: u32) -> Option<&Placed> {
        match &self.solo {
            Some(p) if p.id == id => Some(p),
            Some(_) => None,
            None => self.layout.get(id),
        }
    }

    /// Where the grid goes in a window of `w` x `h`; also sets the automatic grid for it.
    pub fn fit(&mut self, w: f32, h: f32) {
        self.size = (w, h);
        let side = if self.edit.is_some() {
            crate::edit::SIDEBAR
        } else {
            0.0
        };
        let avail_w = (w - 2.0 * MARGIN - side).max(100.0);
        let avail_h = (h - BAR - TIPS - 2.0 * MARGIN).max(100.0);
        let _ = (avail_w, avail_h);
        // As many 100 px cells as the Slint version counts (its margin is 24 px, its bar 44), so
        // a screen shows the same widgets in both; the cells then shrink to what fits here.
        self.layout.set_screen_grid(
            (((w - 48.0) / crate::layout::AUTO_CELL).floor() as i32).max(4),
            (((h - 68.0) / crate::layout::AUTO_CELL).floor() as i32).max(3),
        );
    }

    pub fn geometry(&self) -> Geometry {
        let (w, h) = self.size;
        let side = if self.edit.is_some() {
            crate::edit::SIDEBAR
        } else {
            0.0
        };
        let avail_w = (w - 2.0 * MARGIN - side).max(100.0);
        let avail_h = (h - BAR - TIPS - 2.0 * MARGIN).max(100.0);
        let (cols, rows) = (
            self.layout.cols.max(1) as f32,
            self.layout.rows.max(1) as f32,
        );
        let cell = (avail_w / cols).min(avail_h / rows).floor();
        let width = cell * cols;
        let height = cell * rows;
        Geometry {
            x: MARGIN + (avail_w - width) / 2.0,
            y: BAR + MARGIN + (avail_h - height) / 2.0,
            cell,
            width,
            height,
        }
    }

    /// Write what the layout became into config.kdl (not while it is broken: the user's text
    /// stays until they fix it).
    pub fn save_layout(&mut self) {
        if self.config_broken {
            self.toast("config.kdl doesn't parse: changes aren't saved");
            return;
        }
        self.config_text = self.layout.save();
    }

    /// config.kdl (or a theme file) changed on disk.
    pub fn reload(&mut self) {
        let loaded = Layout::read();
        if loaded.text == self.config_text && !loaded.broken {
            // Our own write; the theme files may have changed though.
            self.relook();
            return;
        }
        self.config_text = loaded.text;
        self.config_broken = loaded.broken;
        if let Some(w) = loaded.warnings.first() {
            self.toast(w.clone());
        }
        if !loaded.broken {
            let refit = self.layout.replace(loaded.layout);
            if let Some(m) = refit.message() {
                self.toast(m);
            }
        }
        self.relook();
        self.settings_changed();
        self.tick();
        if self.placed(self.nav.active).is_none() {
            self.nav = Nav::default();
        }
    }

    pub fn relook(&mut self) {
        let (look, notes) = look::resolve(
            &self.layout.colorscheme,
            &self.layout.style,
            &self.layout.style_overrides,
        );
        if let Some(n) = notes.first() {
            self.toast(n.clone());
        }
        self.look = look;
    }

    // -- the keyboard -------------------------------------------------------------------------

    /// The stops and binds of the widget entered, as it is drawn now.
    pub fn entered_ctx(&self, hd: WeakEntity<Dash>) -> Option<(Vec<Stop>, Vec<widgets::Bind>)> {
        let p = self.placed(self.nav.active)?;
        let g = self.geometry();
        let big = self.nav.expanded == p.id || self.solo.is_some();
        let (w, h) = if big {
            (self.size.0 * 0.6, self.size.1 * 0.6)
        } else {
            let (_, _, w, h) = g.place(p.rect(), self.look.style.gap);
            (w, h - 20.0)
        };
        let mut c = Ctx::new(self, p, w, h, Some(self.nav.cursor), big, hd);
        let _ = widgets::build(&mut c);
        Some((c.stops, c.binds))
    }

    pub fn enter(&mut self, id: u32) {
        self.nav.mode = Mode::Widget;
        self.nav.active = id;
        self.nav.cursor = 0;
        self.nav.typed.clear();
        self.nav.op = None;
    }

    pub fn leave(&mut self) {
        if self.solo.is_some() {
            return;
        }
        self.nav = Nav::default();
    }

    /// The neighbouring widget that way from the entered one (or the first one).
    fn neighbour(&self, dir: Dir) -> Option<u32> {
        let tiles = self.tiles();
        let rects: Vec<Rect> = tiles.iter().map(|(p, _)| p.rect()).collect();
        match tiles.iter().position(|(p, _)| p.id == self.nav.active) {
            Some(i) => nav::neighbor(&rects, i, dir).map(|j| tiles[j].0.id),
            None => tiles.first().map(|(p, _)| p.id),
        }
    }

    /// A key pressed. `typing`: a text field has it, so only Escape and Tab are ours.
    pub fn key(
        &mut self,
        k: &Key,
        typing: bool,
        hd: WeakEntity<Dash>,
        cx: &mut Context<Self>,
    ) -> bool {
        let used = self.key_inner(k, typing, hd, cx);
        if used {
            self.changed(cx);
        }
        used
    }

    fn key_inner(
        &mut self,
        k: &Key,
        typing: bool,
        hd: WeakEntity<Dash>,
        cx: &mut Context<Self>,
    ) -> bool {
        if typing {
            // The field's: Enter keeps what was typed, Escape drops it (Blender's own keys).
            return false;
        }
        if self.data.menu.is_some() {
            return crate::widgets::menu_key(self, k);
        }
        if k.ctrl && k.is("e") && self.solo.is_none() {
            if self.edit.is_some() {
                self.edit = None;
                self.nav = Nav::default();
            } else {
                let picked = if self.nav.mode == Mode::Widget {
                    self.nav.active
                } else {
                    0
                };
                self.nav = Nav::default();
                self.edit = Some(Edit::new(picked));
                self.nav.active = picked;
            }
            return true;
        }
        if let Some(op) = self.nav.op {
            return self.op_key(op, k);
        }
        if k.ctrl && (k.is("m") || k.is("r")) && self.nav.active != 0 && self.solo.is_none() {
            let op = if k.is("m") { Op::Move } else { Op::Resize };
            let pinned = self.layout.get(self.nav.active).is_some_and(|p| p.pinned);
            if pinned {
                self.toast("Pinned: unpin it in edit mode first");
            } else {
                self.nav.op = Some(op);
            }
            return true;
        }
        if self.edit.is_some() {
            return crate::edit::key(self, k, cx);
        }
        match self.nav.mode {
            Mode::Hints => self.hints_key(k),
            Mode::Widget => self.widget_key(k, hd, cx),
            Mode::Off => self.dash_key(k),
        }
    }

    /// The keys of the dashboard itself, with nothing entered.
    fn dash_key(&mut self, k: &Key) -> bool {
        if k.is("Escape") {
            self.hide();
            return true;
        }
        if k.text() == Some("f") && !k.ctrl {
            self.nav.mode = Mode::Hints;
            self.nav.typed.clear();
            return true;
        }
        if let Some(dir) = k.ctrl_dir() {
            let _ = dir;
            if let Some(id) = self.neighbour(dir) {
                self.enter(id);
            }
            return true;
        }
        false
    }

    fn hints_key(&mut self, k: &Key) -> bool {
        if k.is("Escape") || (k.text() == Some("f") && self.nav.typed.is_empty()) {
            self.nav.mode = if self.nav.active != 0 && self.edit.is_none() {
                Mode::Widget
            } else {
                Mode::Off
            };
            return true;
        }
        if k.is("BackSpace") {
            self.nav.typed.pop();
            return true;
        }
        let Some(t) = k
            .text()
            .filter(|t| t.chars().all(|c| c.is_ascii_alphanumeric()))
        else {
            return true;
        };
        self.nav.typed.push_str(t);
        let tiles = self.tiles();
        let labels: Vec<String> = tiles.iter().map(|(_, l)| l.clone()).collect();
        match nav::pick(&labels, &self.nav.typed) {
            Ok(Some(i)) => {
                let id = tiles[i].0.id;
                if let Some(e) = &mut self.edit {
                    e.pick(id);
                    self.nav = Nav {
                        active: id,
                        ..Nav::default()
                    };
                } else {
                    self.enter(id);
                }
            }
            Ok(None) => {}
            Err(()) => {
                self.nav.typed.pop();
            }
        }
        true
    }

    fn widget_key(&mut self, k: &Key, hd: WeakEntity<Dash>, cx: &mut Context<Self>) -> bool {
        if k.is("Escape") {
            if self.nav.expanded != 0 {
                self.nav.expanded = 0;
            } else if self.w.escape(self.nav.active, &self.tx) {
                // The widget took it (a menu, a field).
            } else {
                self.leave();
            }
            return true;
        }
        if let Some(dir) = k.ctrl_dir().or(k.shift_dir()) {
            if let Some(id) = self.neighbour(dir) {
                self.enter(id);
            }
            return true;
        }
        if !k.ctrl && self.solo.is_none() {
            match k.text() {
                Some("f") => {
                    self.nav.mode = Mode::Hints;
                    self.nav.typed.clear();
                    return true;
                }
                Some("e") => {
                    self.nav.expanded = if self.nav.expanded == self.nav.active {
                        0
                    } else {
                        self.nav.active
                    };
                    return true;
                }
                Some("s") => {
                    let id = self.nav.active;
                    self.nav = Nav::default();
                    let mut e = Edit::new(id);
                    e.settings = true;
                    self.edit = Some(e);
                    self.nav.active = id;
                    return true;
                }
                _ => {}
            }
        }
        let Some((stops, binds)) = self.entered_ctx(hd) else {
            self.leave();
            return true;
        };
        let n = stops.len();
        let cur = self.nav.cursor.min(n.saturating_sub(1));
        let stop = stops.get(cur).cloned();
        // A widget's own letters first: they say what they are in the tips.
        if !k.ctrl
            && let Some(t) = k.text()
            && let Some(b) = binds.iter().find(|b| b.key == t)
        {
            (b.act)(self, cx);
            return true;
        }
        let slide = stop.as_ref().and_then(|s| s.slide.clone());
        let mv = |d: &mut Dash, to: usize| d.nav.cursor = to.min(n.saturating_sub(1));
        // A lone knob or slider: j k turn it, as h l do.
        if n == 1
            && let Some((v, set)) = slide.clone()
        {
            match k.name() {
                "j" | "Down" => {
                    return {
                        set(self, v - 0.05, cx);
                        true
                    };
                }
                "k" | "Up" => {
                    return {
                        set(self, v + 0.05, cx);
                        true
                    };
                }
                _ => {}
            }
        }
        match (k.name(), k.shift) {
            ("j" | "Down", _) | ("Tab", false) => mv(self, cur + 1),
            ("k" | "Up", _) | ("Tab", true) | ("ISO_Left_Tab", _) => {
                mv(self, cur.saturating_sub(1))
            }
            ("g", false) => mv(self, 0),
            ("G", _) | ("g", true) => mv(self, n.saturating_sub(1)),
            ("h" | "Left", _) => match slide {
                Some((v, set)) => set(self, v - 0.05, cx),
                None => mv(self, cur.saturating_sub(1)),
            },
            ("l" | "Right", _) => match slide {
                Some((v, set)) => set(self, v + 0.05, cx),
                None => mv(self, cur + 1),
            },
            ("Return" | "KP_Enter" | "space", _) => {
                if let Some(a) = stop.and_then(|s| s.act) {
                    a(self, cx);
                }
            }
            (d, _) if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() && slide.is_some() => {
                let (_, set) = slide.expect("checked");
                let n = (d.as_bytes()[0] - b'0') as f32;
                set(self, if n == 0.0 { 1.0 } else { n / 10.0 }, cx);
            }
            _ => return k.text().is_some(),
        }
        true
    }

    /// Move or resize mode: the direction keys act on the widget in hand.
    fn op_key(&mut self, op: Op, k: &Key) -> bool {
        let id = self.nav.active;
        if k.is("Escape") || (k.ctrl && k.is(if op == Op::Move { "m" } else { "r" })) {
            self.nav.op = None;
            return true;
        }
        if k.ctrl && (k.is("m") || k.is("r")) {
            self.nav.op = Some(if k.is("m") { Op::Move } else { Op::Resize });
            return true;
        }
        let Some(dir) = k.dir() else { return true };
        let Some(p) = self.layout.get(id).cloned() else {
            self.nav.op = None;
            return true;
        };
        let ok = match op {
            Op::Move => {
                let (dx, dy) = dir.step();
                self.layout
                    .move_reflow(id, p.col + dx, p.row + dy, (dx, dy))
            }
            Op::Resize => {
                let grow = !k.shift;
                let edge = if grow { dir } else { dir.opposite() };
                match nav::resized(p.rect(), edge, grow, p.shape == Shape::Round, p.min_size()) {
                    Some(r) => self.layout.resize_reflow(id, r),
                    None => false,
                }
            }
        };
        if ok {
            self.save_layout();
        } else {
            self.toast("No room there");
        }
        true
    }

    /// What the quick tips say: the widget's (or mode's) keys, and the dashboard's.
    pub fn tips(&self, hd: WeakEntity<Dash>) -> (Vec<nav::Tip>, Vec<nav::Tip>) {
        let tip = |k: &str, d: &str| nav::Tip {
            key: k.into(),
            does: d.into(),
        };
        if let Some(op) = self.nav.op {
            let size = self
                .layout
                .get(self.nav.active)
                .map(|p| format!("{} {}×{}", p.kind.info().title, p.w, p.h))
                .unwrap_or_default();
            let own = match op {
                Op::Move => vec![tip("h j k l", "move"), tip("", &size)],
                Op::Resize => vec![
                    tip("h j k l", "grow"),
                    tip("⇧ h j k l", "shrink"),
                    tip("", &size),
                ],
            };
            return (
                own,
                vec![
                    tip("Ctrl+M", "move"),
                    tip("Ctrl+R", "resize"),
                    tip("Esc", "done"),
                ],
            );
        }
        if let Some(e) = &self.edit {
            return crate::edit::tips(self, e);
        }
        match self.nav.mode {
            Mode::Off => (
                Vec::new(),
                vec![
                    tip("f", "go to a widget"),
                    tip("Ctrl+h j k l", "next widget"),
                    tip("Ctrl+E", "edit"),
                    tip("Esc", "close"),
                ],
            ),
            Mode::Hints => (
                vec![tip("", "type a label to go into that widget")],
                vec![tip("Backspace", "back"), tip("f Esc", "cancel")],
            ),
            Mode::Widget => {
                let mut own = Vec::new();
                if let Some((stops, binds)) = self.entered_ctx(hd) {
                    let cur = stops.get(self.nav.cursor.min(stops.len().saturating_sub(1)));
                    if stops.len() > 1 {
                        own.push(tip("j k", "move"));
                    }
                    match cur {
                        Some(s) if s.slide.is_some() => {
                            own.push(tip(if stops.len() == 1 { "j k" } else { "h l" }, "adjust"));
                            own.push(tip("0-9", "set"));
                            if s.act.is_some() && !s.does.is_empty() {
                                own.push(tip("Enter", s.does));
                            }
                        }
                        Some(s) if s.act.is_some() => own.push(tip(
                            "Enter",
                            if s.does.is_empty() { "press" } else { s.does },
                        )),
                        _ => {}
                    }
                    for b in &binds {
                        own.push(tip(b.key, &b.does));
                    }
                }
                let mut general = vec![];
                if self.solo.is_none() {
                    general.push(tip(
                        "e",
                        if self.nav.expanded != 0 {
                            "put back"
                        } else {
                            "blow up"
                        },
                    ));
                    general.push(tip("s", "settings"));
                    general.push(tip("Ctrl+M", "move"));
                    general.push(tip("Ctrl+R", "resize"));
                    general.push(tip("f", "another widget"));
                    general.push(tip("Esc", "leave"));
                }
                (nav::shown(&[], &[], &own), general)
            }
        }
    }
}

/// A key as the dashboard reads it.
#[derive(Debug, Clone)]
pub struct Key {
    /// The key's name ("Return", "Escape", "a").
    pub key: String,
    /// What it types, if anything.
    pub chr: Option<String>,
    pub ctrl: bool,
    pub shift: bool,
}

impl Key {
    pub fn from(ks: &Keystroke) -> Self {
        Self {
            key: ks.key.clone(),
            chr: ks.key_char.clone(),
            ctrl: ks.modifiers.control,
            shift: ks.modifiers.shift,
        }
    }

    /// The key's name, with the backspace and space keys under one name each.
    pub fn name(&self) -> &str {
        match self.key.as_str() {
            "Backspace" | "BackSpace" => "BackSpace",
            " " | "Space" | "space" => "space",
            "Enter" => "Return",
            k => k,
        }
    }

    pub fn is(&self, name: &str) -> bool {
        let n = self.name();
        n == name
            || (n.len() == 1 && n.eq_ignore_ascii_case(name) && !self.shift && name.len() == 1)
    }

    /// The character typed, for keys that type one.
    pub fn text(&self) -> Option<&str> {
        self.chr
            .as_deref()
            .filter(|c| !c.is_empty() && !c.chars().any(char::is_control))
    }

    /// h j k l or the arrows, as a direction.
    pub fn dir(&self) -> Option<Dir> {
        match self.name() {
            "h" | "H" | "Left" => Some(Dir::Left),
            "j" | "J" | "Down" => Some(Dir::Down),
            "k" | "K" | "Up" => Some(Dir::Up),
            "l" | "L" | "Right" => Some(Dir::Right),
            _ => None,
        }
    }

    fn ctrl_dir(&self) -> Option<Dir> {
        if self.ctrl { self.dir() } else { None }
    }

    fn shift_dir(&self) -> Option<Dir> {
        match self.text() {
            Some("H") => Some(Dir::Left),
            Some("J") => Some(Dir::Down),
            Some("K") => Some(Dir::Up),
            Some("L") => Some(Dir::Right),
            _ => None,
        }
    }
}

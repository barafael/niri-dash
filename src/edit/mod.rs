//! Edit mode: arranging and styling the dashboard. A port of `legacy/ui/app.slint` and
//! `legacy/ui/frame.slint` (the drawer, the settings card, the frames' handles and toolbar, the
//! key hints in the top bar) and of `legacy/src/app/{edit,arrange,hold,look}.rs` (what they do to
//! the layout).
//!
//! - [`Edit`] is the state of it all, the properties of the Slint window: it hangs on the
//!   [`Dash`] as `d.edit`.
//! - `ops` changes the layout (move, resize, add, remove, grid), `settings` the open widget's
//!   settings, `arrange` and `looks` the Arrange, Colors and Style tabs.
//! - `keys` is the keyboard model, `pointer` the mouse (drags, carrying, long press), `hints` the
//!   shortcuts the top bar shows.
//! - `view` draws: the frames' edit controls, the drawer, the settings card, the top bar.
//! - `anim` is what makes things glide.

pub mod anim;
mod arrange;
mod hints;
mod keys;
mod looks;
mod ops;
mod pointer;
mod settings;
pub mod view;

#[cfg(feature = "testing")]
pub mod probe;

pub use anim::Anims;
pub use arrange::Arrange;
pub use hints::KeyHint;
pub use keys::{edit_key, field_key};
pub use looks::{Scheme, StyleCard};
pub use pointer::{Drag, DragMode, Hold, PalettePress, Press, PressKind};
pub use settings::{RowKind, SettingRow, StyleRow};

use crate::dash::{Dash, MARGIN};
use crate::layout::{Kind, Layout, Reflow};
use gpui::{Context, ScrollHandle};
use niri_dash_system::{Capabilities, Compositor};
use strum::IntoEnumIterator;

/// The width of the sidebar, and its distance to the screen's edges.
pub const DRAWER_W: f32 = 340.0;
pub const EDGE: f32 = 16.0;

/// The sidebar's sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DrawerTab {
    #[default]
    Widgets,
    Arrange,
    Colors,
    Style,
}

impl DrawerTab {
    /// In the order of the tab bar (and of Tab).
    pub const ALL: [DrawerTab; 4] = [
        DrawerTab::Widgets,
        DrawerTab::Arrange,
        DrawerTab::Colors,
        DrawerTab::Style,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DrawerTab::Widgets => "Widgets",
            DrawerTab::Arrange => "Arrange",
            DrawerTab::Colors => "Colors",
            DrawerTab::Style => "Style",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

/// A widget in the palette of the Widgets tab.
#[derive(Debug, Clone)]
pub struct PaletteItem {
    pub kind: Kind,
    pub title: &'static str,
    pub description: String,
    /// The size a new one has, in cells.
    pub w: i32,
    pub h: i32,
}

/// What edit mode keeps: the properties of the Slint window that were not the layout or the look.
pub struct Edit {
    pub anim: Anims,
    pub tab: DrawerTab,
    /// The widget being worked on in edit mode (0 = none); its settings open in a card in the
    /// middle of the screen, if it has any.
    pub settings_id: u32,
    /// A single widget opened for moving and resizing with a long press or a double click
    /// (0 = none).
    pub adjust_id: u32,
    /// The keyboard's place in the sidebar's tab (or in the card's rows), shown once a key moved
    /// it ([`Edit::side_keys`]).
    pub side_cursor: usize,
    /// The keys of edit mode are on the sidebar (or the card): it shows its cursor.
    pub side_keys: bool,
    /// The keys of edit mode are on the widgets (size, move, pick another): the settings card
    /// keeps out of the way.
    pub canvas_keys: bool,
    /// What sizing made of the widget, for the status line ("Volume 3×2").
    pub size_note: String,
    /// How often each grid stepper (columns, rows) refused a step: the row shakes.
    pub refusals: [u32; 2],
    /// While dragging: where everything would go, with the widget dragged (0 for a new one).
    pub preview: Option<(u32, Reflow)>,
    pub arrange: Arrange,
    pub palette: Vec<PaletteItem>,
    pub schemes: Vec<Scheme>,
    pub styles: Vec<StyleCard>,
    pub style_rows: Vec<StyleRow>,
    pub style_customized: bool,
    pub press: Option<Press>,
    pub drag: Drag,
    pub pal_press: PalettePress,
    pub hold: Hold,
    /// The keyboard goes to a text field of the card (set by keys, taken by the window).
    pub field: settings::Fields,
    pub scroll: Scrolls,
    /// How far out the drawer is (0 closed, 1 open), as of the last frame.
    shift: std::cell::Cell<f32>,
    /// The frame the pointer is over, in edit mode (0 = none): its handles show.
    pub hover: Option<u32>,
    /// Which row the sidebar's list was last scrolled to show (see `view::drawer`).
    pub revealed: std::cell::Cell<Option<Reveal>>,
    /// The last toast's text, for it to fade out with.
    pub toast_text: std::cell::RefCell<String>,
}

/// Where the keyboard cursor was when its row was last scrolled into view: the tab (None: the
/// card), the cursor, whether it showed, and the widget whose card it was.
pub type Reveal = (Option<DrawerTab>, usize, bool, u32);

/// The scrolling lists of the sidebar and the card, one handle each.
#[derive(Default)]
pub struct Scrolls {
    pub widgets: ScrollHandle,
    pub arrange: ScrollHandle,
    pub colors: ScrollHandle,
    pub style: ScrollHandle,
    pub card: ScrollHandle,
}

impl Edit {
    pub fn new(caps: Capabilities, layout: &Layout) -> Self {
        Self {
            anim: Anims::default(),
            tab: DrawerTab::Widgets,
            settings_id: 0,
            adjust_id: 0,
            side_cursor: 0,
            side_keys: false,
            canvas_keys: false,
            size_note: String::new(),
            refusals: [0; 2],
            preview: None,
            arrange: Arrange::new(layout.autolayout.clone()),
            palette: palette(caps),
            schemes: Vec::new(),
            styles: Vec::new(),
            style_rows: Vec::new(),
            style_customized: false,
            press: None,
            drag: Drag::default(),
            pal_press: PalettePress::default(),
            hold: Hold::default(),
            field: settings::Fields::default(),
            scroll: Scrolls::default(),
            shift: std::cell::Cell::new(0.0),
            hover: None,
            revealed: std::cell::Cell::new(None),
            toast_text: std::cell::RefCell::new(String::new()),
        }
    }

    /// How far out the drawer is (0 closed, 1 open): it slides in with edit mode, and the grid's
    /// cells shrink to make room.
    pub fn drawer_shift(&self) -> f32 {
        self.shift.get()
    }

    /// The animated values of this frame: they stay as they are until the next.
    pub fn sample(&self, editing: bool, motion: f32) {
        self.shift.set(self.anim.follow(
            ("drawer", 0),
            if editing { 1.0 } else { 0.0 },
            240.0,
            motion,
            anim::Ease::Soft,
        ));
    }

    pub fn dragging(&self) -> bool {
        self.drag.mode != DragMode::None
    }
}

/// How a widget's description reads here, where the compositor lacks what it needs (None: as
/// usual), and whether it can't work here at all.
pub fn caveat(caps: Capabilities, kind: Kind) -> Option<(String, bool)> {
    match kind {
        // KWin offers the list to programs with a desktop entry that asks for it.
        Kind::Windows if !caps.windows && caps.compositor == Compositor::Kwin => Some((
            "Open windows (KWin gives no window list: run niri-dash desktop-entries)".into(),
            true,
        )),
        Kind::Windows if !caps.windows => Some((
            format!("Open windows by workspace ({})", caps.unavailable()),
            true,
        )),
        // There are no workspaces to group them by.
        Kind::Windows if caps.compositor == Compositor::Kwin => {
            Some(("Open windows".into(), false))
        }
        Kind::ClaudeSessions if !caps.windows => {
            Some(("Running Claude Code sessions".into(), false))
        }
        Kind::Privacy if !caps.screencast => Some((
            format!(
                "Who uses the microphone or camera (screen sharing {})",
                caps.unavailable()
            ),
            false,
        )),
        Kind::Notifications if !caps.notifications_server => {
            Some(("Pause Plasma's notification pop-ups".into(), false))
        }
        Kind::NightLight if caps.night_light_native => {
            Some(("Pause or resume Plasma's night light".into(), false))
        }
        Kind::KeyboardLayout if !caps.keyboard_layouts => {
            Some((format!("Keyboard layouts ({})", caps.unavailable()), true))
        }
        Kind::ColorPicker if !caps.color_picker => {
            Some((format!("Color picker ({})", caps.unavailable()), true))
        }
        _ => None,
    }
}

/// The widgets the palette offers; the ones that can't work under this compositor go last,
/// saying what they need.
fn palette(caps: Capabilities) -> Vec<PaletteItem> {
    let mut kinds: Vec<Kind> = Kind::iter().collect();
    kinds.sort_by_key(|&k| caveat(caps, k).is_some_and(|(_, out)| out));
    kinds
        .into_iter()
        .map(|k| {
            let info = k.info();
            let caveat = caveat(caps, k).map(|(text, _)| text);
            PaletteItem {
                kind: k,
                title: info.title,
                description: caveat.unwrap_or_else(|| info.description.to_string()),
                w: info.new_size().0,
                h: info.new_size().1,
            }
        })
        .collect()
}

// ---- the keys' entry points, called by the registry ----------------------------------------

/// Open widget `id`'s settings (`s` in an entered widget).
pub fn open_settings(d: &mut Dash, id: u32, cx: &mut Context<Dash>) {
    d.open_settings(id, cx);
}

/// Pick widget `id` to work on in edit mode (a label typed in edit mode, Ctrl+E inside a widget):
/// its settings open, and the keys are on the widgets.
pub fn configure(d: &mut Dash, id: u32, cx: &mut Context<Dash>) {
    d.configure_widget(id, cx);
    d.edit.canvas_keys = true;
}

/// Escape, before the dashboard takes it: a text field of the card gives the keyboard back, a
/// drag is cancelled, a widget opened alone is put down. True if one of them was.
pub fn escape(d: &mut Dash, cx: &mut Context<Dash>) -> bool {
    if d.editing && d.edit.field.focused {
        // Out of a sidebar text field, back to the keys of edit mode.
        d.edit.field.leave();
        return true;
    }
    if d.edit.dragging() {
        d.finish_drag(false);
        return true;
    }
    if d.op() != crate::dash::NavOp::None {
        d.op_end();
        return true;
    }
    if d.edit.adjust_id != 0 {
        d.edit.adjust_id = 0;
        d.edit.press = None;
        d.set_preview(None);
        cx.notify();
        return true;
    }
    false
}

/// Escape in edit mode with nothing else open: the card closes and the widget is let go, or edit
/// mode ends.
pub fn escape_editing(d: &mut Dash, cx: &mut Context<Dash>) {
    if d.edit.settings_id != 0 {
        d.set_settings_id(0);
    } else {
        d.set_editing(false, cx);
    }
}

/// Edit mode was turned on or off (`Dash::set_editing`).
pub fn editing_changed(d: &mut Dash, on: bool) {
    let e = &mut d.edit;
    e.side_keys = false;
    e.canvas_keys = false;
    e.side_cursor = 0;
    e.adjust_id = 0;
    e.press = None;
    e.drag = Drag::default();
    if on {
        // The sidebar opens with edit mode; fill its theme lists.
        d.refresh_looks();
    } else {
        e.settings_id = 0;
        e.size_note.clear();
        e.field.leave();
        d.set_preview(None);
        d.save_layout();
    }
}

/// The dashboard goes away: edit mode's loose ends.
pub fn go_down(d: &mut Dash) {
    let e = &mut d.edit;
    e.settings_id = 0;
    e.adjust_id = 0;
    e.press = None;
    e.drag = Drag::default();
    e.hold.cancel();
    e.side_keys = false;
    e.canvas_keys = false;
    e.side_cursor = 0;
    e.field.leave();
    d.set_preview(None);
}

/// The dashboard shows itself: the entrance plays.
pub fn come_up(d: &mut Dash) {
    d.edit.anim.shown();
}

/// The grid's room when the drawer is `shift` out (0 closed, 1 open): the pieces
/// [`Dash::geometry`] is made of. The drawer only shrinks the cells, so nothing gets hidden:
/// cards keep the same distance to the screen edge and to the drawer, and start level with its
/// top.
pub struct Room {
    pub avail_w: f32,
    pub side: f32,
    pub top: f32,
    pub bottom: f32,
    pub shift: f32,
}

impl Room {
    pub fn new(width: f32, height: f32, gap: f32, shift: f32) -> Self {
        let bar = crate::dash::BAR;
        let drawer_y = bar + 8.0;
        Self {
            avail_w: width - shift * (DRAWER_W + EDGE),
            side: MARGIN + shift * (EDGE - gap - MARGIN),
            top: bar + shift * (drawer_y - gap - bar),
            bottom: height - MARGIN + shift * (MARGIN - EDGE + gap),
            shift,
        }
    }
}

/// The tiles as the views draw them, with the preview of a drag applied: widgets that make room
/// for the dragged one are where they would go.
pub fn previewed(preview: &Option<(u32, Reflow)>, placed: &mut [crate::layout::Placed]) {
    let Some((dragged, moves)) = preview else {
        return;
    };
    for p in placed {
        if p.id != *dragged
            && let Some(&(_, r)) = moves.iter().find(|(id, _)| *id == p.id)
        {
            (p.col, p.row, p.w, p.h) = (r.col, r.row, r.w, r.h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dash::BAR;

    fn said(compositor: Compositor, kind: Kind) -> Option<(String, bool)> {
        caveat(Capabilities::for_compositor(compositor, true, true), kind)
    }

    /// Where a widget can't work, it says so the same way, naming the platform.
    #[test]
    fn caveats_name_the_platform() {
        assert_eq!(said(Compositor::Niri, Kind::Windows), None);
        assert_eq!(
            said(Compositor::Kwin, Kind::Windows),
            Some((
                "Open windows (KWin gives no window list: run niri-dash desktop-entries)".into(),
                true
            ))
        );
        assert_eq!(
            said(Compositor::Other, Kind::Windows),
            Some((
                "Open windows by workspace (not available on this desktop)".into(),
                true
            ))
        );
        // Once KWin has let us bind the list.
        let mut granted = Capabilities::for_compositor(Compositor::Kwin, true, true);
        granted.windows = true;
        assert_eq!(
            caveat(granted, Kind::Windows),
            Some(("Open windows".into(), false))
        );
        assert_eq!(
            said(Compositor::Kwin, Kind::Privacy),
            Some((
                "Who uses the microphone or camera (screen sharing not available on KDE Plasma)"
                    .into(),
                false
            ))
        );
        assert_eq!(
            said(Compositor::Other, Kind::KeyboardLayout),
            Some((
                "Keyboard layouts (not available on this desktop)".into(),
                true
            ))
        );
        assert_eq!(
            said(Compositor::Other, Kind::ColorPicker),
            Some(("Color picker (not available on this desktop)".into(), true))
        );
        assert_eq!(said(Compositor::Kwin, Kind::ColorPicker), None);
    }

    /// The palette lists the widgets that can't work here last.
    #[test]
    fn the_palette_puts_what_cannot_work_last() {
        let caps = Capabilities::for_compositor(Compositor::Other, true, true);
        let items = palette(caps);
        assert_eq!(items.len(), Kind::iter().count());
        let first_out = items
            .iter()
            .position(|p| caveat(caps, p.kind).is_some_and(|(_, out)| out))
            .expect("some widget can't work on an unknown desktop");
        assert!(
            items[first_out..]
                .iter()
                .all(|p| caveat(caps, p.kind).is_some_and(|(_, out)| out)),
            "a widget that works comes after one that can't"
        );
    }

    #[test]
    fn a_closed_drawer_leaves_the_margins() {
        let r = Room::new(1000.0, 600.0, 8.0, 0.0);
        assert_eq!(r.avail_w, 1000.0);
        assert_eq!((r.side, r.top, r.bottom), (MARGIN, BAR, 600.0 - MARGIN));
    }

    #[test]
    fn an_open_drawer_takes_its_width_and_the_cards_keep_their_distance() {
        let gap = 8.0;
        let r = Room::new(1000.0, 600.0, gap, 1.0);
        assert_eq!(r.avail_w, 1000.0 - DRAWER_W - EDGE);
        // Cells reach a gap past their cards: the cards are EDGE from the screen's edge, level
        // with the drawer's top and bottom.
        assert_eq!(r.side + gap, EDGE);
        assert_eq!(r.top + gap, BAR + 8.0);
        assert_eq!(r.bottom - gap, 600.0 - EDGE);
    }
}

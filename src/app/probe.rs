//! What the edit-mode tests (`tests/edit.rs`) look at beyond the UI's globals: the layout as the
//! dashboard holds it, and whether it agrees with what the UI shows, what config.kdl says, and
//! the rules a layout keeps (inside the grid, no overlaps, round widgets square, sizes at least
//! the minimum, settings the sidebar can show).

use super::testing::Harness;
use crate::layout::{Kind, Layout, Shape};
use crate::{Nav, NavMode};
use slint::platform::{PointerEventButton, WindowEvent};
use slint::{ComponentHandle, LogicalPosition, Model};
use std::collections::HashSet;
use strum::IntoEnumIterator;

/// One widget of the layout, visible or not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tile {
    pub id: u32,
    /// The name config.kdl uses.
    pub kind: String,
    pub round: bool,
    pub col: i32,
    pub row: i32,
    pub w: i32,
    pub h: i32,
    /// Shown on the current grid.
    pub visible: bool,
    /// As stored: (name, value as KDL).
    pub settings: Vec<(String, String)>,
}

/// A tile without its id, to compare with one read back from the file.
type ShapeKey = (String, bool, i32, i32, i32, i32, Vec<(String, String)>);

impl Tile {
    /// Where it is and what it is, without the id (which a reload hands out anew).
    fn shape_key(&self) -> ShapeKey {
        (
            self.kind.clone(),
            self.round,
            self.col,
            self.row,
            self.w,
            self.h,
            self.settings.clone(),
        )
    }
}

/// Every widget kind's config name, in the palette's order.
pub fn kind_names() -> Vec<&'static str> {
    Kind::iter().map(Kind::name).collect()
}

/// The setting labels a kind has, in the sidebar's order.
pub fn setting_labels(kind: &str) -> Vec<&'static str> {
    Kind::from_str_opt(kind)
        .map(|k| k.info().settings.iter().map(|o| o.label()).collect())
        .unwrap_or_default()
}

/// The UI's name for a kind, by its config name.
pub fn ui_kind(kind: &str) -> crate::WidgetKind {
    super::enums::kind_to_ui(Kind::from_str_opt(kind).expect("a widget kind"))
}

/// The (w, h) a kind may not shrink below, rectangular.
pub fn min_size(kind: &str) -> (i32, i32) {
    Kind::from_str_opt(kind).map_or((1, 1), |k| k.info().min_size)
}

pub fn can_be_round(kind: &str) -> bool {
    Kind::from_str_opt(kind).is_some_and(|k| k.info().can_be_round)
}

fn tiles_of(layout: &Layout) -> Vec<Tile> {
    let shown: HashSet<u32> = layout.visible().iter().map(|p| p.id).collect();
    layout
        .widgets
        .iter()
        .map(|p| Tile {
            id: p.id,
            kind: p.kind.name().to_string(),
            round: p.shape == Shape::Round,
            col: p.col,
            row: p.row,
            w: p.w,
            h: p.h,
            visible: shown.contains(&p.id),
            settings: p
                .settings
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect(),
        })
        .collect()
}

impl Harness {
    /// Press `button` at (x, y), in logical pixels.
    /// As the shell passes the pointer on.
    fn pointer(&self, event: WindowEvent) {
        self.app().hold.watch(self.app(), &event);
        self.ui().window().dispatch_event(event);
    }

    pub fn down(&mut self, x: f32, y: f32, button: PointerEventButton) {
        let position = LogicalPosition::new(x, y);
        self.pointer(WindowEvent::PointerMoved { position });
        self.pointer(WindowEvent::PointerPressed { position, button });
        self.pump();
    }

    pub fn up(&mut self, x: f32, y: f32, button: PointerEventButton) {
        let position = LogicalPosition::new(x, y);
        self.pointer(WindowEvent::PointerReleased { position, button });
        self.pump();
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        let position = LogicalPosition::new(x, y);
        self.pointer(WindowEvent::PointerMoved { position });
        self.pump();
    }

    /// Press at `from`, move to `to` in `steps` even steps, release there.
    pub fn drag(&mut self, from: (f32, f32), to: (f32, f32), steps: u32) {
        self.down(from.0, from.1, PointerEventButton::Left);
        for i in 1..=steps.max(1) {
            let t = i as f32 / steps.max(1) as f32;
            self.move_to(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
        }
        self.up(to.0, to.1, PointerEventButton::Left);
    }

    /// Press a key as the platform layer would: "f", "ctrl+e", "esc", "enter", "tab", ….
    pub fn press(&mut self, spec: &str) {
        super::snapshot::press_key(self.ui().window(), spec);
        self.pump();
    }

    /// Let animations finish (they run on real time).
    pub fn settle(&mut self, ms: u64) {
        let end = std::time::Instant::now() + std::time::Duration::from_millis(ms);
        while std::time::Instant::now() < end {
            self.pump();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// The window position of grid cell (col, row)'s top left corner, and a cell's size.
    pub fn cell_origin(&self, col: f32, row: f32) -> (f32, f32) {
        let ui = self.ui();
        (
            ui.get_grid_x() + col * ui.get_cell(),
            ui.get_grid_y() + row * ui.get_cell(),
        )
    }

    pub fn cell(&self) -> f32 {
        self.ui().get_cell()
    }

    /// The layout as the dashboard holds it, every widget in file order.
    pub fn tiles(&self) -> Vec<Tile> {
        tiles_of(&self.app().layout.borrow())
    }

    /// The grid in use, and whether config.kdl sets it (rather than the screen).
    pub fn grid(&self) -> (i32, i32, bool) {
        let l = self.app().layout.borrow();
        (l.cols, l.rows, l.grid.is_some())
    }

    /// Write `text` as config.kdl, as if the dashboard had started from it (a harness only holds
    /// it in memory).
    pub fn seed_config(text: &str) {
        crate::files::write_atomic(&Layout::path(), text).expect("writing config.kdl");
    }

    /// The text config.kdl has now.
    pub fn config_text(&self) -> String {
        crate::files::flush();
        std::fs::read_to_string(Layout::path()).unwrap_or_default()
    }

    /// Everything that disagrees or breaks a rule, empty if all is well. Call when no drag is
    /// under way (a drag shows a preview that differs from the layout on purpose).
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let app = self.app();
        let ui = self.ui();
        let layout = app.layout.borrow();
        let tiles = tiles_of(&layout);
        let (cols, rows) = (layout.cols, layout.rows);

        // The rules of a layout.
        let mut seen = HashSet::new();
        for t in &tiles {
            if !seen.insert(t.id) {
                out.push(format!("two widgets with id {}", t.id));
            }
            if t.round && t.w != t.h {
                out.push(format!("round {} #{} is {}x{}", t.kind, t.id, t.w, t.h));
            }
            if t.round && !can_be_round(&t.kind) {
                out.push(format!("{} #{} is round but can't be", t.kind, t.id));
            }
            let (mw, mh) = if t.round { (1, 1) } else { min_size(&t.kind) };
            if t.w < mw || t.h < mh {
                out.push(format!(
                    "{} #{} is {}x{}, below its minimum {mw}x{mh}",
                    t.kind, t.id, t.w, t.h
                ));
            }
            if t.col < 0 || t.row < 0 {
                out.push(format!(
                    "{} #{} at negative {},{}",
                    t.kind, t.id, t.col, t.row
                ));
            }
            if t.visible && (t.col + t.w > cols || t.row + t.h > rows) {
                out.push(format!(
                    "{} #{} shown past the {cols}x{rows} grid at {},{} {}x{}",
                    t.kind, t.id, t.col, t.row, t.w, t.h
                ));
            }
            if let Some(kind) = Kind::from_str_opt(&t.kind) {
                let p = layout.get(t.id).expect("the tile's widget");
                for w in crate::widget_settings::check(kind, &p.settings) {
                    out.push(format!("#{}: {w}", t.id));
                }
            }
        }
        if !(crate::layout::COLUMNS.contains(&cols) && crate::layout::ROWS.contains(&rows)) {
            out.push(format!("grid {cols}x{rows} is outside the steppers' range"));
        }

        // The UI shows the visible widgets, where the layout has them.
        if app.preview.borrow().is_none() {
            let shown: Vec<_> = ui
                .get_widgets()
                .iter()
                .map(|w| (w.id as u32, w.col, w.row, w.w, w.h))
                .collect();
            let want: Vec<_> = layout
                .visible()
                .iter()
                .map(|p| (p.id, p.col, p.row, p.w, p.h))
                .collect();
            if shown != want {
                out.push(format!("the UI shows {shown:?}, the layout has {want:?}"));
            }
        }
        if ui.get_hidden_widgets() as usize != layout.hidden_count() {
            out.push(format!(
                "the UI says {} hidden, the layout has {}",
                ui.get_hidden_widgets(),
                layout.hidden_count()
            ));
        }
        if (ui.get_cols(), ui.get_rows()) != (cols, rows) {
            out.push(format!(
                "the UI's grid is {}x{}, the layout's {cols}x{rows}",
                ui.get_cols(),
                ui.get_rows()
            ));
        }

        // Hint labels: one per shown widget, none the start of another.
        let labels: Vec<String> = ui
            .get_widgets()
            .iter()
            .map(|w| w.hint.to_string())
            .collect();
        for (i, a) in labels.iter().enumerate() {
            if a.is_empty() {
                out.push(format!("widget {i} has no hint label"));
            }
            for (j, b) in labels.iter().enumerate() {
                if i != j && b.starts_with(a.as_str()) {
                    out.push(format!("hint {a:?} is the start of {b:?}"));
                }
            }
        }
        let nav = ui.global::<Nav>();
        if nav.get_mode() == NavMode::Widget
            && !ui.get_solo()
            && !layout
                .visible()
                .iter()
                .any(|p| p.id as i32 == nav.get_active_id())
        {
            out.push(format!(
                "entered widget #{} isn't on screen",
                nav.get_active_id()
            ));
        }

        // The sidebar's settings are the open widget's.
        let open = ui.get_settings_id();
        if open != 0 {
            match layout.get(open as u32) {
                None => out.push(format!("settings open for #{open}, which is gone")),
                Some(p) => {
                    let rows = ui.get_setting_rows();
                    let want = p.kind.info().settings.len();
                    if rows.row_count() != want {
                        out.push(format!(
                            "{} settings shown for {} #{open}, which has {want}",
                            rows.row_count(),
                            p.kind.name()
                        ));
                    }
                }
            }
        }

        // config.kdl says the same, and reads back without warnings.
        drop(layout);
        if !app.config_broken.get() {
            let text = self.config_text();
            match Layout::parse(&text) {
                Err(e) => out.push(format!("config.kdl doesn't parse: {e}")),
                Ok((mut disk, warnings)) => {
                    for w in warnings {
                        out.push(format!("config.kdl warns: {w}"));
                    }
                    disk.set_screen_grid(cols, rows);
                    let mine: Vec<_> = tiles.iter().map(Tile::shape_key).collect();
                    let theirs: Vec<_> = tiles_of(&disk).iter().map(Tile::shape_key).collect();
                    if mine != theirs {
                        out.push(format!(
                            "config.kdl differs from the dashboard:\n  file: {theirs:?}\n  dash: {mine:?}"
                        ));
                    }
                    if disk.grid != app.layout.borrow().grid {
                        out.push(format!(
                            "config.kdl's grid is {:?}, the dashboard's {:?}",
                            disk.grid,
                            app.layout.borrow().grid
                        ));
                    }
                }
            }
        }
        out
    }
}

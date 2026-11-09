//! What more than one of the desk's widgets needs: the keyboard cursors, where a widget's card is
//! in the window, putting the dashboard away, and the wording of the times.

use crate::dash::{Dash, DashEvent, Tile};
use crate::scope::Scope;
use gpui::Context;
use niri_dash_system::Compositor;
use std::collections::HashMap;

/// The keyboard cursor of each instance of a widget, by tile id.
#[derive(Default)]
pub struct Scopes(pub HashMap<u32, Scope>);

impl Scopes {
    /// Instance `id`'s cursor (a new one if it has none yet).
    pub fn get(&self, id: u32) -> Scope {
        self.0.get(&id).copied().unwrap_or_default()
    }

    pub fn put(&mut self, id: u32, scope: Scope) {
        self.0.insert(id, scope);
    }

    /// The instances that are still on the dashboard.
    pub fn keep(&mut self, d_tiles: &[Tile]) {
        self.0.retain(|id, _| d_tiles.iter().any(|t| t.id() == *id));
    }

    /// Every cursor back at the first stop (the dashboard opened).
    pub fn reset(&mut self) {
        self.0.clear();
    }
}

/// `scope` as it is on a list that now has `count` stops: the cursor on one that exists. (A widget
/// that was resized, or expanded, has fewer stops than it had; the cursor is moved for real at
/// the next key.)
pub fn eff(mut scope: Scope, count: usize) -> Scope {
    if scope.cursor >= count {
        scope.cursor = count.saturating_sub(1);
        scope.sub = 0;
    }
    scope
}

/// Where the card of `tile` is in the window and how big, as `ui` draws it: x, y, width, height
/// in pixels. A round one is a circle in the middle of its cells; an expanded one is blown up in
/// the middle of the window.
pub fn card(d: &Dash, tile: &Tile) -> (f32, f32, f32, f32) {
    let (area_w, area_h) = d.size;
    if area_w <= 0.0 || area_h <= 0.0 {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let g = d.geometry(area_w, area_h);
    let gap = d.look.style.gap;
    let r = tile.rect();
    let expanded = d.nav.expanded_id == tile.id();
    let (x, y, w, h) = if expanded {
        let big_w = area_w.min((r.w as f32 * g.cell).max(area_w * 0.55).max(8.0 * g.cell));
        let big_h = area_h.min((r.h as f32 * g.cell).max(area_h * 0.8).max(6.0 * g.cell));
        ((area_w - big_w) / 2.0, (area_h - big_h) / 2.0, big_w, big_h)
    } else {
        (
            g.x + r.col as f32 * g.cell,
            g.y + r.row as f32 * g.cell,
            r.w as f32 * g.cell,
            r.h as f32 * g.cell,
        )
    };
    let (cw, ch) = (w - 2.0 * gap, h - 2.0 * gap);
    if tile.round() && !expanded {
        let diameter = cw.min(ch);
        (
            x + (w - diameter) / 2.0,
            y + (h - diameter) / 2.0,
            diameter,
            diameter,
        )
    } else {
        (x + gap, y + gap, cw, ch)
    }
}

/// The tile `id`, its card and whether it is drawn round.
pub fn card_of(d: &Dash, id: u32) -> Option<((f32, f32, f32, f32), bool)> {
    let tile = d.tile(id)?;
    let round = tile.round() && d.nav.expanded_id != id;
    Some((card(d, tile), round))
}

/// A click or key took you to something outside the dashboard (a window, a notification's app):
/// it gets out of the way. In the demo there is nothing to go to, and putting the dashboard away
/// would look like the page reloading.
pub fn hide(d: &Dash, cx: &mut Context<Dash>) {
    if !d.caps.simulated {
        cx.emit(DashEvent::Hide);
    }
}

pub fn plasma(d: &Dash) -> bool {
    d.caps.compositor == Compositor::Kwin
}

pub fn niri(d: &Dash) -> bool {
    d.caps.compositor == Compositor::Niri
}

/// A time of day, in the locale's way.
pub fn clock_text(t: chrono::DateTime<chrono::Local>, us: bool) -> String {
    if us {
        t.format("%-I:%M %p").to_string()
    } else {
        t.format("%-H:%M").to_string()
    }
}

/// When something happened: the time of day, with the date before it if that isn't today.
pub fn when_text(t: chrono::DateTime<chrono::Local>, us: bool) -> String {
    let time = clock_text(t, us);
    if t.date_naive() == chrono::Local::now().date_naive() {
        time
    } else if us {
        format!("{} {time}", t.format("%-m/%-d"))
    } else {
        format!("{} {time}", t.format("%d.%m."))
    }
}

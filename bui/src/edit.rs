//! Edit mode: drag widgets around and resize them from their edges, pick one to change its shape,
//! pin it, remove it or open its settings, and add widgets, pick a color scheme and a style or
//! set the grid in the sidebar. Every change is written to config.kdl at once, comments kept.

use crate::autolayout::{self, Knob};
use crate::dash::{Dash, Geometry, Key, Mode, Nav};
use crate::layout::{Group, Kind, Placed, Rect, Shape};
use crate::look::{COLORSCHEMES, STYLES};
use crate::nav::{self, Dir, Tip};
use crate::widget_settings::{self as ws, Opt, Ty};
use crate::widgets::{act, run};
use blenderui::prelude::*;
use kdl::KdlValue;
use strum::IntoEnumIterator;

/// The sidebar's width.
pub const SIDEBAR: f32 = 300.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Widgets,
    Colors,
    Style,
    Grid,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Widgets, Tab::Colors, Tab::Style, Tab::Grid];

    fn name(self) -> &'static str {
        match self {
            Tab::Widgets => "Widgets",
            Tab::Colors => "Colors",
            Tab::Style => "Style",
            Tab::Grid => "Grid",
        }
    }
}

/// What a drag with the pointer does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Grab {
    Move,
    /// Pull this edge.
    Edge(Dir),
    /// A new widget, carried from the sidebar.
    Add(Kind),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Drag {
    pub id: u32,
    pub grab: Grab,
    pub start: (f32, f32),
    pub at: (f32, f32),
}

#[derive(Debug, Clone, Default)]
pub struct Edit {
    /// The widget picked (0: none).
    pub picked: u32,
    /// Its settings card is open.
    pub settings: bool,
    pub tab: Tab,
    pub drag: Option<Drag>,
    /// A drag just ended: the click that ends it isn't a pick.
    pub dragged: bool,
    /// The Arrange tab's settings, from the config's `autolayout` line until changed here.
    pub params: Option<autolayout::Params>,
    /// Where the widgets were before the last Arrange, for Undo.
    pub before: Option<Vec<(u32, Rect)>>,
    /// What the last Arrange came to.
    pub arranged: Option<String>,
    /// Opened by a press held on one widget: only that one is open, without the sidebar, and
    /// Escape is done with it.
    pub adjust: bool,
}

impl Edit {
    pub fn new(picked: u32) -> Self {
        Self {
            picked,
            ..Default::default()
        }
    }

    /// Widget `id` open on its own for adjusting.
    pub fn adjust(id: u32) -> Self {
        Self {
            picked: id,
            adjust: true,
            ..Default::default()
        }
    }

    pub fn pick(&mut self, id: u32) {
        if self.picked != id {
            self.settings = false;
        }
        self.picked = id;
    }
}

/// Where a drag would put its widget, and whether it fits there.
fn target(d: &Dash, g: &Geometry, drag: &Drag) -> Option<(Rect, bool)> {
    if let Grab::Add(kind) = drag.grab {
        // Its default size, centered on the pointer, wherever on the grid that is.
        let (w, h) = kind.info().new_size();
        let col = ((drag.at.0 - g.x) / g.cell - w as f32 / 2.0).round() as i32;
        let row = ((drag.at.1 - g.y) / g.cell - h as f32 / 2.0).round() as i32;
        let to = Rect {
            col: col.clamp(0, (d.layout.cols - w).max(0)),
            row: row.clamp(0, (d.layout.rows - h).max(0)),
            w,
            h,
        };
        let over = drag.at.0 >= g.x
            && drag.at.0 < g.x + g.width
            && drag.at.1 >= g.y
            && drag.at.1 < g.y + g.height;
        return Some((to, over && d.layout.reflow(0, to, (0, 1)).is_some()));
    }
    let p = d.layout.get(drag.id)?;
    let from = p.rect();
    let dc = ((drag.at.0 - drag.start.0) / g.cell).round() as i32;
    let dr = ((drag.at.1 - drag.start.1) / g.cell).round() as i32;
    let to = match drag.grab {
        Grab::Move => Rect {
            col: (from.col + dc).clamp(0, (d.layout.cols - from.w).max(0)),
            row: (from.row + dr).clamp(0, (d.layout.rows - from.h).max(0)),
            ..from
        },
        Grab::Edge(Dir::Right) => Rect {
            w: from.w + dc,
            ..from
        },
        Grab::Edge(Dir::Down) => Rect {
            h: from.h + dr,
            ..from
        },
        Grab::Edge(Dir::Left) => Rect {
            col: from.col + dc,
            w: from.w - dc,
            ..from
        },
        Grab::Edge(Dir::Up) => Rect {
            row: from.row + dr,
            h: from.h - dr,
            ..from
        },
        Grab::Add(_) => unreachable!("handled above"),
    };
    if to == from {
        return Some((to, true));
    }
    Some(match drag.grab {
        Grab::Move => (
            to,
            d.layout
                .reflow(drag.id, to, (dc.signum(), dr.signum()))
                .is_some(),
        ),
        Grab::Edge(_) => match d.layout.plan_resize(drag.id, to) {
            Some((r, _)) => (r, true),
            None => (to, false),
        },
        Grab::Add(_) => unreachable!("handled above"),
    })
}

/// The pointer let go: put the widget where the drag took it.
fn drop(d: &mut Dash) {
    let Some(drag) = d.edit.as_mut().and_then(|e| e.drag.take()) else {
        return;
    };
    let g = d.geometry();
    let Some((to, ok)) = target(d, &g, &drag) else {
        return;
    };
    if let Grab::Add(kind) = drag.grab {
        let over = drag.at.0 >= g.x && drag.at.0 < g.x + g.width;
        if !over {
            // Let go over the sidebar: nothing.
            return;
        }
        let added = if ok {
            d.layout.add_at(kind, to.col, to.row)
        } else {
            None
        };
        match added {
            Some(id) => {
                if let Some(e) = &mut d.edit {
                    e.pick(id);
                }
                d.nav.active = id;
                d.save_layout();
                d.settings_changed();
            }
            None => d.toast("No room there"),
        }
        return;
    }
    let from = d.layout.get(drag.id).map(Placed::rect);
    if Some(to) == from {
        return;
    }
    if let Some(e) = &mut d.edit {
        e.dragged = true;
    }
    let done = ok
        && match drag.grab {
            Grab::Move => {
                let f = from.expect("it was there");
                d.layout.move_reflow(
                    drag.id,
                    to.col,
                    to.row,
                    ((to.col - f.col).signum(), (to.row - f.row).signum()),
                )
            }
            Grab::Edge(_) => d.layout.resize_reflow(drag.id, to),
            Grab::Add(_) => false,
        };
    if done {
        d.save_layout();
    } else {
        d.toast("No room there");
    }
}

/// The grid's cells, faintly, while widgets are being placed; and the drag's ghost.
pub fn grid(d: &Dash, g: &Geometry) -> Div {
    let mut el = div().style("position: absolute; left: 0; top: 0; right: 0; bottom: 0");
    for r in 0..d.layout.rows {
        for c in 0..d.layout.cols {
            el = el.child(div().class("gridcell").style(format!(
                "left: {}px; top: {}px; width: {}px; height: {}px",
                g.x + c as f32 * g.cell,
                g.y + r as f32 * g.cell,
                g.cell,
                g.cell
            )));
        }
    }
    if let Some(drag) = d.edit.as_ref().and_then(|e| e.drag)
        && let Some((to, ok)) = target(d, g, &drag)
    {
        let (x, y, w, h) = g.place(to, d.look.style.gap);
        el = el.child(
            div()
                .class(if ok { "ghost" } else { "ghost bad" })
                .style(format!(
                    "left: {x}px; top: {y}px; width: {w}px; height: {h}px; z-index: 5"
                )),
        );
    }
    el
}

/// A widget's card in edit mode: picked by a click, dragged by its body, resized by its edges;
/// in move or resize mode, a veil that says so.
pub fn decorate(d: &Dash, hd: WeakEntity<Dash>, p: &Placed, card: Div) -> Div {
    let id = p.id;
    let picked = d.edit.as_ref().is_some_and(|e| e.picked == id)
        || (d.nav.op.is_some() && d.nav.active == id);
    let mut card = card
        .on_click(move |_: &ClickEvent, _: &mut Window, app: &mut App| {
            hd.update(app, |d, cx| {
                if let Some(e) = &mut d.edit {
                    if std::mem::take(&mut e.dragged) {
                        return;
                    }
                    e.pick(id);
                    e.settings = true;
                }
                d.nav.active = id;
                cx.notify();
            });
            app.stop_propagation();
        })
        .on_press_drag(move |e: &PressDragEvent, _: &mut Window, app: &mut App| {
            if e.button != MouseButton::Left {
                return;
            }
            hd.update(app, |d, cx| {
                let pinned = d.layout.get(id).is_some_and(|p| p.pinned);
                if let Some(ed) = &mut d.edit {
                    if pinned {
                        d.toast = Some((
                            "Pinned: press p to unpin it".into(),
                            web_time::Instant::now() + std::time::Duration::from_secs(3),
                        ));
                    } else {
                        ed.pick(id);
                        ed.drag = Some(Drag {
                            id,
                            grab: Grab::Move,
                            start: (e.start.x.0, e.start.y.0),
                            at: (e.position.x.0, e.position.y.0),
                        });
                    }
                }
                d.nav.active = id;
                cx.notify();
            });
        });
    if picked {
        card = card.class("picked");
    }
    if picked && !p.pinned && d.edit.is_some() {
        for (dir, css) in [
            (
                Dir::Right,
                "right: -2px; top: 20%; width: 6px; height: 60%; cursor: ew-resize",
            ),
            (
                Dir::Left,
                "left: -2px; top: 20%; width: 6px; height: 60%; cursor: ew-resize",
            ),
            (
                Dir::Down,
                "bottom: -2px; left: 20%; height: 6px; width: 60%; cursor: ns-resize",
            ),
            (
                Dir::Up,
                "top: -2px; left: 20%; height: 6px; width: 60%; cursor: ns-resize",
            ),
        ] {
            card = card.child(div().class("handle").style(css).on_press_drag(
                move |e: &PressDragEvent, _: &mut Window, app: &mut App| {
                    hd.update(app, |d, cx| {
                        if let Some(ed) = &mut d.edit {
                            ed.drag = Some(Drag {
                                id,
                                grab: Grab::Edge(dir),
                                start: (e.start.x.0, e.start.y.0),
                                at: (e.position.x.0, e.position.y.0),
                            });
                        }
                        cx.notify();
                    });
                    app.stop_propagation();
                },
            ));
        }
    }
    if let Some(op) = d.nav.op.filter(|_| d.nav.active == id) {
        let (name, keys) = match op {
            crate::dash::Op::Move => ("Move", "h j k l move · Esc done"),
            crate::dash::Op::Resize => ("Resize", "h j k l grow · Shift shrinks · Esc done"),
        };
        card = card.child(
            div()
                .class("veil")
                .child(div().class("title").child(name))
                .child(div().child(format!("{} {}×{}", p.kind.info().title, p.w, p.h)))
                .child(div().class("small").child(keys)),
        );
    }
    card
}

/// While a drag is on, the whole window follows the pointer, wherever the drag started (a card,
/// an edge, the sidebar), until it lets go.
pub fn follow(d: &Dash, hd: WeakEntity<Dash>, el: Div) -> Div {
    if !d.edit.as_ref().is_some_and(|e| e.drag.is_some()) {
        return el;
    }
    el.on_mouse_event(
        move |e: &MouseMoveEvent, phase: DispatchPhase, _: &mut Window, app: &mut App| {
            if phase == DispatchPhase::Capture {
                hd.update(app, |d, cx| {
                    if let Some(drag) = d.edit.as_mut().and_then(|e| e.drag.as_mut()) {
                        drag.at = (e.position.x.0, e.position.y.0);
                        cx.notify();
                    }
                });
            }
        },
    )
    .on_mouse_event(
        move |_: &MouseUpEvent, phase: DispatchPhase, _: &mut Window, app: &mut App| {
            if phase == DispatchPhase::Capture {
                hd.update(app, |d, cx| {
                    drop(d);
                    cx.notify();
                });
            }
        },
    )
}

/// The sidebar: tabs for adding widgets, the color scheme, the style and the grid, and what can
/// be done to the picked widget.
pub fn sidebar(d: &Dash, hd: WeakEntity<Dash>) -> Div {
    let Some(e) = &d.edit else { return div() };
    let tabs = div().class("tabs").children(Tab::ALL.into_iter().map(|t| {
        div()
            .class(if t == e.tab { "tab on" } else { "tab" })
            .child(t.name())
            .on_click(run::<ClickEvent>(
                hd,
                act(move |d: &mut Dash, _| {
                    if let Some(e) = &mut d.edit {
                        e.tab = t;
                    }
                }),
            ))
    }));
    let mut pane = div().class("pane").id(format!("pane-{}", e.tab.name()));
    if let Some(p) = d.layout.get(e.picked) {
        pane = pane.child(picked_tools(d, hd, p));
    }
    pane = match e.tab {
        Tab::Widgets => widgets_tab(d, hd, pane),
        Tab::Colors => looks_tab(d, hd, pane, true),
        Tab::Style => looks_tab(d, hd, pane, false),
        Tab::Grid => grid_tab(d, hd, pane),
    };
    div().class("sidebar").child(tabs).child(pane)
}

/// What can be done to the picked widget.
/// The tools of a widget opened by a hold, over it (or under it, at the top of the screen).
pub fn adjust_bar(d: &Dash, hd: WeakEntity<Dash>) -> Option<Div> {
    let e = d.edit.as_ref().filter(|e| e.adjust)?;
    let p = d.layout.get(e.picked)?;
    let g = d.geometry();
    let (x, y, _, h) = g.place(p.rect(), d.look.style.gap);
    let top = if y > 80.0 { y - 34.0 } else { y + h + 4.0 };
    Some(
        picked_tools(d, hd, p)
            .class("settings")
            .style(format!(
                "left: {x}px; top: {top}px; width: auto; z-index: 12; padding: 4px; border: 0"
            ))
            .child(Button().child("Done").on_click(run::<ClickEvent>(
                hd,
                act(|d: &mut Dash, _| {
                    d.edit = None;
                    d.nav = Nav::default();
                }),
            ))),
    )
}

fn picked_tools(d: &Dash, hd: WeakEntity<Dash>, p: &Placed) -> Div {
    let compact = d.edit.as_ref().is_some_and(|e| e.adjust);
    let id = p.id;
    let info = p.kind.info();
    let mut row = div().class("row wrap");
    if info.can_be_round {
        row = row.child(
            Button()
                .child(if p.shape == Shape::Round {
                    "Make square"
                } else {
                    "Make round"
                })
                .on_click(run::<ClickEvent>(
                    hd,
                    act(move |d: &mut Dash, _| shape(d, id)),
                )),
        );
    }
    row = row
        .child(
            Button()
                .child(if p.pinned { "Unpin" } else { "Pin" })
                .on_click(run::<ClickEvent>(
                    hd,
                    act(move |d: &mut Dash, _| pin(d, id)),
                )),
        )
        .child(Button().child("Settings").on_click(run::<ClickEvent>(
            hd,
            act(move |d: &mut Dash, _| {
                if let Some(e) = &mut d.edit {
                    e.settings = !e.settings;
                }
            }),
        )))
        .child(Button().child("Remove").on_click(run::<ClickEvent>(
            hd,
            act(move |d: &mut Dash, _| remove(d, id)),
        )));
    if compact {
        return row;
    }
    div()
        .class("col")
        .style("border-bottom: 1px solid var(--border); padding-bottom: 6px")
        .child(
            div()
                .class("title")
                .child(format!("{} {}×{}", info.title, p.w, p.h)),
        )
        .child(div().class("small dim").child(info.description))
        .child(row)
}

fn widgets_tab(d: &Dash, hd: WeakEntity<Dash>, mut pane: Div) -> Div {
    for g in Group::ALL {
        pane = pane.child(div().class("group").child(g.name()));
        for kind in Kind::iter().filter(|k| k.info().group == g) {
            let info = kind.info();
            let count = d.layout.widgets.iter().filter(|w| w.kind == kind).count();
            pane = pane.child(
                div()
                    .class("item")
                    .id(format!("add-{}", kind.name()))
                    .title("Drag it onto the grid, or Add it where there is room")
                    .on_press_drag(move |e: &PressDragEvent, _: &mut Window, app: &mut App| {
                        if e.button != MouseButton::Left {
                            return;
                        }
                        hd.update(app, |d, cx| {
                            if let Some(ed) = &mut d.edit {
                                ed.drag = Some(Drag {
                                    id: 0,
                                    grab: Grab::Add(kind),
                                    start: (e.start.x.0, e.start.y.0),
                                    at: (e.position.x.0, e.position.y.0),
                                });
                            }
                            cx.notify();
                        });
                    })
                    .child(
                        div()
                            .class("col grow")
                            .child(div().child(info.title))
                            .child(div().class("small faint ellipsis").child(info.description)),
                    )
                    .children(
                        (count > 0).then(|| span().class("small dim").child(format!("×{count}"))),
                    )
                    .child(Button().child("Add").on_click(run::<ClickEvent>(
                        hd,
                        act(move |d: &mut Dash, _| add(d, kind)),
                    ))),
            );
        }
    }
    pane
}

/// The bundled schemes or styles, and the user's own next to config.kdl.
fn names(colors: bool) -> Vec<String> {
    let mut out: Vec<String> = (if colors { COLORSCHEMES } else { STYLES })
        .iter()
        .map(|(n, _)| n.to_string())
        .collect();
    if let Some(dir) = crate::layout::Layout::path().parent() {
        let dir = dir.join(if colors { "colorschemes" } else { "styles" });
        for p in crate::files::list(&dir) {
            if let Some(n) = p
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|_| p.extension().is_some_and(|e| e == "kdl"))
                && !out.iter().any(|o| o == n)
            {
                out.push(n.to_string());
            }
        }
    }
    out
}

fn looks_tab(d: &Dash, hd: WeakEntity<Dash>, mut pane: Div, colors: bool) -> Div {
    let current = if colors {
        &d.layout.colorscheme
    } else {
        &d.layout.style
    };
    let current = if colors && (current.is_empty() || current == "default") {
        "blender"
    } else {
        current.as_str()
    };
    for name in names(colors) {
        let on = name == current;
        let mut row = div()
            .class(if on { "item on" } else { "item" })
            .child(div().class("grow").child(name.clone()));
        if colors {
            let (look, _) = crate::look::resolve(&name, &d.layout.style, &[]);
            row = row.child(
                div().class("swatches").children(
                    ["card", "panel", "text", "accent", "good", "warn", "bad"]
                        .iter()
                        .map(|r| {
                            div()
                                .class("swatch")
                                .style(format!("background: {}", look.color(r)))
                        }),
                ),
            );
        }
        let n = name.clone();
        pane = pane.child(row.on_click(run::<ClickEvent>(
            hd,
            act(move |d: &mut Dash, _| {
                if colors {
                    // Blender's scheme is what "default" means here; the name stays one the Slint
                    // version knows.
                    d.layout.colorscheme = if n == "blender" {
                        "default".into()
                    } else {
                        n.clone()
                    };
                } else {
                    d.layout.style = n.clone();
                }
                d.relook();
                d.save_layout();
            }),
        )));
    }
    pane
}

fn grid_tab(d: &Dash, hd: WeakEntity<Dash>, pane: Div) -> Div {
    let auto = d.layout.grid.is_none();
    let (cols, rows) = (d.layout.cols, d.layout.rows);
    let set = move |c: i32, r: i32| {
        act(move |d: &mut Dash, _| {
            let refit = d.layout.set_grid(Some((c, r)));
            if let Some(m) = refit.message() {
                d.toast(m);
            }
            d.save_layout();
        })
    };
    pane.child(
        div()
            .class("row")
            .child(Checkbox().checked(auto).on_change(run::<ChangeEvent>(
                hd,
                act(move |d: &mut Dash, _| {
                    let g = if d.layout.grid.is_none() {
                        Some((d.layout.cols, d.layout.rows))
                    } else {
                        None
                    };
                    let refit = d.layout.set_grid(g);
                    if let Some(m) = refit.message() {
                        d.toast(m);
                    }
                    let (w, h) = d.size;
                    d.fit(w, h);
                    d.save_layout();
                }),
            )))
            .child("Fit the screen (100 px cells)"),
    )
    .child(
        div()
            .class("row")
            .child(div().class("grow").child(format!("Columns {cols}")))
            .child(
                Button()
                    .child("−")
                    .on_click(run::<ClickEvent>(hd, set(cols - 1, rows))),
            )
            .child(
                Button()
                    .child("+")
                    .on_click(run::<ClickEvent>(hd, set(cols + 1, rows))),
            ),
    )
    .child(
        div()
            .class("row")
            .child(div().class("grow").child(format!("Rows {rows}")))
            .child(
                Button()
                    .child("−")
                    .on_click(run::<ClickEvent>(hd, set(cols, rows - 1))),
            )
            .child(
                Button()
                    .child("+")
                    .on_click(run::<ClickEvent>(hd, set(cols, rows + 1))),
            ),
    )
    .child(arrange(d, hd))
    .child(div().class("group").child("Start over"))
    .child(
        Button()
            .child("Reset to the default layout")
            .on_click(run::<ClickEvent>(
                hd,
                act(|d: &mut Dash, _| {
                    let refit = d.layout.reset();
                    if let Some(m) = refit.message() {
                        d.toast(m);
                    }
                    d.save_layout();
                }),
            )),
    )
}

/// The Arrange section: the algorithm's settings, and laying the widgets out by them.
fn arrange(d: &Dash, hd: WeakEntity<Dash>) -> Div {
    let Some(e) = &d.edit else { return div() };
    let params = e
        .params
        .clone()
        .unwrap_or_else(|| d.layout.autolayout.clone());
    let mut el = div()
        .class("col")
        .child(div().class("group").child("Arrange (experimental)"))
        .child(div().class("small faint").child(params.algo.describe()));
    for k in Knob::of(params.algo) {
        let step = move |delta: i32| {
            act(move |d: &mut Dash, _| {
                let mut p = d
                    .edit
                    .as_ref()
                    .and_then(|e| e.params.clone())
                    .unwrap_or_else(|| d.layout.autolayout.clone());
                if p.can_step(k, delta) {
                    p.step(k, delta);
                }
                if let Some(e) = &mut d.edit {
                    e.params = Some(p);
                }
            })
        };
        let mut row = div()
            .class("row small")
            .child(div().class("grow dim").child(k.label()))
            .child(
                div()
                    .class(if params.is_default(k) { "" } else { "accent" })
                    .child(params.text(k)),
            )
            .child(
                Button()
                    .child("−")
                    .style("width: 32px; flex: none")
                    .disabled(!params.can_step(k, -1))
                    .on_click(run::<ClickEvent>(hd, step(-1))),
            )
            .child(
                Button()
                    .child("+")
                    .style("width: 32px; flex: none")
                    .disabled(!params.can_step(k, 1))
                    .on_click(run::<ClickEvent>(hd, step(1))),
            );
        if !params.is_default(k) {
            row = row.child(
                Button()
                    .child("↺")
                    .style("width: 32px; flex: none")
                    .on_click(run::<ClickEvent>(
                        hd,
                        act(move |d: &mut Dash, _| {
                            let mut p = d
                                .edit
                                .as_ref()
                                .and_then(|e| e.params.clone())
                                .unwrap_or_else(|| d.layout.autolayout.clone());
                            p.reset(k);
                            if let Some(e) = &mut d.edit {
                                e.params = Some(p);
                            }
                        }),
                    )),
            );
        }
        el = el.child(row);
    }
    let buttons = div()
        .class("row wrap")
        .child(Button().child("Arrange").on_click(run::<ClickEvent>(
            hd,
            act(|d: &mut Dash, _| arrange_now(d, 0)),
        )))
        .child(Button().child("Shuffle").on_click(run::<ClickEvent>(
            hd,
            act(|d: &mut Dash, _| arrange_now(d, 1)),
        )))
        .child(
            Button()
                .child("Undo")
                .disabled(e.before.is_none())
                .on_click(run::<ClickEvent>(
                    hd,
                    act(|d: &mut Dash, _| arrange_undo(d)),
                )),
        )
        .child(Button().child("Copy as config").on_click(run::<ClickEvent>(
            hd,
            act(|d: &mut Dash, _| {
                let line = d
                    .edit
                    .as_ref()
                    .and_then(|e| e.params.clone())
                    .unwrap_or_else(|| d.layout.autolayout.clone())
                    .to_kdl();
                crate::widgets::copy_text(d, &line);
            }),
        )));
    el = el.child(buttons);
    if let Some(r) = &e.arranged {
        el = el.child(div().class("small dim").child(r.clone()));
    }
    el
}

/// Lay the widgets out by the Arrange tab's settings; `shuffle`: with the next seed first.
pub fn arrange_now(d: &mut Dash, shuffle: u32) {
    let mut p = d
        .edit
        .as_ref()
        .and_then(|e| e.params.clone())
        .unwrap_or_else(|| d.layout.autolayout.clone());
    p.seed = (p.seed + shuffle).min(999);
    let before: Vec<(u32, Rect)> = d.layout.widgets.iter().map(|w| (w.id, w.rect())).collect();
    let a = autolayout::arrange(&mut d.layout, &p);
    if let Some(e) = &mut d.edit {
        e.before = Some(before);
        e.arranged = Some(a.summary());
        e.params = Some(p);
    }
    d.save_layout();
}

/// Put the widgets back where they were before the last Arrange.
fn arrange_undo(d: &mut Dash) {
    let Some(before) = d.edit.as_mut().and_then(|e| e.before.take()) else {
        return;
    };
    d.layout.set_rects(&before);
    if let Some(e) = &mut d.edit {
        e.arranged = None;
    }
    d.save_layout();
}

/// The picked widget's settings, as Blender properties.
pub fn settings_card(d: &Dash, hd: WeakEntity<Dash>) -> Option<Div> {
    let e = d.edit.as_ref().filter(|e| e.settings)?;
    let p = d.layout.get(e.picked)?;
    let id = p.id;
    let info = p.kind.info();
    let us = d.locale.us;
    let g = d.geometry();
    let (x, y, w, _) = g.place(p.rect(), d.look.style.gap);
    let left = if x + w + 290.0 < d.size.0 - SIDEBAR {
        x + w + 8.0
    } else {
        (x - 288.0).max(8.0)
    };
    let mut card = div()
        .class("settings")
        .style(format!(
            "left: {left}px; top: {}px; z-index: 10",
            y.max(40.0)
        ))
        .child(
            div()
                .class("row")
                .child(
                    div()
                        .class("title grow")
                        .child(format!("{} settings", info.title)),
                )
                .child(
                    Button()
                        .child("✕")
                        .attr("aria-label", "Close the settings")
                        .on_click(run::<ClickEvent>(
                            hd,
                            act(|d: &mut Dash, _| {
                                if let Some(e) = &mut d.edit {
                                    e.settings = false;
                                }
                            }),
                        )),
                ),
        );
    if info.settings.is_empty() {
        card = card.child(div().class("empty").child("Nothing to set"));
    }
    for &opt in info.settings {
        let cur = ws::get(&p.settings, opt, us);
        let label = div().class("grow").child(opt.label());
        let control: Div = match opt.ty() {
            Ty::Toggle => Checkbox()
                .checked(matches!(cur, KdlValue::Bool(true)))
                .on_change(set_opt(hd, id, opt, |e| Some(KdlValue::Bool(e.checked)))),
            Ty::Steps(steps) => Dropdown()
                .children(steps.iter().map(|s| {
                    option()
                        .selected(KdlValue::Integer((*s).into()) == cur)
                        .child(opt.value_text(&KdlValue::Integer((*s).into())))
                }))
                .on_change(set_opt(hd, id, opt, move |e| {
                    steps
                        .get(e.number as usize)
                        .map(|s| KdlValue::Integer((*s).into()))
                })),
            Ty::Choice(words) => Dropdown()
                .children(words.iter().map(|w| {
                    option()
                        .selected(KdlValue::String(w.to_string()) == cur)
                        .child(opt.value_text(&KdlValue::String(w.to_string())))
                }))
                .on_change(set_opt(hd, id, opt, move |e| {
                    words
                        .get(e.number as usize)
                        .map(|w| KdlValue::String(w.to_string()))
                })),
            Ty::Text { placeholder } => TextField()
                .value(cur.as_string().unwrap_or_default().to_string())
                .placeholder(placeholder)
                .style("width: 150px")
                .on_change(set_opt(hd, id, opt, |e| {
                    Some(KdlValue::String(e.value.clone()))
                })),
        };
        card = card.child(div().class("row").child(label).child(control));
    }
    let complaints = ws::check(p.kind, &p.settings);
    for c in complaints {
        card = card.child(div().class("small bad").child(c));
    }
    Some(card)
}

fn set_opt(
    hd: WeakEntity<Dash>,
    id: u32,
    opt: Opt,
    value: impl Fn(&ChangeEvent) -> Option<KdlValue> + 'static,
) -> impl Fn(&ChangeEvent, &mut Window, &mut App) + 'static {
    move |e: &ChangeEvent, _: &mut Window, app: &mut App| {
        let Some(v) = value(e) else { return };
        hd.update(app, |d, cx| {
            let us = d.locale.us;
            if let Some(values) = d.layout.settings_mut(id) {
                ws::set(values, opt, v, us);
            }
            d.save_layout();
            d.settings_changed();
            cx.notify();
        });
    }
}

fn shape(d: &mut Dash, id: u32) {
    if d.layout.toggle_shape(id) {
        d.save_layout();
    } else {
        d.toast("No room for that shape");
    }
}

fn pin(d: &mut Dash, id: u32) {
    if let Some(on) = d.layout.toggle_pin(id) {
        d.toast(if on {
            "Pinned: it stays where it is"
        } else {
            "Unpinned"
        });
        d.save_layout();
    }
}

fn remove(d: &mut Dash, id: u32) {
    d.layout.remove(id);
    if let Some(e) = &mut d.edit {
        e.pick(0);
    }
    if d.edit.as_ref().is_some_and(|e| e.adjust) {
        d.edit = None;
    }
    d.nav = Nav::default();
    d.save_layout();
    d.settings_changed();
}

fn add(d: &mut Dash, kind: Kind) {
    match d.layout.add(kind) {
        Some(id) => {
            if let Some(e) = &mut d.edit {
                e.pick(id);
            }
            d.nav.active = id;
            d.save_layout();
            d.settings_changed();
        }
        None => d.toast("No room for it: make room, or a bigger grid"),
    }
}

/// The keys of edit mode.
pub fn key(d: &mut Dash, k: &Key, _cx: &mut Context<Dash>) -> bool {
    if d.nav.mode == Mode::Hints {
        // Picking a widget by its label.
        return d.hints_key_pub(k);
    }
    let Some(e) = d.edit.clone() else {
        return false;
    };
    let picked = e.picked;
    if e.adjust {
        return adjust_key(d, k, picked);
    }
    if k.is("Escape") {
        let ed = d.edit.as_mut().expect("editing");
        if ed.drag.take().is_some() {
        } else if ed.settings {
            ed.settings = false;
        } else if ed.picked != 0 {
            ed.pick(0);
            d.nav.active = 0;
        } else {
            d.edit = None;
            d.nav = Nav::default();
        }
        return true;
    }
    if k.is("Tab") {
        let i = Tab::ALL.iter().position(|t| *t == e.tab).unwrap_or(0);
        let n = Tab::ALL.len();
        let next = if k.shift {
            (i + n - 1) % n
        } else {
            (i + 1) % n
        };
        if let Some(ed) = &mut d.edit {
            ed.tab = Tab::ALL[next];
        }
        return true;
    }
    match k.text() {
        Some("f") => {
            d.nav.mode = Mode::Hints;
            d.nav.typed.clear();
            return true;
        }
        Some(t @ ("w" | "a" | "s" | "d")) => {
            let dir = match t {
                "w" => Dir::Up,
                "a" => Dir::Left,
                "s" => Dir::Down,
                _ => Dir::Right,
            };
            pick_towards(d, dir);
            return true;
        }
        _ => {}
    }
    if picked == 0 {
        return k.text().is_some();
    }
    if let Some(dir) = k.dir().filter(|_| !k.ctrl && !k.shift) {
        pick_towards(d, dir);
        return true;
    }
    match (k.text(), k.name()) {
        (Some("o"), _) => shape(d, picked),
        (Some("p"), _) => pin(d, picked),
        (Some("x"), _) | (_, "Delete") => remove(d, picked),
        (_, "Return") => {
            if let Some(ed) = &mut d.edit {
                ed.settings = !ed.settings;
            }
        }
        _ => return k.text().is_some(),
    }
    true
}

/// The keys of a widget opened by a hold: its own tools, and Escape when done.
fn adjust_key(d: &mut Dash, k: &Key, id: u32) -> bool {
    match (k.text(), k.name()) {
        (_, "Escape") => {
            let ed = d.edit.as_mut().expect("adjusting");
            if ed.drag.take().is_none() && !std::mem::take(&mut ed.settings) {
                d.edit = None;
                d.nav = Nav::default();
            }
        }
        (Some("o"), _) => shape(d, id),
        (Some("p"), _) => pin(d, id),
        (Some("x"), _) | (_, "Delete") => {
            remove(d, id);
            d.edit = None;
        }
        (Some("s"), _) | (_, "Return") => {
            if let Some(ed) = &mut d.edit {
                ed.settings = !ed.settings;
            }
        }
        _ => return k.text().is_some(),
    }
    true
}

/// Pick the nearest widget that way from the picked one (or the first).
fn pick_towards(d: &mut Dash, dir: Dir) {
    let tiles = d.tiles();
    let rects: Vec<Rect> = tiles.iter().map(|(p, _)| p.rect()).collect();
    let picked = d.edit.as_ref().map_or(0, |e| e.picked);
    let next = match tiles.iter().position(|(p, _)| p.id == picked) {
        Some(i) => nav::neighbor(&rects, i, dir).map(|j| tiles[j].0.id),
        None => tiles.first().map(|(p, _)| p.id),
    };
    if let Some(id) = next {
        if let Some(e) = &mut d.edit {
            e.pick(id);
        }
        d.nav.active = id;
    }
}

/// The quick tips in edit mode.
pub fn tips(d: &Dash, e: &Edit) -> (Vec<Tip>, Vec<Tip>) {
    let tip = |k: &str, w: &str| Tip {
        key: k.into(),
        does: w.into(),
    };
    if e.adjust {
        let own = match d.layout.get(e.picked) {
            Some(p) => {
                let mut v = vec![tip("", &format!("{} {}×{}", p.kind.info().title, p.w, p.h))];
                if !p.pinned {
                    v.extend([
                        tip("", "drag to move, pull an edge to resize"),
                        tip("Ctrl+M", "move"),
                        tip("Ctrl+R", "resize"),
                    ]);
                    if p.kind.info().can_be_round {
                        v.push(tip("o", "shape"));
                    }
                }
                v.extend([
                    tip("p", if p.pinned { "unpin" } else { "pin" }),
                    tip("s", "settings"),
                    tip("x", "remove"),
                ]);
                v
            }
            None => Vec::new(),
        };
        return (own, vec![tip("Esc", "done")]);
    }
    let own = if d.nav.mode == Mode::Hints {
        vec![
            tip("", "type a label to pick that widget"),
            tip("f Esc", "cancel"),
        ]
    } else if let Some(p) = d.layout.get(e.picked) {
        let mut v = vec![tip("", &format!("{} {}×{}", p.kind.info().title, p.w, p.h))];
        if p.pinned {
            v.push(tip("p", "unpin"));
        } else {
            v.extend([
                tip("Ctrl+M", "move"),
                tip("Ctrl+R", "resize"),
                tip("p", "pin"),
            ]);
            if p.kind.info().can_be_round {
                v.push(tip("o", "shape"));
            }
        }
        v.extend([
            tip("x", "remove"),
            tip("Enter", "settings"),
            tip("h j k l", "pick another"),
            tip("Esc", "let go"),
        ]);
        v
    } else {
        vec![
            tip("f", "pick by label"),
            tip("w a s d", "pick"),
            tip("", "drag widgets by their body, resize by their edges"),
        ]
    };
    (
        own,
        vec![
            tip("Tab", "sidebar tab"),
            tip("Ctrl+E", "done"),
            tip("Esc", "back"),
        ],
    )
}

impl Dash {
    pub fn hints_key_pub(&mut self, k: &Key) -> bool {
        // Same as on the dashboard; edit mode picks instead of entering.
        if k.is("Escape") || (k.text() == Some("f") && self.nav.typed.is_empty()) {
            self.nav.mode = Mode::Off;
            return true;
        }
        if k.is("BackSpace") {
            self.nav.typed.pop();
            return true;
        }
        let Some(t) = k.text() else { return true };
        self.nav.typed.push_str(t);
        let tiles = self.tiles();
        let labels: Vec<String> = tiles.iter().map(|(_, l)| l.clone()).collect();
        match nav::pick(&labels, &self.nav.typed) {
            Ok(Some(i)) => {
                let id = tiles[i].0.id;
                if let Some(e) = &mut self.edit {
                    e.pick(id);
                }
                self.nav = Nav {
                    active: id,
                    ..Nav::default()
                };
            }
            Ok(None) => {}
            Err(()) => {
                self.nav.typed.pop();
            }
        }
        true
    }
}

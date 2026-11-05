//! Laying out the dashboard by itself.
//!
//! Given the widgets and the grid, [`arrange`] decides where every widget goes and how big it
//! is. Each widget has a size it looks best at ([`crate::layout::KindInfo::default_size`]), a
//! smallest and a largest size, and a [`Group`] it belongs with. A layout is good when every
//! widget is close to its preferred size and shape, the grid is full, and a group stays together.
//!
//! There are three algorithms, because none is best everywhere ([`Algo`]):
//!
//! - **Guillotine** cuts the grid into two rectangles, and those into two, until each holds one
//!   widget, and finds *the cheapest* such set of cuts by dynamic programming over (widgets
//!   `i..j`, width, height). The widgets keep a given order, so a group sits together. It fills
//!   the grid exactly and respects minimum sizes. Cost grows with the cube of the widget count,
//!   which is fine for a dashboard.
//! - **Treemap** cuts the grid greedily in proportion to the widgets' preferred areas. It takes
//!   no time and always fills the grid, but it chooses shapes less carefully.
//! - **Gravity** puts each widget at its preferred size in the first free spot, then lets them
//!   grow into the holes. Unlike the others it may leave gaps, and it shows the widgets in order
//!   like a masonry wall.
//!
//! All of them are deterministic for a given [`Params`]. Different orderings of the widgets give
//! different, equally valid layouts: `seed` picks another one, and `restarts` tries several and
//! keeps the best by [`Params`]' own idea of cost.
//!
//! A [`Params`] reads from and writes to a line of config.kdl (`autolayout algo="guillotine" …`),
//! so a set that works can be copied out of edit mode and kept.

use crate::layout::{Group, Kind, Layout, ROUND_MAX, ROUND_SIZE, Rect};
use kdl::{KdlNode, KdlValue};
use strum::{AsRefStr, EnumIter, EnumString, IntoEnumIterator};
use web_time::Instant;

mod anneal;

/// The ways to lay out a dashboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, EnumString, AsRefStr)]
#[strum(serialize_all = "kebab-case")]
pub enum Algo {
    Guillotine,
    Treemap,
    Gravity,
    /// Widgets that belong together sit together; see [`crate::affinity`].
    Neighbours,
}

impl Algo {
    pub fn describe(self) -> &'static str {
        match self {
            Algo::Guillotine => "optimal cuts, fills the grid",
            Algo::Treemap => "greedy cuts, fills the grid",
            Algo::Gravity => "first fit, then grow; may leave gaps",
            Algo::Neighbours => "liked widgets side by side, disliked apart; keeps gaps",
        }
    }
}

/// Which widget goes first. Neighbours in the order end up as neighbours on the grid, and the
/// last ones are the first to be dropped when there is no room for all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, EnumString, AsRefStr)]
#[strum(serialize_all = "kebab-case")]
pub enum Order {
    /// By [`Group`], the big ones of a group first.
    Group,
    /// The big ones first.
    Area,
    /// Where they are now, top to bottom, left to right.
    Reading,
    /// As in the widget list.
    Kind,
    /// Widgets that like each other next to each other, one long chain.
    Affinity,
}

/// Where [`Algo::Neighbours`] leans the widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, EnumString, AsRefStr)]
#[strum(serialize_all = "kebab-case")]
pub enum EdgeMode {
    Off,
    /// To the screen's edges, keeping the middle free.
    Edges,
    /// To the middle, keeping the edges free.
    Center,
}

/// Everything that shapes a layout: the algorithm and the numbers it weighs.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    pub algo: Algo,
    pub order: Order,
    /// 0 is the order as it is; another number is another, repeatable, shuffle of it.
    pub seed: u32,
    /// How many shuffles (`seed`, `seed + 1`, …) to try; the cheapest layout wins.
    pub restarts: u32,
    /// How far a shuffle moves a widget in the order, as a fraction of the widget count (and,
    /// for the group order, whether groups swap places).
    pub jitter: f32,
    /// What it costs for a widget to be smaller / larger than it likes, per squared log of the
    /// area ratio.
    pub shrink: f32,
    pub grow: f32,
    /// What a squarer or wider shape than the widget likes costs, per squared log of the ratio.
    pub aspect: f32,
    /// What each cell beyond a widget's largest size costs, squared.
    pub overshoot: f32,
    /// What each cell a round widget leaves empty around itself costs.
    pub hole: f32,
    /// Gravity: how much larger than its preferred area a widget may grow.
    pub max_grow: f32,
    /// Treemap: the area is weighted by the preferred area to this power. 1 is proportional, 0
    /// gives every widget the same.
    pub weight_exponent: f32,
    /// Treemap: above 1, cuts prefer to divide the width; below, the height.
    pub cut_bias: f32,
    /// Neighbours: how much liking each other counts against the size costs.
    pub affinity: f32,
    /// Neighbours: how many cells apart still count as near, for liking and disliking.
    pub reach: f32,
    /// Neighbours: where to lean, and how hard.
    pub placement: EdgeMode,
    pub edge: f32,
    /// Neighbours: how much the whole should sit in the middle of the grid, not in a corner.
    pub balance: f32,
    /// Neighbours: how long to search, as a multiple of the usual.
    pub effort: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            algo: Algo::Guillotine,
            order: Order::Group,
            seed: 0,
            restarts: 1,
            jitter: 0.25,
            shrink: 2.0,
            grow: 0.6,
            aspect: 0.35,
            overshoot: 0.5,
            hole: 0.3,
            max_grow: 1.6,
            weight_exponent: 1.0,
            cut_bias: 1.0,
            affinity: 1.0,
            reach: 1.5,
            placement: EdgeMode::Edges,
            edge: 0.5,
            balance: 0.1,
            effort: 1.0,
        }
    }
}

/// One setting of [`Params`], as the sidebar lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Knob {
    Algo,
    Order,
    Seed,
    Restarts,
    Jitter,
    Shrink,
    Grow,
    Aspect,
    Overshoot,
    Hole,
    MaxGrow,
    WeightExponent,
    CutBias,
    Affinity,
    Reach,
    Placement,
    Edge,
    Balance,
    Effort,
}

impl Knob {
    /// The key in config.kdl.
    pub fn key(self) -> &'static str {
        match self {
            Knob::Algo => "algo",
            Knob::Order => "order",
            Knob::Seed => "seed",
            Knob::Restarts => "restarts",
            Knob::Jitter => "jitter",
            Knob::Shrink => "shrink",
            Knob::Grow => "grow",
            Knob::Aspect => "aspect",
            Knob::Overshoot => "overshoot",
            Knob::Hole => "hole",
            Knob::MaxGrow => "max-grow",
            Knob::WeightExponent => "weight-exponent",
            Knob::CutBias => "cut-bias",
            Knob::Affinity => "affinity",
            Knob::Reach => "reach",
            Knob::Placement => "placement",
            Knob::Edge => "edge",
            Knob::Balance => "balance",
            Knob::Effort => "effort",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Knob::Algo => "Algorithm",
            Knob::Order => "Order",
            Knob::Seed => "Seed",
            Knob::Restarts => "Tries",
            Knob::Jitter => "Shuffle",
            Knob::Shrink => "Shrink cost",
            Knob::Grow => "Grow cost",
            Knob::Aspect => "Shape cost",
            Knob::Overshoot => "Oversize cost",
            Knob::Hole => "Round gap cost",
            Knob::MaxGrow => "Max growth",
            Knob::WeightExponent => "Area weight",
            Knob::CutBias => "Cut bias",
            Knob::Affinity => "Neighbour pull",
            Knob::Reach => "Neighbour reach",
            Knob::Placement => "Lean to",
            Knob::Edge => "Lean strength",
            Knob::Balance => "Centering",
            Knob::Effort => "Effort",
        }
    }

    /// The settings that matter to `algo`, in the order the sidebar lists them.
    pub fn of(algo: Algo) -> Vec<Knob> {
        let mut v = vec![
            Knob::Algo,
            Knob::Order,
            Knob::Seed,
            Knob::Restarts,
            Knob::Jitter,
        ];
        match algo {
            Algo::Guillotine => {}
            Algo::Treemap => v.extend([Knob::WeightExponent, Knob::CutBias]),
            Algo::Gravity => v.push(Knob::MaxGrow),
            // Nothing is cut or tiled, so the cost of an odd cut or a gap round widgets leave
            // doesn't apply: it's the liking, where it leans, and how hard to look.
            Algo::Neighbours => {
                v.extend([
                    Knob::Affinity,
                    Knob::Reach,
                    Knob::Placement,
                    Knob::Edge,
                    Knob::Balance,
                    Knob::Effort,
                    Knob::Shrink,
                    Knob::Grow,
                    Knob::Aspect,
                ]);
                return v;
            }
        }
        // What a good layout is: the guillotine optimizes it, the others choose between tries by it.
        v.extend([
            Knob::Shrink,
            Knob::Grow,
            Knob::Aspect,
            Knob::Overshoot,
            Knob::Hole,
        ]);
        v
    }
}

/// A number printed with at most two decimals, always with a point (as KDL wants for a float).
fn num(v: f32) -> String {
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0');
    if s.ends_with('.') {
        format!("{s}0")
    } else {
        s.to_string()
    }
}

fn round_to(v: f32, step: f32) -> f32 {
    ((v / step).round() * step * 100.0).round() / 100.0
}

/// The next / previous variant of an enum, wrapping.
fn cycle<T: IntoEnumIterator + PartialEq + Copy>(cur: T, delta: i32) -> T {
    let all: Vec<T> = T::iter().collect();
    let i = all.iter().position(|v| *v == cur).unwrap_or(0) as i32;
    all[(i + delta).rem_euclid(all.len() as i32) as usize]
}

impl Params {
    pub fn text(&self, k: Knob) -> String {
        match k {
            Knob::Algo => self.algo.as_ref().into(),
            Knob::Order => self.order.as_ref().into(),
            Knob::Seed => self.seed.to_string(),
            Knob::Restarts => self.restarts.to_string(),
            Knob::Jitter => num(self.jitter),
            Knob::Shrink => num(self.shrink),
            Knob::Grow => num(self.grow),
            Knob::Aspect => num(self.aspect),
            Knob::Overshoot => num(self.overshoot),
            Knob::Hole => num(self.hole),
            Knob::MaxGrow => num(self.max_grow),
            Knob::WeightExponent => num(self.weight_exponent),
            Knob::CutBias => num(self.cut_bias),
            Knob::Affinity => num(self.affinity),
            Knob::Reach => num(self.reach),
            Knob::Placement => self.placement.as_ref().into(),
            Knob::Edge => num(self.edge),
            Knob::Balance => num(self.balance),
            Knob::Effort => num(self.effort),
        }
    }

    /// The range and step of a numeric setting.
    fn span(k: Knob) -> (f32, f32, f32) {
        match k {
            Knob::Jitter => (0.0, 1.0, 0.05),
            Knob::Shrink | Knob::Grow | Knob::Aspect | Knob::Overshoot | Knob::Hole => {
                (0.0, 10.0, 0.25)
            }
            Knob::MaxGrow => (1.0, 3.0, 0.1),
            Knob::WeightExponent => (0.0, 2.0, 0.1),
            Knob::CutBias => (0.5, 2.0, 0.1),
            Knob::Affinity => (0.0, 5.0, 0.25),
            Knob::Reach => (0.5, 4.0, 0.25),
            Knob::Edge => (0.0, 2.0, 0.05),
            Knob::Balance => (0.0, 1.0, 0.05),
            Knob::Effort => (0.25, 8.0, 0.25),
            _ => (0.0, 0.0, 0.0),
        }
    }

    fn float_mut(&mut self, k: Knob) -> Option<&mut f32> {
        Some(match k {
            Knob::Jitter => &mut self.jitter,
            Knob::Shrink => &mut self.shrink,
            Knob::Grow => &mut self.grow,
            Knob::Aspect => &mut self.aspect,
            Knob::Overshoot => &mut self.overshoot,
            Knob::Hole => &mut self.hole,
            Knob::MaxGrow => &mut self.max_grow,
            Knob::WeightExponent => &mut self.weight_exponent,
            Knob::CutBias => &mut self.cut_bias,
            Knob::Affinity => &mut self.affinity,
            Knob::Reach => &mut self.reach,
            Knob::Edge => &mut self.edge,
            Knob::Balance => &mut self.balance,
            Knob::Effort => &mut self.effort,
            _ => return None,
        })
    }

    /// Step a setting up (+1) or down (-1), clamped; enums wrap around.
    pub fn step(&mut self, k: Knob, delta: i32) {
        match k {
            Knob::Algo => self.algo = cycle(self.algo, delta),
            Knob::Order => self.order = cycle(self.order, delta),
            Knob::Placement => self.placement = cycle(self.placement, delta),
            Knob::Seed => self.seed = (self.seed as i32 + delta).clamp(0, 999) as u32,
            // 1, 2, 4, … 64: trying twice as many is the interesting step.
            Knob::Restarts => {
                self.restarts = if delta > 0 {
                    (self.restarts * 2).min(64)
                } else {
                    (self.restarts / 2).max(1)
                }
            }
            _ => {
                let (lo, hi, step) = Self::span(k);
                if let Some(v) = self.float_mut(k) {
                    *v = round_to(*v + step * delta as f32, step).clamp(lo, hi);
                }
            }
        }
    }

    /// Whether a setting can still be stepped `delta`.
    pub fn can_step(&self, k: Knob, delta: i32) -> bool {
        match k {
            Knob::Algo | Knob::Order | Knob::Placement => true,
            Knob::Seed => (delta < 0 && self.seed > 0) || (delta > 0 && self.seed < 999),
            Knob::Restarts => (delta < 0 && self.restarts > 1) || (delta > 0 && self.restarts < 64),
            _ => {
                let (lo, hi, step) = Self::span(k);
                let mut p = self.clone();
                let v = p.float_mut(k).map_or(0.0, |v| *v);
                (delta < 0 && v - step >= lo - 1e-4) || (delta > 0 && v + step <= hi + 1e-4)
            }
        }
    }

    pub fn is_default(&self, k: Knob) -> bool {
        self.text(k) == Params::default().text(k)
    }

    pub fn reset(&mut self, k: Knob) {
        let d = Params::default();
        match k {
            Knob::Algo => self.algo = d.algo,
            Knob::Order => self.order = d.order,
            Knob::Placement => self.placement = d.placement,
            Knob::Seed => self.seed = d.seed,
            Knob::Restarts => self.restarts = d.restarts,
            _ => {
                let mut d = d;
                let dv = d.float_mut(k).map(|v| *v);
                if let (Some(v), Some(dv)) = (self.float_mut(k), dv) {
                    *v = dv;
                }
            }
        }
    }

    /// The `autolayout` line for config.kdl: every setting, so it reads back as it was.
    pub fn to_kdl(&self) -> String {
        let mut line = String::from("autolayout");
        for k in [
            Knob::Algo,
            Knob::Order,
            Knob::Seed,
            Knob::Restarts,
            Knob::Jitter,
            Knob::Shrink,
            Knob::Grow,
            Knob::Aspect,
            Knob::Overshoot,
            Knob::Hole,
            Knob::MaxGrow,
            Knob::WeightExponent,
            Knob::CutBias,
            Knob::Affinity,
            Knob::Reach,
            Knob::Placement,
            Knob::Edge,
            Knob::Balance,
            Knob::Effort,
        ] {
            let v = self.text(k);
            if matches!(k, Knob::Algo | Knob::Order | Knob::Placement) {
                line.push_str(&format!(" {}={v:?}", k.key()));
            } else {
                line.push_str(&format!(" {}={v}", k.key()));
            }
        }
        line
    }

    /// Read an `autolayout` node; what it doesn't say stays as it is, what is wrong is said in
    /// `warnings`.
    pub fn from_node(node: &KdlNode, warnings: &mut Vec<String>) -> Self {
        let mut p = Params::default();
        for e in node.entries() {
            let Some(key) = e.name().map(|n| n.value()) else {
                warnings.push("autolayout: expected key=value".into());
                continue;
            };
            let v = e.value();
            let float = match v {
                KdlValue::Integer(i) => Some(*i as f64),
                KdlValue::Float(f) => Some(*f),
                _ => None,
            };
            let mut bad = |what: &str| warnings.push(format!("autolayout {key}: {what}"));
            match key {
                "algo" => match v.as_string().and_then(|s| s.parse().ok()) {
                    Some(a) => p.algo = a,
                    None => bad(r#"expected "guillotine", "treemap", "gravity" or "neighbours""#),
                },
                "order" => match v.as_string().and_then(|s| s.parse().ok()) {
                    Some(o) => p.order = o,
                    None => bad(r#"expected "group", "area", "reading", "kind" or "affinity""#),
                },
                "placement" => match v.as_string().and_then(|s| s.parse().ok()) {
                    Some(m) => p.placement = m,
                    None => bad(r#"expected "off", "edges" or "center""#),
                },
                "seed" => match float {
                    Some(f) if (0.0..=999.0).contains(&f) => p.seed = f as u32,
                    _ => bad("expected a number from 0 to 999"),
                },
                "restarts" => match float {
                    Some(f) if (1.0..=64.0).contains(&f) => p.restarts = f as u32,
                    _ => bad("expected a number from 1 to 64"),
                },
                _ => {
                    let knob = [
                        Knob::Jitter,
                        Knob::Shrink,
                        Knob::Grow,
                        Knob::Aspect,
                        Knob::Overshoot,
                        Knob::Hole,
                        Knob::MaxGrow,
                        Knob::WeightExponent,
                        Knob::CutBias,
                        Knob::Affinity,
                        Knob::Reach,
                        Knob::Edge,
                        Knob::Balance,
                        Knob::Effort,
                    ]
                    .into_iter()
                    .find(|k| k.key() == key);
                    match (knob, float) {
                        (Some(k), Some(f)) => {
                            let (lo, hi, _) = Self::span(k);
                            if let Some(slot) = p.float_mut(k) {
                                *slot = (f as f32).clamp(lo, hi);
                            }
                        }
                        (Some(_), None) => bad("expected a number"),
                        (None, _) => warnings.push(format!("autolayout: unknown setting `{key}`")),
                    }
                }
            }
        }
        p
    }
}

// ------------------------------------------------------------------------------------------
// The problem.

/// A widget to place, as the algorithms see it.
#[derive(Debug, Clone, Copy)]
pub struct Item {
    pub id: u32,
    pub kind: Kind,
    pub round: bool,
    /// Where it is now.
    pub at: Rect,
}

impl Item {
    fn pref(&self) -> (i32, i32) {
        if self.round {
            (ROUND_SIZE, ROUND_SIZE)
        } else {
            self.kind.info().default_size
        }
    }

    fn min(&self) -> (i32, i32) {
        if self.round {
            (1, 1)
        } else {
            self.kind.info().min_size
        }
    }

    fn max(&self) -> (i32, i32) {
        if self.round {
            (ROUND_MAX, ROUND_MAX)
        } else {
            self.kind.info().max_size
        }
    }

    fn pref_area(&self) -> f32 {
        let (w, h) = self.pref();
        (w * h) as f32
    }

    /// What it costs for the widget to be `w` x `h`; infinite below its minimum size.
    fn cost(&self, w: i32, h: i32, p: &Params) -> f32 {
        let (mw, mh) = self.min();
        if w < mw || h < mh {
            return f32::INFINITY;
        }
        let (pw, ph) = self.pref();
        let a = ((w * h) as f32 / (pw * ph) as f32).ln();
        let area = a * a * if a < 0.0 { p.shrink } else { p.grow };
        let s = ((w as f32 / h as f32) / (pw as f32 / ph as f32)).ln();
        let (xw, xh) = self.max();
        let over = ((w - xw).max(0) + (h - xh).max(0)) as f32;
        area + s * s * p.aspect + over * over * p.overshoot
    }

    /// What it costs to be given a region of `w` x `h` cells: a round widget fills the largest
    /// square in it and leaves the rest empty.
    fn region_cost(&self, w: i32, h: i32, p: &Params) -> f32 {
        if self.round {
            let s = w.min(h).min(ROUND_MAX);
            self.cost(s, s, p) + p.hole * (w * h - s * s) as f32
        } else {
            self.cost(w, h, p)
        }
    }

    /// The rectangle the widget takes in a region: all of it, or for a round one the middle
    /// square.
    fn in_region(&self, r: Rect) -> Rect {
        if !self.round {
            return r;
        }
        let s = r.w.min(r.h).min(ROUND_MAX);
        Rect {
            col: r.col + (r.w - s) / 2,
            row: r.row + (r.h - s) / 2,
            w: s,
            h: s,
        }
    }
}

/// What came out.
#[derive(Debug, Clone)]
pub struct Arrangement {
    /// Where each widget that fit goes.
    pub rects: Vec<(u32, Rect)>,
    /// The widgets there was no room for.
    pub unplaced: Vec<u32>,
    /// The mean cost of a widget under the parameters (lower is better), the share of the grid
    /// the widgets cover, and how many shuffles were tried.
    pub cost: f32,
    pub fill: f32,
    pub tries: u32,
    /// Of the pairs that like each other, how many touch and how many there are; and how many
    /// that dislike each other touch.
    pub likes: (usize, usize, usize),
    /// How many widgets were pinned and so stayed where they were.
    pub pinned: usize,
    pub millis: u128,
    pub note: Option<String>,
}

impl Arrangement {
    /// One line for a toast.
    pub fn summary(&self) -> String {
        let mut s = format!(
            "{} placed \u{b7} cost {:.2} \u{b7} {:.0}% filled \u{b7} {} ms",
            self.rects.len(),
            self.cost,
            self.fill * 100.0,
            self.millis
        );
        if self.likes.1 > 0 {
            s.push_str(&format!(
                " \u{b7} {}/{} liked pairs touch",
                self.likes.0, self.likes.1
            ));
        }
        if self.likes.2 > 0 {
            s.push_str(&format!(" \u{b7} {} disliked touch", self.likes.2));
        }
        if !self.unplaced.is_empty() {
            s.push_str(&format!(" \u{b7} no room for {}", self.unplaced.len()));
        }
        if self.pinned > 0 {
            s.push_str(&format!(" \u{b7} {} pinned", self.pinned));
        }
        if self.tries > 1 {
            s.push_str(&format!(" \u{b7} best of {}", self.tries));
        }
        if let Some(n) = &self.note {
            s.push_str(&format!(" \u{b7} {n}"));
        }
        s
    }
}

/// Lay out the widgets of `layout` on its grid and move them there. Widgets there is no room
/// for are left outside the grid (where they are hidden, not lost).
pub fn arrange(layout: &mut Layout, p: &Params) -> Arrangement {
    // Pinned widgets that show stay where they are, and are walls for the rest. (A pinned one
    // that doesn't show, outside the grid, has no place to keep and is laid out like the others.)
    let pinned: Vec<(u32, Rect)> = layout
        .visible()
        .into_iter()
        .filter(|w| w.pinned)
        .map(|w| (w.id, w.rect()))
        .collect();
    let pins: Vec<Rect> = pinned.iter().map(|&(_, r)| r).collect();
    let items: Vec<Item> = layout
        .widgets
        .iter()
        .filter(|w| !pinned.iter().any(|&(id, _)| id == w.id))
        .map(|w| Item {
            id: w.id,
            kind: w.kind,
            round: w.shape == crate::layout::Shape::Round,
            at: w.rect(),
        })
        .collect();
    let a = solve(&items, &pins, layout.cols, layout.rows, p);
    layout.set_rects(&a.rects);
    layout.park(&a.unplaced);
    a
}

/// The grid the items go on: its size, and the rectangles `pins` take out of it.
#[derive(Clone, Copy)]
struct Grid<'a> {
    cols: i32,
    rows: i32,
    pins: &'a [Rect],
}

impl Grid<'_> {
    /// The cells there are to fill.
    fn free_area(&self) -> i32 {
        self.cols * self.rows - self.pins.iter().map(|r| r.w * r.h).sum::<i32>()
    }

    /// The free cells as rectangles that don't overlap, the whole grid when nothing is pinned.
    /// `wide` takes each as far right as it goes, then as far down; otherwise the other way
    /// round. Neither is always the nicer one, so the algorithms try both.
    fn regions(&self, wide: bool) -> Vec<Rect> {
        let (c, r) = (self.cols, self.rows);
        let mut taken = vec![false; (c * r) as usize];
        for p in self.pins {
            for y in p.row..p.row + p.h {
                for x in p.col..p.col + p.w {
                    taken[(y * c + x) as usize] = true;
                }
            }
        }
        let free = |t: &[bool], x: i32, y: i32| x < c && y < r && !t[(y * c + x) as usize];
        let mut out = Vec::new();
        for y in 0..r {
            for x in 0..c {
                if !free(&taken, x, y) {
                    continue;
                }
                let (mut w, mut h) = (1, 1);
                if wide {
                    while free(&taken, x + w, y) {
                        w += 1;
                    }
                    while (x..x + w).all(|xx| free(&taken, xx, y + h)) {
                        h += 1;
                    }
                } else {
                    while free(&taken, x, y + h) {
                        h += 1;
                    }
                    while (y..y + h).all(|yy| free(&taken, x + w, yy)) {
                        w += 1;
                    }
                }
                for yy in y..y + h {
                    for xx in x..x + w {
                        taken[(yy * c + xx) as usize] = true;
                    }
                }
                out.push(Rect {
                    col: x,
                    row: y,
                    w,
                    h,
                });
            }
        }
        out
    }
}

/// Lay `items` out on a `cols` x `rows` grid, around the rectangles in `pins`.
pub fn solve(items: &[Item], pins: &[Rect], cols: i32, rows: i32, p: &Params) -> Arrangement {
    let grid = Grid { cols, rows, pins };
    let start = Instant::now();
    let mut note = None;
    let mut best: Option<Candidate> = None;
    let mut tries = 0;
    let mut algo = p.algo;
    let mut budget = DP_BUDGET;
    for r in 0..p.restarts.max(1) {
        let order = ordered(items, p, if r == 0 && p.seed == 0 { 0 } else { p.seed + r });
        let mut out = run(algo, items, &order, grid, p, &mut budget, p.seed + r);
        if out.is_none() && algo == Algo::Guillotine && tries == 0 {
            note = Some("too many widgets for exact cuts; used treemap".into());
            algo = Algo::Treemap;
            out = run(algo, items, &order, grid, p, &mut budget, p.seed + r);
        }
        let Some((placed, unplaced)) = out else {
            break; // the time for tries is used up
        };
        tries += 1;
        let c = if algo == Algo::Neighbours {
            anneal::energy(items, &placed, grid, p)
        } else {
            score(items, &placed, p)
        };
        let better = best.as_ref().is_none_or(|(_, un, bc)| {
            unplaced.len() < un.len() || (unplaced.len() == un.len() && c < *bc)
        });
        if better {
            best = Some((placed, unplaced, c));
        }
    }
    let (placed, unplaced, cost) = best.unwrap_or_default();
    let likes = anneal::tally(items, &placed);
    let covered: i32 = placed.iter().map(|(_, r)| r.w * r.h).sum::<i32>()
        + pins.iter().map(|r| r.w * r.h).sum::<i32>();
    Arrangement {
        rects: placed.iter().map(|&(i, r)| (items[i].id, r)).collect(),
        unplaced: unplaced.iter().map(|&i| items[i].id).collect(),
        cost,
        fill: covered as f32 / (cols * rows).max(1) as f32,
        likes,
        tries,
        pinned: pins.len(),
        millis: start.elapsed().as_millis(),
        note,
    }
}

/// How much work the exact search may do in all before it gives up (about a second in a
/// browser).
const DP_BUDGET: u64 = 60_000_000;

/// The mean cost of the widgets in `placed`.
fn score(items: &[Item], placed: &[(usize, Rect)], p: &Params) -> f32 {
    if placed.is_empty() {
        return 0.0;
    }
    let total: f32 = placed
        .iter()
        .map(|&(i, r)| items[i].cost(r.w, r.h, p))
        .sum();
    total / placed.len() as f32
}

type Tried = (Vec<(usize, Rect)>, Vec<usize>);

/// A try that is the best so far: where the widgets go, the ones left out, how it scored.
type Candidate = (Vec<(usize, Rect)>, Vec<usize>, f32);

/// One try with `algo`, the widgets in `order` (indices into `items`). Widgets that don't fit
/// are dropped from the end of the order, one at a time, until the rest do.
fn run(
    algo: Algo,
    items: &[Item],
    order: &[usize],
    grid: Grid,
    p: &Params,
    budget: &mut u64,
    seed: u32,
) -> Option<Tried> {
    let mut keep = order.to_vec();
    let mut dropped = Vec::new();
    // They can't all fit when their minimum areas don't.
    let min_area = |idx: &[usize]| -> i32 {
        idx.iter()
            .map(|&i| {
                let (w, h) = items[i].min();
                w * h
            })
            .sum()
    };
    while !keep.is_empty() && min_area(&keep) > grid.free_area() {
        dropped.push(keep.pop()?);
    }
    loop {
        if keep.is_empty() {
            return Some((Vec::new(), dropped));
        }
        if algo == Algo::Gravity {
            return Some(gravity(items, &keep, grid, p, dropped));
        }
        if algo == Algo::Neighbours {
            return Some(anneal::anneal(items, &keep, grid, p, seed, dropped));
        }
        // With walls the free space is several rectangles, cut one of two ways; the cheaper
        // layout of the two stays.
        let mut cuts = vec![grid.regions(true)];
        if !grid.pins.is_empty() {
            cuts.push(grid.regions(false));
        }
        let mut best: Option<(f32, Vec<(usize, Rect)>)> = None;
        for regions in &cuts {
            let found = match algo {
                Algo::Guillotine => guillotine(items, &keep, regions, p, budget)?,
                _ => treemap(items, &keep, regions, p),
            };
            if let Some(found) = found {
                let found: Vec<(usize, Rect)> = found
                    .into_iter()
                    .map(|(i, r)| (i, items[i].in_region(r)))
                    .collect();
                let c = score(items, &found, p);
                if best.as_ref().is_none_or(|(bc, _)| c < *bc) {
                    best = Some((c, found));
                }
            }
        }
        if let Some((_, placed)) = best {
            return Some((placed, dropped));
        }
        dropped.push(keep.pop()?);
    }
}

// ------------------------------------------------------------------------------------------
// Orders.

/// splitmix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// In 0..1.
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// The widgets in the order `p.order` gives, shuffled by `seed` (0: not at all).
fn ordered(items: &[Item], p: &Params, seed: u32) -> Vec<usize> {
    let idx = shuffled(items, p, seed);
    if p.order == Order::Affinity {
        seriate(items, &idx)
    } else {
        idx
    }
}

/// `base` as one chain in which each widget is next to the ones it likes most: it starts where
/// `base` does, always goes on to the liked-most of the rest, and then swaps stretches round
/// while that makes the neighbours like each other more.
fn seriate(items: &[Item], base: &[usize]) -> Vec<usize> {
    let like = |a: usize, b: usize| crate::affinity::affinity(items[a].kind, items[b].kind);
    let Some((&first, rest)) = base.split_first() else {
        return Vec::new();
    };
    let mut path = vec![first];
    let mut rest = rest.to_vec();
    while !rest.is_empty() {
        let last = path[path.len() - 1];
        let mut at = 0;
        for (n, &c) in rest.iter().enumerate() {
            if like(last, c) > like(last, rest[at]) {
                at = n;
            }
        }
        path.push(rest.remove(at));
    }
    // Reversing path[i..=j] swaps the two links at its ends for two others.
    let n = path.len();
    let link = |path: &[usize], a: usize, b: usize| like(path[a], path[b]);
    loop {
        let mut better = false;
        for i in 1..n {
            for j in i + 1..n {
                let before = link(&path, i - 1, i)
                    + if j + 1 < n {
                        link(&path, j, j + 1)
                    } else {
                        0.0
                    };
                let after = link(&path, i - 1, j)
                    + if j + 1 < n {
                        link(&path, i, j + 1)
                    } else {
                        0.0
                    };
                if after > before + 1e-4 {
                    path[i..=j].reverse();
                    better = true;
                }
            }
        }
        if !better {
            break;
        }
    }
    path
}

fn shuffled(items: &[Item], p: &Params, seed: u32) -> Vec<usize> {
    let kinds: Vec<Kind> = Kind::iter().collect();
    let kind_pos = |i: usize| kinds.iter().position(|k| *k == items[i].kind).unwrap_or(0);
    let area = |i: usize| (items[i].pref_area() * 100.0) as i64;
    let mut idx: Vec<usize> = (0..items.len()).collect();
    match p.order {
        Order::Group | Order::Affinity => {
            idx.sort_by_key(|&i| (items[i].kind.info().group, -area(i), kind_pos(i), i))
        }
        Order::Area => idx.sort_by_key(|&i| (-area(i), kind_pos(i), i)),
        Order::Reading => idx.sort_by_key(|&i| (items[i].at.row, items[i].at.col, i)),
        Order::Kind => idx.sort_by_key(|&i| (kind_pos(i), i)),
    }
    if seed == 0 || idx.len() < 2 {
        return idx;
    }
    let mut rng = Rng(u64::from(seed).wrapping_mul(0x2545_F491_4F6C_DD1D) ^ 0xD1B5_4A32_D192_ED03);
    // Groups swap places (the group order only).
    let mut group_rank: Vec<(Group, f32)> = Vec::new();
    if p.order == Order::Group {
        let present: std::collections::BTreeSet<Group> =
            items.iter().map(|it| it.kind.info().group).collect();
        group_rank = present.into_iter().map(|g| (g, rng.unit())).collect();
        group_rank.sort_by(|a, b| a.1.total_cmp(&b.1));
    }
    let n = idx.len() as f32;
    let mut keyed: Vec<(f32, usize)> = idx
        .iter()
        .enumerate()
        .map(|(pos, &i)| {
            let jitter = (rng.unit() * 2.0 - 1.0) * p.jitter * n;
            let g = group_rank
                .iter()
                .position(|(g, _)| *g == items[i].kind.info().group)
                .unwrap_or(0) as f32;
            // A group's items stay within its band; the jitter only moves them inside it.
            (g * (n + 1.0) * 2.0 + pos as f32 + jitter, i)
        })
        .collect();
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
    keyed.into_iter().map(|(_, i)| i).collect()
}

// ------------------------------------------------------------------------------------------
// Guillotine: the cheapest tree of cuts, by dynamic programming.

struct Dp {
    n: usize,
    cols: usize,
    rows: usize,
    /// Cost of widget i in a region of w x h: `leaf[(i * (cols + 1) + w) * (rows + 1) + h]`.
    leaf: Vec<f32>,
    /// Prefix sums of the widgets' minimum areas; and, for a run of widgets i..j, the widest
    /// and tallest of their minimum sizes.
    min_area: Vec<i32>,
    min_w: Vec<i32>,
    min_h: Vec<i32>,
    cost: Vec<f32>,
    choice: Vec<u32>,
    work: u64,
    limit: u64,
}

impl Dp {
    fn at(&self, i: usize, j: usize, w: usize, h: usize) -> usize {
        ((i * (self.n + 1) + j) * (self.cols + 1) + w) * (self.rows + 1) + h
    }

    fn run(&self, i: usize, j: usize) -> usize {
        i * (self.n + 1) + j
    }

    /// The cheapest way to fill `w` x `h` with widgets `i..j`.
    fn solve(&mut self, i: usize, j: usize, w: usize, h: usize) -> f32 {
        if j - i == 1 {
            return self.leaf[(i * (self.cols + 1) + w) * (self.rows + 1) + h];
        }
        let at = self.at(i, j, w, h);
        if !self.cost[at].is_nan() {
            return self.cost[at];
        }
        let r = self.run(i, j);
        let mut best = f32::INFINITY;
        let mut how = 0u32;
        let feasible = self.min_area[j] - self.min_area[i] <= (w * h) as i32
            && self.min_w[r] <= w as i32
            && self.min_h[r] <= h as i32;
        if feasible {
            for k in i + 1..j {
                let (a_area, b_area) = (
                    self.min_area[k] - self.min_area[i],
                    self.min_area[j] - self.min_area[k],
                );
                for cut in 1..w {
                    if self.work >= self.limit {
                        break;
                    }
                    self.work += 1;
                    if a_area > (cut * h) as i32 || b_area > ((w - cut) * h) as i32 {
                        continue;
                    }
                    let a = self.solve(i, k, cut, h);
                    if a >= best {
                        continue;
                    }
                    let c = a + self.solve(k, j, w - cut, h);
                    if c < best {
                        best = c;
                        how = (k as u32) << 16 | cut as u32;
                    }
                }
                for cut in 1..h {
                    if self.work >= self.limit {
                        break;
                    }
                    self.work += 1;
                    if a_area > (w * cut) as i32 || b_area > (w * (h - cut)) as i32 {
                        continue;
                    }
                    let a = self.solve(i, k, w, cut);
                    if a >= best {
                        continue;
                    }
                    let c = a + self.solve(k, j, w, h - cut);
                    if c < best {
                        best = c;
                        how = 1 << 31 | (k as u32) << 16 | cut as u32;
                    }
                }
            }
        }
        self.cost[at] = best;
        self.choice[at] = how;
        best
    }

    fn build(&self, i: usize, j: usize, r: Rect, out: &mut [Rect]) {
        if j - i == 1 {
            out[i] = r;
            return;
        }
        let how = self.choice[self.at(i, j, r.w as usize, r.h as usize)];
        let (k, cut) = (((how >> 16) & 0x7fff) as usize, (how & 0xffff) as i32);
        if how >> 31 == 0 {
            self.build(i, k, Rect { w: cut, ..r }, out);
            self.build(
                k,
                j,
                Rect {
                    col: r.col + cut,
                    w: r.w - cut,
                    ..r
                },
                out,
            );
        } else {
            self.build(i, k, Rect { h: cut, ..r }, out);
            self.build(
                k,
                j,
                Rect {
                    row: r.row + cut,
                    h: r.h - cut,
                    ..r
                },
                out,
            );
        }
    }
}

/// What it costs to leave a free cell empty, which only happens when a wall splits the grid and
/// there is nothing to put in a piece of it.
const EMPTY_CELL: f32 = 0.5;

/// `None` if the search ran out of budget; `Some(None)` if there is no way to fit them all.
/// `regions` are the free rectangles; the widgets, in order, go in them one run after another,
/// each run cut up optimally.
fn guillotine(
    items: &[Item],
    order: &[usize],
    regions: &[Rect],
    p: &Params,
    budget: &mut u64,
) -> Option<Option<Vec<(usize, Rect)>>> {
    let n = order.len();
    let c = regions.iter().map(|r| r.w).max().unwrap_or(0) as usize;
    let r = regions.iter().map(|r| r.h).max().unwrap_or(0) as usize;
    // Too many widgets for the table to be sensible.
    if n > 60 || c < 1 || r < 1 {
        return None;
    }
    let seq: Vec<&Item> = order.iter().map(|&i| &items[i]).collect();
    let mut leaf = vec![f32::INFINITY; n * (c + 1) * (r + 1)];
    for (i, it) in seq.iter().enumerate() {
        for w in 1..=c {
            for h in 1..=r {
                leaf[(i * (c + 1) + w) * (r + 1) + h] = it.region_cost(w as i32, h as i32, p);
            }
        }
    }
    let mut min_area = vec![0; n + 1];
    let mut min_w = vec![0; (n + 1) * (n + 1)];
    let mut min_h = vec![0; (n + 1) * (n + 1)];
    for i in 0..n {
        let (mw, mh) = seq[i].min();
        min_area[i + 1] = min_area[i] + mw * mh;
        for j in i + 1..=n {
            let (w, h) = seq[j - 1].min();
            // (Zero for the empty run i..i.)
            min_w[i * (n + 1) + j] = min_w[i * (n + 1) + j - 1].max(w);
            min_h[i * (n + 1) + j] = min_h[i * (n + 1) + j - 1].max(h);
        }
    }
    let size = (n + 1) * (n + 1) * (c + 1) * (r + 1);
    let mut dp = Dp {
        n,
        cols: c,
        rows: r,
        leaf,
        min_area,
        min_w,
        min_h,
        cost: vec![f32::NAN; size],
        choice: vec![0; size],
        work: 0,
        limit: *budget,
    };
    // g[k][j]: the cheapest way to put the first j widgets in the first k regions; from[k][j]
    // is where the run in region k begins. A region may stay empty.
    let m = regions.len();
    let mut g = vec![f32::INFINITY; (m + 1) * (n + 1)];
    let mut from = vec![0usize; (m + 1) * (n + 1)];
    g[0] = 0.0;
    for (k, reg) in regions.iter().enumerate() {
        let (w, h) = (reg.w as usize, reg.h as usize);
        for j in 0..=n {
            for i in 0..=j {
                let before = g[k * (n + 1) + i];
                if !before.is_finite() {
                    continue;
                }
                let here = if i == j {
                    EMPTY_CELL * (w * h) as f32
                } else {
                    dp.solve(i, j, w, h)
                };
                if before + here < g[(k + 1) * (n + 1) + j] {
                    g[(k + 1) * (n + 1) + j] = before + here;
                    from[(k + 1) * (n + 1) + j] = i;
                }
            }
        }
    }
    *budget = budget.saturating_sub(dp.work);
    if dp.work >= dp.limit {
        return None;
    }
    if !g[m * (n + 1) + n].is_finite() {
        return Some(None);
    }
    let mut rects = vec![
        Rect {
            col: 0,
            row: 0,
            w: 0,
            h: 0
        };
        n
    ];
    let mut j = n;
    for k in (1..=m).rev() {
        let i = from[k * (n + 1) + j];
        if i < j {
            dp.build(i, j, regions[k - 1], &mut rects);
        }
        j = i;
    }
    Some(Some(order.iter().copied().zip(rects).collect()))
}

// ------------------------------------------------------------------------------------------
// Treemap: cut in proportion to the preferred areas.

fn treemap(
    items: &[Item],
    order: &[usize],
    regions: &[Rect],
    p: &Params,
) -> Option<Vec<(usize, Rect)>> {
    // The widgets, in order, go to the regions in proportion to the regions' areas: a region gets
    // the widgets whose middle falls within its share of the total weight.
    let weight = |i: usize| items[i].pref_area().powf(p.weight_exponent);
    let total: f32 = order.iter().map(|&i| weight(i)).sum();
    let area: f32 = regions.iter().map(|r| (r.w * r.h) as f32).sum();
    let mut runs: Vec<Vec<usize>> = vec![Vec::new(); regions.len()];
    let (mut done, mut upto, mut k) = (0.0, 0.0, 0);
    for &i in order {
        let mid = done + weight(i) / 2.0;
        while k + 1 < regions.len()
            && mid > upto + total * (regions[k].w * regions[k].h) as f32 / area
        {
            upto += total * (regions[k].w * regions[k].h) as f32 / area;
            k += 1;
        }
        runs[k].push(i);
        done += weight(i);
    }
    // First insisting that round widgets get a roughly square region (backtracking over the cuts
    // for a while to find one), then taking what there is.
    for strict in [true, false] {
        let mut out = Vec::with_capacity(order.len());
        let mut steps = if strict { 4000 } else { u32::MAX };
        let all = runs.iter().zip(regions).all(|(run, &region)| {
            run.is_empty() || cut_region(items, run, region, p, strict, &mut steps, &mut out)
        });
        if all {
            return Some(out);
        }
    }
    None
}

/// The smallest extent along one axis that holds `idx` in a region `across` cells wide the
/// other way.
fn needs(items: &[Item], idx: &[usize], across: i32, along_w: bool) -> i32 {
    let area: i32 = idx
        .iter()
        .map(|&i| {
            let (w, h) = items[i].min();
            w * h
        })
        .sum();
    let widest = idx
        .iter()
        .map(|&i| {
            let (w, h) = items[i].min();
            if along_w { w } else { h }
        })
        .max()
        .unwrap_or(0);
    widest.max((area + across - 1) / across.max(1))
}

fn cut_region(
    items: &[Item],
    idx: &[usize],
    r: Rect,
    p: &Params,
    strict: bool,
    steps: &mut u32,
    out: &mut Vec<(usize, Rect)>,
) -> bool {
    if *steps == 0 {
        return false;
    }
    *steps -= 1;
    if idx.len() == 1 {
        let it = &items[idx[0]];
        let (mw, mh) = it.min();
        if r.w < mw || r.h < mh {
            return false;
        }
        // A sliver of a widget that wants a normal shape isn't worth having.
        if strict && !it.round {
            let (pw, ph) = it.pref();
            let skew = ((r.w as f32 / r.h as f32) / (pw as f32 / ph as f32))
                .ln()
                .abs();
            if skew > 2.5f32.ln() {
                return false;
            }
        }
        // A circle in a long strip, or in a region far bigger than it gets to be, leaves a hole.
        if strict && it.round && (r.w.max(r.h) > r.w.min(r.h) + 1 || r.w.min(r.h) > ROUND_MAX + 1) {
            return false;
        }
        out.push((idx[0], r));
        return true;
    }
    let weight = |i: usize| items[i].pref_area().powf(p.weight_exponent);
    let total: f32 = idx.iter().map(|&i| weight(i)).sum();
    // Candidate cuts: every split of the run, both directions, the most natural first.
    let mut cands: Vec<(f32, usize, bool)> = Vec::new();
    let mut left = 0.0;
    for k in 1..idx.len() {
        left += weight(idx[k - 1]);
        let share = left / total;
        let boundary = items[idx[k - 1]].kind.info().group != items[idx[k]].kind.info().group;
        let balance = (share - 0.5).abs() - if boundary { 0.12 } else { 0.0 };
        for vertical in [true, false] {
            // Cut across the longer side; cut_bias tilts that towards the width.
            let long = if vertical {
                r.w as f32 * p.cut_bias >= r.h as f32
            } else {
                r.w as f32 * p.cut_bias < r.h as f32
            };
            cands.push((balance + if long { 0.0 } else { 1.0 }, k, vertical));
        }
    }
    cands.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, k, vertical) in cands {
        let (a, b) = idx.split_at(k);
        let share = a.iter().map(|&i| weight(i)).sum::<f32>() / total;
        let (span, across) = if vertical { (r.w, r.h) } else { (r.h, r.w) };
        let lo = needs(items, a, across, vertical);
        let hi = span - needs(items, b, across, vertical);
        if lo > hi {
            continue;
        }
        let cut = ((span as f32 * share).round() as i32).clamp(lo.max(1), hi.min(span - 1));
        if cut < 1 {
            continue;
        }
        let (ra, rb) = if vertical {
            (
                Rect { w: cut, ..r },
                Rect {
                    col: r.col + cut,
                    w: r.w - cut,
                    ..r
                },
            )
        } else {
            (
                Rect { h: cut, ..r },
                Rect {
                    row: r.row + cut,
                    h: r.h - cut,
                    ..r
                },
            )
        };
        let mark = out.len();
        if cut_region(items, a, ra, p, strict, steps, out)
            && cut_region(items, b, rb, p, strict, steps, out)
        {
            return true;
        }
        out.truncate(mark);
    }
    false
}

// ------------------------------------------------------------------------------------------
// Gravity: first fit at the preferred size, then grow.

/// Which cells of the grid are taken, and by whom.
#[derive(Clone)]
struct Cells {
    cols: i32,
    rows: i32,
    v: Vec<usize>,
}

impl Cells {
    const FREE: usize = usize::MAX;
    /// Pinned widgets: cells nobody may take.
    const WALL: usize = usize::MAX - 1;

    fn new(walls: Grid) -> Self {
        let mut c = Cells {
            cols: walls.cols,
            rows: walls.rows,
            v: vec![Self::FREE; (walls.cols * walls.rows) as usize],
        };
        for w in walls.pins {
            c.paint(w, Self::WALL);
        }
        c
    }

    fn paint(&mut self, r: &Rect, v: usize) {
        for y in r.row..r.row + r.h {
            for x in r.col..r.col + r.w {
                self.v[(y * self.cols + x) as usize] = v;
            }
        }
    }

    /// Whether `r` is on the grid and nothing but `me` and `also` is in it.
    fn free_but(&self, me: usize, also: usize, r: &Rect) -> bool {
        r.col >= 0
            && r.row >= 0
            && r.col + r.w <= self.cols
            && r.row + r.h <= self.rows
            && (r.row..r.row + r.h).all(|y| {
                (r.col..r.col + r.w).all(|x| {
                    let o = self.v[(y * self.cols + x) as usize];
                    o == Self::FREE || o == me || o == also
                })
            })
    }

    fn free(&self, me: usize, r: &Rect) -> bool {
        self.free_but(me, me, r)
    }
}

/// How large a widget may be placed when the sizes are held to `scale`: 1.0 is its preferred
/// size, 0.0 its smallest.
fn capped(it: &Item, scale: f32) -> (i32, i32) {
    let (pw, ph) = it.pref();
    let (mw, mh) = it.min();
    (
        mw + ((pw - mw) as f32 * scale).round() as i32,
        mh + ((ph - mh) as f32 * scale).round() as i32,
    )
}

/// The sizes held to, from the preferred one down.
const SCALES: [f32; 7] = [1.0, 0.85, 0.7, 0.55, 0.4, 0.2, 0.0];

/// First fit: each widget of `order` into the first spot with room, the largest size within
/// `scale` that fits. Returns what is taken, where each went, and the ones that found no room.
fn first_fit(
    items: &[Item],
    order: &[usize],
    walls: Grid,
    scale: f32,
) -> (Cells, Vec<Option<Rect>>, Vec<usize>) {
    let (cols, rows) = (walls.cols, walls.rows);
    let mut cells = Cells::new(walls);
    let mut placed: Vec<Option<Rect>> = vec![None; items.len()];
    let mut failed = Vec::new();
    for &i in order {
        let it = &items[i];
        let (pw, ph) = it.pref();
        let (mw, mh) = it.min();
        let (cap_w, cap_h) = capped(it, scale);
        let mut sizes: Vec<(i32, i32)> = (mw..=cap_w)
            .flat_map(|w| (mh..=cap_h).map(move |h| (w, h)))
            .filter(|&(w, h)| !it.round || w == h)
            .collect();
        sizes.sort_by_key(|&(w, h)| (-(w * h), (w - pw).abs() + (h - ph).abs()));
        let spot = sizes.iter().find_map(|&(w, h)| {
            (0..=rows - h)
                .flat_map(|row| (0..=cols - w).map(move |col| Rect { col, row, w, h }))
                .find(|r| cells.free(i, r))
        });
        match spot {
            Some(r) => {
                cells.paint(&r, i);
                placed[i] = Some(r);
            }
            None => failed.push(i),
        }
    }
    (cells, placed, failed)
}

/// Everyone at their preferred size if that fits; when it doesn't, the same again with the
/// sizes held down further and further until the widgets that can fit do. The last value is
/// the scale it took.
fn cascade(
    items: &[Item],
    order: &[usize],
    walls: Grid,
) -> (Cells, Vec<Option<Rect>>, Vec<usize>, f32) {
    let mut last = None;
    for scale in SCALES {
        let (cells, placed, failed) = first_fit(items, order, walls, scale);
        let done = failed.is_empty();
        last = Some((cells, placed, failed, scale));
        if done {
            break;
        }
    }
    last.expect("SCALES is not empty")
}

fn gravity(
    items: &[Item],
    order: &[usize],
    walls: Grid,
    p: &Params,
    mut dropped: Vec<usize>,
) -> Tried {
    let (mut cells, mut rects, failed, _) = cascade(items, order, walls);
    dropped.extend(failed);
    let free = |cells: &Cells, me: usize, r: &Rect| cells.free(me, r);
    // Grow into the holes, the ones furthest below their preferred size first.
    loop {
        let mut grew = false;
        let mut by_need: Vec<usize> = order
            .iter()
            .copied()
            .filter(|&i| rects[i].is_some())
            .collect();
        by_need.sort_by(|&a, &b| {
            let ra = rects[a].map_or(1.0, |r| (r.w * r.h) as f32 / items[a].pref_area());
            let rb = rects[b].map_or(1.0, |r| (r.w * r.h) as f32 / items[b].pref_area());
            ra.total_cmp(&rb)
        });
        for i in by_need {
            let Some(r) = rects[i] else { continue };
            let it = &items[i];
            let (xw, xh) = it.max();
            let cap = (it.pref_area() * p.max_grow) as i32;
            let tries: Vec<Rect> = if it.round {
                let s = r.w + 1;
                [(0, 0), (-1, 0), (0, -1), (-1, -1)]
                    .into_iter()
                    .map(|(dc, dr)| Rect {
                        col: r.col + dc,
                        row: r.row + dr,
                        w: s,
                        h: s,
                    })
                    .collect()
            } else {
                vec![
                    Rect { w: r.w + 1, ..r },
                    Rect { h: r.h + 1, ..r },
                    Rect {
                        col: r.col - 1,
                        w: r.w + 1,
                        ..r
                    },
                    Rect {
                        row: r.row - 1,
                        h: r.h + 1,
                        ..r
                    },
                ]
            };
            for t in tries {
                let ok = t.w * t.h <= cap.max(r.w * r.h)
                    && t.w <= xw.max(r.w)
                    && t.h <= xh.max(r.h)
                    && free(&cells, i, &t);
                if ok {
                    cells.paint(&r, Cells::FREE);
                    cells.paint(&t, i);
                    rects[i] = Some(t);
                    grew = true;
                    break;
                }
            }
        }
        if !grew {
            break;
        }
    }
    let placed = order
        .iter()
        .filter_map(|&i| rects[i].map(|r| (i, r)))
        .collect();
    (placed, dropped)
}

// ------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Shape;

    fn item(id: u32, kind: Kind, round: bool) -> Item {
        Item {
            id,
            kind,
            round,
            at: Rect {
                col: 0,
                row: 0,
                w: 1,
                h: 1,
            },
        }
    }

    /// The default dashboard's widgets.
    fn default_set() -> Vec<Item> {
        let (l, _) = Layout::parse(&Layout::default_text()).unwrap();
        l.widgets
            .iter()
            .map(|w| Item {
                id: w.id,
                kind: w.kind,
                round: w.shape == Shape::Round,
                at: w.rect(),
            })
            .collect()
    }

    fn every_kind() -> Vec<Item> {
        Kind::iter()
            .enumerate()
            .map(|(i, k)| item(i as u32 + 1, k, false))
            .collect()
    }

    fn picture(a: &Arrangement, cols: i32, rows: i32) -> String {
        let mut g = vec![vec!['.'; cols as usize]; rows as usize];
        for (n, (_, r)) in a.rects.iter().enumerate() {
            let ch = (b'A' + (n % 26) as u8) as char;
            for y in r.row..r.row + r.h {
                for x in r.col..r.col + r.w {
                    g[y as usize][x as usize] = ch;
                }
            }
        }
        g.iter()
            .map(|r| r.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The rules every layout keeps, whatever the algorithm.
    fn check(items: &[Item], a: &Arrangement, cols: i32, rows: i32, what: &str) {
        let mut taken = vec![false; (cols * rows) as usize];
        for (id, r) in &a.rects {
            let it = items.iter().find(|i| i.id == *id).unwrap();
            assert!(
                r.col >= 0 && r.row >= 0 && r.col + r.w <= cols && r.row + r.h <= rows,
                "{what}: {id} {r:?} outside"
            );
            let (mw, mh) = it.min();
            assert!(
                r.w >= mw && r.h >= mh,
                "{what}: {:?} {r:?} below its minimum",
                it.kind
            );
            if it.round {
                assert_eq!(r.w, r.h, "{what}: {:?} not square", it.kind);
            }
            for y in r.row..r.row + r.h {
                for x in r.col..r.col + r.w {
                    assert!(
                        !taken[(y * cols + x) as usize],
                        "{what}: overlap at {x},{y}"
                    );
                    taken[(y * cols + x) as usize] = true;
                }
            }
        }
        assert_eq!(
            a.rects.len() + a.unplaced.len(),
            items.len(),
            "{what}: lost widgets"
        );
    }

    #[test]
    fn every_algorithm_keeps_the_rules() {
        for algo in Algo::iter() {
            for order in Order::iter() {
                for (cols, rows) in [(16, 9), (12, 7), (24, 13), (7, 14), (6, 4), (3, 3)] {
                    for items in [default_set(), every_kind()] {
                        let p = Params {
                            algo,
                            order,
                            ..Params::default()
                        };
                        let a = solve(&items, &[], cols, rows, &p);
                        check(
                            &items,
                            &a,
                            cols,
                            rows,
                            &format!("{algo:?}/{order:?} on {cols}x{rows}"),
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn guillotine_fills_the_grid_when_it_places_everything() {
        for (cols, rows) in [(16, 9), (12, 7), (24, 13), (7, 14)] {
            let items = default_set();
            let a = solve(&items, &[], cols, rows, &Params::default());
            assert!(a.unplaced.is_empty(), "{cols}x{rows}: {:?}", a.unplaced);
            // Round widgets leave their corners empty; with none, the grid is full.
            let rects: i32 = a.rects.iter().map(|(_, r)| r.w * r.h).sum();
            let rounds: i32 = a
                .rects
                .iter()
                .filter(|(id, _)| items.iter().any(|i| i.id == *id && i.round))
                .map(|_| 1)
                .sum();
            if rounds == 0 {
                assert_eq!(
                    rects,
                    cols * rows,
                    "{cols}x{rows} not full\n{}",
                    picture(&a, cols, rows)
                );
            }
        }
    }

    #[test]
    fn guillotine_beats_the_others_on_cost() {
        let items = default_set();
        let cost = |algo| {
            solve(
                &items,
                &[],
                16,
                9,
                &Params {
                    algo,
                    ..Params::default()
                },
            )
            .cost
        };
        assert!(cost(Algo::Guillotine) <= cost(Algo::Treemap) + 1e-4);
        assert!(cost(Algo::Guillotine) <= cost(Algo::Gravity) + 1e-4);
    }

    #[test]
    fn same_parameters_same_layout_and_a_seed_changes_it() {
        let items = default_set();
        let p = Params::default();
        let rects = |p: &Params| solve(&items, &[], 16, 9, p).rects;
        assert_eq!(rects(&p), rects(&p));
        let seeds: std::collections::HashSet<String> = (1..8)
            .map(|seed| format!("{:?}", rects(&Params { seed, ..p.clone() })))
            .collect();
        assert!(seeds.len() > 3, "seeds barely differ: {}", seeds.len());
    }

    #[test]
    fn more_tries_never_cost_more() {
        let items = default_set();
        let one = solve(
            &items,
            &[],
            16,
            9,
            &Params {
                seed: 1,
                restarts: 1,
                ..Params::default()
            },
        );
        let many = solve(
            &items,
            &[],
            16,
            9,
            &Params {
                seed: 1,
                restarts: 8,
                ..Params::default()
            },
        );
        assert!(
            many.cost <= one.cost + 1e-4,
            "{} vs {}",
            many.cost,
            one.cost
        );
        assert_eq!(many.tries, 8);
    }

    #[test]
    fn no_room_drops_from_the_end_of_the_order() {
        let items = every_kind();
        let a = solve(&items, &[], 5, 4, &Params::default());
        assert!(!a.unplaced.is_empty());
        check(&items, &a, 5, 4, "5x4");
    }

    #[test]
    fn round_widgets_are_squares_in_the_middle_of_their_region() {
        let items = vec![item(1, Kind::Volume, true), item(2, Kind::Clock, false)];
        for algo in Algo::iter() {
            let a = solve(
                &items,
                &[],
                8,
                4,
                &Params {
                    algo,
                    ..Params::default()
                },
            );
            check(&items, &a, 8, 4, &format!("{algo:?}"));
        }
    }

    #[test]
    fn a_huge_dashboard_falls_back_instead_of_hanging() {
        let items: Vec<Item> = (0..40)
            .map(|i| item(i + 1, Kind::iter().nth(i as usize % 31).unwrap(), false))
            .collect();
        let a = solve(&items, &[], 48, 32, &Params::default());
        check(&items, &a, 48, 32, "40 widgets on 48x32");
        assert!(a.millis < 5000, "{} ms", a.millis);
    }

    #[test]
    fn params_round_trip_through_kdl() {
        let p = Params {
            algo: Algo::Treemap,
            order: Order::Area,
            seed: 7,
            restarts: 8,
            jitter: 0.5,
            shrink: 3.25,
            cut_bias: 1.2,
            affinity: 2.5,
            reach: 2.25,
            placement: EdgeMode::Center,
            edge: 0.75,
            balance: 0.3,
            effort: 2.0,
            ..Params::default()
        };
        let doc = kdl::KdlDocument::parse(&p.to_kdl()).unwrap();
        let mut warnings = Vec::new();
        let back = Params::from_node(doc.get("autolayout").unwrap(), &mut warnings);
        assert_eq!(back, p);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn bad_params_are_said_and_ignored() {
        let doc =
            kdl::KdlDocument::parse(r#"autolayout algo="nope" seed=5000 wibble=1 shrink="x""#)
                .unwrap();
        let mut warnings = Vec::new();
        let p = Params::from_node(doc.get("autolayout").unwrap(), &mut warnings);
        assert_eq!(p, Params::default());
        assert_eq!(warnings.len(), 4, "{warnings:?}");
    }

    #[test]
    fn steps_stay_in_range_and_reset() {
        let mut p = Params::default();
        for k in Knob::of(Algo::Treemap)
            .into_iter()
            .chain(Knob::of(Algo::Gravity))
            .chain(Knob::of(Algo::Neighbours))
        {
            for _ in 0..1100 {
                p.step(k, 1);
            }
            assert!(
                !p.can_step(k, 1) || matches!(k, Knob::Algo | Knob::Order | Knob::Placement),
                "{k:?}"
            );
            for _ in 0..1100 {
                p.step(k, -1);
            }
            assert!(
                !p.can_step(k, -1) || matches!(k, Knob::Algo | Knob::Order | Knob::Placement),
                "{k:?}"
            );
            p.reset(k);
            assert!(p.is_default(k), "{k:?} {}", p.text(k));
        }
    }

    /// Walls: the rectangles the algorithms must leave alone, in the corner, in the middle, and
    /// across the grid.
    fn walls() -> Vec<(i32, i32, Vec<Rect>)> {
        let r = |col, row, w, h| Rect { col, row, w, h };
        vec![
            (16, 9, vec![r(0, 0, 4, 2)]),
            (16, 9, vec![r(6, 3, 4, 3)]),
            (16, 9, vec![r(0, 0, 4, 2), r(12, 7, 4, 2)]),
            (16, 9, vec![r(7, 0, 2, 9)]),
            (24, 13, vec![r(0, 11, 24, 2), r(10, 0, 4, 4)]),
            (12, 7, vec![r(0, 0, 3, 3), r(9, 4, 3, 3)]),
        ]
    }

    #[test]
    fn pinned_widgets_are_walls_for_every_algorithm() {
        for (cols, rows, pins) in walls() {
            for algo in Algo::iter() {
                for items in [default_set(), every_kind()] {
                    let p = Params {
                        algo,
                        ..Params::default()
                    };
                    let a = solve(&items, &pins, cols, rows, &p);
                    let what = format!("{algo:?} on {cols}x{rows} around {pins:?}");
                    // The rules, with the walls as widgets of their own.
                    let mut taken = vec![false; (cols * rows) as usize];
                    for w in &pins {
                        for y in w.row..w.row + w.h {
                            for x in w.col..w.col + w.w {
                                taken[(y * cols + x) as usize] = true;
                            }
                        }
                    }
                    for (id, r) in &a.rects {
                        let it = items.iter().find(|i| i.id == *id).unwrap();
                        assert!(
                            r.col >= 0 && r.row >= 0 && r.col + r.w <= cols && r.row + r.h <= rows,
                            "{what}: outside"
                        );
                        let (mw, mh) = it.min();
                        assert!(r.w >= mw && r.h >= mh, "{what}: {:?} too small", it.kind);
                        for y in r.row..r.row + r.h {
                            for x in r.col..r.col + r.w {
                                assert!(
                                    !taken[(y * cols + x) as usize],
                                    "{what}: {:?} on a wall or another widget at {x},{y}",
                                    it.kind
                                );
                                taken[(y * cols + x) as usize] = true;
                            }
                        }
                    }
                    assert_eq!(
                        a.rects.len() + a.unplaced.len(),
                        items.len(),
                        "{what}: lost widgets"
                    );
                    assert_eq!(a.pinned, pins.len());
                }
            }
        }
    }

    #[test]
    fn guillotine_still_fills_everything_around_a_wall() {
        // Nothing round: then nothing may be left empty either.
        let items: Vec<Item> = default_set().into_iter().filter(|i| !i.round).collect();
        for (cols, rows, pins) in walls() {
            let a = solve(&items, &pins, cols, rows, &Params::default());
            if !a.unplaced.is_empty() {
                continue; // no room for all of them
            }
            assert!(
                (a.fill - 1.0).abs() < 1e-4,
                "{cols}x{rows} around {pins:?}: {:.2} filled\n{}",
                a.fill,
                picture(&a, cols, rows)
            );
        }
    }

    #[test]
    fn a_wall_that_leaves_no_room_drops_widgets_instead_of_failing() {
        let wall = [Rect {
            col: 0,
            row: 0,
            w: 15,
            h: 9,
        }];
        let items = default_set();
        for algo in Algo::iter() {
            let a = solve(
                &items,
                &wall,
                16,
                9,
                &Params {
                    algo,
                    ..Params::default()
                },
            );
            assert!(a.rects.len() < items.len(), "{algo:?}");
            assert_eq!(a.rects.len() + a.unplaced.len(), items.len());
        }
    }

    #[test]
    fn arranging_a_layout_keeps_its_pinned_widgets_put() {
        let (mut l, _) = Layout::parse(&Layout::default_text()).unwrap();
        l.set_screen_grid(16, 9);
        let clock = l.widgets.iter().find(|w| w.kind == Kind::Clock).unwrap().id;
        let notes = l
            .widgets
            .iter()
            .find(|w| w.kind == Kind::Notifications)
            .unwrap()
            .id;
        l.toggle_pin(clock);
        l.toggle_pin(notes);
        let (before_clock, before_notes) =
            (l.get(clock).unwrap().rect(), l.get(notes).unwrap().rect());
        for algo in Algo::iter() {
            for seed in 0..4 {
                let mut m = l.clone();
                let a = arrange(
                    &mut m,
                    &Params {
                        algo,
                        seed,
                        ..Params::default()
                    },
                );
                assert_eq!(
                    m.get(clock).unwrap().rect(),
                    before_clock,
                    "{algo:?} {seed}"
                );
                assert_eq!(
                    m.get(notes).unwrap().rect(),
                    before_notes,
                    "{algo:?} {seed}"
                );
                assert_eq!(a.pinned, 2);
                assert_eq!(
                    m.visible().len() + a.unplaced.len(),
                    m.widgets.len(),
                    "{algo:?} {seed}: overlap or lost"
                );
            }
        }
    }

    /// Gravity used to give up on widgets that would fit once the early ones took less.
    #[test]
    fn gravity_drops_nothing_that_fits() {
        for items in [default_set(), every_kind()] {
            for seed in 1..=100 {
                let a = solve(
                    &items,
                    &[],
                    16,
                    9,
                    &Params {
                        algo: Algo::Gravity,
                        seed,
                        ..Params::default()
                    },
                );
                assert!(
                    a.unplaced.is_empty(),
                    "{} widgets, seed {seed}: dropped {:?}",
                    items.len(),
                    a.unplaced
                );
            }
        }
    }

    fn neighbours(seed: u32) -> Params {
        Params {
            algo: Algo::Neighbours,
            seed,
            ..Params::default()
        }
    }

    fn at(items: &[Item], a: &Arrangement, k: Kind) -> Option<Rect> {
        let id = items.iter().find(|i| i.kind == k)?.id;
        a.rects.iter().find(|(i, _)| *i == id).map(|(_, r)| *r)
    }

    fn side_by_side(a: &Rect, b: &Rect) -> bool {
        let ox = (a.col + a.w).min(b.col + b.w) - a.col.max(b.col);
        let oy = (a.row + a.h).min(b.row + b.h) - a.row.max(b.row);
        (ox == 0 && oy > 0) || (oy == 0 && ox > 0)
    }

    #[test]
    fn neighbours_places_everything_around_pins() {
        let items = default_set();
        let pins = [Rect {
            col: 6,
            row: 2,
            w: 4,
            h: 3,
        }];
        for seed in 0..20 {
            let a = solve(&items, &pins, 16, 9, &neighbours(seed));
            assert!(a.unplaced.is_empty(), "seed {seed}: {:?}", a.unplaced);
            check(&items, &a, 16, 9, &format!("neighbours seed {seed}"));
            for (_, r) in &a.rects {
                assert!(!r.overlaps(&pins[0]), "seed {seed}: {r:?} on the pin");
            }
        }
    }

    #[test]
    fn neighbours_is_repeatable() {
        let items = every_kind();
        let a = solve(&items, &[], 16, 9, &neighbours(3));
        let b = solve(&items, &[], 16, 9, &neighbours(3));
        assert_eq!(a.rects, b.rects);
        let c = solve(&items, &[], 16, 9, &neighbours(4));
        assert_ne!(a.rects, c.rects);
    }

    /// Widgets are as big as they like to be, no smaller, when there is room.
    #[test]
    fn neighbours_keeps_the_preferred_size_as_a_floor() {
        let items = default_set();
        for seed in 0..20 {
            let a = solve(&items, &[], 24, 13, &neighbours(seed));
            for (id, r) in &a.rects {
                let it = items.iter().find(|i| i.id == *id).unwrap();
                let (pw, ph) = it.pref();
                assert!(
                    r.w >= pw && r.h >= ph,
                    "seed {seed}: {:?} is {r:?}",
                    it.kind
                );
            }
        }
    }

    #[test]
    fn neighbours_puts_the_liked_side_by_side_and_the_disliked_apart() {
        let items = every_kind();
        let pairs = [
            (Kind::Volume, Kind::Brightness),
            (Kind::Wifi, Kind::Bluetooth),
            (Kind::Clock, Kind::Calendar),
        ];
        let (mut hits, mut total, mut clashes) = (0, 0, 0);
        for seed in 1..=20 {
            let a = solve(&items, &[], 24, 13, &neighbours(seed));
            for (x, y) in pairs {
                total += 1;
                if side_by_side(&at(&items, &a, x).unwrap(), &at(&items, &a, y).unwrap()) {
                    hits += 1;
                }
            }
            if side_by_side(
                &at(&items, &a, Kind::Session).unwrap(),
                &at(&items, &a, Kind::Windows).unwrap(),
            ) {
                clashes += 1;
            }
        }
        assert!(hits * 10 >= total * 9, "liked pairs touch {hits}/{total}");
        assert!(clashes <= 2, "session and windows touch in {clashes}/20");
    }

    /// Better than a layout that doesn't know the matrix.
    #[test]
    fn neighbours_keeps_more_liked_pairs_together_than_gravity() {
        let items = default_set();
        let score = |p: &dyn Fn(u32) -> Params| -> usize {
            (1..=10)
                .map(|s| solve(&items, &[], 16, 9, &p(s)).likes.0)
                .sum()
        };
        let gravity = score(&|seed| Params {
            algo: Algo::Gravity,
            seed,
            ..Params::default()
        });
        let ours = score(&neighbours);
        assert!(ours > gravity, "neighbours {ours}, gravity {gravity}");
    }

    #[test]
    fn neighbours_leans_to_the_edges_or_the_middle() {
        let items = default_set();
        let border = |placement| -> i32 {
            (1..=8)
                .map(|seed| {
                    let a = solve(
                        &items,
                        &[],
                        24,
                        13,
                        &Params {
                            placement,
                            edge: 1.0,
                            ..neighbours(seed)
                        },
                    );
                    // Cells in the outer ring.
                    a.rects
                        .iter()
                        .map(|(_, r)| {
                            (r.row..r.row + r.h)
                                .flat_map(|y| (r.col..r.col + r.w).map(move |x| (x, y)))
                                .filter(|&(x, y)| x < 2 || y < 2 || x >= 22 || y >= 11)
                                .count() as i32
                        })
                        .sum::<i32>()
                })
                .sum()
        };
        let (edges, center) = (border(EdgeMode::Edges), border(EdgeMode::Center));
        assert!(edges > center * 2, "edges {edges}, center {center}");
    }

    #[test]
    fn the_affinity_order_chains_the_liked_together() {
        let items = every_kind();
        let p = Params {
            order: Order::Affinity,
            ..Params::default()
        };
        let o = ordered(&items, &p, 0);
        let pos = |k: Kind| o.iter().position(|&i| items[i].kind == k).unwrap();
        for (x, y) in [
            (Kind::Volume, Kind::Brightness),
            (Kind::Wifi, Kind::Bluetooth),
        ] {
            assert!(pos(x).abs_diff(pos(y)) <= 2, "{x:?} {y:?}: {o:?}");
        }
    }

    #[test]
    #[ignore]
    fn neighbours_gallery() {
        let items = default_set();
        for (edge, balance, placement) in [
            (0.25, 0.1, EdgeMode::Edges),
            (0.6, 0.1, EdgeMode::Edges),
            (1.0, 0.0, EdgeMode::Edges),
            (0.6, 0.1, EdgeMode::Center),
        ] {
            for seed in [1, 2] {
                let a = solve(
                    &items,
                    &[],
                    24,
                    13,
                    &Params {
                        algo: Algo::Neighbours,
                        seed,
                        edge,
                        balance,
                        placement,
                        ..Params::default()
                    },
                );
                println!(
                    "== edge {edge} balance {balance} {placement:?} seed {seed}: {}\n{}\n",
                    a.summary(),
                    picture(&a, 24, 13),
                );
            }
        }
    }

    /// `cargo test gallery -- --ignored --nocapture`: look at what each algorithm makes.
    #[test]
    #[ignore]
    fn gallery() {
        for (cols, rows) in [(16, 9), (12, 7), (24, 13), (7, 14)] {
            for items in [default_set(), every_kind()] {
                for algo in Algo::iter() {
                    let a = solve(
                        &items,
                        &[],
                        cols,
                        rows,
                        &Params {
                            algo,
                            ..Params::default()
                        },
                    );
                    println!(
                        "== {algo:?} {cols}x{rows} ({} widgets): {}\n{}\n",
                        items.len(),
                        a.summary(),
                        picture(&a, cols, rows)
                    );
                }
            }
        }
    }
}

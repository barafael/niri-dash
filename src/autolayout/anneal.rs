//! Neighbours: simulated annealing on where the widgets sit.
//!
//! Every widget starts at its preferred size in the first spot with room, then moves, swaps
//! places, jumps and grows one cell at a time. A move that makes the layout better is kept; one
//! that makes it worse is kept with a chance that shrinks as the search cools, which lets it
//! climb out of a bad arrangement early and settle late.
//!
//! "Better" is the energy of [`Energy`]: liked widgets near each other, disliked ones apart (the
//! matrix of [`crate::affinity`]), each widget near the size and shape it likes, a slight lean
//! towards the edges or the middle of the grid, and the whole not piled into one corner. A
//! widget never shrinks below its preferred size unless the grid is too full for that.
//! Nothing makes it grow, so empty space stays empty.

use super::{Cells, Grid, Item, Params, Rect, Rng, Tried, capped, cascade};
use crate::affinity::affinity;
use crate::autolayout::EdgeMode;

/// Moves per widget at effort 1.
const STEPS_PER_WIDGET: f32 = 4000.0;

/// What a layout costs, the pieces that can be told one widget at a time.
pub(super) struct Energy<'a> {
    items: &'a [Item],
    p: &'a Params,
    cols: i32,
    rows: i32,
    /// `like[i][j]`: how much widgets `i` and `j` like each other.
    like: Vec<Vec<f32>>,
    /// How many widgets are being placed.
    n: f32,
}

impl<'a> Energy<'a> {
    pub fn new(items: &'a [Item], grid: Grid, p: &'a Params) -> Self {
        let like = items
            .iter()
            .map(|a| items.iter().map(|b| affinity(a.kind, b.kind)).collect())
            .collect();
        Self {
            items,
            p,
            cols: grid.cols,
            rows: grid.rows,
            like,
            n: items.len().max(1) as f32,
        }
    }

    /// How near two rectangles are, 0 to 1: 1 for sides that touch along their whole length,
    /// less for a shorter shared border, less again for a gap or a corner. The gap and the
    /// reach make it fall off like `1 / (1 + (gap / reach)²)`.
    fn near(&self, a: &Rect, b: &Rect) -> f32 {
        let ox = (a.col + a.w).min(b.col + b.w) - a.col.max(b.col);
        let oy = (a.row + a.h).min(b.row + b.h) - a.row.max(b.row);
        let (gx, gy) = ((-ox).max(0), (-oy).max(0));
        // How much of the facing sides line up.
        let (facing, gap) = if gx == 0 && ox > 0 {
            (ox as f32 / a.w.min(b.w) as f32, gy)
        } else if gy == 0 && oy > 0 {
            (oy as f32 / a.h.min(b.h) as f32, gx)
        } else {
            // Corner to corner, or off to a side: no shared side at all.
            (0.25, (gx + gy).max(1))
        };
        let g = gap as f32 / self.p.reach;
        facing / (1.0 + g * g)
    }

    /// What liking or disliking costs between widgets `i` and `j`.
    fn pair(&self, i: usize, j: usize, a: &Rect, b: &Rect) -> f32 {
        -self.p.affinity * self.like[i][j] * self.near(a, b)
    }

    /// What it costs for widget `i` to be `r` on its own: its size and shape, and where it leans.
    fn own(&self, i: usize, r: &Rect) -> f32 {
        self.items[i].cost(r.w, r.h, self.p) + self.lean(r)
    }

    /// The pull to the edges or the middle: the mean over the widget's cells of how deep in the
    /// grid they are (0 at the border, 1 in the middle), or the opposite, times the strength.
    fn lean(&self, r: &Rect) -> f32 {
        if self.p.placement == EdgeMode::Off || self.p.edge <= 0.0 {
            return 0.0;
        }
        let mut sum = 0.0;
        for y in r.row..r.row + r.h {
            for x in r.col..r.col + r.w {
                sum += self.depth(x, y);
            }
        }
        let mean = sum / (r.w * r.h) as f32;
        self.p.edge
            * match self.p.placement {
                EdgeMode::Edges => mean,
                _ => 1.0 - mean,
            }
    }

    /// 0 on the border of the grid, 1 in its middle.
    fn depth(&self, x: i32, y: i32) -> f32 {
        let u = (x as f32 + 0.5) / self.cols as f32 * 2.0 - 1.0;
        let v = (y as f32 + 0.5) / self.rows as f32 * 2.0 - 1.0;
        1.0 - u.abs().max(v.abs())
    }

    /// What it costs for the widgets' centre of mass to be `(cx, cy)` (each -1 to 1 from the
    /// middle) rather than in the middle.
    fn off_center(&self, area: f32, sx: f32, sy: f32) -> f32 {
        if area <= 0.0 {
            return 0.0;
        }
        let ux = (sx / area) / (self.cols as f32 / 2.0) - 1.0;
        let uy = (sy / area) / (self.rows as f32 / 2.0) - 1.0;
        self.p.balance * self.n * (ux * ux + uy * uy)
    }

    /// The cost of everything about `moved` (their own, and between them and the rest), for
    /// rectangles as `at` gives them.
    fn part(&self, moved: &[usize], at: &impl Fn(usize) -> Option<Rect>) -> f32 {
        let mut e = 0.0;
        for (n, &i) in moved.iter().enumerate() {
            let Some(a) = at(i) else { continue };
            e += self.own(i, &a);
            for j in 0..self.items.len() {
                if j == i || moved[..n].contains(&j) {
                    continue;
                }
                if let Some(b) = at(j) {
                    e += self.pair(i, j, &a, &b);
                }
            }
        }
        e
    }

    /// The cost of a whole layout, `rects` indexed like the items.
    pub fn total(&self, rects: &[Option<Rect>]) -> f32 {
        let all: Vec<usize> = (0..self.items.len()).collect();
        let (mut area, mut sx, mut sy) = (0.0, 0.0, 0.0);
        for r in rects.iter().flatten() {
            let a = (r.w * r.h) as f32;
            area += a;
            sx += a * (r.col as f32 + r.w as f32 / 2.0);
            sy += a * (r.row as f32 + r.h as f32 / 2.0);
        }
        self.part(&all, &|i| rects[i]) + self.off_center(area, sx, sy)
    }
}

/// The running totals the centering term needs.
#[derive(Clone, Copy, Default)]
struct Mass {
    area: f32,
    sx: f32,
    sy: f32,
}

impl Mass {
    fn add(&mut self, r: &Rect, sign: f32) {
        let a = (r.w * r.h) as f32 * sign;
        self.area += a;
        self.sx += a * (r.col as f32 + r.w as f32 / 2.0);
        self.sy += a * (r.row as f32 + r.h as f32 / 2.0);
    }
}

/// How much liking is satisfied, for the line under the buttons: of the pairs that like each
/// other at least this much, how many touch; and how many of those that dislike each other do.
pub(super) fn tally(items: &[Item], rects: &[(usize, Rect)]) -> (usize, usize, usize) {
    let (mut liked, mut touching, mut disliked) = (0, 0, 0);
    for (n, &(i, a)) in rects.iter().enumerate() {
        for &(j, b) in &rects[n + 1..] {
            let v = affinity(items[i].kind, items[j].kind);
            let touch = touches(&a, &b);
            if v >= 0.5 {
                liked += 1;
                touching += usize::from(touch);
            } else if v <= -0.25 && touch {
                disliked += 1;
            }
        }
    }
    (touching, liked, disliked)
}

/// Whether two rectangles share a piece of side.
fn touches(a: &Rect, b: &Rect) -> bool {
    let ox = (a.col + a.w).min(b.col + b.w) - a.col.max(b.col);
    let oy = (a.row + a.h).min(b.row + b.h) - a.row.max(b.row);
    (ox == 0 && oy > 0) || (oy == 0 && ox > 0)
}

/// The cost of a layout `placed` under `p`, for choosing between tries.
pub(super) fn energy(items: &[Item], placed: &[(usize, Rect)], grid: Grid, p: &Params) -> f32 {
    let e = Energy::new(items, grid, p);
    let mut rects = vec![None; items.len()];
    for &(i, r) in placed {
        rects[i] = Some(r);
    }
    e.total(&rects) / placed.len().max(1) as f32
}

/// One search. `order` is the widgets' order (the start is first fit along it) and `seed`
/// makes the search repeat itself. Widgets that find no room even at their smallest size are
/// added to `dropped`.
pub(super) fn anneal(
    items: &[Item],
    order: &[usize],
    walls: Grid,
    p: &Params,
    seed: u32,
    mut dropped: Vec<usize>,
) -> Tried {
    let (cols, rows) = (walls.cols, walls.rows);
    let (mut cells, mut rects, failed, scale) = cascade(items, order, walls);
    dropped.extend(failed);
    let active: Vec<usize> = order
        .iter()
        .copied()
        .filter(|&i| rects[i].is_some())
        .collect();
    if active.len() < 2 {
        return (collect(order, &rects), dropped);
    }
    // No smaller than the size it got to begin with, or its preferred size if it got that.
    let floor: Vec<(i32, i32)> = (0..items.len())
        .map(|i| match rects[i] {
            Some(r) => {
                let (cw, ch) = capped(&items[i], scale);
                (cw.min(r.w), ch.min(r.h))
            }
            None => (0, 0),
        })
        .collect();

    let e = Energy::new(items, walls, p);
    let mut rng = Rng(u64::from(seed).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x1234_5678_9ABC_DEF1);
    let mut mass = Mass::default();
    for r in rects.iter().flatten() {
        mass.add(r, 1.0);
    }

    // A proposal: new rectangles for one or two widgets.
    let propose = |rects: &[Option<Rect>], cells: &Cells, radius: i32, rng: &mut Rng| {
        propose_move(
            items,
            &active,
            &floor,
            rects,
            cells,
            (cols, rows),
            radius,
            rng,
        )
    };

    // How warm to start: such that a typical move for the worse is taken more often than not.
    let mut uphill = Vec::new();
    for _ in 0..300 {
        if let Some(m) = propose(&rects, &cells, cols.max(rows) / 2, &mut rng) {
            let d = delta(&e, &rects, &mass, &m);
            if d > 0.0 {
                uphill.push(d);
            }
        }
    }
    let t0 = if uphill.is_empty() {
        1.0
    } else {
        1.5 * uphill.iter().sum::<f32>() / uphill.len() as f32
    };
    let t_end = t0 * 0.002;
    let steps = ((STEPS_PER_WIDGET * p.effort * active.len() as f32) as usize).max(2000);
    let cooling = (t_end / t0).powf(1.0 / steps as f32);

    let mut t = t0;
    for step in 0..steps {
        // The reach of a shift falls with the temperature: far at first, a cell at the end.
        let radius = 1 + ((cols.max(rows) / 2 - 1) as f32 * (t / t0)) as i32;
        // The last tenth only takes what is better.
        let cold = step > steps - steps / 10;
        t *= cooling;
        let Some(m) = propose(&rects, &cells, radius, &mut rng) else {
            continue;
        };
        let d = delta(&e, &rects, &mass, &m);
        let take = d <= 0.0 || (!cold && rng.unit() < (-d / t).exp());
        if take {
            apply(&mut rects, &mut cells, &mut mass, &m);
        }
    }
    (collect(order, &rects), dropped)
}

/// The widgets that got a place, in `order`.
fn collect(order: &[usize], rects: &[Option<Rect>]) -> Vec<(usize, Rect)> {
    order
        .iter()
        .filter_map(|&i| rects[i].map(|r| (i, r)))
        .collect()
}

/// New places for up to two widgets.
type Move = [Option<(usize, Rect)>; 2];

/// A random move that is legal, or none (it ran into something).
#[allow(clippy::too_many_arguments)]
fn propose_move(
    items: &[Item],
    active: &[usize],
    floor: &[(i32, i32)],
    rects: &[Option<Rect>],
    cells: &Cells,
    (cols, rows): (i32, i32),
    radius: i32,
    rng: &mut Rng,
) -> Option<Move> {
    let pick = |rng: &mut Rng, n: usize| (rng.next() % n as u64) as usize;
    let range =
        |rng: &mut Rng, lo: i32, hi: i32| lo + (rng.next() % (hi - lo + 1).max(1) as u64) as i32;
    let i = active[pick(rng, active.len())];
    let r = rects[i]?;
    let kind = rng.next() % 100;
    if kind < 45 {
        // Shift.
        let (dx, dy) = (range(rng, -radius, radius), range(rng, -radius, radius));
        if dx == 0 && dy == 0 {
            return None;
        }
        let to = Rect {
            col: r.col + dx,
            row: r.row + dy,
            ..r
        };
        cells.free(i, &to).then_some([Some((i, to)), None])
    } else if kind < 65 {
        // Jump anywhere.
        let to = Rect {
            col: range(rng, 0, cols - r.w),
            row: range(rng, 0, rows - r.h),
            ..r
        };
        (to != r && cells.free(i, &to)).then_some([Some((i, to)), None])
    } else if kind < 85 {
        // Swap places with another: each takes the other's middle.
        let k = active[pick(rng, active.len())];
        if k == i {
            return None;
        }
        let o = rects[k]?;
        let centered = |mine: &Rect, theirs: &Rect| Rect {
            col: (theirs.col + (theirs.w - mine.w) / 2).clamp(0, (cols - mine.w).max(0)),
            row: (theirs.row + (theirs.h - mine.h) / 2).clamp(0, (rows - mine.h).max(0)),
            ..*mine
        };
        let (ri, rk) = (centered(&r, &o), centered(&o, &r));
        // Each must have room where the other was, and they must not land on each other.
        let apart = ri.col + ri.w <= rk.col
            || rk.col + rk.w <= ri.col
            || ri.row + ri.h <= rk.row
            || rk.row + rk.h <= ri.row;
        (apart && cells.free_but(i, k, &ri) && cells.free_but(k, i, &rk))
            .then_some([Some((i, ri)), Some((k, rk))])
    } else {
        // Grow or shrink by a row or a column, from either side.
        let it = &items[i];
        let (xw, xh) = it.max();
        let grow = rng.next().is_multiple_of(2);
        let side = rng.next() % 4;
        let d = if grow { 1 } else { -1 };
        let to = if it.round {
            // Stays square: moves the corner opposite `side`.
            let s = r.w + d;
            Rect {
                col: if side.is_multiple_of(2) {
                    r.col
                } else {
                    r.col - d
                },
                row: if side / 2 == 0 { r.row } else { r.row - d },
                w: s,
                h: s,
            }
        } else {
            match side {
                0 => Rect { w: r.w + d, ..r },
                1 => Rect { h: r.h + d, ..r },
                2 => Rect {
                    col: r.col - d,
                    w: r.w + d,
                    ..r
                },
                _ => Rect {
                    row: r.row - d,
                    h: r.h + d,
                    ..r
                },
            }
        };
        let (fw, fh) = floor[i];
        let ok =
            to.w >= fw.max(1) && to.h >= fh.max(1) && to.w <= xw.max(r.w) && to.h <= xh.max(r.h);
        (ok && cells.free(i, &to)).then_some([Some((i, to)), None])
    }
}

/// How the energy changes if `m` is made.
fn delta(e: &Energy, rects: &[Option<Rect>], mass: &Mass, m: &Move) -> f32 {
    let moved: Vec<usize> = m.iter().flatten().map(|&(i, _)| i).collect();
    let before = e.part(&moved, &|i| rects[i]);
    let after = e.part(&moved, &|i| {
        m.iter()
            .flatten()
            .find(|&&(k, _)| k == i)
            .map(|&(_, r)| r)
            .or(rects[i])
    });
    let mut next = *mass;
    for &(i, r) in m.iter().flatten() {
        if let Some(old) = rects[i] {
            next.add(&old, -1.0);
        }
        next.add(&r, 1.0);
    }
    after - before + e.off_center(next.area, next.sx, next.sy)
        - e.off_center(mass.area, mass.sx, mass.sy)
}

fn apply(rects: &mut [Option<Rect>], cells: &mut Cells, mass: &mut Mass, m: &Move) {
    for &(i, _) in m.iter().flatten() {
        if let Some(old) = rects[i] {
            cells.paint(&old, Cells::FREE);
            mass.add(&old, -1.0);
        }
    }
    for &(i, r) in m.iter().flatten() {
        cells.paint(&r, i);
        mass.add(&r, 1.0);
        rects[i] = Some(r);
    }
}

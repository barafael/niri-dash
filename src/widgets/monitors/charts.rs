//! Charts made with GPUI's `canvas` and `PathBuilder`: the history graph, rings, round gauges and
//! bars. Ports of `Graph`, `Ring`, `RoundGauge` and `Bar` of `legacy/ui/common.slint`.
//!
//! The geometry (which samples are on screen, where the line runs, what the rounded corners cut
//! off) is plain functions on numbers, tested below; the elements only paint it.
//! Other groups may copy these, or move them to `kit` once two widgets use them.

use crate::kit::Tk;
use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, Div, ElementId, Hsla, PathBuilder, Pixels, Point, Rgba, Role, SharedString,
    Stateful, canvas, div, fill, point, px, quad, relative, size,
};

type P = (f32, f32);

fn pt(p: P) -> Point<Pixels> {
    point(px(p.0), px(p.1))
}

fn finite(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

/// A reading as a fraction: 0 to 1, whatever the source said.
pub fn unit(v: f32) -> f32 {
    finite(v).clamp(0.0, 1.0)
}

/// A fraction as the widgets print it: "42%".
pub fn percent(v: f32) -> String {
    format!("{}%", (finite(v) * 100.0).round() as i64)
}

/// `a` of the way from `b` to `a`: Slint's `a.mix(b, f)` (`f` of `a`, the rest of `b`).
pub fn mix(a: Hsla, b: Hsla, f: f32) -> Hsla {
    let (a, b) = (Rgba::from(a), Rgba::from(b));
    let m = |x: f32, y: f32| x * f + y * (1.0 - f);
    Rgba {
        r: m(a.r, b.r),
        g: m(a.g, b.g),
        b: m(a.b, b.b),
        a: m(a.a, b.a),
    }
    .into()
}

// ---- the history line ----------------------------------------------------------------------

/// Where the line of `values` runs in a graph of `w` by `h` pixels that is `capacity` samples
/// wide: the newest sample at the right edge, one step (`w / (capacity - 1)`) per sample, a
/// value of 0 two pixels above the bottom and 1 two pixels below the top. Samples older than the
/// capacity are off the left edge (the line starts at the oldest one that is on screen). Fewer
/// than two samples draw nothing.
pub fn graph_points(values: &[f32], capacity: i64, w: f32, h: f32) -> Vec<P> {
    let n = values.len() as i64;
    if n < 2 || w <= 0.0 {
        return Vec::new();
    }
    let step = w / (capacity - 1).max(1) as f32;
    let first = capacity - n;
    let x = |i: i64| (first + i) as f32 * step;
    let y = |v: f32| 2.0 + (1.0 - unit(v)) * (h - 4.0).max(0.0);
    // Samples at x < 0 are off screen; the one at x = 0 is the oldest shown.
    let s = (-first).max(0);
    if n - s < 2 {
        return Vec::new();
    }
    (s..n).map(|i| (x(i), y(values[i as usize]))).collect()
}

/// A convex polygon to cut shapes to (a graph's rounded corners).
pub struct Clip {
    edges: Vec<(P, P)>,
    /// Which way round it is, so that "inside" is the same side of every edge.
    sign: f32,
}

fn side(a: P, b: P, p: P) -> f32 {
    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
}

impl Clip {
    pub fn new(poly: &[P]) -> Self {
        let n = poly.len();
        let area: f32 = (0..n)
            .map(|i| {
                let (a, b) = (poly[i], poly[(i + 1) % n]);
                a.0 * b.1 - b.0 * a.1
            })
            .sum();
        Self {
            edges: (0..n).map(|i| (poly[i], poly[(i + 1) % n])).collect(),
            sign: if area < 0.0 { -1.0 } else { 1.0 },
        }
    }

    /// A `w` by `h` rectangle with corners of radius `r`.
    pub fn rounded(w: f32, h: f32, r: f32) -> Self {
        let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
        let steps = 6;
        let mut poly = Vec::new();
        // Top left, top right, bottom right, bottom left: round the screen clockwise.
        let corners = [
            (r, r, 180.0),
            (w - r, r, 270.0),
            (w - r, h - r, 0.0),
            (r, h - r, 90.0),
        ];
        for (cx, cy, from) in corners {
            for k in 0..=steps {
                let a = (from + 90.0 * k as f32 / steps as f32).to_radians();
                poly.push((cx + r * a.cos(), cy + r * a.sin()));
            }
        }
        Self::new(&poly)
    }

    fn inside(&self, a: P, b: P, p: P) -> bool {
        side(a, b, p) * self.sign >= 0.0
    }

    /// The part of the polygon `subject` inside (Sutherland-Hodgman).
    pub fn polygon(&self, subject: &[P]) -> Vec<P> {
        let mut out = subject.to_vec();
        for &(a, b) in &self.edges {
            let input = std::mem::take(&mut out);
            let Some(&last) = input.last() else { break };
            let mut s = last;
            for &e in &input {
                match (self.inside(a, b, e), self.inside(a, b, s)) {
                    (true, was_in) => {
                        if !was_in {
                            out.push(cross(s, e, a, b));
                        }
                        out.push(e);
                    }
                    (false, true) => out.push(cross(s, e, a, b)),
                    (false, false) => {}
                }
                s = e;
            }
        }
        out
    }

    /// The part of the segment `p` to `q` inside, as the range of the way along it (Cyrus-Beck).
    fn segment(&self, p: P, q: P) -> Option<(f32, f32)> {
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        for &(a, b) in &self.edges {
            let (dp, dq) = (side(a, b, p) * self.sign, side(a, b, q) * self.sign);
            if dp < 0.0 && dq < 0.0 {
                return None;
            }
            if dp < 0.0 {
                t0 = t0.max(dp / (dp - dq));
            } else if dq < 0.0 {
                t1 = t1.min(dp / (dp - dq));
            }
        }
        (t0 <= t1).then_some((t0, t1))
    }

    /// The parts of the polyline inside, each in one piece.
    pub fn polyline(&self, line: &[P]) -> Vec<Vec<P>> {
        let mut parts: Vec<Vec<P>> = Vec::new();
        let mut current: Vec<P> = Vec::new();
        for w in line.windows(2) {
            let (p, q) = (w[0], w[1]);
            let Some((t0, t1)) = self.segment(p, q) else {
                if current.len() > 1 {
                    parts.push(std::mem::take(&mut current));
                }
                current.clear();
                continue;
            };
            let at = |t: f32| (p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t);
            if t0 > 1e-4 || current.is_empty() {
                if current.len() > 1 {
                    parts.push(std::mem::take(&mut current));
                }
                current = vec![at(t0)];
            }
            current.push(at(t1));
            if t1 < 1.0 - 1e-4 {
                parts.push(std::mem::take(&mut current));
            }
        }
        if current.len() > 1 {
            parts.push(current);
        }
        parts
    }
}

/// Where the segment `p` to `q` crosses the line through `a` and `b`.
fn cross(p: P, q: P, a: P, b: P) -> P {
    let (dp, dq) = (side(a, b, p), side(a, b, q));
    let t = if dp == dq { 0.0 } else { dp / (dp - dq) };
    (p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t)
}

/// A history line chart, newest sample on the right, `capacity` samples wide.
pub struct Graph<'a> {
    pub label: &'static str,
    pub values: &'a [f32],
    pub capacity: i64,
    pub tint: Hsla,
    /// Off for a second line drawn on top of another graph.
    pub fill: bool,
    pub background: bool,
}

impl<'a> Graph<'a> {
    pub fn new(label: &'static str, values: &'a [f32], capacity: i64, tint: Hsla) -> Self {
        Self {
            label,
            values,
            capacity,
            tint,
            fill: true,
            background: true,
        }
    }

    /// Just the line, on nothing: for drawing over another graph.
    pub fn overlay(mut self) -> Self {
        self.fill = false;
        self.background = false;
        self
    }

    /// The graph, `size_full`; the caller says how much room it takes in its layout.
    pub fn element(self, id: impl Into<ElementId>, tk: &Tk) -> Stateful<Div> {
        let radius = tk.radius_small().min(8.0);
        let Graph {
            label,
            values,
            capacity,
            tint,
            fill: fill_area,
            background,
        } = self;
        let values = values.to_vec();
        let area = mix(tint, tk.c(crate::theme::ColorRole::Card).hsla(), 0.18);
        let area = Hsla { a: 1.0, ..area };
        let plot = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                let (ox, oy) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
                let points = graph_points(&values, capacity, w, h);
                if points.len() < 2 {
                    return;
                }
                let clip = (radius > 0.0).then(|| Clip::rounded(w, h, radius));
                let at = |p: &P| pt((ox + p.0, oy + p.1));
                if fill_area {
                    let (first, last) = (points[0], points[points.len() - 1]);
                    let mut poly = points.clone();
                    poly.push((last.0, h));
                    poly.push((first.0, h));
                    let poly = match &clip {
                        Some(c) => c.polygon(&poly),
                        None => poly,
                    };
                    if poly.len() >= 3 {
                        let mut b = PathBuilder::fill();
                        b.move_to(at(&poly[0]));
                        for p in &poly[1..] {
                            b.line_to(at(p));
                        }
                        b.close();
                        if let Ok(path) = b.build() {
                            window.paint_path(path, area);
                        }
                    }
                }
                let lines = match &clip {
                    Some(c) => c.polyline(&points),
                    None => vec![points],
                };
                for line in lines {
                    let mut b = PathBuilder::stroke(px(1.6));
                    b.move_to(at(&line[0]));
                    for p in &line[1..] {
                        b.line_to(at(p));
                    }
                    if let Ok(path) = b.build() {
                        window.paint_path(path, tint);
                    }
                }
            },
        )
        .size_full();
        div()
            .id(id)
            .role(Role::Image)
            .aria_label(label)
            .debug_selector(move || label.to_string())
            .w_full()
            .rounded(px(radius))
            .overflow_hidden()
            .when(background, |d| d.bg(tk.subtle()))
            .child(plot)
    }
}

// ---- rings, gauges, bars -------------------------------------------------------------------

/// A gauge for screen readers: its name, and where it stands as a percentage.
fn gauge_role(base: Stateful<Div>, label: SharedString, value: f32) -> Stateful<Div> {
    let pct = (unit(value) * 100.0).round();
    let named = label.to_string();
    base.role(Role::ProgressIndicator)
        .debug_selector(move || named)
        .aria_label(label)
        .aria_numeric_value(f64::from(pct))
        .aria_min_numeric_value(0.0)
        .aria_max_numeric_value(100.0)
        .aria_value(format!("{pct}%"))
}

/// The arc from 12 o'clock clockwise that `value` (0 to 1) of a ring covers: its end point.
pub fn arc_end(cx: f32, cy: f32, r: f32, v: f32) -> P {
    let a = v * std::f32::consts::TAU;
    (cx + r * a.sin(), cy - r * a.cos())
}

/// The radius of a ring's middle line in a `size` square, for a stroke `thickness` thick.
pub fn ring_radius(size: f32, thickness: f32) -> f32 {
    size / 2.0 - thickness / 2.0 - size * 0.005
}

/// An arc from 12 o'clock clockwise, `value` in 0 to 1, over a track; `size_full`.
pub fn ring(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    value: f32,
    thickness: f32,
    tint: Hsla,
    tk: &Tk,
) -> Stateful<Div> {
    let track = tk.track();
    let v = unit(value).min(0.9999);
    let plot = canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let size = f32::from(bounds.size.width.min(bounds.size.height));
            let c = bounds.center();
            let (cx, cy) = (f32::from(c.x), f32::from(c.y));
            let r = ring_radius(size, thickness);
            let outer = r + thickness / 2.0;
            let square = |r: f32| Bounds::centered_at(c, size_of(r));
            window.paint_quad(quad(
                square(outer),
                px(outer),
                gpui::transparent_black(),
                px(thickness.min(outer)),
                track,
                gpui::BorderStyle::default(),
            ));
            if v > 0.002 {
                let end = arc_end(cx, cy, r, v);
                let mut b = PathBuilder::stroke(px(thickness));
                b.move_to(point(px(cx), px(cy - r)));
                b.arc_to(point(px(r), px(r)), px(0.0), v > 0.5, true, pt(end));
                if let Ok(path) = b.build() {
                    window.paint_path(path, tint);
                }
                // Round ends.
                for p in [(cx, cy - r), end] {
                    let cap = Bounds::centered_at(pt(p), size_of(thickness / 2.0));
                    window.paint_quad(fill(cap, tint).corner_radii(px(thickness / 2.0)));
                }
            }
        },
    )
    .size_full();
    gauge_role(div().id(id), label.into(), value)
        .size_full()
        .child(plot)
}

fn size_of(radius: f32) -> gpui::Size<Pixels> {
    size(px(radius * 2.0), px(radius * 2.0))
}

/// A round widget's gauge: a ring (and an inner one for a second value) around a big figure and
/// a caption, or around the children.
pub struct Gauge {
    /// What the outer ring measures (for screen readers).
    pub label: &'static str,
    pub value: f32,
    pub tint: Hsla,
    /// The inner ring: what it measures, its value and color.
    pub inner: Option<(&'static str, f32, Hsla)>,
    /// Usage rather than progress: a nearly full outer ring shows in the bad color.
    pub high_is_bad: bool,
    pub big: String,
    pub caption: String,
}

impl Gauge {
    pub fn new(label: &'static str, value: f32, tint: Hsla) -> Self {
        Self {
            label,
            value,
            tint,
            inner: None,
            high_is_bad: true,
            big: String::new(),
            caption: String::new(),
        }
    }

    /// A gauge in a card of `width` pixels (and as high), with `children` under the figures.
    pub fn element(self, width: f32, tk: &Tk, children: Vec<AnyElement>) -> AnyElement {
        let outer_tint = if self.high_is_bad && self.value > 0.9 {
            tk.bad()
        } else {
            self.tint
        };
        let mut gauge = div()
            .relative()
            .size_full()
            .child(div().absolute().size_full().child(ring(
                ("ring", 0usize),
                self.label,
                self.value,
                (width * 0.06).max(6.0),
                outer_tint,
                tk,
            )));
        if let Some((label, value, tint)) = self.inner.filter(|(_, v, _)| *v >= 0.0) {
            gauge = gauge.child(
                div()
                    .absolute()
                    .top(relative(0.12))
                    .left(relative(0.12))
                    .w(relative(0.76))
                    .h(relative(0.76))
                    .child(ring(
                        ("ring", 1usize),
                        label,
                        value,
                        (width * 0.05).max(5.0),
                        tint,
                        tk,
                    )),
            );
        }
        let Gauge { big, caption, .. } = self;
        gauge
            .child(
                div()
                    .absolute()
                    .size_full()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .items_center()
                    .when(!big.is_empty(), |d| {
                        let shown = big.clone();
                        d.child(
                            div()
                                .w_full()
                                .text_center()
                                .text_size(px((width * 0.15).max(13.0)))
                                .font_weight(tk.display_weight())
                                .text_color(tk.text())
                                .debug_selector(move || shown.clone())
                                .child(big),
                        )
                    })
                    .when(!caption.is_empty(), |d| {
                        let shown = caption.clone();
                        d.child(
                            div()
                                .w_full()
                                .px(px(width * 0.08))
                                .text_center()
                                .text_size(px((width * 0.06).max(9.0)))
                                .text_color(tk.dim())
                                .truncate()
                                .debug_selector(move || shown.clone())
                                .child(caption),
                        )
                    })
                    .children(children),
            )
            .into_any_element()
    }
}

/// A thin progress bar, `value` in 0 to 1: nearly full shows in the bad color if `high_is_bad`.
pub fn bar(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    value: f32,
    tint: Hsla,
    high_is_bad: bool,
    tk: &Tk,
) -> Stateful<Div> {
    let v = unit(value);
    let color = if high_is_bad && v > 0.9 {
        tk.bad()
    } else {
        tint
    };
    gauge_role(div().id(id), label.into(), value)
        .w_full()
        .h(px(6.0))
        .flex_none()
        .rounded_full()
        .bg(tk.track())
        .overflow_hidden()
        .child(div().h_full().w(relative(v)).rounded_full().bg(color))
}

/// One bar per core, each as high as the core is busy: `h` pixels high, `gap` apart.
pub fn columns(values: &[f32], h: f32, gap: f32, tk: &Tk) -> Div {
    div()
        .w_full()
        .h(px(h))
        .flex_none()
        .flex()
        .flex_row()
        .gap(px(gap))
        .children(values.iter().map(|&c| {
            let c = unit(c);
            let color = if c > 0.85 {
                tk.bad()
            } else {
                Hsla {
                    a: tk.accent().a * 0.7,
                    ..tk.accent()
                }
            };
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .relative()
                .rounded(px(2.0))
                .bg(tk.track())
                .child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .w_full()
                        .h(relative(c))
                        .rounded(px(2.0))
                        .bg(color),
                )
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn percent_rounds_like_the_slint_widgets() {
        assert_eq!(percent(0.0), "0%");
        assert_eq!(percent(0.424), "42%");
        assert_eq!(percent(0.425), "43%");
        assert_eq!(percent(1.0), "100%");
        assert_eq!(percent(f32::NAN), "0%");
    }

    #[test]
    fn history_is_right_aligned() {
        // Three samples on a graph five samples wide, 40 px: steps of 10 px, the first two
        // slots empty.
        let p = graph_points(&[0.0, 0.5, 1.0], 5, 40.0, 24.0);
        assert_eq!(p.len(), 3);
        assert!(close(p[0].0, 20.0) && close(p[1].0, 30.0) && close(p[2].0, 40.0));
        // 0 is two pixels above the bottom, 1 two pixels below the top.
        assert!(close(p[0].1, 22.0) && close(p[1].1, 12.0) && close(p[2].1, 2.0));
    }

    #[test]
    fn old_samples_fall_off_the_left_edge() {
        // Six samples, room for four: the two oldest are off screen, the line starts at the
        // third, at x = 0, and ends at the newest, at the right edge.
        let p = graph_points(&[0.0, 0.0, 1.0, 0.0, 0.0, 0.0], 4, 30.0, 24.0);
        assert_eq!(p.len(), 4);
        assert!(close(p[0].0, 0.0) && close(p[0].1, 2.0));
        assert!(close(p[3].0, 30.0));
        // One more sample than room leaves a single one on screen: no line.
        assert!(graph_points(&[0.0, 0.0, 1.0], 1, 20.0, 24.0).is_empty());
    }

    #[test]
    fn a_line_needs_two_samples() {
        assert!(graph_points(&[], 90, 100.0, 40.0).is_empty());
        assert!(graph_points(&[0.5], 90, 100.0, 40.0).is_empty());
        assert!(graph_points(&[0.5, 0.5], 90, 0.0, 40.0).is_empty());
        assert_eq!(graph_points(&[0.5, 0.5], 90, 100.0, 40.0).len(), 2);
        // A capacity of one or less does not divide by zero.
        assert!(
            graph_points(&[0.5, 0.5, 0.5], 1, 100.0, 40.0)
                .iter()
                .all(|p| p.0.is_finite())
        );
        assert!(
            graph_points(&[0.5, 0.5, 0.5], 0, 100.0, 40.0)
                .iter()
                .all(|p| p.0.is_finite())
        );
    }

    #[test]
    fn readings_out_of_range_stay_in_the_graph() {
        let p = graph_points(&[-3.0, 7.0, f32::NAN], 3, 20.0, 24.0);
        assert!(close(p[0].1, 22.0) && close(p[1].1, 2.0) && close(p[2].1, 22.0));
        // A graph too short for its margins.
        let p = graph_points(&[0.0, 1.0], 2, 20.0, 3.0);
        assert!(p.iter().all(|p| close(p.1, 2.0)));
    }

    #[test]
    fn rounded_corners_cut_the_fill() {
        let clip = Clip::rounded(40.0, 20.0, 8.0);
        // A square corner that sticks out of the rounded one loses its tip.
        let cut = clip.polygon(&[(0.0, 10.0), (40.0, 10.0), (40.0, 20.0), (0.0, 20.0)]);
        assert!(cut.len() > 4);
        assert!(
            cut.iter()
                .all(|p| p.0 >= -1e-3 && p.0 <= 40.001 && p.1 <= 20.001)
        );
        assert!(!cut.iter().any(|p| close(p.0, 0.0) && close(p.1, 20.0)));
        // Inside the rectangle's straight part it stays as it is.
        let inner = [(10.0, 5.0), (30.0, 5.0), (30.0, 15.0), (10.0, 15.0)];
        assert_eq!(clip.polygon(&inner).len(), 4);
    }

    #[test]
    fn rounded_corners_cut_the_line() {
        let clip = Clip::rounded(40.0, 20.0, 8.0);
        // A line along the bottom, two pixels up, starts a little in from the corner.
        let parts = clip.polyline(&[(0.0, 18.0), (20.0, 18.0), (40.0, 18.0)]);
        assert_eq!(parts.len(), 1);
        let line = &parts[0];
        assert!(line[0].0 > 1.0 && line[0].0 < 5.0, "{line:?}");
        assert!(line.last().unwrap().0 < 39.0);
        // A line outside is gone, and one that dips out and back comes in two pieces.
        assert!(clip.polyline(&[(-5.0, 30.0), (45.0, 30.0)]).is_empty());
        let parts = clip.polyline(&[(5.0, 10.0), (20.0, 40.0), (35.0, 10.0)]);
        assert_eq!(parts.len(), 2, "{parts:?}");
    }

    #[test]
    fn a_ring_starts_at_twelve_and_goes_clockwise() {
        let (x, y) = arc_end(50.0, 50.0, 40.0, 0.25);
        assert!(close(x, 90.0) && close(y, 50.0));
        let (x, y) = arc_end(50.0, 50.0, 40.0, 0.5);
        assert!(close(x, 50.0) && close(y, 90.0));
        // The middle line sits inside the outer edge by half the stroke (and a hair).
        assert!(close(ring_radius(100.0, 6.0), 46.5));
    }

    #[test]
    fn mixing_takes_the_share_of_the_first() {
        let red = Hsla::from(Rgba {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        });
        let blue = Hsla::from(Rgba {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 1.0,
        });
        let m = Rgba::from(mix(red, blue, 0.18));
        assert!(close(m.r, 0.18) && close(m.b, 0.82));
    }
}

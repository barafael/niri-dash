//! Drawing for the time widgets: rings, hands, ticks, dots. GPUI has no stroked arcs with round
//! ends, so everything here is built out of filled polygons (a ring is an annulus, a hand a
//! capsule) that are merged into one path with the non-zero rule, which keeps translucent colors
//! even where the pieces overlap.

use crate::kit::Tk;
use gpui::prelude::*;
use gpui::{
    Bounds, FillOptions, FillRule, Hsla, PathBuilder, PathStyle, Pixels, Role, Stateful, Window,
    canvas, div, point, px,
};
use std::f32::consts::TAU;

/// A point as the polygons keep them, in window pixels.
pub type Pt = (f32, f32);

/// How a polygon runs, once all of them are made to agree: the circles' own direction.
fn area(points: &[Pt]) -> f32 {
    let mut sum = 0.0;
    for i in 0..points.len() {
        let (a, b) = (points[i], points[(i + 1) % points.len()]);
        sum += a.0 * b.1 - b.0 * a.1;
    }
    sum / 2.0
}

fn pixel((x, y): Pt) -> gpui::Point<Pixels> {
    point(px(x), px(y))
}

/// The pieces of one filled shape.
pub struct Shape {
    pb: PathBuilder,
}

impl Shape {
    pub fn new() -> Self {
        let style = PathStyle::Fill(FillOptions::default().with_fill_rule(FillRule::NonZero));
        Self {
            pb: PathBuilder::fill().with_style(style),
        }
    }

    /// A polygon that adds to the shape.
    fn solid(&mut self, mut points: Vec<Pt>) {
        if area(&points) < 0.0 {
            points.reverse();
        }
        let points: Vec<_> = points.into_iter().map(pixel).collect();
        self.pb.add_polygon(&points, true);
    }

    /// A polygon that cuts a hole in what it lies on.
    fn hole(&mut self, mut points: Vec<Pt>) {
        if area(&points) > 0.0 {
            points.reverse();
        }
        let points: Vec<_> = points.into_iter().map(pixel).collect();
        self.pb.add_polygon(&points, true);
    }

    pub fn circle(&mut self, c: Pt, r: f32) {
        self.solid(circle_points(c, r));
    }

    /// The whole ring of mid radius `r` and width `thickness`.
    pub fn ring(&mut self, c: Pt, r: f32, thickness: f32) {
        self.solid(circle_points(c, r + thickness / 2.0));
        self.hole(circle_points(c, (r - thickness / 2.0).max(0.0)));
    }

    /// The part of the ring from angle `from` to `to` (turns, clockwise from 12 o'clock), with
    /// round ends.
    pub fn arc(&mut self, c: Pt, r: f32, thickness: f32, from: f32, to: f32) {
        let (outer, inner) = (r + thickness / 2.0, (r - thickness / 2.0).max(0.0));
        let steps = (((to - from).abs() * 120.0).ceil() as usize).max(2);
        let at = |radius: f32, t: f32| -> Pt { on_circle(c, radius, from + (to - from) * t) };
        let mut points: Vec<Pt> = (0..=steps)
            .map(|i| at(outer, i as f32 / steps as f32))
            .collect();
        points.extend((0..=steps).rev().map(|i| at(inner, i as f32 / steps as f32)));
        self.solid(points);
        self.circle(on_circle(c, r, from), thickness / 2.0);
        self.circle(on_circle(c, r, to), thickness / 2.0);
    }

    /// A line `width` thick with round ends.
    pub fn line(&mut self, a: Pt, b: Pt, width: f32) {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt();
        if len > 0.0 {
            let (nx, ny) = (-dy / len * width / 2.0, dx / len * width / 2.0);
            self.solid(vec![
                (a.0 + nx, a.1 + ny),
                (b.0 + nx, b.1 + ny),
                (b.0 - nx, b.1 - ny),
                (a.0 - nx, a.1 - ny),
            ]);
        }
        self.circle(a, width / 2.0);
        self.circle(b, width / 2.0);
    }

    pub fn paint(self, color: Hsla, window: &mut Window) {
        if let Ok(path) = self.pb.build() {
            window.paint_path(path, color);
        }
    }
}

fn circle_points(c: Pt, r: f32) -> Vec<Pt> {
    let n = ((r.max(1.0).sqrt() * 8.0) as usize).clamp(16, 120);
    (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * TAU;
            (c.0 + r * a.cos(), c.1 + r * a.sin())
        })
        .collect()
}

/// The point at `turns` (a full turn is 1, 0 is 12 o'clock, clockwise) on the circle round `c`.
pub fn on_circle(c: Pt, r: f32, turns: f32) -> Pt {
    let a = turns * TAU;
    (c.0 + r * a.sin(), c.1 - r * a.cos())
}

fn centre(b: Bounds<Pixels>) -> (Pt, f32) {
    let (x, y) = (f32::from(b.origin.x), f32::from(b.origin.y));
    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
    ((x + w / 2.0, y + h / 2.0), w.min(h))
}

/// A canvas over its parent that draws with `paint(bounds, window)`.
pub fn painter(paint: impl Fn(Bounds<Pixels>, &mut Window) + 'static) -> impl IntoElement {
    canvas(|_, _, _| (), move |bounds, _, window, _| paint(bounds, window))
        .absolute()
        .size_full()
}

/// A ring from 12 o'clock clockwise, `value` (0 to 1) of the way round. It is a gauge for screen
/// readers: `label` says what it measures.
pub fn ring(
    id: &'static str,
    label: &'static str,
    value: f32,
    thickness: f32,
    tint: Hsla,
    tk: &Tk,
) -> Stateful<gpui::Div> {
    let track = tk.track();
    let value = value.clamp(0.0, 0.9999);
    div()
        .id(id)
        .role(Role::ProgressIndicator)
        .aria_label(label)
        .aria_value(format!("{}%", (value * 100.0).round() as i32))
        .aria_min_numeric_value(0.0)
        .aria_max_numeric_value(100.0)
        .aria_numeric_value((value * 100.0).round() as f64)
        .absolute()
        .size_full()
        .child(painter(move |bounds, window| {
            let (c, side) = centre(bounds);
            // As Slint's `Ring` is drawn: its stroked path is fitted inside the box with half the
            // stroke to spare, so the ring floats a little in from the card's edge.
            let r = crate::kit::ring_radius(side, thickness);
            let mut shape = Shape::new();
            shape.ring(c, r, thickness);
            shape.paint(track, window);
            if value > 0.002 {
                let mut shape = Shape::new();
                shape.arc(c, r, thickness, 0.0, value);
                shape.paint(tint, window);
            }
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_run_clockwise_from_twelve() {
        let c = (50.0, 50.0);
        let (x, y) = on_circle(c, 10.0, 0.0);
        assert!((x - 50.0).abs() < 1e-4 && (y - 40.0).abs() < 1e-4);
        let (x, y) = on_circle(c, 10.0, 0.25);
        assert!((x - 60.0).abs() < 1e-4 && (y - 50.0).abs() < 1e-4);
        let (x, y) = on_circle(c, 10.0, 0.5);
        assert!((x - 50.0).abs() < 1e-4 && (y - 60.0).abs() < 1e-4);
    }

    #[test]
    fn polygons_are_turned_to_agree() {
        let square = vec![(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)];
        let mut reversed = square.clone();
        reversed.reverse();
        assert!(area(&square) * area(&reversed) < 0.0);
        let mut shape = Shape::new();
        shape.solid(square);
        shape.hole(reversed);
    }
}

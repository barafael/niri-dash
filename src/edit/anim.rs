//! Things that glide: Slint's `animate prop { duration; easing }`, for GPUI. A view asks
//! [`Anims::follow`] for the value of a property it wants to reach `target`; when the target
//! changes the value eases over to it. The look's `motion` scales every duration, and 0 (or the
//! headless harness, which sets [`Anims::snap`]) turns animating off.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How a value eases from where it was to where it goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ease {
    Linear,
    /// Slint's `ease-out`.
    Out,
    /// `cubic-bezier(0.2, 0.9, 0.3, 1.0)`: fast at first, settling softly.
    Soft,
}

impl Ease {
    /// The progress of an animation `t` (0 to 1) of the way through its time.
    pub fn at(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Ease::Linear => t,
            Ease::Out => bezier(0.0, 0.0, 0.58, 1.0, t),
            Ease::Soft => bezier(0.2, 0.9, 0.3, 1.0, t),
        }
    }
}

/// The y of a CSS cubic bezier (control points (x1, y1) and (x2, y2)) at x = `t`.
fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let coord = |a: f32, b: f32, s: f32| {
        let c = 3.0 * a;
        let b_ = 3.0 * (b - a) - c;
        let a_ = 1.0 - c - b_;
        ((a_ * s + b_) * s + c) * s
    };
    // Solve x(s) = t by bisection: slow and certain.
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if coord(x1, x2, mid) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    coord(y1, y2, (lo + hi) / 2.0)
}

#[derive(Debug, Clone, Copy)]
struct Tween {
    from: f32,
    to: f32,
    started: Instant,
    dur: Duration,
    ease: Ease,
}

impl Tween {
    fn value(&self, now: Instant) -> (f32, bool) {
        if self.dur.is_zero() {
            return (self.to, false);
        }
        let t = now.saturating_duration_since(self.started).as_secs_f32() / self.dur.as_secs_f32();
        if t >= 1.0 {
            (self.to, false)
        } else {
            (self.from + (self.to - self.from) * self.ease.at(t), true)
        }
    }
}

/// What a property is: its kind and the widget (or row) it belongs to.
pub type Key = (&'static str, u32);

#[derive(Default)]
struct Inner {
    tweens: HashMap<Key, Tween>,
    shown_at: Option<Instant>,
    busy: bool,
}

#[derive(Default)]
pub struct Anims {
    /// Everything is at its end at once.
    pub snap: bool,
    inner: RefCell<Inner>,
}

impl Anims {
    /// The dashboard shows itself: its entrance starts now.
    pub fn shown(&mut self) {
        self.inner.get_mut().shown_at = Some(Instant::now());
    }

    /// Whether something is still moving: the window wants another frame.
    pub fn take_busy(&self) -> bool {
        std::mem::take(&mut self.inner.borrow_mut().busy)
    }

    /// The value of `key`, going to `target` over `ms` milliseconds (times `motion`) whenever the
    /// target changes. The first time a key is asked for, it is at its target.
    pub fn follow(&self, key: Key, target: f32, ms: f32, motion: f32, ease: Ease) -> f32 {
        let dur = Duration::from_secs_f32((ms * motion).max(0.0) / 1000.0);
        let mut inner = self.inner.borrow_mut();
        if self.snap {
            inner.tweens.remove(&key);
            return target;
        }
        let now = Instant::now();
        if dur.is_zero() {
            // No gliding now, but where it is now is where the next glide starts.
            inner.tweens.insert(
                key,
                Tween {
                    from: target,
                    to: target,
                    started: now,
                    dur: Duration::ZERO,
                    ease,
                },
            );
            return target;
        }
        let current = match inner.tweens.get(&key) {
            None => {
                inner.tweens.insert(
                    key,
                    Tween {
                        from: target,
                        to: target,
                        started: now,
                        dur: Duration::ZERO,
                        ease,
                    },
                );
                return target;
            }
            Some(t) => t.value(now).0,
        };
        let tween = inner.tweens.get_mut(&key).expect("just looked up");
        if (tween.to - target).abs() > f32::EPSILON {
            *tween = Tween {
                from: current,
                to: target,
                started: now,
                dur,
                ease,
            };
        }
        let (v, moving) = tween.value(now);
        if moving {
            inner.busy = true;
        }
        v
    }

    /// How far the entrance of the dashboard has got (0 to 1), `ms` long after `delay_ms`.
    pub fn entrance(&self, delay_ms: f32, ms: f32, motion: f32, ease: Ease) -> f32 {
        let mut inner = self.inner.borrow_mut();
        let Some(at) = inner.shown_at.filter(|_| !self.snap && motion > 0.0) else {
            return 1.0;
        };
        let elapsed = at.elapsed().as_secs_f32() * 1000.0;
        let (delay, dur) = (delay_ms * motion, (ms * motion).max(1.0));
        let t = (elapsed - delay) / dur;
        if t < 1.0 {
            inner.busy = true;
        }
        ease.at(t)
    }

    /// Where `key` is now (0 if it was never asked for).
    pub fn value(&self, key: Key) -> f32 {
        self.inner
            .borrow()
            .tweens
            .get(&key)
            .map_or(0.0, |t| t.value(Instant::now()).0)
    }

    /// Forget a widget's properties (it went away).
    pub fn forget(&mut self, id: u32) {
        self.inner.get_mut().tweens.retain(|(_, i), _| *i != id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_starts_and_ends_where_it_should() {
        for e in [Ease::Linear, Ease::Out, Ease::Soft] {
            assert!(e.at(0.0).abs() < 1e-4, "{e:?}");
            assert!((e.at(1.0) - 1.0).abs() < 1e-4, "{e:?}");
            let mid = e.at(0.5);
            assert!(mid > 0.0 && mid < 1.0, "{e:?}");
        }
        // Slint's ease-out and the soft curve are ahead of linear at the middle.
        assert!(Ease::Out.at(0.5) > 0.5 && Ease::Soft.at(0.5) > 0.5);
    }

    #[test]
    fn a_value_glides_to_its_new_target() {
        let a = Anims::default();
        let key = ("x", 1);
        // Seen for the first time, it is at its target.
        assert_eq!(a.follow(key, 3.0, 1000.0, 1.0, Ease::Linear), 3.0);
        // A new target: it starts from where it was.
        let v = a.follow(key, 10.0, 1000.0, 1.0, Ease::Linear);
        assert!((3.0..10.0).contains(&v), "{v}");
        assert!(a.take_busy());
        // No motion: at once.
        assert_eq!(a.follow(key, 0.0, 1000.0, 0.0, Ease::Linear), 0.0);
    }

    #[test]
    fn a_headless_run_has_no_animation() {
        let a = Anims {
            snap: true,
            ..Anims::default()
        };
        assert_eq!(a.follow(("x", 1), 1.0, 500.0, 1.0, Ease::Soft), 1.0);
        assert_eq!(a.follow(("x", 1), 7.0, 500.0, 1.0, Ease::Soft), 7.0);
        assert_eq!(a.entrance(10.0, 100.0, 1.0, Ease::Out), 1.0);
        assert!(!a.take_busy());
    }
}

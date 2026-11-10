//! The keyboard cursor of an entered widget: which stop (a row, a button) it is on, which part of
//! the stop, and what the keys do to it. One [`Scope`] per widget that has controls; the widget
//! says what its stops do through [`Stops`].
//!
//! `j`/`k` (or the arrows) move between stops, by `stride` in a grid; `h`/`l` move between the
//! parts of a stop, or, for a stop without parts, adjust it (a slider); Enter or Space presses;
//! `g`/`G` go to the first and last stop; Tab and Shift+Tab go on and back; any other letter is
//! the widget's own; Escape and Backspace go back.

use crate::keys::{Code, Key, Named};

/// What a widget's stops do. Everything has a default that does nothing.
pub trait Stops {
    /// `h`/`l` (-1/+1) on a stop without parts, and `j`/`k` in a widget with a single stop
    /// (-1 for `j`, +1 for `k`): true if it did something.
    fn adjust(&mut self, _cursor: usize, _delta: i32) -> bool {
        false
    }
    /// Enter, or Space when the widget has no use for it: press part `sub` of the stop.
    fn activate(&mut self, _cursor: usize, _sub: usize) {}
    /// Any other key. True if the widget took it.
    fn letter(&mut self, _key: &Key, _cursor: usize) -> bool {
        false
    }
    /// Escape or Backspace: step back (close a menu, say). True if there was something to do.
    fn back(&mut self) -> bool {
        false
    }
    /// A stop the cursor passes over (a section heading).
    fn skip(&self, _stop: usize) -> bool {
        false
    }
    /// Sees every key first (while the widget shows a menu, say); true if it took it.
    fn filter(&mut self, _key: &Key) -> bool {
        false
    }
    /// Ctrl+C on the stop under the cursor: true if there was something to copy.
    fn copy(&mut self, _cursor: usize) -> bool {
        false
    }
}

/// How many skipped stops in a row the cursor passes over.
const SKIPS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scope {
    pub cursor: usize,
    pub sub: usize,
    /// How many stops there are.
    pub count: usize,
    /// How many parts the stop under the cursor has (1: just itself).
    pub subs: usize,
    /// How far `j`/`k` move: a grid's row length.
    pub stride: usize,
}

impl Default for Scope {
    fn default() -> Self {
        Self {
            cursor: 0,
            sub: 0,
            count: 0,
            subs: 1,
            stride: 1,
        }
    }
}

impl Scope {
    pub fn new() -> Self {
        Self::default()
    }

    fn fit(&self, c: isize) -> usize {
        c.clamp(0, self.count.saturating_sub(1) as isize) as usize
    }

    /// Move to `c`, or on past skipped stops in direction `d` (staying put if all are skipped).
    pub fn go(&mut self, c: isize, d: isize, stops: &impl Stops) {
        let mut n = self.fit(c);
        for _ in 0..SKIPS {
            if stops.skip(n) {
                n = self.fit(n as isize + d);
            } else {
                break;
            }
        }
        if !stops.skip(n) && n != self.cursor {
            self.cursor = n;
            self.sub = 0;
        }
    }

    /// Put the cursor on `c` (or past skipped stops after it).
    pub fn move_to(&mut self, c: usize, stops: &impl Stops) {
        self.go(c as isize, 1, stops);
    }

    /// The number of stops changed: the cursor stays on a stop that exists, and not on one that
    /// is skipped.
    pub fn set_count(&mut self, count: usize, stops: &impl Stops) {
        self.count = count;
        if self.cursor >= count {
            self.cursor = count.saturating_sub(1);
            self.sub = 0;
        }
        self.go(self.cursor as isize, 1, stops);
    }

    /// The parts of the stop under the cursor changed (a network came up, and its details button
    /// is Disconnect now): back to the stop itself.
    pub fn set_subs(&mut self, subs: usize) {
        let subs = subs.max(1);
        if subs != self.subs {
            self.subs = subs;
            self.sub = 0;
        }
    }

    /// The list changed under the cursor and put a stop it can't rest on there: on to the next
    /// one, or the one before.
    pub fn settle(&mut self, stops: &impl Stops) {
        if stops.skip(self.cursor) {
            self.go(self.cursor as isize, 1, stops);
        }
        if stops.skip(self.cursor) {
            self.go(self.cursor as isize, -1, stops);
        }
    }

    /// The part of the stop the cursor is on, within bounds.
    pub fn part(&self) -> usize {
        self.sub.min(self.subs.saturating_sub(1))
    }

    /// A key, for a widget that is entered and has the keyboard. True if it was taken.
    pub fn press(&mut self, key: &Key, stops: &mut impl Stops) -> bool {
        if (key.is('c') || key.is('C')) && key.ctrl && !key.shift && !key.alt && !key.meta {
            return stops.copy(self.cursor);
        }
        if key.is_named(Named::Escape) {
            return stops.back();
        }
        if !key.bare() {
            return false;
        }
        if stops.filter(key) {
            return true;
        }
        let stride = self.stride.max(1);
        match key.code {
            Code::Named(Named::Tab | Named::BackTab) => {
                let d: isize = if key.is_named(Named::Tab) { 1 } else { -1 };
                if self.count > 1 {
                    self.go(self.fit(self.cursor as isize + d) as isize, d, stops);
                }
                true
            }
            Code::Char('j')
            | Code::Named(Named::Down)
            | Code::Char('k')
            | Code::Named(Named::Up) => {
                let down = key.is('j') || key.is_named(Named::Down);
                if self.count <= 1 {
                    stops.adjust(self.cursor, if down { -1 } else { 1 });
                } else if down && self.cursor + stride < self.count {
                    self.go((self.cursor + stride) as isize, 1, stops);
                } else if !down && self.cursor >= stride {
                    self.go((self.cursor - stride) as isize, -1, stops);
                }
                true
            }
            Code::Char('g') | Code::Named(Named::Home) => {
                self.go(0, 1, stops);
                true
            }
            Code::Char('G') | Code::Named(Named::End) => {
                self.go(self.count as isize - 1, -1, stops);
                true
            }
            Code::Char('h')
            | Code::Named(Named::Left)
            | Code::Char('l')
            | Code::Named(Named::Right) => {
                let d: i32 = if key.is('h') || key.is_named(Named::Left) {
                    -1
                } else {
                    1
                };
                if self.subs > 1 {
                    self.sub = (self.sub as i32 + d).clamp(0, self.subs as i32 - 1) as usize;
                    true
                } else {
                    stops.adjust(self.cursor, d)
                }
            }
            Code::Named(Named::Enter) => {
                stops.activate(self.cursor, self.part());
                true
            }
            // Space is the widget's own first (play / pause, say), else the same as Enter.
            Code::Char(' ') => {
                if !stops.letter(key, self.cursor) {
                    stops.activate(self.cursor, self.part());
                }
                true
            }
            Code::Named(Named::Backspace) => stops.back(),
            // `e` blows the widget up: the dashboard's.
            Code::Char('e') => false,
            _ => stops.letter(key, self.cursor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Rows {
        skipped: Vec<usize>,
        pressed: Vec<(usize, usize)>,
        adjusted: Vec<(usize, i32)>,
    }

    impl Stops for Rows {
        fn skip(&self, stop: usize) -> bool {
            self.skipped.contains(&stop)
        }
        fn activate(&mut self, cursor: usize, sub: usize) {
            self.pressed.push((cursor, sub));
        }
        fn adjust(&mut self, cursor: usize, delta: i32) -> bool {
            self.adjusted.push((cursor, delta));
            true
        }
    }

    fn scope(count: usize) -> Scope {
        Scope {
            count,
            ..Scope::default()
        }
    }

    fn press(s: &mut Scope, r: &mut Rows, name: &str) -> bool {
        s.press(&Key::parse(name).unwrap(), r)
    }

    #[test]
    fn j_and_k_move_between_stops_and_stop_at_the_ends() {
        let (mut s, mut r) = (scope(3), Rows::default());
        press(&mut s, &mut r, "j");
        press(&mut s, &mut r, "down");
        press(&mut s, &mut r, "j");
        assert_eq!(s.cursor, 2);
        press(&mut s, &mut r, "k");
        assert_eq!(s.cursor, 1);
        press(&mut s, &mut r, "g");
        assert_eq!(s.cursor, 0);
        press(&mut s, &mut r, "G");
        assert_eq!(s.cursor, 2);
    }

    #[test]
    fn headings_are_passed_over() {
        let (mut s, mut r) = (scope(5), Rows::default());
        r.skipped = vec![0, 3];
        s.set_count(5, &r);
        assert_eq!(s.cursor, 1, "starts past the heading");
        press(&mut s, &mut r, "j");
        press(&mut s, &mut r, "j");
        assert_eq!(s.cursor, 4, "3 is a heading");
        press(&mut s, &mut r, "k");
        assert_eq!(s.cursor, 2);
    }

    #[test]
    fn one_stop_turns_j_and_k_into_adjusting() {
        let (mut s, mut r) = (scope(1), Rows::default());
        press(&mut s, &mut r, "j");
        press(&mut s, &mut r, "k");
        assert_eq!(r.adjusted, [(0, -1), (0, 1)]);
    }

    #[test]
    fn h_and_l_walk_the_parts_of_a_stop_or_adjust_it() {
        let (mut s, mut r) = (scope(2), Rows::default());
        s.set_subs(3);
        press(&mut s, &mut r, "l");
        press(&mut s, &mut r, "l");
        press(&mut s, &mut r, "l");
        assert_eq!(s.sub, 2);
        press(&mut s, &mut r, "enter");
        assert_eq!(r.pressed, [(0, 2)]);
        press(&mut s, &mut r, "j");
        assert_eq!(s.sub, 0, "a new stop starts at its first part");
        s.set_subs(1);
        assert!(press(&mut s, &mut r, "l"));
        assert_eq!(r.adjusted, [(1, 1)]);
    }

    #[test]
    fn a_grid_moves_by_its_row() {
        let (mut s, mut r) = (scope(6), Rows::default());
        s.stride = 3;
        press(&mut s, &mut r, "j");
        assert_eq!(s.cursor, 3);
        press(&mut s, &mut r, "j");
        assert_eq!(s.cursor, 3, "no row below");
        press(&mut s, &mut r, "k");
        assert_eq!(s.cursor, 0);
    }

    #[test]
    fn the_cursor_comes_back_when_the_list_shrinks() {
        let (mut s, r) = (scope(5), Rows::default());
        s.cursor = 4;
        s.set_count(2, &r);
        assert_eq!(s.cursor, 1);
        s.set_count(0, &r);
        assert_eq!(s.cursor, 0);
    }

    #[test]
    fn modified_keys_are_not_the_scopes() {
        let (mut s, mut r) = (scope(3), Rows::default());
        assert!(!press(&mut s, &mut r, "ctrl+j"));
        assert_eq!(s.cursor, 0);
        // `e` is the dashboard's.
        assert!(!press(&mut s, &mut r, "e"));
    }
}

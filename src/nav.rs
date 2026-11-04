//! Keyboard navigation between widgets: the hint labels `f` shows on each tile, and which tile
//! Ctrl+h/j/k/l moves to.
//!
//! A label is the start of the widget's name, as short as the other widgets allow, so a widget
//! is reached by typing its name until it is the only one left: `v` for the volume, `ca` for the
//! calendar next to a clock (`clo`) and a clipboard (`cli`). Names of two words go by the first
//! one's initial: `cs` for claude-sessions, `sm` for system-monitor, `nl` for night-light.

use crate::layout::{Kind, Rect};

/// `f` shows the hints and, before anything is typed, hides them again (app.slint), so no
/// label may start with it.
#[cfg(test)]
const TRIGGER: char = 'f';

/// What a widget's label is the start of: its config name with the first word of a two-word
/// name cut down to its initial ("claude-sessions" → "csessions"), letters only.
fn name_key(kind: Kind) -> String {
    let words: Vec<&str> = kind.name().split('-').filter(|w| !w.is_empty()).collect();
    let mut key = String::new();
    for (i, w) in words.iter().enumerate() {
        if i + 1 < words.len() {
            key.extend(w.chars().next());
        } else {
            key.push_str(w);
        }
    }
    key.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The shortest start of `key` that no other of `keys` starts with, or None if `key` is itself
/// the start of another one.
fn unique_prefix(key: &str, keys: &[&str]) -> Option<String> {
    let others: Vec<&str> = keys.iter().copied().filter(|k| *k != key).collect();
    (1..=key.len())
        .map(|n| &key[..n])
        .find(|p| !others.iter().any(|o| o.starts_with(p)))
        .map(String::from)
}

/// A label for each of `widgets` (same order). Each is the start of the widget's name (see the
/// module doc), so moving a widget around doesn't change it. Widgets of the same kind add a
/// number in id order (`clo1`, `clo2`); no label is the start of another.
pub fn assign_hints(widgets: &[(u32, Kind)]) -> Vec<String> {
    let keys: Vec<String> = widgets.iter().map(|&(_, k)| name_key(k)).collect();
    let mut distinct: Vec<&str> = keys.iter().map(String::as_str).collect();
    distinct.sort_unstable();
    distinct.dedup();
    let mut labels = vec![String::new(); widgets.len()];
    for key in &distinct {
        let mut group: Vec<usize> = (0..widgets.len()).filter(|&i| keys[i] == *key).collect();
        group.sort_by_key(|&i| widgets[i].0);
        let (stem, numbered) = match unique_prefix(key, &distinct) {
            Some(p) => (p, group.len() > 1),
            // Its whole name starts another one's: a digit sets it apart (the other label goes
            // on with a letter).
            None => (key.to_string(), true),
        };
        // Numbers of the same width, so `1` isn't the start of `10`.
        let width = group.len().to_string().len();
        for (n, &i) in group.iter().enumerate() {
            labels[i] = if numbered {
                format!("{stem}{:0width$}", n + 1)
            } else {
                stem.clone()
            };
        }
    }
    labels
}

/// The widget whose label is `typed`: `Ok(Some(i))` for a full match, `Ok(None)` while `typed` is
/// still the start of some label, `Err(())` when it matches nothing.
pub fn pick(labels: &[String], typed: &str) -> Result<Option<usize>, ()> {
    let typed = typed.to_lowercase();
    if typed.is_empty() {
        return Ok(None);
    }
    if let Some(i) = labels.iter().position(|l| *l == typed) {
        return Ok(Some(i));
    }
    if labels.iter().any(|l| l.starts_with(&typed)) {
        Ok(None)
    } else {
        Err(())
    }
}

/// A direction to move in, as h/j/k/l.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Down,
    Up,
    Right,
}

impl Dir {
    /// 0..3 for h, j, k, l (what the UI passes).
    pub fn from_index(i: i32) -> Option<Dir> {
        Some(match i {
            0 => Dir::Left,
            1 => Dir::Down,
            2 => Dir::Up,
            3 => Dir::Right,
            _ => return None,
        })
    }
}

/// The tile next to `tiles[from]` in `dir`: of the tiles whose centre lies that way, the ones
/// level with it come first (they share a row or column), then the nearest edge, then the one
/// best lined up with it.
pub fn neighbor(tiles: &[Rect], from: usize, dir: Dir) -> Option<usize> {
    let a = tiles.get(from)?;
    // Doubled, so centres stay whole numbers.
    let centre = |r: &Rect| (2 * r.col + r.w, 2 * r.row + r.h);
    let (ax, ay) = centre(a);
    tiles
        .iter()
        .enumerate()
        .filter(|&(i, _)| i != from)
        .filter_map(|(i, b)| {
            let (bx, by) = centre(b);
            let overlaps_rows = b.row < a.row + a.h && a.row < b.row + b.h;
            let overlaps_cols = b.col < a.col + a.w && a.col < b.col + b.w;
            let (ahead, gap, level, off) = match dir {
                Dir::Left => (
                    bx < ax,
                    a.col - (b.col + b.w),
                    overlaps_rows,
                    (by - ay).abs(),
                ),
                Dir::Right => (
                    bx > ax,
                    b.col - (a.col + a.w),
                    overlaps_rows,
                    (by - ay).abs(),
                ),
                Dir::Up => (
                    by < ay,
                    a.row - (b.row + b.h),
                    overlaps_cols,
                    (bx - ax).abs(),
                ),
                Dir::Down => (
                    by > ay,
                    b.row - (a.row + a.h),
                    overlaps_cols,
                    (bx - ax).abs(),
                ),
            };
            ahead.then_some((i, (!level, gap.max(0), off)))
        })
        .min_by_key(|&(_, key)| key)
        .map(|(i, _)| i)
}

impl Dir {
    /// One cell that way, as (columns, rows).
    pub fn step(self) -> (i32, i32) {
        match self {
            Dir::Left => (-1, 0),
            Dir::Down => (0, 1),
            Dir::Up => (0, -1),
            Dir::Right => (1, 0),
        }
    }
}

/// `r` with its `edge` side pushed out one cell (`grow`) or pulled in, for the keyboard's sizing
/// mode: L widens to the right, l narrows from the right, H widens to the left, and so on. Round
/// widgets stay square, so either side changes both. None if it would go below `min`.
pub fn resized(r: Rect, edge: Dir, grow: bool, round: bool, min: (i32, i32)) -> Option<Rect> {
    let d = if grow { 1 } else { -1 };
    let sideways = matches!(edge, Dir::Left | Dir::Right);
    let (mut w, mut h) = if sideways {
        (r.w + d, r.h)
    } else {
        (r.w, r.h + d)
    };
    if round {
        let s = if sideways { w } else { h };
        (w, h) = (s, s);
    }
    if w < min.0 || h < min.1 {
        return None;
    }
    Some(Rect {
        col: if edge == Dir::Left {
            r.col + r.w - w
        } else {
            r.col
        },
        row: if edge == Dir::Up {
            r.row + r.h - h
        } else {
            r.row
        },
        w,
        h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use strum::IntoEnumIterator;

    fn rect(col: i32, row: i32, w: i32, h: i32) -> Rect {
        Rect { col, row, w, h }
    }

    /// Labels are fine: none empty, none twice, none the start of another, none starting with f.
    fn assert_sound(l: &[String]) {
        for (i, a) in l.iter().enumerate() {
            assert!(!a.is_empty() && !a.starts_with(TRIGGER), "{a:?}");
            for (j, b) in l.iter().enumerate() {
                assert!(i == j || !b.starts_with(a.as_str()), "{a:?} starts {b:?}");
            }
        }
    }

    #[test]
    fn labels_are_name_starts() {
        let l = assign_hints(&[(1, Kind::Wifi), (2, Kind::Clock), (3, Kind::Bluetooth)]);
        assert_eq!(l, ["w", "c", "b"]);
    }

    #[test]
    fn the_default_dashboard() {
        use Kind::*;
        let kinds = [
            Clock,
            Weather,
            Calendar,
            Session,
            Windows,
            ClaudeSessions,
            Media,
            Tray,
            Notifications,
            Clipboard,
            Volume,
            Brightness,
            Wifi,
            Bluetooth,
            Battery,
            SystemMonitor,
        ];
        let all: Vec<(u32, Kind)> = kinds
            .iter()
            .enumerate()
            .map(|(i, &k)| (i as u32, k))
            .collect();
        let l = assign_hints(&all);
        assert_eq!(
            l,
            [
                "clo", "we", "ca", "se", "win", "cs", "m", "t", "n", "cli", "v", "br", "wif", "bl",
                "ba", "sm"
            ]
        );
    }

    #[test]
    fn two_word_names_go_by_initial() {
        assert_eq!(name_key(Kind::ClaudeSessions), "csessions");
        assert_eq!(name_key(Kind::NightLight), "nlight");
        assert_eq!(name_key(Kind::Wifi), "wifi");
        let l = assign_hints(&[(1, Kind::ClaudeSessions), (2, Kind::ClaudeUsage)]);
        assert_eq!(l, ["cs", "cu"]);
    }

    #[test]
    fn same_kind_gets_numbers_by_id() {
        let l = assign_hints(&[(7, Kind::Clock), (3, Kind::Clock), (9, Kind::Calendar)]);
        assert_eq!(l, ["cl2", "cl1", "ca"]);
        let l = assign_hints(&[(7, Kind::Clock), (3, Kind::Clock), (5, Kind::Timer)]);
        assert_eq!(l, ["c2", "c1", "t"]);
    }

    #[test]
    fn numbers_keep_one_width() {
        let all: Vec<(u32, Kind)> = (0..12).map(|i| (i, Kind::Notes)).collect();
        let l = assign_hints(&all);
        assert_eq!(l[0], "n01");
        assert_eq!(l[11], "n12");
        assert_sound(&l);
    }

    #[test]
    fn a_name_inside_another() {
        // Made-up names: one is the whole start of the other.
        let keys = ["note", "notes"];
        assert_eq!(unique_prefix("note", &keys), None);
        assert_eq!(unique_prefix("notes", &keys).as_deref(), Some("notes"));
    }

    #[test]
    fn labels_dont_depend_on_order() {
        let a = assign_hints(&[(1, Kind::Clock), (2, Kind::Calendar), (3, Kind::Clock)]);
        let b = assign_hints(&[(3, Kind::Clock), (1, Kind::Clock), (2, Kind::Calendar)]);
        assert_eq!(
            (a[0].as_str(), a[1].as_str(), a[2].as_str()),
            (b[1].as_str(), b[2].as_str(), b[0].as_str())
        );
    }

    #[test]
    fn every_kind_at_once() {
        let all: Vec<(u32, Kind)> = Kind::iter()
            .enumerate()
            .map(|(i, k)| (i as u32, k))
            .collect();
        let l = assign_hints(&all);
        assert_sound(&l);
        // Short enough to type: a few letters at most.
        assert!(l.iter().all(|s| s.len() <= 4), "{l:?}");
    }

    #[test]
    fn many_of_every_kind() {
        let all: Vec<(u32, Kind)> = (0..80)
            .map(|i| (i, Kind::iter().nth(i as usize % 31).unwrap()))
            .collect();
        assert_sound(&assign_hints(&all));
    }

    #[test]
    fn picking() {
        let l: Vec<String> = ["ca", "cb", "da"].map(String::from).into();
        assert_eq!(pick(&l, "c"), Ok(None));
        assert_eq!(pick(&l, "CB"), Ok(Some(1)));
        assert_eq!(pick(&l, "x"), Err(()));
        assert_eq!(pick(&l, "cx"), Err(()));
    }

    #[test]
    fn sizing_moves_the_named_edge() {
        let r = rect(4, 2, 3, 2);
        let grow = |e| resized(r, e, true, false, (2, 1)).unwrap();
        assert_eq!(grow(Dir::Right), rect(4, 2, 4, 2));
        assert_eq!(grow(Dir::Left), rect(3, 2, 4, 2));
        assert_eq!(grow(Dir::Down), rect(4, 2, 3, 3));
        assert_eq!(grow(Dir::Up), rect(4, 1, 3, 3));
        let shrink = |e| resized(r, e, false, false, (2, 1));
        assert_eq!(shrink(Dir::Right), Some(rect(4, 2, 2, 2)));
        assert_eq!(shrink(Dir::Left), Some(rect(5, 2, 2, 2)));
        assert_eq!(shrink(Dir::Up), Some(rect(4, 3, 3, 1)));
        // Not below the minimum.
        assert_eq!(
            resized(rect(0, 0, 2, 1), Dir::Down, false, false, (2, 1)),
            None
        );
    }

    #[test]
    fn round_widgets_stay_square() {
        let r = rect(4, 2, 2, 2);
        assert_eq!(
            resized(r, Dir::Right, true, true, (1, 1)),
            Some(rect(4, 2, 3, 3))
        );
        assert_eq!(
            resized(r, Dir::Up, true, true, (1, 1)),
            Some(rect(4, 1, 3, 3))
        );
        assert_eq!(
            resized(r, Dir::Left, false, true, (1, 1)),
            Some(rect(5, 2, 1, 1))
        );
    }

    #[test]
    fn neighbors() {
        // A B
        // C D D   (D is wide)
        // E
        let t = [
            rect(0, 0, 2, 2), // A
            rect(2, 0, 2, 2), // B
            rect(0, 2, 2, 2), // C
            rect(2, 2, 4, 2), // D
            rect(0, 4, 2, 1), // E
        ];
        assert_eq!(neighbor(&t, 0, Dir::Right), Some(1));
        assert_eq!(neighbor(&t, 0, Dir::Down), Some(2));
        assert_eq!(neighbor(&t, 0, Dir::Left), None);
        assert_eq!(neighbor(&t, 0, Dir::Up), None);
        assert_eq!(neighbor(&t, 1, Dir::Down), Some(3));
        assert_eq!(neighbor(&t, 3, Dir::Left), Some(2));
        assert_eq!(neighbor(&t, 3, Dir::Up), Some(1));
        assert_eq!(neighbor(&t, 2, Dir::Down), Some(4));
        // Nothing level with E to the right: the nearest tile that way.
        assert_eq!(neighbor(&t, 4, Dir::Right), Some(3));
    }

    #[test]
    fn level_tiles_beat_nearer_diagonal_ones() {
        // A sits left; B is far right on the same rows; C is just right but lower.
        let t = [rect(0, 0, 1, 1), rect(5, 0, 1, 1), rect(1, 1, 1, 1)];
        assert_eq!(neighbor(&t, 0, Dir::Right), Some(1));
    }
}

//! Which widgets like to sit next to each other.
//!
//! [`affinity`] says, for two kinds of widget, how much they belong together: 1.0 is "right
//! beside each other" (volume and brightness), 0.0 doesn't care (notifications and the
//! weather), -1.0 "keep apart" (the session buttons and the window list). The "Neighbours"
//! auto-layout ([`crate::autolayout::Algo::Neighbours`]) pulls liked pairs together and pushes
//! disliked ones apart.
//!
//! The matrix is built in layers, so the table below stays short: widgets of one [`Group`] get a
//! small baseline liking, then each line of [`TABLE`] sets the pairs it lists, later lines over
//! earlier ones. Tune it by editing the table.

use crate::layout::Kind;
use std::sync::OnceLock;
use strum::IntoEnumIterator;

/// How much two widgets of the same group like each other when nothing says otherwise.
const SAME_GROUP: f32 = 0.25;
/// Two widgets of one kind (two clocks, say).
const SAME_KIND: f32 = 0.5;

use Kind::*;

/// Kinds that like each other to this degree, every pair among them. A line with two kinds is
/// one pair; a negative number is a dislike.
const TABLE: &[(&[Kind], f32)] = &[
    // Time.
    (&[Clock, Calendar], 1.0),
    (&[Clock, Weather], 0.6),
    (&[Calendar, Weather], 0.5),
    (&[Timer, Stopwatch], 0.9),
    (&[Clock, Timer], 0.5),
    (&[Clock, Stopwatch], 0.4),
    // Sliders and switches that change the room.
    (&[Volume, Brightness], 1.0),
    (&[Brightness, NightLight], 0.9),
    (&[Volume, NightLight], 0.4),
    (&[Volume, Media], 0.7),
    (&[Volume, Privacy], 0.3),
    // Radios and what plugs in.
    (&[Wifi, Bluetooth], 1.0),
    (&[Wifi, Network], 0.7),
    (&[Bluetooth, Phone], 0.6),
    (&[Phone, RemovableDrives], 0.5),
    (&[Battery, Wifi], 0.3),
    (&[Battery, Bluetooth], 0.3),
    // The machine's load.
    (&[SystemMonitor, Memory, Gpu], 0.9),
    (&[SystemMonitor, Disks, Network], 0.7),
    (&[Memory, Disks], 0.5),
    (&[Gpu, Network], 0.4),
    (&[Disks, RemovableDrives], 0.7),
    (&[Battery, SystemMonitor], 0.4),
    // What is going on.
    (&[ClaudeSessions, ClaudeUsage], 1.0),
    (&[ClaudeSessions, Windows], 0.4),
    (&[Notifications, Tray], 0.6),
    (&[Notifications, Windows], 0.2),
    (&[Notes, Clipboard], 0.6),
    (&[Clipboard, ColorPicker], 0.4),
    (&[Notes, ClaudeSessions], 0.3),
    (&[Notes, Timer], 0.2),
    // The tray and its neighbours.
    (&[Tray, KeyboardLayout], 0.5),
    (&[Tray, Session], 0.3),
    (&[Session, Battery], 0.4),
    (&[Media, Mascot], 0.2),
    // Dislikes: mixing these up is a click away from a mistake, or just noisy.
    (&[Session, Windows], -0.5),
    (&[Session, Notifications], -0.2),
    (&[Session, ClaudeSessions], -0.3),
];

type Matrix = [[f32; KINDS]; KINDS];

/// How many kinds there are.
const KINDS: usize = 31;

fn index(k: Kind) -> usize {
    Kind::iter().position(|o| o == k).unwrap_or(0)
}

fn matrix() -> &'static Matrix {
    static M: OnceLock<Box<Matrix>> = OnceLock::new();
    M.get_or_init(|| {
        let kinds: Vec<Kind> = Kind::iter().collect();
        let mut m = Box::new([[0.0; KINDS]; KINDS]);
        for (i, a) in kinds.iter().enumerate() {
            for (j, b) in kinds.iter().enumerate() {
                m[i][j] = if i == j {
                    SAME_KIND
                } else if a.info().group == b.info().group {
                    SAME_GROUP
                } else {
                    0.0
                };
            }
        }
        for (set, v) in TABLE {
            for a in set.iter() {
                for b in set.iter().filter(|b| *b != a) {
                    m[index(*a)][index(*b)] = *v;
                    m[index(*b)][index(*a)] = *v;
                }
            }
        }
        m
    })
}

/// How much `a` and `b` like to be neighbours, from -1.0 to 1.0. Symmetric.
pub fn affinity(a: Kind, b: Kind) -> f32 {
    matrix()[index(a)][index(b)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symmetric_and_in_range() {
        for a in Kind::iter() {
            for b in Kind::iter() {
                let v = affinity(a, b);
                assert_eq!(v, affinity(b, a), "{a:?} {b:?}");
                assert!((-1.0..=1.0).contains(&v), "{a:?} {b:?} {v}");
            }
        }
    }

    #[test]
    fn the_examples() {
        assert_eq!(affinity(Volume, Brightness), 1.0);
        assert_eq!(affinity(Wifi, Bluetooth), 1.0);
        assert_eq!(affinity(Clock, Calendar), 1.0);
        assert!(affinity(Memory, Gpu) >= 0.7);
        assert!(affinity(Session, Windows) < 0.0);
        assert_eq!(affinity(Notifications, Weather), 0.0);
        assert_eq!(affinity(Clock, Clock), SAME_KIND);
    }

    /// A kind added without a thought for its neighbours fails here.
    #[test]
    fn every_kind_has_a_partner() {
        for a in Kind::iter() {
            let best = Kind::iter()
                .filter(|b| *b != a)
                .map(|b| affinity(a, b))
                .fold(f32::MIN, f32::max);
            assert!(best >= 0.2, "{a:?} likes nobody ({best})");
        }
    }

    #[test]
    fn the_matrix_covers_every_kind() {
        assert_eq!(Kind::iter().count(), KINDS);
    }
}

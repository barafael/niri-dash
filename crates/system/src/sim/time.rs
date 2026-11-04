//! How the simulated world moves: smooth noise, waves and slots, all plain functions of the time,
//! each with a period of its own, and the day's script of what the machine is being used for.
//! Everything repeats sooner or later, but as the periods share no common beat the whole rarely
//! looks the same twice.

/// A number from 0 to 1 for `seed` and `n`, always the same (splitmix64).
pub fn hash(seed: u64, n: u64) -> f64 {
    let mut z = seed
        .wrapping_mul(0x2545_f491_4f6c_dd1d)
        .wrapping_add(n)
        .wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    (z ^ (z >> 31)) as f64 / u64::MAX as f64
}

/// Smooth noise from 0 to 1 that changes about once per `period`, and repeats after 4096 of them.
pub fn noise(t: f64, period: f64, seed: u64) -> f64 {
    let x = t / period;
    let step = x.floor();
    let at = |n: f64| hash(seed, n.rem_euclid(4096.0) as u64);
    let f = x - step;
    let f = f * f * (3.0 - 2.0 * f);
    at(step) * (1.0 - f) + at(step + 1.0) * f
}

/// A sine from 0 to 1 with this period; `phase` from 0 to 1 shifts it.
pub fn wave(t: f64, period: f64, phase: f64) -> f64 {
    (((t / period + phase) * std::f64::consts::TAU).sin() + 1.0) / 2.0
}

/// How far into a cycle of `period` the time is, from 0 up to `period`.
pub fn into(t: f64, period: f64) -> f64 {
    t.rem_euclid(period)
}

/// Which of `len` steps of `period` seconds each is up.
pub fn slot(t: f64, period: f64, len: usize) -> usize {
    (t / period).floor().rem_euclid(len.max(1) as f64) as usize
}

pub fn mix(a: f64, b: f64, x: f64) -> f64 {
    a + (b - a) * x
}

/// What the machine is being used for at the moment. The widgets agree on it: a build heats the
/// CPU and renames a terminal, a call lights the microphone and takes the GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scene {
    Coding,
    Building,
    Browsing,
    Call,
    Idle,
}

/// The script, which runs about 18 minutes before it starts over.
const SCRIPT: &[(Scene, f64)] = &[
    (Scene::Coding, 170.0),
    (Scene::Building, 70.0),
    (Scene::Coding, 130.0),
    (Scene::Browsing, 180.0),
    (Scene::Call, 210.0),
    (Scene::Idle, 90.0),
    (Scene::Browsing, 95.0),
    (Scene::Building, 40.0),
    (Scene::Coding, 110.0),
];

fn script_len() -> f64 {
    SCRIPT.iter().map(|(_, len)| len).sum()
}

/// The scene at `t`, the one before it, and how many seconds it has been going.
fn scenes(t: f64) -> (Scene, Scene, f64) {
    let mut left = into(t, script_len());
    let mut before = SCRIPT[SCRIPT.len() - 1].0;
    for (scene, len) in SCRIPT {
        if left < *len {
            return (*scene, before, left);
        }
        left -= len;
        before = *scene;
    }
    (before, before, 0.0)
}

pub fn scene(t: f64) -> Scene {
    scenes(t).0
}

/// How many seconds the scene at `t` has been going.
pub fn scene_age(t: f64) -> f64 {
    scenes(t).2
}

/// A value that depends on the scene, gliding over `fade` seconds when the scene changes.
pub fn by_scene(t: f64, fade: f64, f: impl Fn(Scene) -> f64) -> f64 {
    let (now, before, age) = scenes(t);
    if age < fade {
        let x = age / fade;
        mix(f(before), f(now), x * x * (3.0 - 2.0 * x))
    } else {
        f(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_stays_in_range_and_moves() {
        let values: Vec<f64> = (0..5000).map(|i| noise(i as f64, 7.0, 3)).collect();
        assert!(values.iter().all(|v| (0.0..=1.0).contains(v)));
        let (min, max) = values
            .iter()
            .fold((1.0_f64, 0.0_f64), |(a, b), v| (a.min(*v), b.max(*v)));
        assert!(max - min > 0.5, "from {min} to {max}");
    }

    #[test]
    fn noise_has_no_jumps() {
        for i in 0..5000 {
            let t = i as f64 / 4.0;
            assert!((noise(t, 7.0, 9) - noise(t + 0.25, 7.0, 9)).abs() < 0.1);
        }
    }

    #[test]
    fn noise_repeats() {
        assert!((noise(123.0, 2.0, 5) - noise(123.0 + 2.0 * 4096.0, 2.0, 5)).abs() < 1e-9);
    }

    #[test]
    fn every_scene_comes_up_and_the_script_repeats() {
        let seen: Vec<Scene> = (0..script_len() as i64).map(|t| scene(t as f64)).collect();
        for s in [
            Scene::Coding,
            Scene::Building,
            Scene::Browsing,
            Scene::Call,
            Scene::Idle,
        ] {
            assert!(seen.contains(&s), "{s:?} never comes up");
        }
        assert_eq!(scene(100.0), scene(100.0 + script_len()));
    }

    #[test]
    fn scene_values_glide() {
        let f = |s| if s == Scene::Building { 1.0 } else { 0.0 };
        for i in 0..2000 {
            let t = i as f64;
            assert!((by_scene(t, 20.0, f) - by_scene(t + 1.0, 20.0, f)).abs() < 0.2);
        }
    }
}

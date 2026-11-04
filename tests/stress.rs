//! The dashboard used hard by keyboard alone, on the mock system's made-up data: random keys,
//! weighted by what is on screen, with the dashboard checked after each one (see
//! `src/app/stress.rs`). Headless and as fast as it goes; `niri-dash stress` runs the same on
//! screen, four keys a second, to watch.
//!
//! Nothing on the system changes: the mock stands in for it, and the dashboard's files go to a
//! temporary directory.
//!
//! `NIRI_DASH_STRESS_SEEDS=1,2,3` picks the seeds, `NIRI_DASH_STRESS_OPS=2000` the keys per seed.

use niri_dash::testing::{Harness, Monkey, Sandbox, default_config};
use std::panic::AssertUnwindSafe;

fn env_or<T: std::str::FromStr>(var: &str, default: T) -> T {
    std::env::var(var)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn main() {
    let _sandbox = Sandbox::enter(None).expect("a temporary directory for the dashboard's files");
    // SAFETY: nothing else runs yet that could read the environment.
    unsafe { std::env::set_var("LC_ALL", "C") };
    let seeds: Vec<u64> = std::env::var("NIRI_DASH_STRESS_SEEDS")
        .ok()
        .map(|s| s.split(',').filter_map(|n| n.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![1, 2, 3]);
    let ops = env_or("NIRI_DASH_STRESS_OPS", 500);
    let config = default_config();
    let mut failed = 0;
    for &seed in &seeds {
        let run = std::panic::catch_unwind(AssertUnwindSafe(|| {
            Harness::with_mock(&config)
                .stress(Monkey::new(seed, false).keep_going(), ops, 0.0)
                .map(|monkey| (monkey.summary(), monkey.finish()))
        }));
        let run = run.map(|r| match r {
            Ok((summary, Ok(_))) => Ok(summary),
            Ok((_, Err(failure))) | Err(failure) => Err(failure),
        });
        match run {
            Ok(Ok(summary)) => println!("test seed {seed} ... ok: {summary}"),
            Ok(Err(failure)) => {
                failed += 1;
                println!("test seed {seed} ... FAILED\n{failure}");
            }
            Err(_) => {
                failed += 1;
                println!("test seed {seed} ... FAILED: panicked");
            }
        }
    }
    println!(
        "\ntest result: {}. {} passed; {failed} failed",
        if failed == 0 { "ok" } else { "FAILED" },
        seeds.len() - failed
    );
    if failed > 0 {
        std::process::exit(1);
    }
}

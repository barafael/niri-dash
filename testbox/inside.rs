//! Whether this program runs in the test box, for the tests that run system code: the real
//! system (`live()`), and the programs a screen reader test starts (a bus, Orca, KWin). Those
//! change the machine they run on, so they run in the box and nowhere else, however they are
//! started, and say so when they skip.
//!
//! Included by each of them (`#[path = ...] mod inside;`) rather than made a crate, so there is
//! one definition. Both files are ones the box has and a machine doesn't: Podman writes
//! `/run/.containerenv` into its containers, and the box's image has the marker (Containerfile).
//! An environment variable would be no proof: it can be set anywhere.

use std::path::Path;

pub fn inside() -> bool {
    Path::new("/run/.containerenv").exists() && Path::new("/etc/niri-dash-testbox").exists()
}

/// What a test prints, and succeeds, where it must not run. (Unused where a test has something
/// to check outside the box too.)
#[allow(dead_code)]
pub fn skipped(test: &str) {
    println!(
        "test {test}: skipped, it runs system code, so it only runs in the test box \
         (`cargo testbox test`)\n\ntest result: ok. 0 passed; 0 failed"
    );
}

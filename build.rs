//! Compiles the Slint UI, and writes down what this build is made of for the niri-dash info
//! widget (`src/build_info.rs`): `cargo:rustc-env` variables named `NIRI_DASH_*`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let config = slint_build::CompilerConfiguration::new().with_style("cosmic-dark".into());
    slint_build::compile_with_config("ui/app.slint", config).expect("slint build failed");
    build_info();
}

fn set(name: &str, value: &str) {
    // A value with a line break would end the instruction early.
    let value = value.replace(['\n', '\r'], " ");
    println!("cargo:rustc-env=NIRI_DASH_{name}={value}");
}

fn build_info() {
    let env = |name: &str| std::env::var(name).unwrap_or_default();
    let manifest_dir = PathBuf::from(env("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env("OUT_DIR"));

    set("SLINT_VERSION", &slint_version(&out_dir));
    set("PROFILE", &env("PROFILE"));
    set("OPT_LEVEL", &env("OPT_LEVEL"));
    set("TARGET", &env("TARGET"));
    set("BUILD_DIR", &build_dir(&out_dir).to_string_lossy());
    let rustc = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_default();
    set("RUSTC", rustc.trim());

    // A package from a registry has no .git; `cargo package` wrote down the commit it came from.
    let vcs_info = std::fs::read_to_string(manifest_dir.join(".cargo_vcs_info.json")).ok();
    let packaged = vcs_info.is_some();
    let from_crates_io = packaged
        && manifest_dir.components().any(|c| {
            c.as_os_str()
                .to_string_lossy()
                .starts_with("index.crates.io-")
        });
    set("FROM_CRATES_IO", if from_crates_io { "1" } else { "" });

    let git = if packaged {
        None
    } else {
        Git::find(&manifest_dir)
    };
    let (commit, dirty, branch, date) = match (&git, &vcs_info) {
        (Some(git), _) => {
            git.watch();
            (
                git.run(&["rev-parse", "HEAD"]),
                git.run(&["status", "--porcelain", "--untracked-files=no"])
                    .map(|s| !s.is_empty()),
                git.run(&["symbolic-ref", "--short", "-q", "HEAD"]),
                git.run(&["log", "-1", "--format=%cs"]),
            )
        }
        (None, Some(json)) => (
            json_string(json, "sha1"),
            Some(json.contains("\"dirty\": true") || json.contains("\"dirty\":true")),
            None,
            None,
        ),
        (None, None) => (None, None, None, None),
    };
    set("GIT_COMMIT", commit.as_deref().unwrap_or(""));
    set("GIT_DIRTY", if dirty == Some(true) { "1" } else { "" });
    set("GIT_BRANCH", branch.as_deref().unwrap_or(""));
    set("GIT_DATE", date.as_deref().unwrap_or(""));
}

/// The Slint the UI was compiled for: the generated code names it in a check that it is the same
/// as the runtime's (`VersionCheck_1_18_1`), so this can't be another version than the one linked.
fn slint_version(out_dir: &Path) -> String {
    let generated = std::fs::read_to_string(out_dir.join("app.rs")).unwrap_or_default();
    let Some(at) = generated.find("VersionCheck_") else {
        return String::new();
    };
    generated[at + "VersionCheck_".len()..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '_')
        .collect::<String>()
        .trim_matches('_')
        .replace('_', ".")
}

/// Where cargo builds this profile: OUT_DIR is `<build dir>/[<target>/]<profile>/build/<pkg>-<hash>/out`.
fn build_dir(out_dir: &Path) -> PathBuf {
    out_dir
        .ancestors()
        .find(|p| p.file_name().is_some_and(|n| n == "build"))
        .and_then(Path::parent)
        .unwrap_or(out_dir)
        .to_path_buf()
}

/// A string field of a small, flat JSON object, without a JSON parser.
fn json_string(json: &str, key: &str) -> Option<String> {
    let rest = &json[json.find(&format!("\"{key}\""))? + key.len() + 2..];
    let rest = &rest[rest.find('"')? + 1..];
    Some(rest[..rest.find('"')?].to_string())
}

/// The git checkout the crate is built from.
struct Git {
    dir: PathBuf,
}

impl Git {
    fn find(dir: &Path) -> Option<Git> {
        let git = Git {
            dir: dir.to_path_buf(),
        };
        git.run(&["rev-parse", "--git-dir"])?;
        Some(git)
    }

    fn run(&self, args: &[&str]) -> Option<String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.dir)
            .args(args)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Run the build script again when HEAD moves (a commit, a checkout) or what is staged
    /// changes. Not on every edit: that would recompile the UI each time. So the dirty flag is as
    /// of the last commit, checkout or `git add`.
    fn watch(&self) {
        let path = |args: &[&str]| self.run(args).map(|p| self.dir.join(p));
        let mut watched = vec![
            path(&["rev-parse", "--git-path", "HEAD"]),
            path(&["rev-parse", "--git-path", "index"]),
            path(&["rev-parse", "--git-path", "packed-refs"]),
        ];
        if let Some(branch) = self.run(&["symbolic-ref", "-q", "HEAD"]) {
            watched.push(path(&["rev-parse", "--git-path", &branch]));
        }
        // A path that doesn't exist would run the script on every build.
        for p in watched.into_iter().flatten().filter(|p| p.exists()) {
            println!("cargo:rerun-if-changed={}", p.display());
        }
    }
}

//! `cargo testbox`, a Cargo alias of this xtask (.cargo/config.toml): runs the tests that run
//! system code in a Podman container, the test box (testbox/README.md).
//!
//! Those tests (the real system, a private bus, Orca and KWin) change the machine they run on, so
//! they refuse to run anywhere else (testbox/inside.rs). This is where they do run: the test
//! binaries are built here, on the host, and only run in the box, which has no network, no
//! capabilities and a read-only root, and sees nothing of the host but those binaries.

use clap::{Parser, Subcommand, ValueEnum};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const IMAGE: &str = "localhost/niri-dash-testbox";

/// Runs the tests that run system code in a Podman container, and not on this machine.
///
/// The box has no network, drops every capability, and mounts only the test binaries. Needs
/// podman (CONTAINER_ENGINE picks another) and cargo.
#[derive(Parser)]
#[command(bin_name = "cargo testbox")]
struct Cli {
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Builds the image and the test binaries, and runs the phases, each test in a box of its own.
    /// Every phase if none is named.
    Test {
        phases: Vec<Phase>,
        /// Only the scenarios of the `system` phase's `live` test with this in their name.
        #[arg(long)]
        only: Option<String>,
    },
    /// Builds the image only.
    Build,
    /// A shell in the box, with the phases' test binaries in /test, for looking into a failure.
    Shell,
}

/// What `test` runs, by what each needs from the box.
#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Phase {
    /// The real system against mocks of NetworkManager, BlueZ and UPower, and the contract every
    /// system keeps (crates/system/tests).
    System,
    /// The (Slint) dashboard on a private accessibility bus, read by libatspi.
    Atspi,
    /// Orca reading the (Slint) dashboard, with KWin as its display.
    Orca,
    /// The GPUI dashboard on a private accessibility bus, read by libatspi.
    GpuiAtspi,
    /// Orca reading the GPUI dashboard, with KWin as its display.
    GpuiOrca,
}

/// A test program: the package it is in, its name, the arguments it is run with, the programs of
/// the package it starts (the dashboard itself, for the screen reader tests, which read what the
/// real window publishes) and the files of the source tree it reads beside itself.
struct Test {
    package: &'static str,
    name: &'static str,
    args: &'static [&'static str],
    bins: &'static [&'static str],
    files: &'static [&'static str],
}

impl Phase {
    fn tests(self) -> &'static [Test] {
        match self {
            Self::System => &[
                Test {
                    package: "niri-dash-system",
                    name: "live",
                    args: &[],
                    bins: &[],
                    files: &[],
                },
                // One at a time: each starts the live system, and two share one bus.
                Test {
                    package: "niri-dash-system",
                    name: "contract",
                    args: &["--test-threads=1"],
                    bins: &[],
                    files: &[],
                },
            ],
            Self::Atspi => &[Test {
                package: "niri-dash",
                name: "atspi",
                args: &[],
                bins: &[],
                files: &["tests/atspi_client.py"],
            }],
            Self::Orca => &[Test {
                package: "niri-dash",
                name: "orca",
                args: &[],
                bins: &[],
                files: &[],
            }],
            Self::GpuiAtspi => &[Test {
                package: "niri-dash-gpui",
                name: "atspi",
                args: &[],
                bins: &["niri-dash-gpui"],
                files: &["gpui/tests/gpui_atspi_client.py"],
            }],
            Self::GpuiOrca => &[Test {
                package: "niri-dash-gpui",
                name: "orca",
                args: &[],
                bins: &["niri-dash-gpui"],
                files: &[],
            }],
        }
    }

    fn name(self) -> String {
        self.to_possible_value()
            .map(|v| v.get_name().to_string())
            .unwrap_or_default()
    }
}

type Result<T> = std::result::Result<T, String>;

fn main() -> ExitCode {
    match run(Cli::parse().command) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(why) => {
            eprintln!("cargo testbox: {why}");
            ExitCode::FAILURE
        }
    }
}

fn run(command: Action) -> Result<bool> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("no repository above the xtask")?
        .to_path_buf();
    let engine = std::env::var("CONTAINER_ENGINE").unwrap_or_else(|_| "podman".into());
    match command {
        Action::Build => {
            build_image(&engine, &root)?;
            Ok(true)
        }
        Action::Test { phases, only } => test(&engine, &root, phases, only),
        Action::Shell => shell(&engine, &root),
    }
}

fn build_image(engine: &str, root: &Path) -> Result<()> {
    let dir = root.join("testbox");
    let status = Command::new(engine)
        .args(["build", "-q", "-t", IMAGE, "-f"])
        .arg(dir.join("Containerfile"))
        .arg(&dir)
        .stdout(Stdio::null())
        .status()
        .map_err(|e| format!("{engine}: {e}"))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("building the image: {status}"))
}

/// What `build_tests` built: the test programs, and the package's own programs they start.
struct Built {
    tests: Vec<(String, PathBuf)>,
    bins: Vec<(String, PathBuf)>,
}

/// Builds `package`'s test targets `names`, and where each program is. Cargo builds the package's
/// own programs for its integration tests; `bins` are the ones to hand back.
fn build_tests(root: &Path, package: &str, names: &[&str], bins: &[&str]) -> Result<Built> {
    let mut cargo = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cargo.current_dir(root).args([
        "test",
        "--locked",
        "--no-run",
        "--message-format=json",
        "-p",
        package,
    ]);
    // The GPUI dashboard's test programs are built on its headless harness, behind this feature,
    // and with the profile that optimises its dependencies.
    if package == "niri-dash-gpui" {
        cargo.args(["--features", "testing", "--profile", "gpui"]);
    }
    for name in names {
        cargo.args(["--test", name]);
    }
    let output = cargo
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("cargo: {e}"))?;
    if !output.status.success() {
        return Err(format!("building {package}'s tests: {}", output.status));
    }
    let mut found = Vec::new();
    let mut programs = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let (Some(name), Some(path)) = (
            message["target"]["name"].as_str(),
            message["executable"].as_str(),
        ) else {
            continue;
        };
        let kind = |what: &str| {
            message["target"]["kind"]
                .as_array()
                .is_some_and(|k| k.iter().any(|k| k == what))
        };
        if kind("test") && names.contains(&name) {
            found.push((name.to_string(), PathBuf::from(path)));
        } else if kind("bin") && bins.contains(&name) {
            programs.push((name.to_string(), PathBuf::from(path)));
        }
    }
    for name in names {
        if !found.iter().any(|(n, _)| n == name) {
            return Err(format!("cargo didn't build the {name} test"));
        }
    }
    for name in bins {
        if !programs.iter().any(|(n, _)| n == name) {
            return Err(format!("cargo didn't build the {name} program"));
        }
    }
    Ok(Built {
        tests: found,
        bins: programs,
    })
}

/// The directory the box mounts: the test programs, and the files they read beside themselves.
struct Stage {
    dir: PathBuf,
    /// Each test's name, and the name of its program in `dir`.
    programs: Vec<(String, String)>,
}

impl Stage {
    fn new(root: &Path) -> Result<Self> {
        let target =
            std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from);
        let dir = target.join("testbox-stage");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        // The box's user isn't this one: it needs to read and run what is here.
        set_mode(&dir, 0o755)?;
        Ok(Self {
            dir,
            programs: Vec::new(),
        })
    }

    /// Where a test's program is in the box.
    fn program(&self, test: &Test) -> String {
        let staged = self
            .programs
            .iter()
            .find(|(name, _)| name == test.name)
            .map_or(test.name, |(_, staged)| staged);
        format!("/test/{staged}")
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn stage_tests(root: &Path, phases: &[Phase]) -> Result<Stage> {
    let mut stage = Stage::new(root)?;
    let tests: Vec<&Test> = phases.iter().flat_map(|p| p.tests()).collect();
    for package in ["niri-dash-system", "niri-dash"] {
        let mine: Vec<&Test> = tests
            .iter()
            .copied()
            .filter(|t| t.package == package)
            .collect();
        if mine.is_empty() {
            continue;
        }
        let names: Vec<&str> = mine.iter().map(|t| t.name).collect();
        let bins: Vec<&str> = mine.iter().flat_map(|t| t.bins).copied().collect();
        let built = build_tests(root, package, &names, &bins)?;
        // The programs the tests start, beside them under their own names: the screen reader
        // tests read what the real dashboard window publishes.
        for (name, path) in built.bins {
            let to = stage.dir.join(&name);
            std::fs::copy(&path, &to).map_err(|e| format!("{}: {e}", path.display()))?;
            set_mode(&to, 0o755)?;
        }
        for (name, path) in built.tests {
            // Under the name cargo gave it (`orca-<hash>`), not the test's own: Orca won't start
            // beside another process called `orca` (`pgrep -x orca`), and this would be one.
            let staged = path
                .file_name()
                .map_or_else(|| name.clone(), |n| n.to_string_lossy().into_owned());
            let to = stage.dir.join(&staged);
            std::fs::copy(&path, &to).map_err(|e| format!("{}: {e}", path.display()))?;
            set_mode(&to, 0o755)?;
            stage.programs.push((name, staged));
        }
    }
    for file in tests.iter().flat_map(|t| t.files) {
        let from = root.join(file);
        let to = stage
            .dir
            .join(from.file_name().ok_or("a file without a name")?);
        std::fs::copy(&from, &to).map_err(|e| format!("{}: {e}", from.display()))?;
        set_mode(&to, 0o644)?;
    }
    Ok(stage)
}

/// `podman run` as every box is run: no network, no capabilities, no new privileges, a read-only
/// root with a scratch home, and the test programs mounted read-only.
fn boxed(engine: &str, stage: &Stage) -> Command {
    let mut run = Command::new(engine);
    run.args([
        "run",
        "--rm",
        "--network=none",
        "--cap-drop=all",
        "--security-opt=no-new-privileges",
        "--read-only",
        "--mount=type=tmpfs,destination=/home/box,tmpfs-mode=1777",
        "-v",
    ])
    .arg(format!("{}:/test:ro,Z", stage.dir.display()));
    run
}

fn test(engine: &str, root: &Path, phases: Vec<Phase>, only: Option<String>) -> Result<bool> {
    let phases = if phases.is_empty() {
        Phase::value_variants().to_vec()
    } else {
        phases
    };
    build_image(engine, root)?;
    let stage = stage_tests(root, &phases)?;
    let mut failed = Vec::new();
    let mut passed = 0;
    for phase in &phases {
        for test in phase.tests() {
            println!("\n== {} / {}", phase.name(), test.name);
            let mut run = boxed(engine, &stage);
            if let Some(only) = &only {
                run.args(["-e", &format!("NIRI_DASH_SYSTEM_ONLY={only}")]);
            }
            let status = run
                .arg(IMAGE)
                .arg(stage.program(test))
                .args(test.args)
                .status()
                .map_err(|e| format!("{engine}: {e}"))?;
            if status.success() {
                passed += 1;
            } else {
                failed.push(format!("{} / {}", phase.name(), test.name));
            }
        }
    }
    println!("\ntestbox: {passed} passed, {} failed", failed.len());
    for name in &failed {
        println!("  failed: {name}");
    }
    Ok(failed.is_empty())
}

fn shell(engine: &str, root: &Path) -> Result<bool> {
    build_image(engine, root)?;
    let stage = stage_tests(root, Phase::value_variants())?;
    let status = boxed(engine, &stage)
        .args([
            "-it",
            "--entrypoint",
            "/usr/local/bin/mocks.py",
            IMAGE,
            "bash",
        ])
        .status()
        .map_err(|e| format!("{engine}: {e}"))?;
    Ok(status.success())
}

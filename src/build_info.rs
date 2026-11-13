//! What this build is: its version, where it came from, how and where it was built, and what it
//! draws with. `build.rs` finds these out; the niri-dash info widget shows them.

/// The UI toolkit the dashboard is made with.
pub const TOOLKIT: &str = "Slint";
pub const SLINT_VERSION: &str = env!("NIRI_DASH_SLINT_VERSION");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const NAME: &str = env!("CARGO_PKG_NAME");

/// Built from the crates.io package (`cargo install niri-dash`), not a git checkout.
const FROM_CRATES_IO: bool = !env!("NIRI_DASH_FROM_CRATES_IO").is_empty();
const COMMIT: &str = env!("NIRI_DASH_GIT_COMMIT");
/// There were uncommitted changes, as of the last commit, checkout or `git add` (see build.rs).
const DIRTY: bool = !env!("NIRI_DASH_GIT_DIRTY").is_empty();
const BRANCH: &str = env!("NIRI_DASH_GIT_BRANCH");
const COMMIT_DATE: &str = env!("NIRI_DASH_GIT_DATE");
const PROFILE: &str = env!("NIRI_DASH_PROFILE");
const OPT_LEVEL: &str = env!("NIRI_DASH_OPT_LEVEL");
const TARGET: &str = env!("NIRI_DASH_TARGET");
const RUSTC: &str = env!("NIRI_DASH_RUSTC");
const SOURCE_DIR: &str = env!("CARGO_MANIFEST_DIR");
const BUILD_DIR: &str = env!("NIRI_DASH_BUILD_DIR");

/// What clicking a fact does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    None,
    /// A web page.
    Url(String),
    /// A folder on this machine.
    Dir(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub label: &'static str,
    pub value: String,
    pub link: Link,
}

impl Fact {
    fn new(label: &'static str, value: impl Into<String>, link: Link) -> Fact {
        Fact {
            label,
            value: value.into(),
            link,
        }
    }
}

/// "Slint 1.18.1", the toolkit and the version the UI was compiled with.
pub fn toolkit() -> String {
    format!("{TOOLKIT} {SLINT_VERSION}").trim().to_string()
}

/// The facts, in the order the widget lists them. `renderer` is what draws the dashboard ("OpenGL
/// ES", "software"), which is only known once it runs; `home` shortens paths under it to `~`.
pub fn facts(renderer: &str, home: Option<&str>) -> Vec<Fact> {
    let repo = REPOSITORY.trim_end_matches('/');
    let shown_path = |p: &str| match home {
        Some(h) if !h.is_empty() && p.starts_with(h) && p[h.len()..].starts_with('/') => {
            format!("~{}", &p[h.len()..])
        }
        _ => p.to_string(),
    };
    let mut facts = vec![Fact::new(
        "UI",
        format!("{}, {renderer}", toolkit()),
        Link::Url("https://slint.dev".into()),
    )];
    if !repo.is_empty() {
        facts.push(Fact::new(
            "Source",
            repo.trim_start_matches("https://"),
            Link::Url(repo.to_string()),
        ));
    }
    if FROM_CRATES_IO {
        facts.push(Fact::new(
            "crates.io",
            format!("{NAME} {VERSION}"),
            Link::Url(format!("https://crates.io/crates/{NAME}/{VERSION}")),
        ));
    }
    if !COMMIT.is_empty() {
        let short = &COMMIT[..COMMIT.len().min(9)];
        let mut value = short.to_string();
        if !COMMIT_DATE.is_empty() {
            value += &format!(", {COMMIT_DATE}");
        }
        if DIRTY {
            value += ", with changes";
        }
        let link = if repo.contains("github.com") {
            Link::Url(format!("{repo}/commit/{COMMIT}"))
        } else {
            Link::None
        };
        facts.push(Fact::new("Commit", value, link));
    }
    if !BRANCH.is_empty() {
        facts.push(Fact::new("Branch", BRANCH, Link::None));
    }
    let profile = match OPT_LEVEL {
        "" => PROFILE.to_string(),
        level => format!("{PROFILE}, opt-level {level}"),
    };
    facts.push(Fact::new("Profile", profile, Link::None));
    facts.push(Fact::new("Target", TARGET, Link::None));
    if !RUSTC.is_empty() {
        // "rustc 1.95.0 (59807616e 2026-04-14)": the version is enough.
        let version = RUSTC
            .split_whitespace()
            .take(2)
            .collect::<Vec<_>>()
            .join(" ");
        facts.push(Fact::new("Compiler", version, Link::None));
    }
    // A browser has no folders to open, and the paths are the build machine's.
    let dir = |p: &str| {
        if cfg!(target_arch = "wasm32") {
            Link::None
        } else {
            Link::Dir(p.to_string())
        }
    };
    facts.push(Fact::new(
        "Checkout",
        shown_path(SOURCE_DIR),
        dir(SOURCE_DIR),
    ));
    facts.push(Fact::new(
        "Build dir",
        shown_path(BUILD_DIR),
        dir(BUILD_DIR),
    ));
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(exe) = std::env::current_exe() {
        let shown = shown_path(&exe.to_string_lossy());
        let folder = exe.parent().map(|p| p.to_string_lossy().into_owned());
        facts.push(Fact::new(
            "Binary",
            shown,
            folder.map_or(Link::None, Link::Dir),
        ));
    }
    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slint_version_is_found() {
        let parts: Vec<_> = SLINT_VERSION.split('.').collect();
        assert_eq!(parts.len(), 3, "{SLINT_VERSION:?}");
        assert!(parts.iter().all(|p| p.parse::<u32>().is_ok()));
        assert!(toolkit().starts_with("Slint 1."));
    }

    #[test]
    fn facts_say_what_draws_and_link_the_repository() {
        let facts = facts("software", None);
        assert_eq!(facts[0].label, "UI");
        assert!(facts[0].value.ends_with(", software"), "{:?}", facts[0]);
        let source = facts.iter().find(|f| f.label == "Source").unwrap();
        assert_eq!(source.value, "github.com/barafael/niri-dash");
        assert_eq!(
            source.link,
            Link::Url("https://github.com/barafael/niri-dash".into())
        );
        assert!(
            facts
                .iter()
                .any(|f| f.label == "Profile" && !f.value.is_empty())
        );
    }

    #[test]
    fn paths_under_home_are_shortened() {
        let home = SOURCE_DIR.rsplit_once('/').unwrap().0;
        let checkout = facts("software", Some(home))
            .into_iter()
            .find(|f| f.label == "Checkout")
            .unwrap();
        assert!(checkout.value.starts_with("~/"), "{checkout:?}");
        assert_eq!(checkout.link, Link::Dir(SOURCE_DIR.into()));
    }
}

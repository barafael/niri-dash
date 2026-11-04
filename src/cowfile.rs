//! The cow mascot's drawing: our own, or a `.cow` file from the user's `cowsay` install.
//!
//! No cowsay cow ships with niri-dash (they aren't under this project's licenses): a `.cow` file is
//! only read at run time, from where cowsay keeps them or from a path, when the widget's `cowfile`
//! setting names one. Without such a file the widget draws our own cow of that name, from
//! `assets/cows/`, drawn for this project.
//!
//! A `.cow` file is a Perl script. It is read, never run: only the text of its `$the_cow` here-doc
//! is used, with `$thoughts`, `$eyes` and `$tongue` filled in. Whatever Perl comes before that is
//! ignored (`three-eyes.cow` computes a third eye there, and gets two), and a drawing that uses
//! anything else Perl would expand is refused.

use std::path::{Path, PathBuf};

/// Where cowsay keeps its cows, after what `COWPATH` says.
const COW_DIRS: &[&str] = &[
    "/usr/share/cowsay/cows",
    "/usr/local/share/cowsay/cows",
    "/usr/share/cows",
];

/// Larger than any real cow; a path to something else shouldn't end up on screen.
const MAX_BYTES: u64 = 64 * 1024;
const MAX_LINES: usize = 60;
const MAX_WIDTH: usize = 160;

/// Our own cows, for when cowsay has no cow of that name (or isn't installed). They carry names
/// cowsay's cows have, so a config means the same animal with or without it. `default` is the one
/// the widget shows when no cow is named.
const OWN_COWS: &[(&str, &str)] = &[
    ("default", include_str!("../assets/cows/default.cow")),
    ("bunny", include_str!("../assets/cows/bunny.cow")),
    ("dragon", include_str!("../assets/cows/dragon.cow")),
    ("moose", include_str!("../assets/cows/moose.cow")),
    ("sheep", include_str!("../assets/cows/sheep.cow")),
    ("tux", include_str!("../assets/cows/tux.cow")),
];

/// Our own cow of this name.
fn own_named(name: &str) -> Option<Cow> {
    let name = name.strip_suffix(".cow").unwrap_or(name);
    OWN_COWS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, text)| Cow::parse(text).expect("our own cows parse"))
}

#[derive(Debug, Clone, PartialEq)]
enum Piece {
    Text(String),
    Thoughts,
    Eyes,
    Tongue,
}

/// A drawing with its blanks.
#[derive(Debug, Clone, PartialEq)]
pub struct Cow {
    pieces: Vec<Piece>,
}

impl Cow {
    /// The cow shown when none is named.
    pub fn own() -> Self {
        own_named("default").expect("we have a default cow")
    }

    /// Read a `.cow` file's text. The error says why it can't be used.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text.lines();
        let (terminator, raw) = lines
            .by_ref()
            .find_map(heredoc_start)
            .ok_or("it has no $the_cow here-doc")?;
        let mut body = Vec::new();
        for line in lines.by_ref() {
            if line.trim_end() == terminator {
                // In a <<'EOC' here-doc Perl expands nothing.
                let pieces = if raw {
                    vec![Piece::Text(body.join("\n"))]
                } else {
                    parse_interpolated(&body.join("\n"))?
                };
                let cow = Self { pieces };
                let (w, h) = cow.size();
                if w > MAX_WIDTH || h > MAX_LINES {
                    return Err(format!("it is too big ({w}x{h})"));
                }
                return Ok(cow);
            }
            body.push(line);
        }
        Err("its here-doc never ends".into())
    }

    /// The drawing with its blanks filled in; a line that says something uses a backslash.
    pub fn draw(&self, eyes: &str, tongue: &str) -> String {
        let mut out = String::new();
        for piece in &self.pieces {
            match piece {
                Piece::Text(t) => out.push_str(t),
                Piece::Thoughts => out.push('\\'),
                Piece::Eyes => out.push_str(eyes),
                Piece::Tongue => out.push_str(tongue),
            }
        }
        out
    }

    /// Its size in (columns, lines), with two-letter eyes and tongue.
    pub fn size(&self) -> (usize, usize) {
        let drawn = self.draw("oo", "  ");
        let width = drawn.lines().map(|l| l.chars().count()).max().unwrap_or(0);
        (width, drawn.lines().count())
    }
}

/// If this line starts the drawing: what ends it, and whether Perl would leave its text alone.
fn heredoc_start(line: &str) -> Option<(String, bool)> {
    let line = line.trim();
    if line.starts_with('#') {
        return None;
    }
    let (_, rest) = line.split_once("$the_cow")?;
    let (_, rest) = rest.split_once("<<")?;
    let rest = rest.trim_start();
    let (raw, rest) = match rest.chars().next()? {
        '\'' => (true, &rest[1..]),
        '"' => (false, &rest[1..]),
        _ => (false, rest),
    };
    let end = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    (end > 0).then(|| (rest[..end].to_string(), raw))
}

fn push_text(pieces: &mut Vec<Piece>, text: &mut String) {
    if !text.is_empty() {
        pieces.push(Piece::Text(std::mem::take(text)));
    }
}

/// What a blank's name stands for.
fn blank(name: &str) -> Option<Piece> {
    match name {
        "thoughts" => Some(Piece::Thoughts),
        "eyes" => Some(Piece::Eyes),
        "tongue" => Some(Piece::Tongue),
        _ => None,
    }
}

/// A plain here-doc, as Perl would read it: `\\` is a backslash, `\$` and `\@` the character, and
/// `$thoughts`, `${eyes}` and so on the blanks. Anything else Perl would expand is refused.
fn parse_interpolated(body: &str) -> Result<Vec<Piece>, String> {
    let mut pieces = Vec::new();
    let mut text = String::new();
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(e @ ('\\' | '$' | '@' | '"' | '\'' | '/' | '`' | ' ')) => text.push(e),
                Some('n') => text.push('\n'),
                Some(other) => return Err(format!("it uses the Perl escape \\{other}")),
                None => text.push('\\'),
            },
            '$' => {
                let braced = chars.next_if_eq(&'{').is_some();
                let mut name = String::new();
                while let Some(n) = chars.next_if(|n| n.is_ascii_alphanumeric() || *n == '_') {
                    name.push(n);
                }
                if braced && chars.next() != Some('}') {
                    return Err("it uses a Perl expression".into());
                }
                if name.is_empty() {
                    // A `$` that Perl leaves alone, as before a space.
                    text.push('$');
                    continue;
                }
                let piece =
                    blank(&name).ok_or_else(|| format!("it uses the Perl variable ${name}"))?;
                push_text(&mut pieces, &mut text);
                pieces.push(piece);
            }
            '@' if chars.peek().is_some_and(|n| n.is_ascii_alphabetic()) => {
                return Err("it uses a Perl array".into());
            }
            c if c.is_control() && c != '\n' && c != '\t' => {
                return Err("it has control characters".into());
            }
            c => text.push(c),
        }
    }
    push_text(&mut pieces, &mut text);
    Ok(pieces)
}

/// The system's monospace font family, for drawing cows. Slint looks fonts up by their real names,
/// so "monospace" alone, fontconfig's alias, finds nothing and the drawing falls apart.
pub fn monospace_family() -> String {
    std::process::Command::new("fc-match")
        .args(["-f", "%{family[0]}", "monospace"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|family| family.trim().to_string())
        .filter(|family| !family.is_empty())
        .unwrap_or_else(|| "monospace".to_string())
}

/// Where `name` is: a path if it looks like one, else a cow in `dirs`.
fn find_in(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    if name.contains('/') {
        let path = match name.strip_prefix("~/") {
            Some(rest) => dirs::home_dir()?.join(rest),
            None => PathBuf::from(name),
        };
        return path.is_file().then_some(path);
    }
    let file = if name.ends_with(".cow") {
        name.to_string()
    } else {
        format!("{name}.cow")
    };
    dirs.iter()
        .map(|dir| dir.join(&file))
        .find(|path| path.is_file())
}

/// `COWPATH`, then the usual places.
fn search_path() -> Vec<PathBuf> {
    let from_env = std::env::var_os("COWPATH");
    from_env
        .iter()
        .flat_map(std::env::split_paths)
        .chain(COW_DIRS.iter().map(PathBuf::from))
        .collect()
}

fn read(path: &Path) -> Result<Cow, String> {
    let shown = path.display();
    let size = std::fs::metadata(path)
        .map_err(|e| format!("{shown}: {e}"))?
        .len();
    if size > MAX_BYTES {
        return Err(format!("{shown} is too big for a cow file"));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{shown}: {e}"))?;
    Cow::parse(&String::from_utf8_lossy(&bytes)).map_err(|why| format!("Can't use {shown}: {why}"))
}

/// The cow the `cowfile` setting names: from cowsay's cows if it is there and can be used, else
/// ours of that name. Nothing named: our own. An error is what to tell the user.
pub fn load(name: &str) -> Result<Cow, String> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(Cow::own());
    }
    let found = find_in(name, &search_path()).map(|path| read(&path));
    match found {
        Some(Ok(cow)) => Ok(cow),
        Some(Err(why)) => own_named(name).ok_or(why),
        None if name.contains('/') => Err(format!("No cow file at \u{201c}{name}\u{201d}")),
        None => own_named(name).ok_or_else(|| {
            let ours: Vec<&str> = OWN_COWS.iter().map(|(n, _)| *n).collect();
            format!(
                "No cow called \u{201c}{name}\u{201d}. Built in: {}; cowsay brings more.",
                ours.join(", ")
            )
        }),
    }
}

/// What to say about the `cowfile` setting, if it can't be used.
pub fn complaint(name: &str) -> Option<String> {
    load(name).err()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Drawings made up for the tests, in the shapes real cow files come in.
    const PLAIN: &str = "##\n## a test animal\n##\n$the_cow = <<EOC;\n  $thoughts\n (${eyes})\\\\\n $tongue|\\$5 \\@ home\nEOC\n";
    const QUOTED: &str = "$the_cow = <<\"EOC\";\n$thoughts $eyes\nEOC\n";

    #[test]
    fn our_own_cow_has_all_its_blanks() {
        let cow = Cow::own();
        let drawn = cow.draw("oo", "UU");
        assert!(drawn.contains("( oo )"), "{drawn}");
        assert!(drawn.contains("UU"), "{drawn}");
        assert!(drawn.lines().next().unwrap().contains('\\'), "{drawn}");
        assert!(!drawn.contains('$'), "{drawn}");
        assert_eq!(cow.size().1, 6);
    }

    #[test]
    fn our_own_cows_all_work_and_are_our_own() {
        for (name, text) in OWN_COWS {
            let cow = Cow::parse(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            let drawn = cow.draw("@@", "UU");
            assert!(drawn.contains("@@"), "{name} has no eyes:\n{drawn}");
            assert!(drawn.contains("UU"), "{name} has no tongue:\n{drawn}");
            assert!(
                drawn.lines().next().unwrap().contains('\\'),
                "{name}:\n{drawn}"
            );
            assert!(!drawn.contains('$'), "{name}:\n{drawn}");
            assert!(
                cow.size().0 <= 50 && cow.size().1 <= 8,
                "{name}: {:?}",
                cow.size()
            );
            // cowsay's own cow isn't ours to ship.
            assert!(!drawn.contains("||----w |"), "{name}");
        }
    }

    #[test]
    fn a_named_cow_falls_back_to_ours() {
        // With nothing installed under these names in the test's search path.
        assert_eq!(own_named("tux"), own_named("tux.cow"));
        assert!(own_named("tux").is_some());
        assert!(own_named("no-such-animal").is_none());
        let said = complaint("surely-no-such-cow").unwrap();
        assert!(said.contains("Built in: default, bunny"), "{said}");
    }

    #[test]
    fn a_cow_file_is_read_as_perl_would() {
        let cow = Cow::parse(PLAIN).unwrap();
        assert_eq!(cow.draw("xx", "U "), "  \\\n (xx)\\\n U |$5 @ home");
        assert_eq!(cow.size(), (13, 3));
        assert_eq!(Cow::parse(QUOTED).unwrap().draw("oo", ""), "\\ oo");
        let raw = Cow::parse("$the_cow = <<'EOC';\n$eyes \\\\\nEOC\n").unwrap();
        assert_eq!(raw.draw("oo", ""), "$eyes \\\\");
    }

    #[test]
    fn perl_before_the_drawing_is_ignored_not_run() {
        let cow = Cow::parse(
            "$extra = chop($eyes);\nsystem('rm -rf /');\n$the_cow = <<EOC;\n($eyes)\nEOC\n",
        );
        assert_eq!(cow.unwrap().draw("oo", ""), "(oo)");
    }

    #[test]
    fn what_only_perl_could_draw_is_refused() {
        for (text, why) in [
            ("$the_cow = <<EOC;\n$extra\nEOC\n", "$extra"),
            ("$the_cow = <<EOC;\n@things\nEOC\n", "array"),
            ("$the_cow = <<EOC;\n\\e[31m\nEOC\n", "\\e"),
            ("$the_cow = <<EOC;\n${ \\ run() }\nEOC\n", "expression"),
            ("$the_cow = <<EOC;\nnever ends\n", "never ends"),
            ("no cow here\n", "no $the_cow"),
            ("# $the_cow = <<EOC;\nEOC\n", "no $the_cow"),
        ] {
            let err = Cow::parse(text).unwrap_err();
            assert!(err.contains(why), "{text:?}: {err}");
        }
        let wide = format!("$the_cow = <<EOC;\n{}\nEOC\n", "x".repeat(MAX_WIDTH + 1));
        assert!(Cow::parse(&wide).unwrap_err().contains("too big"));
    }

    #[test]
    fn cows_are_found_by_name_or_path() {
        let dir = std::env::temp_dir().join(format!("niri-dash-cows-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("moose.cow");
        std::fs::write(&file, QUOTED).unwrap();
        let dirs = vec![PathBuf::from("/nonexistent"), dir.clone()];
        assert_eq!(find_in("moose", &dirs), Some(file.clone()));
        assert_eq!(find_in("moose.cow", &dirs), Some(file.clone()));
        assert_eq!(find_in("elk", &dirs), None);
        assert_eq!(find_in(file.to_str().unwrap(), &[]), Some(file.clone()));
        assert!(load(file.to_str().unwrap()).is_ok());
        assert!(complaint(file.to_str().unwrap()).is_none());
        std::fs::write(&file, "nothing").unwrap();
        assert!(
            complaint(file.to_str().unwrap())
                .unwrap()
                .starts_with("Can't use")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn nothing_named_is_our_own_and_a_bad_name_says_so() {
        assert_eq!(load("  ").unwrap(), Cow::own());
        assert!(complaint("").is_none());
        let said = complaint("surely-no-such-cow").unwrap();
        assert!(said.contains("surely-no-such-cow"), "{said}");
        assert!(
            complaint("/no/such/file.cow")
                .unwrap()
                .starts_with("No cow file")
        );
    }
}

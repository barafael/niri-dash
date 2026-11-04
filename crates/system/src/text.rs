//! Small things about text that every side needs.

/// A name chosen by someone else (a Bluetooth device's, a Wi-Fi network's), fit to show: without
/// control characters, and without the invisible ones that reorder or hide text (bidi
/// overrides, zero-width characters), which could make one name look like another. At most
/// 64 characters.
pub fn clean_label(s: &str) -> String {
    let invisible = |c: char| {
        c.is_control()
            || matches!(c,
                '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{2069}'
                | '\u{FEFF}')
    };
    let cleaned: String = s.chars().filter(|&c| !invisible(c)).take(64).collect();
    cleaned.trim().to_string()
}

/// Decode the five XML entities. `&amp;` goes last, so `&amp;lt;` stays `&lt;`.
pub fn unescape_entities(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Collapse whitespace and cut to `max` characters, the last of them an ellipsis.
pub fn one_line(s: &str, max: usize) -> String {
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        let mut cut: String = flat.chars().take(max - 1).collect();
        cut.push('…');
        cut
    }
}

/// A title without a leading activity spinner: terminals running Claude Code (and other tools)
/// animate the first character of their title (⠋⠙⠹, ◐◑◒◓, ✳✶✻✽…) several times a second, which
/// would otherwise make every frame of the spinner a change to show.
pub fn strip_spinner(title: &str) -> &str {
    let mut chars = title.chars();
    match chars.next() {
        Some('\u{2800}'..='\u{28FF}' | '\u{25D0}'..='\u{25D3}' | '\u{2722}'..='\u{273D}' | '·') => {
            chars.as_str().trim_start()
        }
        _ => title,
    }
}

/// The locale for dates and times.
#[derive(Debug, Clone, Copy)]
pub struct Locale {
    pub chrono: chrono::Locale,
    /// US English: 12-hour clock, weeks start on Sunday, Fahrenheit.
    pub us: bool,
}

/// The locale's name, from LC_ALL / LC_TIME / LANG.
#[cfg(not(target_arch = "wasm32"))]
fn locale_name() -> String {
    ["LC_ALL", "LC_TIME", "LANG"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default()
}

/// The locale's name, from the browser's language: "de-DE" as "de_DE".
#[cfg(target_arch = "wasm32")]
fn locale_name() -> String {
    web_sys::window()
        .and_then(|w| w.navigator().language())
        .unwrap_or_default()
        .replace('-', "_")
}

/// The locale for dates.
pub fn locale() -> Locale {
    let raw = locale_name();
    let name = raw.split(['.', '@']).next().unwrap_or("");
    let loc = chrono::Locale::try_from(name).unwrap_or(chrono::Locale::en_US);
    Locale {
        chrono: loc,
        us: name.starts_with("en_US"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn labels() {
        // A right-to-left override would show "Fairbuds" as something else; zero-width and
        // control characters hide what is there.
        assert_eq!(super::clean_label("Fair\u{202E}sdub"), "Fairsdub");
        assert_eq!(super::clean_label(" Home\u{200B}Net\n"), "HomeNet");
        assert_eq!(super::clean_label(&"x".repeat(100)).len(), 64);
        assert_eq!(super::clean_label("Café ☕"), "Café ☕");
    }

    use super::*;

    #[test]
    fn flattens_and_cuts() {
        assert_eq!(one_line("a\n  b\tc", 10), "a b c");
        assert_eq!(one_line("abcdefghij", 5), "abcd…");
    }

    #[test]
    fn entities() {
        assert_eq!(
            unescape_entities("&lt;b&gt; &amp;lt; &quot;"),
            "<b> &lt; \""
        );
    }

    #[test]
    fn spinners() {
        assert_eq!(strip_spinner("✳ Fix the build"), "Fix the build");
        assert_eq!(strip_spinner("⠙ cargo test"), "cargo test");
        assert_eq!(strip_spinner("plain title"), "plain title");
    }
}

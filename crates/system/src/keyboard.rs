//! Keyboard layouts as the keyboard-layout widget shows them, whichever compositor reports them,
//! and short labels for them. niri reports layouts by their XKB description ("English (US)",
//! "German"); the XKB rules file has the indicator text for each ("en", "de"). Plasma reports
//! short names itself.

use crate::unescape_entities;
use std::collections::HashMap;
use std::sync::OnceLock;

/// One configured layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// "EN", "DE".
    pub short: String,
    /// "English (US)".
    pub name: String,
}

/// The configured layouts and the active one (an index into `layouts`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Layouts {
    pub layouts: Vec<Layout>,
    pub current: usize,
}

impl Layouts {
    /// From XKB descriptions, labelled from the rules file.
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>, current: usize) -> Self {
        Self {
            layouts: names
                .into_iter()
                .map(|n| Layout {
                    short: short_label(n),
                    name: n.to_string(),
                })
                .collect(),
            current,
        }
    }
}

/// Which layout to switch to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Switch {
    Next,
    Prev,
    Index(u32),
}

const RULES: &[&str] = &[
    "/usr/share/X11/xkb/rules/evdev.xml",
    "/usr/share/X11/xkb/rules/base.xml",
];

/// Description → short label, from an XKB rules XML file. Variants without a label of their own
/// take their layout's.
pub fn parse_rules(xml: &str) -> HashMap<String, String> {
    fn tag<'a>(s: &'a str, name: &str) -> Option<&'a str> {
        let open = format!("<{name}>");
        let start = s.find(&open)? + open.len();
        let end = s[start..].find(&format!("</{name}>"))? + start;
        Some(s[start..end].trim())
    }
    let mut map = HashMap::new();
    let mut layout_short = String::new();
    let mut chunks = xml.split("<configItem");
    let mut before = chunks.next().unwrap_or("");
    for chunk in chunks {
        let body = chunk.split("</configItem>").next().unwrap_or("");
        let is_layout = before.trim_end().ends_with("<layout>");
        let is_variant = before.trim_end().ends_with("<variant>");
        let name = tag(body, "name").unwrap_or("");
        let short = tag(body, "shortDescription").map(str::to_string);
        if is_layout {
            layout_short = short.clone().unwrap_or_else(|| name.to_string());
        }
        if (is_layout || is_variant)
            && let Some(desc) = tag(body, "description")
        {
            let label = short.unwrap_or_else(|| layout_short.clone());
            map.insert(unescape_entities(desc), label);
        }
        before = chunk;
    }
    map
}

fn table() -> &'static HashMap<String, String> {
    static TABLE: OnceLock<HashMap<String, String>> = OnceLock::new();
    TABLE.get_or_init(|| {
        RULES
            .iter()
            .find_map(|p| std::fs::read_to_string(p).ok())
            .map(|xml| parse_rules(&xml))
            .unwrap_or_default()
    })
}

/// "English (US)" → "EN". Unknown names get their first two letters.
pub fn short_label(description: &str) -> String {
    match table().get(description) {
        Some(s) if !s.is_empty() => s.to_uppercase(),
        _ => description
            .chars()
            .filter(|c| c.is_alphanumeric())
            .take(2)
            .collect::<String>()
            .to_uppercase(),
    }
}

#[derive(Debug)]
pub enum Command {
    Switch(Switch),
}

#[derive(Debug)]
pub enum Event {
    /// None: the compositor doesn't say.
    Layouts(Option<Layouts>),
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"
<layoutList>
    <layout>
      <configItem>
        <name>us</name>
        <!-- Keyboard indicator for English layouts -->
        <shortDescription>en</shortDescription>
        <description>English (US)</description>
      </configItem>
      <variantList>
        <variant>
          <configItem>
            <name>intl</name>
            <description>English (US, intl., with dead keys)</description>
          </configItem>
        </variant>
        <variant>
          <configItem>
            <name>colemak</name>
            <shortDescription>cmk</shortDescription>
            <description>English (Colemak)</description>
          </configItem>
        </variant>
      </variantList>
    </layout>
    <layout>
      <configItem>
        <name>de</name>
        <description>German</description>
      </configItem>
    </layout>
</layoutList>"#;

    #[test]
    fn labels() {
        let m = parse_rules(XML);
        assert_eq!(m["English (US)"], "en");
        assert_eq!(m["English (US, intl., with dead keys)"], "en");
        assert_eq!(m["English (Colemak)"], "cmk");
        assert_eq!(m["German"], "de");
    }
}

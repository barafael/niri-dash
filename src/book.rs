//! The widget facts in the manual (`book/src/reference/widgets.generated.md`), written from
//! [`Kind::info`] so that sizes, shapes and settings can't drift from the code.
//!
//! The file is committed. This test rebuilds it and fails when it differs, or when a widget isn't
//! included by the hand-written page of its group:
//!
//! ```sh
//! cargo test --lib book                        # check
//! NIRI_DASH_BOOK=bless cargo test --lib book   # rewrite the file
//! ```
//!
//! Each widget's block sits between `ANCHOR` comments named after the widget, so a page includes
//! it with `{{#include ../widgets.generated.md:clock}}` (mdBook's include syntax).

use crate::layout::{Group, Kind};
use crate::widget_settings::{Opt, Ty};
use kdl::KdlValue;
use std::fmt::Write as _;
use std::path::PathBuf;
use strum::IntoEnumIterator;

/// The groups in the order the manual lists them (the order [`crate::autolayout`] lays them out).
const GROUPS: [Group; 6] = [
    Group::Time,
    Group::Activity,
    Group::Media,
    Group::System,
    Group::Devices,
    Group::Controls,
];

fn group_name(group: Group) -> String {
    format!("{group:?}")
}

fn group_page(group: Group) -> String {
    group_name(group).to_lowercase()
}

fn raw(v: &KdlValue) -> String {
    match v {
        KdlValue::Bool(b) => format!("#{b}"),
        KdlValue::Integer(i) => i.to_string(),
        KdlValue::String(s) => format!("\"{s}\""),
        other => other.to_string(),
    }
}

/// What a number setting counts in, which config.kdl doesn't say.
fn unit(opt: Opt) -> Option<&'static str> {
    match opt {
        Opt::History | Opt::Interval => Some("seconds"),
        Opt::FocusMinutes | Opt::ShortBreak | Opt::LongBreak => Some("minutes"),
        _ => None,
    }
}

/// A value as written in config.kdl, with the word the sidebar shows for it where that is a word
/// (`2` is "cheeky"), not just the number with its unit.
fn shown(opt: Opt, v: &KdlValue) -> String {
    let text = opt.value_text(v);
    let numeric = text.chars().next().is_some_and(|c| c.is_ascii_digit());
    match v {
        KdlValue::Integer(_) if !numeric => format!("`{}` ({text})", raw(v)),
        _ => format!("`{}`", raw(v)),
    }
}

fn values(opt: Opt) -> String {
    match opt.ty() {
        Ty::Toggle => "`#true`, `#false`".into(),
        Ty::Steps(steps) => {
            let list = steps
                .iter()
                .map(|s| shown(opt, &KdlValue::Integer(*s as i128)))
                .collect::<Vec<_>>()
                .join(", ");
            match unit(opt) {
                Some(unit) => format!("{list} ({unit})"),
                None => list,
            }
        }
        Ty::Choice(words) => words
            .iter()
            .map(|w| format!("`\"{w}\"`"))
            .collect::<Vec<_>>()
            .join(", "),
        Ty::Text { placeholder } => format!("text (the sidebar's hint: *{placeholder}*)"),
    }
}

fn default(opt: Opt) -> String {
    let (world, us) = (opt.default(false), opt.default(true));
    let empty = matches!(&world, KdlValue::String(s) if s.is_empty());
    match (empty, world == us) {
        (true, _) => "empty".into(),
        (false, true) => shown(opt, &world),
        (false, false) => format!(
            "{} ({} with a US locale)",
            shown(opt, &world),
            shown(opt, &us)
        ),
    }
}

fn size((w, h): (i32, i32)) -> String {
    format!("{w}×{h}")
}

fn widget(kind: Kind) -> String {
    let info = kind.info();
    let mut s = String::new();
    let name = kind.name();
    let shapes = match (info.can_be_round, info.starts_round) {
        (false, _) => "rectangular".to_string(),
        (true, false) => "rectangular, round".to_string(),
        (true, true) => "round, rectangular (a new one starts round)".to_string(),
    };
    writeln!(s, "<!-- ANCHOR: {name} -->").unwrap();
    writeln!(s, "| | |\n|---|---|").unwrap();
    writeln!(s, "| Name in `config.kdl` | `{name}` |").unwrap();
    writeln!(s, "| Title | {} |", info.title).unwrap();
    writeln!(s, "| Shows | {} |", info.description).unwrap();
    writeln!(s, "| Group | {} |", group_name(info.group)).unwrap();
    writeln!(
        s,
        "| Size in cells | {} by default, {} at least, {} at most |",
        size(info.default_size),
        size(info.min_size),
        size(info.max_size)
    )
    .unwrap();
    writeln!(s, "| Shapes | {shapes} |").unwrap();
    if info.settings.is_empty() {
        writeln!(s, "| Settings | none |").unwrap();
    } else {
        writeln!(s, "\n| Setting | Label in the sidebar | Values | Default |").unwrap();
        writeln!(s, "|---|---|---|---|").unwrap();
        for &opt in info.settings {
            writeln!(
                s,
                "| `{}` | {} | {} | {} |",
                opt.as_ref(),
                opt.label(),
                values(opt),
                default(opt)
            )
            .unwrap();
        }
    }
    writeln!(s, "<!-- ANCHOR_END: {name} -->\n").unwrap();
    s
}

/// The index: every widget, by group.
fn index() -> String {
    let mut s = String::new();
    writeln!(s, "<!-- ANCHOR: index -->").unwrap();
    writeln!(
        s,
        "| Name | Group | Shows | Default size | Shapes |\n|---|---|---|---|---|"
    )
    .unwrap();
    for group in GROUPS {
        for kind in Kind::iter().filter(|k| k.info().group == group) {
            let info = kind.info();
            let shapes = match (info.can_be_round, info.starts_round) {
                (false, _) => "rectangular",
                (true, false) => "rectangular, round",
                (true, true) => "round, rectangular",
            };
            writeln!(
                s,
                "| [`{}`]({}.md#{}) | {} | {} | {} | {shapes} |",
                kind.name(),
                group_page(group),
                kind.name(),
                group_name(group),
                info.description,
                size(info.default_size),
            )
            .unwrap();
        }
    }
    writeln!(s, "<!-- ANCHOR_END: index -->\n").unwrap();
    s
}

fn generated() -> String {
    let mut s = String::from(
        "<!-- Written by `NIRI_DASH_BOOK=bless cargo test --lib book` (src/book.rs). Don't edit. -->\n\n",
    );
    s.push_str(&index());
    for kind in Kind::iter() {
        s.push_str(&widget(kind));
    }
    s
}

fn book_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("book/src/reference")
}

#[test]
fn widget_reference_is_current() {
    let path = book_dir().join("widgets.generated.md");
    let want = generated();
    if std::env::var("NIRI_DASH_BOOK").as_deref() == Ok("bless") {
        std::fs::write(&path, &want).unwrap();
        return;
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        have == want,
        "{} is out of date; run `NIRI_DASH_BOOK=bless cargo test --lib book`",
        path.display()
    );
}

#[test]
fn every_widget_is_on_its_groups_page() {
    for kind in Kind::iter() {
        assert!(
            GROUPS.contains(&kind.info().group),
            "{} is in a group the manual doesn't list",
            kind.name()
        );
        let group = kind.info().group;
        let page = book_dir()
            .join("widgets")
            .join(format!("{}.md", group_page(group)));
        let text =
            std::fs::read_to_string(&page).unwrap_or_else(|e| panic!("{}: {e}", page.display()));
        let include = format!("widgets.generated.md:{}}}}}", kind.name());
        assert_eq!(
            text.matches(&include).count(),
            1,
            "{} should include `{include}` once",
            page.display()
        );
        // The index links to `<group>.md#<name>`, so the heading carries that id.
        let id = format!("{{#{}}}", kind.name());
        assert!(
            text.contains(&id),
            "{} needs a heading ending in `{id}`",
            page.display()
        );
    }
}

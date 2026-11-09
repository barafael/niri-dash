//! The mascots and the about card against channels standing in for the system: what they say
//! (lines sent with `niri-dash say`, a widget's command, their own remarks), what clicks and
//! keys do, and that every look of them draws (ports of the mascot, cow and say scenarios of the
//! Slint tests).
//!
//! A plain program (`harness = false`): each scenario runs in turn on a fresh dashboard.

use niri_dash::Control;
use niri_dash::testing::{Harness, Sandbox};
use niri_dash_system::mascot;
use std::panic::AssertUnwindSafe;

fn show(config: &str) -> Harness {
    let mut h = Harness::new(config);
    h.control(Control::Show);
    h
}

const ONE: &str = "layout {\n    mascot col=0 row=0 width=4 height=3\n}\n";

/// The id of the first widget of a kind.
fn id_of(h: &Harness, title: &str) -> u32 {
    h.read(|d| {
        d.tiles
            .iter()
            .find(|t| t.kind().info().title == title)
            .map(|t| t.id())
    })
    .unwrap_or_else(|| panic!("no widget titled {title:?}"))
}

fn enter(h: &mut Harness, title: &str) {
    let hint = h
        .read(|d| {
            d.tiles
                .iter()
                .find(|t| t.kind().info().title == title)
                .map(|t| t.hint.clone())
        })
        .unwrap_or_else(|| panic!("no widget titled {title:?}"));
    h.key("f");
    for c in hint.chars() {
        h.type_text(&c.to_string());
    }
}

/// What mascot `id` says now.
fn says(h: &Harness, id: u32) -> String {
    h.read(|d| {
        d.widgets
            .extras
            .mascot
            .lines
            .get(&id)
            .map(|v| v.text.clone())
            .unwrap_or_default()
    })
}

/// `niri-dash say` with this JSON.
fn say(h: &mut Harness, json: &str) {
    let say = serde_json::from_str(json).expect("a line that parses");
    h.update(|d, cx| d.mascot_say(say, cx));
}

/// A line sent from outside shows, a click takes it away, and the mascot's own comes back.
fn say_shows_and_a_click_takes_it_away() {
    let mut h = show(ONE);
    let id = id_of(&h, "Mascot");
    let own = says(&h, id);
    assert!(!own.is_empty(), "no remark of its own");
    say(&mut h, r#"{"text": "Build failed", "secs": 0}"#);
    assert_eq!(says(&h, id), "Build failed");
    say(&mut h, r#"{"text": "And another", "secs": 0}"#);
    assert_eq!(says(&h, id), "Build failed", "the second waits");
    assert_eq!(h.read(|d| d.widgets.extras.mascot.lines[&id].queued), 1);
    h.click_on("Next line");
    assert_eq!(says(&h, id), "And another");
    h.click_on("Next line");
    assert!(!says(&h, id).is_empty() && says(&h, id) != "And another");
}

/// Enter and Space are a click on the mascot.
fn keys_are_the_next_line() {
    let mut h = show(ONE);
    let id = id_of(&h, "Mascot");
    say(&mut h, r#"{"text": "one", "secs": 0}"#);
    say(&mut h, r#"{"text": "two", "secs": 0}"#);
    enter(&mut h, "Mascot");
    h.key("space");
    assert_eq!(says(&h, id), "two");
    h.key("enter");
    assert_ne!(says(&h, id), "two");
    // The keys of a list do nothing here, and are the widget's.
    h.keys("j k h l g G");
    assert_eq!(h.read(|d| format!("{:?}", d.nav.mode)), "Widget");
}

/// With no mascot on the dashboard a line has nowhere to go: it is shown as a toast.
fn say_with_no_mascot_is_a_toast() {
    let mut h = show("layout {\n    clock col=0 row=0 width=3 height=2\n}\n");
    say(&mut h, r#"{"text": "Hello there"}"#);
    assert_eq!(
        h.read(|d| d.toast.as_ref().map(|t| t.text.clone())),
        Some("Hello there".to_string())
    );
}

/// A line goes to the one named, if it is there.
fn say_to_one_of_them() {
    let mut h = show(
        "layout {\n    mascot col=0 row=0 width=4 height=3 { character \"zolli\" }\n    mascot col=4 row=0 width=4 height=3 { character \"cow\" }\n}\n",
    );
    let ids: Vec<u32> = h.read(|d| d.tiles.iter().map(|t| t.id()).collect());
    say(&mut h, r#"{"text": "for the cow", "to": "cow", "secs": 0}"#);
    assert_eq!(says(&h, ids[1]), "for the cow");
    assert_ne!(says(&h, ids[0]), "for the cow");
    say(&mut h, r#"{"text": "for all", "to": "all", "secs": 0}"#);
    assert_eq!(says(&h, ids[0]), "for all");
    assert_eq!(says(&h, ids[1]), "for the cow");
}

/// A widget with a command asks for it to be run, and says what it printed.
fn command_lines() {
    let mut h =
        show("layout {\n    mascot col=0 row=0 width=4 height=3 { command \"echo hi\" }\n}\n");
    let id = id_of(&h, "Mascot");
    let mut asked = false;
    while let Ok(c) = h.inbox.mascot.try_recv() {
        let mascot::Command::Run {
            id: for_id,
            command,
            ..
        } = c;
        assert_eq!((for_id, command.as_str()), (id, "echo hi"));
        asked = true;
    }
    assert!(asked, "the command was not asked for");
    h.send(mascot::Event::Output {
        id,
        stdout: Some("It works\n".into()),
    });
    assert_eq!(says(&h, id), "It works");
    // A command that failed gets a remark of the mascot's own.
    h.click_on("Next line");
    h.send(mascot::Event::Output { id, stdout: None });
    assert!(!says(&h, id).is_empty());
}

/// Every mascot draws, in every size, outfit and cow.
fn everything_draws() {
    let cows = ["", "tux", "surely-no-such-cow"];
    for character in ["schraubi", "zolli", "verschnitti", "cow"] {
        for (w, hgt) in [(2, 1), (3, 2), (4, 3), (8, 6), (1, 1)] {
            for cow in cows {
                let config = format!(
                    "layout {{\n    mascot col=0 row=0 width={w} height={hgt} {{\n        character \"{character}\"\n        cowfile \"{cow}\"\n    }}\n}}\n"
                );
                let mut h = show(&config);
                for outfit in [
                    "plain", "helmet", "vest", "beard", "shades", "tophat", "party",
                ] {
                    say(
                        &mut h,
                        &format!(
                            r#"{{"text": "Some words, a few more of them to wrap around a bubble.", "outfit": "{outfit}", "secs": 0}}"#
                        ),
                    );
                    h.draw();
                }
                assert!(h.has("Next line"), "{character} {w}x{hgt}: not on screen");
                // Time passes: blinks, glances and ducking.
                h.advance(10);
            }
        }
    }
}

/// The pointer over a mascot makes no trouble, and neither does leaving it.
fn the_pointer_is_followed() {
    let mut h = show(ONE);
    let b = h.bounds_of("Next line").expect("the mascot on screen");
    let (x, y) = (f32::from(b.center().x), f32::from(b.center().y));
    h.click(x, y);
    h.click(x + 40.0, y - 20.0);
    h.click(1.0, 1.0);
}

/// Hiding the dashboard and showing it again: the mascots come up with something new.
fn it_comes_up_with_something_new() {
    let mut h = show(ONE);
    let id = id_of(&h, "Mascot");
    let seq = h.read(|d| d.widgets.extras.mascot.lines[&id].seq);
    h.control(Control::Hide);
    h.control(Control::Show);
    assert_ne!(h.read(|d| d.widgets.extras.mascot.lines[&id].seq), seq);
}

/// The about card says what version this is.
fn about_card() {
    let mut h = show("layout {\n    about col=0 row=0 width=3 height=2\n}\n");
    assert!(h.has("Open repository"));
    enter(&mut h, "About");
    h.key("enter");
    h.keys("j k");
    h.click_on("Open repository");
}

fn main() {
    let _sandbox = Sandbox::enter(None).expect("a temporary directory for the dashboard's files");
    let tests: &[(&str, fn())] = &[
        (
            "mascot: a sent line shows and a click takes it away",
            say_shows_and_a_click_takes_it_away,
        ),
        (
            "mascot: Enter and Space are the next line",
            keys_are_the_next_line,
        ),
        (
            "mascot: a line with no mascot is a toast",
            say_with_no_mascot_is_a_toast,
        ),
        ("mascot: a line to one of them", say_to_one_of_them),
        ("mascot: lines from a command", command_lines),
        (
            "mascot: every one draws, in every size and outfit",
            everything_draws,
        ),
        ("mascot: the pointer is followed", the_pointer_is_followed),
        (
            "mascot: it comes up with something new",
            it_comes_up_with_something_new,
        ),
        ("about: the card", about_card),
    ];
    let filter = std::env::args().nth(1);
    let mut failed = 0;
    for (name, test) in tests {
        if filter.as_ref().is_some_and(|f| !name.contains(f.as_str())) {
            continue;
        }
        match std::panic::catch_unwind(AssertUnwindSafe(test)) {
            Ok(()) => println!("test {name} ... ok"),
            Err(_) => {
                failed += 1;
                println!("test {name} ... FAILED");
            }
        }
    }
    println!(
        "\ntest result: {}. {} passed; {failed} failed",
        if failed == 0 { "ok" } else { "FAILED" },
        tests.len() - failed
    );
    if failed > 0 {
        std::process::exit(1);
    }
}

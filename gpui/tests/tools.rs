//! The tools widgets (keyboard layout, color picker, notes, clipboard history) against channels
//! standing in for the system: what they send when they are used, and what they show when the
//! system reports something. Plain program (`harness = false`): each scenario runs in turn on a
//! fresh dashboard.

use niri_dash_gpui::Control;
use niri_dash_gpui::testing::{Harness, Sandbox};
use niri_dash_system::keyboard::{self, Layouts, Switch};
use niri_dash_system::{clipboard, color};
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

fn dashboard(layout: &str) -> Harness {
    let mut h = Harness::new(layout);
    // What an earlier scenario picked or wrote is not this one's.
    h.update(|d, _| d.state.colors.clear());
    h.control(Control::Show);
    h
}

fn layouts() -> keyboard::Event {
    keyboard::Event::Layouts(Some(Layouts::from_names(
        ["English (US)", "German", "French"],
        0,
    )))
}

/// Enter the widget of this kind by its hint label.
fn enter(h: &mut Harness, kind: &str) {
    let hint = h.read(|d| {
        d.tiles
            .iter()
            .find(|t| t.kind().name() == kind)
            .unwrap_or_else(|| panic!("no {kind} widget"))
            .hint
            .clone()
    });
    h.key("f");
    h.type_text(&hint);
    assert!(
        h.read(|d| format!("{:?}", d.nav.mode) == "Widget"),
        "not in {kind}"
    );
}

fn switched(h: &mut Harness) -> Option<Switch> {
    match h.inbox.keyboard.try_recv() {
        Ok(keyboard::Command::Switch(s)) => Some(s),
        Err(_) => None,
    }
}

fn copied(text: &str) -> clipboard::Event {
    clipboard::Event::Copied(clipboard::Clip {
        kind: clipboard::ClipKind::Text,
        data: Arc::new(text.as_bytes().to_vec()),
    })
}

fn clips(h: &Harness) -> usize {
    h.read(|d| d.widgets.tools.clipboard_len())
}

fn texts(h: &Harness) -> Vec<String> {
    h.read(|d| d.widgets.tools.clipboard_texts())
}

fn set_text(h: &mut Harness) -> Option<String> {
    match h.inbox.clipboard.try_recv() {
        Ok(clipboard::Command::Set(c)) => Some(String::from_utf8_lossy(&c.data).into_owned()),
        _ => None,
    }
}

fn cursor(h: &Harness) -> (usize, usize) {
    h.read(|d| d.widgets.tools.cursor(d.nav.active_id))
}

fn holds(h: &Harness) -> Result<(), String> {
    h.read(|d| d.widgets.tools.check(d))
}

fn center(h: &Harness, selector: &'static str) -> (f32, f32) {
    let b = h
        .bounds_of(selector)
        .unwrap_or_else(|| panic!("nothing called {selector:?}"));
    (f32::from(b.center().x), f32::from(b.center().y))
}

// ---- keyboard layout --------------------------------------------------------------------------

/// Without room for a list the widget is one button: a click or the wheel steps through the
/// layouts, a notch at a time (small steps add up).
fn keyboard_steps_by_click_and_wheel() {
    let mut h = dashboard("layout {\n  keyboard-layout col=0 row=0 width=2 height=1\n}");
    h.send(layouts());
    assert!(h.has("Next keyboard layout"));
    h.click_on("Next keyboard layout");
    assert_eq!(switched(&mut h), Some(Switch::Next));
    let (x, y) = center(&h, "Next keyboard layout");
    h.scroll(x, y, -60.0);
    assert_eq!(switched(&mut h), Some(Switch::Next));
    h.scroll(x, y, 60.0);
    assert_eq!(switched(&mut h), Some(Switch::Prev));
    h.scroll(x, y, -25.0);
    assert_eq!(switched(&mut h), None, "a small step is not a notch");
    h.scroll(x, y, -25.0);
    h.scroll(x, y, -25.0);
    assert_eq!(switched(&mut h), Some(Switch::Next), "small steps add up");
}

/// The keyboard: h/l (or Enter) step through the layouts of a small widget.
fn keyboard_keys_step_in_a_small_widget() {
    let mut h = dashboard("layout {\n  keyboard-layout col=0 row=0 width=2 height=1\n}");
    h.send(layouts());
    enter(&mut h, "keyboard-layout");
    h.key("l");
    assert_eq!(switched(&mut h), Some(Switch::Next));
    h.key("h");
    assert_eq!(switched(&mut h), Some(Switch::Prev));
    h.key("enter");
    assert_eq!(switched(&mut h), Some(Switch::Next));
    h.key("k");
    assert_eq!(switched(&mut h), Some(Switch::Next));
    h.key("j");
    assert_eq!(switched(&mut h), Some(Switch::Prev));
}

/// With room the layouts are a list: click picks, j/k move the cursor (it starts on the layout in
/// use), Enter picks.
fn keyboard_list_picks() {
    let mut h = dashboard("layout {\n  keyboard-layout col=0 row=0 width=3 height=3\n}");
    h.send(layouts());
    assert!(!h.has("Next keyboard layout"), "a list, not one button");
    h.click_on("German");
    assert_eq!(switched(&mut h), Some(Switch::Index(1)));
    enter(&mut h, "keyboard-layout");
    assert_eq!(cursor(&h), (0, 0));
    h.key("j");
    h.key("enter");
    assert_eq!(switched(&mut h), Some(Switch::Index(1)));
    h.key("G");
    h.key("enter");
    assert_eq!(switched(&mut h), Some(Switch::Index(2)));
    h.key("l");
    assert_eq!(
        switched(&mut h),
        Some(Switch::Next),
        "h and l step through the layouts too"
    );
    // The layouts change: the list follows, and the cursor stays on a stop that exists.
    h.send(keyboard::Event::Layouts(Some(Layouts::from_names(
        ["German", "French"],
        0,
    ))));
    assert!(!h.has("English (US)"));
    assert_eq!(holds(&h), Ok(()));
    h.send(keyboard::Event::Layouts(None));
    assert_eq!(holds(&h), Ok(()));
    assert!(
        h.has("Next keyboard layout"),
        "nothing to list: the single button, which says so"
    );
}

// ---- color picker ---------------------------------------------------------------------------

const PICKER: &str = "layout {\n  color-picker col=0 row=0 width=4 height=3\n}";

/// Picking a color first hides the dashboard and only picks once it is gone (the window, which
/// isn't there in a test, says when); the picked color comes back, is kept and copied, and the
/// dashboard returns.
fn color_pick() {
    let mut h = dashboard(PICKER);
    h.click_on("Pick a color");
    assert_eq!(h.controls(), [Control::Hide]);
    assert!(
        h.inbox.color.try_recv().is_err(),
        "picked while still shown"
    );
    h.control(Control::Hide);
    assert!(
        matches!(h.inbox.color.try_recv(), Ok(color::Command::Pick)),
        "the pick starts once the dashboard is gone"
    );
    h.send(color::Event::Picked(Some([1.0, 0.5, 0.0])));
    assert!(
        h.read(|d| d.visible) || h.controls().contains(&Control::Show),
        "the dashboard returns"
    );
    assert_eq!(h.read(|d| d.state.colors.clone()), ["#ff8000"]);
    assert_eq!(set_text(&mut h).as_deref(), Some("#ff8000"));
    // The same color again moves to the front instead of twice into the list.
    h.send(color::Event::Picked(Some([0.0, 0.0, 1.0])));
    h.send(color::Event::Picked(Some([1.0, 0.5, 0.0])));
    assert_eq!(h.read(|d| d.state.colors.clone()), ["#ff8000", "#0000ff"]);
    // Cancelled: nothing changes.
    h.send(color::Event::Picked(None));
    assert_eq!(h.read(|d| d.state.colors.len()), 2);
}

/// Keys: j/k over the values, the pipette and the history; Enter copies or picks; p picks.
fn color_keys() {
    let mut h = dashboard(PICKER);
    for rgb in [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]] {
        h.send(color::Event::Picked(Some(rgb)));
    }
    while set_text(&mut h).is_some() {}
    enter(&mut h, "color-picker");
    // hex, rgb(), hsl(), the pipette, then the history.
    h.key("enter");
    assert_eq!(set_text(&mut h).as_deref(), Some("#ff0000"));
    h.key("j");
    h.key("enter");
    assert_eq!(set_text(&mut h).as_deref(), Some("rgb(255, 0, 0)"));
    h.key("j");
    h.key("enter");
    assert_eq!(set_text(&mut h).as_deref(), Some("hsl(0, 100%, 50%)"));
    h.key("j");
    h.key("j");
    h.key("enter");
    assert_eq!(
        set_text(&mut h).as_deref(),
        Some("#00ff00"),
        "the history's first swatch"
    );
    h.key("G");
    h.key("enter");
    assert_eq!(
        set_text(&mut h).as_deref(),
        Some("#0000ff"),
        "the last swatch"
    );
    assert!(h.controls().is_empty());
    h.key("g");
    h.key("p");
    assert_eq!(h.controls(), [Control::Hide], "p picks");
    assert_eq!(holds(&h), Ok(()));
}

// ---- clipboard history ----------------------------------------------------------------------

/// Out of the box nothing is recorded: the system isn't asked to, and a stray copy isn't kept.
fn clipboard_history_off_by_default() {
    let mut h = Harness::new(&niri_dash_gpui::testing::default_config());
    assert!(h.inbox.clipboard.try_recv().is_err());
    h.send(copied("hunter2"));
    assert_eq!(clips(&h), 0);
}

/// A clipboard widget turns the recording on; removing it turns it off and forgets the history,
/// and adding one turns it back on.
fn clipboard_history_follows_the_widget() {
    let mut h = Harness::new("layout {\n  clipboard col=0 row=0 width=4 height=4\n}");
    assert!(matches!(
        h.inbox.clipboard.try_recv(),
        Ok(clipboard::Command::Record(true))
    ));
    h.send(copied("kept"));
    assert_eq!(clips(&h), 1);

    let id = h.read(|d| d.tiles[0].id());
    h.update(|d, _| {
        d.layout.remove(id);
        d.sync_widgets();
    });
    assert!(matches!(
        h.inbox.clipboard.try_recv(),
        Ok(clipboard::Command::Record(false))
    ));
    assert_eq!(clips(&h), 0, "the history outlived its widget");
    h.send(copied("read as the widget went"));
    assert_eq!(clips(&h), 0);

    h.update(|d, _| {
        d.layout.add_at("clipboard".parse().unwrap(), 0, 0);
        d.sync_widgets();
    });
    assert!(matches!(
        h.inbox.clipboard.try_recv(),
        Ok(clipboard::Command::Record(true))
    ));
}

const CLIPBOARD: &str = "layout {\n  clipboard col=0 row=0 width=4 height=4\n}";

/// The newest copy is first, a repeat moves up instead of doubling, a row puts its text back.
fn clipboard_lists_and_copies() {
    let mut h = dashboard(CLIPBOARD);
    let _ = h.inbox.clipboard.try_recv();
    assert!(!h.has("Clear clipboard history"), "nothing to clear yet");
    for t in ["one", "two", "three", "one"] {
        h.send(copied(t));
    }
    assert_eq!(texts(&h), ["one", "three", "two"]);
    h.click_on("two");
    assert_eq!(set_text(&mut h).as_deref(), Some("two"));
    h.click_on("Clear clipboard history");
    assert_eq!(clips(&h), 0);
    assert!(!h.has("Clear clipboard history"));
}

/// j/k over the entries, Enter copies, l then Enter removes, d removes, D clears.
fn clipboard_keys() {
    let mut h = dashboard(CLIPBOARD);
    let _ = h.inbox.clipboard.try_recv();
    for t in ["a", "b", "c", "d", "e"] {
        h.send(copied(t));
    }
    // e d c b a
    enter(&mut h, "clipboard");
    h.key("j");
    h.key("enter");
    assert_eq!(set_text(&mut h).as_deref(), Some("d"));
    h.key("l");
    h.key("enter");
    assert_eq!(texts(&h), ["e", "c", "b", "a"], "the remove button");
    assert_eq!(cursor(&h), (1, 1));
    h.key("h");
    h.key("d");
    assert_eq!(texts(&h), ["e", "b", "a"]);
    h.key("G");
    h.key("d");
    assert_eq!(texts(&h), ["e", "b"]);
    assert_eq!(cursor(&h).0, 1, "the cursor stays on an entry");
    h.key("D");
    assert_eq!(clips(&h), 0);
    assert_eq!(holds(&h), Ok(()));
    // Nothing to act on: the keys don't panic.
    for k in ["j", "k", "l", "enter", "d", "G"] {
        h.key(k);
    }
    assert_eq!(holds(&h), Ok(()));
}

/// A picture is a thumbnail entry.
fn clipboard_shows_pictures() {
    let mut h = dashboard(CLIPBOARD);
    let mut png_bytes = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png_bytes, 4, 2);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&[255; 4 * 2 * 4]).unwrap();
    }
    h.send(clipboard::Event::Copied(clipboard::Clip {
        kind: clipboard::ClipKind::Png,
        data: Arc::new(png_bytes),
    }));
    assert_eq!(texts(&h), ["Image"]);
    assert!(h.has("Image"));
    // What isn't a picture is dropped.
    h.send(clipboard::Event::Copied(clipboard::Clip {
        kind: clipboard::ClipKind::Png,
        data: Arc::new(b"not a png".to_vec()),
    }));
    assert_eq!(clips(&h), 1);
}

// ---- notes ------------------------------------------------------------------------------------

const NOTES: &str =
    "layout {\n  notes col=0 row=0 width=4 height=3 {\n    note \"scratch pad\"\n  }\n}";

fn note_file(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("NIRI_DASH_HOME").expect("a sandbox"))
        .join("data/niri-dash/notes")
        .join(format!("{name}.md"))
}

fn note(h: &Harness) -> String {
    h.read(|d| d.widgets.tools.note_text(d.tiles[0].id()))
}

fn mode(h: &Harness) -> String {
    h.read(|d| format!("{:?}", d.nav.mode))
}

/// Entering the note puts the keys in the text: every letter is typed, also the ones the dashboard
/// has for itself (f, s, e, H); it is saved once typing pauses, and Escape goes back out.
fn notes_type_and_save() {
    let _ = std::fs::remove_file(note_file("scratch-pad"));
    let mut h = dashboard(NOTES);
    assert!(
        h.has("Scratch pad"),
        "the text area is named after the note"
    );
    enter(&mut h, "notes");
    h.type_text("fish see HJKL");
    assert_eq!(note(&h), "fish see HJKL");
    assert_eq!(mode(&h), "Widget");
    h.key("enter");
    h.type_text("two");
    assert_eq!(note(&h), "fish see HJKL\ntwo");
    h.key("backspace");
    assert_eq!(note(&h), "fish see HJKL\ntw");
    assert!(
        !note_file("scratch-pad").exists(),
        "saved before typing paused"
    );
    h.advance(1);
    assert!(
        !note_file("scratch-pad").exists(),
        "typed during that second"
    );
    h.advance(1);
    assert_eq!(
        std::fs::read_to_string(note_file("scratch-pad")).unwrap(),
        "fish see HJKL\ntw"
    );
    assert_eq!(holds(&h), Ok(()));
    // Escape leaves the note, and the keys are the dashboard's again.
    h.key("esc");
    assert_eq!(mode(&h), "Off");
    h.key("f");
    assert_eq!(mode(&h), "Hints");
    assert_eq!(note(&h), "fish see HJKL\ntw");
}

/// Hiding saves right away; showing reads the note again (it may have changed on disk), and the
/// text area works in the new window.
fn notes_survive_hiding() {
    let _ = std::fs::remove_file(note_file("scratch-pad"));
    let mut h = dashboard(NOTES);
    enter(&mut h, "notes");
    h.type_text("kept");
    h.control(Control::Hide);
    assert_eq!(
        std::fs::read_to_string(note_file("scratch-pad")).unwrap(),
        "kept"
    );
    std::fs::write(note_file("scratch-pad"), "edited elsewhere").unwrap();
    h.control(Control::Show);
    assert_eq!(note(&h), "edited elsewhere");
    enter(&mut h, "notes");
    h.type_text("!");
    assert_eq!(note(&h), "edited elsewhere!");
}

/// Widgets with the same note name show the same text; another name is another note.
fn notes_are_shared_by_name() {
    let _ = std::fs::remove_file(note_file("shared"));
    let _ = std::fs::remove_file(note_file("other"));
    let mut h = dashboard(
        "layout {\n  notes col=0 row=0 width=3 height=3 {\n    note \"shared\"\n  }\n  \
         notes col=3 row=0 width=3 height=3 {\n    note \"Shared\"\n  }\n  \
         notes col=6 row=0 width=3 height=3 {\n    note \"other\"\n  }\n}",
    );
    let ids: Vec<u32> = h.read(|d| d.tiles.iter().map(|t| t.id()).collect());
    enter(&mut h, "notes");
    h.type_text("hello");
    let texts: Vec<String> =
        h.read(|d| ids.iter().map(|&i| d.widgets.tools.note_text(i)).collect());
    assert_eq!(
        texts.iter().filter(|t| *t == "hello").count(),
        2,
        "{texts:?}"
    );
    assert_eq!(
        texts.iter().filter(|t| t.is_empty()).count(),
        1,
        "{texts:?}"
    );
}

// ---- the stress run ---------------------------------------------------------------------------

/// Random keys over every tools widget, in every shape and size, on both screens: the default
/// layout has none of them.
fn stress_tools() {
    use niri_dash_gpui::testing::Monkey;
    let config = "grid columns=16 rows=9\nlayout {\n  \
        keyboard-layout col=0 row=0 width=3 height=3\n  \
        keyboard-layout col=3 row=0 width=2 height=1\n  \
        color-picker col=0 row=3 width=4 height=3\n  \
        color-picker col=4 row=3 size=2 shape=\"round\"\n  \
        color-picker col=6 row=3 width=2 height=1\n  \
        notes col=8 row=0 width=4 height=3 {\n    note \"work\"\n  }\n  \
        notes col=12 row=0 width=3 height=2 {\n    note \"work\"\n  }\n  \
        clipboard col=8 row=3 width=4 height=4\n  \
        clipboard col=12 row=3 size=2 shape=\"round\"\n  \
        clipboard col=12 row=5 width=3 height=1\n}\n";
    let ops = std::env::var("NIRI_DASH_STRESS_OPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(800);
    for seed in [1, 2, 3] {
        for size in [(1706, 1066), (1280, 800)] {
            let mut h = Harness::with_mock_sized(config, size);
            for t in ["alpha", "beta\ngamma", "delta", "epsilon", "zeta"] {
                h.send(copied(t));
            }
            let monkey = h
                .stress(
                    Monkey::new(seed, std::env::var_os("NIRI_DASH_STRESS_LOG").is_some())
                        .keep_going(),
                    ops,
                )
                .unwrap_or_else(|f| panic!("{f}"));
            if let Err(f) = monkey.finish() {
                panic!("{f}");
            }
        }
    }
}

/// A double click on a control is the control's: it doesn't pick the widget up.
fn double_click_on_a_control_does_not_carry() {
    let mut h = dashboard(
        "layout {\n    keyboard-layout col=0 row=0 width=2 height=1\n    color-picker col=2 row=0 width=3 height=2\n    notes col=5 row=0 width=3 height=3\n}\n",
    );
    h.send(layouts());
    for control in ["Next keyboard layout", "Pick a color", "Notes"] {
        let c = h
            .bounds_of(control)
            .unwrap_or_else(|| panic!("no {control:?} on screen"))
            .center();
        h.double_click(f32::from(c.x), f32::from(c.y));
        assert!(
            !h.read(|d| d.edit.dragging()),
            "a double click on {control:?} picked the widget up"
        );
    }
}

fn main() {
    let _sandbox = Sandbox::enter(None).expect("a temporary directory for the dashboard's files");
    // SAFETY: nothing else runs yet that could read the environment.
    unsafe { std::env::set_var("LC_ALL", "C") };
    let scenarios: &[(&str, fn())] = &[
        (
            "keyboard layout: click and wheel",
            keyboard_steps_by_click_and_wheel,
        ),
        (
            "keyboard layout: keys in a small widget",
            keyboard_keys_step_in_a_small_widget,
        ),
        ("keyboard layout: the list picks", keyboard_list_picks),
        ("color picker: pick", color_pick),
        ("color picker: keys", color_keys),
        (
            "clipboard history: off by default",
            clipboard_history_off_by_default,
        ),
        (
            "clipboard history: follows the widget",
            clipboard_history_follows_the_widget,
        ),
        ("clipboard: lists and copies", clipboard_lists_and_copies),
        ("clipboard: keys", clipboard_keys),
        ("clipboard: pictures", clipboard_shows_pictures),
        ("notes: type and save", notes_type_and_save),
        ("notes: survive hiding", notes_survive_hiding),
        ("notes: shared by name", notes_are_shared_by_name),
        (
            "double click on a control does not carry",
            double_click_on_a_control_does_not_carry,
        ),
        ("stress: every tools widget", stress_tools),
    ];
    let mut failed = Vec::new();
    for (name, scenario) in scenarios {
        let ok = std::panic::catch_unwind(AssertUnwindSafe(scenario)).is_ok();
        println!("test {name} ... {}", if ok { "ok" } else { "FAILED" });
        if !ok {
            failed.push(*name);
        }
    }
    println!(
        "\ntest result: {}. {} passed; {} failed",
        if failed.is_empty() { "ok" } else { "FAILED" },
        scenarios.len() - failed.len(),
        failed.len()
    );
    if !failed.is_empty() {
        std::process::exit(1);
    }
}

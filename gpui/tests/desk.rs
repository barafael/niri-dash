//! The desk widgets (windows, notifications, tray and its menu, media, Claude sessions) against
//! channels standing in for the system: what they send when they are used, and what they show
//! when the system reports something. Ports of the scenarios of `tests/ui.rs`
//! (`tray_activate`, `claude_sessions_walk`, `claude_session_enter`, `cursor_stays_in_sight_*`,
//! `stops_in_sight_other_lists`) and the widgets' own behaviour.
//!
//! A plain program (`harness = false`): each scenario runs in turn on a fresh dashboard.

use gpui::MouseButton;
use niri_dash_gpui::Control;
use niri_dash_gpui::testing::{Harness, Sandbox};
use niri_dash_system::notifications::{Dnd, Notification};
use niri_dash_system::{Demo, IconSrc, claude, media, notifications, tray, windows};
use std::panic::AssertUnwindSafe;

const LAYOUT: &str = r#"
grid columns=16 rows=9
layout {
    tray          col=0 row=0 width=5 height=1
    windows       col=0 row=1 width=5 height=3
    notifications col=6 row=0 width=5 height=4
    media         col=11 row=0 width=5 height=3
    claude-sessions col=0 row=5 width=5 height=4
}
"#;

/// The dashboard with `config`, up.
fn up(config: &str) -> Harness {
    let mut h = Harness::new(config);
    h.control(Control::Show);
    h
}

fn dashboard() -> Harness {
    up(LAYOUT)
}

fn alone(kind: &str, w: i32, h: i32) -> Harness {
    up(&format!(
        "grid columns=16 rows=9\nlayout {{\n    {kind} col=0 row=0 width={w} height={h}\n}}\n"
    ))
}

/// Enter the widget with this title by its hint label.
fn enter(h: &mut Harness, title: &str) {
    let hint = h.read(|d| {
        d.tiles
            .iter()
            .find(|t| t.kind().info().title == title)
            .unwrap_or_else(|| panic!("no widget titled {title:?}"))
            .hint
            .clone()
    });
    // Out of whatever the hints were doing (an Escape also closes a menu first).
    for _ in 0..4 {
        if h.read(|d| format!("{:?}", d.nav.mode) == "Hints") {
            h.key("esc");
        }
    }
    h.key("f");
    for c in hint.chars() {
        h.key(&c.to_string());
    }
    assert!(in_widget(h), "entered {title}");
}

/// A widget is entered (it has the keys).
fn in_widget(h: &Harness) -> bool {
    h.read(|d| format!("{:?}", d.nav.mode) == "Widget")
}

fn commands_windows(h: &mut Harness) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(c) = h.inbox.windows.try_recv() {
        out.push(format!("{c:?}"));
    }
    out
}

fn commands_notifications(h: &mut Harness) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(c) = h.inbox.notifications.try_recv() {
        out.push(format!("{c:?}"));
    }
    out
}

fn commands_tray(h: &mut Harness) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(c) = h.inbox.tray.try_recv() {
        out.push(format!("{c:?}"));
    }
    out
}

fn commands_media(h: &mut Harness) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(c) = h.inbox.media.try_recv() {
        out.push(format!("{c:?}"));
    }
    out
}

// ---- tray ----------------------------------------------------------------------------------

/// Clicking a tray item activates it where it was clicked, and gets out of the way.
fn tray_activate() {
    let mut h = dashboard();
    h.send(tray::Event::Items(Demo::new().tray));
    let b = h.bounds_of("Nextcloud").expect("the first icon is drawn");
    let c = b.center();
    h.click(f32::from(c.x), f32::from(c.y));
    let sent = commands_tray(&mut h);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(
        sent[0].contains(":1.40/StatusNotifierItem") && sent[0].contains("Activate"),
        "{sent:?}"
    );
    assert_eq!(h.controls(), [Control::Hide]);
}

fn menu(entries: Vec<tray::Entry>) -> tray::Event {
    tray::Event::Menu(tray::Menu {
        address: ":1.40/StatusNotifierItem".into(),
        title: "Nextcloud".into(),
        entries,
        x: 40.0,
        y: 80.0,
    })
}

fn entry(id: i32, label: &str, separator: bool, enabled: bool, header: bool) -> tray::Entry {
    tray::Entry {
        id,
        label: label.into(),
        depth: 0,
        separator,
        enabled,
        toggle: tray::Toggle::None,
        checked: false,
        header,
    }
}

fn menu_entries() -> Vec<tray::Entry> {
    vec![
        entry(1, "Open Nextcloud", false, true, false),
        entry(0, "", true, true, false),
        entry(2, "Paused", false, false, false),
        entry(3, "Account", false, true, true),
        entry(4, "Quit", false, true, false),
    ]
}

/// A right click opens the menu; the system answers with it, it is drawn over the window, and
/// clicking an entry chooses it, closes the menu and gets out of the way.
fn tray_menu_by_mouse() {
    let mut h = dashboard();
    h.send(tray::Event::Items(Demo::new().tray));
    let c = h.bounds_of("Nextcloud").expect("icon").center();
    h.click_with(f32::from(c.x), f32::from(c.y), MouseButton::Right);
    let sent = commands_tray(&mut h);
    assert!(sent.len() == 1 && sent[0].contains("OpenMenu"), "{sent:?}");
    assert!(!h.has("Tray menu"));
    h.send(menu(menu_entries()));
    assert!(h.has("Tray menu"), "the menu is drawn");
    assert!(h.has("Quit"));
    h.click_on("Quit");
    let sent = commands_tray(&mut h);
    assert!(
        sent.len() == 1 && sent[0].contains("MenuItem") && sent[0].contains("id: 4"),
        "{sent:?}"
    );
    assert!(!h.has("Tray menu"), "chosen: closed");
    assert_eq!(h.controls(), [Control::Hide]);
}

/// A click outside the menu closes it and does nothing else.
fn tray_menu_closes_on_a_click_outside() {
    let mut h = dashboard();
    h.send(menu(menu_entries()));
    assert!(h.has("Tray menu"));
    h.click(900.0, 900.0);
    assert!(!h.has("Tray menu"));
    assert_eq!(
        h.controls(),
        [],
        "the click went to the menu's catcher, not the backdrop"
    );
    assert!(commands_tray(&mut h).is_empty());
}

/// An empty menu isn't opened: the dashboard says so.
fn tray_empty_menu_says_so() {
    let mut h = dashboard();
    h.send(menu(Vec::new()));
    assert!(!h.has("Tray menu"));
    assert!(h.read(|d| {
        d.toast
            .as_ref()
            .is_some_and(|t| t.text.contains("empty menu"))
    }));
}

/// `m` opens the menu of the icon under the cursor; j/k skip what can't be chosen, Enter chooses,
/// Escape closes without leaving the widget.
fn tray_menu_by_keyboard() {
    let mut h = dashboard();
    h.send(tray::Event::Items(Demo::new().tray));
    enter(&mut h, "Tray");
    h.key("l");
    h.key("m");
    let sent = commands_tray(&mut h);
    assert!(
        sent.len() == 1 && sent[0].contains("OpenMenu") && sent[0].contains(":1.52"),
        "{sent:?}"
    );
    h.send(menu(menu_entries()));
    assert!(h.has("Tray menu"));
    // The entries the cursor can stop on: Open (0) and Quit (4).
    h.key("j");
    assert_eq!(
        h.read(|d| d.widgets.desk.tray.menu.as_ref().and_then(|m| m.cursor)),
        Some(0)
    );
    h.key("j");
    assert_eq!(
        h.read(|d| d.widgets.desk.tray.menu.as_ref().and_then(|m| m.cursor)),
        Some(4)
    );
    h.key("j");
    assert_eq!(
        h.read(|d| d.widgets.desk.tray.menu.as_ref().and_then(|m| m.cursor)),
        Some(4),
        "stays on the last entry"
    );
    h.key("k");
    assert_eq!(
        h.read(|d| d.widgets.desk.tray.menu.as_ref().and_then(|m| m.cursor)),
        Some(0)
    );
    // Other keys don't leak to the widget while the menu is up.
    h.key("l");
    assert_eq!(
        h.read(|d| d.widgets.desk.tray.scopes.get(d.nav.active_id).cursor),
        1
    );
    h.key("esc");
    assert!(!h.has("Tray menu"), "Escape closes the menu");
    assert!(in_widget(&h), "still in the tray");
    assert!(h.controls().is_empty(), "and nothing more");
    // Again, and choose.
    h.key("m");
    h.send(menu(menu_entries()));
    h.key("j");
    h.key("j");
    h.key("enter");
    let sent = commands_tray(&mut h);
    assert!(
        sent.iter()
            .any(|c| c.contains("MenuItem") && c.contains("id: 4")),
        "{sent:?}"
    );
    assert!(!h.has("Tray menu"));
    assert_eq!(h.controls(), [Control::Hide]);
}

// ---- windows -------------------------------------------------------------------------------

fn many_windows(n: i64) -> windows::State {
    let mut w = Demo::new().windows;
    let row = w.rows[1].clone();
    w.rows = (0..n as u64)
        .map(|i| windows::Row {
            id: i + 1,
            title: format!("Window {i}"),
            ..row.clone()
        })
        .collect();
    w.count = n as usize;
    w
}

/// Clicking a window goes to it and puts the dashboard away; the close button and a middle click
/// close it.
fn windows_click_focus_close() {
    let mut h = dashboard();
    h.send(windows::Event::State(Demo::new().windows));
    h.click_on("Mozilla Firefox");
    assert_eq!(commands_windows(&mut h), ["Focus(12)"]);
    assert_eq!(h.controls(), [Control::Hide]);
    h.click_on("Close Mozilla Firefox");
    assert_eq!(commands_windows(&mut h), ["Close(12)"]);
    assert_eq!(h.controls(), [], "closing a window leaves the dashboard up");
    let c = h.bounds_of("Mozilla Firefox").expect("row").center();
    h.click_with(f32::from(c.x) - 50.0, f32::from(c.y), MouseButton::Middle);
    assert_eq!(commands_windows(&mut h), ["Close(12)"]);
}

/// j/k pass the workspace headings; Enter focuses, l then Enter closes, x and d close.
fn windows_keys() {
    let mut h = dashboard();
    h.send(windows::Event::State(Demo::new().windows));
    enter(&mut h, "Windows");
    let cursor =
        |h: &Harness| h.read(|d| d.widgets.desk.windows.scopes.get(d.nav.active_id).cursor);
    assert_eq!(cursor(&h), 1, "starts past the heading");
    h.key("j");
    assert_eq!(cursor(&h), 2);
    h.key("j");
    assert_eq!(cursor(&h), 4, "past the second workspace's heading");
    h.key("k");
    assert_eq!(cursor(&h), 2);
    h.key("enter");
    assert_eq!(commands_windows(&mut h), ["Focus(12)"]);
    assert_eq!(h.controls(), [Control::Hide]);
    h.key("l");
    h.key("enter");
    assert_eq!(commands_windows(&mut h), ["Close(12)"]);
    h.key("h");
    h.key("x");
    assert_eq!(commands_windows(&mut h), ["Close(12)"]);
    h.key("g");
    assert_eq!(cursor(&h), 1);
    h.key("d");
    assert_eq!(commands_windows(&mut h), ["Close(11)"]);
}

/// A window moving to another workspace puts a heading where the cursor was: it moves on.
fn windows_cursor_leaves_a_heading() {
    let mut h = dashboard();
    let mut w = Demo::new().windows;
    h.send(windows::Event::State(w.clone()));
    enter(&mut h, "Windows");
    h.key("j");
    h.key("j");
    // Rows: [1] 11, 12 | [2] 21. The cursor is on 21 (row 4). 21 goes away.
    w.rows.truncate(3);
    h.send(windows::Event::State(w));
    let c = h.read(|d| d.widgets.desk.windows.scopes.get(d.nav.active_id).cursor);
    assert!(c == 2, "back on the last window, not on a heading: {c}");
}

/// Without a window list the widget says so instead of listing.
fn windows_unavailable() {
    let mut h = alone("windows", 4, 3);
    let mut w = Demo::new().windows;
    w.connected = false;
    h.send(windows::Event::State(w));
    assert!(!h.has("Mozilla Firefox"));
}

// ---- Claude sessions -----------------------------------------------------------------------

/// The Claude sessions, entered, the first `with_window` of them with a window to go to.
fn claude_sessions(with_window: usize) -> Harness {
    let mut h = alone("claude-sessions", 5, 4);
    let mut state = Demo::new().claude;
    for (i, s) in state.sessions.iter_mut().enumerate() {
        if i >= with_window {
            s.window = None;
        }
    }
    h.send(claude::Event::Sessions(state));
    enter(&mut h, "Claude sessions");
    h
}

fn session_cursor(h: &Harness) -> usize {
    h.read(|d| d.widgets.desk.claude.scopes.get(d.nav.active_id).cursor)
}

/// j and k walk the list, whether or not a session has a window to go to.
fn claude_sessions_walk() {
    for with_window in [0, 1, 2] {
        let mut h = claude_sessions(with_window);
        assert_eq!(
            session_cursor(&h),
            0,
            "{with_window} with a window: starts on the first"
        );
        h.key("j");
        assert_eq!(session_cursor(&h), 1, "{with_window} with a window: j");
        h.key("k");
        assert_eq!(session_cursor(&h), 0, "{with_window} with a window: k");
    }
}

/// Enter goes to a session's window; one without stays put rather than hiding the dashboard.
fn claude_session_enter() {
    let mut h = claude_sessions(1);
    h.key("j");
    h.key("enter");
    assert!(commands_windows(&mut h).is_empty(), "focused nothing");
    assert_eq!(h.controls(), [], "hid the dashboard for nothing");
    h.key("k");
    h.key("enter");
    assert_eq!(commands_windows(&mut h), ["ShowTab(4101)", "Focus(4101)"]);
    assert_eq!(h.controls(), [Control::Hide]);
}

/// A click on a session with a window goes there; one without does nothing.
fn claude_session_click() {
    let mut h = claude_sessions(1);
    h.click_on("Keyboard navigation");
    assert!(commands_windows(&mut h).is_empty());
    assert_eq!(h.controls(), []);
    h.click_on("Split off the system");
    assert_eq!(commands_windows(&mut h), ["ShowTab(4101)", "Focus(4101)"]);
    assert_eq!(h.controls(), [Control::Hide]);
}

// ---- cursor in sight -----------------------------------------------------------------------

/// Walk the cursor to the last stop; every stop passed has to be inside its list.
fn walk_in_sight(h: &mut Harness, in_sight: impl Fn(&Harness) -> (usize, Option<bool>)) {
    let mut last = in_sight(h).0;
    for step in 0..40 {
        h.key("j");
        let (now, visible) = in_sight(h);
        assert_eq!(
            visible,
            Some(true),
            "step {step}: the cursor is on {now}, out of sight"
        );
        if now == last && step > 0 {
            return;
        }
        last = now;
    }
    panic!("never reached the last stop");
}

/// Short tiles: lists that scroll, and the cursor stays in sight all the way down and up.
fn stops_in_sight_lists() {
    let mut h = alone("windows", 3, 2);
    h.send(windows::Event::State(many_windows(12)));
    enter(&mut h, "Windows");
    let probe = |h: &Harness| {
        h.read(|d| {
            let id = d.nav.active_id;
            let c = d.widgets.desk.windows.scopes.get(id).cursor;
            (c, d.widgets.desk.windows.lists.of(id).in_sight(c))
        })
    };
    walk_in_sight(&mut h, probe);
    assert!(probe(&h).0 >= 11);
    for _ in 0..14 {
        h.key("k");
        assert_eq!(probe(&h).1, Some(true), "going up, on {}", probe(&h).0);
    }
    assert_eq!(probe(&h).0, 0);

    let mut h = alone("claude-sessions", 3, 2);
    let mut c = Demo::new().claude;
    let session = c.sessions[0].clone();
    c.sessions = (0..12)
        .map(|i| claude::Session {
            pid: i,
            title: format!("Session {i}"),
            window: Some(1),
            ..session.clone()
        })
        .collect();
    h.send(claude::Event::Sessions(c));
    enter(&mut h, "Claude sessions");
    let probe = |h: &Harness| {
        h.read(|d| {
            let id = d.nav.active_id;
            let c = d.widgets.desk.claude.scopes.get(id).cursor;
            (c, d.widgets.desk.claude.lists.of(id).in_sight(c))
        })
    };
    walk_in_sight(&mut h, probe);
    assert!(probe(&h).0 >= 11);

    let mut h = alone("notifications", 3, 2);
    let mut n = Demo::new().notifications;
    let one = n.items[0].clone();
    n.items = (0..10)
        .map(|i| Notification {
            id: i,
            summary: format!("Note {i}"),
            ..one.clone()
        })
        .collect();
    h.send(notifications::Event::State(n));
    enter(&mut h, "Notifications");
    let probe = |h: &Harness| {
        h.read(|d| {
            let id = d.nav.active_id;
            let n = &d.widgets.desk.notifications;
            let c = n.inst(id).scope.cursor;
            (c, n.lists.of(id).in_sight(c))
        })
    };
    walk_in_sight(&mut h, probe);
    assert!(probe(&h).0 >= 9);
}

/// A list growing above the row the cursor is on scrolls, so the cursor stays in sight.
fn cursor_stays_in_sight_as_a_list_grows() {
    let mut h = alone("windows", 3, 2);
    h.send(windows::Event::State(many_windows(12)));
    enter(&mut h, "Windows");
    h.key("G");
    let probe = |h: &Harness| {
        h.read(|d| {
            let id = d.nav.active_id;
            let c = d.widgets.desk.windows.scopes.get(id).cursor;
            (c, d.widgets.desk.windows.lists.of(id).in_sight(c))
        })
    };
    assert_eq!(probe(&h), (11, Some(true)));
    // Six more windows come in above the cursor's row (new ones sort first).
    let mut w = many_windows(18);
    for (i, r) in w.rows.iter_mut().enumerate() {
        r.title = format!("Window {i}");
    }
    h.send(windows::Event::State(w));
    h.draw();
    let (c, visible) = probe(&h);
    assert_eq!(visible, Some(true), "cursor on {c} fell out of sight");
}

// ---- notifications -------------------------------------------------------------------------

fn note(id: u32, summary: &str, actions: &[(&str, &str)]) -> Notification {
    Notification {
        id,
        app: "Mail".into(),
        summary: summary.into(),
        body: "Body".into(),
        icon: IconSrc::None,
        actions: actions
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect(),
        urgent: false,
        time: chrono::Local::now(),
    }
}

fn notif_state(items: Vec<Notification>) -> notifications::State {
    notifications::State {
        server: true,
        can_pause: true,
        items,
        dnd: Dnd::Off,
        missed: 0,
    }
}

/// Dismiss, clear and a notification's default action, by mouse and by keys.
fn notifications_commands() {
    let mut h = dashboard();
    h.send(notifications::Event::State(notif_state(vec![
        note(5, "Build done", &[("default", "Open")]),
        note(6, "Mail", &[]),
    ])));
    h.click_on("Dismiss Mail");
    assert_eq!(commands_notifications(&mut h), ["Dismiss(6)"]);
    h.click_on("Clear notifications");
    assert_eq!(commands_notifications(&mut h), ["Clear"]);
    // A click on one with a default action invokes it and gets out of the way; one without
    // does nothing.
    h.click_on("Mail: Mail");
    assert!(commands_notifications(&mut h).is_empty());
    assert_eq!(h.controls(), []);
    h.click_on("Mail: Build done");
    assert_eq!(commands_notifications(&mut h), ["Invoke(5)"]);
    assert_eq!(h.controls(), [Control::Hide]);
    // Keys.
    enter(&mut h, "Notifications");
    h.key("enter");
    assert_eq!(commands_notifications(&mut h), ["Invoke(5)"]);
    assert_eq!(h.controls(), [Control::Hide]);
    h.key("l");
    h.key("enter");
    assert_eq!(commands_notifications(&mut h), ["Dismiss(5)"]);
    h.key("h");
    h.key("j");
    h.key("d");
    assert_eq!(commands_notifications(&mut h), ["Dismiss(6)"]);
    h.key("D");
    assert_eq!(commands_notifications(&mut h), ["Clear"]);
}

/// The bell shows the pause choices, which come first among the stops; choosing one sets do not
/// disturb and hides them. z does the same from the keyboard, Escape closes them.
fn notifications_pause_choices() {
    let mut h = alone("notifications", 5, 4);
    h.send(notifications::Event::State(notif_state(vec![note(
        5,
        "Build done",
        &[],
    )])));
    assert!(h.has("Pause notifications"));
    assert!(!h.has("Pause notifications for 1 hour"));
    h.click_on("Pause notifications");
    assert!(h.has("Pause notifications for 1 hour"));
    assert!(h.has("Pause notifications until turned off"));
    assert!(!h.has("Stop pausing notifications"), "nothing to stop yet");
    h.click_on("Pause notifications for 1 hour");
    let sent = commands_notifications(&mut h);
    assert!(
        sent.len() == 1 && sent[0].contains("SetDnd(Until("),
        "{sent:?}"
    );
    assert!(
        !h.has("Pause notifications for 1 hour"),
        "the choices are gone"
    );
    // Keyboard.
    enter(&mut h, "Notifications");
    h.key("z");
    assert!(h.has("Pause notifications for 1 hour"));
    let cursor = |h: &Harness| {
        h.read(|d| {
            d.widgets
                .desk
                .notifications
                .inst(d.nav.active_id)
                .scope
                .cursor
        })
    };
    assert_eq!(cursor(&h), 0, "the choices come first");
    h.key("j");
    h.key("j");
    h.key("enter");
    let sent = commands_notifications(&mut h);
    assert_eq!(sent, ["SetDnd(Forever)"], "third choice: until turned off");
    assert!(!h.has("Pause notifications for 1 hour"));
    h.key("z");
    h.key("esc");
    assert!(
        !h.has("Pause notifications for 1 hour"),
        "Escape closes the choices"
    );
    assert!(in_widget(&h), "Escape closed the choices, not the widget");
}

/// Do not disturb, as the server reports it: shown and kept for the next start; the control
/// toggles it and says so on screen; a saved pause is handed to the server at the start.
fn notifications_dnd() {
    let mut h = alone("notifications", 5, 4);
    let mut s = notif_state(vec![note(5, "Build done", &[])]);
    s.dnd = Dnd::Forever;
    s.missed = 3;
    h.send(notifications::Event::State(s));
    assert_eq!(
        h.read(|d| d.state.dnd_until),
        Some(i64::MAX),
        "remembered for restarts"
    );
    assert!(h.read(|d| d.widgets.desk.notifications.dnd_active));
    // Off is a choice while paused.
    h.click_on("Change notification pause");
    assert!(h.has("Stop pausing notifications"));
    h.click_on("Stop pausing notifications");
    assert_eq!(commands_notifications(&mut h), ["SetDnd(Off)"]);
    h.send(notifications::Event::State(notif_state(Vec::new())));
    assert_eq!(h.read(|d| d.state.dnd_until), None);
    // The control: on, with an on-screen message.
    h.control(Control::Dnd);
    let sent = commands_notifications(&mut h);
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert_eq!(sent[0], "SetDnd(Forever)");
    assert!(
        sent[1].contains("Osd") && sent[1].contains("Do not disturb"),
        "{sent:?}"
    );
    // And back off, once the server has said it is on.
    let mut s = notif_state(Vec::new());
    s.dnd = Dnd::Forever;
    h.send(notifications::Event::State(s));
    h.control(Control::Dnd);
    let sent = commands_notifications(&mut h);
    assert_eq!(sent[0], "SetDnd(Off)");
    assert!(sent[1].contains("Notifications back on"), "{sent:?}");
}

// ---- media ---------------------------------------------------------------------------------

fn playing() -> media::State {
    media::State {
        bus_name: "org.mpris.MediaPlayer2.demo".into(),
        player: "Demo".into(),
        title: "A Song".into(),
        artist: "Somebody".into(),
        playing: true,
        art: Some(IconSrc::Rgba {
            width: 8,
            height: 8,
            data: std::sync::Arc::new(vec![180; 8 * 8 * 4]),
        }),
        progress: 0.4,
    }
}

/// The controls send play/pause, next and previous; the keys do the same.
fn media_controls() {
    let mut h = alone("media", 5, 3);
    h.send(media::Event::State(Some(playing())));
    h.click_on("Pause");
    assert_eq!(commands_media(&mut h), ["PlayPause"]);
    h.click_on("Next track");
    h.click_on("Previous track");
    assert_eq!(commands_media(&mut h), ["Next", "Previous"]);
    enter(&mut h, "Media player");
    h.key("l");
    h.key("h");
    h.key("space");
    h.key("enter");
    assert_eq!(
        commands_media(&mut h),
        ["Next", "Previous", "PlayPause", "PlayPause"]
    );
    // Paused it says Play; with no player the controls do nothing.
    let mut p = playing();
    p.playing = false;
    h.send(media::Event::State(Some(p)));
    assert!(h.has("Play"));
    h.send(media::Event::State(None));
    h.click_on("Next track");
    assert!(
        commands_media(&mut h).is_empty(),
        "disabled without a player"
    );
}

/// A single cell tall shows the controls beside the title; round shows a ring.
fn media_shapes() {
    for config in [
        "grid columns=16 rows=9\nlayout {\n    media col=0 row=0 width=5 height=1\n}\n",
        "grid columns=16 rows=9\nlayout {\n    media col=0 row=0 size=3 shape=\"round\"\n}\n",
        "grid columns=16 rows=9\nlayout {\n    media col=0 row=0 width=5 height=4\n}\n",
    ] {
        let mut h = up(config);
        h.send(media::Event::State(Some(playing())));
        assert!(h.has("Pause"), "{config}");
        h.send(media::Event::State(None));
        assert!(h.has("Play"), "{config}");
    }
}

/// How far the play button is from the bottom of the media card.
fn media_gap_below(h: &Harness) -> f32 {
    let (_, y, _, height) = h.read(|d| d.card_rect(d.tiles[0].id())).expect("the card");
    let play = h.bounds_of("Pause").expect("the play button");
    y + height - f32::from(play.bottom())
}

/// With art the media card's top takes the room and the controls sit at the bottom; without it
/// the text, the bar and the controls are one block in the middle, as in Slint.
fn media_layout_with_and_without_art() {
    let mut h = alone("media", 5, 3);
    h.send(media::Event::State(Some(playing())));
    let with = media_gap_below(&h);
    assert!(
        with < 15.0,
        "with art the controls are {with} px from the bottom"
    );
    let mut plain = playing();
    plain.art = None;
    h.send(media::Event::State(Some(plain)));
    let without = media_gap_below(&h);
    assert!(
        without > 30.0,
        "without art the controls are {without} px from the bottom"
    );
}

/// A double click on a control is not a double click on the card behind it: the widget is not
/// picked up (the carry). Rows, buttons and the media controls.
fn double_click_on_controls_does_not_carry() {
    let mut h = dashboard();
    h.send(windows::Event::State(Demo::new().windows));
    h.send(media::Event::State(Some(playing())));
    for control in [
        "Mozilla Firefox",
        "Close Mozilla Firefox",
        "Pause",
        "Next track",
    ] {
        let Some(b) = h.bounds_of(control) else {
            continue;
        };
        let c = b.center();
        h.double_click(f32::from(c.x), f32::from(c.y));
        assert_eq!(
            h.adjust_id(),
            0,
            "a double click on {control:?} picked the widget up"
        );
    }
    assert!(
        h.has("Pause") || h.has("Play"),
        "the media controls were never exercised"
    );
}

// ---- stress --------------------------------------------------------------------------------

/// Random keys, clicks and wheel turns on a dashboard of nothing but the desk's widgets, on the
/// mock system's data, with their invariants checked after each one (`desk::check`). Every
/// shape: tall, short, round, on a large screen and a small one.
fn desk_stress() {
    let config = r#"
grid columns=16 rows=9
layout {
    tray            col=0  row=0 width=5 height=1
    windows         col=0  row=1 width=3 height=2
    notifications   col=6  row=0 width=4 height=3
    media           col=11 row=0 width=5 height=1
    claude-sessions col=0  row=4 width=3 height=2
    media           col=11 row=2 size=3 shape="round"
    notifications   col=4  row=4 size=3 shape="round"
    tray            col=7  row=4 size=2 shape="round"
    windows         col=11 row=5 width=5 height=4
    claude-sessions col=7  row=7 width=3 height=1
}
"#;
    let ops = std::env::var("NIRI_DASH_STRESS_OPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(800);
    for seed in [11, 12, 13] {
        for size in [(1706, 1066), (1280, 800)] {
            let run = Harness::with_mock_sized(config, size)
                .stress(
                    niri_dash_gpui::testing::Monkey::new(seed, false).keep_going(),
                    ops,
                )
                .and_then(|m| m.finish());
            if let Err(failure) = run {
                panic!("seed {seed} on {size:?}: {failure}");
            }
        }
    }
}

/// The mock system never answers a request for a menu, so the stress run never has one open:
/// keys, clicks and menus handed in at random, with the menu's cursor checked after each.
fn tray_menu_fuzz() {
    let mut h = dashboard();
    h.send(tray::Event::Items(Demo::new().tray));
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut next = move |n: u64| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state % n
    };
    let keys = [
        "f",
        "j",
        "k",
        "l",
        "h",
        "m",
        "enter",
        "space",
        "esc",
        "q",
        "backspace",
        "down",
        "up",
        "g",
        "G",
        "tab",
        "x",
    ];
    enter(&mut h, "Tray");
    for step in 0..600 {
        match next(10) {
            0 => {
                let entries = menu_entries();
                let n = 1 + next(entries.len() as u64) as usize;
                h.send(menu(entries.into_iter().take(n).collect()));
            }
            1 => h.click(next(1700) as f32, next(1000) as f32),
            _ => {
                let k = keys[next(keys.len() as u64) as usize];
                h.key(k);
            }
        }
        for c in h.controls() {
            h.control(c);
        }
        let problem = h.read(|d| {
            let t = &d.widgets.desk.tray;
            t.menu.as_ref().and_then(|m| {
                let at = m.cursor?;
                let e = m.entries.get(at)?;
                (e.separator || !e.enabled || e.header).then(|| format!("cursor on entry {at}"))
            })
        });
        assert!(problem.is_none(), "step {step}: {problem:?}");
        let _ = commands_tray(&mut h);
        // Chosen entries and clicks on icons put the dashboard away: open it again.
        if !h.read(|d| d.visible) {
            h.control(Control::Show);
            enter(&mut h, "Tray");
        } else if !in_widget(&h) {
            enter(&mut h, "Tray");
        }
    }
}

// ---- the run -------------------------------------------------------------------------------

fn main() {
    let _sandbox = Sandbox::enter(None).expect("a temporary directory for the dashboard's files");
    // SAFETY: nothing else runs yet that could read the environment.
    unsafe { std::env::set_var("LC_ALL", "C") };
    let scenarios: &[(&str, fn())] = &[
        ("tray_activate", tray_activate),
        ("tray_menu_by_mouse", tray_menu_by_mouse),
        (
            "tray_menu_closes_on_a_click_outside",
            tray_menu_closes_on_a_click_outside,
        ),
        ("tray_empty_menu_says_so", tray_empty_menu_says_so),
        ("tray_menu_by_keyboard", tray_menu_by_keyboard),
        ("windows_click_focus_close", windows_click_focus_close),
        ("windows_keys", windows_keys),
        (
            "windows_cursor_leaves_a_heading",
            windows_cursor_leaves_a_heading,
        ),
        ("windows_unavailable", windows_unavailable),
        ("claude_sessions_walk", claude_sessions_walk),
        ("claude_session_enter", claude_session_enter),
        ("claude_session_click", claude_session_click),
        ("stops_in_sight_lists", stops_in_sight_lists),
        (
            "cursor_stays_in_sight_as_a_list_grows",
            cursor_stays_in_sight_as_a_list_grows,
        ),
        ("notifications_commands", notifications_commands),
        ("notifications_pause_choices", notifications_pause_choices),
        ("notifications_dnd", notifications_dnd),
        ("media_controls", media_controls),
        ("media_shapes", media_shapes),
        (
            "double_click_on_controls_does_not_carry",
            double_click_on_controls_does_not_carry,
        ),
        (
            "media_layout_with_and_without_art",
            media_layout_with_and_without_art,
        ),
        ("tray_menu_fuzz", tray_menu_fuzz),
        ("desk_stress", desk_stress),
    ];
    let only = std::env::args().nth(1);
    let mut failed = 0;
    for (name, run) in scenarios {
        if only
            .as_deref()
            .is_some_and(|o| !name.contains(o) && !o.starts_with("--"))
        {
            continue;
        }
        match std::panic::catch_unwind(AssertUnwindSafe(run)) {
            Ok(()) => println!("test {name} ... ok"),
            Err(_) => {
                failed += 1;
                println!("test {name} ... FAILED");
            }
        }
    }
    println!(
        "\ntest result: {}. {failed} failed",
        if failed == 0 { "ok" } else { "FAILED" }
    );
    if failed > 0 {
        std::process::exit(1);
    }
}

//! The controls widgets (battery, volume, brightness, night light, session) against channels
//! standing in for the system: what they send when they are used, and what they show when the
//! system reports something. A plain program, one scenario after the other on a fresh dashboard
//! (see `tests/stress.rs`).
//!
//! Ported from `legacy/tests/ui.rs`: volume_keys, brightness_drag_outlasts_stale_level,
//! session_lock_hides_first, battery_without_a_current_profile, a11y_brightness_knob and
//! a11y_lock_button; and new ones for what only the mouse did there (sliders, the mixer's
//! picker, the confirm flow, night light).

use niri_dash::Control;
use niri_dash::testing::{Harness, Sandbox};
use niri_dash_system::night_light::{self as gamma, Cmd, NightLightState};
use niri_dash_system::{Demo, audio, brightness, notifications, power, session};
use std::panic::AssertUnwindSafe;

/// A dashboard of the widgets in `layout`, up on a large screen.
fn dashboard(layout: &str) -> Harness {
    let mut h = Harness::new(&format!("grid columns=16 rows=9\nlayout {{\n{layout}\n}}\n"));
    h.control(Control::Show);
    h
}

/// Enter the widget of kind `kind` with its hint label.
fn enter(h: &mut Harness, kind: &str) {
    let hint = h.read(|d| {
        d.tiles
            .iter()
            .find(|t| t.kind().name() == kind)
            .unwrap_or_else(|| panic!("no {kind} on the dashboard"))
            .hint
            .clone()
    });
    h.key("f");
    for c in hint.chars() {
        h.key(&c.to_string());
    }
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.001
}

/// Where a click lands a slider's knob at `at` (0 to 1): its x, and the middle of its height.
fn slider_at(h: &Harness, selector: &'static str, at: f32) -> (f32, f32) {
    let b = h
        .bounds_of(selector)
        .unwrap_or_else(|| panic!("no slider called {selector:?}"));
    let (left, width) = (f32::from(b.left()), f32::from(b.size.width));
    let y = f32::from(b.center().y);
    (left + 8.0 + at * (width - 16.0), y)
}

/// The commands the volume widget has sent, in order.
fn audio_sent(h: &mut Harness) -> Vec<audio::Command> {
    std::iter::from_fn(|| h.inbox.audio.try_recv().ok()).collect()
}

fn brightness_sent(h: &mut Harness) -> Vec<brightness::Command> {
    std::iter::from_fn(|| h.inbox.brightness.try_recv().ok()).collect()
}

fn session_sent(h: &mut Harness) -> Vec<session::Command> {
    std::iter::from_fn(|| h.inbox.session.try_recv().ok()).collect()
}

fn power_sent(h: &mut Harness) -> Vec<power::Command> {
    std::iter::from_fn(|| h.inbox.power.try_recv().ok()).collect()
}

fn gamma_sent(h: &mut Harness) -> Vec<gamma::Command> {
    std::iter::from_fn(|| h.inbox.night_light.try_recv().ok()).collect()
}

/// In the volume widget j is quieter and k louder, on the slider under the cursor; Tab goes to
/// the next slider.
fn volume_keys() {
    let mut h = dashboard("    volume col=0 row=0 width=4 height=4");
    h.send(audio::Event::State(audio::State {
        volume: 0.5,
        mic: Some((0.5, false)),
        ..Default::default()
    }));
    enter(&mut h, "volume");
    let levels = |h: &Harness| h.read(|d| (d.widgets.controls.volume.volume, d.widgets.controls.volume.mic_volume));
    h.key("j");
    assert!(near(levels(&h).0, 0.45), "j: {:?}", levels(&h));
    h.key("k");
    h.key("k");
    assert!(near(levels(&h).0, 0.55), "k: {:?}", levels(&h));
    h.key("tab");
    h.key("j");
    assert!(near(levels(&h).0, 0.55), "Tab then j: {:?}", levels(&h));
    assert!(near(levels(&h).1, 0.45), "Tab then j: {:?}", levels(&h));
    // Digits set 10 to 100 %, m mutes.
    h.key("7");
    assert!(near(levels(&h).1, 0.7), "7: {:?}", levels(&h));
    h.key("0");
    assert!(near(levels(&h).1, 1.0), "0: {:?}", levels(&h));
    audio_sent(&mut h);
    h.key("m");
    assert!(matches!(audio_sent(&mut h)[..], [audio::Command::ToggleMicMute]));
    h.key("h");
    h.key("l");
    h.key("l");
    assert!(near(levels(&h).1, 1.0), "clamped: {:?}", levels(&h));
}

/// The round volume widget is a knob: the keys act on it, a click mutes.
fn volume_knob() {
    let mut h = dashboard("    volume col=0 row=0 size=3 shape=\"round\"");
    h.send(audio::Event::State(audio::State {
        volume: 0.5,
        ..Default::default()
    }));
    assert!(h.has("Volume"), "the knob is named by what it sets");
    h.click_on("Volume");
    assert!(matches!(audio_sent(&mut h)[..], [audio::Command::ToggleMute]));
    enter(&mut h, "volume");
    h.key("k");
    h.key("k");
    assert!(near(h.read(|d| d.widgets.controls.volume.volume), 0.6));
    assert!(matches!(
        audio_sent(&mut h)[..],
        [audio::Command::SetVolume(a), audio::Command::SetVolume(b)] if near(a, 0.55) && near(b, 0.6)
    ));
    h.key("3");
    assert!(near(h.read(|d| d.widgets.controls.volume.volume), 0.3));
}

/// A press on a slider sets the value where it landed, the wheel steps it, and a slider that was
/// just set keeps its value against a level the source reports late.
fn volume_slider_mouse_and_hold() {
    let mut h = dashboard("    volume col=0 row=0 width=4 height=4");
    h.send(audio::Event::State(audio::State {
        volume: 0.5,
        mic: Some((0.5, false)),
        ..Default::default()
    }));
    let (x, y) = slider_at(&h, "Volume", 0.25);
    h.click(x, y);
    let sent = audio_sent(&mut h);
    assert!(
        matches!(sent[..], [audio::Command::SetVolume(v)] if (v - 0.25).abs() < 0.02),
        "{sent:?}"
    );
    let set = h.read(|d| d.widgets.controls.volume.volume);
    // The source is late with an old level: the slider stays where the user put it ...
    h.send(audio::Event::State(audio::State {
        volume: 0.5,
        mic: Some((0.5, false)),
        ..Default::default()
    }));
    assert!(near(h.read(|d| d.widgets.controls.volume.volume), set));
    // ... for a moment, then it shows what the source says.
    h.advance(3);
    h.send(audio::Event::State(audio::State {
        volume: 0.5,
        mic: Some((0.5, false)),
        ..Default::default()
    }));
    assert!(near(h.read(|d| d.widgets.controls.volume.volume), 0.5));
    // The wheel: up is louder.
    let (x, y) = slider_at(&h, "Volume", 0.5);
    h.scroll(x, y, 96.0);
    let sent = audio_sent(&mut h);
    assert!(
        matches!(sent[..], [audio::Command::SetVolume(v)] if v > 0.5),
        "wheel up: {sent:?}"
    );
    h.scroll(x, y, -192.0);
    let sent = audio_sent(&mut h);
    assert!(
        matches!(sent[..], [audio::Command::SetVolume(v)] if v < 0.5),
        "wheel down: {sent:?}"
    );
    // The mute button of the row.
    h.click_on("Mute");
    assert!(matches!(audio_sent(&mut h)[..], [audio::Command::ToggleMute]));
}

fn sink(id: u32, name: &str, default: bool) -> audio::Sink {
    audio::Sink {
        id,
        name: name.into(),
        default,
    }
}

fn stream(id: u32, app: &str, volume: f32) -> audio::Stream {
    audio::Stream {
        id,
        app: app.into(),
        icon: niri_dash_system::IconSrc::None,
        volume,
        muted: false,
    }
}

/// The mixer: the output picker (Enter opens it, Enter again picks, Escape closes it before it
/// leaves the widget), and a slider per app that the keys reach after the output and the
/// microphone.
fn volume_mixer() {
    let mut h = dashboard("    volume col=0 row=0 width=4 height=4");
    h.send(audio::Event::State(audio::State {
        volume: 0.5,
        mic: Some((0.5, false)),
        sink_name: "Speakers".into(),
        sinks: vec![sink(1, "Speakers", true), sink(2, "Headphones", false)],
        streams: vec![stream(10, "Firefox", 0.4), stream(11, "Spotify", 0.8)],
        ..Default::default()
    }));
    assert!(h.has("Choose output") && !h.has("Firefox, muted") && h.has("Firefox"));
    enter(&mut h, "volume");
    // Stop 0 is the picker.
    h.key("enter");
    assert!(h.has("Close output list") && h.has("Headphones"));
    assert!(!h.has("Firefox"), "the apps give way to the outputs");
    h.key("esc");
    assert!(h.has("Choose output"), "Escape closed the list");
    assert_eq!(h.read(|d| format!("{:?}", d.nav.mode)), "Widget", "still in the widget");
    h.key("enter");
    // The cursor goes down the outputs: stop 1 is Speakers, stop 2 Headphones.
    h.key("j");
    h.key("j");
    h.key("enter");
    assert!(matches!(audio_sent(&mut h)[..], [audio::Command::SetDefaultSink(2)]));
    assert!(h.has("Choose output"), "picking closes it");
    assert_eq!(h.read(|d| d.widgets.controls.volume.scope.cursor), 1, "back on the output slider");
    // Output, microphone, then the apps: j on an app is quieter.
    h.key("tab");
    h.key("tab");
    h.key("j");
    let sent = audio_sent(&mut h);
    assert!(
        matches!(sent[..], [audio::Command::SetStreamVolume(10, v)] if near(v, 0.35)),
        "{sent:?}"
    );
    h.key("tab");
    h.key("m");
    assert!(matches!(audio_sent(&mut h)[..], [audio::Command::ToggleStreamMute(11)]));
    // An app's slider that was just set keeps its value against the lagging level.
    h.send(audio::Event::State(audio::State {
        volume: 0.5,
        mic: Some((0.5, false)),
        sinks: vec![sink(1, "Speakers", false), sink(2, "Headphones", true)],
        streams: vec![stream(10, "Firefox", 0.4), stream(11, "Spotify", 0.8)],
        ..Default::default()
    }));
    let firefox = h.read(|d| d.widgets.controls.volume.streams[0].volume);
    assert!(near(firefox, 0.35), "held: {firefox}");
}

/// A dragged slider keeps its value against a level the backlight reports late.
fn brightness_drag_outlasts_stale_level() {
    let mut h = dashboard("    brightness col=0 row=0 width=4 height=1");
    h.send(brightness::Event::Level(Some(0.7)));
    let (x, y) = slider_at(&h, "Brightness", 0.3);
    h.click(x, y);
    let sent = brightness_sent(&mut h);
    assert!(
        matches!(sent[..], [brightness::Command::Set(v)] if (v - 0.3).abs() < 0.02),
        "{sent:?}"
    );
    let set = h.read(|d| d.widgets.controls.brightness.level);
    h.send(brightness::Event::Level(Some(0.9)));
    assert!(h.read(|d| d.widgets.controls.brightness.available));
    assert!(near(h.read(|d| d.widgets.controls.brightness.level), set));
    h.advance(3);
    h.send(brightness::Event::Level(Some(0.9)));
    assert!(near(h.read(|d| d.widgets.controls.brightness.level), 0.9));
}

/// The brightness knob: its keys, the wheel, a click.
fn brightness_knob() {
    let mut h = dashboard("    brightness col=0 row=0 size=2 shape=\"round\"");
    h.send(brightness::Event::Level(Some(0.7)));
    assert!(h.has("Brightness"));
    enter(&mut h, "brightness");
    h.key("l");
    assert!(matches!(
        brightness_sent(&mut h)[..],
        [brightness::Command::Set(v)] if near(v, 0.75)
    ));
    h.key("j");
    h.key("j");
    assert!(near(h.read(|d| d.widgets.controls.brightness.level), 0.65));
    h.key("5");
    h.key("enter");
    // Dim at 50 %: Enter (the click) goes to full.
    let sent = brightness_sent(&mut h);
    assert!(matches!(sent.last(), Some(brightness::Command::Set(v)) if near(*v, 1.0)), "{sent:?}");
    h.click_on("Brightness");
    let sent = brightness_sent(&mut h);
    assert!(matches!(sent[..], [brightness::Command::Set(v)] if near(v, 0.3)), "{sent:?}");
    // Without a backlight the slider is off, and the keys still step.
    h.send(brightness::Event::Level(None));
    assert!(!h.read(|d| d.widgets.controls.brightness.available));
}

/// Locking hides the dashboard, then locks.
fn session_lock_hides_first() {
    let mut h = dashboard("    session col=0 row=0 width=4 height=1");
    h.click_on("Lock");
    assert_eq!(h.controls(), [Control::Hide]);
    assert!(matches!(session_sent(&mut h)[..], [session::Command::Lock]));
}

/// Restart and power off ask twice (within three seconds); the others don't, and Enter from the
/// keyboard goes the same way.
fn session_confirm() {
    let mut h = dashboard("    session col=0 row=0 width=5 height=1");
    h.click_on("Power off");
    assert!(session_sent(&mut h).is_empty(), "armed, not done");
    h.click_on("Power off");
    assert!(matches!(session_sent(&mut h)[..], [session::Command::Poweroff]));
    assert!(h.controls().is_empty(), "power off doesn't hide first");
    // Armed, and let go: it disarms after three seconds.
    h.click_on("Restart");
    h.advance(4);
    h.click_on("Restart");
    assert!(session_sent(&mut h).is_empty(), "a late second press arms again");
    h.click_on("Restart");
    assert!(matches!(session_sent(&mut h)[..], [session::Command::Reboot]));
    // Suspend and log out hide first; the keyboard walks the buttons.
    enter(&mut h, "session");
    h.key("l");
    h.key("enter");
    assert_eq!(h.controls(), [Control::Hide]);
    assert!(matches!(session_sent(&mut h)[..], [session::Command::Suspend]));
    h.key("l");
    h.key("enter");
    assert!(matches!(session_sent(&mut h)[..], [session::Command::Logout]));
    h.key("G");
    h.key("enter");
    h.key("enter");
    assert!(matches!(session_sent(&mut h)[..], [session::Command::Poweroff]));
}

/// No current profile: the cursor still shows on the choices, and Enter picks the one lit.
fn battery_without_a_current_profile() {
    let mut h = dashboard("    battery col=0 row=0 width=4 height=3");
    h.send(power::Event::State(power::State {
        profile: None,
        ..Demo::new().power
    }));
    enter(&mut h, "battery");
    h.key("enter");
    assert!(matches!(
        power_sent(&mut h)[..],
        [power::Command::SetProfile(power::Profile::PowerSaver)]
    ));
}

/// h and l step the profile (wrapping), j goes to the sleep switch and Enter flips it; clicks do
/// the same.
fn battery_profiles_and_inhibit() {
    let mut h = dashboard("    battery col=0 row=0 width=4 height=3");
    h.send(power::Event::State(Demo::new().power));
    enter(&mut h, "battery");
    h.key("l");
    assert!(matches!(
        power_sent(&mut h)[..],
        [power::Command::SetProfile(power::Profile::Performance)]
    ));
    h.key("h");
    assert!(matches!(
        power_sent(&mut h)[..],
        [power::Command::SetProfile(power::Profile::PowerSaver)]
    ));
    h.key("j");
    h.key("enter");
    assert!(matches!(power_sent(&mut h)[..], [power::Command::ToggleInhibit]));
    h.click_on("Saver");
    assert!(matches!(
        power_sent(&mut h)[..],
        [power::Command::SetProfile(power::Profile::PowerSaver)]
    ));
    h.click_on("Block sleep and screen locking");
    assert!(matches!(power_sent(&mut h)[..], [power::Command::ToggleInhibit]));
    // What blocks sleep is shown when there is room.
    let mut big = dashboard("    battery col=0 row=0 width=4 height=4");
    big.send(power::Event::State(Demo::new().power));
    big.send(power::Event::SleepBlockers(vec![power::Blocker {
        who: "Firefox".into(),
        why: "Playing video".into(),
    }]));
    assert!(big.has("Battery") && big.has("Balanced"));
}

/// The round battery cycles the profiles on a tap: saver, balanced, performance, saver.
fn battery_round_cycles() {
    let mut h = dashboard("    battery col=0 row=0 size=3 shape=\"round\"");
    h.send(power::Event::State(Demo::new().power));
    h.click_on("Change power profile");
    assert!(matches!(
        power_sent(&mut h)[..],
        [power::Command::SetProfile(power::Profile::Performance)]
    ));
    h.send(power::Event::State(power::State {
        profile: Some(power::Profile::PowerSaver),
        ..Demo::new().power
    }));
    h.click_on("Change power profile");
    assert!(matches!(
        power_sent(&mut h)[..],
        [power::Command::SetProfile(power::Profile::Balanced)]
    ));
}

/// Night light through our own gamma tables: the switch, the presets, the keys, the command line.
fn night_light_gamma() {
    let mut h = dashboard("    night-light col=0 row=0 width=4 height=2");
    // Neutral on start.
    assert!(matches!(gamma_sent(&mut h)[..], [gamma::Command::Gamma(None)]));
    h.click_on("Night light");
    assert!(matches!(gamma_sent(&mut h)[..], [gamma::Command::Gamma(Some(4500))]));
    assert!(h.read(|d| d.widgets.controls.night.on));
    h.click_on("3400 K");
    assert!(matches!(gamma_sent(&mut h)[..], [gamma::Command::Gamma(Some(3400))]));
    assert_eq!(h.read(|d| d.widgets.controls.night.kelvin), 3400);
    // The slider: a press at the warm end.
    let (x, y) = slider_at(&h, "Warmth", 1.0);
    h.click(x, y);
    assert!(matches!(gamma_sent(&mut h)[..], [gamma::Command::Gamma(Some(2500))]));
    // Keys: t toggles, h and l move the warmth on the slider by 5 % (200 K).
    enter(&mut h, "night-light");
    h.key("t");
    assert!(matches!(gamma_sent(&mut h)[..], [gamma::Command::Gamma(None)]));
    h.key("j");
    h.key("h");
    assert!(matches!(gamma_sent(&mut h)[..], [gamma::Command::Gamma(Some(2700))]));
    // The presets are parts of the last stop: l walks them, Enter sets one.
    h.key("j");
    h.key("l");
    h.key("enter");
    assert!(matches!(gamma_sent(&mut h)[..], [gamma::Command::Gamma(Some(4500))]));
    // `niri-dash night-light 3000` sets it and says so on screen.
    h.update(|d, cx| d.night_light_command(Cmd::Kelvin(3000), cx));
    assert!(matches!(gamma_sent(&mut h)[..], [gamma::Command::Gamma(Some(3000))]));
    let osd: Vec<_> = std::iter::from_fn(|| h.inbox.notifications.try_recv().ok()).collect();
    assert!(
        matches!(&osd[..], [notifications::Command::Osd { text, .. }] if text == "Night light on · 3000 K"),
        "{osd:?}"
    );
    // What was set is saved once it settles.
    h.advance(1);
    let saved = h.read(|d| d.state.night_light);
    assert!(saved.on && saved.kelvin == 3000);
}

/// On Plasma KWin has the night light: the widget shows its state, has no slider, and asks KWin.
fn night_light_kwin() {
    let mut h = dashboard("    night-light col=0 row=0 width=4 height=2");
    h.update(|d, _| d.caps.night_light_native = true);
    h.send(niri_dash_system::night_light::Event::Kwin(NightLightState {
        available: true,
        enabled: true,
        inhibited: false,
        paused_by_us: false,
        temperature: 3600,
    }));
    gamma_sent(&mut h);
    assert!(h.has("Night light"));
    assert!(!h.has("Warmth"), "KWin sets the temperature");
    assert!(!h.has("3400 K"));
    assert!(h.read(|d| d.widgets.controls.night.on));
    h.click_on("Night light");
    assert!(matches!(
        gamma_sent(&mut h)[..],
        [gamma::Command::Kwin { cmd: Cmd::Toggle, announce: false }]
    ));
    // Paused by another program: shown, not ours.
    h.send(niri_dash_system::night_light::Event::Kwin(NightLightState {
        available: true,
        enabled: true,
        inhibited: true,
        paused_by_us: false,
        temperature: 6500,
    }));
    let (status, note) = h.read(|d| {
        let n = &d.widgets.controls.night;
        (n.status.clone(), n.note.clone())
    });
    assert_eq!((status.as_str(), note.as_str()), ("Paused", "Paused by another program"));
}

fn top_of(h: &Harness, selector: &'static str) -> f32 {
    f32::from(
        h.bounds_of(selector)
            .unwrap_or_else(|| panic!("nothing called {selector:?} on screen"))
            .top(),
    )
}

fn width_of(h: &Harness, selector: &'static str) -> f32 {
    f32::from(
        h.bounds_of(selector)
            .unwrap_or_else(|| panic!("nothing called {selector:?} on screen"))
            .size
            .width,
    )
}

/// What the Slint widgets' layout did that a flexbox does not by itself: the profile row sits
/// under the ring whatever the height of the card (the ring row can't grow past its ring), and
/// each profile starts from the width of its name.
fn battery_layout_follows_slint() {
    let saver_top = |rows: u32| {
        let mut h = dashboard(&format!("    battery col=0 row=0 width=4 height={rows}"));
        h.send(power::Event::State(Demo::new().power));
        top_of(&h, "Saver")
    };
    assert!(near(saver_top(3), saver_top(4)), "the profiles moved with the height");
    let mut h = dashboard("    battery col=0 row=0 width=4 height=3");
    h.send(power::Event::State(Demo::new().power));
    assert!(
        width_of(&h, "Performance") > width_of(&h, "Saver"),
        "the profiles share the row equally instead of by their names"
    );
}

/// The session buttons are as wide as their labels and share what is left, as in Slint.
fn session_buttons_follow_their_labels() {
    let h = dashboard("    session col=0 row=0 width=5 height=1");
    assert!(
        width_of(&h, "Power off") > width_of(&h, "Lock"),
        "the buttons are all as wide as each other"
    );
}

/// A widget whose content is taller than its card starts at the top (Slint's centered layout
/// does); one that fits is centered. The night light's header is at the card's padding on a card
/// one cell high.
fn night_light_overflow_starts_at_the_top() {
    let h = dashboard("    night-light col=0 row=0 width=3 height=1");
    let (_, y, _, height) = h
        .read(|d| d.card_rect(d.tiles[0].id()))
        .expect("the night light's card");
    let switch = h.bounds_of("Night light").expect("the switch");
    let top = f32::from(switch.top()) - y;
    // Header 28 high under the padding; the switch 22 in the middle of it. Header, slider and
    // the space between them are 60 high.
    let overflows = height - 32.0 < 60.0;
    let expected = if overflows { 16.0 + 3.0 } else { 3.0 + (height - 60.0) / 2.0 };
    assert!(
        (top - expected).abs() < 1.5,
        "the switch is {top} px under the top of a card {height} px high, not {expected}"
    );
}

/// `Dash::card_size` and `card_rect` say how the frame draws a card, round or not.
fn card_geometry() {
    let h = dashboard(
        "    volume col=0 row=0 width=3 height=3\n    volume col=4 row=0 size=3 shape=\"round\"",
    );
    let (w, hh, round) = h.read(|d| d.card_size(d.tiles[0].id())).unwrap();
    assert!(!round && w > 0.0 && hh > 0.0);
    let (rw, rh, rround) = h.read(|d| d.card_size(d.tiles[1].id())).unwrap();
    assert!(rround && near(rw, rh));
    let (_, _, cw, ch) = h.read(|d| d.card_rect(d.tiles[0].id())).unwrap();
    assert!(near(cw, w) && near(ch, hh));
    // The slider row fills the card's width inside its padding.
    let slider = h.bounds_of("Volume").unwrap();
    assert!(f32::from(slider.size.width) < w);
}

fn main() {
    let _sandbox = Sandbox::enter(None).expect("a temporary directory for the dashboard's files");
    let scenarios: &[(&str, fn())] = &[
        ("battery: layout follows Slint", battery_layout_follows_slint),
        ("session: buttons follow their labels", session_buttons_follow_their_labels),
        ("night light: overflow starts at the top", night_light_overflow_starts_at_the_top),
        ("card geometry", card_geometry),
        ("volume: j and k", volume_keys),
        ("volume: the round knob", volume_knob),
        ("volume: slider mouse and hold", volume_slider_mouse_and_hold),
        ("volume: the mixer", volume_mixer),
        ("brightness: drag outlasts stale level", brightness_drag_outlasts_stale_level),
        ("brightness: the knob", brightness_knob),
        ("session: lock hides first", session_lock_hides_first),
        ("session: confirm", session_confirm),
        ("battery: no current profile", battery_without_a_current_profile),
        ("battery: profiles and inhibit", battery_profiles_and_inhibit),
        ("battery: round cycles", battery_round_cycles),
        ("night light: gamma", night_light_gamma),
        ("night light: KWin", night_light_kwin),
    ];
    let mut failed = 0;
    for (name, scenario) in scenarios {
        match std::panic::catch_unwind(AssertUnwindSafe(scenario)) {
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
        scenarios.len() - failed
    );
    if failed > 0 {
        std::process::exit(1);
    }
}

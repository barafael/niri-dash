//! Edit mode, on the mock system: long scripted and seeded sequences of what edit mode offers
//! (moving, resizing, reshaping, adding, removing, the grid, every widget setting), by pointer,
//! by keyboard and through the UI's callbacks. After each step the dashboard is checked against
//! itself (`Harness::problems`): the layout keeps its rules, the UI shows it, config.kdl says the
//! same and reads back cleanly, the hint labels work. Steps that do nothing although the UI
//! offers them, and things only a pointer can do, are reported as issues too.
//!
//! A plain program (`harness = false`): Slint wants one thread. Each scenario gets a fresh
//! dashboard and reports every issue it finds, not just the first.
//!
//! `NIRI_DASH_EDIT_SEEDS=1,2,3` and `NIRI_DASH_EDIT_OPS=400` set the random sequences.

use niri_dash::testing::{self, Harness, Sandbox, Tile};
use niri_dash::{Nav, NavMode, RowKind, SettingRow};
use slint::platform::PointerEventButton;
use slint::{ComponentHandle, Model, SharedString};
use std::collections::BTreeSet;
use std::panic::AssertUnwindSafe;

/// A layout with room to spare, mixed shapes and kinds with settings.
const MIXED: &str = r#"
grid columns=16 rows=9
layout {
    clock          col=0  row=0 width=4 height=2
    calendar       col=0  row=2 width=4 height=4
    session        col=0  row=6 width=4 height=1
    volume         col=4  row=0 size=2 shape="round"
    brightness     col=6  row=0 size=2 shape="round"
    wifi           col=4  row=2 width=3 height=4
    bluetooth      col=7  row=2 width=3 height=3
    system-monitor col=10 row=0 width=4 height=3
    notes          col=10 row=3 width=3 height=3
    clipboard      col=13 row=3 width=3 height=3
    mascot         col=4  row=6 width=3 height=2
    weather        col=7  row=6 width=4 height=2
}
"#;

/// Packed: almost no free cell, so every move has to push others.
const PACKED: &str = r#"
grid columns=8 rows=4
layout {
    clock     col=0 row=0 width=4 height=2
    calendar  col=4 row=0 width=4 height=4
    volume    col=0 row=2 size=2 shape="round"
    timer     col=2 row=2 size=2 shape="round"
}
"#;

// ------------------------------------------------------------------------------------------
// Reporting.

/// The issues a scenario found, each with the step that showed it.
#[derive(Default)]
struct Issues {
    list: Vec<String>,
    /// Problems already reported, so a lasting one isn't repeated after every step.
    known: BTreeSet<String>,
}

impl Issues {
    fn add(&mut self, s: impl Into<String>) {
        let s = s.into();
        if self.known.insert(s.clone()) {
            self.list.push(s);
        }
    }

    /// Check the dashboard after `step`.
    fn check(&mut self, h: &Harness, step: &str) {
        for p in h.problems() {
            if self.known.insert(p.clone()) {
                self.list.push(format!("after {step}: {p}"));
            }
        }
    }
}

/// The dashboard on the mock system, started from `config` (written to config.kdl first).
fn dashboard(config: &str) -> Harness {
    Harness::seed_config(config);
    Harness::with_mock(config)
}

fn tile(h: &Harness, id: u32) -> Option<Tile> {
    h.tiles().into_iter().find(|t| t.id == id)
}

fn id_of(h: &Harness, kind: &str) -> u32 {
    h.tiles()
        .iter()
        .find(|t| t.kind == kind)
        .unwrap_or_else(|| panic!("no {kind} in the layout"))
        .id
}

fn rows(h: &Harness) -> Vec<SettingRow> {
    h.ui().get_setting_rows().iter().collect()
}

fn toast(h: &Harness) -> String {
    let ui = h.ui();
    if ui.get_toast_shown() {
        ui.get_toast().to_string()
    } else {
        String::new()
    }
}

fn visible_ids(h: &Harness) -> BTreeSet<u32> {
    h.tiles()
        .iter()
        .filter(|t| t.visible)
        .map(|t| t.id)
        .collect()
}

/// splitmix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: i64) -> i64 {
        (self.next() % n.max(1) as u64) as i64
    }
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below(hi - lo + 1)
    }
}

// ------------------------------------------------------------------------------------------
// Settings.

/// Every setting of every kind of widget: each stepper walked to both ends, each choice around,
/// each toggle both ways, each text set and cleared, then all reset. A step the sidebar offers
/// must change the value; one it doesn't offer must not.
fn settings_every_widget(issues: &mut Issues) {
    for kind in testing::kind_names() {
        let labels = testing::setting_labels(kind);
        if labels.is_empty() {
            continue;
        }
        let config = format!(
            "grid columns=16 rows=9\nlayout {{\n    {kind} col=0 row=0 width=6 height=6\n}}\n"
        );
        let mut h = dashboard(&config);
        let id = id_of(&h, kind);
        h.ui().invoke_configure_widget(id as i32);
        issues.check(&h, &format!("{kind}: opening its settings"));
        if !h.ui().get_editing() {
            issues.add(format!("{kind}: opening settings didn't enter edit mode"));
        }
        if h.ui().get_settings_title() == "" {
            issues.add(format!("{kind}: the settings tab has no title"));
        }
        let shown: Vec<String> = rows(&h).iter().map(|r| r.label.to_string()).collect();
        if shown != labels {
            issues.add(format!(
                "{kind}: sidebar shows {shown:?}, the widget has {labels:?}"
            ));
        }
        for (i, label) in labels.iter().enumerate() {
            let i = i as i32;
            let step = |h: &mut Harness, d: i32| {
                h.ui().invoke_setting_step(i, d);
                h.pump();
            };
            let row = |h: &Harness| rows(h)[i as usize].clone();
            let at = format!("{kind} / {label}");
            let first = row(&h);
            match first.kind {
                RowKind::Toggle => {
                    step(&mut h, 1);
                    let r = row(&h);
                    if r.on == first.on {
                        issues.add(format!("{at}: the switch doesn't flip"));
                    }
                    if !r.changed {
                        issues.add(format!("{at}: flipped but not marked as changed"));
                    }
                    issues.check(&h, &format!("{at}: flipped"));
                    step(&mut h, 1);
                    let r = row(&h);
                    if r.on != first.on || r.changed {
                        issues.add(format!(
                            "{at}: flipping twice doesn't get back to the default"
                        ));
                    }
                }
                RowKind::Stepper => {
                    // To the bottom, then up to the top noting every value, then down again.
                    for _ in 0..40 {
                        if !row(&h).can_dec {
                            break;
                        }
                        step(&mut h, -1);
                    }
                    let walk =
                        |h: &mut Harness, d: i32, issues: &mut Issues| -> (Vec<String>, bool) {
                            let mut seen = vec![row(h).value.to_string()];
                            let mut wrapped = false;
                            for _ in 0..40 {
                                let before = row(h);
                                if !(if d > 0 {
                                    before.can_inc
                                } else {
                                    before.can_dec
                                }) {
                                    break;
                                }
                                step(h, d);
                                let after = row(h);
                                if after.value == before.value {
                                    issues.add(format!(
                                        "{at}: {} is offered at {:?} but changes nothing",
                                        if d > 0 { "+" } else { "−" },
                                        before.value
                                    ));
                                    break;
                                }
                                if seen.contains(&after.value.to_string()) {
                                    wrapped = true;
                                    break;
                                }
                                seen.push(after.value.to_string());
                                issues.check(h, &format!("{at}: stepped to {:?}", after.value));
                            }
                            (seen, wrapped)
                        };
                    let (ups, wrapped) = walk(&mut h, 1, issues);
                    if ups.len() < 2 {
                        issues.add(format!("{at}: only one value: {ups:?}"));
                    }
                    if !wrapped {
                        let top = row(&h);
                        step(&mut h, 1);
                        if row(&h).value != top.value {
                            issues.add(format!(
                                "{at}: + isn't offered at {:?} but still changes it",
                                top.value
                            ));
                        }
                        let (downs, _) = walk(&mut h, -1, issues);
                        let a: BTreeSet<_> = ups.iter().collect();
                        let b: BTreeSet<_> = downs.iter().collect();
                        if a != b {
                            issues.add(format!("{at}: + reaches {ups:?} but − reaches {downs:?}"));
                        }
                    }
                    h.ui().invoke_setting_reset(i);
                    h.pump();
                    if row(&h).value != first.value || row(&h).changed {
                        issues.add(format!(
                            "{at}: reset gives {:?}, the default is {:?}",
                            row(&h).value,
                            first.value
                        ));
                    }
                }
                RowKind::Text => {
                    // Something harmless that runs if it is a command.
                    let text = match *label {
                        "Other time zones" => "Asia/Tokyo",
                        "Location" => "Hamburg",
                        "Lines from command" => "true",
                        "Cow file" => "tux",
                        _ => "work",
                    };
                    h.ui()
                        .invoke_setting_text(i, SharedString::from(format!("  {text}  ")));
                    h.pump();
                    let r = row(&h);
                    if r.value != text {
                        issues.add(format!("{at}: typed {text:?}, shows {:?}", r.value));
                    }
                    if !r.changed {
                        issues.add(format!("{at}: text set but not marked as changed"));
                    }
                    issues.check(&h, &format!("{at}: set to {text:?}"));
                    h.ui().invoke_setting_text(i, "".into());
                    h.pump();
                    if row(&h).changed {
                        issues.add(format!(
                            "{at}: clearing the text doesn't go back to the default"
                        ));
                    }
                }
            }
            issues.check(&h, &format!("{at}: done"));
        }
        if h.tiles()[0].settings.iter().any(|_| true) {
            issues.add(format!(
                "{kind}: settings left after resetting each: {:?}",
                h.tiles()[0].settings
            ));
        }
    }
}

/// Settings that take any text: what a wrong value does. A time zone that doesn't exist, a
/// place the weather can't find: the dashboard should say so rather than keep it silently.
fn settings_bad_text(issues: &mut Issues) {
    let config = "grid columns=16 rows=9\nlayout {\n    clock col=0 row=0 width=4 height=2\n    weather col=4 row=0 width=4 height=2\n}\n";
    let mut h = dashboard(config);
    let clock = id_of(&h, "clock");
    h.ui().invoke_configure_widget(clock as i32);
    let zones = testing::setting_labels("clock")
        .iter()
        .position(|l| *l == "Other time zones")
        .expect("clock has time zones") as i32;
    // Clock.zones holds one list per clock, by widget id; it refreshes with the clock's tick.
    let zone_rows = |h: &Harness| {
        h.ui()
            .global::<niri_dash::Clock>()
            .get_zones()
            .row_data(clock as usize)
            .map_or(0, |z| z.items.row_count())
    };
    h.ui().invoke_setting_text(zones, "Asia/Tokyo".into());
    h.settle(1200);
    let valid = zone_rows(&h);
    h.ui().invoke_setting_text(zones, "Nowhere/Atlantis".into());
    h.settle(1200);
    let invalid = zone_rows(&h);
    let said = toast(&h);
    let row = rows(&h)[zones as usize].clone();
    // Cleared again before the check: an unknown zone in config.kdl is (rightly) a warning.
    h.ui().invoke_setting_text(zones, "".into());
    h.pump();
    if valid == 1 && invalid == 0 && said.is_empty() {
        issues.add(format!(
            "clock / Other time zones: an unknown zone (\"Nowhere/Atlantis\") is kept and saved silently: the clock drops it, no toast, the sidebar shows {:?} as set",
            row.value
        ));
    } else if valid != 1 {
        issues.add(format!(
            "clock / Other time zones: \"Asia/Tokyo\" gives {valid} zone rows"
        ));
    }
    issues.check(&h, "an unknown time zone");
}

/// The cow draws (it once panicked in the software renderer, measuring text while being drawn):
/// with no cow file, with our own of a name, with a real file, and with one that doesn't exist,
/// which falls back and says so.
fn cow_draws(issues: &mut Issues) {
    let config = "grid columns=16 rows=9\nlayout {\n    mascot col=0 row=0 width=4 height=3 {\n        character \"cow\"\n    }\n}\n";
    let mut h = dashboard(config);
    h.render();
    let mascot = id_of(&h, "mascot");
    h.ui().invoke_configure_widget(mascot as i32);
    h.pump();
    let at = testing::setting_labels("mascot")
        .iter()
        .position(|l| *l == "Cow file")
        .expect("the mascot has a cow file") as i32;
    let dir = std::env::temp_dir().join(format!("niri-dash-edit-cow-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory for the test cow");
    let file = dir.join("test.cow");
    std::fs::write(
        &file,
        "$extra = chop($eyes);\n$the_cow = <<EOC;\n$thoughts ($eyes)\nEOC\n",
    )
    .expect("writing the test cow");
    let path = file.to_string_lossy().to_string();
    let art = |h: &Harness, name: &str| -> String {
        h.ui()
            .global::<niri_dash::Mascots>()
            .invoke_cow_art(name.into(), "@@".into(), "  ".into())
            .to_string()
    };
    for name in [
        "tux",
        "dragon",
        "bunny",
        "moose",
        "sheep",
        "default",
        path.as_str(),
    ] {
        h.ui().invoke_setting_text(at, name.into());
        h.pump();
        h.render();
        if toast(&h).contains("cow") {
            issues.add(format!(
                "cow: {name:?} should load, and said {:?}",
                toast(&h)
            ));
        }
        issues.check(&h, &format!("cow: {name}"));
        if !art(&h, name).contains("@@") {
            issues.add(format!("cow: {name:?} draws no eyes: {}", art(&h, name)));
        }
    }
    h.ui().invoke_setting_text(at, "surely-no-such-cow".into());
    h.pump();
    h.render();
    if !toast(&h).contains("surely-no-such-cow") {
        issues.add(format!("cow: an unknown cow said {:?}", toast(&h)));
    }
    if art(&h, "") != art(&h, "surely-no-such-cow") {
        issues.add("cow: an unknown cow doesn't fall back to our own");
    }
    h.ui().invoke_setting_text(at, "".into());
    h.pump();
    h.render();
    issues.check(&h, "cow: back to our own");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A short Bluetooth tile: Enter on "Add a device" makes the list grow above that row, which
/// pushes it out of sight until the next pass of the event loop, when it scrolls back.
fn bluetooth_row_stays_in_sight(issues: &mut Issues) {
    let config =
        "grid columns=16 rows=9\nlayout {\n    bluetooth col=0 row=0 width=3 height=2\n}\n";
    let mut h = dashboard(config);
    h.settle(50);
    h.press("f");
    type_label(&mut h, "bluetooth");
    h.press("G");
    h.press("enter");
    h.pump();
    if let Err(why) = h.spoken() {
        issues.add(format!(
            "bluetooth: the cursor's row is out of sight a pass later: {why}"
        ));
    }
    // As the daemon does: a pass of the event loop, then a frame, for a while.
    for _ in 0..30 {
        h.render();
        h.settle(10);
        if h.spoken().is_err() {
            // The list changed on the last pass; the row is due back on this one.
            h.pump();
            if let Err(why) = h.spoken() {
                issues.add(format!(
                    "bluetooth: the cursor's row stays out of sight: {why}"
                ));
            }
        }
    }
    match h.spoken() {
        Ok(Some(said)) if said.contains("Stop looking") => {}
        said => issues.add(format!(
            "bluetooth: the cursor should be on Stop looking: {said:?}"
        )),
    }
}

// ------------------------------------------------------------------------------------------
// Random edit sequences through the callbacks the pointer would call.

#[derive(Debug)]
enum Op {
    Move(u32, i32, i32),
    Resize(u32, i32, i32, i32, i32),
    Shape(u32),
    Add(&'static str),
    AddAt(&'static str, i32, i32),
    Remove(u32),
    Grid(i32, i32),
    GridAuto,
    Reset,
    Setting(u32, i32, i32),
    PreviewThenDrop(u32, i32, i32),
    PreviewThenCancel(u32, i32, i32),
}

fn random_op(rng: &mut Rng, h: &Harness) -> Op {
    let tiles = h.tiles();
    let (cols, rows, _) = h.grid();
    let kinds = testing::kind_names();
    let any = |rng: &mut Rng| -> Option<&Tile> {
        (!tiles.is_empty()).then(|| &tiles[rng.below(tiles.len() as i64) as usize])
    };
    let pos = |rng: &mut Rng, n: i32| rng.range(-1, n as i64 + 1) as i32;
    let roll = rng.below(100);
    match (roll, any(rng)) {
        (0..=24, Some(t)) => Op::Move(t.id, pos(rng, cols), pos(rng, rows)),
        (25..=39, Some(t)) => {
            // From any side: pulling the left or top one keeps the right or bottom one in place.
            let (w, hh) = (rng.range(0, 8) as i32, rng.range(0, 7) as i32);
            let col = if rng.below(2) == 0 {
                t.col
            } else {
                t.col + t.w - w
            };
            let row = if rng.below(2) == 0 {
                t.row
            } else {
                t.row + t.h - hh
            };
            Op::Resize(t.id, col, row, w, hh)
        }
        (40..=46, Some(t)) => Op::Shape(t.id),
        (47..=54, _) => Op::Add(kinds[rng.below(kinds.len() as i64) as usize]),
        (55..=61, _) => Op::AddAt(
            kinds[rng.below(kinds.len() as i64) as usize],
            pos(rng, cols),
            pos(rng, rows),
        ),
        (62..=67, Some(t)) => Op::Remove(t.id),
        (68..=72, _) => Op::Grid(rng.below(2) as i32, if rng.below(2) == 0 { -1 } else { 1 }),
        (73, _) => Op::GridAuto,
        (74, _) => Op::Reset,
        (75..=86, Some(t)) => Op::Setting(
            t.id,
            rng.below(6) as i32,
            if rng.below(2) == 0 { -1 } else { 1 },
        ),
        (87..=94, Some(t)) => Op::PreviewThenDrop(t.id, pos(rng, cols), pos(rng, rows)),
        (_, Some(t)) => Op::PreviewThenCancel(t.id, pos(rng, cols), pos(rng, rows)),
        _ => Op::Add("clock"),
    }
}

/// Seeded sequences of edits on a full and a packed layout. Beyond the checks after every step:
/// moving, resizing, reshaping, adding and removing never hide a widget that was shown, and a
/// drop that the preview said fits does land.
fn random_edits(issues: &mut Issues, layout: &str, name: &str) {
    let seeds: Vec<u64> = std::env::var("NIRI_DASH_EDIT_SEEDS")
        .ok()
        .map(|s| s.split(',').filter_map(|n| n.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![1, 2, 3, 4]);
    let ops: usize = std::env::var("NIRI_DASH_EDIT_OPS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(400);
    for seed in seeds {
        let mut h = dashboard(layout);
        h.ui().invoke_set_editing(true);
        h.pump();
        let mut rng = Rng(seed);
        let mut recent: Vec<String> = Vec::new();
        for n in 0..ops {
            let op = random_op(&mut rng, &h);
            recent.push(format!("{op:?}"));
            if recent.len() > 12 {
                recent.remove(0);
            }
            let before = visible_ids(&h);
            let ui = h.ui().clone_strong();
            let mut keeps_shown = true;
            let mut dropped_ok = None;
            match op {
                Op::Move(id, c, r) => ui.invoke_move_widget(id as i32, c, r),
                Op::Resize(id, c, r, w, hh) => ui.invoke_resize_widget(id as i32, c, r, w, hh),
                Op::Shape(id) => ui.invoke_toggle_shape(id as i32),
                Op::Add(k) => ui.invoke_add_widget(testing::ui_kind(k)),
                Op::AddAt(k, c, r) => ui.invoke_add_widget_at(testing::ui_kind(k), c, r),
                Op::Remove(id) => ui.invoke_remove_widget(id as i32),
                Op::Grid(i, d) => {
                    keeps_shown = false;
                    ui.invoke_grid_step(i, d)
                }
                Op::GridAuto => {
                    keeps_shown = false;
                    ui.invoke_grid_auto()
                }
                Op::Reset => {
                    keeps_shown = false;
                    ui.invoke_reset_layout()
                }
                Op::Setting(id, i, d) => {
                    ui.invoke_configure_widget(id as i32);
                    if (i as usize) < ui.get_setting_rows().row_count() {
                        ui.invoke_setting_step(i, d);
                    }
                }
                Op::PreviewThenDrop(id, c, r) => {
                    let fits = ui.invoke_preview_move(id as i32, c, r);
                    ui.invoke_move_widget(id as i32, c, r);
                    dropped_ok = Some((id, c, r, fits));
                }
                Op::PreviewThenCancel(id, c, r) => {
                    let before_tiles = h.tiles();
                    ui.invoke_preview_move(id as i32, c, r);
                    ui.invoke_end_preview();
                    h.pump();
                    if h.tiles() != before_tiles {
                        issues.add(format!(
                            "{name} seed {seed}: a cancelled drag changed the layout"
                        ));
                    }
                }
            }
            h.pump();
            let step = format!("{name} seed {seed} op {n} {:?}", recent.last().unwrap());
            issues.check(&h, &step);
            if keeps_shown {
                let removed: BTreeSet<u32> = match op {
                    Op::Remove(id) => [id].into(),
                    _ => BTreeSet::new(),
                };
                let lost: Vec<u32> = before
                    .difference(&visible_ids(&h))
                    .filter(|id| !removed.contains(id))
                    .copied()
                    .collect();
                if !lost.is_empty() {
                    issues.add(format!(
                        "{step}: hid widgets {lost:?} that were shown (last ops: {recent:?})"
                    ));
                }
            }
            if let Some((id, c, r, fits)) = dropped_ok {
                let (cols, rows, _) = h.grid();
                let t = tile(&h, id).expect("the dragged widget");
                let want_c = c.clamp(0, (cols - t.w).max(0));
                let want_r = r.clamp(0, (rows - t.h).max(0));
                let landed = (t.col, t.row) == (want_c, want_r);
                if fits && !landed && (c, r) == (want_c, want_r) {
                    issues.add(format!(
                        "{step}: the preview said it fits at {c},{r} but it landed at {},{}",
                        t.col, t.row
                    ));
                }
            }
        }
    }
}

// ------------------------------------------------------------------------------------------
// The pointer, as a person would use edit mode.

/// Edit mode by pointer on a defined layout: drag a widget onto another (which makes room), drag
/// one off the grid's edge, resize from a corner, use the toolbar's shape, settings and remove
/// buttons, then a double click outside edit mode to carry one, and the background's right click.
fn pointer_editing(issues: &mut Issues) {
    let mut h = dashboard(MIXED);
    h.settle(400);
    let gap = h.ui().global::<niri_dash::Theme>().get_gap();
    // Right click on the background: edit mode.
    let (bx, by) = h.cell_origin(15.5, 8.5);
    h.down(bx, by, PointerEventButton::Right);
    h.up(bx, by, PointerEventButton::Right);
    h.settle(300);
    if !h.ui().get_editing() {
        issues.add("pointer: right click on the background didn't open edit mode");
        h.ui().invoke_set_editing(true);
        h.settle(300);
    }
    issues.check(&h, "pointer: edit mode");
    // The sidebar is open now and the cells smaller.
    let cell = h.cell();

    // Drag the clock two cells right, onto the volume knob, which has to make room.
    let clock = id_of(&h, "clock");
    let c0 = tile(&h, clock).unwrap();
    let from = h.cell_origin(c0.col as f32 + 1.0, c0.row as f32 + 1.0);
    let to = (from.0 + 4.0 * cell, from.1);
    h.drag(from, to, 12);
    h.settle(300);
    let c1 = tile(&h, clock).unwrap();
    if (c1.col, c1.row) != (c0.col + 4, c0.row) {
        issues.add(format!(
            "pointer: dragged the clock 4 cells right from {},{}; it is at {},{} ({})",
            c0.col,
            c0.row,
            c1.col,
            c1.row,
            toast(&h)
        ));
    }
    issues.check(&h, "pointer: dragging the clock onto the volume knob");

    // Drag the weather far past the bottom right corner: it should end up in the corner.
    let weather = id_of(&h, "weather");
    let w0 = tile(&h, weather).unwrap();
    let from = h.cell_origin(w0.col as f32 + 1.0, w0.row as f32 + 1.0);
    h.drag(from, (from.0 + 30.0 * cell, from.1 + 30.0 * cell), 10);
    h.settle(300);
    let w1 = tile(&h, weather).unwrap();
    let (cols, rows, _) = h.grid();
    if (w1.col + w1.w, w1.row + w1.h) != (cols, rows) {
        issues.add(format!(
            "pointer: dragged the weather past the corner; it is at {},{} ({}x{}) on a {cols}x{rows} grid",
            w1.col, w1.row, w1.w, w1.h
        ));
    }
    issues.check(&h, "pointer: dragging past the grid");

    // Resize the notes from the bottom right corner, one cell wider and taller.
    let notes = id_of(&h, "notes");
    let n0 = tile(&h, notes).unwrap();
    let (gx, gy) = h.cell_origin((n0.col + n0.w) as f32, (n0.row + n0.h) as f32);
    let grip = (gx - gap, gy - gap);
    h.drag(grip, (grip.0 + cell, grip.1 + cell), 8);
    h.settle(300);
    let n1 = tile(&h, notes).unwrap();
    if (n1.w, n1.h) != (n0.w + 1, n0.h + 1) {
        issues.add(format!(
            "pointer: dragged the notes' corner a cell out; {}x{} became {}x{}",
            n0.w, n0.h, n1.w, n1.h
        ));
    }
    issues.check(&h, "pointer: resizing from the corner");

    // The toolbar, top right of a card: settings, shape (if it can be round), remove.
    let toolbar = |h: &Harness, t: &Tile, nth: usize| {
        let buttons = 1
            + usize::from(testing::can_be_round(&t.kind))
            + usize::from(!testing::setting_labels(&t.kind).is_empty());
        let (rx, ry) = h.cell_origin((t.col + t.w) as f32, t.row as f32);
        let width = 4.0 + 34.0 * buttons as f32;
        let left = rx - gap - 12.0 - width;
        (
            left + 2.0 + 15.0 + 32.0 * nth as f32,
            ry + gap + 12.0 + 17.0,
        )
    };
    let sysmon = id_of(&h, "system-monitor");
    let s0 = tile(&h, sysmon).unwrap();
    let (x, y) = toolbar(&h, &s0, 0);
    h.click(x, y);
    h.settle(200);
    if h.ui().get_settings_id() != sysmon as i32 {
        issues.add("pointer: the toolbar's settings button didn't open the widget's settings");
    }
    let (x, y) = toolbar(&h, &s0, 1);
    h.click(x, y);
    h.settle(200);
    let s1 = tile(&h, sysmon).unwrap();
    if !s1.round {
        issues.add(format!(
            "pointer: the toolbar's shape button left the system monitor {} ({})",
            if s1.round { "round" } else { "rectangular" },
            toast(&h)
        ));
    }
    issues.check(&h, "pointer: the shape button");
    let clip = id_of(&h, "clipboard");
    let k0 = tile(&h, clip).unwrap();
    let (x, y) = toolbar(&h, &k0, 0);
    h.click(x, y);
    h.settle(200);
    if tile(&h, clip).is_some() {
        issues.add("pointer: the clipboard's remove button didn't remove it");
    }
    issues.check(&h, "pointer: the remove button");

    // Out of edit mode: a double click on a widget's empty part carries it; a click drops it.
    h.ui().invoke_set_editing(false);
    h.settle(300);
    let cal = id_of(&h, "calendar");
    let a0 = tile(&h, cal).unwrap();
    // The calendar's day grid is display only: an empty part.
    let (px, py) = h.cell_origin(a0.col as f32 + 2.0, a0.row as f32 + 3.0);
    for _ in 0..2 {
        h.down(px, py, PointerEventButton::Left);
        h.up(px, py, PointerEventButton::Left);
    }
    h.settle(100);
    if h.ui().get_adjust_id() != cal as i32 {
        issues.add("pointer: a double click on the calendar didn't pick it up");
    } else {
        // Carry it to the bottom right free area and drop.
        let free = h.cell_origin(13.0 + 2.0, 6.0 + 1.5);
        h.move_to(free.0, free.1);
        h.down(free.0, free.1, PointerEventButton::Left);
        h.up(free.0, free.1, PointerEventButton::Left);
        h.settle(300);
        let a1 = tile(&h, cal).unwrap();
        if (a1.col, a1.row) == (a0.col, a0.row) {
            issues.add(format!(
                "pointer: carried the calendar and dropped it, but it stayed ({})",
                toast(&h)
            ));
        }
    }
    issues.check(&h, "pointer: carrying a widget outside edit mode");
}

/// Every edge and corner of a widget resizes it, by a press anywhere along it: its side (or two)
/// follows the pointer and the sides across stay where they are. It stops at its minimum size and
/// at the grid's edge, however far the pointer goes.
fn pointer_resize_handles(issues: &mut Issues) {
    // Name, the handle's side on each axis.
    const HANDLES: [(&str, i32, i32); 8] = [
        ("top left", -1, -1),
        ("top", 0, -1),
        ("top right", 1, -1),
        ("left", -1, 0),
        ("right", 1, 0),
        ("bottom left", -1, 1),
        ("bottom", 0, 1),
        ("bottom right", 1, 1),
    ];
    const ALONE: &str = "grid columns=16 rows=9\nlayout {\n    notes col=6 row=3 width=4 height=3\n    volume col=1 row=1 size=2 shape=\"round\"\n}\n";
    let mut h = dashboard(ALONE);
    h.ui().invoke_set_editing(true);
    h.settle(300);
    let gap = h.ui().global::<niri_dash::Theme>().get_gap();
    let cell = h.cell();
    let notes = id_of(&h, "notes");
    let home = tile(&h, notes).unwrap();
    // A point on `side` of the card, `along` (0 to 1) the way along it where it is an edge.
    let point = |h: &Harness, t: &Tile, sx: i32, sy: i32, along: f32| {
        let (x0, y0) = h.cell_origin(t.col as f32, t.row as f32);
        let (w, hh) = (t.w as f32 * cell, t.h as f32 * cell);
        let pos = |side: i32, start: f32, len: f32| match side {
            -1 => start + gap,
            1 => start + len - gap,
            _ => start + len * along,
        };
        (pos(sx, x0, w), pos(sy, y0, hh))
    };
    let place_back = |h: &mut Harness| {
        h.ui()
            .invoke_resize_widget(notes as i32, home.col, home.row, home.w, home.h);
        h.settle(280);
    };
    for (name, sx, sy) in HANDLES {
        // Out by a cell, from near the ends of an edge as well as its middle.
        let alongs: &[f32] = if sx != 0 && sy != 0 {
            &[0.5]
        } else {
            &[0.25, 0.5, 0.75]
        };
        for &along in alongs {
            let at = point(&h, &home, sx, sy, along);
            let by = (sx as f32 * cell, sy as f32 * cell);
            h.drag(at, (at.0 + by.0, at.1 + by.1), 6);
            h.settle(280);
            let t = tile(&h, notes).unwrap();
            let want = (
                home.col - i32::from(sx < 0),
                home.row - i32::from(sy < 0),
                home.w + sx.abs(),
                home.h + sy.abs(),
            );
            if (t.col, t.row, t.w, t.h) != want {
                issues.add(format!(
                    "pointer: pulling the {name} handle out a cell ({along} along): {:?} from {:?}, wanted {want:?}",
                    (t.col, t.row, t.w, t.h),
                    (home.col, home.row, home.w, home.h)
                ));
            }
            issues.check(&h, &format!("pointer: pulling the {name} handle out"));
            place_back(&mut h);
        }
        // Across the whole widget and beyond: it stops at its minimum, the far sides in place.
        let (min_w, min_h) = testing::min_size("notes");
        let at = point(&h, &home, sx, sy, 0.5);
        let far = 12.0 * cell;
        h.drag(at, (at.0 - sx as f32 * far, at.1 - sy as f32 * far), 9);
        h.settle(280);
        let t = tile(&h, notes).unwrap();
        let want_w = if sx != 0 { min_w } else { home.w };
        let want_h = if sy != 0 { min_h } else { home.h };
        let want = (
            if sx < 0 {
                home.col + home.w - want_w
            } else {
                home.col
            },
            if sy < 0 {
                home.row + home.h - want_h
            } else {
                home.row
            },
            want_w,
            want_h,
        );
        if (t.col, t.row, t.w, t.h) != want {
            issues.add(format!(
                "pointer: pulling the {name} handle far in: {:?}, wanted {want:?}",
                (t.col, t.row, t.w, t.h)
            ));
        }
        place_back(&mut h);
        // Far out: it stops at the grid's edge.
        let (cols, rows, _) = h.grid();
        let at = point(&h, &home, sx, sy, 0.5);
        h.drag(
            at,
            (
                at.0 + sx as f32 * 40.0 * cell,
                at.1 + sy as f32 * 40.0 * cell,
            ),
            7,
        );
        h.settle(280);
        let t = tile(&h, notes).unwrap();
        let right = home.col + home.w;
        let bottom = home.row + home.h;
        let want = (
            if sx < 0 { 0 } else { home.col },
            if sy < 0 { 0 } else { home.row },
            match sx {
                -1 => right,
                1 => cols - home.col,
                _ => home.w,
            },
            match sy {
                -1 => bottom,
                1 => rows - home.row,
                _ => home.h,
            },
        );
        if (t.col, t.row, t.w, t.h) != want {
            issues.add(format!(
                "pointer: pulling the {name} handle past the grid: {:?}, wanted {want:?} ({})",
                (t.col, t.row, t.w, t.h),
                toast(&h)
            ));
        }
        issues.check(
            &h,
            &format!("pointer: pulling the {name} handle past the grid"),
        );
        place_back(&mut h);
    }
    // A handle pressed and let go changes nothing, and neither does a pull that comes back.
    let at = point(&h, &home, -1, -1, 0.5);
    h.down(at.0, at.1, PointerEventButton::Left);
    h.move_to(at.0 - cell, at.1 - cell);
    h.move_to(at.0, at.1);
    h.up(at.0, at.1, PointerEventButton::Left);
    h.settle(280);
    if tile(&h, notes).unwrap() != home {
        issues.add("pointer: a pull that came back moved the notes");
    }
    // The handle's reach covers the gap: the frame's outermost pixel grabs too.
    let (x0, y0) = h.cell_origin(home.col as f32, home.row as f32);
    let edge = (x0 + 1.0, y0 + home.h as f32 * cell / 2.0);
    h.drag(edge, (edge.0 - cell, edge.1), 5);
    h.settle(280);
    let t = tile(&h, notes).unwrap();
    if (t.col, t.w) != (home.col - 1, home.w + 1) {
        issues.add(format!(
            "pointer: the gap beside the left edge doesn't grab it: col {} width {}",
            t.col, t.w
        ));
    }
    place_back(&mut h);
    // A round widget stays square from any handle, the far side still in place.
    let volume = id_of(&h, "volume");
    let round = tile(&h, volume).unwrap();
    let centre = h.cell_origin(round.col as f32 + 1.0, round.row as f32 + 1.0);
    let radius = cell - gap;
    for (name, sx, sy) in HANDLES {
        let reach = if sx != 0 && sy != 0 { 0.707 } else { 1.0 } * radius;
        let at = (centre.0 + sx as f32 * reach, centre.1 + sy as f32 * reach);
        h.drag(at, (at.0 + sx as f32 * cell, at.1 + sy as f32 * cell), 6);
        h.settle(280);
        let t = tile(&h, volume).unwrap();
        let want = (
            round.col - i32::from(sx < 0),
            round.row - i32::from(sy < 0),
            3,
            3,
        );
        if (t.col, t.row, t.w, t.h) != want {
            issues.add(format!(
                "pointer: pulling the round volume's {name} handle out: {:?}, wanted {want:?}",
                (t.col, t.row, t.w, t.h)
            ));
        }
        issues.check(&h, &format!("pointer: the round volume's {name} handle"));
        h.ui()
            .invoke_resize_widget(volume as i32, round.col, round.row, round.w, round.h);
        h.settle(280);
    }
}

/// A press held still on a widget opens that widget alone for adjusting, on its controls too,
/// which don't react. Moving off or letting go in time is just a press; the widget stays as it is
/// otherwise. The same press goes on to drag it.
fn pointer_long_press(issues: &mut Issues) {
    let mut h = dashboard(MIXED);
    h.settle(300);
    let cell = h.cell();
    let cal = id_of(&h, "calendar");
    let c0 = tile(&h, cal).unwrap();
    let on_cal = h.cell_origin(c0.col as f32 + 2.0, c0.row as f32 + 3.0);

    // Let go too soon: nothing.
    h.down(on_cal.0, on_cal.1, PointerEventButton::Left);
    h.settle(150);
    h.up(on_cal.0, on_cal.1, PointerEventButton::Left);
    h.settle(600);
    if h.ui().get_adjust_id() != 0 {
        issues.add("pointer: a short press opened the widget for adjusting");
    }
    // Moved off while held: nothing.
    h.down(on_cal.0, on_cal.1, PointerEventButton::Left);
    h.move_to(on_cal.0 + 30.0, on_cal.1);
    h.settle(700);
    let moved_off = h.ui().get_adjust_id();
    h.up(on_cal.0 + 30.0, on_cal.1, PointerEventButton::Left);
    h.settle(100);
    if moved_off != 0 {
        issues.add("pointer: a press that moved still counted as a long press");
    }
    h.ui().set_adjust_id(0);
    // A little wobble doesn't matter; held long enough it opens the widget, and letting go leaves
    // it open, with no edit mode around it and no settings opened.
    h.down(on_cal.0, on_cal.1, PointerEventButton::Left);
    h.move_to(on_cal.0 + 2.0, on_cal.1 + 1.0);
    h.settle(700);
    if h.ui().get_adjust_id() != cal as i32 {
        issues.add("pointer: holding on the calendar didn't open it for adjusting");
    }
    h.up(on_cal.0 + 2.0, on_cal.1 + 1.0, PointerEventButton::Left);
    h.settle(100);
    if h.ui().get_adjust_id() != cal as i32 {
        issues.add("pointer: letting go of the long press closed the widget again");
    }
    if h.ui().get_editing() {
        issues.add("pointer: letting go of the long press opened edit mode for all widgets");
    }
    if h.ui().get_settings_id() != 0 {
        issues.add("pointer: letting go of the long press opened the widget's settings");
    }
    issues.check(&h, "pointer: a long press on the calendar");
    // Its handles work now, and only its own.
    let (x1, y1) = h.cell_origin((c0.col + c0.w) as f32, c0.row as f32 + 2.0);
    let gap = h.ui().global::<niri_dash::Theme>().get_gap();
    h.drag((x1 - gap, y1), (x1 - gap - cell, y1), 5);
    h.settle(280);
    let c1 = tile(&h, cal).unwrap();
    if c1.w != c0.w - 1 {
        issues.add(format!(
            "pointer: the right edge after a long press: width {} from {}",
            c1.w, c0.w
        ));
    }
    // Escape puts it down.
    h.press("esc");
    if h.ui().get_adjust_id() != 0 {
        issues.add("pointer: Escape didn't end adjusting after a long press");
    }
    if h.ui().get_editing() {
        issues.add("pointer: Escape after a long press left edit mode on");
    }

    // On a control: the same, and the control doesn't get the click. The session widget is all
    // buttons, and its lock button asks the surface to hide.
    let mut h = dashboard(MIXED);
    h.settle(300);
    let session = id_of(&h, "session");
    let s0 = tile(&h, session).unwrap();
    let on_button = h.cell_origin(s0.col as f32 + 0.5, s0.row as f32 + 0.5);
    h.controls();
    h.down(on_button.0, on_button.1, PointerEventButton::Left);
    h.settle(700);
    h.up(on_button.0, on_button.1, PointerEventButton::Left);
    h.settle(100);
    if h.ui().get_adjust_id() != session as i32 {
        issues.add("pointer: holding on a session button didn't open the widget");
    }
    if !h.controls().is_empty() {
        issues.add("pointer: letting go of a long press on a session button clicked it");
    }
    issues.check(&h, "pointer: a long press on a control");

    // The same press goes on to drag the widget.
    let mut h = dashboard(MIXED);
    h.settle(300);
    let cal = id_of(&h, "calendar");
    let on_cal = h.cell_origin(c0.col as f32 + 2.0, c0.row as f32 + 3.0);
    h.down(on_cal.0, on_cal.1, PointerEventButton::Left);
    h.settle(700);
    let to = (on_cal.0, on_cal.1 + cell);
    for i in 1..=5 {
        h.move_to(on_cal.0, on_cal.1 + cell * i as f32 / 5.0);
    }
    h.up(to.0, to.1, PointerEventButton::Left);
    h.settle(280);
    let c2 = tile(&h, cal).unwrap();
    if c2.row != c0.row + 1 {
        issues.add(format!(
            "pointer: dragging on from a long press: row {} from {} ({})",
            c2.row,
            c0.row,
            toast(&h)
        ));
    }
    issues.check(&h, "pointer: dragging on from a long press");

    // Where it means nothing: edit mode is on, or the background.
    let mut h = dashboard(MIXED);
    h.settle(300);
    let free = h.cell_origin(15.5, 8.5);
    h.down(free.0, free.1, PointerEventButton::Left);
    h.settle(700);
    let on_background = h.ui().get_adjust_id();
    h.move_to(free.0 + 40.0, free.1);
    h.up(free.0 + 40.0, free.1, PointerEventButton::Left);
    h.settle(50);
    if on_background != 0 {
        issues.add("pointer: holding on the background opened a widget");
    }
    h.ui().invoke_set_editing(true);
    h.settle(300);
    let cal = id_of(&h, "calendar");
    let at = h.cell_origin(c0.col as f32 + 2.0, c0.row as f32 + 3.0);
    h.down(at.0, at.1, PointerEventButton::Left);
    h.settle(700);
    h.up(at.0, at.1, PointerEventButton::Left);
    h.settle(100);
    if h.ui().get_adjust_id() != 0 {
        issues.add("pointer: holding in edit mode opened the widget on its own");
    }
    if h.ui().get_settings_id() != cal as i32 {
        issues.add("pointer: in edit mode a held click no longer opens the settings");
    }
}

/// A drag lands where the pointer lets go, however many motion events the drag is made of (a
/// fast flick sends few, a slow drag many), on an empty grid so nothing has to make room.
fn pointer_drag_precision(issues: &mut Issues) {
    let mut h =
        dashboard("grid columns=16 rows=9\nlayout {\n    clock col=2 row=2 width=3 height=2\n}\n");
    h.ui().invoke_set_editing(true);
    h.settle(300);
    let cell = h.cell();
    let clock = id_of(&h, "clock");
    let mut landed = Vec::new();
    for (dc, dr) in [(3, 0), (0, 2), (5, 3), (-2, -1)] {
        for steps in [1, 2, 5, 20, 60] {
            h.ui().invoke_move_widget(clock as i32, 2, 2);
            h.settle(300);
            let from = h.cell_origin(3.5, 3.0);
            let to = (from.0 + dc as f32 * cell, from.1 + dr as f32 * cell);
            h.drag(from, to, steps);
            h.settle(300);
            let t = tile(&h, clock).unwrap();
            if (t.col, t.row) != (2 + dc, 2 + dr) {
                landed.push(format!(
                    "({dc:+},{dr:+}) in {steps} moves → ({:+},{:+})",
                    t.col - 2,
                    t.row - 2
                ));
            }
            issues.check(&h, &format!("drag ({dc},{dr}) in {steps} moves"));
        }
    }
    // Onto occupied cells: the clock over the volume and brightness knobs, which make room.
    drop(h);
    for (dc, dr) in [(4, 0), (2, 0), (1, 0), (3, 1)] {
        for steps in [1, 3, 12, 40] {
            let mut fresh = dashboard(MIXED);
            let clock = id_of(&fresh, "clock");
            fresh.ui().invoke_set_editing(true);
            fresh.settle(300);
            let from = fresh.cell_origin(1.0, 1.0);
            let to = (from.0 + dc as f32 * cell, from.1 + dr as f32 * cell);
            fresh.drag(from, to, steps);
            fresh.settle(300);
            let t = tile(&fresh, clock).unwrap();
            if (t.col, t.row) != (dc, dr) {
                landed.push(format!(
                    "onto others ({dc:+},{dr:+}) in {steps} moves → ({:+},{:+}) [{}]",
                    t.col,
                    t.row,
                    toast(&fresh)
                ));
            }
            issues.check(
                &fresh,
                &format!("drag onto others ({dc},{dr}) in {steps} moves"),
            );
        }
    }
    if !landed.is_empty() {
        issues.add(format!(
            "pointer: drags don't land where they are let go (cells moved, wanted → got): {}",
            landed.join("; ")
        ));
    }
}

// ------------------------------------------------------------------------------------------
// The keyboard in edit mode.

/// Type the hint label of the first widget of `kind`.
fn type_label(h: &mut Harness, kind: &str) {
    let label = h
        .ui()
        .get_widgets()
        .iter()
        .find(|w| w.kind == testing::ui_kind(kind))
        .map(|w| w.hint.to_string())
        .unwrap_or_else(|| panic!("no label for {kind}"));
    for c in label.chars() {
        h.press(&c.to_string());
    }
}

/// Edit mode with the keyboard alone, start to end: everything the pointer can do there.
fn keyboard_editing(issues: &mut Issues) {
    fn mode(h: &Harness) -> NavMode {
        h.ui().global::<Nav>().get_mode()
    }
    let mut h = dashboard(MIXED);

    // In from inside a widget.
    h.press("f");
    type_label(&mut h, "wifi");
    if mode(&h) != NavMode::Widget {
        issues.add("keyboard: typing the Wi-Fi label didn't enter it");
    }
    h.press("ctrl+e");
    if !h.ui().get_editing() {
        issues.add("keyboard: Ctrl+E inside a widget didn't open edit mode");
    }
    if mode(&h) != NavMode::Off {
        issues.add("keyboard: edit mode opened with a widget still entered");
    }
    issues.check(&h, "keyboard: Ctrl+E from a widget");

    // The sidebar's tabs.
    let tab = |h: &Harness| h.ui().get_drawer_tab();
    use niri_dash::DrawerTab as T;
    h.press("tab");
    if tab(&h) != T::Colors {
        issues.add(format!("keyboard: Tab from Widgets went to {:?}", tab(&h)));
    }
    h.press("tab");
    if tab(&h) != T::Style {
        issues.add(format!("keyboard: Tab from Colors went to {:?}", tab(&h)));
    }
    h.press("tab");
    if tab(&h) != T::Widgets {
        issues.add(format!(
            "keyboard: Tab from Style, no widget open, went to {:?}",
            tab(&h)
        ));
    }
    h.press("backtab");
    if tab(&h) != T::Style {
        issues.add(format!(
            "keyboard: Shift+Tab from Widgets went to {:?}",
            tab(&h)
        ));
    }
    h.press("tab");

    // The grid's steppers: l and h step, a refused step shakes, Backspace goes back to automatic.
    let (c0, r0, _) = h.grid();
    h.press("g"); // Tab showed the cursor; Columns is the first stop
    h.press("l");
    if h.grid().0 != c0 + 1 {
        issues.add(format!(
            "keyboard: l on Columns gave {:?} from {c0}",
            h.grid()
        ));
    }
    h.press("h");
    h.press("j");
    h.press("l");
    if h.grid().1 != r0 + 1 {
        issues.add(format!("keyboard: l on Rows gave {:?} from {r0}", h.grid()));
    }
    h.press("h");
    issues.check(&h, "keyboard: the grid steppers");
    let shakes = h.ui().get_grid_refusals_rows();
    for _ in 0..12 {
        h.press("h");
    }
    if h.ui().get_grid_refusals_rows() == shakes {
        issues.add(format!(
            "keyboard: stepping Rows down 12 times never refused, at {:?}",
            h.grid()
        ));
    }
    let shown = h.tiles().iter().filter(|t| t.visible).count();
    if shown != h.tiles().len() {
        issues.add("keyboard: stepping the grid down hid widgets");
    }
    issues.check(&h, "keyboard: Rows down until refused");
    h.press("backspace");
    if h.grid().2 {
        issues.add("keyboard: Backspace on a grid stepper left the grid explicit");
    }
    issues.check(&h, "keyboard: Backspace on a grid stepper");

    // The palette: down to an entry, Enter adds it.
    let n = h.tiles().len();
    h.press("j");
    h.press("j");
    h.press("enter");
    if h.tiles().len() != n + 1 {
        issues.add(format!(
            "keyboard: Enter on a palette entry: {n} widgets became {} ({})",
            h.tiles().len(),
            toast(&h)
        ));
    }
    issues.check(&h, "keyboard: adding from the palette");

    // Colors: another scheme.
    h.press("tab");
    let schemes: Vec<_> = h.ui().get_colorschemes().iter().collect();
    if schemes.len() > 1 {
        let current = schemes.iter().position(|s| s.current).unwrap_or(0);
        let want = if current == 0 { 1 } else { 0 };
        h.press("g");
        for _ in 0..want {
            h.press("j");
        }
        h.press("enter");
        h.pump();
        let now: Vec<_> = h.ui().get_colorschemes().iter().collect();
        if !now[want].current {
            issues.add(format!(
                "keyboard: Enter on color scheme {:?} didn't choose it",
                now[want].name
            ));
        }
    } else {
        issues.add(format!(
            "keyboard: only {} color scheme(s) to choose from",
            schemes.len()
        ));
    }
    issues.check(&h, "keyboard: choosing a color scheme");

    // Style: adjust a variable, then reset it.
    h.press("tab");
    let adjust = h.ui().get_styles().row_count();
    h.press("g");
    for _ in 0..adjust {
        h.press("j");
    }
    let before = h.ui().get_style_rows().row_data(0).expect("a style row");
    h.press(if before.can_inc { "l" } else { "h" });
    let after = h.ui().get_style_rows().row_data(0).expect("a style row");
    if after.value == before.value {
        issues.add(format!(
            "keyboard: stepping style {:?} changed nothing",
            before.label
        ));
    }
    if !after.overridden {
        issues.add(format!(
            "keyboard: style {:?} stepped but isn't marked",
            before.label
        ));
    }
    // "Reset all" appeared above it, so the cursor is now on the row above; find the row again.
    h.press("j");
    h.press("backspace");
    if h.ui()
        .get_style_rows()
        .row_data(0)
        .expect("a style row")
        .overridden
    {
        issues.add(format!(
            "keyboard: Backspace didn't reset style {:?}",
            before.label
        ));
    }
    issues.check(&h, "keyboard: adjusting the style");
    h.press("tab");

    // A widget: f and its label open its settings, without entering it.
    h.press("f");
    if mode(&h) != NavMode::Hints {
        issues.add("keyboard: f in edit mode doesn't show the hints");
    }
    type_label(&mut h, "clock");
    let clock = id_of(&h, "clock");
    if mode(&h) != NavMode::Off {
        issues.add("keyboard: picking a widget in edit mode entered it");
    }
    if h.ui().get_settings_id() != clock as i32 || tab(&h) != T::Widget {
        issues.add("keyboard: picking the clock in edit mode didn't open its settings");
    }

    // Its settings: a stepper, a switch, the text field. (Time format, Seconds, Date, Zones.)
    let labels = testing::setting_labels("clock");
    let at = |l: &str| labels.iter().position(|x| *x == l).unwrap();
    let row = |h: &Harness, l: &str| rows(h)[labels.iter().position(|x| *x == l).unwrap()].clone();
    h.press("g");
    let was = row(&h, "Time format");
    h.press(if was.can_inc { "l" } else { "h" });
    if row(&h, "Time format").value == was.value {
        issues.add("keyboard: h/l on the clock's time format changed nothing");
    }
    h.press("backspace");
    if row(&h, "Time format").changed {
        issues.add("keyboard: Backspace didn't reset the clock's time format");
    }
    for _ in 0..at("Seconds") {
        h.press("j");
    }
    let was = row(&h, "Seconds");
    h.press("space");
    if row(&h, "Seconds").on == was.on {
        issues.add("keyboard: Space on the Seconds switch didn't flip it");
    }
    h.press("enter");
    if row(&h, "Seconds").on != was.on {
        issues.add("keyboard: Enter on the Seconds switch didn't flip it back");
    }
    issues.check(&h, "keyboard: the clock's stepper and switch");
    h.press("G");
    h.press("enter");
    for c in "Tokyo".chars() {
        h.press(&c.to_string());
    }
    if row(&h, "Other time zones").changed {
        issues.add("keyboard: the text field saved before Enter");
    }
    h.press("esc");
    if !h.ui().get_editing() {
        issues.add("keyboard: Escape in a sidebar text field left edit mode");
    }
    // What was typed is dropped: typing again doesn't add to it.
    h.press("enter");
    for c in "Tokyo".chars() {
        h.press(&c.to_string());
    }
    h.press("enter");
    if !h.tiles().iter().any(|t| {
        t.settings
            .iter()
            .any(|(k, v)| k == "zones" && v.contains("Tokyo"))
    }) {
        issues.add(format!(
            "keyboard: typing a time zone into the clock's field and Enter didn't save it: {:?}",
            row(&h, "Other time zones").value
        ));
    }
    // After Enter the keys work again.
    h.press("k");
    h.press("space");
    issues.check(&h, "keyboard: the clock's text field");
    if !h
        .tiles()
        .iter()
        .any(|t| t.settings.iter().any(|(k, _)| k == "date"))
    {
        issues.add("keyboard: after a text field's Enter, the sidebar's keys don't work");
    }

    // Moving it: pushes others aside; sizing it; the next widget.
    let c0 = tile(&h, clock).unwrap();
    h.press("ctrl+shift+L");
    let c1 = tile(&h, clock).unwrap();
    if (c1.col, c1.row) != (c0.col + 1, c0.row) {
        issues.add(format!(
            "keyboard: Ctrl+Shift+L moved the clock from {},{} to {},{} ({})",
            c0.col,
            c0.row,
            c1.col,
            c1.row,
            toast(&h)
        ));
    }
    issues.check(&h, "keyboard: moving the clock");
    h.press("ctrl+r");
    h.press("L");
    let c2 = tile(&h, clock).unwrap();
    if c2.w != c1.w + 1 {
        issues.add(format!(
            "keyboard: sizing mode's L made the clock {}x{} from {}x{}",
            c2.w, c2.h, c1.w, c1.h
        ));
    }
    h.press("h");
    if tile(&h, clock).unwrap().w != c1.w {
        issues.add("keyboard: sizing mode's h didn't pull the side back in");
    }
    h.press("enter");
    issues.check(&h, "keyboard: sizing the clock");
    // Sizing is over: L is no longer a push.
    let w = tile(&h, clock).unwrap().w;
    h.press("L");
    if tile(&h, clock).unwrap().w != w {
        issues.add("keyboard: Enter didn't end sizing mode");
    }
    h.press("ctrl+j");
    let next = h.ui().get_settings_id();
    if next == clock as i32 || next == 0 {
        issues.add("keyboard: Ctrl+J in edit mode didn't go to the widget below");
    }
    issues.check(&h, "keyboard: the widget below");

    // Shape and removal, on the volume knob.
    h.press("f");
    type_label(&mut h, "volume");
    let volume = id_of(&h, "volume");
    let round = tile(&h, volume).unwrap().round;
    h.press("o");
    if tile(&h, volume).unwrap().round == round {
        issues.add(format!(
            "keyboard: o didn't change the volume's shape ({})",
            toast(&h)
        ));
    }
    issues.check(&h, "keyboard: reshaping the volume");
    h.press("x");
    if tile(&h, volume).is_some() {
        issues.add("keyboard: x didn't remove the widget");
    }
    if h.ui().get_settings_id() != 0 {
        issues.add("keyboard: the removed widget's settings stayed open");
    }
    issues.check(&h, "keyboard: removing the volume");

    // Reset: the last stop of the Widgets tab.
    h.press("G");
    h.press("enter");
    issues.check(&h, "keyboard: resetting the layout");

    // Out: Ctrl+E closes it; Escape does too; the hints work again.
    h.press("ctrl+e");
    if h.ui().get_editing() {
        issues.add("keyboard: Ctrl+E didn't close edit mode");
    }
    h.press("f");
    if mode(&h) != NavMode::Hints {
        issues.add("keyboard: after edit mode, f doesn't show the hints");
    }
    h.press("ctrl+e");
    if !h.ui().get_editing() {
        issues.add("keyboard: Ctrl+E did nothing while the hints were up");
    }
    h.press("f");
    h.press("esc");
    if !h.ui().get_editing() || mode(&h) != NavMode::Off {
        issues.add("keyboard: Escape with the hints up in edit mode should only close the hints");
    }
    h.press("esc");
    if h.ui().get_editing() {
        issues.add("keyboard: Escape didn't close edit mode");
    }
    issues.check(&h, "keyboard: leaving edit mode");
}

// ------------------------------------------------------------------------------------------
// The grid.

/// Shrinking the grid step by step: a step that would leave no room for a widget is refused (the
/// stepper shakes, a toast names the widgets, nothing changes), so no widget is ever lost;
/// growing back then restores the layout as it was.
fn grid_shrink_and_grow(issues: &mut Issues) {
    let mut h = dashboard(MIXED);
    h.ui().invoke_set_editing(true);
    h.pump();
    let start = h.tiles();
    let (c0, r0, _) = h.grid();
    let controls = |h: &Harness| -> Vec<SettingRow> { h.ui().get_grid_controls().iter().collect() };
    let mut refused = [false, false];
    for _ in 0..80 {
        for index in 0..2 {
            if refused[index] || !controls(&h)[index].can_dec {
                continue;
            }
            let before = (h.grid(), h.tiles());
            let shakes = |h: &Harness| {
                if index == 0 {
                    h.ui().get_grid_refusals_cols()
                } else {
                    h.ui().get_grid_refusals_rows()
                }
            };
            let s0 = shakes(&h);
            h.ui().invoke_grid_step(index as i32, -1);
            h.pump();
            issues.check(
                &h,
                &format!(
                    "grid: − on {} at {:?}",
                    ["columns", "rows"][index],
                    before.0
                ),
            );
            if h.grid() == before.0 {
                refused[index] = true;
                if shakes(&h) != s0 + 1 {
                    issues.add("grid: a refused step didn't shake the stepper");
                }
                if !toast(&h).starts_with("No room for") {
                    issues.add(format!("grid: a refused step said {:?}", toast(&h)));
                }
                if h.tiles() != before.1 {
                    issues.add("grid: a refused step changed the layout anyway");
                }
            }
        }
        if refused == [true, true] {
            break;
        }
    }
    if refused != [true, true] {
        issues.add(format!(
            "grid: shrunk to {:?} without either stepper refusing",
            h.grid()
        ));
    }
    let now = h.tiles();
    if now.len() != start.len() {
        issues.add(format!(
            "grid: shrinking lost {} widget(s)",
            start.len() - now.len()
        ));
    }
    if now.iter().any(|t| !t.visible) {
        issues.add("grid: shrinking hid widgets");
    }
    // Back to where it was: everything is shown again, in some arrangement.
    for _ in 0..60 {
        let (c, r, _) = h.grid();
        if (c, r) == (c0, r0) {
            break;
        }
        if c < c0 {
            h.ui().invoke_grid_step(0, 1);
        }
        if r < r0 {
            h.ui().invoke_grid_step(1, 1);
        }
        h.pump();
    }
    issues.check(&h, "grid: grown back");
    if h.tiles().len() != start.len() || h.tiles().iter().any(|t| !t.visible) {
        issues.add("grid: back at the old size, not every widget is shown");
    }
    h.ui().invoke_grid_auto();
    h.pump();
    issues.check(&h, "grid: back to the automatic grid");
    if h.grid().2 {
        issues.add("grid: the reset button left the grid explicit");
    }
}

// ------------------------------------------------------------------------------------------
// The sidebar follows the widgets.

/// The settings tab belongs to one widget: it closes when that widget goes, follows it when it
/// moves or changes shape, and a removed widget's hint label is gone.
fn sidebar_follows(issues: &mut Issues) {
    let mut h = dashboard(PACKED);
    h.ui().invoke_set_editing(true);
    let clock = id_of(&h, "clock");
    h.ui().invoke_configure_widget(clock as i32);
    h.pump();
    h.ui().invoke_move_widget(clock as i32, 0, 2);
    h.pump();
    issues.check(&h, "sidebar: moved the configured clock");
    if h.ui().get_settings_id() != clock as i32 {
        issues.add("sidebar: moving the widget closed its settings");
    }
    h.ui().invoke_toggle_shape(clock as i32);
    h.pump();
    issues.check(&h, "sidebar: reshaped the configured clock");
    h.ui().invoke_remove_widget(clock as i32);
    h.pump();
    issues.check(&h, "sidebar: removed the configured clock");
    if h.ui().get_settings_id() != 0 {
        issues.add("sidebar: settings stay open for a removed widget");
    }
    if h.ui().get_drawer_tab() == niri_dash::DrawerTab::Widget {
        issues.add("sidebar: still on the removed widget's tab");
    }
    // Packed: adding has nowhere to go and says so.
    let n = h.tiles().len();
    for _ in 0..3 {
        h.ui().invoke_add_widget(testing::ui_kind("calendar"));
        h.pump();
    }
    issues.check(&h, "sidebar: adding to a full grid");
    let added = h.tiles().len() - n;
    if added > 0 && h.tiles().iter().filter(|t| !t.visible).count() > 0 {
        issues.add("sidebar: adding to a full grid added a widget that isn't shown");
    }
    // Resetting brings the default layout, and the settings close.
    h.ui().invoke_configure_widget(id_of(&h, "calendar") as i32);
    h.ui().invoke_reset_layout();
    h.pump();
    issues.check(&h, "sidebar: reset the layout");
}

// ------------------------------------------------------------------------------------------

type Scenario = (&'static str, fn(&mut Issues));

fn main() {
    let _sandbox = Sandbox::enter(None).expect("a temporary directory for the dashboard's files");
    // SAFETY: nothing else runs yet that could read the environment.
    unsafe { std::env::set_var("LC_ALL", "C") };
    let scenarios: &[Scenario] = &[
        (
            "settings: every setting of every widget",
            settings_every_widget,
        ),
        ("settings: text that makes no sense", settings_bad_text),
        ("cow: draws, with any cow file", cow_draws),
        (
            "bluetooth: the cursor's row stays in sight",
            bluetooth_row_stays_in_sight,
        ),
        ("edits: random on a roomy layout", |i| {
            random_edits(i, MIXED, "mixed")
        }),
        ("edits: random on a packed layout", |i| {
            random_edits(i, PACKED, "packed")
        }),
        ("pointer: edit mode as a person uses it", pointer_editing),
        (
            "pointer: drags land where they are let go",
            pointer_drag_precision,
        ),
        (
            "pointer: every edge and corner resizes",
            pointer_resize_handles,
        ),
        (
            "pointer: a long press adjusts the widget",
            pointer_long_press,
        ),
        ("keyboard: edit mode", keyboard_editing),
        (
            "grid: shrink to the smallest and back",
            grid_shrink_and_grow,
        ),
        ("sidebar: follows its widget", sidebar_follows),
    ];
    let only = std::env::var("NIRI_DASH_EDIT_ONLY").unwrap_or_default();
    let scenarios: Vec<_> = scenarios
        .iter()
        .filter(|(n, _)| n.contains(only.as_str()))
        .collect();
    let mut failed = 0;
    for (name, scenario) in &scenarios {
        let mut issues = Issues::default();
        let run = std::panic::catch_unwind(AssertUnwindSafe(|| scenario(&mut issues)));
        if let Err(e) = run {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            issues.list.push(format!("panicked: {msg}"));
        }
        if issues.list.is_empty() {
            println!("test {name} ... ok");
        } else {
            failed += 1;
            println!("test {name} ... {} issue(s)", issues.list.len());
            for i in &issues.list {
                println!("  - {i}");
            }
        }
    }
    println!(
        "\ntest result: {}. {} passed; {failed} with issues",
        if failed == 0 { "ok" } else { "FAILED" },
        scenarios.len() - failed
    );
    if failed > 0 {
        std::process::exit(1);
    }
}

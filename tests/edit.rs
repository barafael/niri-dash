//! Edit mode, on the mock system: long scripted and seeded sequences of what edit mode offers
//! (moving, resizing, reshaping, adding, removing, the grid, every widget setting), by pointer,
//! by keyboard and through the dashboard's own operations. After each step the dashboard is
//! checked against itself (`Harness::problems`): the layout keeps its rules, the views show it,
//! config.kdl says the same and reads back cleanly, the hint labels work. Steps that do nothing
//! although the sidebar offers them, and things only a pointer can do, are reported as issues too.
//!
//! A plain program (`harness = false`). Each scenario gets a fresh dashboard and reports every
//! issue it finds, not just the first.
//!
//! `NIRI_DASH_EDIT_SEEDS=1,2,3` and `NIRI_DASH_EDIT_OPS=400` set the random sequences,
//! `NIRI_DASH_EDIT_ONLY=pointer` runs the scenarios whose names contain that.

use gpui::MouseButton;
use niri_dash::testing::{
    self, DrawerTab as T, Harness, NavMode, NavOp, RowKind, Sandbox, SettingRow, Tile,
};
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
    h.tile(id)
}

fn id_of(h: &Harness, kind: &str) -> u32 {
    h.id_of(kind)
}

fn visible_ids(h: &Harness) -> BTreeSet<u32> {
    h.tiles().iter().filter(|t| t.visible).map(|t| t.id).collect()
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
        h.configure(id);
        issues.check(&h, &format!("{kind}: opening its settings"));
        if !h.editing() {
            issues.add(format!("{kind}: opening settings didn't enter edit mode"));
        }
        if h.read(|d| d.settings_title()).is_empty() {
            issues.add(format!("{kind}: the settings card has no title"));
        }
        let shown: Vec<String> = h.setting_rows().iter().map(|r| r.label.clone()).collect();
        if shown != labels {
            issues.add(format!(
                "{kind}: sidebar shows {shown:?}, the widget has {labels:?}"
            ));
        }
        for (i, label) in labels.iter().enumerate() {
            let step = |h: &mut Harness, d: i32| h.setting_step(i, d);
            let row = |h: &Harness| h.setting_rows()[i].clone();
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
                            let mut seen = vec![row(h).value.clone()];
                            let mut wrapped = false;
                            for _ in 0..40 {
                                let before = row(h);
                                if !(if d > 0 { before.can_inc } else { before.can_dec }) {
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
                                if seen.contains(&after.value) {
                                    wrapped = true;
                                    break;
                                }
                                seen.push(after.value.clone());
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
                    h.setting_reset(i);
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
                    h.setting_text(i, &format!("  {text}  "));
                    let r = row(&h);
                    if r.value != text {
                        issues.add(format!("{at}: typed {text:?}, shows {:?}", r.value));
                    }
                    if !r.changed {
                        issues.add(format!("{at}: text set but not marked as changed"));
                    }
                    issues.check(&h, &format!("{at}: set to {text:?}"));
                    h.setting_text(i, "");
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

/// Settings that take any text: what a wrong value does. A time zone that doesn't exist, a place
/// the weather can't find: the dashboard should say so rather than keep it silently.
fn settings_bad_text(issues: &mut Issues) {
    let config = "grid columns=16 rows=9\nlayout {\n    clock col=0 row=0 width=4 height=2\n    weather col=4 row=0 width=4 height=2\n}\n";
    let mut h = dashboard(config);
    let clock = id_of(&h, "clock");
    h.configure(clock);
    let zones = testing::setting_labels("clock")
        .iter()
        .position(|l| *l == "Other time zones")
        .expect("clock has time zones");
    h.setting_text(zones, "Asia/Tokyo");
    let valid = h.toast_text();
    h.setting_text(zones, "Nowhere/Atlantis");
    let said = h.toast_text();
    let row = h.setting_rows()[zones].clone();
    // Cleared again before the check: an unknown zone in config.kdl is (rightly) a warning.
    h.setting_text(zones, "");
    if said.is_empty() || said == valid {
        issues.add(format!(
            "clock / Other time zones: an unknown zone (\"Nowhere/Atlantis\") is kept and saved silently: no toast, the sidebar shows {:?} as set",
            row.value
        ));
    }
    if valid.contains("Tokyo") {
        issues.add(format!(
            "clock / Other time zones: \"Asia/Tokyo\" gave the complaint {valid:?}"
        ));
    }
    issues.check(&h, "an unknown time zone");
}

/// The cow file: a name we know loads without a word, one that doesn't exist falls back and says
/// so.
fn cow_toasts(issues: &mut Issues) {
    let config = "grid columns=16 rows=9\nlayout {\n    mascot col=0 row=0 width=4 height=3 {\n        character \"cow\"\n    }\n}\n";
    let mut h = dashboard(config);
    let mascot = id_of(&h, "mascot");
    h.configure(mascot);
    let at = testing::setting_labels("mascot")
        .iter()
        .position(|l| *l == "Cow file")
        .expect("the mascot has a cow file");
    for name in ["tux", "dragon", "bunny", "moose", "sheep", "default"] {
        h.setting_text(at, name);
        if h.toast_text().contains("cow") {
            issues.add(format!(
                "cow: {name:?} should load, and said {:?}",
                h.toast_text()
            ));
        }
        issues.check(&h, &format!("cow: {name}"));
    }
    h.setting_text(at, "surely-no-such-cow");
    if !h.toast_text().contains("surely-no-such-cow") {
        issues.add(format!("cow: an unknown cow said {:?}", h.toast_text()));
    }
    h.setting_text(at, "");
    issues.check(&h, "cow: back to our own");
}

// ------------------------------------------------------------------------------------------
// Random edit sequences through the operations the pointer would call.

#[derive(Debug)]
enum Op {
    Move(u32, i32, i32),
    Resize(u32, i32, i32, i32, i32),
    Shape(u32),
    Add(&'static str),
    AddAt(&'static str, i32, i32),
    Remove(u32),
    Grid(usize, i32),
    GridAuto,
    Reset,
    Setting(u32, usize, i32),
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
            let col = if rng.below(2) == 0 { t.col } else { t.col + t.w - w };
            let row = if rng.below(2) == 0 { t.row } else { t.row + t.h - hh };
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
        (68..=72, _) => Op::Grid(rng.below(2) as usize, if rng.below(2) == 0 { -1 } else { 1 }),
        (73, _) => Op::GridAuto,
        (74, _) => Op::Reset,
        (75..=86, Some(t)) => Op::Setting(
            t.id,
            rng.below(6) as usize,
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
        h.set_editing(true);
        let mut rng = Rng(seed);
        let mut recent: Vec<String> = Vec::new();
        for n in 0..ops {
            let op = random_op(&mut rng, &h);
            recent.push(format!("{op:?}"));
            if recent.len() > 12 {
                recent.remove(0);
            }
            let before = visible_ids(&h);
            let mut keeps_shown = true;
            let mut dropped_ok = None;
            match op {
                Op::Move(id, c, r) => h.move_widget(id, c, r),
                Op::Resize(id, c, r, w, hh) => h.resize_widget(id, c, r, w, hh),
                Op::Shape(id) => h.toggle_shape(id),
                Op::Add(k) => h.add_widget(k),
                Op::AddAt(k, c, r) => h.add_widget_at(k, c, r),
                Op::Remove(id) => h.remove_widget(id),
                Op::Grid(i, d) => {
                    keeps_shown = false;
                    h.grid_step(i, d)
                }
                Op::GridAuto => {
                    keeps_shown = false;
                    h.grid_auto()
                }
                Op::Reset => {
                    keeps_shown = false;
                    h.reset_layout()
                }
                Op::Setting(id, i, d) => {
                    h.configure(id);
                    if i < h.setting_rows().len() {
                        h.setting_step(i, d);
                    }
                }
                Op::PreviewThenDrop(id, c, r) => {
                    let fits = h.preview_move(id, c, r);
                    h.move_widget(id, c, r);
                    dropped_ok = Some((id, c, r, fits));
                }
                Op::PreviewThenCancel(id, c, r) => {
                    let before_tiles = h.tiles();
                    h.preview_move(id, c, r);
                    h.end_preview();
                    if h.tiles() != before_tiles {
                        issues.add(format!(
                            "{name} seed {seed}: a cancelled drag changed the layout"
                        ));
                    }
                }
            }
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
    let gap = h.gap();
    // Right click on the background: edit mode.
    let (bx, by) = h.cell_origin(15.5, 8.5);
    h.click_with(bx, by, MouseButton::Right);
    if !h.editing() {
        issues.add("pointer: right click on the background didn't open edit mode");
        h.set_editing(true);
    }
    issues.check(&h, "pointer: edit mode");
    // The sidebar is open now and the cells smaller.
    let cell = h.cell();

    // Drag the clock four cells right, onto the volume knob, which has to make room.
    let clock = id_of(&h, "clock");
    let c0 = tile(&h, clock).unwrap();
    let from = h.cell_origin(c0.col as f32 + 1.0, c0.row as f32 + 1.0);
    let to = (from.0 + 4.0 * cell, from.1);
    h.drag(from, to, 12);
    let c1 = tile(&h, clock).unwrap();
    if (c1.col, c1.row) != (c0.col + 4, c0.row) {
        issues.add(format!(
            "pointer: dragged the clock 4 cells right from {},{}; it is at {},{} ({})",
            c0.col,
            c0.row,
            c1.col,
            c1.row,
            h.toast_text()
        ));
    }
    issues.check(&h, "pointer: dragging the clock onto the volume knob");

    // Drag the weather far past the bottom right corner: it should end up in the corner.
    let weather = id_of(&h, "weather");
    let w0 = tile(&h, weather).unwrap();
    let from = h.cell_origin(w0.col as f32 + 1.0, w0.row as f32 + 1.0);
    h.drag(from, (from.0 + 30.0 * cell, from.1 + 30.0 * cell), 10);
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
    let n1 = tile(&h, notes).unwrap();
    if (n1.w, n1.h) != (n0.w + 1, n0.h + 1) {
        issues.add(format!(
            "pointer: dragged the notes' corner a cell out; {}x{} became {}x{}",
            n0.w, n0.h, n1.w, n1.h
        ));
    }
    issues.check(&h, "pointer: resizing from the corner");

    // The toolbar, top right of a card: settings (if it has any), shape (if it can be round),
    // pin, remove.
    let sysmon = id_of(&h, "system-monitor");
    let name = testing::title("system-monitor");
    h.click_named(&format!("Settings for {name}"));
    if h.settings_id() != sysmon {
        issues.add("pointer: the toolbar's settings button didn't open the widget's settings");
    }
    // The card is in the way of the other buttons until it is closed: Escape closes it.
    h.key("esc");
    if h.settings_id() != 0 || !h.editing() {
        issues.add("pointer: Escape didn't close the settings card and stay in edit mode");
    }
    h.click_named(&format!("Make {name} round"));
    let s1 = tile(&h, sysmon).unwrap();
    if !s1.round {
        issues.add(format!(
            "pointer: the toolbar's shape button left the system monitor {} ({})",
            if s1.round { "round" } else { "rectangular" },
            h.toast_text()
        ));
    }
    issues.check(&h, "pointer: the shape button");
    // The clipboard has neither settings nor a round shape: the lock, then remove.
    let clip = id_of(&h, "clipboard");
    let cname = testing::title("clipboard");
    h.click_named(&format!("Pin {cname}"));
    if h.pinned(clip) != Some(true) {
        issues.add("pointer: the toolbar's lock didn't pin the clipboard");
    }
    issues.check(&h, "pointer: the lock button");
    h.click_named(&format!("Remove {cname}"));
    if tile(&h, clip).is_some() {
        issues.add("pointer: the clipboard's remove button didn't remove it");
    }
    issues.check(&h, "pointer: the remove button");

    // Out of edit mode: a double click on a widget's empty part carries it; a click drops it.
    h.set_editing(false);
    let cal = id_of(&h, "calendar");
    let a0 = tile(&h, cal).unwrap();
    // The calendar's day grid is display only: an empty part.
    let (px, py) = h.cell_origin(a0.col as f32 + 2.0, a0.row as f32 + 3.0);
    h.double_click(px, py);
    if h.adjust_id() != cal {
        issues.add("pointer: a double click on the calendar didn't pick it up");
    } else {
        // Carry it to the bottom right free area and drop.
        let free = h.cell_origin(13.0 + 2.0, 6.0 + 1.5);
        h.move_to(free.0, free.1);
        h.down(free.0, free.1, MouseButton::Left);
        h.up(free.0, free.1, MouseButton::Left);
        let a1 = tile(&h, cal).unwrap();
        if (a1.col, a1.row) == (a0.col, a0.row) {
            issues.add(format!(
                "pointer: carried the calendar and dropped it, but it stayed ({})",
                h.toast_text()
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
    h.set_editing(true);
    let gap = h.gap();
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
    let place_back = |h: &mut Harness| h.resize_widget(notes, home.col, home.row, home.w, home.h);
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
        let t = tile(&h, notes).unwrap();
        let want_w = if sx != 0 { min_w } else { home.w };
        let want_h = if sy != 0 { min_h } else { home.h };
        let want = (
            if sx < 0 { home.col + home.w - want_w } else { home.col },
            if sy < 0 { home.row + home.h - want_h } else { home.row },
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
            (at.0 + sx as f32 * 40.0 * cell, at.1 + sy as f32 * 40.0 * cell),
            7,
        );
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
                h.toast_text()
            ));
        }
        issues.check(&h, &format!("pointer: pulling the {name} handle past the grid"));
        place_back(&mut h);
    }
    // A handle pressed and let go changes nothing, and neither does a pull that comes back.
    let at = point(&h, &home, -1, -1, 0.5);
    h.down(at.0, at.1, MouseButton::Left);
    h.drag_to(at.0 - cell, at.1 - cell);
    h.drag_to(at.0, at.1);
    h.up(at.0, at.1, MouseButton::Left);
    if tile(&h, notes).unwrap() != home {
        issues.add("pointer: a pull that came back moved the notes");
    }
    // The handle's reach covers the gap: the frame's outermost pixel grabs too.
    let (x0, y0) = h.cell_origin(home.col as f32, home.row as f32);
    let edge = (x0 + 1.0, y0 + home.h as f32 * cell / 2.0);
    h.drag(edge, (edge.0 - cell, edge.1), 5);
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
        h.resize_widget(volume, round.col, round.row, round.w, round.h);
    }
}

/// A press held still on a widget opens that widget alone for adjusting, on its controls too,
/// which don't react. Moving off or letting go in time is just a press; the widget stays as it is
/// otherwise. The same press goes on to drag it.
fn pointer_long_press(issues: &mut Issues) {
    let mut h = dashboard(MIXED);
    let cell = h.cell();
    let cal = id_of(&h, "calendar");
    let c0 = tile(&h, cal).unwrap();
    let on_cal = h.cell_origin(c0.col as f32 + 2.0, c0.row as f32 + 3.0);

    // Let go too soon: nothing.
    h.down(on_cal.0, on_cal.1, MouseButton::Left);
    h.wait_ms(150);
    h.up(on_cal.0, on_cal.1, MouseButton::Left);
    h.wait_ms(600);
    if h.adjust_id() != 0 {
        issues.add("pointer: a short press opened the widget for adjusting");
    }
    // Moved off while held: nothing.
    h.down(on_cal.0, on_cal.1, MouseButton::Left);
    h.drag_to(on_cal.0 + 30.0, on_cal.1);
    h.wait_ms(700);
    let moved_off = h.adjust_id();
    h.up(on_cal.0 + 30.0, on_cal.1, MouseButton::Left);
    if moved_off != 0 {
        issues.add("pointer: a press that moved still counted as a long press");
    }
    // A little wobble doesn't matter; held long enough it opens the widget, and letting go leaves
    // it open, with no edit mode around it and no settings opened.
    h.down(on_cal.0, on_cal.1, MouseButton::Left);
    h.drag_to(on_cal.0 + 2.0, on_cal.1 + 1.0);
    h.wait_ms(700);
    if h.adjust_id() != cal {
        issues.add("pointer: holding on the calendar didn't open it for adjusting");
    }
    h.up(on_cal.0 + 2.0, on_cal.1 + 1.0, MouseButton::Left);
    if h.adjust_id() != cal {
        issues.add("pointer: letting go of the long press closed the widget again");
    }
    if h.editing() {
        issues.add("pointer: letting go of the long press opened edit mode for all widgets");
    }
    if h.settings_id() != 0 {
        issues.add("pointer: letting go of the long press opened the widget's settings");
    }
    issues.check(&h, "pointer: a long press on the calendar");
    // Its handles work now, and only its own.
    let (x1, y1) = h.cell_origin((c0.col + c0.w) as f32, c0.row as f32 + 2.0);
    let gap = h.gap();
    h.drag((x1 - gap, y1), (x1 - gap - cell, y1), 5);
    let c1 = tile(&h, cal).unwrap();
    if c1.w != c0.w - 1 {
        issues.add(format!(
            "pointer: the right edge after a long press: width {} from {}",
            c1.w, c0.w
        ));
    }
    // Escape puts it down.
    h.key("esc");
    if h.adjust_id() != 0 {
        issues.add("pointer: Escape didn't end adjusting after a long press");
    }
    if h.editing() {
        issues.add("pointer: Escape after a long press left edit mode on");
    }

    // From any state: the keyboard's hints, an entered widget, a blown-up one, or another widget
    // already open for adjusting all give way to the held widget.
    {
        let mut h = dashboard(MIXED);
        let cal = id_of(&h, "calendar");
        let c0 = tile(&h, cal).unwrap();
        let on_cal = h.cell_origin(c0.col as f32 + 2.0, c0.row as f32 + 3.0);
        let hold = |h: &mut Harness| {
            h.down(on_cal.0, on_cal.1, MouseButton::Left);
            h.wait_ms(700);
            h.up(on_cal.0, on_cal.1, MouseButton::Left);
        };
        let other = h.shown().iter().map(|w| w.0).find(|&i| i != cal).unwrap();
        for (what, setup) in [
            ("the hints", "f"),
            ("an entered widget", "f ca"),
            ("a blown-up widget", "f ca e"),
        ] {
            h.update(|d, _| d.edit.adjust_id = 0);
            for k in setup.split_whitespace() {
                if k.len() > 1 {
                    for c in k.chars() {
                        h.key(&c.to_string());
                    }
                } else {
                    h.key(k);
                }
            }
            if h.nav_mode() == NavMode::Off {
                issues.add(format!("pointer: couldn't get into {what} to hold from it"));
            }
            let before = h.read(|d| d.nav.expanded_id);
            hold(&mut h);
            if h.adjust_id() != cal {
                issues.add(format!(
                    "pointer: a long press from {what} didn't open the widget (expanded {before}, adjust now {}, cal {cal})",
                    h.adjust_id()
                ));
            }
            if h.nav_mode() != NavMode::Off {
                issues.add(format!(
                    "pointer: a long press from {what} left keyboard navigation on"
                ));
            }
            issues.check(&h, &format!("pointer: a long press from {what}"));
            h.key("esc");
        }
        // Another widget open for adjusting: this one takes over.
        h.update(|d, _| d.edit.adjust_id = other);
        hold(&mut h);
        if h.adjust_id() != cal {
            issues.add(
                "pointer: a long press while another widget was open didn't switch to the held one",
            );
        }
    }

    // On a control: the same, and the control doesn't get the click. The session widget is all
    // buttons, and its lock button asks the surface to hide.
    let mut h = dashboard(MIXED);
    let session = id_of(&h, "session");
    let s0 = tile(&h, session).unwrap();
    let on_button = h.cell_origin(s0.col as f32 + 0.5, s0.row as f32 + 0.5);
    h.controls();
    h.down(on_button.0, on_button.1, MouseButton::Left);
    h.wait_ms(700);
    h.up(on_button.0, on_button.1, MouseButton::Left);
    if h.adjust_id() != session {
        issues.add("pointer: holding on a session button didn't open the widget");
    }
    if !h.controls().is_empty() {
        issues.add("pointer: letting go of a long press on a session button clicked it");
    }
    issues.check(&h, "pointer: a long press on a control");

    // The same press goes on to drag the widget.
    let mut h = dashboard(MIXED);
    let cal = id_of(&h, "calendar");
    let on_cal = h.cell_origin(c0.col as f32 + 2.0, c0.row as f32 + 3.0);
    h.down(on_cal.0, on_cal.1, MouseButton::Left);
    h.wait_ms(700);
    let to = (on_cal.0, on_cal.1 + cell);
    for i in 1..=5 {
        h.drag_to(on_cal.0, on_cal.1 + cell * i as f32 / 5.0);
    }
    h.up(to.0, to.1, MouseButton::Left);
    let c2 = tile(&h, cal).unwrap();
    if c2.row != c0.row + 1 {
        issues.add(format!(
            "pointer: dragging on from a long press: row {} from {} ({})",
            c2.row,
            c0.row,
            h.toast_text()
        ));
    }
    issues.check(&h, "pointer: dragging on from a long press");

    // Where it means nothing: edit mode is on, or the background.
    let mut h = dashboard(MIXED);
    let free = h.cell_origin(15.5, 8.5);
    h.down(free.0, free.1, MouseButton::Left);
    h.wait_ms(700);
    let on_background = h.adjust_id();
    h.drag_to(free.0 + 40.0, free.1);
    h.up(free.0 + 40.0, free.1, MouseButton::Left);
    if on_background != 0 {
        issues.add("pointer: holding on the background opened a widget");
    }
    h.set_editing(true);
    let cal = id_of(&h, "calendar");
    let at = h.cell_origin(c0.col as f32 + 2.0, c0.row as f32 + 3.0);
    h.down(at.0, at.1, MouseButton::Left);
    h.wait_ms(700);
    h.up(at.0, at.1, MouseButton::Left);
    if h.adjust_id() != 0 {
        issues.add("pointer: holding in edit mode opened the widget on its own");
    }
    if h.settings_id() != cal {
        issues.add("pointer: in edit mode a held click no longer opens the settings");
    }
}

/// A drag lands where the pointer lets go, however many motion events the drag is made of (a fast
/// flick sends few, a slow drag many), on an empty grid so nothing has to make room.
fn pointer_drag_precision(issues: &mut Issues) {
    let mut h =
        dashboard("grid columns=16 rows=9\nlayout {\n    clock col=2 row=2 width=3 height=2\n}\n");
    h.set_editing(true);
    let cell = h.cell();
    let clock = id_of(&h, "clock");
    let mut landed = Vec::new();
    for (dc, dr) in [(3, 0), (0, 2), (5, 3), (-2, -1)] {
        for steps in [1, 2, 5, 20, 60] {
            h.move_widget(clock, 2, 2);
            let from = h.cell_origin(3.5, 3.0);
            let to = (from.0 + dc as f32 * cell, from.1 + dr as f32 * cell);
            h.drag(from, to, steps);
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
            fresh.set_editing(true);
            let from = fresh.cell_origin(1.0, 1.0);
            let to = (from.0 + dc as f32 * cell, from.1 + dr as f32 * cell);
            fresh.drag(from, to, steps);
            let t = tile(&fresh, clock).unwrap();
            if (t.col, t.row) != (dc, dr) {
                landed.push(format!(
                    "onto others ({dc:+},{dr:+}) in {steps} moves → ({:+},{:+}) [{}]",
                    t.col,
                    t.row,
                    fresh.toast_text()
                ));
            }
            issues.check(&fresh, &format!("drag onto others ({dc},{dr}) in {steps} moves"));
        }
    }
    if !landed.is_empty() {
        issues.add(format!(
            "pointer: drags don't land where they are let go (cells moved, wanted → got): {}",
            landed.join("; ")
        ));
    }
}

/// A new widget dragged in from the palette lands where it is let go; a click on an entry adds it
/// where there is room.
fn pointer_palette(issues: &mut Issues) {
    let mut h = dashboard("grid columns=16 rows=9\nlayout {\n    clock col=0 row=0 width=4 height=2\n}\n");
    h.set_editing(true);
    let cell = h.cell();
    let n = h.tiles().len();
    // Drag the first entry that isn't the clock onto a free spot of the grid.
    let name = testing::title("volume");
    let from = {
        let b = h.bounds_of_named(name).expect("the Volume entry of the palette");
        let c = b.center();
        (f32::from(c.x), f32::from(c.y))
    };
    let to = h.cell_origin(8.5, 4.5);
    h.drag(from, to, 10);
    if h.tiles().len() != n + 1 {
        issues.add(format!(
            "palette: dragging Volume onto the grid made {} widgets from {n} ({})",
            h.tiles().len(),
            h.toast_text()
        ));
    } else {
        let t = h.tiles().into_iter().find(|t| t.kind == "volume").unwrap();
        let near = (t.col - 7).abs() <= 1 && (t.row - 3).abs() <= 1;
        if !near {
            issues.add(format!(
                "palette: Volume was let go at cell 8,4 and landed at {},{}",
                t.col, t.row
            ));
        }
    }
    issues.check(&h, "palette: dragging in");
    // Let go off the grid: nothing is added.
    let n = h.tiles().len();
    h.drag(from, (from.0 - 10.0, from.1 + 80.0), 10);
    if h.tiles().len() != n {
        issues.add("palette: letting go over the sidebar added a widget");
    }
    // A click adds.
    h.click_named(name);
    if h.tiles().len() != n + 1 {
        issues.add(format!(
            "palette: a click on Volume made {} widgets from {n} ({})",
            h.tiles().len(),
            h.toast_text()
        ));
    }
    let _ = cell;
    issues.check(&h, "palette: a click");
}

// ------------------------------------------------------------------------------------------
// The keyboard in edit mode.

/// Move and resize modes (Ctrl+M, Ctrl+R) from inside a widget, without edit mode.
fn keyboard_modes(issues: &mut Issues) {
    let mut h = dashboard(MIXED);
    let clock = id_of(&h, "clock");
    h.key("f");
    h.type_label("clock");
    if h.nav_mode() != NavMode::Widget {
        issues.add("modes: the clock's label didn't enter it");
    }
    let c0 = tile(&h, clock).unwrap();
    h.key("ctrl+m");
    if h.nav_op() != NavOp::Move {
        issues.add("modes: Ctrl+M inside a widget didn't start move mode");
    }
    // The widget has handed the keyboard over: a letter does nothing, an arrow moves.
    h.key("m");
    h.key("j");
    let c1 = tile(&h, clock).unwrap();
    if (c1.col, c1.row) == (c0.col, c0.row) {
        issues.add(format!(
            "modes: j in move mode left the clock at {},{} ({})",
            c0.col,
            c0.row,
            h.toast_text()
        ));
    }
    issues.check(&h, "modes: moving");
    h.key("ctrl+r");
    h.key("l");
    let c2 = tile(&h, clock).unwrap();
    if c2.w != c1.w + 1 {
        issues.add(format!(
            "modes: l left the clock {}x{} from {}x{}",
            c2.w, c2.h, c1.w, c1.h
        ));
    }
    h.key("L");
    if tile(&h, clock).unwrap().w != c1.w {
        issues.add("modes: Shift+l didn't pull the right side in again");
    }
    issues.check(&h, "modes: resizing");
    // Escape ends the mode only; the widget is still entered, and then Escape leaves it.
    h.key("esc");
    if h.nav_op() != NavOp::None || h.nav_mode() != NavMode::Widget {
        issues.add("modes: Escape didn't just end the mode");
    }
    h.key("ctrl+r");
    h.key("ctrl+r");
    if h.nav_op() != NavOp::None {
        issues.add("modes: Ctrl+R twice didn't end resize mode");
    }
    h.key("esc");
    if h.nav_mode() != NavMode::Off {
        issues.add("modes: Escape after the mode didn't leave the widget");
    }
    // Nothing in hand, nothing to do.
    h.key("ctrl+m");
    if h.nav_op() != NavOp::None {
        issues.add("modes: Ctrl+M on the bare dashboard started a mode");
    }
    // The veil and the tips show while it lasts.
    h.key("f");
    h.type_label("calendar");
    h.key("ctrl+r");
    if !h.has("mode veil") {
        issues.add("modes: the widget in hand wears no veil");
    }
    if !h.has("quick tips") {
        issues.add("modes: no quick tips along the bottom");
    }
    h.key("esc");
    if h.has("mode veil") {
        issues.add("modes: the veil stayed after Escape");
    }
    issues.check(&h, "modes: after");
}

/// Edit mode with the keyboard alone, start to end: everything the pointer can do there.
fn keyboard_editing(issues: &mut Issues) {
    let mut h = dashboard(MIXED);

    // In from inside a widget.
    h.key("f");
    h.type_label("wifi");
    if h.nav_mode() != NavMode::Widget {
        issues.add("keyboard: typing the Wi-Fi label didn't enter it");
    }
    h.key("ctrl+e");
    if !h.editing() {
        issues.add("keyboard: Ctrl+E inside a widget didn't open edit mode");
    }
    if h.nav_mode() != NavMode::Off {
        issues.add("keyboard: edit mode opened with a widget still entered");
    }
    issues.check(&h, "keyboard: Ctrl+E from a widget");

    // The sidebar's tabs.
    h.key("tab");
    if h.drawer_tab() != T::Arrange {
        issues.add(format!("keyboard: Tab from Widgets went to {:?}", h.drawer_tab()));
    }
    h.key("tab");
    if h.drawer_tab() != T::Colors {
        issues.add(format!("keyboard: Tab from Arrange went to {:?}", h.drawer_tab()));
    }
    h.key("tab");
    if h.drawer_tab() != T::Style {
        issues.add(format!("keyboard: Tab from Colors went to {:?}", h.drawer_tab()));
    }
    h.key("tab");
    if h.drawer_tab() != T::Widgets {
        issues.add(format!(
            "keyboard: Tab from Style, no widget open, went to {:?}",
            h.drawer_tab()
        ));
    }
    h.key("backtab");
    if h.drawer_tab() != T::Style {
        issues.add(format!("keyboard: Shift+Tab from Widgets went to {:?}", h.drawer_tab()));
    }
    h.key("tab");

    // The grid's steppers: l and h step, a refused step shakes, Backspace goes back to automatic.
    let (c0, r0, _) = h.grid();
    h.key("g"); // Tab showed the cursor; Columns is the first stop
    h.key("l");
    if h.grid().0 != c0 + 1 {
        issues.add(format!("keyboard: l on Columns gave {:?} from {c0}", h.grid()));
    }
    h.key("h");
    h.key("j");
    h.key("l");
    if h.grid().1 != r0 + 1 {
        issues.add(format!("keyboard: l on Rows gave {:?} from {r0}", h.grid()));
    }
    h.key("h");
    issues.check(&h, "keyboard: the grid steppers");
    let shakes = h.refusals(1);
    for _ in 0..12 {
        h.key("h");
    }
    if h.refusals(1) == shakes {
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
    h.key("backspace");
    if h.grid().2 {
        issues.add("keyboard: Backspace on a grid stepper left the grid explicit");
    }
    issues.check(&h, "keyboard: Backspace on a grid stepper");

    // The palette: down to an entry, Enter adds it.
    let n = h.tiles().len();
    h.key("j");
    h.key("j");
    h.key("enter");
    if h.tiles().len() != n + 1 {
        issues.add(format!(
            "keyboard: Enter on a palette entry: {n} widgets became {} ({})",
            h.tiles().len(),
            h.toast_text()
        ));
    }
    issues.check(&h, "keyboard: adding from the palette");

    // Colors (past Arrange): another scheme.
    h.key("tab");
    h.key("tab");
    let schemes = h.schemes();
    if schemes.len() > 1 {
        let current = schemes.iter().position(|s| s.1).unwrap_or(0);
        let want = if current == 0 { 1 } else { 0 };
        h.key("g");
        for _ in 0..want {
            h.key("j");
        }
        h.key("enter");
        let now = h.schemes();
        if !now[want].1 {
            issues.add(format!(
                "keyboard: Enter on color scheme {:?} didn't choose it",
                now[want].0
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
    h.key("tab");
    let adjust = h.style_presets();
    h.key("g");
    for _ in 0..adjust {
        h.key("j");
    }
    let before = h.style_rows()[0].clone();
    let (_, can_inc) = h.style_row_can(0);
    h.key(if can_inc { "l" } else { "h" });
    let after = h.style_rows()[0].clone();
    if after.1 == before.1 {
        issues.add(format!("keyboard: stepping style {:?} changed nothing", before.0));
    }
    if !after.2 {
        issues.add(format!("keyboard: style {:?} stepped but isn't marked", before.0));
    }
    // "Reset all" appeared above it, so the cursor is now on the row above; find the row again.
    h.key("j");
    h.key("backspace");
    if h.style_rows()[0].2 {
        issues.add(format!("keyboard: Backspace didn't reset style {:?}", before.0));
    }
    issues.check(&h, "keyboard: adjusting the style");
    h.key("tab");

    // A widget: f and its label open its settings, without entering it.
    h.key("f");
    if h.nav_mode() != NavMode::Hints {
        issues.add("keyboard: f in edit mode doesn't show the hints");
    }
    h.type_label("clock");
    let clock = id_of(&h, "clock");
    if h.nav_mode() != NavMode::Off {
        issues.add("keyboard: picking a widget in edit mode entered it");
    }
    if h.settings_id() != clock || h.setting_rows().is_empty() {
        issues.add("keyboard: picking the clock in edit mode didn't open its settings");
    }
    if h.drawer_tab() != T::Widgets {
        issues.add(format!(
            "keyboard: opening the clock's settings left the sidebar's tab for {:?}",
            h.drawer_tab()
        ));
    }

    // Its settings: a stepper, a switch, the text field. (Time format, Seconds, Date, Zones.)
    let labels = testing::setting_labels("clock");
    let at = |l: &str| labels.iter().position(|x| *x == l).unwrap();
    let row = |h: &Harness, l: &str| h.setting_rows()[at(l)].clone();
    h.key("g");
    let was = row(&h, "Time format");
    h.key(if was.can_inc { "l" } else { "h" });
    if row(&h, "Time format").value == was.value {
        issues.add("keyboard: h/l on the clock's time format changed nothing");
    }
    h.key("backspace");
    if row(&h, "Time format").changed {
        issues.add("keyboard: Backspace didn't reset the clock's time format");
    }
    for _ in 0..at("Seconds") {
        h.key("j");
    }
    let was = row(&h, "Seconds");
    h.key("space");
    if row(&h, "Seconds").on == was.on {
        issues.add("keyboard: Space on the Seconds switch didn't flip it");
    }
    h.key("enter");
    if row(&h, "Seconds").on != was.on {
        issues.add("keyboard: Enter on the Seconds switch didn't flip it back");
    }
    issues.check(&h, "keyboard: the clock's stepper and switch");
    h.key("G");
    h.key("enter");
    h.type_text("Tokyo");
    if row(&h, "Other time zones").changed {
        issues.add("keyboard: the text field saved before Enter");
    }
    h.key("esc");
    if !h.editing() {
        issues.add("keyboard: Escape in a sidebar text field left edit mode");
    }
    // What was typed is dropped: typing again doesn't add to it.
    h.key("enter");
    h.type_text("Tokyo");
    h.key("enter");
    if !h.tiles().iter().any(|t| {
        t.settings
            .iter()
            .any(|(k, v)| k == "zones" && v.contains("Tokyo") && !v.contains("TokyoTokyo"))
    }) {
        issues.add(format!(
            "keyboard: typing a time zone into the clock's field and Enter didn't save it: {:?}",
            row(&h, "Other time zones").value
        ));
    }
    // After Enter the keys work again.
    h.key("k");
    h.key("space");
    issues.check(&h, "keyboard: the clock's text field");
    if !h
        .tiles()
        .iter()
        .any(|t| t.settings.iter().any(|(k, _)| k == "date"))
    {
        issues.add("keyboard: after a text field's Enter, the sidebar's keys don't work");
    }

    // Move mode (Ctrl+M): h j k l move the widget a cell, pushing others aside, until Escape.
    let c0 = tile(&h, clock).unwrap();
    h.key("ctrl+m");
    if h.nav_op() != NavOp::Move {
        issues.add("keyboard: Ctrl+M didn't start move mode");
    }
    h.key("l");
    let c1 = tile(&h, clock).unwrap();
    if (c1.col, c1.row) != (c0.col + 1, c0.row) {
        issues.add(format!(
            "keyboard: l in move mode moved the clock from {},{} to {},{} ({})",
            c0.col,
            c0.row,
            c1.col,
            c1.row,
            h.toast_text()
        ));
    }
    issues.check(&h, "keyboard: moving the clock");
    // Resize mode (Ctrl+R): a key pulls its own side out, Shift pulls it in.
    h.key("ctrl+r");
    if h.nav_op() != NavOp::Resize {
        issues.add("keyboard: Ctrl+R didn't switch to resize mode");
    }
    h.key("l");
    let c2 = tile(&h, clock).unwrap();
    if c2.w != c1.w + 1 || c2.col != c1.col {
        issues.add(format!(
            "keyboard: l made the clock {}x{} at {} from {}x{} at {}",
            c2.w, c2.h, c2.col, c1.w, c1.h, c1.col
        ));
    }
    h.key("L");
    if tile(&h, clock).unwrap().w != c1.w {
        issues.add("keyboard: Shift+l didn't pull the right side in again");
    }
    // h pulls the left side out: the right edge stays.
    let right = c1.col + c1.w;
    h.key("h");
    let c3 = tile(&h, clock).unwrap();
    if c3.w != c1.w + 1 || c3.col + c3.w != right {
        issues.add(format!(
            "keyboard: h made the clock {}x{} at {} (right edge was {right})",
            c3.w, c3.h, c3.col
        ));
    }
    h.key("H");
    if tile(&h, clock).unwrap().w != c1.w {
        issues.add("keyboard: Shift+h didn't pull the left side in again");
    }
    h.key("j");
    let c4 = tile(&h, clock).unwrap();
    if c4.h != c1.h + 1 {
        issues.add(format!(
            "keyboard: j didn't make the clock taller ({} from {})",
            c4.h, c1.h
        ));
    }
    h.key("J");
    if tile(&h, clock).unwrap().h != c1.h {
        issues.add("keyboard: Shift+j didn't pull the bottom in again");
    }
    // Other keys do nothing in the mode; the arrows work like h j k l.
    h.key("x");
    if tile(&h, clock).is_none() {
        issues.add("keyboard: x removed the widget in resize mode");
    }
    h.key("right");
    if tile(&h, clock).unwrap().w != c1.w + 1 {
        issues.add("keyboard: the right arrow didn't widen the clock");
    }
    h.key("shift+right");
    if tile(&h, clock).unwrap().w != c1.w {
        issues.add("keyboard: Shift+right didn't narrow the clock");
    }
    if !h.size_note().contains('×') {
        issues.add("keyboard: sizing didn't say how big the widget is");
    }
    issues.check(&h, "keyboard: sizing the clock");
    // Escape ends the mode and keeps the widget picked.
    h.key("esc");
    if h.nav_op() != NavOp::None || h.settings_id() != clock {
        issues.add("keyboard: Escape in resize mode didn't just end the mode");
    }
    h.key("s");
    let next = h.settings_id();
    if next == clock || next == 0 {
        issues.add("keyboard: s in edit mode didn't go to the widget below");
    }
    issues.check(&h, "keyboard: the widget below");

    // Shape and removal, on the volume knob.
    h.key("f");
    h.type_label("volume");
    let volume = id_of(&h, "volume");
    let round = tile(&h, volume).unwrap().round;
    h.key("o");
    if tile(&h, volume).unwrap().round == round {
        issues.add(format!(
            "keyboard: o didn't change the volume's shape ({})",
            h.toast_text()
        ));
    }
    issues.check(&h, "keyboard: reshaping the volume");
    h.key("x");
    if tile(&h, volume).is_some() {
        issues.add("keyboard: x didn't remove the widget");
    }
    if h.settings_id() != 0 {
        issues.add("keyboard: the removed widget's settings stayed open");
    }
    issues.check(&h, "keyboard: removing the volume");

    // Reset: the last stop of the Widgets tab.
    h.key("G");
    h.key("enter");
    issues.check(&h, "keyboard: resetting the layout");

    // Out: Ctrl+E closes it; Escape does too; the hints work again.
    h.key("ctrl+e");
    if h.editing() {
        issues.add("keyboard: Ctrl+E didn't close edit mode");
    }
    h.key("f");
    if h.nav_mode() != NavMode::Hints {
        issues.add("keyboard: after edit mode, f doesn't show the hints");
    }
    h.key("ctrl+e");
    if !h.editing() {
        issues.add("keyboard: Ctrl+E did nothing while the hints were up");
    }
    h.key("f");
    h.key("esc");
    if !h.editing() || h.nav_mode() != NavMode::Off {
        issues.add("keyboard: Escape with the hints up in edit mode should only close the hints");
    }
    h.key("esc");
    if h.editing() {
        issues.add("keyboard: Escape didn't close edit mode");
    }
    issues.check(&h, "keyboard: leaving edit mode");
}

// ------------------------------------------------------------------------------------------
// The grid.

/// Shrinking the grid step by step: a step that would leave no room for a widget is refused (the
/// stepper shakes, a toast names the widgets, nothing changes), so no widget is ever lost; growing
/// back then restores the layout as it was.
fn grid_shrink_and_grow(issues: &mut Issues) {
    let mut h = dashboard(MIXED);
    h.set_editing(true);
    let start = h.tiles();
    let (c0, r0, _) = h.grid();
    let mut refused = [false, false];
    for _ in 0..80 {
        for index in 0..2 {
            if refused[index] || !h.grid_controls()[index].can_dec {
                continue;
            }
            let before = (h.grid(), h.tiles());
            let s0 = h.refusals(index);
            h.grid_step(index, -1);
            issues.check(
                &h,
                &format!("grid: − on {} at {:?}", ["columns", "rows"][index], before.0),
            );
            if h.grid() == before.0 {
                refused[index] = true;
                if h.refusals(index) != s0 + 1 {
                    issues.add("grid: a refused step didn't shake the stepper");
                }
                if !h.toast_text().starts_with("No room for") {
                    issues.add(format!("grid: a refused step said {:?}", h.toast_text()));
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
        issues.add(format!("grid: shrinking lost {} widget(s)", start.len() - now.len()));
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
            h.grid_step(0, 1);
        }
        if r < r0 {
            h.grid_step(1, 1);
        }
    }
    issues.check(&h, "grid: grown back");
    if h.tiles().len() != start.len() || h.tiles().iter().any(|t| !t.visible) {
        issues.add("grid: back at the old size, not every widget is shown");
    }
    h.grid_auto();
    issues.check(&h, "grid: back to the automatic grid");
    if h.grid().2 {
        issues.add("grid: the reset button left the grid explicit");
    }
}

// ------------------------------------------------------------------------------------------
// The settings card follows the widgets.

/// The settings card belongs to one widget: it closes when that widget goes, follows it when it
/// moves or changes shape, and a removed widget's hint label is gone.
fn sidebar_follows(issues: &mut Issues) {
    let mut h = dashboard(PACKED);
    h.set_editing(true);
    let clock = id_of(&h, "clock");
    h.configure(clock);
    h.move_widget(clock, 0, 2);
    issues.check(&h, "sidebar: moved the configured clock");
    if h.settings_id() != clock {
        issues.add("sidebar: moving the widget closed its settings");
    }
    h.toggle_shape(clock);
    issues.check(&h, "sidebar: reshaped the configured clock");
    h.remove_widget(clock);
    issues.check(&h, "sidebar: removed the configured clock");
    if h.settings_id() != 0 {
        issues.add("sidebar: settings stay open for a removed widget");
    }
    // Packed: adding has nowhere to go and says so.
    let n = h.tiles().len();
    for _ in 0..3 {
        h.add_widget("calendar");
    }
    issues.check(&h, "sidebar: adding to a full grid");
    let added = h.tiles().len() - n;
    if added > 0 && h.tiles().iter().filter(|t| !t.visible).count() > 0 {
        issues.add("sidebar: adding to a full grid added a widget that isn't shown");
    }
    // Resetting brings the default layout, and the settings close.
    h.configure(id_of(&h, "calendar"));
    h.reset_layout();
    issues.check(&h, "sidebar: reset the layout");
}

// ------------------------------------------------------------------------------------------
// The Arrange tab.

/// Every algorithm lays the widgets out without losing or overlapping any, undo puts the old
/// layout back, the settings copy as a line config.kdl reads, and the rows follow the algorithm.
fn arrange_tab(issues: &mut Issues) {
    let rects = |h: &Harness| -> Vec<(u32, i32, i32, i32, i32)> {
        h.tiles().iter().map(|t| (t.id, t.col, t.row, t.w, t.h)).collect()
    };
    for (config, name) in [(MIXED, "mixed"), (PACKED, "packed")] {
        let mut h = dashboard(config);
        h.key("ctrl+e");
        h.set_drawer_tab(T::Arrange);
        let before = rects(&h);
        let mut row_counts = Vec::new();
        for round in 0..4 {
            row_counts.push(h.arrange_rows().len());
            h.arrange_now();
            issues.check(&h, &format!("arrange: {name}, algorithm {round}"));
            let lost = h.tiles().iter().filter(|t| !t.visible).count();
            if lost > 0 && name == "mixed" {
                issues.add(format!("arrange: {name}, algorithm {round} lost {lost} widgets"));
            }
            if h.arrange_result().is_empty() {
                issues.add("arrange: no summary under the buttons");
            }
            // The next algorithm.
            h.arrange_step(0, 1);
        }
        if row_counts.iter().collect::<BTreeSet<_>>().len() < 2 {
            issues.add(format!("arrange: the rows don't follow the algorithm: {row_counts:?}"));
        }
        if !h.arrange_can_undo() {
            issues.add("arrange: nothing to undo after arranging");
        }
        h.arrange_undo();
        if rects(&h) != before {
            issues.add(format!("arrange: undo didn't bring {name} back as it was"));
        }
        if h.arrange_can_undo() {
            issues.add("arrange: still something to undo after undoing");
        }
        issues.check(&h, &format!("arrange: {name}, undone"));

        // Shuffling steps the seed and arranges again.
        let seed = |h: &Harness| {
            h.arrange_rows()
                .iter()
                .find(|r| r.label == "Seed")
                .map(|r| r.value.clone())
        };
        h.arrange_shuffle();
        if seed(&h).as_deref() != Some("1") {
            issues.add(format!("arrange: shuffle left the seed at {:?}", seed(&h)));
        }
        issues.check(&h, &format!("arrange: {name}, shuffled"));

        // Copy: a toast.
        h.arrange_copy();
        if !h.toast_text().contains("Copied") {
            issues.add(format!("arrange: copying said {:?}", h.toast_text()));
        }
        // Reset puts a setting back.
        let seed_row = h
            .arrange_rows()
            .iter()
            .find(|r| r.label == "Seed")
            .map(|r| r.index);
        if let Some(i) = seed_row {
            h.arrange_reset(i);
            if seed(&h).as_deref() != Some("0") {
                issues.add("arrange: resetting the seed left it changed");
            }
        }
    }

    // Keyboard: on the tab the first stop is the Arrange button, Enter presses it.
    let mut h = dashboard(MIXED);
    h.key("ctrl+e");
    h.set_drawer_tab(T::Arrange);
    let before = rects(&h);
    h.key("tab");
    h.key("enter");
    issues.check(&h, "arrange: Enter on the Arrange button");
    if h.arrange_result().is_empty() && rects(&h) == before {
        issues.add("arrange: Enter on the first stop didn't arrange");
    }
}

// ------------------------------------------------------------------------------------------
// Pinning.

/// The quick tips along the bottom follow what the keys do just now.
fn shortcut_hints(issues: &mut Issues) {
    let has = |h: &Harness, what: &str| h.tips().iter().any(|s| s.contains(what));
    let mut h = dashboard(MIXED);
    let clock = id_of(&h, "clock");
    // Outside edit mode they say how to begin, and what a widget has once entered.
    if !has(&h, "pick a widget") || !has(&h, "edit mode") {
        issues.add(format!("hints: nothing picked, no edit mode: {:?}", h.tips()));
    }
    h.key("f");
    h.type_label("clock");
    for want in ["Ctrl+M move", "Ctrl+R resize", "e expand"] {
        if !has(&h, want) {
            issues.add(format!("hints: in the clock, no {want:?}: {:?}", h.tips()));
        }
    }
    h.key("ctrl+m");
    if !has(&h, "MOVE") || !has(&h, "Ctrl+R resize instead") {
        issues.add(format!("hints: move mode: {:?}", h.tips()));
    }
    h.key("ctrl+r");
    if !has(&h, "RESIZE") || !has(&h, "Shift + h j k l pull it in") {
        issues.add(format!("hints: resize mode: {:?}", h.tips()));
    }
    h.key("esc");
    if !has(&h, "Ctrl+M move") {
        issues.add(format!("hints: after the mode: {:?}", h.tips()));
    }
    h.key("esc");
    h.key("ctrl+e");
    if !has(&h, "pick a widget") || has(&h, "h j k l size") {
        issues.add(format!("hints: nothing picked: {:?}", h.tips()));
    }
    h.key("f");
    if !has(&h, "Type a label") {
        issues.add(format!("hints: with the labels up: {:?}", h.tips()));
    }
    h.key("f");
    // A widget picked: the keys are on it.
    h.configure(clock);
    for want in [
        "Ctrl+M move",
        "Ctrl+R resize",
        "h j k l pick another",
        "pin",
    ] {
        if !has(&h, want) {
            issues.add(format!("hints: clock picked, no {want:?}: {:?}", h.tips()));
        }
    }
    if !h.tips().iter().any(|s| s.contains("Clock")) {
        issues.add("hints: the picked widget isn't named");
    }
    // Pinned: it can't be sized or moved, and says how to unpin.
    h.key("p");
    if !has(&h, "p unpin") || has(&h, "Ctrl+M") {
        issues.add(format!("hints: clock pinned: {:?}", h.tips()));
    }
    h.key("p");
    if !has(&h, "Ctrl+M move") {
        issues.add(format!("hints: clock unpinned again: {:?}", h.tips()));
    }
    // Tab goes to the settings: the keys of the rows, and the way back.
    h.key("tab");
    if !has(&h, "w a s d") || has(&h, "Ctrl+M") {
        issues.add(format!("hints: in the sidebar: {:?}", h.tips()));
    }
    h.key("d");
    if !has(&h, "Ctrl+M move") {
        issues.add(format!("hints: back on the widgets: {:?}", h.tips()));
    }
    h.key("ctrl+e");
    if has(&h, "pick another") || !has(&h, "pick a widget") {
        issues.add(format!("hints: after leaving edit mode: {:?}", h.tips()));
    }
    issues.check(&h, "hints: the quick tips");
}

/// A pinned widget can't be moved, resized or pushed, the others work around it, Arrange leaves it
/// where it is, `p` toggles it, and config.kdl remembers.
fn pinning(issues: &mut Issues) {
    let rect = |h: &Harness, id: u32| tile(h, id).map(|t| (t.col, t.row, t.w, t.h));
    let mut h = dashboard(MIXED);
    h.key("ctrl+e");
    let clock = id_of(&h, "clock");
    let calendar = id_of(&h, "calendar");
    let was = rect(&h, clock);

    h.toggle_pin(clock);
    issues.check(&h, "pin: the clock");
    if h.pinned(clock) != Some(true) {
        issues.add("pin: the toolbar's lock didn't pin the clock");
    }
    let layout_block = |h: &Harness| {
        let t = h.config_text();
        t.find("\nlayout {").map_or(String::new(), |i| t[i..].to_string())
    };
    if !layout_block(&h).contains("pinned=#true") {
        issues.add("pin: config.kdl doesn't say the clock is pinned");
    }

    // It stays: dragged away, shrunk, reshaped, or run over by another widget.
    h.move_widget(clock, 6, 6);
    if rect(&h, clock) != was || !h.toast_text().contains("Pinned") {
        issues.add(format!(
            "pin: dragging a pinned clock gave {:?} and {:?}",
            rect(&h, clock),
            h.toast_text()
        ));
    }
    h.resize_widget(clock, 0, 0, 2, 1);
    h.toggle_shape(clock);
    if rect(&h, clock) != was {
        issues.add("pin: a pinned clock was resized or reshaped");
    }
    h.move_widget(calendar, 0, 0);
    if !h.toast_text().contains("pinned") {
        issues.add(format!("pin: dropping onto a pinned clock said {:?}", h.toast_text()));
    }
    if rect(&h, clock) != was {
        issues.add("pin: dropping onto the clock moved it");
    }
    issues.check(&h, "pin: refused edits");

    // Others still move, around it: push the calendar's neighbours about.
    for (c, r) in [(0, 3), (1, 2), (0, 2)] {
        h.move_widget(calendar, c, r);
        if rect(&h, clock) != was {
            issues.add(format!("pin: moving the calendar to {c},{r} shoved the clock"));
        }
        issues.check(&h, "pin: moving others");
    }

    // Arrange keeps it, whatever the algorithm.
    h.set_drawer_tab(T::Arrange);
    for algo in 0..3 {
        h.arrange_now();
        if rect(&h, clock) != was {
            issues.add(format!("pin: Arrange (algorithm {algo}) moved the pinned clock"));
        }
        if !h.arrange_result().contains("1 pinned") {
            issues.add(format!("pin: the result line says {:?}", h.arrange_result()));
        }
        issues.check(&h, &format!("pin: arranged around the clock, algorithm {algo}"));
        h.arrange_step(0, 1);
    }

    // The keyboard: p on the widget being worked on lets go, and pins again.
    h.configure(clock);
    h.key("p");
    if h.pinned(clock) != Some(false) {
        issues.add("pin: p didn't unpin the clock");
    }
    if layout_block(&h).contains("pinned=") {
        issues.add("pin: config.kdl still says pinned after unpinning");
    }
    h.key("p");
    if h.pinned(clock) != Some(true) {
        issues.add("pin: p didn't pin the clock again");
    }
    h.key("p");
    // Unpinned, it moves again.
    h.move_widget(clock, 5, 7);
    if rect(&h, clock) == was {
        issues.add("pin: the clock wouldn't move after being unpinned");
    }
    issues.check(&h, "pin: unpinned and moved");
}

// ------------------------------------------------------------------------------------------
// The settings card.

/// `s` in a widget opens its settings in the card; Tab steps through its rows; Escape closes the
/// card first and edit mode after.
fn settings_card_keys(issues: &mut Issues) {
    let mut h = dashboard(MIXED);
    let clock = id_of(&h, "clock");
    h.key("f");
    h.type_label("clock");
    if h.editing() {
        issues.add("card: entering the clock opened edit mode");
    }
    h.key("s");
    if !h.editing() || h.settings_id() != clock {
        issues.add(format!(
            "card: s in the clock didn't open its settings (editing {}, open {})",
            h.editing(),
            h.settings_id()
        ));
    }
    if h.setting_rows().is_empty() {
        issues.add("card: the clock's settings have no rows");
    }
    issues.check(&h, "card: s in the clock");

    // Tab goes down the rows: the second one is Seconds, a switch that Space flips.
    let labels = testing::setting_labels("clock");
    let at = labels.iter().position(|l| *l == "Seconds").unwrap();
    for _ in 0..at {
        h.key("tab");
    }
    let was = h.setting_rows()[at].on;
    h.key("space");
    if h.setting_rows()[at].on == was {
        issues.add("card: Tab to the Seconds row and Space didn't flip it");
    }
    if h.drawer_tab() != T::Widgets {
        issues.add("card: Tab in the card switched the sidebar's tabs");
    }

    // Escape: the card, then edit mode.
    h.key("esc");
    if h.settings_id() != 0 || !h.editing() {
        issues.add("card: the first Escape should close the card and stay in edit mode");
    }
    h.key("esc");
    if h.editing() {
        issues.add("card: the second Escape didn't leave edit mode");
    }
    issues.check(&h, "card: Escape twice");

    // A widget without settings says so, and stays out of edit mode.
    let none = h
        .tiles()
        .into_iter()
        .find(|t| testing::setting_labels(&t.kind).is_empty());
    if let Some(t) = none {
        h.key("f");
        h.type_label(&t.kind);
        h.key("s");
        if h.editing() {
            issues.add(format!("card: s in {} (no settings) opened edit mode", t.kind));
        }
        if !h.toast_text().contains("no settings") {
            issues.add(format!("card: s in {} said {:?}", t.kind, h.toast_text()));
        }
    }
}

// ------------------------------------------------------------------------------------------
// Resizing pushes others aside, and lets them back.

/// Growing a widget into its neighbours pushes them out of the way; shrinking it again in the same
/// resize brings them back to where they were, and ending the resize without a drop leaves
/// everything where it was.
fn resize_gives_room_back(issues: &mut Issues) {
    const ROOMY: &str = "grid columns=16 rows=9\nlayout {\n    clock col=0 row=0 width=4 height=2\n    notes col=4 row=0 width=3 height=3\n    wifi col=0 row=3 width=3 height=3\n}\n";
    let mut h = dashboard(ROOMY);
    h.set_editing(true);
    let clock = id_of(&h, "clock");
    // Where the views show it, preview included (the layout itself only changes on a drop).
    let at = |h: &Harness, kind: &str| {
        let id = id_of(h, kind);
        let w = h.shown().into_iter().find(|w| w.0 == id).unwrap();
        (w.1, w.2, w.3, w.4)
    };
    let home = (at(&h, "notes"), at(&h, "wifi"));
    let preview = |h: &mut Harness, w: i32, hh: i32| h.preview_resize(clock, 0, 0, w, hh);
    if !preview(&mut h, 6, 2) {
        issues.add("resize: growing the clock by two cells found no room");
    }
    if at(&h, "notes") == home.0 {
        issues.add("resize: growing the clock into the notes didn't push them aside");
    }
    // Grow further (down too), then shrink step by step: each size is planned from where the
    // widgets were, so the ones pushed out come back as room appears.
    preview(&mut h, 8, 4);
    for (w, hh) in [(7, 3), (6, 2), (5, 2), (4, 2)] {
        preview(&mut h, w, hh);
    }
    if (at(&h, "notes"), at(&h, "wifi")) != home {
        issues.add(format!(
            "resize: after shrinking back, the notes are at {:?} and wifi at {:?} instead of {:?}",
            at(&h, "notes"),
            at(&h, "wifi"),
            home
        ));
    }
    h.end_preview();
    if (at(&h, "notes"), at(&h, "wifi")) != home {
        issues.add("resize: ending the resize without a drop moved something");
    }
    issues.check(&h, "resize: grow and shrink in one resize");
}

// ------------------------------------------------------------------------------------------
// What the drawer does to the grid.

/// With the drawer open the cells shrink so that the grid and the drawer fit side by side; the
/// number of cells stays, so nothing is hidden; closing gives the room back.
fn drawer_makes_room(issues: &mut Issues) {
    let mut h = dashboard(MIXED);
    let big = h.cell();
    let (cols, rows, _) = h.grid();
    h.set_editing(true);
    let small = h.cell();
    if small >= big {
        issues.add(format!("drawer: the cells didn't shrink for it ({big} to {small})"));
    }
    if h.grid() != (cols, rows, true) && (h.grid().0, h.grid().1) != (cols, rows) {
        issues.add(format!("drawer: opening it changed the grid to {:?}", h.grid()));
    }
    let right = h.cell_origin(cols as f32, 0.0).0;
    let (width, _) = h.size();
    if right > width as f32 - (340.0 + 16.0) + 1.0 {
        issues.add(format!(
            "drawer: the grid reaches x={right}, into the drawer that starts at {}",
            width as f32 - 356.0
        ));
    }
    if h.tiles().iter().any(|t| !t.visible) {
        issues.add("drawer: opening it hid widgets");
    }
    issues.check(&h, "drawer: open");
    h.set_editing(false);
    if (h.cell() - big).abs() > 0.01 {
        issues.add(format!("drawer: closing it left the cells at {} instead of {big}", h.cell()));
    }
    issues.check(&h, "drawer: closed");
}

// ------------------------------------------------------------------------------------------
// Carrying, expanding, and the card stepping aside.

/// A widget picked up with a double click follows the pointer; Escape (or a right click) puts it
/// back, a second Escape lets go of it, and a click drops it where it is.
fn carrying(issues: &mut Issues) {
    let mut h = dashboard(MIXED);
    let cal = id_of(&h, "calendar");
    let a0 = tile(&h, cal).unwrap();
    let on = h.cell_origin(a0.col as f32 + 2.0, a0.row as f32 + 3.0);
    let carried = |h: &Harness| h.read(|d| d.edit.dragging());
    h.double_click(on.0, on.1);
    if !carried(&h) || h.adjust_id() != cal {
        issues.add("carry: a double click didn't pick the calendar up");
    }
    if !h.tips().iter().any(|s| s.contains("place it")) {
        issues.add(format!("carry: the quick tips say {:?}", h.tips()));
    }
    // Away over the free area: it is not where it was until it is dropped.
    let free = h.cell_origin(14.0, 7.0);
    h.move_to(free.0, free.1);
    if tile(&h, cal).unwrap().col != a0.col {
        issues.add("carry: the widget moved before it was dropped");
    }
    h.key("esc");
    if carried(&h) || h.adjust_id() != cal {
        issues.add("carry: Escape should end the carrying and keep the widget open");
    }
    if tile(&h, cal).unwrap().col != a0.col || tile(&h, cal).unwrap().row != a0.row {
        issues.add("carry: Escape moved the widget");
    }
    h.key("esc");
    if h.adjust_id() != 0 {
        issues.add("carry: the second Escape didn't let go of the widget");
    }
    // A right click cancels too.
    h.double_click(on.0, on.1);
    h.move_to(free.0, free.1);
    h.click_with(free.0, free.1, MouseButton::Right);
    if carried(&h) || tile(&h, cal).unwrap().col != a0.col {
        issues.add("carry: a right click should put the widget back");
    }
    h.key("esc");
    // A click drops it.
    h.double_click(on.0, on.1);
    h.move_to(free.0, free.1);
    h.click(free.0, free.1);
    let a1 = tile(&h, cal).unwrap();
    if carried(&h) || (a1.col, a1.row) == (a0.col, a0.row) {
        issues.add(format!(
            "carry: a click should drop the widget; it is at {},{} ({})",
            a1.col,
            a1.row,
            h.toast_text()
        ));
    }
    issues.check(&h, "carry: dropped");
    // A pinned widget isn't picked up.
    h.key("esc");
    h.toggle_pin(cal);
    let at = h.cell_origin(a1.col as f32 + 2.0, a1.row as f32 + 3.0);
    h.double_click(at.0, at.1);
    if carried(&h) {
        issues.add("carry: a pinned widget was picked up");
    }
}

/// `e` in an entered widget blows it up in the middle of the grid; Escape (or `e`) puts it back.
fn expanding(issues: &mut Issues) {
    let mut h = dashboard(MIXED);
    let name = format!("widget {}", testing::title("clock"));
    let before = h.bounds_of_named(&name).expect("the clock on screen");
    h.key("f");
    h.type_label("clock");
    h.key("e");
    let big = h.bounds_of_named(&name).expect("the clock on screen");
    if big.size.width <= before.size.width || big.size.height <= before.size.height {
        issues.add(format!("expand: the clock is {:?} blown up, {:?} before", big.size, before.size));
    }
    let (w, hgt) = h.size();
    let (cx, cy) = (f32::from(big.center().x), f32::from(big.center().y));
    if (cx - w as f32 / 2.0).abs() > w as f32 * 0.1 || (cy - hgt as f32 / 2.0).abs() > hgt as f32 * 0.15 {
        issues.add(format!("expand: the blown-up clock is centered at {cx},{cy} in a {w}x{hgt} window"));
    }
    h.key("esc");
    let back = h.bounds_of_named(&name).expect("the clock on screen");
    if back != before {
        issues.add(format!("expand: Escape left the clock {back:?}, it was {before:?}"));
    }
    if h.nav_mode() != NavMode::Widget {
        issues.add("expand: Escape should only put the widget back, not leave it");
    }
    h.key("e");
    h.key("e");
    if h.bounds_of_named(&name) != Some(before) {
        issues.add("expand: e twice should end where it began");
    }
    issues.check(&h, "expand");
}

/// The settings card steps aside while the keys are on the widgets or the hint labels are up.
fn card_steps_aside(issues: &mut Issues) {
    let mut h = dashboard("layout {\n    clock col=0 row=0 width=3 height=2\n}\n");
    h.key("ctrl+e");
    // `s` picks the widget, Enter puts the cursor on its settings.
    h.key("s");
    h.key("enter");
    if !h.read(|d| d.card_up()) {
        issues.add("card: Enter on a picked widget should show its settings");
    }
    h.key("f");
    if h.read(|d| d.card_up()) {
        issues.add("card: the hint labels should hide the settings card");
    }
    h.key("esc");
    if !h.read(|d| d.card_up()) {
        issues.add("card: Escape should take the labels down and show the card again");
    }
    // The keys on the widgets: the card keeps out of the way.
    h.key("w");
    if h.read(|d| d.card_up()) {
        issues.add("card: with the keys on the widgets the card should be down");
    }
    h.key("tab");
    if !h.read(|d| d.card_up()) {
        issues.add("card: Tab should bring it back");
    }
    issues.check(&h, "card: stepping aside");
}

/// With animations on, the drawer slides in and the grid shrinks with it, over time; frames glide
/// to their new places; nothing is left half way once the time is up, and none of it breaks the
/// layout.
fn gliding(issues: &mut Issues) {
    let mut h = dashboard(MIXED);
    h.animate(true);
    let big = h.cell();
    h.set_editing(true);
    // The slide takes a while: the cells are not yet at their smallest.
    let during = h.cell();
    if during < big * 0.5 || during > big {
        issues.add(format!("glide: the cells are {during} at the start of the slide, {big} before"));
    }
    std::thread::sleep(std::time::Duration::from_millis(700));
    h.settle();
    h.settle();
    let small = h.cell();
    if small >= big * 0.97 {
        issues.add(format!("glide: the cells are {small} after the drawer slid in, {big} before"));
    }
    // A widget moved glides to its place: a moment later it is there.
    let clock = id_of(&h, "clock");
    h.move_widget(clock, 5, 6);
    std::thread::sleep(std::time::Duration::from_millis(700));
    h.settle();
    h.settle();
    let t = tile(&h, clock).unwrap();
    if (t.col, t.row) != (5, 6) {
        issues.add(format!("glide: the clock is at {},{} after moving to 5,6", t.col, t.row));
    }
    h.set_editing(false);
    std::thread::sleep(std::time::Duration::from_millis(700));
    h.settle();
    h.settle();
    if (h.cell() - big).abs() > 0.5 {
        issues.add(format!("glide: the cells are {} after the drawer slid out, {big} before", h.cell()));
    }
    issues.check(&h, "glide");
}

// ------------------------------------------------------------------------------------------

type Scenario = (&'static str, fn(&mut Issues));

fn main() {
    let _sandbox = Sandbox::enter(None).expect("a temporary directory for the dashboard's files");
    // SAFETY: nothing else runs yet that could read the environment.
    unsafe { std::env::set_var("LC_ALL", "C") };
    let scenarios: &[Scenario] = &[
        ("settings: every setting of every widget", settings_every_widget),
        ("settings: text that makes no sense", settings_bad_text),
        ("cow: the cow file's complaints", cow_toasts),
        ("edits: random on a roomy layout", |i| random_edits(i, MIXED, "mixed")),
        ("edits: random on a packed layout", |i| random_edits(i, PACKED, "packed")),
        ("pointer: edit mode as a person uses it", pointer_editing),
        ("pointer: drags land where they are let go", pointer_drag_precision),
        ("pointer: every edge and corner resizes", pointer_resize_handles),
        ("pointer: a long press adjusts the widget", pointer_long_press),
        ("pointer: the palette's entries", pointer_palette),
        ("keyboard: move and resize modes", keyboard_modes),
        ("keyboard: edit mode", keyboard_editing),
        ("hints: the quick tips along the bottom", shortcut_hints),
        ("grid: shrink to the smallest and back", grid_shrink_and_grow),
        ("sidebar: follows its widget", sidebar_follows),
        ("arrange: algorithms, undo, copy", arrange_tab),
        ("pin: widgets that stay put", pinning),
        ("card: s opens it, Escape closes it", settings_card_keys),
        ("resize: pushed widgets come back when it shrinks", resize_gives_room_back),
        ("drawer: the cells shrink to make room", drawer_makes_room),
        ("carry: a widget picked up with a double click", carrying),
        ("expand: a widget blown up in the middle", expanding),
        ("card: it steps aside for the keys and the labels", card_steps_aside),
        ("glide: animations on", gliding),
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

#[allow(dead_code)]
fn unused(_: &SettingRow) {}

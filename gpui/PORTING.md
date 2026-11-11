# How niri-dash is put together

The dashboard is written on [GPUI](https://gpui.rs), Zed's UI framework (the `gpui-pre` snapshot
and the `gpui-kit` component library on top of it). It was first written on Slint; that version is
the root crate of this repository and stays, for comparison, and this one is a port of it: the
same widgets, the same keys, the same look. This file describes the GPUI version's architecture and
the rules new code follows, then how to compare it with the Slint version. (The file's name is from
the time of the port.) Paths below are inside `gpui/` unless they say otherwise.

What does not touch the window is shared or unchanged from the Slint version: `crates/system` (the
system actors, their `Command` and `Event` enums, the live, simulated and mock systems) is one crate
both build on; `layout`, `autolayout`, `nav`, `theme`, `widget_settings`, `mascots`, `cowfile`,
`pomodoro`, `timer`, `state`, `zones`, `files` are the same code in both, with a copy each.

## The parts

```
crates/system   the machine: actors, Command/Event, live / simulated / mock
src/dash.rs     Dash, the state; keys arrive in Dash::key
src/window.rs   the layer-shell window, the event loop, the control socket, the tick
src/ui.rs       impl Render for Dash: backdrop, top bar, frames, hints, toast
src/edit/       edit mode: frames' handles, drawer, settings card, drags, keys
src/widgets/    the widgets, in groups, behind one registry (mod.rs)
src/kit.rs      the look as GPUI colors (Tk), icons, buttons, switch, bar
src/scope.rs    the keyboard cursor of an entered widget
src/keys.rs     Key: GPUI keystrokes, the stress run's names and tests, one type
src/look.rs     applying and changing the theme; src/config.rs saving, reloading, watching
src/testing.rs  the headless harness; stress.rs, snapshot.rs the development commands on it
```

### `Dash`, the state

`src/dash.rs`: **`Dash`** is a GPUI entity: the layout, the look, the nav state (`Nav`: Off, Hints
or Widget), edit mode, the tiles (placed widgets with their hint labels), the toast, and `Widgets`
(the state of each kind of widget). It outlives the window. Views only read it when they are
drawn, and every change to it ends in `cx.notify()` (the entry points, `key`, `handle_event`,
`tick` and the listeners, do it).

### The window

`src/window.rs` opens a layer-shell window (an ordinary one for `niri-dash app <widget>`) when the
dashboard is shown and closes it when it is hidden: GPUI windows can't be hidden, and a closed one
costs nothing. The `Dash` entity stays. The same module pumps what feeds it into the entity: the
system's events (batched, so a burst is one redraw), the control socket (`Input::Control`), config
changes (a watcher on config.kdl and the theme files, `config::watch_config`), `niri-dash say` and
`night-light`, and a once-a-second `tick` while it is up. `NIRI_DASH_MOCK=1` and `NIRI_DASH_SIM=1`
start the mock and the simulated system in place of the real one.

### Drawing

`src/ui.rs` is `impl Render for Dash`: the backdrop, the top bar, a frame per tile at its grid
position, the hint labels, the toast. A frame calls `widgets::render`. Edit mode's parts (the
frames' handles, the sidebar, the settings card, the top bar) are drawn by `src/edit/view/`.

### The widgets

`src/widgets/mod.rs` is the registry. Widgets are in **groups**, one module each (`time`,
`monitors`, `controls`, `net`, `tools`, `desk`, `devices`, `extras`), all with the same hooks:
`State`, `handles`, `render`, `event`, `key`, `escape`, `tick`, `init`, `come_up`, `go_down`,
`awake`, `entered`, `layout_changed`, `describe`, `check` (`time.rs` and `time/clock.rs` are the
model). `mod.rs` calls them in turn, so a group is added or changed without touching the others.
Inside a group each widget is a submodule `widgets/<group>/<kind>.rs` with:

- its data, as plain structs in the group's `State`;
- `render(&Ctx, cx) -> AnyElement`. `Ctx` has the card's size in pixels (`w`, `h`: widgets adapt to
  it, as they adapt to their size in the Slint version), `round`, `entered`, `opts` (the widget's
  settings) and the look (`tk`);
- event handling (`d: &mut Dash, &sys::Event`) and the keys of an entered widget, through a
  `scope::Scope`;
- invariants for the stress run (`check`, `describe`).

`src/scope.rs` is the keyboard cursor of an entered widget (the port of Slint's `NavScope`): the
keys j k h l g G Tab Enter Space Backspace are the scope's, and a widget gives its stops meaning
with `impl Stops`. It is `Copy`: take it out of the state, press, put it back, so the handler can
borrow the rest of the state.

`src/kit.rs` is the look as GPUI wants it (`Tk`), the icons (`assets/icons/<name>.svg`) and the
small shared pieces: buttons, switch, bar, header. Anything bigger that one widget needs goes in
that widget's module first, and moves to `kit` when a second one uses it.

### The keyboard

Keys arrive in `Dash::key` as a `keys::Key` and go to the level that has the keyboard: the
dashboard (`f` for the hint labels, Ctrl+h/j/k/l between widgets, Ctrl+E, ...), an entered
widget's scope, or edit mode (`src/edit/keys.rs`). The model, in the order a user meets it, is in
the manual: [The keyboard model](../book/src/explanation/keyboard-model.md) and the
[key reference](../book/src/reference/keyboard.md), which is the specification.

### Look and config

`src/theme.rs` resolves a color scheme and a style into a `Look`; `src/look.rs` applies it (blending
from one to the next) and holds what edit mode does to it. `src/config.rs` saves what edit mode
changed into config.kdl (in place, comments kept) and lays out a file that changed on disk.
`src/layout.rs` is the config's model and the facts about every widget kind (`Kind::info`), from
which the manual's widget tables are written (`src/book.rs`).

## Rules

1. **State is plain data in `Dash`.** Views read it when drawn and never keep a copy.
2. **Everything a widget can do from the mouse it can do from the keyboard, and vice versa.**
3. **Every clickable thing has `.id(..)`, `.role(..)`, `.aria_label(..)` and a
   `.debug_selector(..)` named as a screen reader would say it** (`kit::icon_button` does). The
   tests find elements by that name (`Harness::click_on("Rescan")`).
4. **Look sizes up in `tk`** (`tk.pad()`, `tk.radius()`, `tk.font_body()` ...), never hard-code
   what the style sets. Animations take `tk.motion()` (0 = none); a headless run has none.
5. **Keep the edge cases**: empty states, "not available on ..." lines, what a widget does at its
   smallest size. When the Slint version does something, the GPUI one does it.
6. **Text input** is `gpui_kit::component::input` (`InputState`, `Input`, `InputEvent`); keep its
   state in the group's `State`, and make Escape and Enter behave as `TextField` does in
   `slint/ui/common.slint`.
7. **Nothing in a test may touch the real system:** use `Harness::with_mock` or `Harness::new` and
   a `Sandbox`.

## Testing

`cargo gtest` (`cargo test -p niri-dash-gpui --profile gpui --features testing`) runs everything
that needs no real system. The feature turns on
the headless harness, `src/testing.rs`: the real `Dash` in a window of GPUI's test platform (no
compositor, no GPU), with channels standing in for the system. A test hands it `Event`s, presses
keys and clicks through the window (`Harness::key`, `click`, `click_on`), and reads the `Dash`.

- `tests/*.rs` are plain programs (`harness = false`, `required-features = ["testing"]`), one per
  group of widgets plus `edit`, `stress` and `snapshot`. `NIRI_DASH_STRESS_OPS=800` sets the
  stress run's key count. `Harness::new` doesn't show the dashboard (only `with_mock*` do): call
  `h.control(Control::Show)` first when you build on `new`.
- Logic that is pure gets `#[cfg(test)]` tests next to it. `src/book.rs` has the test that the
  manual's widget tables are current (`NIRI_DASH_BOOK=bless cargo test --features testing --lib
  book` rewrites them).
- The stress run's `widgets::check` and `widgets::describe` hooks are where a widget says what must
  hold (cursor on a stop that exists, ...). `niri-dash stress --headless` (a build with the
  feature) is the same run from the command line; without `--headless` the same monkey presses its
  keys into a real window at `--rate` a second (`src/stress/live.rs`), through the `Rig` trait that
  the harness and the window both implement.
- The tests that run system code (the real system, screen readers, KWin) run only in the test box:
  `cargo testbox test` (see `testbox/README.md`).

## Pictures

- `niri-dash snapshot` (a build with the feature, `src/snapshot.rs`) draws the dashboard into a PNG
  without a display: GPUI's test platform with its wgpu renderer on a texture. It takes the Slint
  variant's options (`--mock`, `--width`, `--edit`, `--tab`, `--events`, ...). The test platform's
  windows have a scale factor of 2: the frame is drawn at twice the size and resized.
- `book/screenshots.sh [BIN [OUTDIR]]` makes the manual's pictures with it, from either variant's
  binary.
- `scripts/vshot.sh BIN OUTDIR STEP...` runs a binary (either variant) on a private virtual KWin
  on the mock system and takes screenshots of the real screen. It never touches the desktop session.
  Run both variants on the same config and look at the two PNGs: this is how the look is compared.

## Comparing with the Slint version

The Slint version is the root crate of the workspace (`../Cargo.toml`, the program `niri-dash`);
it shares `crates/system`, `assets/` and `book/` with this crate. Treat it as the specification of
behaviour. In the table, paths on the left are from the repository root, those on the right from
`gpui/`.

| Slint (the root crate) | GPUI (here) |
|---|---|
| `ui/*.slint`, `ui/widgets/*.slint` (the views) | `src/ui.rs`, `src/kit.rs`, `src/widgets/*.rs`, `src/edit/view/` |
| `ui/data.slint` (the data each widget reads: globals and structs) | each widget's own `State` in `src/widgets/<group>/` |
| `src/app/<name>.rs` (pushes system events into Slint models; callbacks) | the same module's `event`, `key` and click handlers, as `impl Dash` or free functions on `&mut Dash` |
| `ui/common.slint` (`NavScope`, `TextField`, `IconButton`, ...) | `src/scope.rs`, `src/kit.rs`, `gpui_kit::component::input` |
| `ui/app.slint`, `ui/frame.slint` (edit mode) | `src/edit/` |
| `src/app/stress.rs`, `testing.rs`, `snapshot.rs` | `src/stress.rs`, `src/testing.rs`, `src/snapshot.rs` |
| `tests/*.rs` | `tests/*.rs`, on `Harness` |
| `src/shell.rs`, `gpu.rs`, `app/wayland.rs` (layer shell, renderers, event loop) | GPUI's own; `src/window.rs` |
| `src/app/a11y/`, AccessKit over AT-SPI | GPUI's accessibility tree (see `docs/accessibility.md`; `../docs/accesskit.md` is the Slint one) |
| `web/`, `Trunk.toml`, the wasm cfgs in `crates/system` | no equivalent: GPUI has no browser build |

Differences on purpose:

- One renderer (GPUI's, on the GPU); the `renderer` setting in config.kdl is read and ignored, and
  `NIRI_DASH_RENDERER` is gone.
- No browser build: the web version, Trunk and the Pages workflow's web part are the Slint
  version's.
- Hiding closes the window at once (no exit animation).
- `snapshot` and `stress` need the `testing` feature; `snapshot` has no `--incremental`, `--keep`
  or `--bench`, which were about Slint's software renderer.

Building and running the Slint version: `cargo run` and `cargo test` at the repository root (no
`-p`); the GPUI version is `-p niri-dash-gpui`. CI builds and tests both.

## Commands

```sh
cargo gcheck                                # check, with the tests
cargo gtest                                 # test
cargo clippy -p niri-dash-gpui --profile gpui --all-targets --features testing
cargo fmt --all
NIRI_DASH_MOCK=1 cargo run -p niri-dash-gpui --profile gpui   # the real window on made-up data (not on
                                                              # the session you work in: use scripts/vshot.sh)
cargo gbuild --features testing && book/screenshots.sh target/gpui/debug/niri-dash-gpui /tmp/pictures
```

# Porting niri-dash from Slint to GPUI

The dashboard is being rewritten on [GPUI](https://gpui.rs) (Zed's UI framework, the
`gpui-pre` snapshot that `../beep/b33p-gpui` uses). What does not touch the window is kept as it
was: `crates/system` (the system actors, their `Command`/`Event` enums, the mock), `layout`,
`autolayout`, `nav`, `theme` (minus its Slint bridge), `widget_settings`, `mascots`, `cowfile`,
`pomodoro`, `timer`, `state`, `files`, `zones`, `testbox/`, `xtask/`.

The **Slint code is in `legacy/`** (not built; for reading, deleted when the port is done):

| Legacy | What it was | Now |
|---|---|---|
| `legacy/ui/*.slint`, `ui/widgets/*.slint` | the views | `src/ui.rs`, `src/kit.rs`, `src/widgets/*.rs` |
| `legacy/ui/data.slint` | the data each widget reads (globals + structs) | each widget's own `State` in `src/widgets/<name>.rs` |
| `legacy/src/app/<name>.rs` | pushes system events into Slint models; callbacks | the same module's `event`/`key`/click handlers, as `impl Dash` or free functions on `&mut Dash` |
| `legacy/src/app/stress.rs`, `testing.rs` | monkey, harness | `src/stress.rs`, `src/testing.rs` (done) |
| `legacy/tests/*.rs` | UI tests | `tests/*.rs`, on `Harness` |
| `legacy/src/shell.rs`, `gpu.rs`, `app/wayland.rs` | layer shell, renderers, event loop | GPUI's own; `src/window.rs` |

## How it is put together

- `src/dash.rs`: **`Dash`**, a GPUI entity: layout, look, nav state (`Nav`: Off / Hints / Widget),
  edit mode, tiles (placed widgets with hint labels), toast, and `Widgets` (per-kind state). Keys
  arrive in `Dash::key`. It outlives the window; `src/window.rs` opens a layer-shell window when
  it is shown and closes it when hidden (GPUI windows can't be hidden), and pumps the system's
  events, the control socket and a once-a-second `tick` into it.
- `src/ui.rs`: `impl Render for Dash`: backdrop, top bar, a frame per tile at its grid position,
  hint labels, toast. A frame calls `widgets::render`.
- `src/widgets/mod.rs`: the registry. Widgets are in **groups**, one module each (`time`,
  `monitors`, `controls`, `net`, `tools`, `desk`, `devices`, `extras`), all with the same hooks
  (`State`, `handles`, `render`, `event`, `key`, `escape`, `tick`, `init`, `come_up`, `go_down`,
  `awake`, `entered`, `layout_changed`, `describe`, `check`; `time.rs` + `time/clock.rs` is the
  model). `mod.rs` calls them in turn, so **a group is yours alone: you never edit `mod.rs`** (nor
  another group's files). Put each widget in a submodule `widgets/<group>/<kind>.rs`. Inside a
  group, a widget has its data (what `legacy/ui/data.slint` held for it, as plain structs in its
  `State`), `render(&Ctx, cx) -> AnyElement`, event handling (`d: &mut Dash, &sys::Event`), keys
  of an entered widget (through a `scope::Scope`), and invariants for the stress run. `Ctx` has the card's size in pixels (`w`, `h`; widgets adapt to it as the Slint ones did to
  `root.width`), `round`, `entered`, `opts` (the widget's settings) and the look (`tk`).
- `src/scope.rs`: the keyboard cursor of an entered widget; port of Slint's `NavScope`
  (`legacy/ui/common.slint`). j/k/h/l/g/G/Tab/Enter/Space/Backspace are the scope's; a widget
  gives its stops meaning with `impl Stops`. It is `Copy`: take it out of the state, press,
  put it back, so the handler can borrow the rest of the state.
- `src/kit.rs`: the look as GPUI colors (`Tk`), icons (`assets/icons/<name>.svg`), buttons,
  switch, bar, header. Ports of `legacy/ui/common.slint`'s components; add what you need
  (rings, graphs, sliders, scroll lists) **in your own module first**, move to `kit` when two
  widgets use one.
- `src/keys.rs`: `Key`, one type for GPUI keystrokes, the stress run's names and tests.
- `src/look.rs`, `src/config.rs`: applying and changing the theme; saving, reloading and
  watching config.kdl.

## Rules

1. **State is plain data in `Dash`.** Views read it when drawn and never keep a copy. A change
   ends in `cx.notify()` (the entry points — `key`, `handle_event`, `tick`, listeners — do it).
2. **Everything a widget can do from the mouse it can do from the keyboard, and vice versa**, as
   in the Slint widgets. Port the Slint widget's keys exactly (`NavScope` callbacks `adjust`,
   `activate`, `letter`, `back`, `skip`, `filter`, `copy`).
3. **Every clickable thing has `.id(..)`, `.role(..)`, `.aria_label(..)` and a
   `.debug_selector(..)` named as a screen reader would say it** (`kit::icon_button` does; the
   tests find elements by that name, `Harness::click_on("Rescan")`).
4. **Look sizes up in `tk`** (`tk.pad()`, `tk.radius()`, `tk.font_body()` …), never hard-code
   what the style sets. Animations take `tk.motion()` (0 = none); a headless run has none.
5. **Port the Slint behaviour, not just the look**: read the `.slint` widget and its
   `legacy/src/app/<x>.rs` before writing, and keep their edge cases (empty states, "not
   available on …" lines, what the widget does at its smallest size).
6. **Tests.** Port `legacy/tests/ui.rs`/`edit.rs` scenarios for your widgets onto `Harness`
   (`tests/<name>.rs`, plain `fn main` programs like `tests/stress.rs`, or `#[cfg(test)]` unit
   tests next to the code when the logic is pure). The stress run's `widgets::check` /
   `widgets::describe` hooks are where a widget says what must hold (cursor on a stop that
   exists, …).
7. Nothing in a test may touch the real system: use `Harness::with_mock` / `Harness::new` and a
   `Sandbox` (see `tests/stress.rs`).
8. **Commit in your own branch/worktree; don't edit other groups' modules.** The only shared
   files you may touch are `src/kit.rs` (additions only: new functions at the end; put anything
   bigger in `src/widgets/<group>/` first) and `Cargo.toml` (new dependencies, justified). A
   widget that needs something of `Dash` itself (a new field, a method) adds it in your group's
   `State` or as `impl Dash { .. }` in your group's files, not in `dash.rs`.
   The exceptions, each a one-line edit in `src/widgets/mod.rs` by the group named: `controls`
   fills `night_light_command` (to call its own), `extras` `mascot_say`, `desk` `overlays` (the
   tray's menu is drawn over the whole window) and `toggle_dnd` (already routed to `desk`). If
   your widget needs a new hook that every group could have, say so in your report instead.
9. **Text input**: `gpui_kit::component::input` (`InputState`, `Input`, `InputEvent`) is the text
   field; keep its state in your group's `State`, and make Escape/Enter behave as the Slint
   `TextField` did (`legacy/ui/common.slint`).
10. **Tests** that need a window go in `tests/<group>.rs` (a `fn main` program like
    `tests/stress.rs`, `[[test]] harness = false, required-features = ["testing"]` in Cargo.toml)
    or `#[cfg(test)]` unit tests next to the code.

## Commands

```sh
cargo check --all-targets --features testing --offline
cargo test --features testing
NIRI_DASH_MOCK=1 cargo run                  # the real window on made-up data (never on the desktop session you work in)
```

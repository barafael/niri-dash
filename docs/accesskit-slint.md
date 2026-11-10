# Screen reader support in the Slint version (AccessKit / AT-SPI)

> This is the note of the Slint version, kept for comparison (`slint/`: its own tree builder in
> `slint/src/app/a11y/`, its tests in `slint/tests/`). The GPUI version, which is the dashboard
> now, is in [accessibility.md](accessibility.md): GPUI builds the tree itself, so most of what is
> below (the tree builder, the bridge, the recording sink) has no counterpart there. What the
> findings say about AT-SPI and Orca (what libatspi calls a switch, what Orca says, how to run
> them without a display) still holds.

Status after the first spike, on branch `accesskit`. It supersedes `accesskit-handover.md` (which
was written before any of this existed) where they differ; what still stands there is noted below.

## What works

The dashboard publishes an accessibility tree on the AT-SPI bus, and a screen reader can read it
and act on it. Checked against libatspi (the library Orca is built on), not only against our own
consumer of the tree.

| Piece | Where |
|---|---|
| The tree: `ItemRc` walk to an AccessKit `TreeUpdate`, stable node ids | `src/app/a11y/tree.rs` |
| The cursor as one function, used for what is spoken and for the tree's focus | `App::cursor` in `src/app/a11y.rs` |
| Requests from the bus, handed to the UI thread; throttled, deduplicated updates | `src/app/a11y/bridge.rs` |
| The daemon and the apps publish (never a stress run, never a test) | `wayland.rs`, `serve()` |
| Kill switch | `NIRI_DASH_ATSPI=0` |

Costs while no screen reader is there: one thread that waits for `org.a11y.Status` `IsEnabled` on
the session bus. Nothing is built until it turns true.

## How the handover's open points came out

- **Which `accesskit_unix` pairs with `accesskit` 0.24?** `accesskit` 0.24.1 with `accesskit_unix`
  0.22.1 (0.21.1 is the one for 0.24.0), in `tokio` mode: it makes its own single-thread runtime on
  its own thread, so it doesn't need ours. `accesskit_consumer` 0.38 reads the tree back in tests.
  The version is the one `i-slint-core`'s `accessibility-text` uses, for when text fields need it.
  All of it resolves offline; 15 crates are new, zbus is shared.
- **Will Orca read a layer-shell overlay?** Orca reads the dashboard: `tests/orca.rs` runs Orca 51
  against it and reads Orca's log of what it spoke (no speaker needed). What that test does not
  cover is the real compositor: the dashboard's surface there is a layer-shell one with exclusive
  keyboard interactivity, which Orca never sees (it only follows AT-SPI focus). A manual check on
  the real session is below.
- **Hooking into Slint (`WindowAdapterInternal`).** Not needed. The tree is read from the
  `slint::Window`, and the loop looks once a turn (`access_sync`), so the GPU `GpuWindow` and
  Slint's `MinimalSoftwareWindow` are treated alike and neither is touched.
- **`accessible-item-selected` on the cursor.** Not announced as "selected": the tree says
  `selected` only for elements that say `accessible-item-selectable` (none do). The marker is read
  as "the cursor is here" and becomes the tree's focus. `accessible-delegate-focus` isn't used.
- **Will anyone use it?** The condition the handover says step 2 waited on is the request to build it.

## Findings worth keeping

- Our sliders say their value as text (`"70%"`). A node needs the number too, or AT-SPI offers no
  value interface: sliders now carry `numeric_value` 70 and a default range of 0 to 100.
- libatspi calls AccessKit's `Switch` a `toggle button`, and "on" is its `pressed` state.
- `Action::Focus` enters the widget that holds the element, as typing its letters does
  (`Nav.active-id`, mode `widget`). The cursor then stands where that widget keeps it, usually its
  first stop; Slint's own focus can't be set on elements that are not focus scopes.
- **What Orca says** (Orca 51, English): entering Wi-Fi gives "Wi-Fi List with 4 items", then
  "Lighthouse" and its Disconnect and Details buttons, then the row's details "connected, signal
  78%, secured"; `j` reads the next row the same way; the brightness knob is "Brightness slider
  70 70 percent." (Orca's own value-then-percentage form). AT-SPI has no value for a row, so a
  row's text value is also its description, which is what Orca reads after the name.
- With both the number and the text "70%" on a slider, Orca said it twice more than it needed to;
  sliders carry the number only.
- Orca cannot start without a display (it crashes setting up its keyboard), so its test runs a
  virtual `kwin_wayland --virtual` in the same private session. Its service name is
  `org.gnome.Orca1.Service`; its settings are kept in memory (`GSETTINGS_BACKEND=memory`).
- dbus-broker, where it is the default, makes `at-spi-bus-launcher` need a systemd user manager. A
  private test session sets `ATSPI_DBUS_IMPLEMENTATION=dbus-daemon`.

## Tests

| Command | Checks |
|---|---|
| `cargo test --test a11y` | the existing audit, now also building the tree at every keyboard stop: it builds in `accesskit_consumer`, is reachable from the root, has unique ids, names everything that can be acted on, has the focus where `spoken()` says the keyboard is, keeps ids between builds. A control proves the check can fail. |
| `cargo test --test access` | the bridge, with a recording sink instead of a bus: nothing sent before a screen reader asks; whole tree first; focus follows the keyboard one move per step; quiet when nothing changes; actions (click, increment, focus); changes arrive as node updates; leaving and coming back. |
| `cargo test --test orca` | Orca itself, on a private bus and a virtual KWin, reading the dashboard while the keyboard is walked into Wi-Fi and brightness: it says each stop's name and details, the list and its length, the slider as a slider. The transcript is printed. Skips if Orca or KWin isn't installed. |
| `cargo test --test atspi` | the real bus: starts itself under `dbus-run-session` with a temporary runtime directory and its own accessibility bus, and runs `tests/atspi_client.py` (libatspi). Skips, and passes, if `dbus-run-session`, `gdbus`, python3 with the `Atspi` bindings or the bus launcher are missing. |

`tests/bus/mod.rs` is what `atspi` and `orca` share: a private session bus whose own files
(`HOME`, `XDG_*`) and runtime directory are temporary, so neither the session's buses nor its
settings are touched.

Tests keep off the session's bus: the only real sink is made by the daemon's loop, and the harness's
`access_connect_unix` refuses without `NIRI_DASH_PRIVATE_BUS`, which only `tests/atspi.rs` sets.

## Manual check with a real screen reader

The tests cannot stand in for the real compositor, so once, by hand:

```
orca &                              # or switch the desktop's screen reader on
niri-dash daemon                    # then toggle the dashboard
# without Orca: look at the tree with libatspi
python3 tests/atspi_client.py niri-dash
```

Things to listen for (or to read in `orca --debug-file=orca.log`, as the test does): the dashboard is announced when it appears; `f`, a letter, then `j` and `k`
read the stops; the values of sliders; whether Orca follows the focus into a widget.

## Not done

- **Text fields:** exposed with their value and read-only state only. Caret, selection and text
  runs need `i-slint-core/accessibility-text`, which we don't enable.
- **A stop for each part:** `Focus` on the Wi-Fi switch enters the Wi-Fi widget at its first stop,
  not on the switch. Mapping an element to its stop needs the stop's index from the widget's
  `NavScope`.
- **Row noise:** Orca reads a row's Disconnect and Details buttons as part of the row each time.
- **Announcements:** toasts and other changes the user should hear aren't live regions yet.
- **From the handover, still open and unchecked:** Tab between text fields speaks the wrong field;
  the edit-mode UI is labelled but the "what is under the keyboard" check skips it; the Privacy
  list marker; the unconfirmed cursor oddities (Wi-Fi sub-part, Cancel/Join, a heading in Windows).
- **Cost:** while a screen reader is there, the whole tree (a few hundred to a thousand nodes) is
  rebuilt at most every 40 ms and sent only if it differs. Not measured on the real dashboard;
  a property tracker could replace it if it shows.
- **Web:** untouched; the code is `cfg`'d to the desktop.

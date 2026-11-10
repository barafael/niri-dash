# Screen reader support (GPUI, AccessKit, AT-SPI)

The dashboard is a screen reader's to read and drive. This is what GPUI does for it, what the
dashboard adds, how it is tested, and what is not there. The Slint version's note, which this
replaces for the GPUI dashboard, is [accesskit-slint.md](accesskit-slint.md); what it found about
AT-SPI and Orca still holds.

## What GPUI offers (`gpui-pre` 0.3.11)

GPUI builds the accessibility tree itself. There is nothing like the Slint version's tree builder
and bridge (`slint/src/app/a11y/`) to write.

- **The tree.** While assistive technology is connected, every frame `Window::draw` collects an
  AccessKit node for each element that has both an `.id(..)` and a `.role(..)`, with the
  properties set by `.aria_label`, `.aria_description`, `.aria_value`, `.aria_numeric_value`
  (and `_min_`, `_max_`, `_step`), `.aria_toggled`, `.aria_selected`, `.aria_expanded` and the
  rest of `StatefulInteractiveElement`'s `aria_*`, and hands the whole `TreeUpdate` to the
  platform window. Node ids are a hash of the element's path of ids, so they hold from frame to
  frame while the element keeps its place in the view. Elements without a role are not in the
  tree; plain text (`.child("...")`) is not either, only `text!(..)` is (a `Label` with its value)
  (`patches/gpui-pre` adds the plain texts, see below).
  Nothing is built while nobody asks: `Window::is_a11y_active`.
- **Actions.** `.on_click(..)` also answers an AccessKit `Click`; `.on_a11y_action(Action, ..)`
  answers any other (`Increment`, `Decrement`, `SetValue`, `Focus`, ...). A click with no listener
  becomes a mouse click at the node's centre.
- **Focus.** The tree's focus is the element with GPUI's keyboard focus that has a node. An element
  with `.aria_active_descendant()` inside it takes the focus over: the pattern for a composite
  widget that keeps the keys on one element and moves a cursor inside it, which is what the
  dashboard is.
- **The Linux platform publishes it.** `gpui-pre-linux` creates an `accesskit_unix::Adapter`
  (AccessKit 0.24, `accesskit_unix` 0.22, the pair the Slint version used) for each Wayland or X11
  window in `a11y_init`. The adapter watches `org.a11y.Status` `IsEnabled` on the session bus; when a
  screen reader turns it on, it connects to the accessibility bus, registers the application there
  (named like the program: `niri-dash`) and sends the tree. A layer-shell window is published like
  any other. `scripts/atspi-dump.sh target/debug/niri-dash niri-dash` prints what libatspi reads.
- **Not in GPUI.** No live regions (`aria_live`), no way to name a node's `Live`, `Busy` or
  `Modal`; text fields other than GPUI's own have no text runs unless an element builds them with
  `a11y_synthetic_children`.
- **The test platform has no screen reader.** `TestWindow` leaves `a11y_init` and
  `a11y_tree_update` empty, so no tree is ever built in a test (`Window::debug_a11y_tree_json`
  stays `None`), and the active flag that switches the tree on is private. See "How it is
  tested" for what the dashboard does about that.

## What the dashboard puts in the tree

The same names the Slint version had (checked against a libatspi dump of both, per widget):

- **The window** is the root, named `niri-dash`; the dashboard's root element (a `Group`) has the
  keyboard.
- **A widget** is a `Group` named as its title (`Wi-Fi`, `Battery & power`); the toolbar has
  `Edit dashboard` and `Close dashboard`.
- **Buttons, switches, sliders, rows, tabs** have a role and a name that says what they do
  (`Disconnect from Lighthouse`, not an icon); a switch says whether it is on (`Toggled`), a slider
  its value as a number with the range 0..100 and a step of 5, a gauge its value as a number.
- **A row says how it stands** (`connected, signal 78%, secured`) as its value and as its
  *description*: AT-SPI has a value only for numbers, and the description is what Orca reads after
  the name (`kit::AriaState`).
- **The keyboard cursor is the screen reader's focus.** The element under the cursor of the entered
  widget says so with `kit::AriaCursor` (`aria_cursor(kfocus)`, an active descendant), so the
  focus follows `j`, `k`, `h`, `l` and Orca reads each stop as the cursor lands on it. It does not
  say `selected`: a screen reader would announce that at every stop. A widget without a cursor of
  its own (the clock) is the focus itself while it is entered. Only one element may claim the focus
  at a time (a claim by something inside the claimant overrides it); GPUI panics in a debug build
  when two unrelated elements claim it, which the tests turn into a report.
- **A toast is an alert**, read out when it appears.

## How it is tested

GPUI's test window never connects a screen reader, so a test cannot see the tree. `patches/gpui-pre`
is the published `gpui-pre` 0.3.11 with the test window given what a platform adapter has
(`a11y_init`, `a11y_tree_update`; four small methods to switch it on, read the tree and send it
an action; see its `NIRI-DASH-PATCH.md`), used through `[patch.crates-io]` in the root
`Cargo.toml`. Delete both if upstream grows the same. One more change is in it: a claim of the
focus by an element inside the claimant's no longer counts as two claims.

`Harness` (`src/testing/access.rs`) gets the tree:

| Call | |
|---|---|
| `h.a11y()` | the tree GPUI builds now, as a `Tree` (connects a screen reader first) |
| `tree.find(Role::Switch, "Wi-Fi")`, `find_all`, `named`, `get`, `children`, `parent`, `below` | find an element by role and name |
| `element.on()`, `numeric`, `min`/`max`, `value`, `description`, `selected`, `expanded`, `disabled`, `actions` | its value and states |
| `element.says()` | what a screen reader says: `switch "Wi-Fi" (on)`, `slider "Brightness" 70%` |
| `h.cursor()`, `h.spoken()` | where the keyboard is in the entered widget: the element, or what it says; `Err` says what doesn't add up (cursor out of sight, on two things, on something with no name) |
| `tree.focused()`, `tree.cursors()` | the tree's focus, and what says it is under the cursor |
| `tree.problems()` | a node listed twice or owned by no one, a root that isn't the window, something to act on with no name |
| `tree.dump()`, `h.a11y_dump()` | the tree as indented text, the focus marked |
| `h.cursor_signature()` | what is under the cursor, as a line (changes when a key moves it) |
| `h.a11y_request(Action::Click, id)`, `h.a11y_do(action, role, name)` | what a screen reader asks of an element, delivered as the adapter delivers it |
| `h.a11y_disconnect()` | the screen reader leaves |

The tests:

| Command | Checks |
|---|---|
| `cargo test --features testing --test a11y` | every widget kind alone on the dashboard, in each of its shapes (normal, smallest, round), on the mock system's data, with the keyboard walked over all its stops and their parts: the tree is sound (`problems()`), every stop has a name, the cursor is on exactly one thing, in sight, and it is the tree's focus; ids stay between builds. A control proves the check can fail. `NIRI_DASH_A11Y_KIND=wifi` for one kind, `NIRI_DASH_A11Y_DUMP=1` to print each tree. |
| `cargo test --features testing --test access` | what a screen reader is given and can do: nothing built until one connects; the whole tree first; states and values; ids hold when values change; an unchanged dashboard builds the same tree; the focus follows the keyboard; actions (click, increment, decrement, focus); leaving and coming back. |
| `cargo testbox test atspi` | the real bus: the dashboard's own program on a virtual KWin and a private session bus, read by libatspi (`tests/atspi_client.py`): roles as libatspi names them, a switch's state, its action turning Wi-Fi off, a slider's value and range. |
| `cargo testbox test orca` | Orca itself reading the real dashboard, keys sent over the control socket; its transcript is printed. |

The last two need the test box (see `testbox/README.md`); outside it they print that they are
skipping and pass, as the Slint ones did.

## Looking at the real thing

```sh
scripts/atspi-dump.sh target/debug/niri-dash niri-dash                    # what libatspi reads
KIND=battery KEYS="f ba j" scripts/atspi-dump.sh target/debug/niri-dash niri-dash   # keys first
scripts/atspi-dump.sh slint/target/debug/niri-dash-slint niri-dash        # the Slint version, to compare
```

It starts the program on the mock system on a private virtual KWin and a private session bus
with its own accessibility bus (never the desktop's). `KEYS` go over the control socket, which
takes `key NAME` lines only when `NIRI_DASH_TEST_KEYS` is set (the script and the tests set it; the
daemon never does).

By hand with a real screen reader, once: `orca &`, `niri-dash daemon`, toggle the dashboard,
`f` and a letter, then `j` and `k`. Things to listen for: the dashboard is announced when it
appears; each stop is read as the cursor lands on it; sliders say their value; a toast is read.

## Differences from the Slint version

- **Static text** is in the tree because the patched GPUI puts each plain text drawn in a node
  under it as a `Label` leaf with the text as its value (a `text!` is one already), as Slint's
  text items were. Their ids are the parent's and the text's place among its texts, so they hold
  while the text changes. Two differences: a text GPUI draws as a styled run is not there, and
  what the Slint version labelled `accessible-role: none` is not left out.
- **No list around a widget's rows.** Slint wrapped each widget's stops in a `list box` named as
  the widget, which Orca announced ("Wi-Fi List with 4 items"). Here the widget is a group.
- **No `NIRI_DASH_ATSPI=0`.** Nothing is built while no screen reader has turned accessibility on
  (`Window::is_a11y_active`), which was what the switch was for.
- **The hidden dashboard isn't there.** GPUI closes the window when the dashboard is hidden, so the
  application leaves the accessibility bus and comes back when it is shown; Slint kept a hidden
  window up.
- **Actions.** Click, Increment and Decrement go to the element's own handlers; `Focus` on any
  part of a widget enters the widget (GPUI hands an unanswered `Focus` to the nearest ancestor that
  answers it; part of `patches/gpui-pre`).

## Not done

- **Text fields** expose what GPUI's input gives them; their labels and the caret are not checked.
- **The list around a widget's rows** (Slint's `list box`, which made Orca say "Wi-Fi List with 4
  items"); Orca says the widget's name and then the row.
- **Announcements** other than the toast (a timer that rings, a notification that arrives).
- **The audit does not enter dialogs** (a Wi-Fi password, Bluetooth pairing) or edit mode's cards;
  `Harness::cursor` handles edit mode's sidebar and the tray menu, but no scenario walks them.
- **The stress run** does not check the cursor through the tree (see the report that came with this
  change: a three-line hook in `Monkey::step`).
- **Real-bus tests** run in the test box only; their output on the author's machine is the
  `scripts/atspi-dump.sh` check above.

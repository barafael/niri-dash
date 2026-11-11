# Screen reader support (GPUI, AccessKit, AT-SPI)

The dashboard is a screen reader's to read and drive. This is what GPUI does for it, what the
dashboard adds, how it is tested, and what is not there. The Slint version's note, which this
replaces for the GPUI dashboard, is [accesskit.md](../../docs/accesskit.md); what it found about
AT-SPI and Orca still holds.

## What GPUI offers (`gpui-pre` 0.3.11)

GPUI builds the accessibility tree itself. There is nothing like the Slint version's tree builder
and bridge (`src/app/a11y/` (at the repository root)) to write.

- **The tree.** While assistive technology is connected, every frame `Window::draw` collects an
  AccessKit node for each element that has both an `.id(..)` and a `.role(..)`, with the
  properties set by `.aria_label`, `.aria_description`, `.aria_value`, `.aria_numeric_value`
  (and `_min_`, `_max_`, `_step`), `.aria_toggled`, `.aria_selected`, `.aria_expanded` and the
  rest of `StatefulInteractiveElement`'s `aria_*`, and hands the whole `TreeUpdate` to the
  platform window. Node ids are a hash of the element's path of ids, so they hold from frame to
  frame while the element keeps its place in the view. Elements without a role are not in the
  tree; plain text (`.child("...")`) is not either, only `text!(..)` is (a `Label` with its value).
  Nothing is built while nobody asks: `Window::is_a11y_active`.
- **Actions.** `.on_click(..)` also answers an AccessKit `Click`; `.on_a11y_action(Action, ..)`
  answers any other (`Increment`, `Decrement`, `SetValue`, `Focus`, ...). A click with no listener
  becomes a mouse click at the node's centre.
- **Focus.** The tree's focus is the element with GPUI's keyboard focus that has a node. An element
  with `.aria_active_descendant()` inside it takes the focus over: the pattern for a composite
  widget that keeps the keys on one element and moves a cursor inside it, which is what the
  dashboard is. Only one element may claim it per frame: a debug build panics on a second claim,
  a release build keeps the last.
- **The Linux platform publishes it.** `gpui-pre-linux` creates an `accesskit_unix::Adapter`
  (AccessKit 0.24, `accesskit_unix` 0.22, the pair the Slint version used) for each Wayland or X11
  window in `a11y_init`. The adapter watches `org.a11y.Status` `IsEnabled` on the session bus; when a
  screen reader turns it on, it connects to the accessibility bus, registers the application there
  (named like the program: `niri-dash`) and sends the tree. A layer-shell window is published like
  any other. `scripts/atspi-dump.sh target/gpui/debug/niri-dash-gpui niri-dash` prints what libatspi reads.
- **Not in GPUI.** No live regions (`aria_live`), no way to name a node's `Live`, `Busy` or
  `Modal`; text fields other than GPUI's own have no text runs unless an element builds them with
  `a11y_synthetic_children`.
- **The test platform has no screen reader.** `TestWindow` leaves `a11y_init` and
  `a11y_tree_update` empty, so no tree is ever built in a headless test (`Window::debug_a11y_tree_json`
  stays `None`), and the flag that switches the tree on is private. See "How it is tested".

## What the dashboard puts in the tree

Names as the Slint version had them (checked against a libatspi dump of both, per widget, while
the dashboard still used a patched GPUI; see "Differences from the Slint version"):

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
  say `selected`: a screen reader would announce that at every stop. A widget with no cursor of its
  own (the clock) is the focus itself while it is entered: its frame claims, and only then, as GPUI
  allows a single claim.
- **A toast is an alert**, read out when it appears.

## How it is tested

There are no headless accessibility tests: GPUI's test window never connects a screen reader, so a
test cannot see the tree, and the published `gpui-pre` has no way to make it. An earlier version of
this work vendored a patched copy of `gpui-pre` that gave the test window one, with a `Harness` over
the tree and the `a11y` and `access` tests (92 widget shapes and ten scenarios) on it; it was dropped
again because a 3.4 MB fork of the toolkit is a lot to carry for tests. It is in the history, in
merge `915964e`, if upstream's `TestWindow` ever grows the same hooks (`a11y_init`,
`a11y_tree_update`, and a way to switch the flag on): `patches/gpui-pre/NIRI-DASH-PATCH.md` there
says what the change was.

What is left are the two real-bus tests, which run the dashboard's own program:

| Command | Checks |
|---|---|
| `cargo testbox test gpui-atspi` | the real bus: the dashboard's own program on a virtual KWin and a private session bus, read by libatspi (`gpui/tests/gpui_atspi_client.py`): roles as libatspi names them, a switch's state, its action turning Wi-Fi off, a slider's value and range. |
| `cargo testbox test gpui-orca` | Orca itself reading the real dashboard, keys sent over the control socket; its transcript is printed. |

Both need the test box (see `testbox/README.md`); outside it they print that they are skipping and
pass, as the Slint ones did. They were written against the patched GPUI; what differs without it is
listed below, and where their expectations leaned on it they have been loosened or marked.

## Looking at the real thing

```sh
scripts/atspi-dump.sh target/gpui/debug/niri-dash-gpui niri-dash                    # what libatspi reads
KIND=battery KEYS="f ba j" scripts/atspi-dump.sh target/gpui/debug/niri-dash-gpui niri-dash   # keys first
scripts/atspi-dump.sh target/debug/niri-dash niri-dash        # the Slint version, to compare
```

It starts the program on the mock system on a private virtual KWin and a private session bus
with its own accessibility bus (never the desktop's). `KEYS` go over the control socket, which
takes `key NAME` lines only when `NIRI_DASH_TEST_KEYS` is set (the script and the tests set it; the
daemon never does).

By hand with a real screen reader, once: `orca &`, `niri-dash daemon`, toggle the dashboard,
`f` and a letter, then `j` and `k`. Things to listen for: the dashboard is announced when it
appears; each stop is read as the cursor lands on it; sliders say their value; a toast is read.

## Differences from the Slint version

- **Static text is not in the tree.** Slint's text items were nodes of their own; in GPUI only
  `text!(..)` is, so a label that is drawn as plain text (the clock's time and date, a caption) is
  not read as such. (The dropped patched GPUI added them; with it the clock read like Slint's.) A
  widget's name and its rows' names and values are set explicitly and are in the tree.
- **No list around a widget's rows.** Slint wrapped each widget's stops in a `list box` named as
  the widget, which Orca announced ("Wi-Fi List with 4 items"). Here the widget is a group.
- **No `NIRI_DASH_ATSPI=0`.** Nothing is built while no screen reader has turned accessibility on
  (`Window::is_a11y_active`), which was what the switch was for.
- **The hidden dashboard isn't there.** GPUI closes the window when the dashboard is hidden, so the
  application leaves the accessibility bus and comes back when it is shown; Slint kept a hidden
  window up.
- **Actions.** Click, Increment and Decrement go to the element's own handlers. A `Focus` request on
  a part of a widget that cannot take the keyboard is not handed on to the widget's frame (the
  patched GPUI did that, so the widget was entered), so it does nothing; `Focus` on the frame itself
  enters the widget.

## Not done

- **Headless accessibility tests** (see "How it is tested").
- **Text fields** expose what GPUI's input gives them; their labels and the caret are not checked.
- **The list around a widget's rows** (Slint's `list box`, which made Orca say "Wi-Fi List with 4
  items"); Orca says the widget's name and then the row.
- **Announcements** other than the toast (a timer that rings, a notification that arrives).
- **Real-bus tests** run in the test box only; their output on the author's machine is the
  `scripts/atspi-dump.sh` check above.

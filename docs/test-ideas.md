# Where the tests could grow

Notes from running the mock-backend stress runs (`niri-dash stress --headless --rate 0 --ops 6000
--seed N --keep-going`, seeds 1 to 3) and reading the harnesses. Nothing here is done yet.

## What that run found, and what it came to

Seeds 1, 2 and 3 failed, with the same findings on the build from before the abstraction change.
All of it was one family of problems, found on small cells (a 1280 by 800 window gives cells of
about 60 pixels, so a widget at its 2x2 minimum is about 120 pixels wide): the keyboard cursor
ended on something the person can't see. The checker's message used to say "the cursor is on
nothing"; it now says "out of sight, on X".

Causes, all fixed:

- A `ScrollList` took the width of its widest row. One row that was too wide (the "Hidden
  network…" row) made the whole column wider than its view and shifted it sideways, so buttons at
  the right edge fell outside it. The list now pins its content width to its view.
- A row's side buttons (close, details) weren't followed by the list: the row scrolled into view for
  the cursor on the row but not on its buttons. `Row` has `kfocus-part` for this.
- The hidden-network form's fields, choices and Join button never scrolled into view at all.
- The Wi-Fi status line was hidden when empty but still took its room in the layout, which in a
  small widget left the form fifteen pixels of height.

Two more were plain overflow, where a row of buttons is wider than a narrow widget: the Security
choice (Open / WPA / 802.1X) in the hidden-network form, and the power profile buttons. They
shrink now: the Security choice with tighter padding, the profile names by being cut short.

Twelve seeds of 6000 keys are clean with the command line's window (`niri-dash stress --headless
--rate 0 --ops 6000 --seed N`), and `tests/stress.rs` covers seeds 1 to 5 on two screen sizes.

## Why the stress test failed one run in four

The same seed gave several different runs. Comparing the transcripts of repeated runs
(`NIRI_DASH_STRESS_LOG=1`, `NIRI_DASH_STRESS_SEEDS=2`, `NIRI_DASH_STRESS_SCREEN=small`) found where
two runs first differ, and each time the cause was time:

- **The mock system answered on a thread of its own.** Its replies landed between keys when the
  machine let them, so the same keys met different states. The harness now answers the mock's
  commands itself, in each turn (`Harness::with_mock`).
- **Slint's timers and animations ran on the clock, and keys came far faster than a person's.** A
  "Press again to confirm" button disarms after a while; a card takes 180 ms to fade; a tile takes
  as long to resize. Whether a key found them done depended on the machine's speed. A stress run now
  keeps its own clock for Slint (`TestPlatform::duration_since_start`) and moves it 120 ms per key.
  With that, eight runs of one seed are identical down to the last line.
- **A real bug that only the first change exposed:** the settings card kept its text fields while it
  faded out, and Tab could take the keyboard into the invisible "Location" field. The fields are
  disabled unless the card is up.
- **Two real layout bugs that were now the same every time:** a list didn't scroll to keep the
  cursor's row in sight when its widget was resized, and a row (Windows, Wi-Fi, Bluetooth) at the
  minimum widget size on a small screen was wider than the widget. Narrow rows drop their extras.

What is left of the real clock: the app's own `Instant::now()` (a slider's hold time, the hide
animation of a real surface). They haven't shown up in a run, and a transcript diff is how to find
them if they do.

## What the run does not cover

The stress monkey only presses keys. The pointer is exercised only by the scripted scenarios in
`tests/edit.rs` (a few clicks and a resize). So "click and move through the whole dashboard" is, in
practice, "type through it".

## Broadening the scenarios

Largest gains first.

1. **Pointer in the monkey.** Clicks on any element the accessibility tree lists, drags (widget
   moves and resizes in edit mode, sliders), wheel and touchpad scrolling, hover. The a11y tree
   already gives the targets, so the monkey can pick from it.
2. **A matrix of widget × shape × state.** Every widget at its smallest, round, tall, wide and
   largest size, against each mock state it can be in: empty lists, disconnected, an error, very
   long names, many rows, unusual locales. Check "nothing overflows its box, nothing is clipped,
   every control is reachable by keyboard", rather than comparing images.
3. **System-side fault injection.** The mock always answers at once. Add a mode where replies are
   late, never come (`FocusedOutput`, `JoinHidden` wait on a `oneshot`), come twice, or events flood
   in. The dashboard should stay usable.
4. **Scale and theme.** Scale factors 1, 1.25, 1.5 and 2, every color scheme and style, the
   reduced-motion setting. Same invariants as in 2.
5. **Config corpus.** `assets/config.kdl`, `config-portrait.kdl`, a config with every widget, a
   config with overlapping or off-grid widgets, and a fuzzed one. The parser should never panic and
   should always report what it dropped.
6. **Golden images with a mask.** Snapshot each widget on the mock with the clock region masked (two
   snapshots of one binary differ only in the clock) and compare with a tolerance. This catches
   visual regressions the invariants don't.
7. **Stress seeds in CI.** `tests/stress.rs` now runs seeds 1 to 3 for 3000 keys on a large and a
   small window. Run it on every push, and on `sim` as well as `mock`.
8. **The contract test.** `crates/system/tests/contract.rs` covers six commands and two capabilities.
   Extend it to every applet's commands, and to a check that a capability that is off means no
   events and no controls.

## System tests in the test box

`cargo testbox test` (`testbox/README.md`) runs the tests that run system code in a Podman
container, and they run nowhere else: `crates/system/tests/live.rs` runs the real system
(`live()`, without a compositor) against mocks of NetworkManager, BlueZ and UPower on a bus of
their own; `contract.rs` checks the mock, the simulation and `live()` against the same contract;
`tests/atspi.rs` and `tests/orca.rs` put the dashboard on a private accessibility bus, read by
libatspi and by Orca. It is built on the neutral event sink: `live(sink, Options { wayland: None,
.. })` runs without a compositor, and the test reads its events from `Sink::channel()`.

What `live.rs` does now:

- Wi-Fi is turned off and on again through the dashboard's `Toggle`, and the state follows.
- The same for the Bluetooth adapter.
- A device battery (a mouse) known to UPower is reported with its charge.
- A program sends a notification over the session bus; the dashboard keeps it, and drops it when
  told to.

The box has no network, no capabilities and a read-only root, and the tests refuse to run outside
it (outside, they print "skipped" and succeed). The test programs are built on the host and
mounted in, so the whole workspace isn't compiled again in the container.

Next steps, from cheap to costly:

1. **More scenarios on the mocks.** The mocks can be driven from the test (a new access point
   appears, the adapter is blocked, a battery drains) to check that the dashboard follows. The
   machine's own battery can't be tested here: it comes from sysfs. Brightness (`/sys/class/backlight`)
   has the same limit, and so does everything that shells out to `loginctl` or `systemctl`.
2. **Real daemons in a privileged container.** NetworkManager with `mac80211_hwsim` virtual radios,
   `bluetoothd` with a virtual controller (`/dev/vhci` exists on this machine), UPower and
   systemd-logind. The kernel modules come from the host, so this needs `--privileged` and a host
   with them loaded. Run it nightly rather than on every push.
3. **A session with a UI.** A headless Wayland compositor with layer-shell support (sway or cage
   with the headless backend) plus `grim` for screenshots, so the real overlay is drawn and then
   checked. Whether niri can run headless is worth finding out, because the windows list and the
   screencast report need it; KWin is a heavier case and can come last.

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

## System tests in a container

`tests/system/run.sh` runs the real system code (`live()`, without a compositor) against mocks of
NetworkManager, BlueZ and UPower on a bus of their own, in a podman (or docker) container. It is
built on the neutral event sink: `live(sink, Options { wayland: None, .. })` runs without a
compositor, and the test reads its events from `Sink::channel()`.

What it does now (`crates/system/tests/live.rs`):

- Wi-Fi is turned off and on again through the dashboard's `Toggle`, and the state follows.
- The same for the Bluetooth adapter.
- A device battery (a mouse) known to UPower is reported with its charge.
- A program sends a notification over the session bus; the dashboard keeps it, and drops it when
  told to.

How it is put together: `tests/system/Containerfile` is an Arch image with `python-dbusmock`;
`mocks.py` starts the two buses and the mocks, then runs the test binary. The binary is built on
the host and mounted in, so the whole workspace isn't compiled again in the container.

It switches radios, so the test binary refuses to run unless it is in a container and
`NIRI_DASH_SYSTEM_TESTS` is set (outside, it prints "skipped" and succeeds).

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

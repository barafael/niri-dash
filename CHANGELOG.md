# Changelog

All notable changes to niri-dash and niri-dash-system. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## Unreleased

### Changed

- **niri-dash-system: a system is one actor, and no queue is unbounded.** The dashboard sends
  one `Command` enum through one sink and gets one `Event` enum back. Breaking for anyone using
  the crate:
  - `live()`, `simulated()` and `mock()` return a `System { commands, caps }` in place of
    `Started { senders, caps }`. Send with `system.commands.tell(wifi::Command::Toggle)`; every
    applet's `Command` converts into `Command`, and back with `TryFrom`.
  - Removed: `Senders`, `Inboxes`, `channels()`, `Started`, `Received` (now `Command`), and the
    re-exports of `UnboundedSender`, `UnboundedReceiver` and `unbounded_channel` (now `Sender`,
    `Receiver` and `channel`).
  - `Sink::new` takes a name for the log, and `Sink::send` returns `Refused::{Full, Closed}` in
    place of `Closed`. `Sink::queue(name, cap)` makes a sink with its async receiver, and
    `System::inbox()` a system's command queue.
  - Every queue has a fixed length (`COMMAND_QUEUE`, `EVENT_QUEUE`). What can wait for room
    waits; what can't (the UI thread, a source reporting an event) drops the value and logs a
    warning.
  - Commands of different applets now arrive in the order they were sent.
- Resize mode: Shift pulls the opposite side in, so an edge moves the way of the key (Shift+l
  pulls the left side in, Shift+h the right, Shift+j the top, Shift+k the bottom).
- The Wi-Fi password prompt is one line tall, and the network list gets the rest of the tile.

### Fixed

- Brightness on desktops: with nothing in `/sys/class/backlight`, the slider moved but nothing
  changed. On KDE Plasma it now goes through PowerDevil (`org.kde.ScreenBrightness`), which
  covers external monitors (DDC/CI, or KWin dimming an HDR screen). With nothing to control, a
  toast says so.
- A Wi-Fi password prompt opened by a click or by NetworkManager takes the keyboard, so the
  password can be typed right away. Escape leaves the field, and a second Escape cancels.
- Tab in a text field stays in its widget instead of jumping to a text field in another widget
  (Tab in the Wi-Fi password no longer lands in the sleep timer). In the hidden network form,
  Tab from the name stops on the security choice.
- In move and resize modes, clicks and wheel turns no longer reach the widget being moved, and
  Tab and Shift+Tab stay inside it.

## 1.0.0-rc.1 - 2026-11-16

The first release candidate.

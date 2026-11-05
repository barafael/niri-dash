# Plan: make the system abstraction independent of Wayland and of which compositor runs

## Context

The system crate has no single `System` trait. The abstraction is per-applet `Command`/`Event` enums, `Senders`/`Inboxes` channels and `Capabilities` (`crates/system/src/lib.rs:108-232`). The backends are `live`, `mock` and `sim`. The `Surface` trait (`src/app.rs:63`) is the seam for the window host and is already generic.

The abstraction still leaks in five places. Each ties the shared types to Wayland, to niri/KDE, or to one OS's conventions, so a new backend can't be written without touching the shared layer. This plan fixes those leaks and nothing else: no new dependencies, no Cargo features, no `cfg` gating, no new backends. Behaviour stays identical.

## Changes

### 1. A neutral event sink
`EventSender` is a `calloop::channel::Sender` taken from smithay-client-toolkit (`lib.rs:63`), so any backend, including a test one, has to use a Wayland crate to send an event.
- Add `Sink<T>` in `crates/system`: an `mpsc::Sender<T>` plus an `Arc<dyn Fn() + Send + Sync>` waker. `EventSender = Sink<Event>`, with `send(&self, T) -> Result<(), Closed>`.
- The 11 use sites (`Ctx.tx` in `sources/mod.rs`, `live.rs`, `mock.rs`, `sim/mod.rs`, `wayland/mod.rs`, `src/app.rs`, `testing.rs`, `snapshot.rs`, `config.rs`, `app/wayland.rs`) switch over.
- `serve` (`src/app/wayland.rs:302`) makes a calloop `Ping` as the waker and drains the receiver in its handler. The rest of the loop stays.
- The wasm build keeps its tokio sender, or uses the same `Sink` with a no-op waker; take whichever is the smaller diff.
- The same `Sink` replaces `calloop::channel::Sender<Input>` for the control input, so `App` no longer needs calloop outside `app/wayland.rs`.

### 2. Capabilities stop meaning "which compositor"
`Capabilities` has `compositor: Compositor`, and the UI branches on `caps.niri()`, `kwin()`, `has_windows()`, `notifications_elsewhere()` and `kwin_night_light()`. `Other` is the only answer that is neither niri nor KDE, so the UI can't tell a backend what it may offer except by naming a compositor.
- Add plain `bool` fields in the style of the existing `clipboard` and `gamma`, with no bit set, and only the ones the UI actually branches on: `windows` (list and focus), `keyboard_layouts`, `color_picker`, `screencast`, `notifications_server` (our own messages go to our history; the inverse of `notifications_elsewhere`) and `night_light_native`.
- `Capabilities::for_compositor(..)` fills them from the compositor that was detected, so the values on Linux are unchanged. `MockSystem::CAPS` and `sim` use the niri values.
- The five helper methods are removed. `Compositor` stays in `Capabilities` only as a label for wording (the "Plasma" captions).
- `edit.rs::caveat` matches on the fields instead of `(compositor, kind)`; `Desktop.niri` in the Slint UI becomes `Desktop.screencast`.

### 3. The alarm sound is a file path owned by the UI
`src/app/timer.rs:14` hardcodes `/usr/share/sounds/freedesktop/stereo/alarm-clock-elapsed.oga` and sends it as `host::Command::PlaySound(path)`.
- Change the command to name the sound, not the file: `PlaySound(host::Sound::Alarm)`. The live actor (`live.rs:194`) maps it to that same file, so nothing changes for the user. The mock records the new command; update the tests that look for it.

### 4. Session commands are confirmed by the backend
`session.rs` already says niri and Plasma ask for confirmation before logout, reboot and poweroff. State it as the contract: **a backend must not carry out `Logout`, `Reboot` or `Poweroff` without the user having confirmed**. The live backend gets this from the compositor. The UI stays as it is, with no confirm step of its own and no flag, since that would be dead code until a backend needs it.

### 5. Path and test-sandbox assumptions
- `src/files.rs:21`: `state_home()` falls back to the data directory when `dirs::state_dir()` is `None`, instead of silently using `./`.
- Tests and `Sandbox::enter` (`src/app/stress.rs:544`, `tests/ui.rs:573`) isolate state with `XDG_*` variables, which `dirs` honours only on some platforms. Add a `NIRI_DASH_HOME` root override that `files.rs` honours first, and have the sandbox set it as well as the `XDG_*` variables, so test isolation doesn't depend on the platform's directory rules.

## Left alone
Vocabulary that has backend-flavoured names but works through an opaque mapping stays as it is: PipeWire/BlueZ ids, `media::State.bus_name`, logind `Mode::method()`, `power::Profile` and `i32` pids. A backend can fill these without changing the types. The Unix-socket IPC, the manifest gating and the fonts code are also unchanged.

## Verification
- `cargo test --locked` (unit, `ui`, `a11y`, `edit`, `stress`, system) and `cargo clippy`; the only clippy warning allowed is the existing one in `mock.rs`.
- `cargo check --target wasm32-unknown-unknown`, because `Sink` and the capability fields touch `sim` and the web build.
- `niri-dash snapshot --mock` before and after the change must give an identical PNG. Use a scratchpad `XDG_CONFIG_HOME` so the real config isn't touched.
- New contract test in `crates/system/tests/contract.rs`. It runs one script against `mock` and `sim`, checks that every applet's commands are accepted, and checks that a backend with a capability turned off sends no events for that applet. Any further backend can be checked with the same script.

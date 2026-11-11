# niri-dash on GPUI

This is the dashboard written on [GPUI](https://gpui.rs), Zed's UI framework. It is a member of the
workspace whose root is the [Slint version](../README.md): the same widgets, the same
[`niri-dash-system`](../crates/system) crate (the actors, their `Command`/`Event` types and the
mock), the same config files, the same control socket, the same test box. What it does and how to
use it is in the [manual](../book/src/introduction.md); this page is what is particular to the
GPUI version.

Program: `niri-dash-gpui`. It has the commands of `niri-dash` (`daemon`, `toggle`, `show`, `hide`,
`edit`, `app <widget>`, `say`, ...), and talks on the same socket, so only one of the two runs at a
time; `niri-dash toggle` from either one reaches whichever daemon is up.

## Build, run, test

Rust 1.95 or newer. System libraries (Debian/Ubuntu names): `pkg-config libfontconfig1-dev
libfreetype-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libxcb1-dev`,
and a Vulkan driver at run time (GPUI draws with wgpu).

The member has its own cargo profile, `gpui` (the root `Cargo.toml`): GPUI's dependencies are far
too slow unoptimised for the headless tests and the stress run. Aliases (`.cargo/config.toml`) use it:

```sh
cargo gbuild                                 # target/gpui/debug/niri-dash-gpui
cargo gbuild --release
cargo gtest                                  # everything that needs no real system
cargo gcheck                                 # check, with the tests
cargo run -p niri-dash-gpui --profile gpui -- daemon --show
NIRI_DASH_MOCK=1 cargo run -p niri-dash-gpui --profile gpui -- daemon --show   # on made-up data
cargo clippy -p niri-dash-gpui --profile gpui --all-targets --features testing
```

`cargo build` and `cargo test` without `-p` build the Slint dashboard (the root package).

The tests are plain programs (`harness = false`) built on `Harness` (`src/testing.rs`): the real
dashboard in GPUI's test platform, with channels standing in for the system, no compositor and no
GPU. `NIRI_DASH_STRESS_OPS=800 cargo gtest` sets the length of the stress run (`tests/stress.rs`).
The `testing` feature, which `cargo gtest` turns on, also gives the binary `niri-dash-gpui snapshot`
(the dashboard drawn headlessly into a PNG) and the on-screen `niri-dash-gpui stress`. The tests that
start real programs (`atspi`, `orca`) skip themselves outside the [test box](../testbox/README.md):
`cargo testbox test gpui-atspi gpui-orca`.

See it on a screen without touching the desktop you work in: `scripts/vshot.sh` runs either
variant on a private virtual KWin on the mock system and takes screenshots:

```sh
scripts/vshot.sh target/gpui/debug/niri-dash-gpui out shot=gpui        # this one
scripts/vshot.sh target/debug/niri-dash out shot=slint                 # the Slint one
```

## Where things are

[`PORTING.md`](PORTING.md) is the architecture (the `Dash` entity, how a widget group is put
together, the keyboard cursor, the harness) and the guide to comparing it with the Slint version.
[`docs/accessibility.md`](docs/accessibility.md) is what a screen reader gets (and what the Slint
version's note, `../docs/accesskit.md`, found still holds).

Differences from the Slint version, on purpose: there is no web build (GPUI has none), the window
is closed when the dashboard is hidden (GPUI can't hide one), and plain text is not in the
accessible tree.

## License

As the rest of the repository: MIT or Apache-2.0, at your option. GPUI and its component library
are Apache-2.0 (GPUI is Zed's, the `gpui-pre` snapshot of it).

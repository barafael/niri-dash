# testbox

The tests that run system code run here, in one Podman container, and nowhere else. System code
is what changes the machine it runs on: the real system (`live()`, which turns radios off and on
and locks the session), and the programs the screen reader tests start (a private bus, Orca,
KWin). On a machine, those tests say they are skipping and pass; in the box, they run.

`cargo testbox --help` is the reference; `cargo testbox` is a Cargo alias (`.cargo/config.toml`)
of the `xtask` package.

```sh
cargo testbox test                    # every phase
cargo testbox test atspi orca         # some
cargo testbox test system --only wifi # the `live` scenarios with "wifi" in their name
cargo testbox shell                   # a shell in the box, the test programs in /test
cargo test -p xtask -- --ignored      # every phase, as a test
```

## The phases

| Phase | Tests | What runs in the box |
|---|---|---|
| `system` | `crates/system/tests/live.rs`, `contract.rs` | `live()` against mocks of NetworkManager, BlueZ and UPower (`mocks.py`, on buses of the box's own); the contract every system keeps, now with `live` among them |
| `atspi` | `tests/atspi.rs` | the dashboard's own program (GPUI) on KWin (`--virtual`) and a private accessibility bus, read by libatspi |
| `orca` | `tests/orca.rs` | Orca reading the dashboard's own program, with KWin as the display of both; keys reach the dashboard over its control socket (`NIRI_DASH_TEST_KEYS`) |

Each test program is run in a box of its own, so none sees what another left.

## What keeps them off the host

- **They refuse.** Each of those tests starts with `inside()` (`testbox/inside.rs`, included by
  each) and, outside the box, prints "skipped" and succeeds before it starts anything. The check
  is two files: Podman's `/run/.containerenv` and `/etc/niri-dash-testbox`, which only the
  image has. An environment variable would prove nothing; it can be set anywhere.
- **Inside, they have little.** The xtask runs the box with no network, every capability
  dropped, `no-new-privileges`, a read-only root and a scratch home, as a user that isn't root.
  It mounts nothing but the test programs and the dashboard's own program (read-only), and the one
  file the AT-SPI test reads beside itself.
- **The tests that don't need it stay on the host.** The UI tests of the GPUI dashboard
  (`cargo test --features testing`: `tests/*.rs`, `edit`, `stress`, `snapshot` and the widget
  groups') run it on the mock system in GPUI's headless test platform, in a throwaway home
  (`Sandbox`), and start nothing. GPUI's test window has no screen reader, so only `atspi` and
  `orca` look at the accessibility tree, on a real bus (`docs/accessibility.md`). The Slint
  variant's tests (`cargo test` in `slint/`) do the same.

What is left on the host: the app asks fontconfig (`fc-match`, `fc-list`) for fonts, and so does
every test that draws. They only read. (`snapshot` also opens the GPU, to draw into a texture,
and skips itself where there is none.)

## How it runs

The xtask builds the image (`Containerfile`: Arch, so that the test programs built on the host
find the same glibc), builds the test programs on the host with `cargo test --no-run` (the
dashboard's with `--features testing`; this also builds `niri-dash`, the program `atspi` and `orca`
start), copies them into `target/testbox-stage`, and runs each in the image with that directory
mounted. The dashboard draws with wgpu: the image has Mesa's software Vulkan (lavapipe) for the GPU
the box doesn't have. Building
the whole workspace in the container too would take far longer for no gain. A program is staged
under the name cargo gave it (`orca-<hash>`): Orca won't start beside another process called
`orca`, and a program of that name would be one.

KWin is installed with its file capability removed (`setcap -r`): with the capabilities dropped,
the kernel refuses to start a program that has one.

## What it needs

Podman, rootless (`CONTAINER_ENGINE` picks another engine), and network while the image builds
(the first time, and when the Containerfile changes). The image is not pinned to a digest; `Arch`
moves, and the host's glibc moves with it.

## Layout

| Path | What it is |
|---|---|
| `Containerfile` | The image |
| `mocks.py` | The box's entrypoint: starts a system bus and a session bus, puts the mocked services on them, runs the program it is given |
| `inside.rs` | The check that a test is in the box |
| `../xtask` | `cargo testbox` itself |

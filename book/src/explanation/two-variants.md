# Two variants: GPUI and Slint

The repository holds the dashboard twice. The root crate is the dashboard on
[GPUI](https://gpui.rs), the one this manual describes and the one a release ships. `slint/` is the
same dashboard as it was written on [Slint](https://slint.dev) before the rewrite, kept for
comparison and because it is the only one that builds for a browser.

| | GPUI (the root crate) | Slint (`slint/`) |
|---|---|---|
| Program | `niri-dash` | `niri-dash-slint` |
| Drawn by | GPUI's wgpu renderer (Vulkan or OpenGL) | Slint: FemtoVG on OpenGL ES, or its software renderer |
| Views | Rust, in `src/ui.rs`, `src/kit.rs`, `src/widgets/` | `.slint` files in `slint/ui/` |
| Browser | No | Yes: the web version, on a simulated system |
| Screen readers | GPUI's accessibility tree (see `docs/`) | AccessKit, over AT-SPI (`docs/accesskit-slint.md`) |
| Tests | `cargo test --features testing` (GPUI's headless test platform) | `cargo test` in `slint/` |
| `snapshot`, `stress` | In a build with `--features testing` | Always |

They share `crates/system` (every part of the machine the dashboard talks to: the live, simulated
and mock systems) and this manual. Each has its own copy of the layout, auto-layout and theme code,
which read the same `config.kdl`, color scheme and style files, so a file written for one loads in
the other.

## Building and running the Slint variant

`slint/` is a workspace of its own, with its own `target/` (it patches Slint's renderer, which only
a workspace root can). Build it from its directory:

```sh
cd slint
cargo run                                  # the daemon
NIRI_DASH_MOCK=1 cargo run -- daemon --show
cargo test
trunk serve                                # the web version
```

Both programs keep their files in the same places, so to compare them, give each a scratch home
(`NIRI_DASH_HOME`, see [Environment variables](../reference/environment.md)) with the same
`config.kdl`. `scripts/vshot.sh BIN OUTDIR STEP...` runs either binary on a private virtual
compositor on made-up data and takes screenshots; `book/screenshots.sh BIN OUTDIR` renders the
manual's pictures with either.

## What differs

Where the GPUI dashboard doesn't do what the Slint variant does, or the other way around:

- **No browser build.** GPUI has no wasm target. The web version is the Slint variant's, and its
  tutorials in this manual are about it.
- **One renderer.** There is no software renderer and no `renderer` choice
  ([Renderers](renderers.md)).
- **Hiding closes the window** at once, without the exit animation.
- **`snapshot` and `stress` are development commands** of a build with `--features testing`, not
  of the program you install. `snapshot` draws at twice the size and resizes the picture, and has no
  `--incremental`, `--keep` or `--bench`: those were about the software renderer's buffers.
- **Text.** Each toolkit shapes and rasterizes text itself, so the same font looks slightly
  different in the two.

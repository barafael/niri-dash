# Renderers

niri-dash draws every frame with [GPUI](https://gpui.rs)'s renderer, which is the GPU's: there is
one renderer and nothing to choose.

| | |
|---|---|
| Drawn by | GPUI's renderer, which uses [wgpu](https://wgpu.rs). |
| Backends | Vulkan, or OpenGL where there is no Vulkan. |
| Where the pixels go | Onto the window's surface, which the compositor shows. |
| Needs | A Vulkan or OpenGL driver. A software Vulkan driver such as lavapipe is a driver like any other, and works where there is no GPU, slowly. |

GPUI picks the adapter itself and logs which one it chose (`Selected GPU adapter: ...`; run with
`RUST_LOG=info`).

## The `renderer` setting

Older `config.kdl` files have a `renderer` line, which chose between `"auto"`, `"gpu"` and
`"software"` for the Slint variant. It is still read, so that such a file loads cleanly, and then
ignored. See [config.kdl](../reference/config.md#renderer).

## The window comes and goes; the state doesn't

GPUI can't hide a window, only close one, so the dashboard's window is opened when it comes up and
closed when it goes away. The renderer's resources belong to the window and go with it; what the
dashboard keeps is its state. See [How the dashboard shows and hides](show-and-hide.md).

## Screenshots

`niri-dash snapshot` draws the same frame with the same renderer, into a texture instead of a
window, so it needs a GPU (or lavapipe) but no display or compositor. GPUI's headless windows have
a scale factor of 2, so the picture is drawn at twice the size asked for and then resized to it.
To see the program on a real screen without touching your own, `scripts/vshot.sh` runs it on a
private virtual KWin and takes screenshots.

## In the Slint variant

The Slint variant in `slint/` had two renderers, chosen by `renderer` in `config.kdl` or the
`NIRI_DASH_RENDERER` environment variable: Slint's FemtoVG renderer with OpenGL ES through EGL, and
Slint's software renderer on the CPU, drawing into a shared-memory buffer and repainting only what
changed. `auto` tried the GPU and fell back to software. That variant carries a patched copy of the
software renderer (`slint/patches/i-slint-renderer-software`), which clips paths to the repainted
region; the reason and the steps for dropping it are in its README. See
[Two variants](two-variants.md).

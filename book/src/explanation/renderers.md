# Renderers: GPU and software

niri-dash draws every frame itself, with one of two renderers. The choice is invisible except in
how much work your machine does.

| | GPU | Software |
|---|---|---|
| Drawn by | Slint's FemtoVG renderer, with OpenGL ES through EGL. | Slint's software renderer, on the CPU. |
| Where the pixels go | Straight onto the surface; `eglSwapBuffers` commits the frame. | Into a pixel buffer already in the byte order `wl_shm` wants (ARGB8888), so presenting a frame is one copy into a shared-memory buffer. |
| Needs | EGL and OpenGL ES. | No GPU support. |

## How `auto` chooses

`renderer "auto"`, the default, tries the GPU and falls back to software if EGL or OpenGL ES can't
be set up. A warning in the log says why. `"gpu"` and `"software"` skip the choice. The choice is
made once, at start. `NIRI_DASH_RENDERER` overrides the setting, which is useful when one of them
misbehaves on your machine.

Reference: [config.kdl](../reference/config.md#renderer),
[Environment variables](../reference/environment.md).

## Both only draw what changed

The software renderer keeps the previous frame's buffer and repaints only the part that changed
(Slint's `ReusedBuffer` mode). Slint 1.18's software renderer had a flaw in that mode: paths (the rings and
graphs) weren't clipped to the repainted region, so an arc that updated under an overlay, such as a
tray menu or a widget being dragged in from the edit-mode drawer, was drawn over it.

niri-dash therefore carries a patched copy of that renderer, with a single change that clips
paths to the dirty region. The copy and the reason are in `patches/i-slint-renderer-software`,
along with the steps for dropping it when Slint fixes the problem. It keeps Slint's own licenses.

## The surface comes and goes; the renderer doesn't

With the GPU, the EGL context is kept when the dashboard goes away and is made current without a
surface, so the renderer's resources survive. See
[How the dashboard shows and hides](show-and-hide.md).

## Screenshots

`niri-dash snapshot` renders offscreen with the software renderer by default, so it needs no
display. `NIRI_DASH_RENDERER=gpu` renders the same frame with FemtoVG into an offscreen texture.

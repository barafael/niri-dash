# How to install niri-dash from source

You need a stable Rust toolchain and the development files for Wayland, xkbcommon, fontconfig and
FreeType. At run time the dashboard needs a Vulkan or OpenGL driver (GPUI draws with wgpu); a
software Vulkan driver such as lavapipe works where there is no GPU.

From a clone of the repository:

```sh
cargo install --path . --locked
```

This puts `niri-dash` in `~/.cargo/bin`. Check that it runs:

```sh
niri-dash --version
```

Then [start it with niri](start-with-niri.md) or [on KDE Plasma](start-on-plasma.md).

To try the dashboard without installing, use the [web version](https://barafael.github.io/niri-dash/)
(the Slint variant: GPUI has no browser build).

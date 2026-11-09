# niri-dash

niri-dash is a fullscreen widget dashboard for the [niri](https://github.com/YaLTeR/niri)
Wayland compositor and for KDE Plasma 6. One key brings it up over everything; another press of
the key, Escape, or a click on the background puts it away.

It runs as a single daemon. The dashboard is a layer-shell overlay drawn with
[GPUI](https://gpui.rs) (Zed's UI framework), on the GPU through Vulkan or OpenGL. While it is
hidden its window is closed, nothing is drawn and its data sources go quiet.

![The dashboard with its default layout, on made-up data](images/dashboard.png)

*The default layout, on made-up data (`niri-dash snapshot --mock`, see [Command line](reference/command-line.md)).*

## Requirements

| | |
|---|---|
| Compositor | niri, or KDE Plasma 6. Some widgets don't work on Plasma: see [KDE Plasma](reference/plasma.md). |
| Building | A stable Rust toolchain, and the Wayland, xkbcommon, fontconfig and FreeType development files. |
| Running | A Vulkan (or OpenGL) driver: GPUI's renderer needs one. A software Vulkan such as lavapipe will do. |
| Without installing | The [web version](https://barafael.github.io/niri-dash/) runs the dashboard on a simulated machine, in your browser. It is the [Slint variant](explanation/two-variants.md), the only one that builds for a browser. |

## How this manual is organised

| If you want to | Read | It contains |
|---|---|---|
| Learn by doing | [Tutorials](tutorials/first-dashboard.md) | Two guided sessions, in the browser, with nothing to break. |
| Get something done | [How-to guides](how-to/index.md) | Short procedures: install, start at login, change a setting. |
| Look something up | [Reference](reference/index.md) | Every widget, key, setting and command, described and nothing else. |
| Understand why | [Explanation](explanation/index.md) | How and why the dashboard behaves as it does. |

The manual describes the GPUI dashboard in this repository (the root crate). The Slint variant in
`slint/` is the same dashboard on another toolkit, kept for comparison: [Two variants](explanation/two-variants.md). Where a page says a setting has a default,
that is the default of the source it was built from.

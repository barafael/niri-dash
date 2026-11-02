# niri-dash

A fullscreen widget dashboard for the [niri](https://github.com/YaLTeR/niri) Wayland compositor,
and for KDE Plasma 6. One key brings it up over everything; another press, Escape or a click on
the background puts it away.

It is a single daemon: a layer-shell overlay drawn with [Slint](https://slint.dev), on the GPU
through EGL (or in software). While hidden it renders nothing and its data sources go quiet.

## Widgets

Clock, calendar, weather (Open-Meteo, no API key), windows by workspace, Wi-Fi, Bluetooth,
battery and power profiles (with a switch that blocks sleep and screen locking), volume,
brightness, night light, notifications, system tray, media player (MPRIS), session (lock,
suspend, log out, …), clipboard history, CPU / memory / disks / network / GPU, keyboard layout,
color picker, timer, stopwatch, sticky notes, privacy (who uses the microphone, camera or
screen), removable drives, phone (KDE Connect), Claude Code sessions and usage, and a mascot
that comments on your day.

Most widgets come in a rectangular and a round shape. Any widget can also run on its own as a
small app window: `niri-dash app clock`.

## Edit mode

The pencil in the corner (or a right-click on the background) opens edit mode: drag widgets
around (the others make room), resize them from the corner, click one for its settings, and add
new ones from the sidebar. The sidebar also sets the grid's columns and rows, the color scheme
(gruvbox, catppuccin, nord, dracula, rose-pine, everforest, solarized, …) and the style:
corners, spacing, fonts, animation speed, frosted glass.

Everything is kept in `~/.config/niri-dash/config.kdl`, which edit mode writes back with your
comments intact. Color schemes and styles are KDL files next to it; add your own.

## Install

Needs a Rust toolchain (stable), and the usual Wayland, EGL, fontconfig and xkbcommon libraries.

```sh
cargo install --path . --locked
```

### On niri

```kdl
spawn-at-startup "niri-dash"

binds {
    Mod+N { spawn "niri-dash" "toggle"; }
    Mod+V { spawn "niri-dash" "app" "clipboard" "--toggle"; }
}
```

`niri-dash window-rules` prints window rules for the standalone apps (floating, centered) to
`include` from niri's config.

### On KDE Plasma

Start `niri-dash` at login (System Settings → Autostart), and bind a key to `niri-dash toggle`
(System Settings → Keyboard → Shortcuts → Add New → Command or Script).

Plasma keeps showing notifications; the notification widget pauses its pop-ups. Night light,
keyboard layouts, the color picker and logging out go through Plasma. The window list, focusing
windows and noticing screen sharing need niri.

`niri-dash --help` lists the other commands: `inhibit`, `dnd`, `night-light`, `say`, ….

## License

MIT or Apache-2.0, at your option.

`patches/i-slint-renderer-software` is a lightly patched copy of Slint's software renderer (see
its README) and keeps Slint's own licenses, in its `LICENSES` directory.

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev)

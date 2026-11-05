# How to run a widget as its own app

Any widget can run in a small window of its own:

```sh
niri-dash app clock
```

If that app is already running, the command shows and focuses it. The widget name is the one
used in `config.kdl`: see the [widget index](../reference/widgets/index.md).

## Bind a key that opens and closes it

`--toggle` closes the app if it is open and shows it otherwise:

```kdl
binds {
    Mod+C { spawn "niri-dash" "app" "clipboard" "--toggle"; }
}
```

## Stop an app that keeps running

Some apps keep running in the background after their window closes (the clipboard history does).
`--quit` stops one for good:

```sh
niri-dash app clipboard --quit
```

## Make the windows float, centered (niri)

`niri-dash window-rules` prints niri window rules for the standalone apps: floating, focused, in
the center third of the screen. Write them to a file and include that file from niri's
`config.kdl`:

```sh
niri-dash window-rules --output ~/.config/niri/niri-dash-rules.kdl
```

```kdl
include "niri-dash-rules.kdl"
```

This is for niri only. On Plasma the window rules are in System Settings.

## Launch apps from a menu

`niri-dash desktop-entries` installs desktop entries such as "Clock (niri-dash)" into
`~/.local/share/applications`. `niri-dash desktop-entries --remove` takes them away again. On
KDE Plasma they also let niri-dash see the window list, so keep them if you use that widget.

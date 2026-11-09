# Command line

```
niri-dash [COMMAND]
```

With no command, `niri-dash` is `niri-dash daemon`. `niri-dash --help` lists the commands,
`niri-dash <command> --help` describes one, and `niri-dash --version` prints the version.

## Commands for the dashboard

| Command | Does |
|---|---|
| `daemon` | Runs the dashboard in the background (the default). Put this in niri's `spawn-at-startup`, or Plasma's Autostart. `--show` shows the dashboard right away. |
| `toggle` | Shows or hides the dashboard. Starts the daemon if it isn't running. |
| `show` | Shows the dashboard. |
| `hide` | Hides the dashboard. |
| `edit` | Shows the dashboard in edit mode. |
| `quit` | Stops the daemon. |
| `inhibit` | Toggles blocking sleep and screen locking, like the switch in the battery widget. |
| `dnd` | Toggles do not disturb: notifications are kept without a pop-up. |
| `night-light [VALUE]` | Makes the screen warmer for the evening. With no value it toggles; `on` and `off` switch it; a temperature in kelvin (1000 to 6500), such as `4000`, turns it on at that warmth. |

## Commands for widgets

| Command | Does |
|---|---|
| `app <WIDGET>` | Runs one widget as an app in its own window. If it is already running, shows and focuses it. |
| `app <WIDGET> --toggle` | Closes the app if it is open, shows it otherwise (for key bindings). |
| `app <WIDGET> --quit` | Stops the app, also if it keeps running in the background (the clipboard history does). |
| `say [OPTIONS] <TEXT>...` | Makes a mascot say something. See below. |

`<WIDGET>` is a widget name, as in `config.kdl`. The names are in the
[widget index](widgets/index.md).

### `say`

| Option | Meaning |
|---|---|
| `<TEXT>...` | What to say. `-` reads it from standard input. |
| `-t`, `--to <TO>` | `schraubi`, `zolli`, `verschnitti`, `cow`, `all`, or `any` (one of them; the default). |
| `-o`, `--outfit <OUTFIT>` | `plain`, `helmet` (a warning), `vest` (careful, this can go wrong), `beard` (musing), `shades`, `tophat`, `party`. |
| `--for <SECS>` | Seconds the line stays up. Default 900. `0` keeps it until clicked. |
| `--show` | Also opens the dashboard. |

## Commands for setup

| Command | Does |
|---|---|
| `window-rules [-o, --output <FILE>]` | Prints niri window rules for the standalone apps (floating, focused, the center third of the screen), for `include` from niri's config. With `--output`, writes them to the file. niri only. |
| `desktop-entries [--remove]` | Installs desktop entries such as "Clock (niri-dash)" that launch the standalone apps, into `~/.local/share/applications`. On KDE Plasma they also let niri-dash see the window list. `--remove` removes them. |

## Development commands

These are not in `niri-dash --help`.

| Command | Does |
|---|---|
| `snapshot <OUT.png>` | Renders the dashboard offscreen to a PNG, without a display and without touching the screen. `--mock` uses made-up data. Other options choose the size (`--width`, `--height`, `--scale`), edit mode (`--edit`, `--tab`, `--configure`, `--arrange`), a standalone app (`--app`), a theme (`--colorscheme`, `--style`) and scripted input (`--events`). The manual's screenshots are made with it: see `book/screenshots.sh`. |
| `stress` | Presses random keys at the dashboard, on made-up data, and checks what they do. Changes nothing on the system: its files go to a temporary directory. By default it runs on screen, in a window of its own, at `--rate` keys a second (with `--ramp-up` and `--ramp-down` seconds of speeding up and slowing down, and a pause button and a pace panel in a second window) until `--ops` keys are done or a window is closed. `--headless` runs without a window as fast as it goes (2000 keys unless `--ops` says otherwise). `--seed` repeats a run, `--config` lays out your own `config.kdl`, `--keep-going` reports every problem at the end. |

Both are only in a build made with the `testing` feature (`cargo build --features testing`, the
headless harness the tests use): the program you install has neither. `snapshot` draws with GPUI's
renderer into a texture, so it needs a GPU (or a software Vulkan such as lavapipe), but no display.
The Slint variant's binary has both always.

Each of these has its own `--help`.

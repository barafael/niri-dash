# Environment variables and files

## Files

| What | Where |
|---|---|
| Configuration | `~/.config/niri-dash/config.kdl` |
| Color schemes | `~/.config/niri-dash/colorschemes/*.kdl` |
| Styles | `~/.config/niri-dash/styles/*.kdl` |
| Sleep timer script | `~/.config/niri-dash/sleep.sh`, or the file named by the widget's `sleep-script` setting |
| Saved state | `niri-dash/state.json` in `$XDG_STATE_HOME` (usually `~/.local/state`; without such a directory, the data directory) |
| Desktop entries | `~/.local/share/applications`, written by `niri-dash desktop-entries` |

`~/.config` is `$XDG_CONFIG_HOME` where that is set.

The bundled color schemes and styles are written to the configuration directory where they are
missing.

## Environment variables

| Variable | Effect |
|---|---|
| `NIRI_DASH_CONFIG` | Path of the configuration file, instead of `~/.config/niri-dash/config.kdl`. |
| `NIRI_DASH_STATE` | Path of the state file, instead of the default. |
| `NIRI_DASH_HOME` | One directory for everything niri-dash keeps: `config/`, `data/` and `state/` below it replace the XDG ones. Used by tests, `niri-dash stress` and `niri-dash snapshot` to stay off your real files. |
| `NIRI_DASH_SIM` | Any value runs on the simulated machine (the web version's) instead of the real one. |
| `NIRI_DASH_MOCK` | Any value runs on the mock system's made-up data. For development. |
| `NIRI_DASH_NO_KEYBOARD` | Any value shows the dashboard without grabbing the keyboard. For development. |
| `NIRI_DASH_ATSPI` | `0` turns off publishing the dashboard to screen readers (AT-SPI). Anything else, or unset, leaves it on. |

niri-dash also sets one variable: `NIRI_DASH_MASCOT` is the name of the mascot in the environment
of a mascot widget's `command`.

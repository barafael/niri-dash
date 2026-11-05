# KDE Plasma

niri-dash runs on KDE Plasma 6 as well as on niri. These are the differences, as `niri-dash
--help` lists them.

| Area | On Plasma |
|---|---|
| Start at login | System Settings → Autostart → Add New → Login Script, and pick the niri-dash binary. Or put a `.desktop` file with `Exec=niri-dash daemon` into `~/.config/autostart`. |
| Key to show the dashboard | System Settings → Keyboard → Shortcuts → Add New → Command or Script: `niri-dash toggle`. |
| Notifications | Plasma keeps showing them. The notification widget's bell and `niri-dash dnd` pause Plasma's pop-ups. |
| Night light | KWin's. The switch and `niri-dash night-light` pause or resume its schedule while niri-dash runs. Its times and warmth are set in System Settings. |
| Keyboard layouts | Go through Plasma. |
| Color picker | Goes through Plasma. |
| Log out | Goes through Plasma. |
| Window list (`windows`) | Not available. |
| Focusing a Claude session's window (`claude-sessions`) | Not available. |
| Noticing screen sharing (`privacy`) | Not available. |
| `niri-dash window-rules` | niri only. On Plasma, window rules are in System Settings. |

How-to: [How to start niri-dash on KDE Plasma](../how-to/start-on-plasma.md).

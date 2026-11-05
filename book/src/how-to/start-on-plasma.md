# How to start niri-dash on KDE Plasma

1. Start `niri-dash` at login. In System Settings, open **Autostart**, choose **Add New** →
   **Login Script**, and pick the `niri-dash` binary. Alternatively, put a `.desktop` file with
   `Exec=niri-dash daemon` into `~/.config/autostart`.
2. Bind a key to `niri-dash toggle`. In System Settings, open **Keyboard** → **Shortcuts** →
   **Add New** → **Command or Script**.

Some widgets and commands don't work on Plasma, and a few go through Plasma instead. See
[KDE Plasma](../reference/plasma.md) for the list.

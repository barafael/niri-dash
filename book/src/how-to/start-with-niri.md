# How to start niri-dash with niri

Add this to niri's `config.kdl` (not niri-dash's), then reload niri's configuration or log in
again:

```kdl
spawn-at-startup "niri-dash"

binds {
    Mod+N { spawn "niri-dash" "toggle"; }
}
```

`niri-dash` with no command runs the daemon. `niri-dash toggle` shows or hides the dashboard,
and starts the daemon if it isn't running.

Change `Mod+N` to the key you want.

## See also

- [How to run a widget as its own app](standalone-apps.md), for keys that open one widget.
- [Command line](../reference/command-line.md), for `show`, `hide`, `edit` and `quit`.

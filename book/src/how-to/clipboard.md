# How to turn clipboard history on and off

niri-dash records what you copy only while a clipboard history is in use. There are two ways to
use one.

## Turn it on

**As a widget on the dashboard.** Open edit mode, open the **Widgets** tab, and add
**Clipboard history**.

**As its own app.** Run it, or bind a key to it:

```sh
niri-dash app clipboard
```

```kdl
binds {
    Mod+V { spawn "niri-dash" "app" "clipboard" "--toggle"; }
}
```

## Turn it off

Remove the widget (the cross in its toolbar in edit mode), or stop the app:

```sh
niri-dash app clipboard --quit
```

Either one stops the recording and forgets the history.

## What is kept

The history stays in memory. What a password manager marks as secret is skipped. For the reasoning
behind this, see *What clipboard history records* (not written yet).

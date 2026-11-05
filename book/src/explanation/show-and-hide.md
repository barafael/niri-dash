# How the dashboard shows and hides

niri-dash is one daemon that is running whether or not you can see it. What changes when you press
the key is whether it is *up*: whether a surface exists on screen, and whether anything is
working to keep it current.

## The surface exists only while the dashboard is up

On niri and on Plasma the dashboard is a fullscreen layer-shell surface on the overlay layer. It
is created when the dashboard comes up and destroyed when it goes away. Hiding is not instant: the
exit animation plays first (a fraction of a second, scaled by the style's `motion`), and the
surface goes once its last frame is drawn.

What outlives the surface is the interface itself. The Slint component stays alive, and with the
GPU renderer the EGL context outlives the surface too, so the GPU resources survive. That is why a
widget's state and history are still there the next time the dashboard comes up, and why coming up
doesn't start from nothing.

While there is no surface, nothing is drawn.

## The sources work only while it is up

Each source of data (Wi-Fi, Bluetooth, the system monitor, the weather, and the rest) polls or
listens for changes. Sources only watch and poll while the dashboard is up, and D-Bus signal
subscriptions are made only then. Coming up refreshes every source at once, so the first frame
shows current values and not the ones from the last time.

The interface follows the same rule: the clock's timer re-arms itself while the dashboard is up
and lapses when it is hidden, since nothing reads the clock then, and coming up ticks it right
away.

The reason is cost. A dashboard that is shown for a few seconds at a time and hidden for hours
would otherwise spend those hours updating a screen nobody sees.

## What does not stop

Some things have to go on while the dashboard is away, because they are about the moment they end
and not about what is on screen. For example:

| Keeps going while hidden | Why |
|---|---|
| The sleep timer's countdown | The machine has to sleep at the time you set. |
| The pomodoro's running part | It tells you when a part ends with a notice and a sound. A running part survives a restart of the daemon. |
| Clipboard history, where it is in use | Recording is its purpose. |

## Coming up and going away reset the interface

Coming up puts every widget back on its first page and leaves no widget entered. Going away leaves
edit mode, closes menus and leaves the widget you were in. The keyboard starts from the dashboard
level each time: see [The keyboard model](keyboard-model.md).

## Standalone apps

`niri-dash app <widget>` runs one widget in an ordinary window, with the same rules: it runs only
the sources its widget needs, and nothing else. When its window closes, the process ends, with one
exception: the clipboard history stays resident in the background, because its history is what it
is for. `niri-dash app clipboard --quit` stops it.

## In the browser

The web version keeps the same shape. The canvas covers the page and the page behind it plays the
desktop. Going away (Escape, say) uncovers the page, and any key or click brings the dashboard
back, as the hotkey would.

Reference: [Command line](../reference/command-line.md), [Time widgets](../reference/widgets/time.md).

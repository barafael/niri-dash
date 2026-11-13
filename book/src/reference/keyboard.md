# Keyboard

niri-dash is built to be used from the keyboard. Keys fall into three levels: the dashboard,
inside a widget, and edit mode. Escape always steps back one level.

![Pressing `f` labels every widget](../images/keys-labels.png)

## On the dashboard

| Key | Does |
|---|---|
| `f` | Labels every widget with the start of its name, as much as it takes to tell it from the others: `v` volume, `ca` calendar, `cl` clock, `n` notifications, `cs` claude-sessions. Two-word names go by the first word's initial. |
| *a label* | Goes into that widget. |
| Backspace | Takes back what you typed of a label. |
| Ctrl+`h` `j` `k` `l` (or Ctrl+`H` `J` `K` `L`) | Goes to the neighbouring widget. |
| Ctrl+E | Opens edit mode. From inside a widget, edit mode starts with that widget picked. |
| Escape | Leaves the widget. A second Escape closes the dashboard. |

A standalone app (`niri-dash app <widget>`) starts inside its widget.

## Inside a widget

| Key | Does |
|---|---|
| `j` `k` | Moves between rows and buttons. Turns a knob or a lone slider down and up. |
| `h` `l` | Moves sideways, or changes a slider. |
| Tab, Shift+Tab | Steps through the stops. |
| Enter, Space | Presses. |
| `g` `G` | Jumps to the first or last item. |
| `e` | Blows the widget up in the middle of the dashboard (round widgets open into their full form). `e` or Escape puts it back. |
| `s` | Opens the widget's settings, the same card as in edit mode. |
| Ctrl+Shift+`h` `j` `k` `l` | Moves the widget one cell, pushing the others aside. |
| Enter on a text field | Types into it. Escape gets you out again. |

Lists have letters of their own:

| Key | Does |
|---|---|
| `d` | Closes a window, or dismisses a notification. |
| `r` | Rescans Wi-Fi. |
| `t` | Turns the thing on and off. |
| `m` | Mutes. |
| `1` … `9`, `0` | Sets a slider to 10 … 90 %, and 100 %. |
| `x` | Ejects a drive. |

### Calendar

| Key | Does |
|---|---|
| `h` `l` | A day back, forward. |
| `j` `k` | A week forward, back. |
| `n` `p` | A month forward, back. |
| `t` | Back to today. |

### Volume

| Key | Does |
|---|---|
| `j` `k` | Makes the slider under the cursor quieter and louder. |
| Tab | Goes to the next slider. |

### Sleep timer

| Key | Does |
|---|---|
| Enter or `i` | Types in the *when* field. Enter there arms the timer. |
| `j` `k`, arrows | Pick the mode. |
| Enter | Arms. |
| Space, `p` | Pauses and resumes. |
| `+` `-` | Moves the deadline a minute. |
| `d`, Ctrl+D | The dry run: counts down for real, then only says what would have run. |
| `a` | Keeps the screen on while it counts. |
| Escape | Gives up on the countdown before it leaves the widget. |

### Pomodoro

| Key | Does |
|---|---|
| Space | Starts and pauses. |
| `n` | Goes to the next part. A skipped focus session doesn't count. |
| `r` | Starts the part over. |

## In edit mode

![Edit mode. The bar at the top shows the keys that work right now.](../images/edit-mode.png)

The bar at the top shows the keys that work at the moment, and changes as you go: with a widget
picked it lists the sizing and moving keys, in the sidebar the row keys, in a text field Enter and
Escape. When nothing else is going on, it lists the starting points below.

Edit mode has two places for the keys: the **widgets** and the **sidebar**. A widget that is
*picked* is the one the keys act on; Tab goes to the sidebar, and `w` `a` `s` `d` comes back.

| Key | Does |
|---|---|
| Ctrl+E | Closes edit mode. |
| Escape | Lets go of the picked widget (and closes its settings card). With none picked, it ends edit mode. |

### Picking a widget

| Key | Does |
|---|---|
| `f`, then a label | Picks that widget. While the labels are up, `f` cancels them and Backspace takes a typed letter back. |
| `w` `a` `s` `d` | Pick the nearest widget up, left, down or right. With none picked, they pick the first one. With a widget picked, the arrow keys do the same. |
| Click | Picks the widget and opens its settings. |

### Working on the picked widget

| Key | Does |
|---|---|
| `h` `l` | Make it one cell narrower, wider. |
| `j` `k` | Make it one cell taller, shorter. |
| Shift, or a capital (`H` `J` `K` `L`) | Size from the left or top side instead of the right or bottom. |
| Ctrl+`h` `j` `k` `l` (or Ctrl+arrows) | Move it one cell, the others making room. |
| `o` | Change its shape. |
| `p` | Pin it, or unpin it. While it is pinned the bar offers only unpinning, picking another widget, its settings and removing it: it can't be sized or moved. |
| `x` (or Delete) | Remove it. |
| Tab | Go to the sidebar. |
| Enter | Show its settings, with the cursor on the first row. |
| Escape | Let go of it. |

A widget that can't grow the way you ask grows the other way instead. A size that leaves no room
for it is refused with a message. The status line shows the size after each step, for example
`Volume 3×2`.

### In the sidebar and in a widget's settings card

| Key | Does |
|---|---|
| Tab | In the sidebar, next tab. In the settings card, next row. |
| `j` `k` | Move between rows. |
| `h` `l` | Step a value, or flip a switch. |
| Enter, Space | Press, or start typing in a text field. In the settings card, toggle or type. |
| Backspace | Put a row back to its default. |
| `f` | In the sidebar: pick a widget. |
| `w` `a` `s` `d` | Back to the widgets. |
| Escape | Done with the sidebar, or close the card. |

In a text field: Enter saves, Escape discards, and Tab leaves the field.

When you carry a new widget in from the Widgets tab: a click places it, and Escape cancels.

On the Widgets tab, the row of categories under **Add** is one stop: `h` `l` move between the
categories, and Enter or Space picks or unpicks the one under the cursor. `j` `k` skip the
widgets of unpicked categories.

## Adjusting a widget without edit mode

Press and hold a widget, without moving, and it opens on its own for adjusting, whatever else the
dashboard was doing: the labels, an entered or expanded widget, a tray menu, another widget being
adjusted all give way.

| Action | Does |
|---|---|
| Drag the widget | Moves it. |
| Drag an edge or corner | Resizes it. |
| The ◯ ▢ button | Changes its shape. |
| Escape | Done. |

This doesn't work in a standalone app, or in edit mode, where every widget is open already.

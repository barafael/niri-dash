# Keyboard

niri-dash is built to be used from the keyboard. Keys fall into three levels: the dashboard,
inside a widget, and edit mode, with two modes on top (moving and resizing a widget). Escape
always steps back one level.

The **quick tips** along the bottom of the screen always list the keys that work right now, and
change as you go. A widget being moved or resized also says so on itself, in a veil with the
mode's name and keys.

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
| Ctrl+`m` | Moves the widget: [move mode](#moving-and-resizing). |
| Ctrl+`r` | Resizes the widget: [resize mode](#moving-and-resizing). |
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

## Moving and resizing

Ctrl+`m` and Ctrl+`r` start the two modes for the widget you are in (or, in edit mode, the one
that is picked). Ctrl letters never collide with a widget's own keys. While a mode lasts the
widget gives up the keyboard, the grid shows, and the widget wears a veil with the mode's name
and its keys; the tips at the bottom say the same.

| Key | Does |
|---|---|
| Ctrl+`m` | Starts move mode. In move mode it ends it, and in resize mode it switches to moving. |
| Ctrl+`r` | The same for resize mode. |
| `h` `j` `k` `l` (or the arrows) | In move mode: moves the widget one cell that way, the others making room. In resize mode: pulls the side on that side out by one cell: `l` the right side, `h` the left, `j` the bottom, `k` the top. |
| Shift + `h` `j` `k` `l` (or Shift + the arrows) | In resize mode: pulls that side in by one cell. |
| Escape | Ends the mode. You are back in the widget (or, in edit mode, with the widget picked). |

A widget that can't grow the way you ask grows the other way instead. A size that leaves no room
for it is refused with a message, and a widget doesn't shrink below its smallest size. The tips
and the veil show the size after each step, for example `Volume 3×2`. Other keys do nothing in a
mode, so a stray letter can't reach the widget.

## In edit mode

![Edit mode. The quick tips at the bottom show the keys that work right now.](../images/edit-mode.png)

The quick tips along the bottom show the keys that work at the moment, and change as you go: with
a widget picked they list the move and resize modes, in the sidebar the row keys, in a text field Enter and
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
| `w` `a` `s` `d` | Pick the nearest widget up, left, down or right. With none picked, they pick the first one. With a widget picked, `h` `j` `k` `l` and the arrow keys do the same. |
| Click | Picks the widget and opens its settings. |

### Working on the picked widget

| Key | Does |
|---|---|
| Ctrl+`m` | [Move mode](#moving-and-resizing): `h` `j` `k` `l` move it one cell, the others making room. |
| Ctrl+`r` | [Resize mode](#moving-and-resizing): `h` `j` `k` `l` pull a side out, Shift pulls it in. |
| `o` | Change its shape. |
| `p` | Pin it, or unpin it. While it is pinned the tips offer only unpinning, picking another widget, its settings and removing it: it can't be sized or moved. |
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

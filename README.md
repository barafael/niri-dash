# niri-dash

A fullscreen widget dashboard for the [niri](https://github.com/YaLTeR/niri) Wayland compositor,
and for KDE Plasma 6. One key brings it up over everything; another press, Escape or a click on
the background puts it away.

It is a single daemon: a layer-shell overlay drawn with [Slint](https://slint.dev), on the GPU
through EGL (or in software). While hidden it renders nothing and its data sources go quiet.

**[Try it in your browser](https://barafael.github.io/niri-dash/)**: the same dashboard, keyboard
and all, on a simulated system whose machine, network and notifications keep changing.

## Widgets

Clock, calendar, weather (Open-Meteo, no API key), windows by workspace, Wi-Fi, Bluetooth,
battery and power profiles (with a switch that blocks sleep and screen locking), volume,
brightness, night light, notifications, system tray, media player (MPRIS), session (lock,
suspend, log out, …), clipboard history, CPU / memory / disks / network / GPU, keyboard layout,
color picker, timer, stopwatch, pomodoro, sleep timer, sticky notes, privacy (who uses the microphone, camera or
screen), removable drives, phone (KDE Connect), Claude Code sessions and usage, and a mascot
that comments on your day.

Most widgets come in a rectangular and a round shape. Any widget can also run on its own as a
small app window: `niri-dash app clock`.

Clipboard history is opt-in. niri-dash reads what you copy only while a clipboard widget is in
your layout (add one in edit mode) or `niri-dash app clipboard` is running. The history stays in
memory and skips what password managers mark secret. Removing the widget, or
`niri-dash app clipboard --quit`, stops the recording and forgets the history.

The sleep timer (`sleep-timer`) is [sleepy](https://github.com/barafael/sleepy) as a widget: count
down to the moment the machine suspends, hibernates or powers off. The *when* is a duration
(`45s`, `30 min`, `1h30m`) or a clock time (`23:45`, the next one to come); what isn't one is
told off in red under the field. Only the modes logind says this machine can do are offered
(suspend, hibernate, hybrid sleep, suspend then hibernate, power off), and a script of your own
(`~/.config/niri-dash/sleep.sh`, or the widget's `sleep-script` setting) as a sixth. The
countdown keeps running while the dashboard is hidden. Keys, once inside the widget: type in the
field with Enter or `i` (Enter there arms), `j`/`k` or the arrows pick the mode, Enter arms,
Space or `p` pauses and resumes, `+` and `-` move the deadline a minute, `d` or Ctrl+D is the
dry run (count down for real, then only say what would have run), `a` keeps the screen on while
it counts, and Escape gives up on the countdown before it leaves the widget. Its settings
(`sleep-when`, `sleep-script`, `dry-run`, `keep-awake`) say where it starts.

The pomodoro (`pomodoro`) alternates focus sessions and breaks: 25 minutes, then 5, and after four
sessions a long break of 15. A ring, or a bar, shows the part in hand, and a dot per session shows
how far the round has come. Each part waits to be started unless `auto-start` is on; a notice and
a sound say when one ends, even while the dashboard is hidden, and a part that is running survives
a restart of the daemon. Space starts and pauses, `n` goes to the next part (a skipped focus
session doesn't count), `r` starts the part over. Its settings are `focus-minutes`, `short-break`,
`long-break`, `rounds` (sessions before the long break) and `auto-start`.

The mascot can also be a cow in the manner of `cowsay`: `character "cow"`, and `cowfile "tux"` for
another animal. Cows are read from your own cowsay install (or from a path); a few of our own stand
in when there is no such file: `bunny`, `dragon`, `moose`, `sheep`, `tux`. Cow files are only read,
never run, so one that needs Perl to draw itself is refused.

## Keyboard

`f` labels every widget with the start of its name, as much as it takes to tell it from the
others: `v` volume, `ca` calendar, `cl` clock, `n` notifications, `cs` claude-sessions (two-word
names go by the first word's initial). Type a label to go into that widget; Backspace takes back
what you typed. Inside, `j`/`k` move between its rows and buttons (or turn a knob or a lone
slider up and down), `h`/`l` move sideways or change a slider, Tab and Shift+Tab step through
the stops, Enter or Space presses, and `g`/`G` jump to the first or last item. Lists have letters
of their own: `d` closes a window or dismisses a notification, `r` rescans Wi-Fi, `t` turns it
on and off, `m` mutes, `1`–`0` set a slider to 10–100 %, `x` ejects a drive, and so on. In the
volume widget `j`/`k` make the slider under the cursor quieter and louder, and Tab goes to the
next one. The calendar has a day cursor: `h`/`l` a day, `j`/`k` a week, `n`/`p` a month, `t` back to today.
Enter on a text field types into it; Escape gets you out again.

`e` blows the widget up in the middle of the dashboard (round widgets open into their full
form); `e` or Escape puts it back. `s` opens the widget's settings (the same card as in edit mode,
below). Ctrl+Shift+`h`/`j`/`k`/`l` move the widget a cell, pushing
the others aside. Ctrl+`r` sizes it: `H`/`J`/`K`/`L` push that side out a cell, `h`/`j`/`k`/`l`
pull it in; Enter, Escape or Ctrl+`r` again when done. Ctrl+`h`/`j`/`k`/`l` (or `H`/`J`/`K`/`L`)
go to the neighbouring widget, Escape leaves the widget, and a second Escape closes the
dashboard. A standalone app starts inside its widget.

## Edit mode

The pencil in the corner (or a right-click on the background) opens edit mode: drag widgets
around (the others make room), resize them from any edge or corner, click one for its settings, and add
new ones from the sidebar. A widget's settings open in a card in the middle of the screen (the
gear in its corner, a click on it, or `s` from inside it); Escape or a click outside closes it.
The sidebar also sets the grid's columns and rows, the color scheme
(gruvbox, catppuccin, nord, dracula, rose-pine, everforest, solarized, …) and the style:
corners, spacing, fonts, animation speed, frosted glass.

All of it works from the keyboard too (Ctrl+E opens and closes edit mode). In the sidebar, Tab
switches tabs (in the settings card, it steps through the rows), `j`/`k` move between rows, `h`/`l` step a value or flip a switch, Enter or Space
presses (or starts typing in a text field), and Backspace puts a row back to its default. `f` and
a label picks a widget and opens its settings; then Ctrl+`h`/`j`/`k`/`l` go to the next widget,
Ctrl+Shift+`h`/`j`/`k`/`l` move it, Ctrl+`r` sizes it (`H`/`J`/`K`/`L` push a side out,
`h`/`j`/`k`/`l` pull it in), `o` changes its shape and `x` removes it. Escape backs out a step at
a time.

The grid won't shrink past what its widgets need: a step that would leave no room for one shakes
and says which, and nothing changes.

Everything is kept in `~/.config/niri-dash/config.kdl`, which edit mode writes back with your
comments intact. Color schemes and styles are KDL files next to it; add your own.

## Install

Needs a Rust toolchain (stable), and the usual Wayland, EGL, fontconfig and xkbcommon libraries.

```sh
cargo install --path . --locked
```

### On niri

```kdl
spawn-at-startup "niri-dash"

binds {
    Mod+N { spawn "niri-dash" "toggle"; }
}
```

If you want clipboard history on a key, bind `spawn "niri-dash" "app" "clipboard" "--toggle"`.

`niri-dash window-rules` prints window rules for the standalone apps (floating, centered) to
`include` from niri's config.

### On KDE Plasma

Start `niri-dash` at login (System Settings → Autostart), and bind a key to `niri-dash toggle`
(System Settings → Keyboard → Shortcuts → Add New → Command or Script).

Plasma keeps showing notifications; the notification widget pauses its pop-ups. Night light,
keyboard layouts, the color picker and logging out go through Plasma. The window list, focusing
windows and noticing screen sharing are not available on Plasma.

`niri-dash --help` lists the other commands: `inhibit`, `dnd`, `night-light`, `say`, ….

## The web version

The dashboard also builds for the browser, against a simulated system instead of the real one
(`crates/system/src/sim`): the same commands and events, with a machine behind them that builds,
browses and takes calls, each thing on a period of its own. Layout and settings stay in the
browser's localStorage; `?reset` starts over. On the desktop, `NIRI_DASH_SIM=1` runs on the same
simulation.

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
trunk serve
```

A push to `main` builds it and puts it on GitHub Pages (`.github/workflows/pages.yml`).

## License

MIT or Apache-2.0, at your option.

`patches/i-slint-renderer-software` is a lightly patched copy of Slint's software renderer (see
its README) and keeps Slint's own licenses, in its `LICENSES` directory.

[![Made with Slint](https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-whitebg.png)](https://slint.dev)

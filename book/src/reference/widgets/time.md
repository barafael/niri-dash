# Time widgets

Clocks, calendars, the weather and timers.

## Clock {#clock}

{{#include ../widgets.generated.md:clock}}

Digital as a rectangle, analog as a circle.

## Calendar {#calendar}

{{#include ../widgets.generated.md:calendar}}

A day cursor moves through the month: see [Keyboard](../keyboard.md#calendar).

## Weather {#weather}

{{#include ../widgets.generated.md:weather}}

Data from Open-Meteo; no API key. Without a location, the city of your time zone is used.

The `weather` node in `config.kdl` can set the location too: see [config.kdl](../config.md#weather).

## Timer {#timer}

{{#include ../widgets.generated.md:timer}}

## Stopwatch {#stopwatch}

{{#include ../widgets.generated.md:stopwatch}}

## Sleep timer {#sleep-timer}

{{#include ../widgets.generated.md:sleep-timer}}

[sleepy](https://github.com/barafael/sleepy) as a widget: it counts down to the moment the machine suspends, hibernates or powers off. The countdown keeps running while the dashboard is hidden.

**When.** A duration (`45s`, `30 min`, `1h30m`) or a clock time (`23:45`, the next one to come). Input that is neither is marked in red under the field.

**What.** Only the modes logind says this machine can do are offered: suspend, hibernate, hybrid sleep, suspend then hibernate and power off. A script of your own is a sixth: `~/.config/niri-dash/sleep.sh`, or the file the `sleep-script` setting names.

**Dry run.** Counts down for real, then only says what would have run.

Keys: see [Keyboard](../keyboard.md#sleep-timer).

## Pomodoro {#pomodoro}

{{#include ../widgets.generated.md:pomodoro}}

Alternates focus sessions and breaks: 25 minutes, then 5, and after four sessions a long break of 15. A ring or a bar shows the part in hand, and a dot per session shows how far the round has come.

Each part waits to be started unless `auto-start` is on. A notice and a sound say when a part ends, even while the dashboard is hidden. A part that is running survives a restart of the daemon. A skipped focus session doesn't count.

Keys: see [Keyboard](../keyboard.md#pomodoro).

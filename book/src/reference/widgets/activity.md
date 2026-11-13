# Activity widgets

What is going on: windows, notifications, sessions, notes.

## Windows {#windows}

{{#include ../widgets.generated.md:windows}}

Click a window to focus it. On KDE Plasma the list is flat, and needs `niri-dash desktop-entries` run once (see [KDE Plasma](../plasma.md)).

## Claude sessions {#claude-sessions}

{{#include ../widgets.generated.md:claude-sessions}}

Click a session to focus its window. On KDE Plasma this works too, and for a session in Konsole its tab is brought up (see [KDE Plasma](../plasma.md)).

## Notifications {#notifications}

{{#include ../widgets.generated.md:notifications}}

The bell pauses pop-ups; `niri-dash dnd` does the same from the command line. Notifications are kept without a pop-up while paused.

On Plasma, which keeps showing notifications of its own, this pauses Plasma's pop-ups. The history there is a copy of what Plasma was sent since niri-dash started, without action buttons.

## Clipboard history {#clipboard}

{{#include ../widgets.generated.md:clipboard}}

Opt-in. niri-dash reads what you copy only while this widget is in your layout or `niri-dash app clipboard` is running. The history stays in memory and skips what password managers mark secret. Removing the widget, or `niri-dash app clipboard --quit`, stops the recording and forgets the history.

See [How to turn clipboard history on and off](../../how-to/clipboard.md).

## Claude usage {#claude-usage}

{{#include ../widgets.generated.md:claude-usage}}

Local only; nothing is sent anywhere. Token counts come from Claude Code's session transcripts in `~/.claude/projects`, and the plan from the `oauthAccount` entry of `~/.claude.json` (two fields of it: the organization type and rate-limit tier). The file with your login tokens, `~/.claude/.credentials.json`, is never opened. The plan's actual limits aren't stored locally, so the widget shows usage, not a percentage of a limit.

## Mascot {#mascot}

{{#include ../widgets.generated.md:mascot}}

Characters: `schraubi`, `zolli`, `verschnitti` and `cow`. `niri-dash say` makes it say something: see [How to make the mascot say something](../../how-to/mascot-say.md).

**`command`** runs a shell command for the mascot's lines, with `NIRI_DASH_MASCOT` set in its environment to the mascot's name.

**Cows.** With `character "cow"` the mascot is a cow in the manner of `cowsay`, and `cowfile` picks another animal. Cows are read from your own cowsay installation (or from a path). When there is no such file, five of our own stand in: `bunny`, `dragon`, `moose`, `sheep` and `tux`. Cow files are only read, never run, so one that needs Perl to draw itself is refused.

## Sticky note {#notes}

{{#include ../widgets.generated.md:notes}}

A note keeps what you type. Two notes with the same name show the same note.

## About {#about}

{{#include ../widgets.generated.md:about}}

The version, the license and the "Made with Slint" credit: [Slint](https://slint.dev) asks for it wherever its widgets are used. It has no settings and no keys.

## niri-dash info {#niri-dash-info}

{{#include ../widgets.generated.md:niri-dash-info}}

What this niri-dash is made of: its version, the UI toolkit (Slint, and its version) and what draws it (OpenGL ES or software), the repository, the crates.io page when it was installed from there, the commit and branch it was built from (and whether there were uncommitted changes), the build profile, target and compiler, and the checkout, build directory and binary.

Click a link (the repository, the commit, a folder) to open it; click anything else to copy it. With the keyboard, `j`/`k` move over the lines, `Enter` opens or copies one and `Ctrl+C` copies it.

The commit, branch and "with changes" are found when the build runs its build script: after a commit, a checkout or `git add`, not after every edit.

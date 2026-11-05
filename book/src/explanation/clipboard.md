# What clipboard history records

A clipboard history is, by nature, a record of things you copied, which may include things you
meant to keep private. niri-dash is built so that the history exists only if you ask for one, only
as long as you want it, and only in memory.

## It is off until a clipboard widget exists

Nothing reads the clipboard by default. Recording starts when a clipboard widget is in your layout
or `niri-dash app clipboard` is running, and stops when neither is. Removing the widget, or
`niri-dash app clipboard --quit`, stops the recording and forgets the history.

There is no setting that turns recording on separately from showing it. If the history is not on
your screen somewhere, it is not being recorded.

## What it reads

niri-dash reads the clipboard through the compositor's data-control protocol (the `ext` or the
`wlr` one), which lets a client see a selection without having the focus. It records two kinds of
content:

| Kind | Types read |
|---|---|
| Text | `text/plain;charset=utf-8`, `text/plain`, `UTF8_STRING`, `STRING`, `TEXT` |
| Image | `image/png` |

Anything else that is offered is not recorded.

## What it skips

A program that copies a secret can say so: password managers such as KeePassXC and KWallet mark
what they put on the clipboard with the `x-kde-passwordManagerHint` type. A selection that offers
that type is skipped, and never read.

This depends on the program doing it. A program that doesn't mark its secrets can't be told apart
from any other copy.

## Where it is kept

In memory, in the daemon or the app that is recording. It is never written to disk. The history
holds the last 50 entries, and one entry can't be larger than 16 MiB. A restart, or removing the
widget, ends it.

## Why it is built this way

A clipboard history that is always on would record everything copied from any program, including
ones that don't mark their secrets, for as long as the machine runs. Opt-in, in memory and
bounded keeps what is at risk small: nothing is recorded unless you set it up, nothing outlives
the process, and nothing is kept without limit.

How-to: [How to turn clipboard history on and off](../how-to/clipboard.md).

# Security

## Supported versions

Only the latest release gets fixes. Before 1.0 that means the latest 0.x or beta release.

## Reporting a vulnerability

Please don't open a public issue for a security problem. Report it privately through GitHub:
[Report a vulnerability](https://github.com/barafael/niri-dash/security/advisories/new).
Say what you found, which version, and how to reproduce it.

This is a small project, so there is no fixed response time, but a security report comes first. A
fix is released before the report is made public, with credit if you want it.

## What niri-dash touches

Worth knowing when you judge a report:

- It reads the clipboard only while a clipboard widget is in the layout, and keeps the history in
  memory only. See the manual's [clipboard page](https://barafael.github.io/niri-dash/manual/explanation/clipboard.html).
- The Claude usage widget reads Claude Code's session transcripts and two plan fields of
  `~/.claude.json`. It never opens the file with the login tokens.
- The weather widget is the only one that talks to the network (Open-Meteo, no key). Everything else
  uses D-Bus, the compositor and local files.
- It asks the system to do things when you press a button: suspend, lock, log out, toggle radios,
  eject a drive.

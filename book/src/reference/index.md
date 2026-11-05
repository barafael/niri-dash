# Reference

Descriptions of what niri-dash has, and nothing else. For procedures see the
[how-to guides](../how-to/index.md); for the reasoning, the [explanation](../explanation/index.md).

| Page | Describes |
|---|---|
| [Widgets](widgets/index.md) | Every widget: name, sizes, shapes, settings, defaults. |
| [Keyboard](keyboard.md) | Every key, on the dashboard, inside widgets and in edit mode. |
| [config.kdl](config.md) | The nodes of the configuration file. |
| [Color scheme and style files](themes.md) | The file format of schemes and styles. |
| [Command line](command-line.md) | Every command and flag. |
| [Environment variables and files](environment.md) | Where niri-dash reads and writes, and what changes that. |
| [KDE Plasma](plasma.md) | What differs from niri. |

The widget tables are written from the source (`Kind::info()` in `src/layout.rs`) by a test, so
they match the program: see `src/book.rs`.

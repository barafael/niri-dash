# How to make the mascot say something

You need a mascot widget on the dashboard (add **Mascot** in edit mode) or running as an app
(`niri-dash app mascot`).

```sh
niri-dash say "Build finished"
```

The line waits in the mascot's speech bubble until you click it away or it expires. While it is
there, the mascot's own remarks pause.

## Choose who says it

`--to` takes `schraubi`, `zolli`, `verschnitti`, `cow`, `all` or `any`. `any`, the default, picks
one of them.

```sh
niri-dash say --to zolli "Build finished"
```

## Dress it

`--outfit` takes `plain`, `helmet` (a warning), `vest` (careful, this can go wrong), `beard`
(musing), `shades`, `tophat` and `party`.

```sh
niri-dash say --outfit helmet "Disk almost full"
```

## Say it from a script

A `-` reads the line from standard input:

```sh
echo "Backup done" | niri-dash say -
```

For example, at the end of a build:

```sh
cargo build && niri-dash say "Build finished" || niri-dash say --outfit helmet "Build failed"
```

## Keep it up longer, or open the dashboard

`--for SECS` sets how long the line stays up (default 900 seconds; `0` keeps it until clicked).
`--show` also opens the dashboard.

```sh
niri-dash say --for 0 --show "Deploy needs a look"
```

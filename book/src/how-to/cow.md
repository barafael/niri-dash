# How to use a different cow

The mascot can be a cow in the manner of `cowsay`. In the mascot's settings, or in `config.kdl`,
set the character to `"cow"` and name the animal with `cowfile`:

```kdl
layout {
    mascot col=4 row=6 width=4 height=2 {
        character "cow"
        cowfile "tux"
    }
}
```

The position is only an example; keep your widget where it is.

`cowfile` takes a name or a path:

| You write | niri-dash looks for |
|---|---|
| `"moose"` or `"moose.cow"` | `moose.cow` in the directories in `$COWPATH`, then `/usr/share/cowsay/cows`, `/usr/local/share/cowsay/cows` and `/usr/share/cows`. |
| `"~/cows/dragon.cow"` or `"/path/to/dragon.cow"` | That file. Anything containing a `/` is a path. |

If cowsay doesn't have the cow, or isn't installed, niri-dash uses one of its own with that name:
`bunny`, `dragon`, `moose`, `sheep` and `tux`. With no `cowfile`, you get niri-dash's own default
cow.

A `.cow` file is only read, never run. One that needs Perl to draw itself is refused, with the reason. An unknown name is reported with the built-in names.

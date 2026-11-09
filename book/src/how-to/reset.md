# How to reset your configuration

## Put the default widgets back

Open edit mode and click **Reset** in the top right corner.

The default widgets replace yours. The rest of `config.kdl` stays as it is: your comments, your
`grid`, your `theme`. The widgets' own settings go with the old widgets.

## Start `config.kdl` over

Move the file away, then start the dashboard again. niri-dash writes a fresh `config.kdl` where
there is none.

```sh
niri-dash quit
mv ~/.config/niri-dash/config.kdl ~/.config/niri-dash/config.kdl.old
niri-dash toggle
```

Color schemes and styles in `~/.config/niri-dash/` are not touched.

## In the web version

The web version is the Slint variant. Add `?reset` to the address and load it. The web version keeps its layout and settings in the
browser's localStorage, and this clears them.

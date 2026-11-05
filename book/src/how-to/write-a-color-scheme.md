# How to write your own color scheme

1. Copy a bundled scheme under a new name:

   ```sh
   cd ~/.config/niri-dash/colorschemes
   cp nord.kdl mine.kdl
   ```

2. Change the first line, which gives the scheme the description shown in the Colors tab:

   ```kdl
   // niri-dash color scheme: "mine" (My colors).
   ```

3. Change the colors. A color is `"#rrggbb"` or `"#rrggbbaa"` (alpha last). Colors you leave out
   keep the default scheme's value:

   ```kdl
   colors {
       card   "#1b1d2b"
       text   "#e6e9f0"
       accent "#ff9e64"
   }
   ```

4. Pick it in the **Colors** tab of edit mode, or name it on the `theme` line:

   ```kdl
   theme colorscheme="mine" style="default"
   ```

If the scheme isn't listed in the Colors tab, restart the daemon with `niri-dash quit` and then
`niri-dash toggle`.

To see what each color is for, see [Color scheme and style files](../reference/themes.md). To
look at a scheme before you pick it:

```sh
niri-dash snapshot --mock --colorscheme mine out.png
```

A style file works the same way, in `~/.config/niri-dash/styles/`.

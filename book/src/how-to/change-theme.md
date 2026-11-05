# How to change the color scheme or style

A theme is a color scheme (colors only) plus a style (corners, spacing, fonts, animation speed).
Any scheme goes with any style.

## In edit mode

1. Open edit mode: click the pencil in the corner, or press Ctrl+E.
2. Open the **Colors** tab and pick a scheme, or the **Style** tab and pick a style.
3. Click **Done**.

Edit mode writes your choice into `config.kdl`.

## In config.kdl

Name the scheme and the style on the `theme` line:

```kdl
theme colorscheme="gruvbox-dark" style="soft"
```

The names are the file names, without `.kdl`, in `~/.config/niri-dash/colorschemes/` and
`~/.config/niri-dash/styles/`. niri-dash writes the bundled ones there where they are missing:

| Color schemes | Styles |
|---|---|
| `default`, `light`, `high-contrast-dark`, `high-contrast-light`, `ayu-dark`, `catppuccin-latte`, `catppuccin-mocha`, `dracula`, `everforest`, `everforest-light`, `gruvbox-dark`, `gruvbox-light`, `kanagawa`, `monokai`, `nord`, `one-dark`, `rose-pine`, `rose-pine-dawn`, `rose-pine-moon`, `solarized-dark`, `solarized-light` | `default`, `compact`, `sharp`, `snappy`, `soft`, `still` |

Leaving out `colorscheme` or `style` means `"default"`.

To write a scheme or style of your own, copy a bundled file under a new name, edit it, and use the
new name on the `theme` line.

To see a scheme or style without changing `config.kdl`, `niri-dash snapshot --colorscheme nord
--style soft out.png` renders one frame with them.

# Widgets

A widget is placed on a grid of cells. Its name is what `config.kdl` and `niri-dash app <widget>`
use.

{{#include ../widgets.generated.md:index}}

## Fields

| Field | Meaning |
|---|---|
| Size in cells | Width × height. *By default* is the size a new rectangular widget gets, and the size [Arrange](../config.md#autolayout) aims for. *At least* is the smallest you can resize it to. *At most* is the size beyond which it only gets emptier, not more useful: nothing stops you resizing past it, but Arrange avoids it. |
| Shapes | Most widgets come in a rectangular and a round shape; some are rectangular only, because lists don't fit in a circle. A round widget without a size is 2×2 cells. |
| Group | Arrange keeps the widgets of a group together. The groups are Time, Activity, Media, System, Devices and Controls. They are also the categories over the list on the Widgets tab in edit mode: unpick one and its widgets fold out of the list until you pick it again (remembered in `~/.local/state/niri-dash/state.json`). |
| Setting | A child of the widget's node in `config.kdl`. Only values that differ from the default are written. |
| Values | What the setting accepts. A number setting accepts the numbers listed, and a value that isn't listed is ignored with a warning. |

A setting that is `empty` by default is text, and an empty text means the widget's own default.

```kdl
layout {
    clock col=0 row=0 width=4 height=2 {
        hours 12
        seconds #false
    }
}
```

## Pages

[Time](time.md) · [Activity](activity.md) · [Media](media.md) · [System](system.md) ·
[Devices](devices.md) · [Controls](controls.md)

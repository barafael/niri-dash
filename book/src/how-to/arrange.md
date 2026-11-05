# How to rearrange widgets automatically

Arrange places every widget by itself, aiming at each one's preferred size and keeping widgets of
a group together. Pinned widgets stay where they are, and the others are laid out around them.

1. Open edit mode (the pencil in the corner, or Ctrl+E) and open the **Arrange** tab.
2. Pick an **Algorithm**:

   | Algorithm | Choose it for |
   |---|---|
   | `guillotine` | A grid that is filled exactly. It finds the cheapest tree of cuts. |
   | `treemap` | Widgets cut greedily in proportion to their preferred areas. |
   | `gravity` | Widgets dropped into the first free spot, then grown into the gaps. |
   | `neighbours` | Widgets that like each other next to each other, and not next to the ones they don't like (volume and brightness; the session buttons and the window list). |

3. Click **Arrange**.

   ![The Arrange tab after a run. The line under the buttons says how many widgets were placed, the cost, how full the grid is and, for `neighbours`, how many liked pairs touch.](../images/edit-arranged.png)

## If you don't like the result

| To | Do |
|---|---|
| Try another, equally valid order of the widgets | Click **Shuffle**. It uses the next seed. |
| Keep the best of several tries | Raise **Tries**, then click **Arrange**. |
| Tune what a good layout is | Change the rows below: **Shrink cost**, **Grow cost**, **Shape cost**, **Oversize cost**, **Round gap cost**. |
| Tilt `neighbours` towards the edges or the middle of the screen | Set **Lean to**. **Neighbour pull** and **Neighbour reach** tune how strongly widgets like each other. |
| Go back | Click **Undo**. It puts back what was there before the first run. |

## Start from these settings next time

**Copy settings** puts the current settings on the clipboard as one line for `config.kdl`:

```kdl
autolayout algo="guillotine" order="group" seed=3 restarts=16 shrink=2.0 grow=0.6
```

Paste it into `config.kdl`. It becomes the starting point of the Arrange tab at the next start.
See [autolayout](../reference/config.md#autolayout) for the keys.

## Limits

The grid won't shrink past what its widgets need. A step that would leave no room for a widget
shakes, says which widget, and changes nothing.

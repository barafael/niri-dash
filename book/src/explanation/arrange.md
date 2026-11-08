# How Arrange chooses a layout

> [!WARNING]
> **Experimental.** Arrange, its algorithms and the `autolayout` line in `config.kdl` may change
> or be removed in any release, before and after 1.0. A line you copied out of the Arrange tab
> may need changing after an update; settings niri-dash no longer knows are reported as config
> warnings.

Placing widgets on a grid is a packing problem with taste. niri-dash has no single answer to it,
so Arrange offers four, and a handful of numbers that say what a good layout is.

## What a good layout is

Every widget has a size it looks best at (its default size), a smallest and a largest, and a group
it belongs with. A layout is good when:

- every widget is close to its preferred size and shape,
- the grid is full, and
- a group stays together.

Those are the three things the costs weigh, and how much each counts is what the cost rows in the
Arrange tab set.

## Four algorithms, because none is best everywhere

| Algorithm | How it works | Trade-off |
|---|---|---|
| `guillotine` | Cuts the grid into two rectangles, and those into two, until each holds one widget, and finds the *cheapest* such set of cuts by dynamic programming over (widgets `i..j`, width, height). The widgets keep a given order, so a group sits together. | Fills the grid exactly and respects minimum sizes. Its cost grows with the cube of the widget count, which is fine for a dashboard. |
| `treemap` | Cuts the grid greedily, in proportion to the widgets' preferred areas. | Takes no time and always fills the grid, but chooses shapes less carefully. |
| `gravity` | Puts each widget at its preferred size in the first free spot, then lets them grow into the holes. | May leave gaps. The widgets sit in order, like a masonry wall. |
| `neighbours` | Searches, by simulated annealing, for a placement where widgets that like each other touch and ones that don't, don't. | Keeps gaps. |

## Neighbours and the affinity table

`src/affinity.rs` holds how much each pair of widget kinds likes to be neighbours. The scale runs
from 1.0, right beside each other (volume and brightness; clock and calendar), through 0.0, no
opinion (notifications and the weather), to −1.0, keep apart (the session buttons and the window
list). The table is built in layers: widgets of one group like each other a little, widgets of the
same kind more, and then lines of explicit pairs, each overriding the earlier ones.

The Arrange tab reports how many liked pairs ended up touching.

## The order the widgets go in

The algorithms place widgets in an order, and the order shapes the result: neighbours in the order
end up as neighbours on the grid, and the last ones are the first to be dropped when there is no
room for all of them.

| Order | Puts first |
|---|---|
| `group` | By group, the big ones of a group first. |
| `area` | The big ones first. |
| `reading` | Where they are now, top to bottom, left to right. |
| `kind` | As in the widget list. |
| `affinity` | Widgets that like each other next to each other, in one long chain. |

## Repeatable, and still tunable

All of it is deterministic for a given set of parameters: the same widgets and the same settings
give the same layout. Variety comes from the seed. Seed 0 is the order as it is, any other number
is another repeatable shuffle of it, and **Tries** (`restarts`) tries that many seeds in a row and
keeps the cheapest. The **Shuffle** row (`jitter`) says how far a shuffle may move a widget in the
order.

That is why the settings can be copied out as a line of `config.kdl` and kept: the line holds
everything that decided the result.

## What the costs cost

| Row | `config.kdl` key | What it charges |
|---|---|---|
| Shrink cost | `shrink` | A widget smaller than it likes, per squared log of the area ratio. |
| Grow cost | `grow` | A widget larger than it likes, likewise. |
| Shape cost | `aspect` | A squarer or wider shape than it likes, per squared log of the ratio. |
| Oversize cost | `overshoot` | Each cell beyond a widget's largest size, squared. |
| Round gap cost | `hole` | Each cell a round widget leaves empty around itself. |

The rest apply to one algorithm each. Gravity has **Max growth**: how much larger than its preferred
area a widget may grow. Treemap has **Area weight**, where the area is weighted by the preferred
area to a power (1 is proportional, 0 gives every widget the same), and **Cut bias**, which makes
cuts prefer to divide the width or the height. Neighbours has **Neighbour pull** (how much liking
counts against the size costs), **Neighbour reach** (how many cells apart still counts as near),
**Lean to** and **Lean strength** (towards the screen's edges, keeping the middle free, or towards
its middle), **Centering** and **Effort** (how long to search).

## Pinned widgets

A pinned widget isn't placed: the others are laid out around it. See
[How to pin a widget](../how-to/pin-a-widget.md).

How-to: [How to rearrange widgets automatically](../how-to/arrange.md). Reference:
[autolayout](../reference/config.md#autolayout).

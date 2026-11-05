# How to pin a widget

A pinned widget stays where it is: nothing pushes it aside, [Arrange](arrange.md) lays the others
out around it, and it can't be moved or resized until you unpin it.

## In edit mode

Click the lock in the widget's toolbar.

From the keyboard, in edit mode, press `f` and the widget's label, then `p`.

To unpin it, do the same again.

## In config.kdl

A pinned widget has `pinned=#true`:

```kdl
layout {
    clock col=0 row=0 width=4 height=2 pinned=#true
}
```

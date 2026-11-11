# i-slint-renderer-software 1.18.1, patched for niri-dash

Slint's software renderer as published on crates.io, with one change in `lib.rs` (marked with
`niri-dash patch` comments): `RenderToBuffer::process_filled_path` and `process_stroked_path`
draw only inside the dirty region, the way rectangles and textures already do. The top-level
`Cargo.toml` swaps it in with `[patch.crates-io]`.

## Why

niri-dash renders with `RepaintBufferType::ReusedBuffer`, so each frame repaints only what
changed. Unpatched, a path that is partly dirty is drawn in full: a ring or graph that updates
under an overlay (a tray menu, a widget being dragged in from the edit-mode drawer) paints over
the parts of the overlay that weren't repainted.

## Upgrading Slint

The patch has to have exactly the version slint depends on. For a new release:

1. Look at `process_filled_path` and `process_stroked_path` in the new release's `lib.rs`. If
   they already clip to `self.dirty_region`, delete this directory and the `[patch.crates-io]`
   entry.
2. Otherwise replace this directory with the new release's crate (from `~/.cargo/registry/src`
   after a build with the patch entry commented out) and apply the same change to both
   functions.

To see the bug, run with the software renderer (`NIRI_DASH_RENDERER=software`), place a
Performance widget and open a tray menu over its graph: unpatched, the graph draws through the
menu as it updates.

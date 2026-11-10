# gpui-pre 0.3.11, with a screen reader for the test window

This is the published `gpui-pre` 0.3.11 (src, build.rs, Cargo.toml without its examples and
benches, README, licence) and two small additions, both in what is only built with `test-support`:

- `src/platform/test/window.rs`: `TestWindow` implements `a11y_init` and `a11y_tree_update`, the
  platform hooks it left empty, keeping the callbacks (activation, action, deactivation) and the
  last `TreeUpdate`. New methods: `a11y_activate`, `a11y_deactivate`, `a11y_tree`, `a11y_request`.
- `src/app/test_context.rs`: the same four on `TestAppContext`, taking the window's handle.

What they do: `a11y_activate` calls what the Linux adapter calls when the desktop turns
accessibility on, so from the next frame the window builds the tree it would give AT-SPI, and
`a11y_tree` returns it. `a11y_request` is what the adapter does with a request from a screen
reader (click, increment, focus). Nothing is active until a test asks, so every other test is
unchanged.

To see the whole change, diff against the registry copy:
`diff -ru ~/.cargo/registry/src/*/gpui-pre-0.3.11/src patches/gpui-pre/src`.

Why not upstream's own way: there is none. `Window::debug_a11y_tree_json` and
`Window::is_a11y_active` only work once the platform has activated the window, and the test platform
never does. If upstream's `TestWindow` grows this, delete this directory and the `[patch.crates-io]`
in the root Cargo.toml; `src/testing.rs` (`Harness::a11y*`) is the only user.

# gpui-pre 0.3.11, with a screen reader for the test window

This is the published `gpui-pre` 0.3.11 (src, build.rs, Cargo.toml without its examples and
benches, README, licence) and the additions below. First, a screen reader for the test window (only built with `test-support`):

- `src/platform/test/window.rs`: `TestWindow` implements `a11y_init` and `a11y_tree_update`, the
  platform hooks it left empty, keeping the callbacks (activation, action, deactivation) and the
  last `TreeUpdate`. New methods: `a11y_activate`, `a11y_deactivate`, `a11y_tree`, `a11y_request`.
- `src/app/test_context.rs`: the same four on `TestAppContext`, taking the window's handle.

Further changes, to the accessibility code itself (they apply to the real window too):

- Plain text drawn (`.child("...")`, a `SharedString`) becomes a `Label` leaf node (value: the text)
  under the node it is drawn in, while a screen reader is connected (`A11y::push_text`).
- A claim of the focus (`aria_active_descendant`) by a node below the one that holds it overrides
  it; only a claim by a node that isn't below the holder still panics in a debug build.
- A request to focus a node that cannot take the keyboard goes to the nearest ancestor that
  answers `Focus` (`A11y::parent_of`).

What the test-window additions do: `a11y_activate` calls what the Linux adapter calls when the desktop turns
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

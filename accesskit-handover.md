# AccessKit handover

Written 2026-11-05 from old session transcripts and a read of the current tree. Nothing was built or
run to write it. Test results and open issues are dated where they come from an old session and
were not re-run or rechecked.

## Status

**AccessKit is not in niri-dash.** It appears nowhere in the working tree, the git history or
`Cargo.lock`, and `Cargo.toml` doesn't enable Slint's winit backend. A screen reader cannot read the
dashboard today.

What did land is the groundwork, in `d4de1f6`: accessibility properties on the widgets, plus tests
that read them.

## How it got dropped

Session `let-s-split-up-some-gentle-cupcake`, 2026-11-03 22:45:

- You asked: "Would it make sense to include accesskit here?"
- The answer split the work into step 1 (labels and tests) and step 2 (real AccessKit for Orca),
  recommended step 1, and asked whether to start.
- You said: "great. Do it. Full send!" The session took that as labelling every widget, not as
  doing step 2.
- 2026-11-04: step 2 was reported as "not started, and only worth doing if someone will use it", and
  the plan written later that day listed it under "Not included".

Step 2 was deferred on the condition that someone would use it. No transcript found records you
deciding against it.

## What exists

- **Labels:** 348 `accessible-*` uses in `ui/`, counting uncommitted edits. Mostly `accessible-label`
  (172), `-role` (53), `-value` (32), `-action-default` (26) and `-item-selected` (21).
- **`src/app/a11y.rs`:** walks Slint's `ItemRc` tree and reports what a screen reader would say about
  the keyboard's position. Only the stress run and tests use it. This is the natural starting point
  for building an AccessKit tree.
- **`tests/a11y.rs`:** checks each widget kind in each shape: every stop has a name and the cursor is
  on exactly one thing. `NIRI_DASH_A11Y_KIND=wifi` limits it to one kind.
- **`cargo run -- stress`:** shows the dashboard in a window, presses 4 keys a second and prints what
  a screen reader would say. It doesn't touch the real system.
- **Last reported results (2026-11-04, on the merged tree, run by another session):** UI tests 18/18,
  accessibility audit 84/84, stress 3/3 default seeds and 6/6 long seeds.

## Why step 2 is real work

Slint's AccessKit support can't be switched on here:

- It lives in `i-slint-backend-winit` 1.18.1, behind its `accessibility` feature (`accesskit` 0.24,
  `accesskit_winit` 0.33). `AccessKitAdapter` holds a `Weak<WinitWindowAdapter>`.
- The dashboard doesn't use winit. `src/shell.rs` hosts Slint on a wlr-layer-shell surface (or an
  xdg-shell window for standalone apps), and frames come from one of two renderers:
  - **GPU:** `GpuWindow`, our own `WindowAdapter` (`src/gpu.rs:283`).
  - **Software:** Slint's `MinimalSoftwareWindow` (`src/shell.rs:165`, `:692`).

So we would build the tree ourselves, run `accesskit_unix`, and send focus changes and actions back.

### Where to hook in

Paths are relative to `~/.cargo/registry/src/index.crates.io-*/`.

- `i-slint-backend-winit-1.18.1/accesskit.rs` (1011 lines) is the reference implementation:
  - `build_node_for_item_recursively`: line 465
  - `handle_request`: line 148, actions coming back from the screen reader
  - `handle_focus_item_change`: line 129
  - `rebuild_tree_of_dirty_nodes`: line 233
  - `try_emit_text_input_accessibility`: line 542
  - Delegate-focus resolution: about line 408
- `i-slint-core-1.18.1/window.rs:225` has `WindowAdapterInternal`, with `register_item_tree` (227),
  `unregister_item_tree` (231) and `handle_focus_change` (265).
- `GpuWindow` doesn't override `WindowAdapter::internal()`, so it gets none of these callbacks yet.
  `MinimalSoftwareWindow` isn't ours, so the software path would need another route, such as a
  wrapper adapter or re-reading focus after each event.

## Known problems for a real screen reader

The first two came from the 2026-11-04 notes and were not rechecked since.

- **Misused property:** the cursor is marked with `accessible-item-selected` (see `NavScope` in
  `ui/common.slint` and the header of `src/app/a11y.rs`). A real screen reader could announce
  "selected" on a button. The plan chose to leave this until AccessKit exists.
- **Tab between text fields:** the spoken field can differ from the one being typed in.
- **`accessible-delegate-focus` is unused (0 uses in `ui/`):** the old session found it fits our
  pattern, where a `FocusScope` holds the keyboard while an index picks the real item. It may be the
  proper replacement for the marker. However, the `a11y.rs` header says a scope's controls are often
  beside it rather than inside, so check that first.
- **Text details:** the winit backend turns on `i-slint-core/accessibility-text`, which we don't
  enable. Text field support may need it.

## Constraints

- **Keep tests off the system bus:** `accesskit_unix` publishes to AT-SPI over D-Bus, so tests and
  stress runs must keep it switched off.
- **Actors, not mutexes:** system code uses actors, with no `Arc<Mutex>`. Slint items stay on the UI
  thread, so requests from the AccessKit side need to be sent there. The winit backend has
  `DeferredAccessKitAction` and `invoke_later` for this.
- **Linux only:** the web build has its own platform (`src/app/web.rs`) and is out of scope.
- **Version pins:** `i-slint-core` and the renderer are pinned to `=1.18.1`, and these are internal
  APIs, so expect Slint upgrades to break the glue.

## Open questions

1. **Will anyone use this with a screen reader?** This is the condition step 2 was deferred on.
2. **Will Orca read a layer-shell overlay at all?** Untested, and it could change the plan, so answer
   it first.
3. **Which `accesskit_unix` version pairs with `accesskit` 0.24?** Several are in the cargo cache
   (0.24.1 is the newest); none is in `Cargo.lock`.

## Suggested first step

A small spike: build a tree for one widget from the `ItemRc` walk in `src/app/a11y.rs`, publish it
with `accesskit_unix` from the GPU path, and see whether Orca reads it. That answers question 2 and
shows how much glue the rest needs.

## Related, not AccessKit

- **Slint's MCP server:** `i-slint-backend-testing` 1.18.1 has an `mcp` feature, run with
  `SLINT_EMIT_DEBUG_INFO=1 SLINT_MCP_PORT=8080` and `--features slint/mcp`. It could let an MCP client
  inspect and drive the running dashboard through the same element tree. Never tried on our platform.
- **Open from the same work (2026-11-04, not rechecked):**
  - Unconfirmed, from reading the code:
    - The Wi-Fi cursor can go stale on a row's sub-part.
    - The Wi-Fi cursor can land on Cancel or Join after toggling "Don't check the server".
    - The Windows cursor can rest on a heading.
    - Some form fields can scroll out of view.
  - **Privacy list:** with only mic or camera apps listed, nothing lights up, and a cursor marker an
    agent added hides this from the check. The session wanted to remove the marker and fix it properly.
  - **Edit mode:** it has labels, but the "what's under the keyboard" check skips it.

## Sources

Reopen the original session (the `actors` worktree still exists, on branch `actors` at `736b40b`):

```
cd /home/rafael/code/niri-dash/.claude/worktrees/actors
claude --resume 572bcea9-1c55-4027-a89a-d353cf122a21
```

Transcript: `~/.claude/projects/-home-rafael-code-niri-dash--claude-worktrees-actors/572bcea9-1c55-4027-a89a-d353cf122a21.jsonl`

| Timestamp (UTC) | What |
|---|---|
| 2026-11-03 22:45 | The question and the two-step answer |
| 2026-11-03 22:49 | "Full send", then the look at delegate-focus |
| 2026-11-04 00:20 | Status report, with bugs and known limits |
| 2026-11-04 11:14 | Results and the open list |
| 2026-11-04 11:49 | The plan listing AccessKit as "Not included" |

Other matches were incidental: `2458800e` (a file listing and a grep) and two subagent transcripts
(dependency dumps and a live-region check).

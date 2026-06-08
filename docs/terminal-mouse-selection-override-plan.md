# Terminal Mouse Selection Override Plan

Updated: 2026-05-30

m107 defines the user-facing policy for local selection while a terminal
application has enabled xterm mouse reporting.

m108 implemented the first slice of this plan in
`docs/terminal-mouse-selection-override.md`.

## Current Behavior

Native window mode currently uses a hard switch:

- when `TerminalMouseModes::reports_mouse()` is false:
  - left press/drag updates local selection
  - left release publishes a dragged selection to primary selection
  - double-click selects a word
  - middle click pastes primary selection
  - wheel scrolls local scrollback
- when `reports_mouse()` is true:
  - button, motion, and wheel events are encoded as terminal input
  - local selection, primary paste, and scrollback are bypassed

Browser mode currently mirrors the application-mouse side of that model:

- pointer and wheel events call `WittyWebSession::handle_mouse()`
- handled events are flushed to the gateway and `preventDefault()` is applied
- no local browser selection API is exposed yet

## Product Decision

Use Shift as the default local-selection override while application mouse
reporting is active.

Default policy:

| Gesture while mouse reporting is active | Behavior |
| --- | --- |
| plain left/middle/right press, release, drag | send xterm mouse report to application |
| plain wheel | send xterm wheel report to application |
| Shift + left press/drag/release | local terminal selection, no app mouse report |
| Shift + left double-click | local word selection, no app mouse report |
| Shift + middle press | local primary-selection paste, no app mouse report |
| Shift + wheel | local scrollback, no app wheel report |
| Alt/Ctrl without Shift | preserve current app mouse reporting behavior |

This matches the common terminal expectation that Shift temporarily gives the
terminal UI priority over a full-screen TUI that captured the mouse.

## Compatibility Tradeoff

Xterm SGR mouse encoding can carry a Shift modifier bit (`Cb += 4`). The default
override means applications will not receive Shift-left selection gestures,
Shift-middle paste gestures, or Shift-wheel gestures. That is the right product
default for usability, but it must not be hard-coded as the only possible
behavior.

Introduce a profile-level policy before or during implementation:

```rust
pub enum MouseSelectionOverridePolicy {
    ShiftSelect,
    Disabled,
}
```

Initial default: `ShiftSelect`.

`Disabled` preserves raw xterm compatibility: Shift mouse events are sent to the
application with the xterm Shift modifier bit. This should be reachable from a
future profile/config layer. Until that layer exists, m108 can wire a constant
or constructor field with `ShiftSelect` as the product default and keep tests
around the pure decision helper.

## Native Implementation Scope For m108

Add a small decision helper in `witty-app` instead of spreading modifier checks
through every event branch.

Suggested shape:

```rust
enum MouseLocalOverrideAction {
    None,
    Selection,
    PrimaryPaste,
    Scrollback,
}
```

The helper should consider:

- current mouse reporting mode
- configured override policy
- current modifiers
- button/state/event kind
- whether a local selection drag is already active

Native event behavior:

1. If a local selection drag is active, continue local selection updates and
   consume the left-button release even if Shift is released mid-drag.
2. If mouse reporting is active and policy is `ShiftSelect`:
   - Shift + left press starts the existing `begin_selection()` flow.
   - Shift + cursor motion during that drag calls `update_selection()`.
   - left release calls `end_selection()`.
   - Shift + middle press calls `paste_primary_selection_to_terminal()`.
   - Shift + wheel calls `scroll_viewport()`.
3. Otherwise keep the current app-reporting behavior.

The active-drag rule matters: once the user starts a Shift-selection drag, no
part of that drag should leak as app mouse motion if the Shift key state changes
before release.

Native focused tests:

- Shift + left press while `1002/1006` is active starts local selection and
  writes no mouse input bytes.
- Shift drag updates local selection while app mouse reporting remains active.
- releasing left after a Shift drag publishes primary selection and writes no
  release mouse report.
- Shift + double-click while app mouse reporting is active selects a word.
- plain left press with reporting active still emits the existing SGR mouse
  bytes.
- Shift + middle press reads primary selection and writes paste bytes instead
  of mouse bytes.
- Shift + wheel scrolls local viewport instead of emitting wheel bytes.
- with policy `Disabled`, Shift + left press still emits SGR bytes with the
  Shift modifier bit.

## Browser Implementation Scope For m108

Browser support needs one extra layer because local selection is not currently
exposed through the wasm API.

Minimum browser parity path:

1. Add wasm methods mirroring the native local-selection operations:
   - `begin_local_selection(offset_x, offset_y, click_count_or_timestamp)`
   - `update_local_selection(offset_x, offset_y)`
   - `end_local_selection()`
   - `paste_primary_selection()` can be deferred because browser primary
     selection support is platform/browser constrained.
2. Keep browser Shift + left drag local to the terminal canvas:
   - do not call `handle_mouse()` for that drag
   - update `BasicTerminal::set_selection()`
   - render the updated frame
   - prevent default browser behavior
3. Continue sending plain mouse events to the gateway unchanged.

If browser clipboard/primary selection is not ready in m108, document that
native has Shift+middle parity first and browser starts with Shift+left
selection parity only.

Browser focused tests:

- Shift + pointerdown while mouse reporting is active starts local selection and
  produces no gateway input frame.
- Shift + pointermove updates the rendered selection range.
- Shift + pointerup ends the local selection without emitting a mouse release
  frame.
- plain pointerdown while mouse reporting is active still emits SGR mouse bytes.

## Runtime Smoke Scope

Keep m108 runtime smoke narrow:

- Extend the browser node-gateway smoke with Shift + pointerdown/drag/release
  after enabling mouse mode.
- Assert no new gateway input frame is produced for the Shift local-selection
  gesture.
- Assert a visible selected-text or selection-range diagnostic from wasm.
- Keep product launcher smoke unchanged unless browser local selection is
  implemented in the same milestone.

Native GUI smoke can remain unit-level until there is a stable headless pointer
event harness for modifier-state mouse gestures.

## Non-Goals

- Full terminal profile UI.
- Multi-modifier remapping UI.
- Browser system primary-selection integration.
- Rich selection handles or touch selection.
- Changing xterm mouse encoding semantics when override policy is disabled.

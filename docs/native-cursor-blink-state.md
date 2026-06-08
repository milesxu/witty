# Native Cursor Blink State

Updated: 2026-06-01

## Purpose

`m328-native-cursor-blink-state` adds the first app-level cursor blink timing
for the OpenGL-first native terminal path without touching a GPU device,
browser runtime, Vulkan, or a real window during validation.

## Behavior

- `CursorState` now carries a `blink` bit in addition to position, shape, and
  visibility.
- `DECSCUSR` keeps the xterm blink/steady split:
  - `0`, `1`: blinking block
  - `2`: steady block
  - `3`: blinking underline
  - `4`: steady underline
  - `5`: blinking bar
  - `6`: steady bar
- Native window mode owns `CursorBlinkState`, a pure state machine that hides
  or restores only the frame cursor overlay every 500 ms.
- Blink resets to a visible phase when the terminal cursor identity changes
  by position or shape.
- Blink is disabled when the terminal cursor is hidden, when the current frame
  has no cursor rect, or when search/command palette owns text input.
- Synchronized output timeout handling keeps priority over cursor blink redraws
  so a synchronized-output burst is not broken by blink-only frames.

## Boundary

The terminal core records what the application requested. The native window
decides when to draw or suppress the cursor overlay. This keeps blink timing
out of `BasicTerminal`, `FramePlanner`, plugins, terminal text, and transport
events.

Browser blink timing is intentionally not added in this slice because local
Chromium/WebGPU validation remains suspended on the Linux/M1000 host.

## Verification

- `cargo test -p witty-core cursor_shape --quiet`
- `cargo test -p witty-app cursor_blink --quiet`
- `cargo test -p witty-render-wgpu cursor --quiet`
- `cargo test -p witty-core --quiet`
- `cargo test -p witty-app --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `cargo check -p witty-app --quiet`
- `git diff --check`

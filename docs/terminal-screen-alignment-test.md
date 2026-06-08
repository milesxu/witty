# Terminal Screen Alignment Test

Updated: 2026-06-01

`m342-terminal-screen-alignment-test` adds parser support for DECALN,
`ESC # 8`, in `witty-core`.

## Implemented

`BasicTerminal` now handles:

- `ESC # 8`: screen alignment test, filling every cell in the active screen
  with a default-style `E`

The implementation uses the `vte` ESC intermediate field, so `ESC # 8` is
distinguished from the existing `ESC 8` cursor-restore control. DECALN follows
the behavior observed in the local Warp source: it replaces all cells on the
active grid with default cells whose text is `E`. It preserves the cursor
position, does not affect the inactive main/alternate screen, clears pending
wrap, follows the tail viewport, and marks full damage.

## Boundary

This is a pure `witty-core` parser/state update. It does not touch PTY transport,
browser runtime, renderer code, `wgpu`, Vulkan, Chromium, or native windows.

## Verification

Covered by:

- `cargo test -p witty-core decaln --quiet`
- `cargo test -p witty-core --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `cargo clippy -p witty-core --all-targets -- -D warnings`
- `cargo fmt --check`
- `git diff --check`

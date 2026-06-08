# Terminal Scroll Up/Down Controls

Updated: 2026-06-01

`m336-terminal-scroll-up-down-controls` adds xterm-compatible parser support for
the scroll-up and scroll-down CSI controls in `witty-core`.

## Implemented

`BasicTerminal` now handles:

- `CSI Ps S`: scroll the active scroll region up by `Ps` lines, defaulting to 1
- `CSI Ps T`: scroll the active scroll region down by `Ps` lines, defaulting to 1

Both controls operate on the effective scroll region, clamp the count to the
region height, clear pending wrap state, and preserve the cursor position. When
the effective region is the full screen, scroll-up contributes removed rows to
the existing scrollback path just like the shared region-scroll primitive.

## Boundary

This is a pure `witty-core` parser/state update. It does not touch PTY transport,
browser runtime, renderer code, `wgpu`, Vulkan, Chromium, or native windows.

## Verification

Covered by:

- `cargo test -p witty-core csi_scroll --quiet`
- `cargo test -p witty-core --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `cargo clippy -p witty-core --all-targets -- -D warnings`
- `cargo fmt --check`
- `git diff --check`

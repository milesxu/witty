# Terminal DECSTR Soft Reset

Updated: 2026-06-01

`m332-terminal-decstr-soft-reset` adds parser-level support for DECSTR,
`CSI ! p`, as a local-safe terminal compatibility slice.

## Behavior

`BasicTerminal` now treats `CSI ! p` as a soft reset. It resets runtime terminal
state that affects future output while preserving visible buffers:

- current SGR style
- active OSC 8 hyperlink attribute
- insert mode
- origin mode
- autowrap mode, restored to Witty's default-on policy
- application cursor-key mode
- application keypad mode
- top/bottom scroll region
- cursor shape, visibility, and blink state
- saved cursor state, reset to the upper-left cell
- OSC palette overrides and default foreground/background overrides

The reset intentionally does not clear main or alternate screen contents,
scrollback, title, tab stops, bracketed paste mode, mouse/focus modes, or
synchronized-output mode. `ESC c` remains the broader full-reset path for
screen and history clearing.

## Boundary

This is a pure `witty-core` parser/state change. It does not open a window,
request a `wgpu` adapter/device, launch Chromium, touch Vulkan, or change PTY
transport behavior.

## Verification

Covered by:

- `cargo test -p witty-core decstr --quiet`
- `cargo test -p witty-core --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `cargo clippy -p witty-core --all-targets -- -D warnings`
- `cargo fmt --check`
- `git diff --check`

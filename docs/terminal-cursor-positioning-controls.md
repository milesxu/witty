# Terminal Cursor Positioning Controls

Updated: 2026-06-01

`m334-terminal-cursor-positioning-controls` fills a small xterm-compatible
cursor-control gap in `witty-core`.

## Implemented

`BasicTerminal` now handles:

- `ESC 6`: back index, moving one column left
- `ESC 9`: forward index, moving one column right
- `ESC D`: index, equivalent to linefeed without carriage return
- `ESC E`: next line, carriage return plus linefeed
- `BS` (`0x08`): move one column left, or with `CSI ? 45 h` reverse-wraparound
  mode enabled, wrap from column 1 to the previous row's final column
- `CSI Ps E`: cursor next line, moving down and to column 1
- `CSI Ps F`: cursor previous line, moving up and to column 1
- `CSI Ps G`: cursor horizontal absolute
- `CSI Ps \``: horizontal position absolute
- `CSI Ps a`: horizontal position relative
- `CSI Ps d`: vertical position absolute
- `CSI Ps e`: vertical position relative

These controls share the existing viewport-following, pending-wrap clearing,
origin-mode row clamping, column clamping, and damage semantics used by the
existing cursor movement paths. Because Witty does not yet model horizontal
left/right margins, DECBI/DECFI are currently single-column cursor moves rather
than margin-scrolling operations.

For relative cursor movement counts (`CUU`, `CUD`, `CUF`, `CUB`, `CNL`, `CPL`,
`HPR`, and `VPR`), missing and explicit `0` parameters move one cell or line.

## Boundary

This is a pure parser/state update. It does not touch PTY transport, browser
runtime, renderer code, `wgpu`, Vulkan, Chromium, or native windows.

## Verification

Covered by:

- `cargo test -p witty-core cursor_next_previous_line --quiet`
- `cargo test -p witty-core horizontal_and_vertical_absolute --quiet`
- `cargo test -p witty-core relative_hpr_and_vpr --quiet`
- `cargo test -p witty-core index_and_next_line --quiet`
- `cargo test -p witty-core --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `cargo clippy -p witty-core --all-targets -- -D warnings`
- `cargo fmt --check`
- `git diff --check`

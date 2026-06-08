# Terminal Repeat Preceding Graphic

Updated: 2026-06-01

`m338-terminal-repeat-preceding-graphic` adds parser support for the xterm REP
control, `CSI Ps b`, in `witty-core`.

## Implemented

`BasicTerminal` now tracks the most recent printable graphic character and
handles:

- `CSI Ps b`: repeat that character `Ps` times

Missing and zero parameters default to one repeat. If no printable character
has been seen yet, REP is ignored. Repeats flow through the same `print_char`
path as ordinary terminal output, so autowrap, insert mode, wide-cell repair,
current style, hyperlink metadata, scrollback, cursor movement, and damage
tracking stay consistent with real printed characters.

Full reset and DECSTR soft reset clear the remembered character.

## Boundary

This is a pure `witty-core` parser/state update. It does not touch PTY transport,
browser runtime, renderer code, `wgpu`, Vulkan, Chromium, or native windows.

## Verification

Covered by:

- `cargo test -p witty-core repeat_preceding_graphic --quiet`
- `cargo test -p witty-core --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `cargo clippy -p witty-core --all-targets -- -D warnings`
- `cargo fmt --check`
- `git diff --check`

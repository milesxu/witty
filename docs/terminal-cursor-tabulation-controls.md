# Terminal Cursor Tabulation Controls

Updated: 2026-06-01

`m340-terminal-cursor-tabulation-controls` adds parser support for xterm-style
cursor tabulation controls in `witty-core`.

## Implemented

`BasicTerminal` now handles:

- `CSI Ps I`: cursor forward tabulation, moving to the `Ps`-th following tab
  stop
- `CSI Ps Z`: cursor backward tabulation, moving to the `Ps`-th preceding tab
  stop

Missing and zero parameters default to one tab stop. Forward tabulation clamps
to the rightmost column when no later tab stop exists. Backward tabulation
clamps to column zero when no earlier tab stop exists. Both controls use the
same default/custom tab stop set as `HT`, `HTS`, and `TBC`, and they clear
pending wrap state without changing screen contents.

## Boundary

This is a pure `witty-core` parser/state update. It does not touch PTY transport,
browser runtime, renderer code, `wgpu`, Vulkan, Chromium, or native windows.

## Verification

Covered by:

- `cargo test -p witty-core tabulation --quiet`
- `cargo test -p witty-core --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `cargo clippy -p witty-core --all-targets -- -D warnings`
- `cargo fmt --check`
- `git diff --check`

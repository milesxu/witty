# Terminal CSI Save Restore Cursor

Updated: 2026-06-01

`m344-terminal-csi-save-restore-cursor` adds parser support for the common
ANSI save/restore cursor controls in `witty-core`.

## Implemented

`BasicTerminal` now handles:

- `CSI s`: save the current active-screen cursor position
- `CSI u`: restore the saved cursor position when the saved cursor belongs to
  the current active screen

The implementation reuses the existing cursor-save state used by `ESC 7`,
`ESC 8`, and DEC private mode `?1048`. Restores remain screen-local and clamp
to the current grid size through the existing restore path. The shared saved
state now includes active charset, G0-G3 charset designations, current SGR
style, the DECSCA protected-character attribute, DECOM origin mode, DECAWM
autowrap mode, and pending autowrap state; see
`terminal-cursor-save-restore.md`.

## Boundary

This slice only handles default-parameter, no-intermediate `CSI s` and
`CSI u`; `vte` represents a missing CSI parameter as `0`, so `CSI 0 s` and
`CSI 0 u` follow the same path. Nonzero `CSI ... s` is left unclaimed for
future margin controls such as DECSLRM, and prefixed/intermediate `CSI u`
variants are left for Kitty keyboard protocol work.

This is a pure `witty-core` parser/state update. It does not touch PTY
transport, browser runtime, renderer code, `wgpu`, Vulkan, Chromium, or native
windows.

## Warp Cross-Check

The local Warp source maps no-intermediate `CSI s` and `CSI u` to save and
restore cursor handlers:

- `/home/mingxu/src/warp/app/src/terminal/model/ansi/mod.rs`
- `/home/mingxu/src/warp/app/src/terminal/model/grid/ansi_handler.rs`

Witty intentionally keeps the same basic behavior while also ignoring
nonzero `CSI s` parameters so future parameterized meanings stay available.

## Verification

Covered by:

- `cargo test -p witty-core csi_cursor --quiet`
- `cargo test -p witty-core --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `cargo clippy -p witty-core --all-targets -- -D warnings`
- `cargo fmt --check`
- `git diff --check`

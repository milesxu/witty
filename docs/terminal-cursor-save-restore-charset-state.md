# Terminal Cursor Save Restore Charset State

Updated: 2026-06-01

`m350-terminal-cursor-save-restore-charset-state` extends the existing
cursor-save slot so it preserves character set state in addition to cursor
position.

## Implemented

`BasicTerminal` now saves and restores:

- active screen
- cursor position
- active locking charset (`G0` or `G1`)
- G0-G3 charset designations

The behavior applies to all controls that share the same saved-cursor path:

- `ESC 7` / `ESC 8`
- `CSI ? 1048 h` / `CSI ? 1048 l`
- default `CSI s` / `CSI u`

Restoring a saved cursor clears pending `SS2`/`SS3` single-shift state. The
one-shot state is transient parser input state rather than saved cursor state;
after restore, future printable characters use the restored locking charset
until another explicit single-shift or locking shift arrives.

## Boundary

This slice intentionally remains a pure `witty-core` state update. It does not
save cursor style, cursor visibility, origin mode, autowrap mode, insert mode,
current SGR style, active hyperlink, palette state, PTY transport state,
renderer state, browser runtime state, `wgpu`, Vulkan, Chromium, or native
windows.

## Verification

Covered by:

- `cargo test -p witty-core cursor_charset --quiet`
- `cargo test -p witty-core csi_cursor --quiet`
- `cargo test -p witty-core dec_special_graphics --quiet`
- `cargo test -p witty-core --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `cargo clippy -p witty-core --all-targets -- -D warnings`
- `cargo fmt --check`
- `git diff --check`

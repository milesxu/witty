# Terminal G2/G3 Single Shift Charsets

Updated: 2026-06-01

`m348-terminal-g2-g3-single-shift-charsets` extends the DEC charset work from
G0/G1 locking shifts to G2/G3 single-shift invocation. The follow-up
`m410-terminal-g2-g3-locking-shifts` also supports locking invocation of G2 and
G3.

## Implemented

`BasicTerminal` now handles:

- `ESC * B`: designate G2 as ASCII
- `ESC * A`: designate G2 as UK national replacement charset
- `ESC * 0`: designate G2 as DEC special graphics
- `ESC + B`: designate G3 as ASCII
- `ESC + A`: designate G3 as UK national replacement charset
- `ESC + 0`: designate G3 as DEC special graphics
- `ESC N`: invoke G2 for the next printable character
- `ESC O`: invoke G3 for the next printable character
- 8-bit `SS2` (`0x8e`): invoke G2 for the next printable character
- 8-bit `SS3` (`0x8f`): invoke G3 for the next printable character
- `ESC n`: make G2 the active locking charset
- `ESC o`: make G3 the active locking charset

The single-shift state is consumed only when the next non-combining printable
character is written. The printable path still computes width from the original
codepoint, then maps the stored glyph through the selected charset, matching
the existing G0/G1 behavior.

`SI`, `SO`, `ESC n`, and `ESC o` switch the active locking charset. They also
clear any pending single-shift state because a later locking shift is more
explicit than an earlier one-shot selection.

`DECSTR` and full reset restore all four designations to ASCII, make G0 active,
and clear pending single-shift state.

## Boundary

This slice stays inside `witty-core` parser/state behavior. It does not add
UTF-8 mode policy, 96-character set variants, saved-cursor charset snapshots,
native box-drawing renderer primitives, PTY transport changes, browser runtime,
`wgpu`, Vulkan, Chromium, or native windows.

The UK national replacement charset maps `#` to `£`; see
`terminal-national-replacement-charsets.md`.

## Verification

Covered by:

- `cargo test -p witty-core dec_special_graphics --quiet`
- `cargo test -p witty-core ss2 --quiet`
- `cargo test -p witty-core --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `cargo clippy -p witty-core --all-targets -- -D warnings`
- `cargo fmt --check`
- `git diff --check`

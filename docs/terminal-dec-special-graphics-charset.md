# Terminal DEC Special Graphics Charset

Updated: 2026-06-01

`m346-terminal-dec-special-graphics-charset` adds parser support for the DEC
special graphics character set used by many older ncurses and vttest-style
line drawing paths.

## Implemented

`BasicTerminal` now handles:

- `ESC ( B`: designate G0 as ASCII
- `ESC ( A`: designate G0 as UK national replacement charset
- `ESC ( 0`: designate G0 as DEC special graphics
- `ESC ) B`: designate G1 as ASCII
- `ESC ) A`: designate G1 as UK national replacement charset
- `ESC ) 0`: designate G1 as DEC special graphics
- `SI` (`0x0f`): make G0 active
- `SO` (`0x0e`): make G1 active

Printable characters pass through the active charset before the existing cell
write path. The DEC special graphics mapping covers the common line drawing
glyphs such as `l`, `q`, `k`, `x`, `m`, and `j`, producing box-drawing
characters without changing renderer code.

`DECSTR` and full reset restore G0/G1 to ASCII and make G0 active.

## Boundary

The original m346 slice intentionally handled only G0/G1 designation and SI/SO
switching. Follow-up charset slices add G2/G3 designation, `SS2`/`SS3`
one-shot invocation, and `ESC n`/`ESC o` G2/G3 locking shifts; see
`terminal-g2-g3-single-shift-charsets.md`.
The UK national replacement charset is documented separately in
`terminal-national-replacement-charsets.md`.

UTF-8 mode selection sequences are consumed as no-ops separately in
`terminal-utf8-mode-selection.md`. This compatibility line still does not
implement native box-drawing renderer primitives, PTY transport changes,
browser runtime, `wgpu`, Vulkan, Chromium, or native windows.

## Warp Cross-Check

The local Warp source handles the same compatibility family:

- `/home/mingxu/src/warp/app/src/terminal/model/ansi/mod.rs` maps `SI`, `SO`,
  `ESC ( B`, `ESC ) B`, `ESC ( 0`, and `ESC ) 0`
- `/home/mingxu/src/warp/crates/warp_terminal/src/model/ansi/control_sequence_parameters.rs`
  carries the DEC special graphics character mapping

Witty keeps the first slice smaller by limiting active switching to G0/G1.

## Verification

Covered by:

- `cargo test -p witty-core dec_special_graphics --quiet`
- `cargo test -p witty-core --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `cargo clippy -p witty-core --all-targets -- -D warnings`
- `cargo fmt --check`
- `git diff --check`

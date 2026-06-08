# Terminal National Replacement Charsets

Updated: 2026-06-01

`BasicTerminal` supports the VT100 UK national replacement charset as a small
extension of the existing G0-G3 charset designation path:

- `ESC ( A`: designate G0 as UK national
- `ESC ) A`: designate G1 as UK national
- `ESC * A`: designate G2 as UK national
- `ESC + A`: designate G3 as UK national

The UK charset maps `#` to `£`; all other printable characters pass through as
ASCII. It works with the existing `SI`/`SO`, `SS2`/`SS3`, and G2/G3 locking
shift paths.

`DECSTR` soft reset and `ESC c` full reset restore all charset designations to
ASCII and clear pending single-shift state.

## Boundary

This is a narrow 94-character replacement set. Witty still does not
implement broader national replacement sets, 96-character sets, or UTF-8 mode
policy toggles.

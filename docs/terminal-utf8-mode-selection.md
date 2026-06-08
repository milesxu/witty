# Terminal UTF-8 Mode Selection

Updated: 2026-06-01

`m440-terminal-utf8-mode-selection-noop` makes historical character-encoding
selection sequences explicit no-ops in `witty-core`:

- `ESC % G`: select UTF-8
- `ESC % @`: select default/non-UTF-8 ISO 2022 encoding

Witty is UTF-8-only at the terminal-core boundary, matching the practical
policy used by modern terminals. The parser consumes these sequences without
printing `%`, `G`, or `@`, without changing the visible grid, and without
switching away from UTF-8 text handling.

The sequences also do not reset G0-G3 charset designation state. DEC special
graphics, UK national replacement charset, locking shifts, and single shifts
continue to use the existing charset state after `ESC % G` or `ESC % @`.

## Boundary

This does not add a non-UTF-8 decoding mode, locale switching, ISO-2022 state
machine, 96-character sets, or transport-level transcoding. Unsupported charset
designation forms remain ignored until they are implemented as separate
compatibility slices.

## Verification

- `cargo test -p witty-core utf8_mode_selection --quiet`
- `cargo test -p witty-core dec_special_graphics --quiet`
- `cargo test -p witty-core --quiet`

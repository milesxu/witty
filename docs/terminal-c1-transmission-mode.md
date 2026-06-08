# Terminal C1 Transmission Mode

Updated: 2026-06-01

`m442-terminal-c1-transmission-mode-noop` makes historical C1 transmission
selection sequences explicit no-ops in `witty-core`:

- `ESC SP F`: S7C1T, request 7-bit C1 transmission.
- `ESC SP G`: S8C1T, request 8-bit C1 transmission.

Witty accepts all supported compatibility forms at the terminal-core
boundary instead of switching a mutable transport mode. That means the parser
continues to accept:

- 7-bit C1 escape forms such as `ESC [` for CSI and `ESC ]` for OSC.
- Raw 8-bit C1 aliases for supported controls.
- UTF-8 encoded C1 control-code aliases such as `C2 9B` for CSI.

The `ESC SP F` and `ESC SP G` sequences are therefore consumed without printing
their bytes, changing the grid, or disabling any supported C1 alias form.

## Boundary

This does not add a user-visible C1 transmission preference, PTY transcoding,
or a mode-dependent reply encoder. Host replies continue to use existing
explicit byte sequences.

## Verification

- `cargo test -p witty-core c1_transmission_mode --quiet`
- `cargo test -p witty-core c1_ --quiet`

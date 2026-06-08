# Terminal DECSCL Conformance Level

Updated: 2026-06-01

`m448-terminal-decscl-selection-noop` makes the DECSCL conformance-level
selection sequence explicit in `witty-core`:

- `CSI Ps ; Ps " p`: select conformance level.

Witty consumes the sequence as a no-op. The terminal core does not switch
between VT100/VT200/VT300/VT400/VT500 emulation personalities, 7-bit versus
8-bit C1 transmission, or non-UTF-8 text modes at runtime.

The corresponding DECRQSS status query is supported:

- `DCS $ q " p ST` replies `DCS 1 $ r 65;1 " p ST`.

This fixed report advertises a VT500-level, 7-bit-compatible UTF-8 terminal
policy while keeping the parser behavior stable. Related C1 transmission
selection sequences are documented in `terminal-c1-transmission-mode.md`.

## Boundary

This does not implement DEC conformance-level state, non-UTF-8 decoding, a
VT-family personality switch, or mode-dependent reply encoding.

## Verification

- `cargo test -p witty-core decscl --quiet`
- `cargo test -p witty-core decrqss --quiet`

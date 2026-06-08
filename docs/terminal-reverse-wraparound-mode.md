# Terminal Reverse Wraparound Mode

Updated: 2026-06-01

`BasicTerminal` supports DEC reverse wraparound mode (`DECRWM`, private mode
`45`):

- `CSI ? 45 h`: enable reverse wraparound.
- `CSI ? 45 l`: disable reverse wraparound.
- `CSI ? 45 $ p`: report the current mode through the DEC
  request-mode-report path.

When enabled, a received BS control byte (`0x08`) at column 0 moves the cursor
to the previous row's final column. At the top row it still clamps at column 0.
When disabled, BS keeps the existing behavior and clamps at the left edge.

BS continues to clear pending autowrap state before moving the cursor. The mode
does not alter local Backspace key encoding; that remains controlled separately
by DECBKM (`?67`).

`DECSTR` soft reset and `ESC c` full reset clear the mode.

## Boundary

This is a parser/state-machine compatibility slice only. It does not touch PTY
transport, local key encoding, renderer code, native windows, Chromium, Vulkan,
or WebGPU.

## Verification

- `cargo test -p witty-core reverse_wraparound --quiet`
- `cargo test -p witty-core request_mode_report --quiet`
- `cargo test -p witty-core --quiet`

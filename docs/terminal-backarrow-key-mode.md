# Terminal Backarrow Key Mode

Updated: 2026-06-01

`BasicTerminal` supports DEC backarrow key mode (`DECBKM`, private mode `67`):

- `CSI ? 67 h`: Backspace encoders send BS (`0x08`).
- `CSI ? 67 l`: Backspace encoders send DEL (`0x7f`).
- `CSI ? 67 $ p`: report the current mode state through the existing DEC
  request-mode-report path.

The default and reset state is DEL, matching the existing Witty behavior.
`TerminalInputModes::backarrow_sends_backspace` carries this state into the
native and browser key encoders.

`DECSTR` soft reset and `ESC c` full reset restore the default DEL behavior.

## Boundary

This mode affects local keyboard encoding only. It does not change the behavior
of received BS/DEL bytes in terminal output, and it does not add modified
Backspace key encodings.

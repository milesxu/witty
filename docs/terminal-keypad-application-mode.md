# Terminal Keypad Application Mode

Updated: 2026-05-30

## Scope

`BasicTerminal` tracks keypad application mode:

- `ESC =` enables application keypad mode (`DECPAM`).
- `ESC >` disables application keypad mode (`DECPNM`).
- `CSI ? 66 h` enables application keypad mode (`DECNKM` set).
- `CSI ? 66 l` disables application keypad mode (`DECNKM` reset).
- `BasicTerminal::application_keypad_enabled()` exposes the current state for
  future keyboard-input encoding.
- Full terminal reset restores numeric keypad mode.

This task only tracks the terminal-controlled mode. Native and browser input
encoders still emit the existing fixed key sequences until a follow-up task wires
the mode into platform input handling.

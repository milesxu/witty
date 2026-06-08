# Terminal Cursor-Key Application Mode

Updated: 2026-05-30

## Scope

`BasicTerminal` tracks cursor-key application mode (`DECCKM`):

- `CSI ? 1 h` enables application cursor-key mode.
- `CSI ? 1 l` disables application cursor-key mode.
- `BasicTerminal::application_cursor_keys_enabled()` exposes the current state
  for input encoding.
- Full terminal reset restores normal cursor-key mode.

This task only tracks the terminal-controlled mode. Native and browser input
encoders still emit the existing fixed arrow-key sequences until a follow-up
integration task wires this state into platform input handling.

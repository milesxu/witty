# Terminal Cursor-Key Input Integration

Updated: 2026-05-30

## Scope

Native and browser arrow-key encoders now consume `TerminalInputModes` from the
local `BasicTerminal`:

- Normal cursor-key mode emits CSI arrow sequences: `ESC [ A/B/C/D`.
- Application cursor-key mode emits SS3 arrow sequences: `ESC O A/B/C/D`.

The shared mode value currently carries:

- `application_cursor_keys`
- `application_keypad`
- `backarrow_sends_backspace`

This task wires only cursor-key mode. Keypad application encoding remains a
separate task because it needs physical key or browser location metadata to
distinguish keypad keys from top-row characters.

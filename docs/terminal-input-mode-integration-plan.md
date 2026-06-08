# Terminal Input Mode Integration Plan

Updated: 2026-05-30

## Current State

`BasicTerminal` now tracks the terminal-controlled modes needed by keyboard
input encoding:

- `application_cursor_keys_enabled()` for `DECCKM` (`CSI ? 1 h/l`).
- `application_keypad_enabled()` for `DECPAM` / `DECPNM` (`ESC =` / `ESC >`).

Native and browser input encoders still emit fixed key sequences:

- Native: `witty-app/src/window.rs::encode_key_input`.
- Browser: `witty-web/src/lib.rs::encode_browser_key_input`.

## Mode Source

Use the local `BasicTerminal` as the mode source. The terminal is updated by
remote output before user key events are encoded, so no gateway protocol change
is needed for cursor-key mode.

For native window mode, `TerminalWindowApp::send_key` can read:

- `self.terminal.application_cursor_keys_enabled()`
- `self.terminal.application_keypad_enabled()`

For browser mode, `WittyWebSession::handle_key` can read the same state from
its `terminal` field.

## m94 Cursor-Key Integration

Add a small input-mode value shared by each encoder call:

```text
TerminalInputModes {
  application_cursor_keys: bool,
  application_keypad: bool,
  keyboard_locked: bool,
  backarrow_sends_backspace: bool,
}
```

Change arrow-key encoding only:

- Normal mode: `ArrowUp/Down/Right/Left` -> `ESC [ A/B/C/D`
- Application cursor-key mode: `ArrowUp/Down/Right/Left` -> `ESC O A/B/C/D`

Keep `Home`, `End`, `PageUp`, `PageDown`, and `Delete` unchanged in m94 unless a
separate compatibility task scopes xterm keyboard profiles.

Required native tests:

- `encode_key_input` keeps normal arrow sequences when mode is off.
- `encode_key_input` emits SS3 arrow sequences when cursor application mode is on.
- `encode_key_input` emits no terminal bytes while ANSI KAM is locked.
- `encode_key_input` emits BS for Backspace when DEC backarrow key mode is on.
- `TerminalWindowApp::send_key` passes the current terminal mode to the encoder.

Required browser tests:

- `encode_browser_key_input` keeps normal arrow sequences when mode is off.
- `encode_browser_key_input` emits SS3 arrow sequences when cursor application
  mode is on.
- `encode_browser_key_input` emits no terminal bytes while ANSI KAM is locked.
- `encode_browser_key_input` emits BS for Backspace when DEC backarrow key mode
  is on.
- Browser session key report or session smoke can drive `CSI ? 1 h` output into
  the terminal, then verify ArrowUp writes `ESC O A`.

## Keypad Integration

Keypad integration should be separate from m94 because native/browser key APIs
represent numpad keys differently:

- Native `winit` exposes logical keys and may need physical key inspection to
  distinguish top-row digits from keypad digits.
- Browser `KeyboardEvent.key` alone is not enough; `code` or `location` is
  needed to identify numpad keys reliably.

Proposed follow-up:

- Add an input event abstraction that carries logical key, text, control state,
  and optional physical/location metadata.
- Encode keypad digits/operators as numeric text in normal mode.
- Encode application keypad sequences in application mode, using xterm-compatible
  SS3 forms after confirming the exact key map.

## Verification Strategy

Run after m94:

- `cargo test -p witty-app -- --nocapture`
- `cargo test -p witty-web -- --nocapture`
- `cargo test --workspace`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `scripts/run-witty-web-smoke.sh`

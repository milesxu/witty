# Terminal Keypad Input Integration

Updated: 2026-05-30

## Scope

Native and browser key encoders now distinguish physical numpad input from
ordinary text before applying `TerminalInputModes::application_keypad`.

Normal keypad mode keeps existing behavior:

- top-row `1` emits `1`
- numpad `1` emits `1`
- main `Enter` emits carriage return
- numpad `Enter` emits carriage return

Application keypad mode now emits xterm/VT SS3 keypad sequences for supported
physical numpad keys:

| Keypad key | Sequence |
| --- | --- |
| `0` | `ESC O p` |
| `1` | `ESC O q` |
| `2` | `ESC O r` |
| `3` | `ESC O s` |
| `4` | `ESC O t` |
| `5` | `ESC O u` |
| `6` | `ESC O v` |
| `7` | `ESC O w` |
| `8` | `ESC O x` |
| `9` | `ESC O y` |
| `*` | `ESC O j` |
| `+` | `ESC O k` |
| `,` | `ESC O l` |
| `-` | `ESC O m` |
| `.` | `ESC O n` |
| `/` | `ESC O o` |
| `Enter` | `ESC O M` |

## Native Path

`witty-app` now builds a local `TerminalKeyInput` from `winit::event::KeyEvent`.
It uses:

- `KeyEvent::physical_key` for `KeyCode::Numpad*`
- `KeyEvent::location == KeyLocation::Numpad` as a fallback
- existing `logical_key` and `text` for normal special-key and text behavior

Application keypad sequences are emitted only for unmodified keypad input.
Control-modified keypad input keeps the existing control-character behavior.

## Browser Path

`witty-web/static/app.js` now passes these DOM fields into wasm:

- `event.code`
- `event.location`
- `event.shiftKey`
- `event.altKey`
- `event.metaKey`

The wasm encoder prefers `code == "Numpad*"` and accepts
`location == 3` as a fallback guard. It does not infer keypad identity from text
alone, so top-row digits remain ordinary text in application keypad mode.

## Deferred

- `NumpadEqual` falls back to text until its target sequence is verified.
- `kp5 = ESC O E` and no-NumLock navigation variants are still out of scope.
- Modified keypad application sequences are not implemented yet.

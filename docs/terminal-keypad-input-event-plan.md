# Terminal Keypad Input Event Plan

Updated: 2026-05-30

## Goal

Implement keypad application-mode input without confusing top-row characters
with physical numpad keys.

`BasicTerminal` already tracks `DECPAM` and `DECPNM` through
`TerminalInputModes::application_keypad`. The remaining gap is that native and
browser encoders currently receive only logical key/text values, which are not
enough to distinguish:

- top-row `1` from numpad `1`
- main `Enter` from numpad `Enter`
- locale text such as decimal comma from the physical decimal key

The next implementation should add source metadata before changing keypad
encoding behavior.

## Current Input Paths

Native path:

- `witty-app/src/window.rs::TerminalWindowApp::send_key`
- source event: `winit::event::KeyEvent`
- current encoder input: `logical_key`, `text`, `control`, `TerminalInputModes`

Browser path:

- `witty-web/static/app.js` canvas `keydown` listener
- `witty-web/src/lib.rs::WittyWebSession::handle_key`
- current encoder input: `event.key`, derived text, `event.ctrlKey`,
  `TerminalInputModes`

The byte transport and gateway protocol do not need to change. This is a
frontend-to-encoder metadata issue only.

## Required Event Shape

Add a neutral event shape at the encoder boundary. It should be small enough for
native and browser callers to construct directly.

```text
TerminalKeyInput {
  logical_key,
  text,
  modifiers,
  location,
  keypad_key,
}
```

Recommended fields:

- `logical_key`: the existing symbolic key value used by normal Enter, arrows,
  Home, End, Delete, and text fallback.
- `text`: printable text from the platform. This stays the normal-mode payload.
- `modifiers`: at least `control`; reserve `shift`, `alt`, and `meta` so future
  modifier encoding does not need another signature break.
- `location`: `standard`, `left`, `right`, `numpad`, or `unknown`.
- `keypad_key`: an optional normalized physical keypad key.

`keypad_key` should be derived from physical/code metadata, not from text. Text
is layout output; keypad application mode is a physical keypad mode.

## Native Mapping

Use `winit 0.30` metadata:

- `KeyEvent::location` identifies `KeyLocation::Numpad`.
- `KeyEvent::physical_key` can carry `PhysicalKey::Code(KeyCode::Numpad*)`.
- `KeyEvent::logical_key` and `KeyEvent::text` remain the normal text/special
  key inputs.

Native mapper rules:

- Treat `PhysicalKey::Code(KeyCode::Numpad*)` as authoritative when it maps to a
  supported keypad key.
- Also accept `KeyLocation::Numpad` for numpad `Enter` and decimal/operator
  fallbacks if the physical code is missing or platform-specific.
- Do not classify top-row digits or punctuation as keypad keys even when their
  text matches keypad output.

Initial supported physical keys:

- `Numpad0` through `Numpad9`
- `NumpadDecimal`
- `NumpadComma`
- `NumpadAdd`
- `NumpadSubtract`
- `NumpadMultiply`
- `NumpadDivide`
- `NumpadEnter`
- `NumpadEqual`

Leave `NumpadClear`, `NumpadBackspace`, memory keys, parentheses, hash, and star
variants out of m96 unless a source event and target sequence are verified.

## Browser Mapping

Pass browser metadata into wasm:

```js
session.handle_key(event.key, text, event.ctrlKey, event.code, event.location)
```

DOM values to use:

- `event.code`: physical code such as `Numpad1`, `NumpadEnter`, `NumpadDecimal`.
- `event.location`: `3` for numpad.
- `event.key`: logical key/text such as `1`, `Enter`, `Decimal`, or locale text.

Browser mapper rules:

- Prefer exact `event.code == "Numpad*"` mapping.
- Use `event.location == 3` as a guard or fallback, especially for `Enter` and
  decimal/operator keys.
- Keep `event.key` and derived `text` as normal-mode payloads.
- Do not infer keypad identity from text alone.

The public wasm function can keep backward compatibility for native tests by
using a helper that defaults `code` to empty and `location` to `0`.

## Application Keypad Sequence Map

Target xterm/VT keypad application sequences for unmodified keypad keys:

| Keypad key | Application sequence |
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

`NumpadEqual` should be deferred unless verified against target terminals. Some
terminfo entries do not advertise it alongside the classic VT keypad map.

`kp5 = ESC O E` and no-NumLock navigation variants should also be deferred. The
m96 target is keypad application mode for the physical numeric keypad, not a
full PC keypad profile.

## Encoding Order

Recommended encoder order:

1. If `modes.application_keypad` is enabled and `input.keypad_key` is supported,
   return the application keypad sequence.
2. Otherwise run existing special key handling: Enter, Tab, Backspace, Escape,
   arrows, Home, End, PageUp, PageDown, Delete.
3. Then run existing control-character handling.
4. Then fall back to `text`.

This preserves normal mode and avoids changing text behavior for top-row
characters, locale text, or unsupported keypad keys.

## Modifiers

m96 should handle only unmodified keypad application sequences plus the existing
control-character behavior. Do not invent xterm modified keypad sequences yet.

Rules for m96:

- `Control` with ordinary letters keeps current control-character behavior.
- `Control` with keypad keys should not produce application keypad sequences
  unless verified separately.
- `Alt`, `Shift`, and `Meta` should be carried in the event shape but not used
  for keypad sequence expansion yet.

## Tests For m96

Native unit tests:

- top-row `1` in application keypad mode still emits text `1`
- `Numpad1` in normal keypad mode emits text `1`
- `Numpad1` in application keypad mode emits `ESC O q`
- `Numpad0`, `Numpad9`, `NumpadDecimal`, `NumpadAdd`, `NumpadSubtract`,
  `NumpadMultiply`, `NumpadDivide`, and `NumpadEnter` emit the mapped sequences
- main `Enter` in application keypad mode still emits carriage return
- unsupported keypad keys fall back to existing text/special-key handling

Browser unit tests:

- `key="1"`, `code="Digit1"`, `location=0` stays text in application keypad mode
- `key="1"`, `code="Numpad1"`, `location=3` emits `ESC O q`
- `key="Enter"`, `code="NumpadEnter"`, `location=3` emits `ESC O M`
- an empty `code` with `location=0` preserves current browser encoder behavior

Mode integration tests:

- feed `ESC =` into `BasicTerminal`, then verify numpad input uses the
  application sequence
- feed `ESC >`, then verify the same input uses normal text/Enter behavior

Regression checks:

- `cargo test -p witty-app -- --nocapture`
- `cargo test -p witty-web -- --nocapture`
- `cargo test --workspace`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `scripts/run-witty-web-smoke.sh`

## m96 Implementation Scope

Recommended write set:

- `crates/witty-app/src/window.rs`
- `crates/witty-web/src/lib.rs`
- `crates/witty-web/static/app.js`
- focused tests in the same Rust files
- a short implementation note under `docs/`

Avoid changing:

- gateway protocol frames
- PTY transport
- plugin API
- renderer state
- `/home/mingxu/src/warp`

# Terminal Native IME Events

`m155-native-winit-ime-events` wires the IME/composition primitives from m154
into the native `winit` window shell.

## Result

Native window mode now opts into OS IME handling and routes committed IME text
to the active terminal transport as UTF-8 bytes. Preedit text remains local UI
state rendered through the IME overlay channel.

## Implementation

- `TerminalWindowApp` owns an `ImeComposition`.
- Window creation calls:
  - `Window::set_ime_allowed(true)`
  - `Window::set_ime_purpose(ImePurpose::Terminal)`
- `WindowEvent::Ime` is handled in the native event loop:
  - `Enabled` marks the local IME state enabled.
  - `Preedit(text, caret)` updates local preedit state and rebuilds the frame.
  - `Commit(text)` clears preedit state and writes non-empty committed text to
    the terminal input transport.
  - `Disabled` clears enabled/preedit state.
- `sync_ime_cursor_area()` maps the terminal cursor cell through current
  `CellMetrics` and calls `Window::set_ime_cursor_area()` so platform candidate
  UI can follow the terminal cursor.
- Opening the command palette or search clears terminal preedit state. While
  those overlays own text input, native IME commits are not sent to the PTY.

## Privacy Boundary

Preedit text is not fed into `BasicTerminal`, not sent to plugins, and not
recorded in snapshots. Only committed IME text is terminal input.

## Tests

Focused unit coverage in `witty-app` verifies:

- preedit updates enable IME state and do not write transport input.
- commit clears preedit and writes UTF-8 bytes once.
- empty commit clears preedit without writing bytes.
- events received while terminal text input is disabled clear preedit and write
  nothing.
- `Ime::Disabled` clears enabled/preedit state.

## Verification

- `cargo fmt`
- `cargo test -p witty-app ime --quiet`
- `cargo test --workspace --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `cargo clippy --workspace --all-targets -- -D warnings`

All listed checks passed on 2026-05-30.

## Next

`m156-browser-ime-input-shim` should add the browser-side hidden input shim,
composition event routing, wasm IME methods, and duplicate keydown suppression.

# Terminal Browser IME Input Shim

`m156-browser-ime-input-shim` adds the first browser-side IME/composition input
path for the WebGPU terminal.

## Result

Browser mode now uses a hidden focusable input as the keyboard/IME target while
keeping the canvas as the visual terminal surface. Composition preedit text is
held in wasm UI state and rendered through the same local IME overlay channel as
native mode. Only committed composition text is sent to the terminal gateway.

## Implementation

- `WittyWebSession` owns an `ImeComposition`.
- The wasm API now exposes:
  - `set_ime_preedit(text, caret_start, caret_end)`
  - `commit_ime_text(text)`
  - `clear_ime_preedit()`
  - `ime_is_active()`
  - `ime_preedit()`
  - IME cursor rectangle getters in CSS pixels.
- `render_current_frame()` applies `apply_ime_preedit_overlay()` after terminal
  frame planning when terminal text input is active.
- `static/index.html` includes a transparent, focusable `#witty-ime-input`
  positioned over the terminal cursor cell.
- `static/app.js` routes keyboard events through the hidden input, while mouse
  and rendering stay canvas-based.
- Browser composition handlers route:
  - `compositionstart`/`compositionupdate` to preedit state.
  - `compositionend` to commit text.
  - duplicate `beforeinput`/`input` events are suppressed after commit.
- `keydown` events with `event.isComposing` or key `Process` are prevented and
  never become terminal input bytes.
- Opening browser search clears terminal IME preedit; search/command-palette IME
  text routing remains a later task.

## Tests

Focused Rust tests verify:

- preedit updates local IME state without writing terminal input.
- commit clears preedit and writes UTF-8 once.
- empty commit clears preedit without writing input.
- disabled terminal text input clears preedit and ignores commit bytes.
- caret normalization and cursor CSS rectangle calculation.

The default Playwright browser smoke now includes a synthetic node-gateway IME
case:

- composition preedit appears in `window.wittyLastIme`.
- preedit emits no gateway input.
- commit for `你` emits one UTF-8 input frame.
- duplicate `beforeinput` after `compositionend` is suppressed.

## Verification

- `cargo fmt`
- `cargo test -p witty-web ime --quiet`
- `node --check crates/witty-web/static/app.js`
- `node --check scripts/run-witty-web-smoke.mjs`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `cargo test --workspace --quiet`
- `scripts/run-witty-web-smoke.sh`
- `cargo clippy --workspace --all-targets -- -D warnings`

All listed checks passed on 2026-05-30.

## Next

`m157-browser-ime-runtime-smoke` is complete. IME coverage now runs through the
node loopback, Rust gateway, and product launcher browser paths. See
`terminal-browser-ime-runtime-smoke.md`.

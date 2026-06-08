# Terminal IME Product Polish

Date: 2026-05-30

`m159-ime-product-polish` tightens the IME path after native, browser, search,
and command-palette routing landed.

## Behavior

- Native and browser terminal candidate positions now follow the IME preedit
  caret instead of always using the base terminal cursor.
- The candidate point is clamped to the visible grid, so long preedit text near
  the right edge does not ask the platform candidate UI to anchor past the last
  terminal cell.
- Search and command palette candidate placement continue to use the active
  overlay input cursor, now through the shared `ImeComposition` caret-width
  helper.
- Browser IME diagnostics expose the active text target, cursor rectangle,
  input mode, active element, preedit state, and written byte count through
  `window.wittyLastIme` and `window.wittyImeDiagnostics()`.

## Browser Soft Keyboard Notes

The browser shell still renders all terminal UI on the canvas, but the hidden
text input is now a more reliable mobile keyboard anchor:

- `inputmode=text` and `enterKeyHint=done` are set in JavaScript.
- the input uses a 16 px font size to avoid mobile browser zoom-on-focus
  behavior.
- the input rectangle is synchronized after canvas resize, gateway output,
  pointer focus, IME updates, and visual viewport resize/scroll.
- the rectangle is clamped to the visual viewport when mobile soft keyboards
  shrink the visible area.

This is not a full mobile terminal UX. It only preserves the correctness path
needed for browser IME and soft-keyboard text entry.

## Manual Validation Checklist

Native Linux:

1. Start a native window with an fcitx5 or ibus pinyin engine.
2. Type a pinyin preedit at the terminal prompt.
3. Verify the preedit overlay appears at the terminal cursor and candidate UI
   follows the preedit caret as the caret moves.
4. Commit a Chinese character and verify the shell receives the committed UTF-8
   text exactly once.
5. Open search with `Ctrl+Shift+F`, compose text, and verify the query updates
   locally without terminal input.
6. Open command palette with `Ctrl+Shift+P`, compose text, and verify the filter
   updates locally without plugin command arguments.

Browser desktop:

1. Run `witty --web` and open Chromium.
2. Compose pinyin in the terminal and in browser search.
3. Verify `window.wittyLastIme.target` reports `terminal` or `search`.
4. Verify `window.wittyLastIme.cursorRect` is finite and close to the active
   input cursor.
5. Confirm no gateway input appears for preedit or search-owned commit.

Browser mobile or responsive emulation:

1. Focus the canvas through a tap.
2. Verify the soft keyboard opens through the hidden input.
3. Compose text while the visual viewport is reduced.
4. Verify `window.wittyLastIme.cursorRect.clamped` is allowed to become
   `true`, but the rectangle remains finite and visible.
5. Commit text and verify it reaches the terminal or local search owner exactly
   once.

## Verification

Ran:

- `cargo fmt`
- `node --check crates/witty-web/static/app.js`
- `node --check scripts/run-witty-web-smoke.mjs`
- `cargo test -p witty-ui ime --quiet`
- `cargo test -p witty-app ime --quiet`
- `cargo test -p witty-web ime --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown`

Full workspace and browser smoke verification are tracked in the supervisor
result for `m159-ime-product-polish`.

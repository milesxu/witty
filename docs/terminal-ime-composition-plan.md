# Terminal IME / Composition Plan

This plan selects the next compatibility line after OSC palette support:
interactive IME/composition input for native and browser Witty sessions.
The immediate product reason is Chinese/Japanese/Korean input. The engineering
reason is that composition text is neither terminal output nor ordinary keydown
text, so it needs an explicit shell-layer boundary before the input stack grows.

## Current Baseline

Native input currently uses `winit` 0.30.13 `WindowEvent::KeyboardInput` and
encodes each pressed key through `encode_key_event_input()`.

Browser input now focuses a hidden text input shim beside the canvas and routes
DOM `keydown` through `WittyWebSession::handle_key()`, deriving printable
text from `event.key` when there is no control/meta modifier. Composition
events update wasm IME state and commit text through explicit IME methods. The
browser IME runtime smoke now covers the node loopback, Rust PTY gateway, and
product launcher paths.

Both paths already share terminal input modes for cursor keys, keypad,
function/navigation keys, mouse reporting, focus reporting, bracketed paste,
OSC 52 host actions, and shared IME preedit overlay primitives. Native window
mode now also handles `winit` IME events, and native search/command palette plus
browser search route IME commits into local UI state instead of the PTY.

## Platform Facts

Native:

- `winit` IME is opt-in. `Window::set_ime_allowed(true)` is required.
- `WindowEvent::Ime` has `Enabled`, `Preedit(String, Option<(usize, usize)>)`,
  `Commit(String)`, and `Disabled`.
- During preedit, winit does not send normal `KeyboardInput` events for those
  composing keystrokes.
- Candidate UI positioning should use `Window::set_ime_cursor_area()`.
- `ImePurpose::Terminal` exists, but is unsupported on many platforms; treat it
  as a best-effort hint, not a correctness dependency.

Browser:

- Canvas keydown alone is not enough for reliable IME composition.
- Browser composition should be driven by a focused hidden/transparent text
  input or contenteditable shim anchored to the terminal cursor.
- Use `compositionstart`, `compositionupdate`, `compositionend`, and
  `beforeinput`/`input` as the source of committed text. Suppress duplicate
  keydown handling while `event.isComposing` or a local composing flag is true.

## Design Boundary

IME state belongs in the shell/UI layer, not in `witty-core`.

Committed text is routed by the active text-input owner. Terminal commits are
written to the active transport as UTF-8 bytes. Search and command palette
commits update local query/filter state only. Preedit text is local UI state
only:

- it is not fed to `BasicTerminal`.
- it is not sent to plugins.
- it is not included in terminal snapshots.
- it is not added to search history before commit, command args, clipboard
  diagnostics, plugin events, or smoke logs.

The renderer should receive IME preedit as an explicit overlay after terminal
frame planning, similar to command/search overlays, rather than mutating grid
cells. This keeps the terminal model faithful to remote output while still
showing composition text at the cursor.

## Native Implementation Shape

1. On native window creation, call:
   - `window.set_ime_allowed(true)`
   - `window.set_ime_purpose(ImePurpose::Terminal)` as a best-effort hint.
2. Track `ImeCompositionState` in `TerminalWindowApp`:
   - enabled/disabled flag
   - current preedit string
   - optional caret byte range from winit
3. Add a helper that maps terminal cursor cell to window pixel coordinates using
   current `CellMetrics`, then calls `set_ime_cursor_area()`.
4. Handle `WindowEvent::Ime`:
   - `Enabled`: mark enabled and update cursor area.
   - `Preedit(text, caret)`: update local preedit state and rebuild frame.
   - `Commit(text)`: clear preedit and write `text.as_bytes()` to transport.
   - `Disabled`: clear preedit.
5. Keep normal key handling unchanged when no composition is active.
6. During composition, rely on winit's keyboard suppression and avoid injecting
   preedit text into the PTY.

## Browser Implementation Shape

1. Add a text input shim beside the canvas:
   - visually hidden or transparent, but focusable.
   - positioned near the terminal cursor for IME candidate placement.
   - kept synchronized after resize, cursor movement, and render.
2. Keep the canvas as the visual surface, but forward focus to the input shim
   when terminal input is active.
3. Add JS composition handlers:
   - `compositionstart`: mark composing.
   - `compositionupdate`: call wasm `set_ime_preedit(text)`.
   - `compositionend`: clear preedit and let `input`/`beforeinput` provide the
     commit path when possible; fall back to `event.data` if needed.
4. Add wasm methods:
   - `set_ime_preedit(text, caret_start, caret_end)`.
   - `commit_ime_text(text) -> bool`.
   - `clear_ime_preedit()`.
5. Ensure `keydown` ignores printable text while composing to avoid duplicate
   bytes.

## Overlay Rendering

First pass:

- render preedit text at the terminal cursor cell as a local overlay glyph run.
- draw an underline/background rectangle under the preedit span.
- hide or visually de-emphasize the terminal cursor while non-empty preedit is
  active.

The overlay can live initially in `witty-app` and `witty-web` composition helpers.
If duplicated logic grows, move `ImeCompositionState` and overlay planning into
`witty-ui`.

## Interaction Rules

Current scope:

- Terminal input receives IME commits only when command palette and search are
  closed.
- Native search and command palette render preedit inline in their overlay
  input text and position the OS candidate UI at the overlay input cursor.
- Browser search and browser command palette use the hidden input shim and wasm
  routing so local UI commits update query/filter state without gateway input.
- Mouse selection, hyperlink hover, diagnostics, and command palette overlays
  continue to render over terminal content; IME preedit should be close to the
  active input cursor and should not alter selection text.

Later scope:

- Revisit mobile soft keyboard behavior after desktop/browser composition is
  stable.

## Verification Plan

Deterministic unit tests:

- preedit update/clear state transitions.
- commit writes UTF-8 bytes exactly once.
- keydown is ignored while browser composing flag is active.
- overlay glyph/underline placement for single-cell and wide text.
- empty preedit clears overlay.

Native smoke:

- handler-level test using synthetic `Ime::Preedit` and `Ime::Commit` events or
  a small wrapper enum so CI does not need a real OS IME.

Browser smoke:

- Playwright dispatches composition events and verifies:
  - preedit appears in `window.wittyLastIme`.
  - no gateway input is emitted for preedit.
  - commit emits UTF-8 bytes once.
  - committed text appears through the node loopback echo path.

Manual smoke:

- Linux: fcitx5/ibus pinyin into native window.
- Browser: Chromium pinyin into `witty --web`.
- Verify candidate window near cursor, no duplicate latin letters, and correct
  committed Chinese text in shell.

## Follow-Up Task Split

1. `m154-ime-state-and-overlay-primitives`
   - done. Added shared `ImeComposition`, IME preedit overlay planning helpers,
     first-class `FramePlan::ime_preedit` rectangles, focused tests, workspace
     tests, wasm check, and clippy. See
     `terminal-ime-state-overlay-primitives.md`.
2. `m155-native-winit-ime-events`
   - done. Wired `set_ime_allowed`, `ImePurpose::Terminal`,
     `set_ime_cursor_area`, native `WindowEvent::Ime` handling, commit-to-PTY
     routing, preedit clearing while search/command palette own text input, and
     synthetic native tests. See `terminal-native-ime-events.md`.
3. `m156-browser-ime-input-shim`
   - done. Added DOM text input shim, wasm IME preedit/commit/clear methods,
     cursor-positioned input placement, duplicate keydown/beforeinput
     suppression, and node-gateway synthetic browser smoke coverage. See
     `terminal-browser-ime-input-shim.md`.
4. `m157-browser-ime-runtime-smoke`
   - done. Hardened Playwright composition smoke through node loopback, Rust
     gateway, and product launcher paths, and added a manual Chromium pinyin
     validation checklist. See `terminal-browser-ime-runtime-smoke.md`.
5. `m158-search-command-palette-ime-routing`
   - done. Routed native IME commits by active text owner, added inline preedit
     and candidate cursor positioning for native search/command palette, routed
     browser search IME commits into local search state without gateway input,
     and extended the browser runtime smoke across node/Rust/launcher paths.
     See `terminal-search-command-palette-ime-routing.md`.
6. `m159-ime-product-polish`
   - done. Candidate positioning now tracks preedit caret offsets and clamps to
     grid bounds, browser IME diagnostics expose target/cursor/input-mode state,
     the hidden input is more robust under mobile visual-viewport changes, and
     manual validation checklists are documented. See
     `terminal-ime-product-polish.md`.
7. `m161-browser-command-palette-shell`
   - done. Browser `Ctrl+Shift+P` now opens a local command palette shell,
     registers builtin/search/web commands, routes palette IME preedit/commit
     into local filter state without gateway input, and verifies the behavior
     across node, Rust gateway, and product launcher browser smokes. See
     `browser-command-palette-shell.md`.
8. `m162-browser-command-palette-visible-windowing`
   - done. Browser command-palette diagnostics and overlay now share a compact
     visible-window model, and selection movement keeps the selected command in
     view. See `browser-command-palette-shell.md`.
9. `m163-browser-renderer-glyphon-buffer-width`
   - done. Browser `glyphon` text buffers are sized to text-run display width
     instead of the remaining surface width, avoiding modal/PTY staging-buffer
     pressure in full smoke coverage. See `browser-command-palette-shell.md`.
10. `m164-browser-command-shortcuts`
   - done. Browser command-palette focus now invokes `F1`/`F2` shortcuts for
     the palette-labelled builtin and first non-builtin commands without
     stealing terminal function-key input while the palette is closed. See
     `browser-command-palette-shell.md`.

## Non-Goals

- Do not implement a terminal protocol for IME; terminal apps receive committed
  UTF-8 input only.
- Do not expose preedit/commit text to plugins.
- Do not add AI-assisted input, spellcheck, or text prediction.
- Do not solve mobile keyboard ergonomics before desktop/browser IME correctness.

# Terminal IME State And Overlay Primitives

`m154-ime-state-and-overlay-primitives` adds the shared foundation for
IME/composition input without wiring platform events yet.

## What Landed

- `witty-ui::ImeComposition`
  - tracks IME enabled state.
  - tracks local preedit text.
  - tracks optional byte-indexed caret range.
  - clears preedit on disable and commit.
  - returns committed text only when non-empty.
- `witty-ui::ime_preedit_overlay()`
  - converts active preedit state plus cursor/metrics/grid size into overlay
    background, underline, and glyph batches.
  - uses terminal cell width rules through `witty_core::terminal_char_width`.
  - clamps overlay width to the remaining visible grid width.
- `witty-ui::apply_ime_preedit_overlay()`
  - appends preedit rects/glyphs to a `FramePlan`.
  - hides the terminal cursor while preedit text is active.
- `witty-render-wgpu::FramePlan::ime_preedit`
  - gives IME preedit rectangles a first-class render channel instead of
    overloading selection/search/hyperlink rects.
  - includes IME rectangles in renderer vertex generation and frame stats.

## Boundary

This is still UI-shell state, not terminal-core state.

Preedit text is not fed to `BasicTerminal`, not written to the transport, and
not included in `RenderSnapshot`. Later platform workers will explicitly call
the overlay helper after terminal frame planning.

Committed text remains the only value that should be written to PTY/gateway
input.

## Verification

- `cargo fmt`
- `cargo test -p witty-ui ime --quiet`
- `cargo test --workspace --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `cargo clippy --workspace --all-targets -- -D warnings`

All checks passed on 2026-05-30.

## Next

`m155-native-winit-ime-events` should wire these primitives into native window
mode:

- enable IME on window creation.
- set `ImePurpose::Terminal` best-effort.
- update IME cursor area from terminal cursor position.
- handle `WindowEvent::Ime` preedit/commit/disable events.
- use `apply_ime_preedit_overlay()` during `TerminalWindowApp::rebuild_frame()`.

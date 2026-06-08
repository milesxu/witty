# Terminal Search / Command Palette IME Routing

Date: 2026-05-30

`m158-search-command-palette-ime-routing` closes the IME ownership gap left after
terminal input composition landed. IME commit text is now routed by the active
text input owner instead of assuming every commit is terminal input.

## Behavior

Native:

- Terminal-focused IME commits still write UTF-8 bytes to the PTY.
- Search-focused IME commits call `TerminalSearch::input_text()` and never write
  PTY bytes.
- Command-palette-focused IME commits call `CommandPalette::input_text()` and
  never write PTY bytes or plugin arguments.
- Search and command palette preedit text is rendered inline in the overlay
  input text without mutating the committed query/filter.
- The native IME candidate cursor area follows the active text owner:
  terminal cursor, find bar input cursor, or command palette title/query cursor.

Browser:

- Browser terminal IME commits still write gateway input bytes when search is
  closed.
- Browser search IME commits update local search state and emit no gateway input.
- Browser search status includes active preedit text for smoke/debug visibility
  while `search_query()` remains committed-only.
- Browser command palette routing is intentionally not implemented yet because
  the browser shell has no command palette surface.

## Privacy Boundary

Preedit text remains local UI state. It is not sent to:

- terminal output/grid state,
- PTY or browser gateway input,
- plugin events or command arguments,
- clipboard diagnostics.

Committed search/palette text updates only the owning UI model. It does not pass
through terminal input or plugin dispatch.

## Verification

Ran:

- `cargo fmt`
- `node --check crates/witty-web/static/app.js`
- `node --check scripts/run-witty-web-smoke.mjs`
- `cargo test -p witty-app ime --quiet`
- `cargo test -p witty-web ime --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `cargo test --workspace --quiet`
- `scripts/run-witty-web-smoke.sh`
- `WITTY_WEB_SMOKE_GATEWAY=rust scripts/run-witty-web-smoke.sh`
- `WITTY_WEB_SMOKE_GATEWAY=launcher scripts/run-witty-web-smoke.sh`
- `cargo clippy --workspace --all-targets -- -D warnings`

The browser runtime smoke now verifies search IME preedit/commit before terminal
IME commit across node loopback, Rust PTY gateway, and product launcher paths.

## Next

`m159-ime-product-polish` should focus on candidate positioning edge cases,
manual fcitx5/ibus/Chromium validation notes, status diagnostics, and mobile soft
keyboard behavior.

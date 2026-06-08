# Terminal Browser IME Runtime Smoke

`m157-browser-ime-runtime-smoke` hardens the browser IME coverage added in
`m156-browser-ime-input-shim`.

## Result

The synthetic browser IME smoke now runs through all browser gateway modes:

- node loopback gateway.
- Rust `witty-gateway` PTY gateway.
- product launcher path via `witty --web`.

This proves the browser hidden-input composition path is not limited to the
node test harness. The same Playwright case now validates preedit state,
commit bytes, duplicate event suppression, and nonblank rendering while using
the same browser surface a product user exercises.

## Runtime Coverage

The smoke dispatches browser composition events against `#witty-ime-input`:

- `compositionstart`
- composing `keydown` with `isComposing`
- `compositionupdate` with preedit text `ni`
- `compositionend` with committed text `你`
- duplicate post-composition `beforeinput`

Assertions:

- preedit text updates `window.wittyLastIme`.
- preedit emits no terminal/gateway input.
- composing keydown is prevented.
- commit clears preedit and writes exactly one UTF-8 payload for `你`.
- duplicate `beforeinput` after commit is prevented and does not write a
  second payload.
- the hidden input value is cleared after commit.

## Gateway Modes Verified

- `scripts/run-witty-web-smoke.sh`
- `WITTY_WEB_SMOKE_GATEWAY=rust scripts/run-witty-web-smoke.sh`
- `WITTY_WEB_SMOKE_GATEWAY=launcher scripts/run-witty-web-smoke.sh`

The Rust gateway and launcher modes intentionally still skip some node-only
byte-inspection smokes, but IME now runs in all three modes.

## Manual Chromium Pinyin Checklist

Use this checklist for a real OS/browser IME pass:

1. Start the product browser launcher:

   ```bash
   cargo run -p witty-app -- --web
   ```

2. Open the printed Chromium URL, or use `--open-browser`.
3. Focus the terminal canvas.
4. Switch the OS input method to Chinese pinyin in Chromium.
5. Type a pinyin preedit such as `ni`.
6. Verify the preedit overlay appears at the terminal cursor and is not echoed
   into the shell.
7. Verify the OS candidate window appears near the terminal cursor.
8. Commit `你`.
9. Verify the shell receives one committed Chinese character and does not
   receive duplicate latin preedit text.
10. Open browser search with `Ctrl+Shift+F`; for now, terminal IME preedit
    should clear because search owns text input. Search/command-palette IME
    routing is a follow-up task.

## Verification

- `node --check scripts/run-witty-web-smoke.mjs`
- `node --check crates/witty-web/static/app.js`
- `scripts/run-witty-web-smoke.sh`
- `WITTY_WEB_SMOKE_GATEWAY=rust scripts/run-witty-web-smoke.sh`
- `WITTY_WEB_SMOKE_GATEWAY=launcher scripts/run-witty-web-smoke.sh`

Full workspace verification for this milestone also includes:

- `cargo fmt`
- `cargo test -p witty-web ime --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `cargo test --workspace --quiet`
- `cargo clippy --workspace --all-targets -- -D warnings`

All listed checks passed on 2026-05-30.

## Next

`m158-search-command-palette-ime-routing` should add IME routing for browser
and native search/command-palette text fields instead of clearing terminal
preedit whenever those overlays take focus.

# Terminal OSC 52 Browser Clipboard

Updated: 2026-05-30

m140 connects terminal-core OSC 52 host actions to the browser shell. The
browser still applies an explicit JavaScript policy before any Clipboard API
call.

## Implemented Surface

`WittyWebSession` exposes:

```rust
pub fn drain_clipboard_write_actions_json(&mut self) -> Result<String, JsValue>;
```

The method drains terminal-core clipboard write host actions and returns a
compact JSON array of sanitized write requests. The payload still does not enter
terminal cells, scrollback, render plans, search state, plugin events, command
arguments, or gateway input frames.

Browser policy:

```text
disabled | confirm | allow
```

Behavior:

- default is `disabled`.
- `disabled` records `denied` with reason `policy-disabled` and does not call
  `writeText()`.
- `confirm` records `denied` with reason `policy-confirm-unimplemented` until a
  real browser confirmation flow exists.
- `allow` is the only policy that attempts `clipboard.writeText()`.
- browser OSC 52 supports only the normal clipboard target; primary selection
  records `unsupported`.
- Clipboard API failures are recorded as `permission-error` without including
  the payload in diagnostics.

`crates/witty-web/static/app.js` exposes smoke helpers:

```text
window.wittyOsc52ClipboardPolicy()
window.wittySetOsc52ClipboardPolicy(policy)
window.wittyOsc52ClipboardResults
window.wittyLastOsc52ClipboardResults
```

The smoke harness can also set `window.wittyClipboardApi` to a narrow test
stub. Normal clipboard shortcuts still default to `navigator.clipboard`.

## Verification

Passed:

- `cargo fmt --all`
- `cargo test -p witty-web browser_osc52 --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `scripts/run-witty-web-smoke.sh`
- `cargo test -p witty-web --quiet`
- `cargo clippy -p witty-web --all-targets -- -D warnings`
- `cargo fmt --all -- --check`

The Playwright node-gateway smoke verifies:

- browser default policy is `disabled`.
- `disabled` records denial and does not call the clipboard stub.
- `allow` records a write through a stubbed clipboard.
- browser primary target is reported as unsupported.
- OSC 52 payload text is not visible in terminal screen text.
- OSC 52 clipboard handling sends no gateway input bytes.

## Follow-Up

`m141-real-tui-compatibility-smoke-plan` is complete. See
`real-tui-compatibility-smoke-plan.md` for the layered real-application smoke
strategy and follow-up task queue.

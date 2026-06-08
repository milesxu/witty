# Terminal Browser Hyperlink Activation

Updated: 2026-05-30

m135 adds browser OSC 8 hyperlink hover and activation. It uses the same core
URL policy as native activation and verifies behavior through the Playwright
browser smoke.

## What Changed

- Moved external URL validation into `witty-core` so native and browser paths
  share the same allowlist.
- Added browser hyperlink activation target hit-testing from render snapshot
  metadata.
- Wired browser hover updates into `RenderSnapshot::hovered_hyperlink`.
- Added `Ctrl+LeftClick` / `Meta+LeftClick` activation in `app.js`.
- Opened allowed links with
  `window.open(uri, "_blank", "noopener,noreferrer")`.
- Added Playwright smoke coverage with a stubbed `window.open`.

## Policy

The browser activation path uses `witty_core::validate_external_url()`.

Allowed initial schemes:

- `http`
- `https`
- `mailto`

Rejected links still consume the explicit activation click so terminal
applications do not receive a modified click that the user intended as a local
open action.

## Smoke Coverage

The browser smoke injects an OSC 8 hyperlink into the terminal, dispatches a
hover event, then dispatches `Ctrl+LeftClick` on the linked cells. It verifies:

- hover rendering does not prevent pointer defaults.
- activation prevents the pointer default.
- `window.open` receives the expected URI, target, and `noopener` feature.
- no gateway input frame is sent by hyperlink activation.

## Verification

- `cargo fmt --all -- --check`
- `cargo test -p witty-core external_url --quiet`
- `cargo test -p witty-launcher external_url --quiet`
- `cargo test -p witty-web hyperlink --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `node --check scripts/run-witty-web-smoke.mjs`
- `scripts/run-witty-web-smoke.sh`
- `cargo test --workspace --quiet`
- `cargo clippy --workspace --all-targets -- -D warnings`

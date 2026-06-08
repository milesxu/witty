# Browser Document Title Bridge

Updated: 2026-05-30

## Scope

The browser session now mirrors terminal title state into `document.title`.
Terminal title state comes from `TerminalApp::title()`, which is populated by
`BasicTerminal` OSC `0`/`2` parsing.

## Behavior

Every wasm render pass updates the browser document title:

- terminal title present and non-empty: use the terminal-provided title
- no title or empty title: use `Witty`

The wasm `WittyWebSession::title()` export returns the same title selection
used for `document.title`, which keeps JS-side diagnostics and smoke harnesses
able to inspect the bridge.

## Runtime Smoke

`scripts/run-witty-web-smoke.sh` now sends `OSC 2` title output through each
browser gateway smoke path:

- Node loopback WebSocket gateway.
- Rust PTY-backed `witty-gateway`.
- Product `witty --web` launcher mode.

The Playwright smoke waits for Chromium to report the expected
`document.title` and checks that `window.wittySession.title()` returns the
same value. This verifies the full gateway-output to terminal-parser to browser
document bridge at runtime.

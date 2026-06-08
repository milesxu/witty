# Browser Alternate Screen Runtime Smoke

Updated: 2026-05-30

## Scope

`scripts/run-witty-web-smoke.sh` now covers a scripted alternate-screen
application over the browser gateway paths.

The smoke output sequence is:

1. Print main-screen startup text.
2. Enter `CSI ? 1049 h`.
3. Render `WITTY ALT SCREEN SMOKE` on the alternate screen.
4. After browser input, leave alternate screen with `CSI ? 1049 l`.
5. Render `WITTY MAIN SCREEN RESTORED` on the restored main screen.

## Assertions

The wasm session exposes `screen_text()` for smoke diagnostics. Playwright
asserts:

- the OSC title bridge still updates Chromium `document.title`.
- the alternate-screen marker is visible before input.
- the main-screen restore marker is visible after gateway output.
- the alternate-screen marker is no longer visible after restore.

The same assertions run against the Node loopback gateway, Rust PTY-backed
gateway, and product `witty --web` launcher mode.

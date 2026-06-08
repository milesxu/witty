# Browser Runtime Smoke Harness

Updated: 2026-05-29

## Purpose

This harness packages the `witty-web` wasm entry, serves a minimal HTML page, opens it in Chromium through Playwright, sends real browser keyboard input to the wasm session, and verifies that the canvas is nonblank.

## Files

| path | purpose |
| --- | --- |
| `crates/witty-web/static/index.html` | minimal page with `#witty-canvas` |
| `crates/witty-web/static/smoke.js` | loads wasm-bindgen output and calls `witty_start_canvas` |
| `scripts/build-witty-web-smoke.sh` | builds `witty-web.wasm`, copies a local smoke font, and runs `wasm-bindgen --target web` |
| `scripts/run-witty-web-smoke.sh` | builds the page and checks required Node/Playwright tools |
| `scripts/run-witty-web-smoke.mjs` | serves the page and performs the Playwright canvas nonblank check |

## Required Tools

The current code expects:

```bash
cargo install wasm-bindgen-cli --version 0.2.122 --locked
npm install --prefix target/witty-web-smoke-tools --no-save playwright
target/witty-web-smoke-tools/node_modules/.bin/playwright install chromium
```

The wasm renderer needs an explicit font because browser builds do not have
filesystem font discovery. By default the build script uses the repository
bundled DejaVu Sans Mono asset. Set
`WITTY_WEB_SMOKE_FONT=/path/to/monospace.ttf` for local override builds.

If the headless shell download is incomplete but Playwright already downloaded
full Chromium, `scripts/run-witty-web-smoke.sh` will use the newest cached
`*/chrome-linux/chrome` executable. You can also set
`WITTY_CHROMIUM_EXECUTABLE=/path/to/chrome` explicitly.

Then run:

```bash
scripts/run-witty-web-smoke.sh
```

## Verification Boundary

The harness is deterministic once tools are installed. It does not require a real PTY or WebSocket gateway; the page renders the browser frame produced by `witty-web` and verifies that key events reach `BrowserGatewayTransport` as outbound gateway bytes.

After `m53-browser-transport-boundary`, the runtime smoke also pushes a
synthetic gateway output message back into the terminal before the nonblank
canvas screenshot.

After `m54-browser-resize-dpi`, the page also synchronizes CSS canvas size and
`devicePixelRatio` into the wasm session. The Rust side resizes the wgpu
surface, rescales cell metrics, resizes the terminal grid and transport, and
then renders a fresh frame.

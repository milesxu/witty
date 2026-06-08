# Gateway Session Token Launch Flow

Updated: 2026-05-29

## Purpose

This plan defines how a native Witty launcher should start the local browser
terminal flow without exposing an unauthenticated shell gateway.

The browser build cannot spawn a PTY or bind a local socket. A native companion
must own the PTY gateway, generate a per-session secret, serve or open the
browser UI, and hand the browser only the minimum connection details needed for
that one session.

## Current State

- `witty-app --window` is native desktop-only and uses
  `TerminalApp<LocalPtyTransport>` directly.
- `witty-web` is browser/wasm and uses `TerminalApp<BrowserGatewayTransport>`.
- `witty-gateway` owns `LocalPtyTransport` and a single WebSocket session in
  `--once` mode.
- `scripts/run-witty-web-smoke.mjs` currently acts as the ad hoc launcher:
  - starts an HTTP server for `target/witty-web-smoke`
  - optionally starts `witty-gateway`
  - builds the gateway URL and token in Node
  - asks browser JavaScript to connect

The next product-shaped step is to move launcher ownership from the smoke
script into a native Rust boundary.

## Required Properties

- Bind the web UI and gateway to loopback by default.
- Use OS-selected ports by default to avoid fixed-port races.
- Generate a high-entropy token per launch.
- Configure `witty-gateway` with exact allowed browser origin.
- Keep command/program selection in the native launcher, not in browser frames.
- Avoid putting the token in the page URL query string.
- Expire the session if the browser never connects.
- Kill the PTY when the browser closes or the launcher exits.
- Keep browser/wasm crates free of native PTY, process, and socket-launch code.

## Proposed Native Components

Add a native launch layer after this planning milestone. It can start as a small
crate or as a `witty-app --web` mode; a separate crate keeps dependencies cleaner.

Suggested crate:

```text
crates/witty-launcher/
  src/lib.rs
  src/main.rs
```

Suggested responsibilities:

- bind loopback HTTP listener on `127.0.0.1:0`
- bind loopback gateway listener on `127.0.0.1:0`
- generate session id and token
- build `GatewayConfig` with:
  - `bind = gateway_listener.local_addr()`
  - `token = <generated token>`
  - `allowed_origins = ["http://127.0.0.1:<ui-port>"]`
  - inherited frame/backpressure limits
  - launcher-selected shell/program
- run `witty-gateway` in a task or child process
- serve static `witty-web` assets plus a per-session config endpoint
- optionally open the browser to the local web URL
- stop gateway and web server on process exit

For the first implementation, prefer in-process Rust tasks/threads over
spawning `cargo run`. A child-process mode can remain useful for tests, but a
product launcher should not depend on Cargo.

## Token And URL Handoff

Use a two-part handoff:

1. Browser navigates to the UI page:

```text
http://127.0.0.1:<ui-port>/index.html#session=<session-id>
```

2. Browser JavaScript fetches session config from the same origin:

```text
GET /session/<session-id>.json
```

Example response:

```json
{
  "protocol": 1,
  "gateway_url": "ws://127.0.0.1:49153/witty",
  "token": "base64url-or-hex-random-token",
  "expires_at_ms": 180000
}
```

The browser then connects to:

```text
ws://127.0.0.1:49153/witty?token=<token>
```

Rationale:

- The HTTP page URL contains only a random session id, not the gateway token.
- The token is served from the same loopback origin as the page.
- The gateway still validates both exact `Origin` and token on WebSocket
  upgrade.
- The browser never sends a requested command/program over the protocol.

The session config endpoint should return:

```text
Cache-Control: no-store
Content-Type: application/json
```

The first version can keep the config readable until gateway connect or timeout.
Later versions should make it one-use if local hostile-process risk becomes a
priority.

## Session Lifecycle

```text
native launcher starts
  -> bind UI listener on 127.0.0.1:0
  -> bind gateway listener on 127.0.0.1:0
  -> generate session id and token
  -> start gateway with exact allowed Origin and token
  -> serve witty-web assets and /session/<id>.json
  -> open or print http://127.0.0.1:<ui-port>/index.html#session=<id>
browser loads page
  -> reads session id from URL fragment
  -> fetches /session/<id>.json from same origin
  -> opens WebSocket to gateway_url?token=<token>
  -> sends hello and resize
gateway accepts
  -> validates Origin and token
  -> returns ready
  -> spawns PTY on first resize/input
session ends
  -> browser close, gateway exit, PTY exit, idle timeout, or launcher exit
  -> drop LocalPtyTransport and stop serving the session config
```

## Token Generation

Use OS randomness in native code. A minimal implementation can use `getrandom`
and hex encoding to avoid pulling in a larger RNG stack:

```text
32 random bytes -> 64 lowercase hex characters
```

Security target for v1:

- at least 128 bits of entropy; 256 bits is cheap
- token scoped to one gateway launch
- token never persisted to disk
- token comparison remains exact string equality in `witty-gateway`

## Browser Changes

Current `witty-web/static/smoke.js` accepts a gateway URL from Playwright through
`window.wittyConnectGateway(gatewayUrl)`.

Product browser entry should add:

- `wittyLoadSessionConfig()`:
  - parse `location.hash`
  - fetch `/session/<id>.json`
  - validate `protocol`
  - build gateway URL with token
  - call the existing WebSocket connect path
- a clear error render path when session config is missing, expired, or protocol
  mismatched
- no command/program fields in the browser config

Keep the smoke hook for deterministic tests, but do not make it the only
connection path.

## Gateway Changes Needed Before Implementation

`witty-gateway` currently binds its own listener inside `run_once(config)`. A
launcher that binds port `0` first needs one of these shapes:

Preferred:

```text
run_once_on_listener(listener: TcpListener, config: GatewayConfig)
```

Alternative:

```text
GatewayServer::new(config) -> bound listener + local_addr + run_once()
```

The preferred helper is smaller and lets the launcher own both port selection
and lifecycle. `GatewayConfig.bind` can remain for CLI parsing.

## HTTP Server Shape

For m62, avoid a full web framework unless routing grows. A tiny native server
can handle:

- `GET /index.html`
- `GET /smoke.js` or future `app.js`
- `GET /pkg/witty_web.js`
- `GET /pkg/witty_web_bg.wasm`
- `GET /fonts/witty-mono.ttf`
- `GET /session/<session-id>.json`

All responses should be local-only and can be served from `target/witty-web-smoke`
in the first launcher smoke. A later packaging task can embed assets into the
binary or serve from an install directory.

## Test Plan

Unit tests:

- token generator returns the expected length and non-identical values
- session URL includes only the session id, not the token
- session config JSON contains gateway URL, token, protocol, expiry
- gateway config uses exact UI origin
- missing or expired session id returns an error response

Integration smoke:

1. Build `witty-web` wasm bundle.
2. Start the native launcher on loopback port `0`.
3. Navigate Playwright to the printed UI URL.
4. Browser fetches session config and connects to gateway.
5. Assert `ready`, initial resize, `shell ready`, typed `xy`, and `pty saw:xy`.
6. Verify canvas screenshot is nonblank.

Negative smoke:

- bad token is rejected by gateway
- wrong origin is rejected by gateway
- session config cannot request a different command

## m62 Implementation Scope

Recommended next worker: `m62-gateway-launcher-spike`.

Write scope:

- `crates/witty-gateway/src/lib.rs`
  - add `run_once_on_listener`
- workspace `Cargo.toml`
  - add a small native launcher crate if chosen
- `crates/witty-launcher/`
  - token/session model
  - tiny loopback static/config server
  - gateway launch glue
  - CLI smoke mode
- `crates/witty-web/static/smoke.js`
  - add session config loading while preserving existing test hook
- `scripts/run-witty-web-smoke.mjs`
  - optional launcher mode, or add a new launcher smoke script

Non-goals for m62:

- multi-session management
- remote SSH gateway
- TLS
- browser-selected commands
- binary WebSocket framing
- packaging static assets into release installers

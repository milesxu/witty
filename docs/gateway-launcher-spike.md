# Gateway Launcher Spike

Updated: 2026-05-29

## Purpose

This milestone adds the first native launcher path for browser Witty
sessions. It turns the previous smoke-script-only orchestration into a Rust
boundary that owns session token generation, loopback HTTP serving, and
`witty-gateway` startup.

## New Native Crate

`crates/witty-launcher` provides:

- `LauncherConfig`
- `LaunchSession`
- `BrowserSessionConfig`
- `parse_config(args)`
- `run(config)`
- `build_launch_session(ui_addr, gateway_addr)`
- `browser_session_config_json(session)`

The CLI shape is:

```text
witty-launcher --web-root target/witty-web-smoke --program /bin/sh --arg -lc --arg '<script>'
```

By default it binds both the UI server and gateway to `127.0.0.1:0`.

## Session Handoff

At startup the launcher:

1. binds a loopback UI listener
2. binds a loopback gateway listener
3. generates a 16-byte hex session id
4. generates a 32-byte hex gateway token
5. starts `witty-gateway` with exact allowed origin and token
6. prints a browser URL:

```text
http://127.0.0.1:<ui-port>/index.html#session=<session-id>
```

The browser loads:

```text
GET /session/<session-id>.json
```

and receives:

```json
{"protocol":1,"gateway_url":"ws://127.0.0.1:<gateway-port>/witty","token":"...","expires_at_ms":180000}
```

The browser then connects to:

```text
ws://127.0.0.1:<gateway-port>/witty?token=<token>
```

The page URL contains only the session id. The gateway still validates both
exact `Origin` and token.

## Gateway Helper

`witty-gateway` now exposes:

```text
run_once_on_listener(listener: TcpListener, config: GatewayConfig)
```

This lets a launcher bind port `0`, discover the assigned port, and pass the
already-bound listener into the gateway. The existing CLI still uses
`run_once(config)`.

## Browser Entry

`crates/witty-web/static/smoke.js` now supports both paths:

- existing test hook: Playwright calls `window.wittyConnectGateway(url)`
- launcher path: page hash contains `#session=<id>`, JS fetches session config
  and connects automatically

The browser still does not receive a command/program field.

## Smoke Coverage

`scripts/run-witty-web-smoke.mjs` supports:

```text
WITTY_WEB_SMOKE_GATEWAY=node
WITTY_WEB_SMOKE_GATEWAY=rust
WITTY_WEB_SMOKE_GATEWAY=launcher
```

The launcher mode spawns:

```text
cargo run -p witty-app -- --web --web-root target/witty-web-smoke --program /bin/sh --arg -lc --arg '<script>'
```

Then Playwright visits the printed `index.html#session=...` URL and verifies the
same terminal roundtrip as the Rust gateway smoke.

The lower-level `witty-launcher` binary remains available for direct crate
testing, but `witty --web` is the product-facing browser launcher entry.

## Current Limits

- The launcher serves only a fixed allowlist of smoke/build assets.
- The session config endpoint is one-use after `m65-session-config-one-use`.
- There is no packaged asset embedding.
- There is no native browser-open command yet.
- HTTP serving is intentionally minimal and synchronous.
- Gateway lifecycle is still single-session.

## Next Work

Completed follow-ups:

- `m63-launcher-browser-smoke` added negative auth/origin tests.
- `m64-launcher-product-mode-decision` selected `witty --web` as the
  product entry while retaining `witty-launcher` as an internal crate boundary.

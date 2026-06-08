# Launcher Browser Smoke Hardening

Updated: 2026-05-29

## Purpose

This milestone tightens coverage around the native launcher path added in
`m62`.

## Added Coverage

Gateway handshake tests now cover:

- missing token
- wrong token
- non-loopback default origin rejection
- exact origin matching when `allowed_origins` is configured

Launcher tests now cover:

- the generated page URL contains only `#session=<id>`, not the gateway token
- generated gateway config uses the exact UI origin and session token
- command/program settings stay launcher-owned

The Playwright smoke runner now fails launcher mode if the printed page URL
contains `token=`.

## Remaining Negative Smoke Gaps

Still worth adding later:

- real browser wrong-token WebSocket attempt
- real browser wrong-origin WebSocket attempt
- launcher process exits when the browser/gateway session ends

Covered after `m65-session-config-one-use`:

- one-use session config endpoint behavior

# Launcher Session Config One-Use

Updated: 2026-05-30

## Purpose

The launcher session config endpoint carries the gateway URL and per-session
token. It should be readable only by the browser startup path and should not
remain reusable for later local reads.

## Behavior

`GET /session/<id>.json` now has one successful read:

- first read before timeout: `200 OK` with JSON config
- second or later read: `410 Gone`
- first read after timeout: `410 Gone`

The response keeps `Cache-Control: no-store`.

The browser page still receives only `#session=<id>` in the URL. It fetches the
config once, then connects to:

```text
ws://127.0.0.1:<gateway-port>/witty?token=<token>
```

## Boundary

The gateway token still remains valid for the WebSocket upgrade after the config
read. One-use applies to the HTTP handoff endpoint, not to the gateway token
itself. The gateway continues to require both:

- exact `Origin`
- matching token query parameter

## Verification

The launcher browser smoke now makes a second HTTP request to the same
`/session/<id>.json` endpoint after the browser has loaded. It fails unless the
launcher returns `410 Gone`.

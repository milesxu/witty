# Launcher Product Mode Decision

Updated: 2026-05-30

## Decision

The product-facing browser terminal entry is:

```text
witty --web
```

The implementation keeps `witty-launcher` as a separate native crate and has
`witty-app` delegate `witty --web` into that crate.

## Rationale

- Users get one main binary: `witty --window` for native window mode and
  `witty --web` for browser-backed mode.
- `witty-launcher` stays independently testable and keeps launcher-specific
  HTTP, token, and gateway lifecycle code out of the window application path.
- `witty-web` remains browser/wasm-only and does not gain native PTY, process,
  or socket dependencies.
- The existing launcher smoke can verify the actual product CLI instead of a
  helper binary.

## CLI Shape

The initial `witty --web` mode forwards launcher options directly:

```text
witty --web \
  --web-root target/witty-web-smoke \
  --program /bin/sh \
  --arg -lc \
  --arg '<shell script>'
```

Forwarded launcher options:

- `--web-root <path>`
- `--ui-bind <ip:port>`
- `--gateway-bind <ip:port>`
- `--program <path>`
- `--arg <value>`
- `--open-browser`

Wasm startup plugin flags are intentionally rejected for `--web` for now. The
browser plugin story needs a separate packaging and permission plan.

## Retained Internal Boundary

`witty-launcher` remains the owner of:

- session id and gateway token generation
- loopback UI and gateway listener binding
- exact browser Origin policy
- `/session/<id>.json` serving
- gateway `GatewayConfig` construction
- browser session lifecycle

`witty-app` only owns product command routing.

## Verification

The launcher browser smoke now starts:

```text
cargo run -p witty-app -- --web ...
```

instead of running `witty-launcher` directly. Unit tests cover forwarding of
launcher arguments from `witty --web`.

## Follow-Up

`m65-session-config-one-use` made `/session/<id>.json` one-use or expiring and
added launcher browser smoke coverage for stale config.

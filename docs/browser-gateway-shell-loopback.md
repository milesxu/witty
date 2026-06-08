# Browser Gateway Shell Loopback

Updated: 2026-05-29

## Purpose

This milestone adds the first native gateway process that connects the browser
terminal WebSocket protocol to a real local PTY.

The browser remains a wasm client using `BrowserGatewayTransport`. The new
native `witty-gateway` crate owns `LocalPtyTransport`, the WebSocket server, and
the process lifecycle.

## New Crate

`crates/witty-gateway` provides:

- `GatewayConfig`
- `GatewaySession`
- `parse_config(args)`
- `run_once(config)`
- `run_cli(args)`

The first CLI mode is intentionally narrow:

```text
witty-gateway --once --bind 127.0.0.1:8788 --program /bin/sh --arg -lc --arg '<script>'
```

`--once` accepts one WebSocket connection and exits after the browser closes or
the PTY exits.

## Protocol Mapping

Browser client frames:

- `hello` validates the v1 protocol and returns `ready`.
- `resize` stores the grid size, spawns the PTY if needed, or resizes an
  existing PTY.
- `input` writes bytes to the PTY.
- `pong` is accepted as a no-op for future heartbeat support.

Gateway server frames:

- `TransportEvent::Output(bytes)` becomes `output`.
- `TransportEvent::Error(message)` becomes `error`.
- `TransportEvent::Exit { code }` becomes `exit`.

The first resize triggers PTY spawn so the shell starts with the browser grid
instead of immediately receiving a resize after launch.

## Smoke Modes

`scripts/run-witty-web-smoke.mjs` now supports:

```text
WITTY_WEB_SMOKE_GATEWAY=node
WITTY_WEB_SMOKE_GATEWAY=rust
```

The default `node` mode keeps the deterministic protocol loopback from `m56`.

The `rust` mode spawns:

```text
cargo run -p witty-gateway -- --once --bind 127.0.0.1:8788 --program /bin/sh --arg -lc --arg 'printf "shell ready\r\n"; while IFS= read -r line; do printf "pty saw:%s\r\n> " "$line"; done'
```

The Playwright smoke then verifies:

- browser connects to the Rust gateway
- gateway sends `ready`
- first browser `resize` spawns the PTY
- PTY output includes `shell ready`
- typing `xy` plus Enter reaches the PTY
- PTY output includes `pty saw:xy`
- manual canvas resize still sends a browser resize frame
- final canvas screenshot is nonblank

## Current Limits

- Single session only.
- Blocking WebSocket loop.
- Loopback bind only in the smoke command.
- No token/origin hardening yet.
- No reconnect/resume.
- No browser-selected command execution.
- JSON byte arrays remain the wire format.

The next security milestone should add a token/origin policy and conservative
bind-address validation before this gateway is treated as anything more than a
local development bridge.

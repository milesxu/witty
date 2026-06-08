# Browser Transport Boundary

Updated: 2026-05-29

## Purpose

The browser build should not depend on native PTY types and should not treat
the current mock session as the long-term remote-shell API.
`BrowserGatewayTransport` is the explicit boundary for browser-hosted terminal
sessions.

## Direction

`TerminalApp` writes user input to `TerminalTransport::write`.

For the browser gateway transport, those bytes become outbound gateway bytes:

```text
browser key event -> witty-web key encoder -> TerminalApp::write_input
  -> BrowserGatewayTransport::write -> outbound byte buffer
```

A future WebSocket, WebTransport, SSH-over-service, or local development bridge
can drain that outbound buffer and forward it to a real remote process.

Remote output enters from the opposite direction:

```text
gateway message -> BrowserGatewayTransport::push_output
  -> TerminalApp::poll_transport -> BasicTerminal::feed -> WgpuRectRenderer
```

## Current API

- `BrowserGatewayTransport::outbound()`
- `BrowserGatewayTransport::drain_outbound()`
- `BrowserGatewayTransport::push_output(bytes)`
- `BrowserGatewayTransport::push_error(message)`
- `BrowserGatewayTransport::push_exit(code)`
- `BrowserGatewayTransport::resize(size)`

The wasm smoke session exposes the same split to JavaScript:

- `WittyWebSession::written_text()`
- `WittyWebSession::drain_outbound_text()`
- `WittyWebSession::push_gateway_output(text)`

## Verification

`scripts/run-witty-web-smoke.sh` now verifies both sides:

- real browser keyboard events write `xy\r` into outbound gateway bytes
- a synthetic gateway output message is pushed back into the terminal session
- the canvas remains nonblank after the roundtrip

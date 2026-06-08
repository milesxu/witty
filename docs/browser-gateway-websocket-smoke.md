# Browser Gateway WebSocket Smoke

Updated: 2026-05-29

## Purpose

The browser smoke now validates the first real WebSocket bridge shape for
`BrowserGatewayTransport`.

The smoke runner still uses a local loopback gateway instead of a PTY-backed
service, but the browser now exchanges the same v1 JSON frames documented in
`browser-gateway-websocket-plan.md`.

## Runtime Path

```text
canvas keydown
  -> WittyWebSession::handle_key()
  -> TerminalApp<BrowserGatewayTransport>::write_input()
  -> WittyWebSession::drain_outbound_message_json()
  -> browser WebSocket send {"type":"input","bytes":[...]}
  -> local Node loopback gateway
  -> browser WebSocket receive {"type":"output","bytes":[...]}
  -> WittyWebSession::push_gateway_message_json()
  -> BrowserGatewayTransport::push_server_message()
  -> TerminalApp::poll_transport()
  -> BasicTerminal::feed()
  -> WgpuRectRenderer::render()
```

## Smoke Assertions

`scripts/run-witty-web-smoke.mjs` now starts two local servers:

- a Python static HTTP server for the wasm page
- a minimal Node WebSocket server for the gateway loopback

The Playwright smoke verifies:

- the browser sends `hello` with protocol `1`
- the browser sends an initial `resize` frame
- typing `xy` plus Enter sends aggregate input bytes `[120,121,13]`
- the loopback gateway returns an `output` frame
- browser JavaScript records the server output frame
- manual canvas resize emits another `resize` frame and keeps the terminal/grid state coherent
- the final canvas screenshot is a nonblank PNG

## Scope

This is not yet a shell gateway. It deliberately avoids process spawning,
authentication, reconnect policy, binary framing, and PTY lifecycle semantics.
Those belong in the next gateway-service milestone after the browser transport
loopback is stable.

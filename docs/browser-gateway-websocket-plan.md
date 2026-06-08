# Browser Gateway WebSocket Plan

Updated: 2026-05-29

## Purpose

The browser terminal already has `BrowserGatewayTransport`, which separates
outbound user input from inbound gateway output. The next step is a WebSocket
bridge that forwards those protocol messages to a local or remote process
gateway.

This document defines the v1 protocol scaffold. It is intentionally small and
JSON-based so it can be tested from Rust, browser JavaScript, Node, and future
gateway services without committing to binary framing too early.

## Frame Format

Each WebSocket text frame is one JSON object. Terminal byte payloads are encoded
as arrays of unsigned byte values.

Client to gateway:

```json
{"type":"hello","protocol":1}
{"type":"input","bytes":[120,121,13]}
{"type":"resize","rows":24,"cols":80}
{"type":"pong","id":"heartbeat-1"}
```

Gateway to client:

```json
{"type":"ready","protocol":1}
{"type":"output","bytes":[111,107,13,10]}
{"type":"error","message":"pty exited unexpectedly"}
{"type":"exit","code":0}
{"type":"ping","id":"heartbeat-1"}
```

## Rust Scaffold

The protocol types live in `witty-transport`:

- `BROWSER_GATEWAY_PROTOCOL_VERSION`
- `BrowserGatewayClientMessage`
- `BrowserGatewayServerMessage`
- `BrowserGatewayClientMessage::to_json/from_json`
- `BrowserGatewayServerMessage::to_json/from_json`
- `BrowserGatewayServerMessage::into_transport_event`

`BrowserGatewayTransport` now has helpers for bridge integration:

- `drain_outbound_message()`
- `resize_message()`
- `push_server_message(message)`

## Browser Integration Shape

The browser loop should eventually do this:

```text
open WebSocket
send {"type":"hello","protocol":1}
on ready:
  send resize_message()
on key input:
  drain outbound bytes and send input message
on ResizeObserver:
  resize wasm session and send resize message
on gateway output:
  push server message into BrowserGatewayTransport
  poll transport and render
```

## Next Implementation Step

`m56-browser-gateway-websocket-smoke` should add a local WebSocket loopback
smoke server for Playwright:

1. Serve the existing wasm page.
2. Start a local WebSocket server in the smoke runner.
3. Connect browser JavaScript to the loopback server.
4. Type `xy` in the canvas.
5. Assert the server receives an `input` frame with `[120,121]`.
6. Echo an `output` frame back to the browser.
7. Assert the browser renders a nonblank canvas after the gateway output.

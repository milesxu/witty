# Browser Gateway Shell Adapter Plan

Updated: 2026-05-29

## Purpose

`m56` proved the browser can speak the v1 gateway JSON protocol over a real
WebSocket. The next step is to replace the Node loopback gateway with a
native-only local gateway that attaches each browser WebSocket session to a
real PTY.

The browser and wasm crates must stay PTY-free. The gateway should be a native
process or native crate that owns `LocalPtyTransport`, translates WebSocket
frames into `TerminalTransport` calls, and translates PTY events back into
gateway server frames.

## Existing Boundaries

- `witty-web`
  - Browser/wasm only.
  - Owns `TerminalApp<BrowserGatewayTransport>`.
  - Sends `hello`, `input`, and `resize` JSON frames over browser WebSocket.
  - Receives `ready`, `output`, `error`, and `exit` JSON frames and pushes them
    into `BrowserGatewayTransport`.
- `witty-transport`
  - Owns `TerminalTransport`.
  - Owns `BrowserGatewayClientMessage` and `BrowserGatewayServerMessage`.
  - Owns native `LocalPtyTransport` behind `cfg(not(target_arch = "wasm32"))`.
- `witty-app`
  - Native window app already uses `TerminalApp<LocalPtyTransport>`.
  - Useful as behavioral reference, but should not become a browser gateway
    dependency because it pulls in window, clipboard, and smoke-mode concerns.

## Proposed Crate Shape

Add a native-only workspace crate:

```text
crates/witty-gateway/
  Cargo.toml
  src/lib.rs
  src/main.rs
```

Suggested responsibilities:

- `lib.rs`
  - `GatewayConfig`
  - `GatewaySession`
  - `GatewaySession::handle_client_message(message)`
  - `GatewaySession::poll_server_messages()`
  - small adapter between `BrowserGatewayClientMessage` and
    `LocalPtyTransport`
- `main.rs`
  - CLI binding for local smoke and manual testing
  - `--bind 127.0.0.1:8788`
  - `--program <path>` plus repeated `--arg <arg>`
  - `--once` to accept one WebSocket session and exit after socket close or PTY
    exit

Keep the first version blocking and boring unless async becomes necessary.
The existing `LocalPtyTransport` already uses reader/waiter threads and
nonblocking `poll_event()`. A synchronous WebSocket server can be enough for a
single-session smoke. Move to `tokio`/`tokio-tungstenite` later only when
multi-session scheduling, cancellation, or browser reconnect semantics require
it.

## Message Mapping

Client to gateway:

- `hello { protocol }`
  - validate `protocol == BROWSER_GATEWAY_PROTOCOL_VERSION`
  - validate origin/token policy
  - respond with `ready { protocol }`
- `resize { rows, cols }`
  - if PTY is already spawned, call `LocalPtyTransport::resize(GridSize)`
  - if PTY is not spawned yet, store initial size for spawn
- `input { bytes }`
  - write bytes to `LocalPtyTransport::write`
  - on write error, send `error { message }`
- `pong { id }`
  - record heartbeat acknowledgement; no terminal action

Gateway to client:

- `ready { protocol }`
  - sent after protocol and access checks pass
- `output { bytes }`
  - sent for each `TransportEvent::Output(bytes)`
- `error { message }`
  - sent for adapter, PTY, protocol, or access failures
- `exit { code }`
  - sent for `TransportEvent::Exit { code }`
- `ping { id }`
  - optional heartbeat; not needed in the first smoke

## Session Lifecycle

```text
accept WebSocket
  -> parse HTTP upgrade
  -> validate bind address, Origin, and optional token
  -> wait for hello
  -> send ready
  -> wait for first resize or use default 24x80
  -> spawn LocalPtyTransport
  -> loop:
       websocket input frame -> LocalPtyTransport::write
       websocket resize frame -> LocalPtyTransport::resize
       LocalPtyTransport::poll_event -> server output/error/exit frame
  -> on socket close: drop LocalPtyTransport, killing the child
  -> on PTY exit: send exit and close socket
```

Spawn after the first resize if possible. That prevents the shell from seeing a
short-lived 24x80 size and then immediately receiving another resize.

## Security Defaults

The first native gateway must not become a general unauthenticated remote shell.

Required first-version defaults:

- bind to `127.0.0.1` by default, never `0.0.0.0`
- reject non-loopback bind addresses unless an explicit unsafe flag is passed
- require an access token for browser connection when the gateway is launched
  outside the controlled Playwright smoke
- validate `Origin` for browser sessions
- accept only one session in `--once` smoke mode
- do not let the browser choose arbitrary commands in v1
- make command/program selection a gateway launch-time option, not a WebSocket
  message
- cap accepted WebSocket text frame size
- cap outbound queue size or close on persistent backpressure

## Backpressure And Encoding

The v1 protocol uses JSON arrays of bytes. That is fine for tests and early
local prototypes but inefficient for high-volume output.

Keep these follow-ups explicit:

- v1 smoke: JSON text frames, byte arrays
- v1.1 local optimization: `output_b64` and `input_b64` if array overhead
  dominates
- v2 production option: binary WebSocket frames tagged by a compact header

Backpressure should initially be conservative: if the WebSocket cannot flush
within a short bounded interval, close the session and kill the PTY rather than
buffering unbounded terminal output.

## m58 Implementation Target

`m58-browser-gateway-shell-loopback` should implement the smallest real PTY
gateway smoke:

1. Add native-only `witty-gateway` crate.
2. Reuse `BrowserGatewayClientMessage` and `BrowserGatewayServerMessage`.
3. Spawn `LocalPtyTransport` with a deterministic command:
   - Unix: `/bin/sh -lc 'printf "shell ready\r\n"; cat'`
   - Windows follow-up can be planned separately.
4. Add a bounded `--once --bind 127.0.0.1:<port>` CLI mode.
5. Update `scripts/run-witty-web-smoke.mjs` to choose between:
   - in-process Node loopback gateway for fast protocol smoke
   - spawned Rust gateway for PTY smoke
6. Assert browser typing reaches the Rust gateway PTY and PTY output reaches
   the browser.
7. Keep the Node loopback smoke as the deterministic protocol-level fallback.

## Non-Goals For m58

- SSH transport
- remote machine authentication
- reconnect/resume
- multiple browser clients
- browser-selected command execution
- terminal file transfer
- binary WebSocket framing
- plugin access to gateway internals

Those are important, but they should not be mixed into the first PTY-backed
browser smoke.

# Gateway Protocol Backpressure

Updated: 2026-05-29

## Purpose

`witty-gateway` now has explicit protocol size limits and conservative write-side
backpressure behavior for the browser-to-PTY bridge.

This is still a local development gateway. The current policy favors closing a
stalled session over buffering unbounded terminal output.

## Limits

Defaults:

| Limit | Default | Scope |
| --- | ---: | --- |
| `--max-client-frame-bytes` | 262144 | incoming browser WebSocket message and frame size |
| `--max-server-frame-bytes` | 262144 | serialized gateway-to-browser JSON frame size |
| `--max-output-burst-bytes` | 524288 | serialized server bytes returned by one PTY poll pass |
| `--max-ws-write-buffer-bytes` | 524288 | tungstenite write-buffer ceiling |
| `--write-timeout-ms` | 5000 | TCP write timeout for browser sends |

The output burst limit must be at least the server-frame limit. If a PTY emits
more data than one poll pass can send, `GatewaySession` leaves the remainder in
an internal pending queue and sends it on later passes.

## Incoming Frames

The WebSocket accept path now uses `WebSocketConfig` with
`max_message_size` and `max_frame_size` set from `--max-client-frame-bytes`.

After JSON parsing, `input.bytes` is checked against the same limit before the
bytes are written to the PTY.

## Outgoing Frames

Every gateway-to-browser JSON frame is sized before send. If a serialized frame
would exceed `--max-server-frame-bytes`, the gateway returns an error and the
session closes when `run_connection` unwinds.

PTY output is sent in bounded bursts. This prevents a single event-loop pass
from building an unbounded vector of output frames while still allowing normal
large command output to progress over multiple passes.

## Slow Browser Policy

The gateway sets a bounded TCP write timeout on the accepted socket. If
tungstenite reports a write timeout or a full write buffer, the gateway treats
that as persistent browser-side backpressure, closes the connection, and drops
the PTY transport.

The current implementation does not retry indefinitely, spool output to disk, or
attempt reconnect/resume.

## Remaining Gaps

- binary framing is still deferred
- JSON byte arrays are inefficient for heavy output
- PTY reader uses an unbounded channel internally
- no reconnect/resume protocol exists
- no per-session metrics are exposed yet

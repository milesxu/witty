# Terminal Focus Event Reporting

Updated: 2026-05-30

m105 wires xterm focus-in/focus-out reporting for `CSI ? 1004 h/l`.

## Shared Encoder

`witty-core` now exports:

- `FocusEventKind`
- `encode_terminal_focus_event()`

The encoder is gated by `TerminalMouseModes::focus_events`, independent of
mouse tracking mode. This matches xterm behavior where focus reporting can be
enabled without `1000`, `1002`, or `1003`.

Sequences:

| Event | Bytes |
| --- | --- |
| Focus in | `ESC [ I` |
| Focus out | `ESC [ O` |

## Native Bridge

Native window mode listens for `WindowEvent::Focused(bool)` and writes the
encoded focus report to the terminal input path when `1004` is enabled.

## Browser Bridge

`WittyWebSession` exposes:

```rust
pub fn handle_focus(&mut self, focused: bool) -> Result<bool, JsValue>
```

`static/app.js` listens for canvas `focus` and `blur` events, calls the wasm
method, and flushes any produced bytes through the existing browser gateway
input frame path.

## Runtime Smoke

The browser smoke now:

- dispatches a focus event while `1004` is disabled and expects no input frame
- feeds `CSI ? 1004 h`
- dispatches focus and blur and expects `ESC [ I` and `ESC [ O`
- feeds `CSI ? 1004 l`
- dispatches another focus event and expects no additional input frame

Focus reporting is runtime-smoke-tested in the deterministic node gateway mode.
PTY-backed smoke modes keep focus reporting covered by the shared encoder,
native/browser unit tests, and the node-gateway runtime path; their broader
browser smokes avoid focus-mode injection so shell echo from prior control-key
checks cannot interleave with the wasm session during the final product
lifecycle checks.

# Terminal Mouse Browser Bridge

Updated: 2026-05-30

m104 connects browser pointer and wheel events to the shared terminal mouse
encoder introduced in m103.

## Wasm API

`WittyWebSession` now exposes:

```rust
pub fn handle_mouse(
    &mut self,
    kind: String,
    button: i16,
    buttons: u16,
    offset_x: f64,
    offset_y: f64,
    delta_y: f64,
    shift: bool,
    alt: bool,
    control: bool,
) -> Result<bool, JsValue>
```

The method returns `true` only when terminal mouse reporting is active and the
event produced encoded bytes. It writes those bytes to `BrowserGatewayTransport`
so the existing `drain_outbound_message_json()` path forwards them to the
gateway.

Browser coordinates are accepted as CSS pixel offsets. The wasm layer converts
them to backing-store pixels with the current device-pixel ratio before mapping
to terminal cells. SGR pixel-position mode uses backing-store pixel positions.

## JavaScript Bridge

`static/app.js` now listens for:

- `pointerdown`
- `pointerup`
- `pointermove`
- `wheel`
- `contextmenu`

When mouse reporting is active, handled pointer/wheel events are flushed to the
gateway as input frames and `preventDefault()` is applied. Pointer capture is
used for handled pointer presses so drag reporting remains stable while the
pointer leaves the canvas.

When reporting is inactive, `handle_mouse()` returns `false` and the bridge does
not prevent default browser behavior.

## Runtime Smoke

The default node-gateway browser smoke now:

- verifies pointer events are ignored before reporting is enabled
- feeds `CSI ? 1000 h` and `CSI ? 1006 h`
- dispatches pointer press/release and checks SGR input bytes
- feeds `CSI ? 1002 h`
- dispatches a same-cell drag that is suppressed and a changed-cell drag that is
  reported with `Cb + 32`
- dispatches a wheel-up event and checks `Cb = 64`

The mouse runtime section now runs for node, Rust PTY, and product launcher
gateway modes. In PTY-backed modes the pointer/wheel section is intentionally
kept at the end of the smoke after title, alternate-screen, keyboard, resize,
screenshot, and function-key checks. It waits for pending gateway output to
settle, enables `1002` and `1006` through the queued browser gateway-output
helper, and then dispatches pointer events. The generated mouse input bytes are
allowed to be discarded by gateway/browser shutdown instead of being followed by
another line-oriented shell assertion. The PTY smoke shell disables terminal echo
so post-assertion control bytes are not echoed back as unrelated gateway output.

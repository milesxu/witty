# Terminal Keypad Runtime Smoke

Updated: 2026-05-30

## Scope

The browser smoke harness now verifies the application-keypad path through the
real wasm/browser event boundary.

The smoke sequence:

1. Loads the product browser bundle.
2. Connects the browser session to the WebSocket gateway.
3. Injects terminal output `ESC =` through `push_gateway_message_json` so
   `BasicTerminal` enters application keypad mode.
4. Dispatches real DOM `KeyboardEvent` values against the terminal canvas.
5. Verifies the client `input` frames sent to the gateway.
6. Injects `ESC >` and verifies keypad input returns to normal text behavior.

Expected input frames:

| Runtime input | Expected bytes |
| --- | --- |
| top-row `1`, `code=Digit1`, `location=0` while application keypad is active | `[49]` |
| numpad `1`, `code=Numpad1`, `location=3` while application keypad is active | `[27, 79, 113]` |
| numpad `1`, `code=Numpad1`, `location=3` after `ESC >` | `[49]` |

This catches regressions that unit tests cannot fully cover: DOM event metadata
must survive `app.js`, the wasm-bindgen method call, Rust event normalization,
terminal mode lookup, gateway outbound draining, and WebSocket frame recording.

# Terminal Mouse Product Smoke

Updated: 2026-05-30

m106 extends the browser mouse runtime smoke from the deterministic node gateway
to the Rust PTY gateway and product `witty --web` launcher modes.

## Safety Boundary

For PTY-backed modes, the product mouse section starts after the `xy` line round
trip and after all subsequent shell assertions. It waits for the page-side
gateway-message queue to drain, then injects `CSI ? 1002 h` and `CSI ? 1006 h`
through the same gateway-output helper used by WebSocket messages. This keeps
mode setup serialized with any real PTY output before browser pointer events are
dispatched.

The smoke shell runs with terminal echo disabled via `stty -echo`. The script
still prints `pty saw:xy` explicitly for the round-trip assertion, while later
function-key and mouse control bytes are not echoed back as asynchronous shell
output during the final smoke sections.

The product mouse smoke then synthesizes browser pointer and wheel events after
the main PTY shell assertions have already completed:

1. gateway connection and initial resize
2. OSC title propagation
3. alternate-screen restore
4. printable `xy` input round trip through the PTY-backed shell
5. manual resize propagation
6. nonblank canvas screenshot
7. function-key smoke
8. mouse reporting smoke

The resulting mouse bytes are still real gateway input frames, so they can reach
the shell. They do not include a newline and the smoke performs no later
shell-line assertions. Browser and gateway shutdown then tears down the PTY
session.

## Covered Runtime Bytes

The default node-gateway smoke still enables normal SGR mouse reporting with
`CSI ? 1000 h` and `CSI ? 1006 h`, verifies inactive events are ignored, then
switches to `CSI ? 1002 h` for drag coverage.

The PTY/product smoke enables `CSI ? 1002 h` and `CSI ? 1006 h` through the
queued gateway-output helper and verifies:

| Event | Expected bytes |
| --- | --- |
| left press | `ESC [ < 0 ; 3 ; 4 M` |
| left release | `ESC [ < 0 ; 3 ; 4 m` |
| button-event drag press | `ESC [ < 0 ; 3 ; 4 M` |
| button-event drag motion | `ESC [ < 32 ; 4 ; 4 M` |
| wheel up | `ESC [ < 64 ; 4 ; 4 M` |

The same assertions now run through:

- default node WebSocket gateway
- `WITTY_WEB_SMOKE_GATEWAY=rust`
- `WITTY_WEB_SMOKE_GATEWAY=launcher`

Focus reporting remains runtime-smoke-tested on the default node gateway path;
PTY-backed product smokes skip that section so this mouse-specific product
coverage is isolated from unrelated focus-mode injection.

## Verification

- `node --check scripts/run-witty-web-smoke.mjs`
- `scripts/run-witty-web-smoke.sh`
- `WITTY_WEB_SMOKE_GATEWAY=rust scripts/run-witty-web-smoke.sh`
- `WITTY_WEB_SMOKE_GATEWAY=launcher scripts/run-witty-web-smoke.sh`

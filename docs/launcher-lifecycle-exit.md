# Launcher Lifecycle Exit

Updated: 2026-05-30

## Purpose

The browser launcher should exit cleanly when its single browser/gateway session
ends. The smoke harness should not rely on `SIGTERM` as proof that the product
path works.

## Lifecycle

The current single-session path is:

1. Browser page connects to the gateway WebSocket.
2. Closing the browser closes the WebSocket.
3. `witty-gateway::run_connection` returns on WebSocket close.
4. The launcher gateway thread sets `gateway_done`.
5. The launcher HTTP loop exits on the next poll.
6. `witty --web` joins the gateway thread and exits with code `0`.

## Smoke Assertion

Launcher browser smoke now closes Chromium after the normal terminal roundtrip
and canvas screenshot, then waits for `witty --web` to exit naturally.

The test fails if:

- the launcher does not exit within the timeout
- the launcher exits with a nonzero code
- the launcher exits by signal

Cleanup still sends `SIGTERM` only after failures or for non-launcher smoke
modes that intentionally keep helper servers alive until the harness ends.

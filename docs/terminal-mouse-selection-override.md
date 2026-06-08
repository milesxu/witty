# Terminal Mouse Selection Override

Updated: 2026-05-30

m108 implements the first `ShiftSelect` local-selection override while xterm
application mouse reporting is active.

m109 exposes this policy through
`docs/terminal-mouse-override-profile-config.md`.

## Native Behavior

Native window mode now uses `ShiftSelect` as the default local override policy:

| Gesture while app mouse reporting is active | Behavior |
| --- | --- |
| plain left/middle/right press, release, drag | send xterm mouse report |
| plain wheel | send xterm wheel report |
| Shift + left press/drag/release | local terminal selection |
| Shift + left double-click | local word selection |
| Shift + middle press | local primary-selection paste |
| Shift + wheel | local scrollback |

The implementation keeps a small decision helper around:

- mouse reporting active/inactive state
- override policy
- current modifier state
- button/motion/wheel event kind
- active local selection drag state

Once a Shift-left drag starts, it remains local until the left-button release
even if Shift is released before the drag ends. This prevents half of a local
selection from leaking to the application as mouse-motion reports.

`MouseSelectionOverridePolicy::Disabled` is present as the raw xterm
compatibility path. It leaves Shift mouse gestures as application mouse reports
with the SGR Shift modifier bit.

## Browser Behavior

Browser mode now supports Shift-left local selection while mouse reporting is
active, plus local scrollback wheel parity:

- Shift + pointerdown calls `begin_local_selection()`.
- pointermove during that local drag calls `update_local_selection()`.
- pointerup calls `end_local_selection()`.
- the pointer events are prevented and do not call `handle_mouse()`.
- plain pointer and wheel events still use the gateway mouse report path while
  mouse reporting is active.
- wheel scrolls local scrollback while mouse reporting is inactive.
- Shift-wheel scrolls local scrollback while mouse reporting is active under
  `shift-select`.

The wasm session exposes diagnostics for smoke coverage:

- `selected_text()`
- `selection_range_text()`
- `viewport_offset()`

Browser primary-selection paste remains native-only for now because the browser
path does not expose a system primary selection.

Browser clipboard follow-up work is planned in
`docs/browser-selection-clipboard-plan.md`.

## Runtime Smoke

The default node-gateway browser smoke now runs a Shift pointer
down/move/up sequence after mouse reporting is enabled. It also exercises local
scrollback wheel handling. It verifies:

- no gateway input frame is produced for the local selection gesture
- the browser session exposes a non-empty selected text
- the browser session exposes a non-empty selection range
- inactive plain wheel scrolls local scrollback without sending input bytes
- active Shift-wheel scrolls local scrollback without sending input bytes
- active plain wheel still sends an xterm wheel report

PTY-backed Rust gateway and product launcher smokes continue to run the mouse
reporting product path, and now also run browser local scrollback wheel smoke.

## Verification

- `cargo test -p witty-app mouse_override -- --nocapture`
- `cargo test -p witty-web local_selection -- --nocapture`
- `cargo test -p witty-web browser_scroll_lines_for_wheel_delta --quiet`
- `cargo test -p witty-app native_mouse -- --nocapture`
- `scripts/run-witty-web-smoke.sh`
- `cargo test -p witty-web browser_mouse -- --nocapture`
- `scripts/run-witty-web-smoke.sh`

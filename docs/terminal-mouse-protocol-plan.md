# Terminal Mouse Protocol Plan

Updated: 2026-05-30

## Goal

Add xterm-compatible mouse reporting without breaking the existing local
selection, primary-selection paste, and scrollback behavior.

The first implementation target should be a practical xterm profile for TUI
apps such as shells, editors, pagers, and terminal UIs:

- Track terminal mouse DEC private modes in `BasicTerminal`.
- Share one mouse encoder between native and browser paths.
- Prefer SGR mouse encoding (`CSI < ... M/m`) when enabled.
- Keep local selection and scrollback behavior when no application mouse mode
  is active.

Do not implement every historical mouse dialect in the first pass.

## Baseline Sources

Primary reference:

- XTerm Control Sequences: <https://www.invisible-island.net/xterm/ctlseqs/ctlseqs.html>

Relevant baseline points from xterm:

- `CSI ? 1000 h/l`: send mouse position on button press/release.
- `CSI ? 1002 h/l`: button-event tracking; report drag/motion while a button is
  down.
- `CSI ? 1003 h/l`: any-event tracking; report all motion.
- `CSI ? 1004 h/l`: focus-in/focus-out events, separate from mouse button
  events.
- `CSI ? 1006 h/l`: SGR mouse encoding.
- `CSI ? 1016 h/l`: SGR pixel-position encoding.
- Wheel buttons add `64` to the encoded event code and do not send release
  events.

## Existing Code Context

Core:

- `BasicTerminalState::set_private_mode()` already tracks DEC private modes for
  cursor keys, alternate screen, bracketed paste, cursor visibility, and related
  modes.
- `TerminalInputModes` currently exposes only keyboard input state:
  `application_cursor_keys` and `application_keypad`.

Native:

- `crates/witty-app/src/window.rs` currently owns all mouse behavior:
  left-click selection, drag selection, primary selection publishing, middle
  click primary paste, and wheel scrollback.
- `cell_point_for_position()` already converts pixel coordinates to terminal
  cells.

Browser:

- `crates/witty-web/static/app.js` currently forwards keyboard events only.
- There is no browser mouse event bridge yet.
- Browser input already uses `WittyWebSession` methods plus
  `drain_outbound_message_json()` to forward input frames to the gateway.

## Scope For m102-m104

### m102: Core Mouse Mode Tracking

Add shared terminal mouse mode state in `witty-core`.

Recommended API:

```rust
pub struct TerminalInputModes {
    pub application_cursor_keys: bool,
    pub application_keypad: bool,
    pub keyboard_locked: bool,
    pub backarrow_sends_backspace: bool,
    pub mouse: TerminalMouseModes,
}

pub struct TerminalMouseModes {
    pub tracking: MouseTrackingMode,
    pub encoding: MouseEncodingMode,
    pub focus_events: bool,
    pub alternate_scroll: bool,
}

pub enum MouseTrackingMode {
    None,
    X10,
    Normal,
    ButtonEvent,
    AnyEvent,
}

pub enum MouseEncodingMode {
    X10,
    Utf8,
    Urxvt,
    Sgr,
    SgrPixels,
}
```

Parsing rules:

- `9` enables X10 compatibility tracking.
- `1000` enables normal press/release tracking.
- `1002` enables button-event tracking.
- `1003` enables any-event tracking.
- `1004` toggles focus events.
- `1006` toggles SGR mouse encoding.
- `1007` toggles alternate-scroll mode.
- `1015` toggles urxvt decimal legacy mouse encoding.
- `1016` toggles SGR pixel mode, but can be parsed before it is emitted.
- Resetting a tracking mode should disable that mode; enabling a higher tracking
  mode should become the active tracking mode.
- Full reset should clear all mouse modes.

Defer:

- `1001` highlight tracking, because it requires a cooperative application
  handshake.
- `1005` UTF-8 mouse mode.
- Readline mouse modes `2001`-`2003`.

### m103: Shared Mouse Encoder And Native Integration

Add a shared encoder in `witty-core` or a new small shared input module. Use
`witty-core` for the first implementation to avoid duplicating protocol logic
between native and browser.

Recommended event model:

```rust
pub struct TerminalMouseEvent {
    pub kind: MouseEventKind,
    pub button: MouseButtonCode,
    pub cell: CellPoint,
    pub pixel: Option<PixelMousePosition>,
    pub modifiers: MouseModifiers,
}

pub enum MouseEventKind {
    Press,
    Release,
    Motion,
    Wheel,
}
```

Encoding rules for m103:

- Coordinates are 1-based in protocol output.
- SGR encoding uses `CSI < Cb ; Cx ; Cy M` for press/motion/wheel and
  `CSI < Cb ; Cx ; Cy m` for release.
- X10 encoding uses `CSI M Cb Cx Cy`, with `32` added to each encoded byte.
- Motion adds `32` to `Cb`.
- Wheel up/down use `Cb = 64/65` plus modifiers and final `M`; no release event.
- Modifiers use xterm mouse bits:
  - Shift: `4`
  - Alt: `8`
  - Ctrl: `16`
- X10/non-SGR release should use `Cb = 3`.
- SGR release should retain the released button value when known.

Native behavior:

- If no mouse tracking is active:
  - left-click and drag keep existing selection behavior.
  - left release publishes primary selection after drag.
  - middle click keeps primary paste.
  - wheel keeps local scrollback.
- If mouse tracking is active:
  - left/middle/right press and release are sent as terminal input.
  - wheel is sent as terminal input.
  - button motion is sent when `1002` or `1003` requires it.
  - local selection and primary paste do not run for those events.
- Keep `cell_point_for_position()` for cell coordinates.
- Track currently pressed mouse button and last reported cell so `1002` can
  avoid duplicate same-cell motion reports.

Open ergonomic question:

- Many terminal users expect a modifier override for local selection while mouse
  reporting is active. Do not add this in m103 unless needed. A later profile
  setting can decide whether Shift bypasses application mouse reporting or is
  encoded as the xterm Shift mouse modifier. m107 resolves this as
  `ShiftSelect` by default with an explicit disabled/raw-xterm compatibility
  policy; see `docs/terminal-mouse-selection-override-plan.md`.

### m104: Browser Mouse Bridge And Runtime Smoke

Add browser DOM pointer/wheel handling in `crates/witty-web/static/app.js`.

Recommended browser events:

- `pointerdown`
- `pointerup`
- `pointermove`
- `wheel`
- optionally `contextmenu` suppression only while mouse tracking is active

Recommended wasm API:

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

The wasm method should:

- Convert browser coordinates to terminal cell coordinates using the session's
  current `CellMetrics` and grid size.
- Read `self.terminal.input_modes().mouse`.
- Encode the mouse event when reporting is active.
- Write encoded bytes to `BrowserGatewayTransport`.
- Return `true` only when the event was handled and should be prevented.

Runtime smoke should:

- Feed `CSI ? 1000 h` and `CSI ? 1006 h` to enable normal SGR mouse reporting.
- Dispatch canvas `pointerdown` and `pointerup` at a known cell.
- Verify gateway input frames contain `CSI < 0 ; x ; y M` and
  `CSI < 0 ; x ; y m`.
- Feed `CSI ? 1002 h`, dispatch a drag to another cell, and verify motion adds
  `32` to `Cb`.
- Dispatch a `wheel` event and verify `Cb = 64` or `65`.
- Run in the default node gateway path first; product launcher mode can be added
  if the smoke does not disrupt the PTY shell state.

## Sequence Examples

Assuming SGR encoding and cell `(row=0, col=0)`:

| Event | Bytes |
| --- | --- |
| Left press | `ESC [ < 0 ; 1 ; 1 M` |
| Left release | `ESC [ < 0 ; 1 ; 1 m` |
| Shift left press | `ESC [ < 4 ; 1 ; 1 M` |
| Ctrl left press | `ESC [ < 16 ; 1 ; 1 M` |
| Left-button drag motion | `ESC [ < 32 ; 1 ; 1 M` |
| Wheel up | `ESC [ < 64 ; 1 ; 1 M` |
| Wheel down | `ESC [ < 65 ; 1 ; 1 M` |

## Test Plan

Core tests:

- DECSET/DECRST tracking for `9`, `1000`, `1002`, `1003`, `1004`, `1006`,
  `1007`, and `1016`.
- Full reset clears all mouse modes.
- Enabling higher tracking modes selects the expected active tracking mode.
- SGR encoder emits press, release, drag, wheel, and modifier forms.
- X10 encoder emits legacy byte forms and suppresses coordinates outside the
  encodable range.

Native tests:

- Mouse reporting disabled preserves existing selection and scroll helpers.
- Mouse reporting enabled causes press/release/wheel helpers to return encoded
  input and bypass local selection/paste behavior.
- `1002` reports motion only when a button is down and the cell changes.
- `1003` reports motion without a button down.

Browser tests:

- DOM cell coordinate conversion is stable across DPR and resize.
- wasm `handle_mouse()` returns `false` when no mouse reporting is active.
- wasm `handle_mouse()` returns `true` and writes gateway input when reporting
  is active.

Smoke:

- Extend `scripts/run-witty-web-smoke.mjs` with a mouse runtime section after
  the keyboard sections.

## Risks

- Existing local selection behavior is user-facing. Mouse reporting must only
  override it when an application explicitly enables reporting.
- Browser pointer capture can make drag reporting more reliable, but incorrect
  capture/release can leave the canvas in a stuck drag state.
- Wheel delta normalization differs by platform and browser. The protocol should
  report direction, not raw delta magnitude, in m104.
- Pixel-mode `1016` needs raw pixel coordinates; it should not reuse clamped
  cell coordinates.
- Product launcher smoke may send control sequences into a real shell. Keep
  mouse smoke minimal and isolated, as with the function-key runtime smoke.

## Deferred

- Highlight tracking `1001`.
- UTF-8 mouse `1005`.
- Pixel-mode runtime smoke for `1016`.
- Focus event reporting `1004` runtime integration.
- Configurable local-selection override policy while mouse reporting is active.

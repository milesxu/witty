# Terminal Mouse Encoder And Native Integration

Updated: 2026-05-30

m103 adds a shared xterm-style mouse event encoder and routes native winit
mouse events through it when an application enables terminal mouse reporting.

## Shared Core Encoder

`witty-core` now exports:

- `TerminalMouseEvent`
- `MouseEventKind`
- `MouseButtonCode`
- `MouseModifiers`
- `PixelMousePosition`
- `encode_terminal_mouse_event()`

The encoder supports the mode snapshot from m102:

- `MouseTrackingMode::X10`
- `MouseTrackingMode::Normal`
- `MouseTrackingMode::ButtonEvent`
- `MouseTrackingMode::AnyEvent`
- `MouseEncodingMode::X10`
- `MouseEncodingMode::Utf8`
- `MouseEncodingMode::Urxvt`
- `MouseEncodingMode::Sgr`
- `MouseEncodingMode::SgrPixels`

SGR output uses:

```text
CSI < Cb ; Cx ; Cy M
CSI < Cb ; Cx ; Cy m
```

Legacy X10 output uses:

```text
CSI M Cb Cx Cy
```

UTF-8 legacy output uses the same packet shape but UTF-8 encodes each
`value + 32` field so larger cell coordinates can be represented.

Urxvt legacy output uses:

```text
CSI Cb ; Cx ; Cy M
```

where `Cb` is the legacy button code plus 32, while `Cx` and `Cy` are
decimal 1-based cell coordinates.

The encoder applies xterm mouse modifier bits:

| Modifier | Bit |
| --- | --- |
| Shift | `4` |
| Alt | `8` |
| Ctrl | `16` |

Motion adds `32` to `Cb`. Wheel up/down use `64` and `65` and are emitted with
final `M`.

## Native Behavior

When no terminal mouse tracking mode is active, native behavior is unchanged:

- left press/drag updates local selection
- left release publishes dragged selection to primary selection
- middle press pastes primary selection
- wheel scrolls local scrollback

When terminal mouse tracking is active:

- left/middle/right press and release are encoded as terminal input
- wheel events are encoded as terminal input
- `1002` button-event mode reports motion only while a button is down
- `1003` any-event mode reports motion even without a pressed button
- local selection, primary paste, and local wheel scrollback are bypassed

Native integration tracks the currently pressed terminal mouse button and the
last reported cell so button-event motion does not send duplicate same-cell
reports.

## Deferred

- Browser pointer/wheel bridge and Playwright runtime smoke are deferred to
  m104.
- Focus event reporting from `1004` is still mode-tracked only.
- Configurable Shift-to-select override while mouse reporting is active is
  planned in `docs/terminal-mouse-selection-override-plan.md`.

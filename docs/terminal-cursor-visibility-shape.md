# Terminal Cursor Visibility And Shape

Updated: 2026-06-01

## Scope

`BasicTerminal` now handles common cursor control sequences:

- `CSI ? 25 l`: hide cursor
- `CSI ? 25 h`: show cursor
- `CSI ? 12 l`: stop cursor blink while preserving shape
- `CSI ? 12 h`: start cursor blink while preserving shape
- `CSI Ps SP q`: DECSCUSR cursor shape

Supported DECSCUSR shape values:

- `0`, `1`: blinking block
- `2`: steady block
- `3`: blinking underline
- `4`: steady underline
- `5`: blinking bar
- `6`: steady bar

`m328-native-cursor-blink-state` preserves the blink bit in `CursorState`.
Native window mode now toggles only the terminal cursor overlay on a 500 ms
timer, resetting to visible when the cursor position or shape changes and
disabling blink while search or command palette owns text input. Browser cursor
blink timing remains deferred while local Chromium/WebGPU validation is
suspended on this Linux/M1000 host.

## Renderer Behavior

`FramePlanner` now converts `CursorShape` into different cursor rectangles:

- block: full cell rectangle
- bar: narrow full-height rectangle
- underline: thin rectangle at the bottom of the cell

`m458-terminal-cursor-color-osc12` adds `RenderSnapshot::cursor_color` and wires
`OSC 12`/`OSC 112` to set, query, and reset the cursor overlay color. When the
field is `None`, the renderer keeps its existing neutral gray cursor color. See
`terminal-cursor-color.md`.

Cursor visibility and shape changes mark the cursor row dirty so retained frame
planning can redraw the affected row.
Private cursor blink mode `?12` uses the same cursor damage path and is reported
through `CSI ? 12 $ p`.

## Follow-Ups

- Add browser cursor blink timing on a stable browser/WebGPU host.
- Add user theme defaults for cursor color.

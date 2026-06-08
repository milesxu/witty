# Terminal Tab Stops

Updated: 2026-06-01

## Scope

`BasicTerminal` supports configurable horizontal tab stops:

- default stops every 8 columns.
- `ESC H` sets a tab stop at the current column.
- `CSI g` or `CSI 0 g` clears the tab stop at the current column.
- `CSI 3 g` clears all tab stops.
- `CSI ? 5 W` (`DECST8C`) resets the tab stop set to default 8-column stops.

Horizontal tab moves to the next configured stop after the cursor. If no stop
exists to the right, it moves to the final visible column.

## Reset And Resize

Full terminal reset and `DECST8C` restore default 8-column tab stops. Resize
preserves tab stops that remain inside the new width and drops stops beyond the
right edge.

Tab stops are terminal-global rather than main/alternate-screen local.

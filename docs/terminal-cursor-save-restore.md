# Terminal Cursor Save Restore

Updated: 2026-06-01

## Scope

`BasicTerminal` supports these cursor save/restore forms:

- `CSI ? 1048 h` / `CSI ? 1048 l`
- `ESC 7` / `ESC 8`
- default `CSI s` / `CSI u`

Both forms use the same screen-local saved cursor slot. Saving records the
active screen, cursor position, active locking charset, and G0-G3 charset
designations, current SGR style, and the current DECSCA protected-character
attribute, plus DECOM origin mode, DECAWM autowrap mode, and pending autowrap
state. Restoring applies only when the saved cursor belongs to the currently
active screen.

## Behavior

Restore clamps the saved position to the current grid size, so a saved cursor
from before a resize cannot move outside the visible terminal. Restoring also
clears pending `SS2`/`SS3` single-shift state because that one-shot parser
state is not part of the saved cursor. Restored pending autowrap applies only
when the restored autowrap mode is enabled.

Cursor style, visibility, insert mode, active hyperlink, and other DEC private
cursor state remain future compatibility work.

The pending-autowrap behavior is aligned with Warp's saved `Cursor`, which
includes `input_needs_wrap`.

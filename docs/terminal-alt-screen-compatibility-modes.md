# Terminal Alternate Screen Compatibility Modes

Updated: 2026-05-30

## Scope

`BasicTerminal` now supports the common alternate-screen compatibility modes in
addition to `1049`:

- `CSI ? 47 h/l`: legacy alternate-screen switch.
- `CSI ? 1047 h/l`: alternate-screen switch.
- `CSI ? 1048 h/l`: save/restore active cursor position.
- `CSI ? 1049 h/l`: save main cursor, switch alternate screen, and clear
  alternate screen on entry.

## Behavior

`47` and `1047` switch between the main and alternate buffers without clearing
the alternate buffer. This lets retained alternate content survive a leave and
re-enter cycle.

`1048` saves and restores the cursor position for the currently active screen.
Restores are screen-local: a cursor saved on the main screen is not applied
while the alternate screen is active.

`1049` keeps its full-screen-app behavior: it saves the main cursor position,
clears the alternate buffer on entry, resets the alternate cursor to `(0, 0)`,
and restores the saved main cursor position on exit. Restored cursor positions
are clamped after resize.

## Follow-Ups

- Add real application smoke coverage with `less`, `vim`, or a small scripted
  TUI once the PTY/browser smoke harness can deterministically drive them.

# Terminal Line Editing Controls

Updated: 2026-05-30

## Scope

`BasicTerminal` supports a focused set of CSI editing controls used by many
shell prompts and full-screen TUIs:

- `CSI Ps @`: insert blank characters at the cursor.
- `CSI Ps P`: delete characters at the cursor.
- `CSI Ps X`: erase characters at the cursor without shifting.
- `CSI Ps L`: insert blank lines at the cursor.
- `CSI Ps M`: delete lines at the cursor.

Missing `Ps` defaults to `1`.

## Behavior

Character editing is constrained to the current row. Insert shifts cells right
and drops overflow at the row end. Delete shifts cells left and blanks the row
tail. Erase leaves surrounding cells in place.

Line insertion and deletion operate from the cursor row through the active
scroll-region bottom. If the cursor is outside the active scroll region, line
insert/delete is ignored. This keeps fixed headers and footers intact for TUI
layouts that set `CSI top;bottom r`.

Blank cells created by these controls use the current style, matching the
existing erase behavior.

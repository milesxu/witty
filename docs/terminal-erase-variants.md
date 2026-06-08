# Terminal Erase Variants

Updated: 2026-05-30

## Scope

`BasicTerminal` now handles the common CSI `J` and `K` erase variants without
moving the cursor.

Display erase support:

- `CSI 0 J` / `CSI J`: erase from cursor through the end of the visible display.
- `CSI 1 J`: erase from the start of the visible display through the cursor.
- `CSI 2 J`: erase the full visible display.
- `CSI 3 J`: clear scrollback history without clearing the visible display.

Line erase support:

- `CSI 0 K` / `CSI K`: erase from cursor through the end of the line.
- `CSI 1 K`: erase from the start of the line through the cursor.
- `CSI 2 K`: erase the full line.

## Style Behavior

Erased cells are written as spaces using the current effective terminal style.
This matches applications that intentionally clear a region with a selected
foreground or background color.

## Damage Behavior

Partial line and display erases mark affected rows dirty. Full visible-display
erase and scrollback erase use full damage because retained rendering must
rebuild broad row state and viewport composition.

## Follow-Ups

- Revisit fine-grained rectangular damage once the core supports dirty column
  ranges.
- Add compatibility tests for alternate screen behavior when that buffer is
  implemented.

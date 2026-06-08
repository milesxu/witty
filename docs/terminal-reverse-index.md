# Terminal Reverse Index

Updated: 2026-05-30

## Scope

`BasicTerminal` supports `ESC M` reverse index.

## Behavior

When the cursor is at the active scroll-region top margin, reverse index scrolls
that region down by one row and inserts a blank row at the top. Rows outside the
region are preserved.

When the cursor is away from the top margin, reverse index moves the cursor up
one row without changing screen contents.

The default scroll region is the full screen, so `ESC M` at row `0` scrolls the
whole screen down.

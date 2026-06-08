# Terminal Insert Mode

Updated: 2026-05-30

## Scope

`BasicTerminal` supports insert/replace mode (`IRM`):

- Replace mode is the default.
- `CSI 4 h` enables insert mode.
- `CSI 4 l` disables insert mode.
- In insert mode, printable input shifts cells from the cursor to the right edge
  one cell to the right before writing the character, dropping the previous final
  cell.
- In replace mode, printable input overwrites the current cell.
- Full terminal reset restores replace mode.

This mode affects printable character input only. Explicit CSI character editing
controls such as `ICH`, `DCH`, and `ECH` keep their own behavior.

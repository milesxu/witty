# Terminal Origin Mode

Updated: 2026-05-30

## Scope

`BasicTerminal` supports DEC origin mode (`DECOM`):

- `CSI ? 6 h` enables origin mode and homes the cursor to the active origin.
- `CSI ? 6 l` disables origin mode and homes the cursor to the absolute top-left.
- When origin mode is enabled, `CUP` / `HVP` row addressing is relative to the
  top of the active scroll region.
- Addressed and relative cursor movement is clamped to the active scroll region
  while origin mode is enabled.
- Setting a scroll region homes the cursor to the region top when origin mode is
  enabled, otherwise to the absolute top-left.
- Full terminal reset clears origin mode.

Origin mode is terminal-global, matching the existing scroll-region state.

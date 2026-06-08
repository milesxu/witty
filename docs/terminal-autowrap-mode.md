# Terminal Autowrap Mode

Updated: 2026-05-30

## Scope

`BasicTerminal` supports DEC autowrap mode (`DECAWM`):

- Autowrap is enabled by default.
- `CSI ? 7 h` enables autowrap.
- `CSI ? 7 l` disables autowrap and clears pending wrap state.
- With autowrap enabled, printing in the final column leaves the cursor on that
  column and sets a pending wrap. The next printable character performs the wrap
  before it is written.
- SGR style changes do not clear pending wrap state.
- With autowrap disabled, printable characters at the final column overwrite the
  final cell and the cursor remains there.
- Full terminal reset restores autowrap mode.

Autowrap is terminal-global rather than main/alternate-screen local.

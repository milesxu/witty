# Terminal Linefeed Newline Mode

Updated: 2026-06-01

`BasicTerminal` supports ANSI linefeed/newline mode (`LNM`, mode `20`):

- `CSI 20 h` enables newline mode.
- `CSI 20 l` disables newline mode.
- `CSI 20 $ p` reports the current state through the existing ANSI
  request-mode-report path.

When enabled, LF, VT, and FF controls first perform carriage return, then
linefeed. When disabled, those controls keep the current column while moving to
the next line or scrolling at the bottom margin.

`DECSTR` soft reset and `ESC c` full reset disable newline mode.

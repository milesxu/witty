# Terminal Reverse Video Mode

Updated: 2026-06-01

`BasicTerminal` supports DEC screen mode (`DECSCNM`, private mode `5`):

- `CSI ? 5 h`: enable reverse-video display.
- `CSI ? 5 l`: disable reverse-video display.
- `CSI ? 5 $ p`: report the current mode state through the DEC
  request-mode-report path.

The mode is terminal-global. It does not rewrite stored cells; instead
`RenderSnapshot` style resolution swaps each cell's final foreground and
background colors while the mode is active. This keeps previously printed cells,
future cells, palette changes, and SGR reverse-video cells on the same rendering
path. A cell already using SGR `7` is swapped when written, then swapped again
by DECSCNM while screen reverse-video is active.

`DECSTR` soft reset and `ESC c` full reset clear the mode. Toggling the mode
marks full damage because all visible cell colors change.

## Boundary

This is a core snapshot/style compatibility slice. It does not launch native
windows, Chromium, Vulkan, WebGPU, or renderer runtime validation.

## Verification

- `cargo test -p witty-core reverse_video_mode --quiet`
- `cargo test -p witty-core request_mode_report --quiet`
- `cargo test -p witty-core --quiet`

# Terminal UTF-8 Mouse Encoding

Updated: 2026-06-01

`m432-terminal-utf8-mouse-encoding` adds xterm private mode `1005`:

- `CSI ? 1005 h`: enable UTF-8 legacy mouse coordinate encoding.
- `CSI ? 1005 l`: disable UTF-8 legacy mouse coordinate encoding.
- `CSI ? 1005 $ p`: report the mode through the DEC request-mode-report path.

The shared `witty-core` mouse encoder now exposes `MouseEncodingMode::Utf8`.
It keeps the legacy `CSI M Cb Cx Cy` packet shape but UTF-8 encodes each
`value + 32` field, allowing cell coordinates beyond the single-byte X10 range.

Encoding precedence remains:

1. `1016` SGR pixel mode
2. `1006` SGR cell mode
3. `1015` urxvt decimal legacy mode
4. `1005` UTF-8 legacy mode
5. X10 byte mode

Full reset clears the `1005` flag with the rest of mouse state.

## Boundary

This is a shared encoder and mode-tracking slice only. It does not add runtime
mouse event capture, native window validation, browser pointer smoke tests,
Chromium, Vulkan, WebGPU, or renderer execution.

## Verification

- `cargo test -p witty-core utf8_encoder_extends_legacy_mouse_coordinates --quiet`
- `cargo test -p witty-core mouse_modes_track_encoding_focus_and_alternate_scroll --quiet`
- `cargo test -p witty-core request_mode_report --quiet`
- `cargo test -p witty-core --quiet`

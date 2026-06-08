# Terminal Urxvt Mouse Encoding

Updated: 2026-06-01

`m436-terminal-urxvt-mouse-encoding` adds xterm private mode `1015`:

- `CSI ? 1015 h`: enable urxvt decimal legacy mouse coordinate encoding.
- `CSI ? 1015 l`: disable urxvt decimal legacy mouse coordinate encoding.
- `CSI ? 1015 $ p`: report the mode through the DEC request-mode-report path.

The shared `witty-core` mouse encoder now exposes `MouseEncodingMode::Urxvt`.
It emits the urxvt packet shape:

```text
CSI Cb ; Cx ; Cy M
```

`Cb` is the legacy xterm button code plus 32. `Cx` and `Cy` are decimal
1-based cell coordinates, so coordinates beyond the single-byte X10 range can
be represented without UTF-8 field encoding.

Encoding precedence is:

1. `1016` SGR pixel mode
2. `1006` SGR cell mode
3. `1015` urxvt decimal legacy mode
4. `1005` UTF-8 legacy mode
5. X10 byte mode

Full reset clears the `1015` flag with the rest of mouse state.

## Boundary

This is a shared encoder and mode-tracking slice only. It does not add runtime
mouse event capture, native window validation, browser pointer smoke tests,
Chromium, Vulkan, WebGPU, or renderer execution.

## Verification

- `cargo test -p witty-core urxvt_encoder_uses_decimal_legacy_mouse_coordinates --quiet`
- `cargo test -p witty-core mouse_modes_track_encoding_focus_and_alternate_scroll --quiet`
- `cargo test -p witty-core private_request_mode_report_covers_mouse_clipboard_and_alt_screen_modes --quiet`
- `cargo test -p witty-core --quiet`

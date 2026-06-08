# Terminal Keyboard Action Mode

Updated: 2026-06-01

`BasicTerminal` supports ANSI keyboard action mode (`KAM`, mode `2`):

- `CSI 2 h`: lock terminal keyboard input.
- `CSI 2 l`: unlock terminal keyboard input.
- `CSI 2 $ p`: report the current mode state through the existing ANSI
  request-mode-report path.

The locked state is exposed through `TerminalInputModes::keyboard_locked`.
Native and browser terminal input encoders return no terminal bytes while it is
set, so user keystrokes are not forwarded to the PTY. Local application
shortcuts and non-terminal UI routing remain outside this mode.

`DECSTR` soft reset and `ESC c` full reset clear the mode.

## Boundary

This is a protocol and input-encoding compatibility slice only. It does not
touch PTY transport, renderer code, browser runtime smoke tests, Chromium,
Vulkan, WebGPU, or native window execution.

## Verification

- `cargo test -p witty-core keyboard_action_mode --quiet`
- `cargo test -p witty-core request_mode_report --quiet`
- `cargo test -p witty-app key_encoder_respects_keyboard_action_mode --quiet`
- `cargo test -p witty-web browser_key_input_respects_keyboard_action_mode --quiet`

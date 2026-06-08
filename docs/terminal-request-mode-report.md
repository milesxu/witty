# Terminal Request Mode Report

Updated: 2026-06-01

`m180-terminal-request-mode-report` extends the existing terminal reply path
with mode status replies for request-mode-report queries.

## Implemented

`BasicTerminal` now queues `TerminalReply` actions for:

- ANSI request mode report: `CSI Ps $ p` -> `CSI Ps ; Pm $ y`
- DEC private request mode report: `CSI ? Ps $ p` -> `CSI ? Ps ; Pm $ y`

The reply status uses the standard `DECRPM` values:

| Status | Meaning |
| ---: | --- |
| `0` | mode is not recognized |
| `1` | mode is set |
| `2` | mode is reset |

Covered ANSI mode:

- `2`: keyboard action mode
- `4`: insert mode
- `20`: linefeed/newline mode

Covered DEC private modes:

- `1`: application cursor keys
- `5`: reverse-video screen mode
- `6`: origin mode
- `7`: autowrap
- `9`, `1000`, `1002`, `1003`: mouse tracking modes
- `12`: cursor blink
- `25`: cursor visibility
- `45`: reverse wraparound
- `66`: application keypad mode
- `67`: backarrow key mode
- `1004`: focus events
- `1005`: UTF-8 mouse encoding
- `1006`: SGR mouse encoding
- `1007`: alternate-scroll wheel mode
- `1016`: SGR pixel mouse encoding
- `1047`, `47`, `1049`: active alternate screen state
- `2004`: bracketed paste
- `2026`: synchronized output mode

## Boundary

Replies use the existing `TerminalHostAction::TerminalReply` boundary. They are
not rendered into terminal cells, exposed through plugin events, or mixed with
clipboard host actions.

The implementation intentionally reports unsupported modes as `0` instead of
guessing. Momentary actions such as cursor save/restore are not exposed as
persistent mode state.

## Verification

Covered by:

- `cargo test -p witty-core request_mode_report --quiet`
- `cargo test -p witty-web browser_host_actions_forward_terminal_replies_and_return_clipboard_writes --quiet`
- `node --check scripts/run-witty-web-smoke.mjs`
- `scripts/run-witty-web-smoke.sh`

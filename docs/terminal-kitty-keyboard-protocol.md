# Terminal Kitty Keyboard Protocol

Updated: 2026-06-24

Witty supports a focused subset of the Kitty keyboard protocol for native and
browser terminal input. This is the keyboard protocol / CSI-u line, not the
Kitty graphics protocol.

Reference: <https://sw.kovidgoyal.net/kitty/keyboard-protocol/>

## Core State

`witty-core` tracks Kitty keyboard flags per active terminal screen:

- `CSI ? u`: query active flags.
- `CSI > flags u`: push current flags and set active flags.
- `CSI = flags ; mode u`: replace, add, or clear active flags.
- `CSI < count u`: pop saved flags.

Supported flags:

- `1`: `DISAMBIGUATE_ESC_CODES`
- `2`: `REPORT_EVENT_TYPES`
- `8`: `REPORT_ALL_KEYS_AS_ESC_CODES`
- `16`: `REPORT_ASSOCIATED_TEXT`

Unsupported flags are masked out. Main and alternate screens have separate
flag values and stacks. Soft reset and full reset clear both stacks and active
flag values.

## Encoding Scope

With flag `1`, Witty emits CSI-u for ambiguous character-key combinations:

- `Ctrl-I` -> `CSI 105;5u`
- `Ctrl-Shift-I` -> `CSI 105;6u`
- `Alt-A` -> `CSI 97;3u`
- `Esc` -> `CSI 27u` or `CSI 27;Nu`

Per the Kitty protocol, flag `1` keeps `Enter`, `Tab`, and `Backspace` on their
legacy byte sequences. This means `Ctrl-Enter`, `Shift-Tab`, and
`Ctrl-Backspace` do not become CSI-u unless flag `8` is also active.

With flag `8`, Witty additionally reports text-producing keys plus `Enter`,
`Tab`, `Backspace`, and `Esc` as CSI-u:

- `a` -> `CSI 97u`
- `Shift-A` -> `CSI 97;2u`
- `Ctrl-Enter` -> `CSI 13;5u`
- `Shift-Tab` -> `CSI 9;2u`
- `Ctrl-Backspace` -> `CSI 127;5u`
- text with no single known key -> `CSI 0u`

With flags `8|16`, Witty adds safe associated text as the third CSI-u
parameter for text-producing character keys:

- `a` -> `CSI 97;;97u`
- `Shift-A` -> `CSI 97;2;65u`
- `Alt-é` -> `CSI 233;3;233u`
- text with no single known key, such as `ab` -> `CSI 0;;97:98u`

Associated text is omitted for `Ctrl`/`Meta` key combinations and omitted when
the text contains C0, DEL, or C1 control codepoints. `REPORT_ASSOCIATED_TEXT`
has no effect unless `REPORT_ALL_KEYS_AS_ESC_CODES` is also active.

With flag `2`, Witty reports Kitty event types on the CSI-u keys it already
encodes:

- key press -> second parameter sub-field `:1`
- key repeat -> second parameter sub-field `:2`
- key release -> second parameter sub-field `:3`

Examples:

- flags `1|2`, `Ctrl-I` press -> `CSI 105;5:1u`
- flags `1|2`, `Ctrl-I` repeat -> `CSI 105;5:2u`
- flags `1|2`, `Ctrl-I` release -> `CSI 105;5:3u`
- flags `8|2`, `a` press -> `CSI 97;1:1u`
- flags `8|2`, `a` release -> `CSI 97;1:3u`
- flags `8|2`, `Ctrl-Enter` release -> `CSI 13;5:3u`
- flags `8|16|2`, `a` press -> `CSI 97;1:1;97u`

`Enter`, `Tab`, and `Backspace` releases are reported only when flag `8` is
also active, because flag `1` alone keeps those keys on legacy byte sequences.
Plain text release events are likewise tied to flag `8`.

Navigation, function, and keypad keys continue through the existing xterm/VT
escape-code encoders. Modified navigation/function keys keep xterm modifier
parameters such as `CSI 1;5A`.

## Deferred

- Alternate key reporting.
- Precise physical-key fallback for shifted symbols.
- Kitty graphics/image protocol.

## Verification

```bash
cargo test -p witty-core kitty_keyboard_protocol --lib
cargo test -p witty-app key_encoder_ --bin witty
cargo test -p witty-web browser_key_input_ --lib
cargo check --workspace
```

# Terminal Kitty Keyboard Protocol

Updated: 2026-06-25

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
- `4`: `REPORT_ALTERNATE_KEYS`
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
- keypad `1` -> `CSI 57400u`
- keypad `Enter` -> `CSI 57414u`

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
- keypad decimal with flags `8|16` -> `CSI 57409;;46u`

Keypad keys that native `winit` or browser metadata identifies as numpad input
use Kitty `KP_*` functional key codes under flags `1` or `8`. Ordinary top-row
digits remain ordinary text unless another Kitty rule applies.

Flag `8` also reports physical modifier keys when native `winit` or browser
keyboard metadata identifies the left/right key:

- left `Shift` press -> `CSI 57441;2u`
- right `Ctrl` press -> `CSI 57448;5u`
- right `Super` release with flags `8|2` -> `CSI 57450;1:3u`

Witty uses native `KeyLocation` / physical `KeyCode` and browser
`KeyboardEvent.location` / `KeyboardEvent.code` for left/right detection. If
the platform only reports a generic modifier key, Witty leaves it unreported
rather than aliasing it to the left-side key code.

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

With flag `4`, Witty adds Kitty alternate key code sub-fields to the first
CSI-u parameter for character keys that Witty is already reporting as CSI-u:

- flags `8|4`, `Shift-A` -> `CSI 97:65;2u`
- flags `8|4`, `Shift-=` producing `+` -> `CSI 61:43;2u`
- flags `8|4`, `é` on the physical `E` key -> `CSI 233::101u`
- flags `1|4`, `Ctrl-Shift-I` -> `CSI 105:73;6u`

The first sub-field is the normalized character key, the second is the shifted
character when Shift produces a distinct character, and the third is the
physical US-layout base key when native `winit` or browser `KeyboardEvent.code`
metadata identifies one. The base key is omitted when it matches the normalized
key.

Navigation and function keys continue through the existing xterm/VT
escape-code encoders. Modified navigation/function keys keep xterm modifier
parameters such as `CSI 1;5A`. Keypad keys use legacy text or application
keypad SS3 sequences until Kitty flags `1` or `8` request disambiguated keypad
reporting.

## Deferred

- Full layout-aware alternate-key reporting beyond the US physical base map.
- Additional Kitty functional-key codes such as Hyper, Meta, ISO level shifts,
  lock keys, media keys, and extended function keys.
- Kitty graphics/image protocol.

## Verification

```bash
cargo test -p witty-core kitty_keyboard_protocol --lib
cargo test -p witty-app key_encoder_ --bin witty
cargo test -p witty-web browser_key_input_ --lib
cargo check --workspace
```

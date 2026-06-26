# Terminal Kitty Keyboard Protocol

Updated: 2026-06-26

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

`witty-core::keyboard` owns the platform-independent terminal key encoder.
Native `winit` input and browser `KeyboardEvent` input are converted into the
shared `TerminalKeyInput` model before escape sequences are generated. This
keeps Kitty/CSI-u behavior identical across native and web builds while
leaving platform-specific key-location and keypad detection in the frontend
layers.

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
- keypad left arrow with flags `8` -> `CSI 57417u`

Keypad keys that native `winit` or browser metadata identifies as numpad input
use Kitty `KP_*` functional key codes under flags `1` or `8`. This includes
numeric keypad digits/operators and NumLock-off navigation semantics such as
`KP_LEFT`, `KP_PAGE_UP`, and `KP_BEGIN`. Ordinary top-row digits and main
navigation keys remain ordinary text or xterm navigation unless another Kitty
rule applies.

Flag `8` also reports physical modifier keys when native `winit` or browser
keyboard metadata identifies the left/right key:

- left `Shift` press -> `CSI 57441;2u`
- right `Ctrl` press -> `CSI 57448;5u`
- right `Super` release with flags `8|2` -> `CSI 57450;1:3u`
- left `Hyper` press -> `CSI 57445;17u`
- native right `Meta` press -> `CSI 57452;33u`

Witty uses native `KeyLocation` / physical `KeyCode` and browser
`KeyboardEvent.location` / `KeyboardEvent.code` for left/right detection. If
the platform only reports a generic modifier key, Witty leaves it unreported
rather than aliasing it to the left-side key code. Browser `Meta` remains
mapped to Kitty `Super`, matching DOM semantics for Windows/Command keys;
native `NamedKey::Meta` maps to Kitty `Meta` when left/right location metadata
is available.

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
encodes and on functional-key escape forms when Kitty protocol mode is active:

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
- flags `1|2`, `ArrowUp` press -> `CSI 1;1:1A`
- flags `1|2`, `Ctrl-ArrowUp` repeat -> `CSI 1;5:2A`
- flags `1|2`, `F5` release -> `CSI 15;1:3~`

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

Navigation and function keys continue through the existing xterm/VT escape-code
encoders when Kitty event-type reporting is not requested. Modified
navigation/function keys keep xterm modifier parameters such as `CSI 1;5A`.
Meta-modified navigation/function keys use Kitty functional forms because the
xterm fallback has no Meta modifier parameter. When flags `1|2` or `8|2` are
active, those functional-key forms include Kitty event-type sub-fields for
press, repeat, and release events.

Witty also reports Kitty PUA functional key codes for keys that do not have a
legacy xterm sequence when flags `1` or `8` are active:

- `F13` -> `CSI 57376u`
- `CapsLock` -> `CSI 57358u`
- browser `MediaReverse` -> `CSI 57431u`
- `MediaTrackNext` release with flags `1|2` -> `CSI 57435;1:3u`
- `AltGraph` / ISO Level 3 Shift -> `CSI 57453u`

The supported native/browser PUA set includes `CapsLock`, `ScrollLock`,
`NumLock`, `PrintScreen`, `Pause`, `ContextMenu`, `F13` through `F35`, common
media keys including browser `MediaReverse`, volume keys, `Hyper` modifier
keys when side metadata is available, native `Meta` modifier keys when side
metadata is available, and `AltGraph`.

When a platform reports AltGraph as both logical `AltGraph` and physical right
Alt, Witty reports the Kitty ISO Level 3 Shift PUA key instead of the right Alt
modifier-key PUA code. Plain right Alt events still report right Alt when the
logical key is not AltGraph.

Keypad keys use legacy text or application keypad SS3 sequences until Kitty
flags `1` or `8` request disambiguated keypad reporting.

## Diagnostics

`witty --keyboard-protocol-diagnostics` prints a non-GUI JSON report for
representative key encodings. The command does not open a window, create a
renderer surface, or start a PTY. Current cases include legacy `Ctrl-I`, Kitty
`Ctrl-I` disambiguation, Kitty event typing, `Ctrl-Enter`, associated text with
alternate keys, keypad digit/navigation keys, PUA functional keys, AltGraph,
and sided modifier press/release events. Each case reports flags, flag names,
key metadata, hex bytes, and escaped bytes. The JSON also includes
`supportedKittyPuaKeys`, a table of Witty's current Kitty PUA key names and
codepoints for capture comparison.

`witty --keyboard-protocol-capture` is the live companion tool. It requires
stdin to be a terminal, temporarily switches the terminal to `stty raw -echo`,
prints the hex and escaped bytes sent by the current terminal for each key
event, and restores the previous `stty -g` state on exit. Press `Ctrl-C` to
leave capture mode. This captures terminal-emitted bytes for comparison.

`witty --keyboard-protocol-live-compare` is the guided live comparison tool. It
requires terminal stdin and stderr, temporarily switches to raw input, sends
Kitty keyboard `CSI > flags u` / `CSI < u` push-pop controls around each case,
prompts for practical representative keys such as `Ctrl-I` and `Ctrl-Enter`,
then prints a JSON report comparing terminal-emitted bytes with Witty's expected
encoding. Prompts and control bytes go to stderr; the final JSON goes to stdout
so it can be redirected to a file.

`witty --keyboard-protocol-native-diagnostics` opens a minimal native `winit`
diagnostic window without starting a PTY or Witty renderer. Each key event is
printed as one JSON line with the native logical key, physical key, location,
modifier state, Witty's resolved modifier/keypad/base-layout metadata, and
legacy/Kitty encoded byte sequences. Use it to compare Witty's native input
metadata with `--keyboard-protocol-capture` output from Kitty, WezTerm, or
Ghostty.

The browser build exports
`witty_browser_keyboard_protocol_diagnostic_report_json(...)`, and `app.js`
wraps it as `window.wittyKeyboardProtocolDiagnostic(eventOrFields)`. The report
contains DOM `key`/`code`/`location`, Witty's resolved browser
modifier/keypad/base-layout metadata, and legacy/Kitty encoded byte sequences.
The most recent browser keydown/keyup report is also stored in
`window.wittyLastKeyboardProtocolDiagnostic`.

The browser page also includes a compact diagnostic panel below the terminal
canvas. Open it with the `Keys` button or `Ctrl+Shift+K`. The panel displays
the latest DOM key metadata, Witty modifier/keypad/base-layout resolution,
legacy bytes, Kitty flag-1 bytes, all-feature Kitty bytes, and a short
recent-event history. Test code can control it through:

```js
window.wittyKeyboardProtocolDiagnosticPanel.open();
window.wittyKeyboardProtocolDiagnosticPanel.close();
window.wittyKeyboardProtocolDiagnosticPanel.toggle();
window.wittyKeyboardProtocolDiagnosticPanel.state();
```

The Playwright smoke script contains a browser-side assertion for this panel:
it opens the panel, asks `window.wittyKeyboardProtocolDiagnostic(...)` to report
`Ctrl-I`, and checks that the panel shows `CSI 105;5u` for Kitty flag 1.

## Current Local Validation

Local validation on 2026-06-26 found:

- Kitty is installed: `kitty 0.46.0`.
- Neovim is installed: `NVIM v0.12.2`.
- tmux is installed: `tmux 3.6b`.
- WezTerm and Ghostty are not installed on this machine, so live comparison
  against those terminals remains manual/future work.

Automated checks passed:

- `cargo test -p witty-core kitty_keyboard_protocol --lib`
- `cargo test -p witty-core keyboard --lib`
- `cargo test -p witty-app key_encoder_ --bin witty`
- `scripts/run-witty-native-opengl.sh --keyboard-protocol-diagnostics`
- `cargo run -p witty-app -- --keyboard-protocol-live-compare`
- `cargo run -p witty-app -- --real-tui-smoke nvim-kitty-keyboard`
- `cargo test -p witty-web browser_key_input_ --lib`
- `cargo test -p witty-web browser_keyboard_protocol_diagnostics --lib`
- `cargo check -p witty-web`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `node --check crates/witty-web/static/app.js`
- `node --check scripts/run-witty-web-smoke.mjs`

Full Chromium/WebGPU browser smoke was not run during this local validation
unless explicitly overridden, because `.witty-local-opengl-only` marks that
runtime path as deferred on this machine.

The Neovim real-TUI smoke artifact was written to:

```text
target/real-tui-smoke/nvim-kitty-keyboard.json
```

Key assertions from that run:

- Neovim enabled Kitty keyboard flags `3`
  (`DISAMBIGUATE_ESC_CODES | REPORT_EVENT_TYPES`).
- Witty encoded `Ctrl-I` as `CSI 105;5:1u` while Kitty event reporting was
  active.
- Neovim selected the `<C-I>` mapping instead of the `<Tab>` mapping.

This proves Witty's core/native encoder path works for the most important
Neovim Kitty keyboard compatibility case. Remaining live validation should
compare Witty's native event metadata against Kitty's emitted bytes for physical
keyboard details such as keypad keys and sided modifiers.

## Deferred

- Full layout-aware alternate-key reporting beyond the US physical base map.
- Less common Kitty functional-key codes such as ISO level shifts beyond
  `AltGraph` and platform-specific media/application keys.
- Live comparison captures against WezTerm and Ghostty once those terminals are
  installed locally.
- Kitty graphics/image protocol.

## Verification

```bash
cargo test -p witty-core kitty_keyboard_protocol --lib
cargo test -p witty-core keyboard --lib
cargo test -p witty-app key_encoder_ --bin witty
cargo run -p witty-app -- --keyboard-protocol-diagnostics
cargo run -p witty-app -- --real-tui-smoke nvim-kitty-keyboard
cargo test -p witty-web browser_key_input_ --lib
node --check crates/witty-web/static/app.js
node --check scripts/run-witty-web-smoke.mjs
cargo check --workspace
```

Manual live capture:

```bash
cargo run -p witty-app -- --keyboard-protocol-capture
cargo run -p witty-app -- --keyboard-protocol-native-diagnostics
```

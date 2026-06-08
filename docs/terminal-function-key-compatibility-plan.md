# Terminal Function-Key Compatibility Plan

Updated: 2026-05-30

## Goal

Bring function/navigation key encoding closer to `xterm-256color` without
expanding into every xterm keyboard option at once.

Current gaps:

- Native and browser encoders do not emit `Insert`.
- Native and browser encoders do not emit `F1` through `F12`.
- `Home` and `End` always use `ESC [ H` / `ESC [ F`; application cursor-key
  mode is not reflected for those keys.
- Shift/Alt/Ctrl modifiers are carried by the event shape but most navigation
  and function keys ignore them.

The m99 target should be an xterm-style default profile for normal application
use, not a full keyboard-profile system.

## Baseline Source

Use the local `xterm-256color` terminfo as the initial baseline:

- `khome = ESC O H`
- `kend = ESC O F`
- `kich1 = ESC [ 2 ~`
- `kdch1 = ESC [ 3 ~`
- `kpp = ESC [ 5 ~`
- `knp = ESC [ 6 ~`
- `kf1..kf4 = ESC O P/Q/R/S`
- `kf5..kf12 = ESC [ 15/17/18/19/20/21/23/24 ~`
- modified cursor/navigation/function keys use xterm modifier parameters
  such as `ESC [ 1 ; 2 A` and `ESC [ 15 ; 5 ~`

The existing code already handles Delete, PageUp, PageDown, arrows, and
Home/End in a simpler form. m99 should preserve compatibility for unmodified
keys where changing behavior would be risky, and only switch to xterm forms
when the terminal mode or modifier clearly calls for it.

## Modifier Model

Use the xterm modifier parameter values:

| Modifier set | Parameter |
| --- | --- |
| Shift | `2` |
| Alt | `3` |
| Shift+Alt | `4` |
| Ctrl | `5` |
| Shift+Ctrl | `6` |
| Alt+Ctrl | `7` |
| Shift+Alt+Ctrl | `8` |

Do not include Meta/Super in m99. The browser path currently carries `meta` so
that it can suppress application keypad expansion; mapping Meta/Super to xterm
keyboard parameters should be a separate profile decision.

Recommended helper:

```text
modifier_parameter(modifiers) -> Option<u8>
```

Return `None` for no Shift/Alt/Ctrl and for any Meta/Super participation.

## Encoding Rules

Apply the rules in this order:

1. Preserve application keypad handling from m96.
2. Encode modified navigation/function keys using CSI parameterized forms.
3. Encode unmodified cursor keys using existing m94 behavior.
4. Encode unmodified Home/End using current normal-mode behavior, but use SS3
   `ESC O H/F` when `application_cursor_keys` is enabled.
5. Encode unmodified Insert, Delete, PageUp, PageDown, and F1-F12 using the
   xterm baseline.
6. Fall back to existing control-character and text handling.

When a modifier parameter exists, prefer CSI parameterized sequences over SS3,
even if application cursor-key mode is active. This matches the terminfo family
for modified cursor/navigation keys.

## Target Sequence Map

Unmodified cursor/navigation keys:

| Key | Normal mode | Application cursor-key mode |
| --- | --- | --- |
| ArrowUp | `ESC [ A` | `ESC O A` |
| ArrowDown | `ESC [ B` | `ESC O B` |
| ArrowRight | `ESC [ C` | `ESC O C` |
| ArrowLeft | `ESC [ D` | `ESC O D` |
| Home | `ESC [ H` | `ESC O H` |
| End | `ESC [ F` | `ESC O F` |
| Insert | `ESC [ 2 ~` | same |
| Delete | `ESC [ 3 ~` | same |
| PageUp | `ESC [ 5 ~` | same |
| PageDown | `ESC [ 6 ~` | same |

Modified cursor/navigation keys:

| Key | Form |
| --- | --- |
| ArrowUp | `ESC [ 1 ; m A` |
| ArrowDown | `ESC [ 1 ; m B` |
| ArrowRight | `ESC [ 1 ; m C` |
| ArrowLeft | `ESC [ 1 ; m D` |
| Home | `ESC [ 1 ; m H` |
| End | `ESC [ 1 ; m F` |
| Insert | `ESC [ 2 ; m ~` |
| Delete | `ESC [ 3 ; m ~` |
| PageUp | `ESC [ 5 ; m ~` |
| PageDown | `ESC [ 6 ; m ~` |

`m` is the xterm modifier parameter from the table above.

Unmodified function keys:

| Key | Sequence |
| --- | --- |
| F1 | `ESC O P` |
| F2 | `ESC O Q` |
| F3 | `ESC O R` |
| F4 | `ESC O S` |
| F5 | `ESC [ 15 ~` |
| F6 | `ESC [ 17 ~` |
| F7 | `ESC [ 18 ~` |
| F8 | `ESC [ 19 ~` |
| F9 | `ESC [ 20 ~` |
| F10 | `ESC [ 21 ~` |
| F11 | `ESC [ 23 ~` |
| F12 | `ESC [ 24 ~` |

Modified function keys:

| Key | Form |
| --- | --- |
| F1 | `ESC [ 1 ; m P` |
| F2 | `ESC [ 1 ; m Q` |
| F3 | `ESC [ 1 ; m R` |
| F4 | `ESC [ 1 ; m S` |
| F5 | `ESC [ 15 ; m ~` |
| F6 | `ESC [ 17 ; m ~` |
| F7 | `ESC [ 18 ; m ~` |
| F8 | `ESC [ 19 ; m ~` |
| F9 | `ESC [ 20 ; m ~` |
| F10 | `ESC [ 21 ; m ~` |
| F11 | `ESC [ 23 ; m ~` |
| F12 | `ESC [ 24 ; m ~` |

## Native And Browser Mapping

Native:

- Reuse `TerminalKeyInput` and `TerminalKeyModifiers`.
- Add `NamedKey::Insert` and `NamedKey::F1` through `NamedKey::F12` handling.
- Be aware that `F1`, `F2`, and `F3` can be intercepted by existing app
  shortcuts before `send_key`; unit tests should target the encoder directly.

Browser:

- Reuse `BrowserTerminalKeyInput` and `BrowserKeyModifiers`.
- Browser `event.key` values should be `"Insert"`, `"F1"` through `"F12"`,
  `"Home"`, `"End"`, and the existing arrow/page/delete strings.
- No additional DOM metadata is required for m99.

## Tests For m99

Native unit tests:

- unmodified F1-F4 emit SS3 `ESC O P/Q/R/S`
- unmodified F5-F12 emit `ESC [ n ~`
- `Insert` emits `ESC [ 2 ~`
- `Home`/`End` keep `ESC [ H/F` in normal cursor-key mode
- `Home`/`End` emit `ESC O H/F` in application cursor-key mode
- Shift/Ctrl/Alt variants of arrows, Home/End, Insert/Delete/PageUp/PageDown,
  and F1/F5 emit the parameterized forms
- Meta/Super participation returns the existing unmodified sequence or no
  modifier expansion, depending on the key

Browser unit tests:

- mirror the native sequence tests for `"F1"`, `"F5"`, `"Insert"`,
  `"Home"`, and modified arrows/navigation keys
- verify `modifierMask` values still decode to Shift/Alt/Meta correctly

Smoke tests:

- Extend browser smoke only if unit coverage misses a wasm-bindgen boundary.
  The m97 keypad smoke already exercises the DOM metadata path, so m99 can
  probably stay unit-test focused unless wasm export signatures change.

Regression commands:

- `cargo test -p witty-app -- --nocapture`
- `cargo test -p witty-web -- --nocapture`
- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `scripts/run-witty-web-smoke.sh`

## Deferred

- F13-F24 direct support and mapping.
- Meta/Super keyboard profile behavior.
- xterm `modifyOtherKeys`.
- Alternate keyboard dialects such as Linux console, rxvt, iTerm2 proprietary
  mappings, and kitty keyboard protocol.
- User-selectable keyboard profile configuration.

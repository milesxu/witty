# Terminal Mouse Mode Core

Updated: 2026-05-30

The terminal core now tracks xterm-style mouse reporting modes as part of
`TerminalInputModes`.

## Public Mode Snapshot

`TerminalInputModes` now includes:

```rust
pub mouse: TerminalMouseModes
```

`TerminalMouseModes` exposes:

- `tracking: MouseTrackingMode`
- `encoding: MouseEncodingMode`
- `focus_events: bool`
- `alternate_scroll: bool`

Tracking modes:

- `None`
- `X10`
- `Normal`
- `ButtonEvent`
- `AnyEvent`

Encoding modes:

- `X10`
- `Utf8`
- `Urxvt`
- `Sgr`
- `SgrPixels`

`BasicTerminal::mouse_modes()` exposes the same mouse snapshot directly.

## Parsed Private Modes

The core now tracks these DEC private modes:

| Mode | Meaning |
| --- | --- |
| `9` | X10 mouse tracking |
| `1000` | normal press/release tracking |
| `1002` | button-event tracking |
| `1003` | any-event tracking |
| `1004` | focus event reporting |
| `1005` | UTF-8 legacy mouse encoding |
| `1006` | SGR mouse encoding |
| `1007` | alternate-scroll mode |
| `1015` | urxvt decimal legacy mouse encoding |
| `1016` | SGR pixel-position encoding |

Full reset clears all mouse state.

## Notes

The core stores `1005`, `1006`, `1015`, and `1016` as independent internal
flags. If multiple encodings are enabled, the exposed precedence is
`SgrPixels`, `Sgr`, `Urxvt`, `Utf8`, then `X10`.

m102 only tracks modes. Mouse event encoding and native/browser event routing
are intentionally deferred to m103 and m104.

## Verification

- `cargo test -p witty-core -- --nocapture`
- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `scripts/run-witty-web-smoke.sh`

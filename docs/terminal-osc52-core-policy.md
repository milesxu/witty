# Terminal OSC 52 Core Policy

Updated: 2026-05-30

m138 implements the terminal-core half of OSC 52 clipboard support. It parses
valid OSC 52 write requests into host actions, but does not write the system
clipboard itself.

## Implemented Surface

New shared core types:

- `TerminalHostAction`
- `TerminalClipboardWrite`
- `TerminalClipboardSelection`
- `Osc52ClipboardPolicy`
- `MAX_OSC52_DECODED_BYTES`

New API:

```rust
impl BasicTerminal {
    pub fn drain_host_actions(&mut self) -> Vec<TerminalHostAction>;
}
```

`BasicTerminal::feed()` remains content-focused and returns `()`. Host actions
are queued privately inside `BasicTerminalState` and must be explicitly drained
by trusted native/browser host code after feeding output bytes.

## Parser Behavior

Supported write forms:

```text
OSC 52 ; c ; <base64-text> ST
OSC 52 ; c ; <base64-text> BEL
OSC 52 ; ; <base64-text> ST
OSC 52 ; p ; <base64-text> ST
```

Target handling:

- empty `Pc` defaults to clipboard.
- any `Pc` containing `c` maps to `TerminalClipboardSelection::Clipboard`.
- exactly `p` maps to `TerminalClipboardSelection::Primary`.
- unsupported targets are ignored.
- query payload `?` is ignored and sends no reply bytes.

Payload handling:

- decoded payloads are capped by `MAX_OSC52_DECODED_BYTES`.
- encoded payloads are capped before decode to avoid large transient allocation.
- invalid base64 is ignored.
- invalid UTF-8 is ignored.
- NUL and non-text C0/C1 controls are ignored.
- tab, line feed, and carriage return are allowed as normal clipboard text.
- Unicode text is preserved as-is; no normalization is applied.

## Privacy Boundary

OSC 52 payloads are not added to:

- `RenderSnapshot`
- `FramePlan`
- terminal cells or scrollback
- title or hyperlink metadata
- plugin events or command args
- search state/history

The host action queue is trusted host-internal state. Native/browser code must
apply `disabled|confirm|allow` policy before calling platform clipboard APIs.

## Verification

Passed:

- `cargo fmt --all -- --check`
- `cargo test -p witty-core osc52 --quiet`
- `cargo test -p witty-core --quiet`
- `cargo check -p witty-core --target wasm32-unknown-unknown --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `cargo test --workspace --quiet`
- `cargo clippy --workspace --all-targets -- -D warnings`

## Follow-Up

`m139-native-osc52-clipboard` is complete. See
`terminal-osc52-native-clipboard.md` for the native host policy wiring.

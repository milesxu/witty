# Terminal Bell Host Action

Updated: 2026-06-01

`m330-terminal-bell-host-action` adds the first host-action boundary for the
terminal bell control character.

## Behavior

- `BEL` (`0x07`) queues `TerminalHostAction::Bell`.
- Bell is not rendered into terminal cells.
- Bell does not mark terminal rows dirty or produce terminal reply bytes.
- Bell is drained through the same host-action path as terminal replies, OSC 52
  clipboard writes, and OSC 133 shell-integration events.
- Native and browser host-action drains currently ignore bell actions. Audible
  sound, visual flash, notifications, throttling, and user settings are deferred
  to a separate host-policy slice.

## Boundary

The terminal core records that an application requested attention. Host shells
decide how to present that request. Keeping bell as a typed host action avoids
mixing it with printable output, transport replies, plugin events, or clipboard
actions.

## Verification

- `cargo test -p witty-core bell --quiet`
- `cargo test -p witty-app bell_host_actions --quiet`
- `cargo test -p witty-web browser_bell --quiet`
- `cargo test -p witty-core --quiet`
- `cargo test -p witty-app --quiet`
- `cargo test -p witty-web --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `cargo clippy -p witty-core -p witty-app -p witty-web --all-targets -- -D warnings`

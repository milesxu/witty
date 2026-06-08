# Terminal Device Identity Replies

Updated: 2026-06-01

`m450-terminal-tertiary-device-attributes` and
`m452-terminal-xtversion-reply` extend the host-reply path for terminal identity
queries.

## Implemented

`BasicTerminal` queues `TerminalReply` actions for:

- `CSI = c` / `CSI = 0 c`: DA3 / tertiary device attributes.
- `CSI > q` / `CSI > 0 q`: XTVERSION terminal name/version query.

DA3 replies with a deterministic zero unit id:

```text
DCS ! | 00000000 ST
```

XTVERSION replies with the crate version:

```text
DCS > | Witty <version> ST
```

Both replies use the same `TerminalHostAction::TerminalReply` transport path as
DA, DSR, CPR, DECRQSS, and palette queries.

## Boundary

This does not expose host OS details, build metadata, profile ids, SSH target
names, plugin inventory, or renderer/backend information. Nonzero parameters
are ignored.

## Verification

- `cargo test -p witty-core device_attributes --quiet`
- `cargo test -p witty-core xtversion --quiet`

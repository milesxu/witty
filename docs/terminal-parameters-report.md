# Terminal Parameters Report

Updated: 2026-06-01

`m454-terminal-parameters-report` adds a bounded DECREQTPARM reply path in
`witty-core`.

## Implemented

`BasicTerminal` queues `TerminalReply` actions for:

- `CSI x`
- `CSI 0 x`
- `CSI 1 x`

The default/`0` query returns:

```text
CSI 2 ; 1 ; 1 ; 128 ; 128 ; 1 ; 0 x
```

The `1` query returns:

```text
CSI 3 ; 1 ; 1 ; 128 ; 128 ; 1 ; 0 x
```

This mirrors the common static terminal-parameter shape used by modern
emulators for compatibility probes.

## Boundary

The reply is intentionally static. Witty does not expose serial line
settings, host transport details, baud rate, or PTY implementation state.
Parameters other than missing/`0` and `1` are ignored.

## Verification

- `cargo test -p witty-core terminal_parameters_report --quiet`

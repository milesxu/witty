# Terminal Query Reply Path

Updated: 2026-05-30

m142 adds a host-internal reply path for terminal queries that must send bytes
back to the foreground program instead of mutating visible terminal cells.

## Implemented Surface

`TerminalHostAction` now has two action classes:

```rust
ClipboardWrite(TerminalClipboardWrite)
TerminalReply(TerminalHostReply)
```

`BasicTerminal` queues `TerminalReply` actions for:

- primary DA: `CSI c` / `CSI 0 c` / `ESC Z` -> `CSI ? 1 ; 2 c`
- secondary DA: `CSI > c` / `CSI > 0 c` -> `CSI > 0 ; 1 ; 0 c`
- tertiary DA: `CSI = c` / `CSI = 0 c` -> `DCS ! | 00000000 ST`
- XTVERSION: `CSI > q` / `CSI > 0 q` -> `DCS > | Witty <version> ST`
- DSR status: `CSI 5 n` -> `CSI 0 n`
- CPR: `CSI 6 n` -> `CSI row ; col R`
- DEC private CPR: `CSI ? 6 n` -> `CSI ? row ; col R`
- terminal parameters: `CSI x` / `CSI 0 x` / `CSI 1 x`
  -> `CSI 2/3 ; 1 ; 1 ; 128 ; 128 ; 1 ; 0 x`
- XTGETTCAP: `DCS + q Pt ST` -> `DCS 1 + r name [ = value ] ST`
  or `DCS 0 + r name ST`
- ANSI request mode report: `CSI Ps $ p` -> `CSI Ps ; Pm $ y`
- DEC private request mode report: `CSI ? Ps $ p` -> `CSI ? Ps ; Pm $ y`
- window size query: `CSI 18 t` / `CSI 19 t` -> `CSI 8 ; rows ; cols t`
- palette query: `OSC 4 ; index ; ? ST` -> `OSC 4 ; index ; rgb:... ST`
- default foreground/background/cursor color query: `OSC 10 ; ? ST`,
  `OSC 11 ; ? ST`, and `OSC 12 ; ? ST` -> `OSC 10/11/12 ; rgb:... ST`

Rows and columns are reported as 1-based terminal coordinates.
Request-mode-report statuses are `0` for not recognized, `1` for set, and `2`
for reset. See `terminal-request-mode-report.md`.
Device identity replies are documented in `terminal-device-identity-replies.md`.
Terminal parameter replies are documented in `terminal-parameters-report.md`.
XTGETTCAP replies are documented in `terminal-xtgettcap-capabilities.md`.

## Transport Boundary

Native:

```text
PTY output -> BasicTerminal::feed -> drain_host_actions
  -> TerminalReply bytes -> TerminalApp::write_input -> local PTY writer
```

Browser:

```text
gateway output -> BasicTerminal::feed -> drain_host_actions
  -> TerminalReply bytes -> BrowserGatewayTransport::write
  -> drain_outbound_message_json -> WebSocket input frame
```

Clipboard host actions still stay behind the OSC 52 policy path. Terminal reply
bytes are not rendered, included in screen text, exposed as clipboard actions,
or routed through plugin events.

## Verification

Passed:

- `cargo fmt`
- `cargo test -p witty-core`
- `cargo test -p witty-web`
- `cargo test -p witty-app`

Coverage includes core DA/DSR/CPR action generation, native transport sink
forwarding, browser host-action splitting, and a Playwright node-gateway smoke
case for browser query replies.

## Follow-Up

m143 can now build real TUI smokes around `less`, `vim`/`nvim`, `tmux`, and
`vttest` subsets without blocking on terminal identity and cursor-position
queries.

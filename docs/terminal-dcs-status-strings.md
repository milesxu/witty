# Terminal DCS Status Strings

`m400-terminal-dcs-decrqss-core` adds a minimal `witty-core` implementation of
DECRQSS, request status string. `m444-terminal-decrqss-decscl-status` extends
that path with the fixed DECSCL conformance-level reply, and
`m446-terminal-decrqss-decslrm-status` reports the current full-width left/right
margin boundary:

```text
DCS $ q Pt ST
```

Replies are emitted as `TerminalHostAction::TerminalReply`, not rendered as
screen text:

```text
DCS 1 $ r Ps ST
DCS 0 $ r Pt ST
```

## Supported Requests

| Request payload | Reports |
| --- | --- |
| `"p` | current DECSCL conformance level as `65;1 " p` |
| `m` | current SGR style as `Ps m` |
| `r` | current top/bottom scroll region as `Pt;Pb r` |
| `s` | current left/right margin as `1;cols s` |
| ` q` | current DECSCUSR cursor style as `Ps SP q` |
| `"q` | current DECSCA character protection attribute as `Ps " q` |

Witty reports DECSCL as `65;1"p`, matching a VT500-level UTF-8 terminal
that keeps C1 replies on the 7-bit-compatible host-reply path. The DECSCL
selection control itself is consumed as an explicit no-op and is documented in
`terminal-decscl-conformance-level.md`.

Witty does not yet implement DECSLRM horizontal margin state, so the `s`
status string reports the effective full terminal width as `1;cols s`. This
keeps DECRQSS callers from receiving a negative reply while avoiding partial
left/right margin behavior in cursor movement and editing controls.

The SGR report serializes the tracked style state that already exists in
`BasicTerminal`: bold, faint, italic, underline variants, blink, reverse,
conceal, strike, framed, encircled, overline, superscript/subscript,
foreground/background color, and underline color. Default style reports `0m`.
DECSCA reports `1"q` while new cells are marked protected and `0"q` otherwise.

Unknown safe printable request payloads receive a negative `DCS 0 $ r ... ST`
reply. Oversized or control-containing requests are ignored instead of echoed
back through the host-action boundary; the collector is capped at 64 bytes.

## Non-Goals

- General DCS passthrough.
- XTSETTCAP or dynamic terminfo mutation.
- Sixel, ReGIS, or other graphics protocols.
- Full DECRQSS coverage for controls whose state Witty does not yet track.

## Verification

- `cargo test -p witty-core decrqss --quiet`
- `cargo test -p witty-core c1_ --quiet`

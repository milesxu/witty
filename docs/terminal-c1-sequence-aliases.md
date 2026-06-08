# Terminal C1 Sequence Aliases

`m356-terminal-c1-sequence-aliases`,
`m358-terminal-c1-string-aliases`, and
`m438-terminal-utf8-encoded-c1-aliases` add UTF-8 aware handling for common
8-bit C1 sequence and string introducers that `vte 0.15` does not parse as
protocol state. The related `m442-terminal-c1-transmission-mode-noop` slice
consumes `ESC SP F` and `ESC SP G` without changing which C1 forms are accepted.

## Supported Aliases

`BasicTerminal::feed` now normalizes these standalone raw C1 bytes before they
reach `vte::Parser`:

| C1 byte | Name | Equivalent 7-bit sequence |
| --- | --- | --- |
| `0x90` | DCS | `ESC P` |
| `0x98` | SOS | `ESC X` |
| `0x9b` | CSI | `ESC [` |
| `0x9c` | ST | `ESC \` |
| `0x9d` | OSC | `ESC ]` |
| `0x9e` | PM | `ESC ^` |
| `0x9f` | APC | `ESC _` |

This lets existing `witty-core` CSI and OSC handlers work for 8-bit CSI, OSC,
and ST without changing renderer, PTY transport, browser, plugin, or host-action
surfaces.

The DCS alias reuses the same `vte` DCS string state as `ESC P`. `witty-core`
implements the small DECRQSS subset documented in
`terminal-dcs-status-strings.md`; unsupported DCS payloads remain shielded from
visible cells. SOS/PM/APC strings are still ignored by the parser.

## UTF-8 Boundary

The normalizer tracks UTF-8 continuation state so legal text bytes are not
rewritten. For example, valid UTF-8 characters whose continuation bytes include
`0x9b`, `0x9c`, or `0x9d` remain printable text, including when the UTF-8
sequence is split across multiple `feed` calls.

Invalid standalone raw C1 bytes keep terminal-control semantics. Existing C1
execute aliases for IND, NEL, HTS, RI, SPA, EPA, DECID, SS2, and SS3 remain
handled by `BasicTerminalState::execute`.

Witty also recognizes supported C1 controls encoded as UTF-8 control-code
scalars (`C2 80` through `C2 9F`). For example, `C2 9B` dispatches as CSI,
`C2 9D` as OSC, and `C2 9C` as ST. This mirrors the compatibility behavior
used by terminals that treat UTF-8 encoded C1 controls as protocol controls
instead of printable text.

The normalizer only withholds a pending `C2` lead byte long enough to decide
whether it is a supported C1 control. Printable Latin-1 text such as `£`
(`C2 A3`) is passed through unchanged, including when the byte sequence is
split across multiple `feed` calls.

## C1 Transmission Selection

Historical S7C1T and S8C1T controls are represented as `ESC SP F` and
`ESC SP G`. Witty treats both as parser-level no-ops because it accepts
the supported 7-bit, raw 8-bit, and UTF-8 encoded C1 forms concurrently. See
`terminal-c1-transmission-mode.md`.

## Deferred

This slice intentionally does not claim full DCS protocol support such as
sixel, ReGIS, XTSETTCAP, or other passthrough/mutation semantics. XTGETTCAP is
handled separately through the terminal host-action boundary; see
`terminal-xtgettcap-capabilities.md`.

## Verification

- `cargo test -p witty-core c1_ --quiet`
- `cargo test -p witty-core utf8_encoded_c1 --quiet`
- `cargo test -p witty-core c1_transmission_mode --quiet`
- `cargo test -p witty-core utf8 --quiet`

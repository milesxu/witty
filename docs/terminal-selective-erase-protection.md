# Terminal Selective Erase Protection

`m352-terminal-selective-erase-protection` adds the core protected-character
state needed by DEC selective erase controls.

## Supported Controls

`BasicTerminal` now handles:

- `CSI Ps " q` / DECSCA: select character protection attribute.
- `CSI ? Ps J` / DECSED: selective erase in display.
- `CSI ? Ps K` / DECSEL: selective erase in line.

DECSCA parameters:

| Parameter | Behavior |
| --- | --- |
| missing, `0`, `2` | following printed characters are unprotected |
| `1` | following printed characters are protected |

SPA (`ESC V` / C1 `0x96`) enables the same current protected-character
attribute for future printed cells. EPA (`ESC W` / C1 `0x97`) disables it.

DECSED and DECSEL support modes `0`, `1`, and `2`, matching the normal ED/EL
range shapes but only replacing unprotected cells. Protected cells keep their
text, width, style, and hyperlink metadata.

## Behavior Boundary

The protection bit is internal terminal-core cell state. It is not exposed in
`RenderCell`, plugin events, terminal replies, screenshots, or diagnostics.
Renderers therefore do not need a visual change for this slice.

Normal erase/edit controls remain intentionally unchanged:

- `CSI Ps J` ED
- `CSI Ps K` EL
- `CSI Ps X` ECH
- `CSI Ps @` ICH
- `CSI Ps P` DCH

Those controls can still replace protected cells. Only DECSED and DECSEL honor
the protected bit.

Blank cells created by erase operations are unprotected. DECSTR (`CSI ! p`) and
full reset clear the current protection attribute for future printed cells.

## Verification

- `cargo test -p witty-core protection --quiet`
- `cargo test -p witty-core selective_erase --quiet`
- `cargo test -p witty-core erase_in_line --quiet`

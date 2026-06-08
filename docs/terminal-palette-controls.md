# Terminal Palette Controls

m150 added the first OSC palette/theme control pass in `witty-core`. m151 wired
palette/default color queries into the existing terminal reply path. m152 added
internal color references so palette/default color changes repaint
already-written indexed/default-color cells.

## Supported Sequences

This line supports palette/default color changes and queries:

```text
OSC 4 ; index ; color ST
OSC 4 ; index ; color ; index ; color ST
OSC 4 ; index ; ? ST
OSC 4 ; index ; ? ; index ; ? ST
OSC 10 ; color ST
OSC 10 ; ? ST
OSC 11 ; color ST
OSC 11 ; ? ST
OSC 12 ; color ST
OSC 12 ; ? ST
OSC 104 ST
OSC 104 ; index ; index ST
OSC 110 ST
OSC 111 ST
OSC 112 ST
```

`OSC 4` updates indexed palette entries for future SGR indexed colors.
`OSC 10` updates the default foreground color for future output and SGR `39`.
`OSC 11` updates the default background color for future output and SGR `49`.
`OSC 12` updates the cursor overlay color and reports it through the same host
reply path. `OSC 104`, `110`, `111`, and `112` reset palette/default/cursor
color overrides.
Queries return `TerminalReply` actions using `rgb:rrrr/gggg/bbbb` and ST
termination.

Accepted color formats:

- `#rgb`, `#rrggbb`, `#rrrgggbbb`, and `#rrrrggggbbbb`
- `rgb:r/g/b`, where each component has one to four hex digits

## Color Reference Model

`BasicTerminal` stores internal color references for:

- default foreground
- default background
- indexed palette colors
- direct truecolor RGB values

Snapshots still expose concrete `CellStyle { foreground: Rgba, background:
Rgba, ... }`, so renderer, web, and plugin-facing contracts do not need to know
about the internal reference model.

Changing or resetting palette/default colors marks full damage so retained row
caches repaint with the newly resolved colors. Direct truecolor cells remain
unchanged by palette updates.

## Verification

Focused tests cover:

- `OSC 4` updates for normal, bright, and `38;5` indexed SGR colors.
- `OSC 4`, `OSC 10`, and `OSC 11` queries return host-internal reply actions.
- `OSC 12` set/query/reset updates `RenderSnapshot::cursor_color` without
  rendering text.
- `OSC 104` palette slot reset.
- `OSC 10`/`OSC 11` default foreground/background updates.
- palette/default updates repaint already-written indexed/default-color cells.
- direct truecolor cells ignore palette repaint.
- full terminal reset restoring built-in palette/default colors.

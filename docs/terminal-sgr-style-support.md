# Terminal SGR Style Support

Updated: 2026-06-01

## Scope

`BasicTerminal` now handles common CSI `m` SGR sequences and stores the effective
style on printed cells.

Supported attributes:

- reset: `0` and empty `m`
- bold: `1` / `22`
- faint/dim: `2` / `22`
- italic: `3` / `23`
- underline: `4`, `4:1` / `4:0`, `24`
- underline styles: `4:2` double, `4:3` curly, `4:4` dotted,
  `4:5` dashed, and `21` double underline
- underline color: `58;2;r;g;b`, `58;5;n`, colon variants, and reset `59`
- blink: `5`, `6` / `25`
- reverse: `7` / `27`
- conceal: `8` / `28`
- strike: `9` / `29`
- framed/encircled: `51`, `52` / `54`
- overline: `53` / `55`
- superscript/subscript baseline: `73`, `74` / `75`
- foreground/background reset: `39` / `49`
- standard and bright ANSI colors: `30-37`, `40-47`, `90-97`, `100-107`
- truecolor: `38;2;r;g;b`, `48;2;r;g;b`
- 256-color palette: `38;5;n`, `48;5;n`
- colon truecolor and palette variants: `38:2:r:g:b`, `38:2::r:g:b`,
  `48:2:r:g:b`, `48:2::r:g:b`, `38:5:n`, and `48:5:n`

## Rendering Boundary

The terminal core stores `CellStyle` per cell. Reverse video is resolved when a
cell is written by swapping foreground and background in the stored style while
preserving the `reverse` flag for diagnostics/run splitting. DEC screen
reverse-video mode (`?5`) is resolved later at snapshot time by swapping each
cell's final foreground/background colors.

The renderer groups glyph and background runs by style and now plans a
`text_decorations` rectangle layer for basic underline, double underline,
dotted underline, dashed underline, curly underline, strikethrough, framed
text, encircled text, and overline spans.
Glyph runs carry their `CellFlags` through `GlyphBatchItem`, and the renderer
text-buffer cache key includes those flags. This preserves style intent at the
glyphon boundary while keeping the current monospace family-selection policy.
Bold and italic flags are mapped to glyphon `Weight::BOLD` and `Style::Italic`
when text buffers are built.
Underline rectangles use `CellStyle::underline_color` when set and fall back to
the effective cell foreground. Dotted and dashed underline are expanded into
cellwise rectangle segments. Curly underline uses a rectangle-batch stepped wave
approximation. Faint/dim cells render glyphs and implicit text decorations by
blending foreground halfway toward background; explicit underline colors remain
explicit. Concealed cells keep their text in the terminal snapshot and selection
paths, but the renderer glyph and text decoration planners skip them.

Blink has an explicit renderer phase gate: `FramePlanner` keeps blinking text
visible by default, and callers can set the phase hidden to suppress blinking
glyphs and decorations. `RetainedFramePlanner` invalidates cached rows when that
phase changes. The native app drives that phase with a 500ms timer when the
visible snapshot contains non-concealed blinking cells; browser timer integration
remains separate app work.
Framed text is rendered as four rectangle borders around the contiguous span.
Encircled text uses a conservative rounded-box rectangle approximation.
Superscript and subscript now shift glyph run origins vertically in the CPU
planner and use a smaller glyphon metrics scale without changing terminal cell
width.

## Follow-Ups

- Wire the browser app loop to the renderer blink phase if product policy needs
  blinking text animation there.
- OSC palette/default color controls now provide overrides, query replies, and
  repaint of already-written indexed/default-color cells through the internal
  color-reference model. Snapshot output remains concrete `Rgba`.

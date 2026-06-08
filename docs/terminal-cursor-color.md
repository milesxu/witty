# Terminal Cursor Color

`m458-terminal-cursor-color-osc12` adds a small cursor-color control surface:

```text
OSC 12 ; color ST
OSC 12 ; ? ST
OSC 112 ST
```

`OSC 12` sets the cursor overlay color using the same color parser as palette
controls: `#rgb`, `#rrggbb`, `#rrrgggbbb`, `#rrrrggggbbbb`, and
`rgb:r/g/b`. `OSC 12 ; ?` replies through `TerminalHostAction::TerminalReply`
as `OSC 12 ; rgb:rrrr/gggg/bbbb ST`. `OSC 112` resets the override to the
renderer default cursor color.

The color is carried in `RenderSnapshot::cursor_color` and consumed by the
wgpu frame planner. When no override is set, the snapshot field is `None` and
the renderer keeps its existing neutral gray cursor color.

## Boundaries

This slice does not add a theme system or browser cursor blink timing. Cursor
color is terminal-state data only, and changes mark the current cursor row dirty
without rebuilding unrelated terminal rows.

## Verification

- `cargo test -p witty-core cursor_color --quiet`
- `cargo test -p witty-render-wgpu cursor_color --quiet`

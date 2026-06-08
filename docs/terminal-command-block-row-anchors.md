# Terminal Command Block Row Anchors

Updated: 2026-05-31

`m195-terminal-command-block-row-anchors` adds durable row identity for the
OSC 133 command-block path.

## Problem

The first command-block overlay used the marker cursor rows directly. That is
correct only while the terminal remains at the same live viewport. Once output
scrolls into history or the user scrolls back, a completed block's original
screen-row coordinates no longer describe where that block is visible.

## Core Model

`witty-core` now assigns monotonically increasing row anchors to terminal rows:

- main-screen rows keep their anchors as they scroll into main scrollback.
- blank rows introduced by full-screen scroll, line insert/delete, reverse
  index, screen clear, reset, alternate-screen clear, and resize receive fresh
  anchors.
- alternate-screen rows use a separate anchor namespace from main-screen rows.
- `BasicTerminal::visible_row_anchors()` returns the currently visible row to
  anchor mapping for native and browser UI layers.

OSC 133 host actions now carry both the legacy `CellPoint` and an optional
`TerminalPointAnchor`. Existing JSON keeps compatibility because the anchor is
optional and omitted when unavailable.

## UI Mapping

`witty-ui` stores command-block marker anchors alongside the original points.
The screen-row span remains available as a fallback, but selected-block
rendering now prefers anchor spans:

1. collect the selected block's anchored start/end rows.
2. intersect those anchors with the terminal's current visible row anchors.
3. render the highlight/gutter only on visible rows that still correspond to
   the selected command block.

Native and browser render paths both call the anchor-aware overlay helper before
IME/search/command-palette overlays.

## Boundary

This is still not a full rendered command-block UI. It gives command-block
navigation and selection stable viewport mapping across scrollback, which is
the required data boundary for later gutter widgets, block folding, duration
metadata, persisted command history, and block-scoped copy/share actions.
`m196-command-block-gutter-overlay` is the first consumer of this boundary for
always-visible completed-block gutter bars.

## Verification

Covered by:

- `cargo test -p witty-core osc133 --quiet`
- `cargo test -p witty-core visible_row_anchors --quiet`
- `cargo test -p witty-core --quiet`
- `cargo test -p witty-ui shell_integration --quiet`
- `cargo test -p witty-ui --quiet`
- `cargo test -p witty-app native_command_block --quiet`
- `cargo test -p witty-app --quiet`
- `cargo test -p witty-web command_block --quiet`
- `cargo test -p witty-web --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `cargo clippy -p witty-core -p witty-ui -p witty-app -p witty-web --all-targets -- -D warnings`
- `cargo run -p witty-app -- --native-command-block-smoke`

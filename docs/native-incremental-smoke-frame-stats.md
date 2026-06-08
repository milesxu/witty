# Native Incremental Smoke Frame Stats

Updated: 2026-05-30

`m176-native-incremental-smoke-frame-stats` makes the native incremental smoke
emit machine-readable planner diagnostics without opening a second UI.

## Implemented

- `witty --incremental-smoke` keeps its compact human-readable row reuse
  summary.
- The smoke now also prints `Incremental smoke stats=<json>`.
- The JSON payload contains:
  - `smoke`
  - `frameCount`
  - `frames`
- Each frame object includes the retained planner `FrameStats` fields, using
  the same camel-case names as browser frame stats where fields overlap:
  - visible dimensions
  - background and glyph run counts
  - glyph character, max-run, and prepare-batch counts
  - selection, search, hyperlink, and IME overlay rect counts
  - cursor/search-active visibility
  - rect vertex count and capacity
  - damage mode, damage-region count, reused rows, and rebuilt rows

## Boundary

This is a smoke diagnostic contract, not a renderer benchmark. The JSON is
bounded to numeric and boolean frame statistics and intentionally avoids
terminal contents, clipboard contents, search query text, or host/session data.

## Verification

Covered by:

- `cargo test -p witty-app incremental_smoke --quiet`
- `cargo run -p witty-app -- --incremental-smoke`

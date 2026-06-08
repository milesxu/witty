# Renderer Glyph Prepare Batch Stats

Updated: 2026-05-30

`m171-renderer-glyph-prepare-batch-stats` adds a stable diagnostic for the
number of bounded `glyphon` prepare batches a frame will use.

## Implemented

- `FrameStats::glyph_prepare_batches` estimates the number of renderer prepare
  chunks implied by the planned glyph runs.
- The count uses the same `MAX_GLYPHON_RENDERER_CHARS` budget as the renderer's
  text-buffer chunking path.
- Native diagnostics now render
  `runs bg=<N> glyph=<N> chars=<N> batches=<N> max=<N>`.
- Browser frame stats JSON includes `glyphPrepareBatches`, and the browser
  smoke asserts that the count is present and internally consistent.
- `m172-renderer-rect-vertex-capacity-stats` continues the same diagnostics
  line by exposing rect vertex capacity buckets.

## Boundary

This pass does not change text shaping, renderer cache lifetime, or glyphon
submission order. It only makes the batch count observable before the next
renderer cache optimization.

## Verification

Covered by:

- `cargo test -p witty-render-wgpu --quiet`
- `cargo test -p witty-app diagnostics_overlay_adds_frame_stats_text --quiet`
- `cargo test -p witty-web browser_frame_stats_json_includes_glyph_run_budget --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `node --check crates/witty-web/static/app.js`
- `node --check scripts/run-witty-web-smoke.mjs`
- `scripts/run-witty-web-smoke.sh`

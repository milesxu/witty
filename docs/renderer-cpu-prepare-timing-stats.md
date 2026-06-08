# Renderer CPU Prepare Timing Stats

Updated: 2026-05-30

`m175-renderer-cpu-prepare-timing-stats` adds bounded renderer CPU timing
diagnostics for the frame preparation path.

## Implemented

- Added `RendererTimingStats` to `witty-render-wgpu`.
- `WgpuRectRenderer::timing_stats()` reports the last frame's CPU preparation
  timing split into:
  - `text_buffer_sync_us`
  - `glyph_prepare_us`
  - `rect_vertex_sync_us`
  - `cpu_prepare_us`
- Native builds use `std::time::Instant`; browser builds use
  `window.performance.now()`.
- Browser sessions save timing stats after each successful frame render.
- Browser frame stats JSON includes:
  - `rendererCpuPrepareUs`
  - `rendererTextBufferSyncUs`
  - `rendererGlyphPrepareUs`
  - `rendererRectVertexSyncUs`
- The Playwright smoke asserts that timing fields are present and internally
  consistent.

## Boundary

These are smoke diagnostics, not a benchmark contract. They are useful for
local before/after comparisons on the same machine and browser, but should not
be treated as stable CI performance thresholds.

## Verification

Covered by:

- `cargo test -p witty-render-wgpu renderer_timing_stats_sum_cpu_prepare_sections --quiet`
- `cargo test -p witty-web browser_frame_stats_json_includes_glyph_run_budget --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `node --check scripts/run-witty-web-smoke.mjs`
- `scripts/run-witty-web-smoke.sh`

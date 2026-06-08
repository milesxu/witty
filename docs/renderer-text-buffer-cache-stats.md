# Renderer Text Buffer Cache Stats

Updated: 2026-05-30

`m173-renderer-text-buffer-cache-stats` exposes renderer cache reuse metrics so
text-run buffering can be measured from browser smoke output.

## Implemented

- Added `RendererCacheStats` to `witty-render-wgpu`.
- `WgpuRectRenderer::cache_stats()` reports the last text buffer sync counts:
  reused, rebuilt, retired, current text buffer count, renderer pool count, and
  the renderer's actual rect vertex capacity.
- Browser sessions save the renderer cache stats after each successful frame
  render.
- Browser frame stats JSON now includes:
  - `rendererTextBuffersReused`
  - `rendererTextBuffersRebuilt`
  - `rendererTextBuffersRetired`
  - `rendererTextBufferCount`
  - `rendererTextRendererCount`
  - `rendererRectVertexCapacity`
- The Playwright smoke asserts that text buffer counts match glyph runs, sync
  reuse/rebuild totals match the current frame, the renderer pool covers the
  prepare batch count, and renderer rect capacity covers frame rect vertices.

## Boundary

This pass does not change glyph shaping, cache keys, or renderer submission
order. It only exposes cache reuse data that already exists inside the renderer.

## Verification

Covered by:

- `cargo test -p witty-render-wgpu --quiet`
- `cargo test -p witty-web browser_frame_stats_json_includes_glyph_run_budget --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `node --check crates/witty-web/static/app.js`
- `node --check scripts/run-witty-web-smoke.mjs`
- `scripts/run-witty-web-smoke.sh`

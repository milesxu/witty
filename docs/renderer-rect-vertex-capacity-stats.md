# Renderer Rect Vertex Capacity Stats

Updated: 2026-05-30

`m172-renderer-rect-vertex-capacity-stats` exposes the reusable rect vertex
buffer capacity bucket implied by each planned frame.

## Implemented

- `FrameStats::rect_vertex_capacity` reports the power-of-two vertex buffer
  capacity bucket needed for the frame's rect vertices.
- Empty rect frames report capacity `0`; non-empty frames use the same capacity
  helper as `WgpuRectRenderer`.
- Native diagnostics now render `rectv=<N> cap=<N> sel=<N>`.
- Browser frame stats JSON includes `rectVertexCapacity`, and the browser smoke
  asserts it is present and large enough for `rectVertices`.

## Boundary

The reusable vertex buffer already lives in `WgpuRectRenderer`; this pass only
surfaces the capacity bucket for smoke diagnostics and future performance
comparisons.

## Verification

Covered by:

- `cargo test -p witty-render-wgpu --quiet`
- `cargo test -p witty-app diagnostics_overlay_adds_frame_stats_text --quiet`
- `cargo test -p witty-web browser_frame_stats_json_includes_glyph_run_budget --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `node --check crates/witty-web/static/app.js`
- `node --check scripts/run-witty-web-smoke.mjs`
- `scripts/run-witty-web-smoke.sh`

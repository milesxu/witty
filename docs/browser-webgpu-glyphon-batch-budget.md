# Browser WebGPU Glyphon Batch Budget

Updated: 2026-05-30

`m170-browser-webgpu-glyphon-batch-budget` formalizes the renderer guardrail
that keeps browser `glyphon` text preparation below WebGPU staging-buffer limits
seen during scrollback smoke testing.

## Context

The launcher smoke exposed a Chrome/WebGPU failure when a large accumulated PTY
line caused `glyphon` to grow an internal mapped staging buffer beyond the
browser implementation limit. The fix keeps the current `glyphon` renderer but
caps the text submitted per text area and per renderer prepare batch.

## Implemented

- `FramePlanner` splits long same-style text runs at
  `MAX_GLYPHON_TEXT_AREA_CHARS`.
- `WgpuRectRenderer` chunks cached text buffer items at
  `MAX_GLYPHON_RENDERER_CHARS` before calling `TextRenderer::prepare`.
- The renderer owns a small pool of `TextRenderer` instances so each bounded
  chunk has an isolated prepare budget.
- `FrameStats::max_glyph_run_chars` reports the largest planned text run in the
  frame.
- `FrameStats::glyph_prepare_batches` reports the number of bounded prepare
  chunks implied by the planned glyph runs.
- Native diagnostics now render
  `runs bg=<N> glyph=<N> chars=<N> batches=<N> max=<N>` so the batch budget is
  visible in the debug overlay.
- Browser sessions expose `frame_stats_json()`, and the Playwright smoke asserts
  that `maxGlyphRunChars` stays within the browser budget and
  `glyphPrepareBatches` is internally consistent.

## Boundaries

This pass does not replace `glyphon`, add a custom atlas, or change terminal
cell semantics. It is a browser stability guardrail while the renderer remains
run-oriented rather than a full row-buffer text layout.

## Verification

Covered by:

- `cargo test -p witty-render-wgpu --quiet`
- `cargo test -p witty-app diagnostics_overlay_adds_frame_stats_text --quiet`
- `cargo test -p witty-web browser_frame_stats_json_includes_glyph_run_budget --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `node --check crates/witty-web/static/app.js`
- `node --check scripts/run-witty-web-smoke.mjs`
- `scripts/run-witty-web-smoke.sh`

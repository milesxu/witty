# Renderer Line Batching Plan

Date: 2026-05-29
Status: planning

This note defines the next renderer work after the MVP wgpu window, glyphon text pass, selection overlay, and command palette overlay. The goal is to move from a correctness-first renderer to a design that can handle real terminal workloads without painting every cell as an independent object forever.

## Current Renderer Shape

The current pipeline is intentionally simple:

1. `witty-core` produces a visible `RenderSnapshot`.
2. `FramePlanner` converts every visible cell into a background rect and every non-blank cell into a `GlyphBatchItem`.
3. `WgpuRectRenderer::render` creates glyphon `Buffer` values from `FramePlan::glyphs`.
4. Rect vertices are rebuilt into a new vertex buffer every frame.
5. The render pass draws rects first, then glyphon text.

This is good for early correctness because every artifact is explicit and easy to test. It is not the final performance model.

## Bottlenecks To Remove

| Area | Current behavior | Problem |
| --- | --- | --- |
| glyph planning | one `GlyphBatchItem` per non-blank cell | too many glyphon buffers and text areas |
| background planning | one rect per cell | high vertex count even when most cells share the same background |
| allocation | new vectors, text buffers, and vertex buffer per frame | unnecessary CPU and allocator pressure |
| shaping | one tiny glyphon buffer per cell | defeats line/run shaping and cache locality |
| damage | full visible snapshot is replanned on each redraw | cursor blink, selection, and output all pay full-frame costs |
| overlay composition | overlay glyphs are merged into the same ad hoc frame | acceptable for MVP, but needs explicit layers soon |

## Target Frame Model

Move `FramePlan` from cell-oriented batches to row/run-oriented batches:

```rust
pub struct FramePlan {
    pub background_runs: Vec<RectBatchItem>,
    pub text_runs: Vec<TextRunItem>,
    pub decorations: Vec<RectBatchItem>,
    pub cursor: Option<RectBatchItem>,
    pub overlays: Vec<OverlayLayer>,
    pub stats: FrameStats,
}

pub struct TextRunItem {
    pub row: u16,
    pub start_col: u16,
    pub origin: PixelPoint,
    pub text: String,
    pub color: Rgba,
}
```

Rules:

- Adjacent cells with the same background become one background run.
- Adjacent cells with the same foreground and compatible style become one text run.
- Selection, search matches, cursor, and command palette should be explicit layers, not hidden mutations of terminal rows.
- `FrameStats` should report run counts, visible rows, visible cols, damaged rows, and total glyph count.

## Renderer Cache Model

`WgpuRectRenderer` should keep reusable GPU and text resources:

| Cache | Key | Value |
| --- | --- | --- |
| row text buffer | row id + text/style generation | glyphon `Buffer` or equivalent line layout |
| rect vertex buffer | capacity bucket | reusable GPU buffer with `Queue::write_buffer` |
| atlas | font + glyph + style | glyphon atlas or later custom atlas entry |
| frame stats | last frame id | CPU timings and batch counts for diagnostics |

Do not introduce a custom atlas until glyphon has been pushed to its natural line-buffer model. The next step is to use glyphon better, not replace it prematurely.

## Damage Model

Add damage as a data contract before optimizing aggressively:

```rust
pub enum DamageRegion {
    Full,
    Rows(Vec<u16>),
    Rects(Vec<CellRange>),
}
```

Initial damage sources:

- PTY output marks touched rows.
- Resize, font change, and theme change mark full damage.
- Cursor blink marks old and new cursor cells.
- Scrollback movement marks visible rows.
- Selection and command palette are overlay damage and should not mutate terminal content.

The first implementation may still rebuild the full visible frame, but the damage fields should exist so later work can be incremental without changing public boundaries again.

## Implementation Milestones

### R1: Planner Run Compression

Write scope:

- `crates/witty-render-wgpu/src/lib.rs`
- focused tests only

Deliverables:

- Convert same-color cell backgrounds into horizontal rect runs.
- Convert same-style adjacent glyph cells into text runs.
- Keep compatibility helpers if the current renderer still consumes old fields.
- Add tests for background run merge boundaries and text run split boundaries.

Acceptance:

- `cargo fmt --all -- --check`
- `cargo test -p witty-render-wgpu`
- full workspace `cargo test --workspace`

### R2: Glyphon Line Buffers

Write scope:

- `crates/witty-render-wgpu/src/lib.rs`

Deliverables:

- Replace one glyphon `Buffer` per cell with one buffer per text run or row.
- Preserve correct origin and clipping.
- Add frame stats for number of text buffers prepared.

Acceptance:

- Non-GUI frame planner tests.
- GUI startup smoke still renders nonblank terminal text.
- Screenshot pass after R2 should show no missing prompt text.
- Status: implemented for text runs. Planned glyph runs are backed by cached
  `glyphon::Buffer` values, and renderer cache reuse/rebuild counts are exposed
  through browser frame stats.

### R3: Reusable Rect Buffer

Write scope:

- `crates/witty-render-wgpu/src/lib.rs`

Deliverables:

- Keep a reusable rect vertex buffer capacity in `WgpuRectRenderer`.
- Use `Queue::write_buffer` when the vertex count fits current capacity.
- Reallocate only when capacity grows.

Acceptance:

- Unit-testable capacity helper.
- Manual smoke shows no validation errors.
- Status: implemented. `WgpuRectRenderer` keeps a reusable vertex buffer,
  updates it with `Queue::write_buffer`, and `FrameStats` reports the capacity
  bucket for diagnostics.

### R4: Damage Contract

Write scope:

- `crates/witty-core`
- `crates/witty-render-wgpu`
- `crates/witty-ui` only if needed

Deliverables:

- Add minimal damage metadata to terminal snapshots or frame requests.
- Cursor/selection/palette stay separate from terminal row damage.
- Document how future scroll optimization will use the contract.

Acceptance:

- Existing terminal parser tests continue to pass.
- Added tests prove cursor-only and selection-only changes do not require terminal row mutation.
- Status: implemented for retained frame planning. Row damage, search overlay,
  hyperlink hover, selection-only, and cursor-only changes have focused
  coverage.

### R5: Renderer Diagnostics

Write scope:

- `witty-render-wgpu`
- optional `witty-app` logging flag

Deliverables:

- `FrameStats` with visible rows, cols, background run count, text run count, glyph count, rect vertex count, and CPU preparation time.
- Optional debug print flag for smoke runs.
- Status: partially implemented. `FrameStats` now includes glyph run count,
  glyph char count, max glyph-run chars, and glyph prepare batch count, with
  rect vertex capacity, renderer text buffer cache stats, renderer CPU prepare
  timing, and native/browser diagnostics coverage.

Acceptance:

- Smoke run can print stats without opening a second UI.
- Stats tests cover deterministic planner output.
- Status: implemented for browser and native incremental smoke. Browser frame
  stats print both planner stats and renderer cache/timing stats without
  opening a second UI. Native `--incremental-smoke` prints retained planner
  `FrameStats` as bounded JSON. See
  `native-incremental-smoke-frame-stats.md`.

## Browser Constraint

Keep the plan compatible with WebGPU:

- Avoid native-only GPU features in the core renderer path.
- Continue using downlevel-friendly limits unless a feature gate proves a stronger path.
- Do not assume persistent mapped buffers are available in browser builds.
- Keep PTY and platform event loop concerns outside `witty-render-wgpu`.

## Open Decisions

1. Whether text runs should be row-owned, style-owned, or a hybrid.
2. Whether ligatures should be disabled by default until cell hit-testing is richer.
3. Whether native box drawing should be a primitive layer before or after run compression.
4. How much of glyphon buffer reuse is stable across font/theme changes.

## Recommended Next Worker

Dispatch `m21-render-plan-run-compression` before touching GPU buffer lifetime. It is smaller, highly testable, and produces immediate reductions in frame plan size without requiring live window automation.

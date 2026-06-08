# Renderer Planner Scrollback Perf Example

Updated: 2026-05-30

`m177-renderer-planner-scrollback-perf-example` adds a repeatable retained
planner measurement entry point for large scrollback workloads. It does not
start a window, create a GPU device, or run `glyphon`; it measures the
CPU-side `RetainedFramePlanner` path over generated terminal history snapshots.

## Implemented

- Added:

```sh
cargo run -p witty-render-wgpu --example scrollback_planner_perf --release
```

- The example generates a long main-screen scrollback workload, then measures:
  - full retained planning at the tail viewport
  - repeated no-damage planning at the tail viewport
  - full retained planning after jumping into history
  - repeated no-damage planning while pinned in history
  - repeated one-row local scrollback viewport steps
- Output is a single JSON object with microsecond timing fields and bounded
  `FrameStats` snapshots for each measured phase.

## Controls

| Env var | Default | Meaning |
| --- | ---: | --- |
| `WITTY_RENDER_PLANNER_PERF_ROWS` | `30` | terminal rows |
| `WITTY_RENDER_PLANNER_PERF_COLS` | `120` | terminal columns |
| `WITTY_RENDER_PLANNER_PERF_LINES` | `20000` | generated input lines |
| `WITTY_RENDER_PLANNER_PERF_MAX_LINES` | `20000` | retained scrollback line limit |
| `WITTY_RENDER_PLANNER_PERF_SNAPSHOTS` | `100` | repeated no-damage plan count |
| `WITTY_RENDER_PLANNER_PERF_SCROLL_STEPS` | `100` | one-row viewport scroll plan count |

## Boundary

The timings isolate frame planning from snapshot construction and GPU work where
possible. The one-row scroll phase pre-collects snapshots before timing the
planner loop. This keeps the example useful for comparing retained planner
changes, not as a full end-to-end renderer benchmark.

## Verification

Covered by:

- `cargo check -p witty-render-wgpu --example scrollback_planner_perf`
- `WITTY_RENDER_PLANNER_PERF_LINES=200 WITTY_RENDER_PLANNER_PERF_MAX_LINES=80 WITTY_RENDER_PLANNER_PERF_SNAPSHOTS=5 WITTY_RENDER_PLANNER_PERF_SCROLL_STEPS=5 cargo run -p witty-render-wgpu --example scrollback_planner_perf --quiet`

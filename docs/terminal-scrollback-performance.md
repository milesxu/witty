# Terminal Scrollback Performance

Updated: 2026-05-30

`m165-terminal-scrollback-storage-windowing` is the first bounded pass on large
scrollback performance. It keeps the existing user-visible scrollback semantics
but removes two obvious costs in the hot path.

`m166-terminal-scrollback-perf-example` adds a repeatable measurement entry
point so later storage, search, and renderer changes can be compared against the
same generated history workload.

`m167-terminal-scrollback-limit-config` makes the retained scrollback line
budget configurable from the core API and the native/browser product entry
points.

`m168-browser-launcher-scrollback-config-smoke` closes the product-path coverage
gap by starting the launcher smoke with an explicit scrollback limit and
asserting that the browser wasm session applies it.

`m169-browser-local-scrollback-wheel` adds browser parity for local scrollback
wheel gestures while preserving application mouse-report wheel bytes.

`m170-browser-webgpu-glyphon-batch-budget` formalizes the browser WebGPU text
batch budget that keeps long scrollback output from triggering oversized
`glyphon` staging buffers.

`m177-renderer-planner-scrollback-perf-example` adds a retained planner timing
entry point for large scrollback workloads without starting a window or GPU
device.

## Implemented

- Main-screen scrollback storage now uses `VecDeque<Vec<BasicCell>>`.
- Capped scrollback trimming drops rows from the front with `pop_front()` instead
  of repeatedly shifting a `Vec` with `drain(0..overflow)`.
- Main-screen `visible_rows()` now computes the visible logical row window and
  clones only those rows.
- Snapshot creation no longer allocates a combined reference list for the full
  scrollback plus visible screen on every frame.
- A focused core test verifies that capped scrollback keeps the newest retained
  history and that viewport scrolling still reaches the retained rows.
- `cargo run -p witty-core --example scrollback_perf --release` generates a long
  scrollback workload and prints feed, tail snapshot, history snapshot, and
  search timings as JSON.
- `BasicTerminal::with_scrollback_limit`,
  `BasicTerminal::set_max_scrollback_lines`, `max_scrollback_lines`, and
  `scrollback_line_count` expose the history budget without touching private
  state in callers.
- `witty --window --scrollback-lines <N>` applies the limit to the native
  `winit`/`wgpu` terminal.
- `witty --web --scrollback-lines <N>` serializes the same value through the
  one-use launcher session JSON and applies it in the browser wasm session.
- The browser smoke harness starts launcher mode with
  `--scrollback-lines ${WITTY_WEB_SMOKE_SCROLLBACK_LINES:-64}` and checks
  both the JavaScript config helper and wasm session limit.
- Browser mode now scrolls the local main-screen viewport on wheel when
  application mouse reporting is inactive, and on Shift-wheel when mouse
  reporting is active under the default `shift-select` override policy.
- Browser plain wheel still sends xterm wheel reports while application mouse
  reporting is active.
- Browser WebGPU text rendering now splits long planned text runs and chunks
  `glyphon` prepare batches under explicit character budgets.
- `FrameStats::max_glyph_run_chars` exposes the largest planned glyph run, and
  the native diagnostics overlay renders it as the `max=<N>` run field.
- `FrameStats::glyph_prepare_batches` exposes the number of bounded renderer
  prepare chunks implied by the planned glyph runs.
- Browser sessions expose frame stats JSON so the Playwright smoke can assert
  the glyph-run budget after scrollback wheel rendering.
- `cargo run -p witty-render-wgpu --example scrollback_planner_perf --release`
  measures full, no-damage, and one-row scrollback viewport planning phases and
  reports timings plus bounded `FrameStats` as JSON.

## Boundaries

The storage/windowing passes do not change:

- the public `RenderSnapshot` shape,
- search indexing semantics,
- selection coordinates,
- scrollback size policy,
- persistent/session scrollback storage.

Search still intentionally scans the full retained history when the user runs a
search. That path should be measured separately from ordinary frame snapshots.

The scrollback limit is local terminal state. It is not sent to the PTY, does
not alter the WebSocket gateway protocol, and does not expose scrollback
contents to plugins.

## Measurement

Default run:

```sh
cargo run -p witty-core --example scrollback_perf --release
```

Optional workload controls:

| Env var | Default | Meaning |
| --- | ---: | --- |
| `WITTY_SCROLLBACK_PERF_ROWS` | `30` | terminal rows |
| `WITTY_SCROLLBACK_PERF_COLS` | `120` | terminal columns |
| `WITTY_SCROLLBACK_PERF_LINES` | `20000` | generated input lines |
| `WITTY_SCROLLBACK_PERF_MAX_LINES` | `20000` | retained scrollback line limit |
| `WITTY_SCROLLBACK_PERF_SNAPSHOTS` | `100` | repeated snapshot count for tail/history views |

The output is a single JSON object. The timing fields use microseconds:

- `feed_us`
- `tail_snapshot_us`
- `history_snapshot_us`
- `search_us`

This is a local comparison tool, not a strict CI benchmark. Use it to compare
before/after changes on the same machine and build profile.

Renderer retained planner run:

```sh
cargo run -p witty-render-wgpu --example scrollback_planner_perf --release
```

See `renderer-planner-scrollback-perf-example.md` for renderer planner workload
controls and output fields.

## Verification

Covered by:

- `cargo fmt --all -- --check`
- `cargo test -p witty-core scrollback --quiet`
- `cargo test -p witty-core --quiet`
- `cargo test -p witty-launcher scrollback --quiet`
- `cargo test -p witty-launcher browser_session_config --quiet`
- `cargo test -p witty-app app_options --quiet`
- `cargo test -p witty-web browser_scroll_lines_for_wheel_delta --quiet`
- `cargo test -p witty-web browser_frame_stats_json_includes_glyph_run_budget --quiet`
- `cargo check -p witty-render-wgpu --example scrollback_planner_perf`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `node --check crates/witty-web/static/app.js`
- `node --check scripts/run-witty-web-smoke.mjs`
- `WITTY_SCROLLBACK_PERF_LINES=200 WITTY_SCROLLBACK_PERF_MAX_LINES=50 WITTY_SCROLLBACK_PERF_SNAPSHOTS=5 cargo run -p witty-core --example scrollback_perf --quiet`
- `WITTY_RENDER_PLANNER_PERF_LINES=200 WITTY_RENDER_PLANNER_PERF_MAX_LINES=80 WITTY_RENDER_PLANNER_PERF_SNAPSHOTS=5 WITTY_RENDER_PLANNER_PERF_SCROLL_STEPS=5 cargo run -p witty-render-wgpu --example scrollback_planner_perf --quiet`
- `cargo test --workspace --quiet`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `scripts/run-witty-web-smoke.sh`
- `WITTY_WEB_SMOKE_GATEWAY=launcher scripts/run-witty-web-smoke.sh`

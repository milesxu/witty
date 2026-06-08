use std::{env, hint::black_box, time::Instant};

use witty_core::{BasicTerminal, GridSize, RenderSnapshot};
use witty_render_wgpu::{CellMetrics, FrameStats, RetainedFramePlanner};

const DEFAULT_ROWS: u16 = 30;
const DEFAULT_COLS: u16 = 120;
const DEFAULT_LINES: usize = 20_000;
const DEFAULT_SNAPSHOTS: usize = 100;
const DEFAULT_SCROLL_STEPS: usize = 100;

fn main() {
    let rows = env_u16("WITTY_RENDER_PLANNER_PERF_ROWS", DEFAULT_ROWS);
    let cols = env_u16("WITTY_RENDER_PLANNER_PERF_COLS", DEFAULT_COLS);
    let lines = env_usize("WITTY_RENDER_PLANNER_PERF_LINES", DEFAULT_LINES);
    let snapshots = env_usize("WITTY_RENDER_PLANNER_PERF_SNAPSHOTS", DEFAULT_SNAPSHOTS);
    let scroll_steps = env_usize(
        "WITTY_RENDER_PLANNER_PERF_SCROLL_STEPS",
        DEFAULT_SCROLL_STEPS,
    );
    let max_scrollback_lines = env_usize("WITTY_RENDER_PLANNER_PERF_MAX_LINES", DEFAULT_LINES);

    let mut terminal =
        BasicTerminal::with_scrollback_limit(GridSize::new(rows, cols), max_scrollback_lines);
    for line in 0..lines {
        terminal.feed(format!("line-{line:08} witty planner perf marker {line:08}\r\n").as_bytes());
    }

    let mut planner = RetainedFramePlanner::new(CellMetrics::default());

    let tail_full_snapshot = terminal.take_snapshot();
    let tail_full_started = Instant::now();
    let tail_full = black_box(planner.plan(&tail_full_snapshot)).stats;
    let tail_full_plan_us = tail_full_started.elapsed().as_micros();

    let tail_reuse_snapshot = terminal.take_snapshot();
    let (tail_reuse_plan_us, tail_reuse) =
        plan_repeated(&mut planner, &tail_reuse_snapshot, snapshots);

    scroll_to_history_top(&mut terminal);
    let history_viewport_offset = terminal.viewport_offset();
    let history_full_snapshot = terminal.take_snapshot();
    let history_full_started = Instant::now();
    let history_full = black_box(planner.plan(&history_full_snapshot)).stats;
    let history_full_plan_us = history_full_started.elapsed().as_micros();

    let history_reuse_snapshot = terminal.take_snapshot();
    let (history_reuse_plan_us, history_reuse) =
        plan_repeated(&mut planner, &history_reuse_snapshot, snapshots);

    let scroll_step_snapshots = scroll_step_snapshots(&mut terminal, scroll_steps);
    let scroll_steps = scroll_step_snapshots.len();
    let scroll_step_started = Instant::now();
    let mut scroll_step = FrameStats::default();
    for snapshot in &scroll_step_snapshots {
        scroll_step = black_box(planner.plan(snapshot)).stats;
        black_box(scroll_step.rebuilt_rows);
    }
    let scroll_step_plan_us = scroll_step_started.elapsed().as_micros();

    println!(
        "{{\"rows\":{rows},\"cols\":{cols},\"input_lines\":{lines},\
         \"snapshots\":{snapshots},\"scroll_steps\":{scroll_steps},\
         \"max_scrollback_lines\":{},\"scrollback_lines\":{},\
         \"history_viewport_offset\":{history_viewport_offset},\
         \"tail_full_plan_us\":{tail_full_plan_us},\"tail_reuse_plan_us\":{tail_reuse_plan_us},\
         \"history_full_plan_us\":{history_full_plan_us},\"history_reuse_plan_us\":{history_reuse_plan_us},\
         \"scroll_step_plan_us\":{scroll_step_plan_us},\
         \"tail_full\":{},\"tail_reuse\":{},\"history_full\":{},\"history_reuse\":{},\"scroll_step\":{}}}",
        terminal.max_scrollback_lines(),
        terminal.scrollback_line_count(),
        stats_json(tail_full),
        stats_json(tail_reuse),
        stats_json(history_full),
        stats_json(history_reuse),
        stats_json(scroll_step)
    );
}

fn scroll_to_history_top(terminal: &mut BasicTerminal) {
    loop {
        let previous = terminal.viewport_offset();
        terminal.scroll_viewport_lines(i16::MAX);
        if terminal.viewport_offset() == previous {
            break;
        }
    }
}

fn scroll_step_snapshots(
    terminal: &mut BasicTerminal,
    requested_steps: usize,
) -> Vec<RenderSnapshot> {
    let mut snapshots = Vec::with_capacity(requested_steps);
    for _ in 0..requested_steps {
        let previous = terminal.viewport_offset();
        terminal.scroll_viewport_lines(-1);
        if terminal.viewport_offset() == previous {
            break;
        }
        snapshots.push(terminal.take_snapshot());
    }
    snapshots
}

fn plan_repeated(
    planner: &mut RetainedFramePlanner,
    snapshot: &RenderSnapshot,
    iterations: usize,
) -> (u128, FrameStats) {
    let started = Instant::now();
    let mut stats = FrameStats::default();
    for _ in 0..iterations {
        stats = black_box(planner.plan(snapshot)).stats;
        black_box(stats.glyph_runs);
    }
    (started.elapsed().as_micros(), stats)
}

fn stats_json(stats: FrameStats) -> String {
    format!(
        "{{\"visibleRows\":{},\"visibleCols\":{},\"backgroundRuns\":{},\
         \"glyphRuns\":{},\"glyphChars\":{},\"glyphPrepareBatches\":{},\
         \"maxGlyphRunChars\":{},\"selectionRects\":{},\"searchHighlightRects\":{},\
         \"hyperlinkHoverRects\":{},\"hyperlinkUnderlineRects\":{},\
         \"textDecorationRects\":{},\"imePreeditRects\":{},\
         \"searchActiveVisible\":{},\"cursorVisible\":{},\"rectVertices\":{},\
         \"rectVertexCapacity\":{},\"fullDamage\":{},\"damageRegions\":{},\
         \"reusedRows\":{},\"rebuiltRows\":{}}}",
        stats.visible_rows,
        stats.visible_cols,
        stats.background_runs,
        stats.glyph_runs,
        stats.glyph_chars,
        stats.glyph_prepare_batches,
        stats.max_glyph_run_chars,
        stats.selection_rects,
        stats.search_highlight_rects,
        stats.hyperlink_hover_rects,
        stats.hyperlink_underline_rects,
        stats.text_decoration_rects,
        stats.ime_preedit_rects,
        stats.search_active_visible,
        stats.cursor_visible,
        stats.rect_vertices,
        stats.rect_vertex_capacity,
        stats.full_damage,
        stats.damage_regions,
        stats.reused_rows,
        stats.rebuilt_rows
    )
}

fn env_u16(name: &str, default: u16) -> u16 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

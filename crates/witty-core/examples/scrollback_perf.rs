use std::{env, hint::black_box, time::Instant};

use witty_core::{BasicTerminal, GridSize, SearchOptions};

const DEFAULT_ROWS: u16 = 30;
const DEFAULT_COLS: u16 = 120;
const DEFAULT_LINES: usize = 20_000;
const DEFAULT_SNAPSHOTS: usize = 100;

fn main() {
    let rows = env_u16("WITTY_SCROLLBACK_PERF_ROWS", DEFAULT_ROWS);
    let cols = env_u16("WITTY_SCROLLBACK_PERF_COLS", DEFAULT_COLS);
    let lines = env_usize("WITTY_SCROLLBACK_PERF_LINES", DEFAULT_LINES);
    let snapshots = env_usize("WITTY_SCROLLBACK_PERF_SNAPSHOTS", DEFAULT_SNAPSHOTS);
    let max_scrollback_lines = env_usize("WITTY_SCROLLBACK_PERF_MAX_LINES", DEFAULT_LINES);

    let mut terminal =
        BasicTerminal::with_scrollback_limit(GridSize::new(rows, cols), max_scrollback_lines);

    let feed_started = Instant::now();
    for line in 0..lines {
        terminal.feed(format!("line-{line:08} witty scrollback perf marker\r\n").as_bytes());
    }
    let feed_us = feed_started.elapsed().as_micros();

    let tail_snapshot_started = Instant::now();
    let mut tail_snapshot_rows = 0;
    for _ in 0..snapshots {
        let snapshot = black_box(terminal.snapshot());
        tail_snapshot_rows = snapshot.rows.len();
        black_box(tail_snapshot_rows);
    }
    let tail_snapshot_us = tail_snapshot_started.elapsed().as_micros();

    terminal.scroll_viewport_lines(i16::MAX);
    let history_viewport_offset = terminal.viewport_offset();
    let history_snapshot_started = Instant::now();
    let mut history_snapshot_rows = 0;
    for _ in 0..snapshots {
        let snapshot = black_box(terminal.snapshot());
        history_snapshot_rows = snapshot.rows.len();
        black_box(history_snapshot_rows);
    }
    let history_snapshot_us = history_snapshot_started.elapsed().as_micros();

    let search_started = Instant::now();
    let matches = terminal.find_matches("perf marker", SearchOptions::default());
    let search_us = search_started.elapsed().as_micros();

    println!(
        "{{\"rows\":{rows},\"cols\":{cols},\"input_lines\":{lines},\"snapshots\":{snapshots},\
         \"max_scrollback_lines\":{},\"scrollback_lines\":{},\
         \"feed_us\":{feed_us},\"tail_snapshot_us\":{tail_snapshot_us},\
         \"history_snapshot_us\":{history_snapshot_us},\"history_viewport_offset\":{history_viewport_offset},\
         \"tail_snapshot_rows\":{tail_snapshot_rows},\"history_snapshot_rows\":{history_snapshot_rows},\
         \"search_matches\":{},\"search_us\":{search_us}}}",
        terminal.max_scrollback_lines(),
        terminal.scrollback_line_count(),
        matches.len()
    );
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

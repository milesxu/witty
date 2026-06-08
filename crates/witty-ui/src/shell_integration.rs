use serde::{Deserialize, Serialize};
use witty_core::{
    BasicTerminal, CellFlags, CellPoint, GridSize, Rgba, TerminalCurrentDirectory,
    TerminalPointAnchor, TerminalScreen, TerminalShellIntegrationEvent,
    TerminalShellIntegrationMarker, TerminalTextRange, TerminalVisibleRowAnchor,
};
use witty_plugin_api::{
    CommandInvocationContext, CommandRegistration, PluginCommandBlock, PluginCommandBlockTextRange,
    PluginCurrentDirectory,
};
use witty_render_wgpu::{
    CellMetrics, FramePlan, GlyphBatchItem, PixelPoint, PixelSize, RectBatchItem,
};

pub const COMMAND_BLOCK_ACTION_MENU_COMMAND_ID: &str = "witty.command_block.actions";
pub const COMMAND_BLOCK_SELECT_LATEST_COMMAND_ID: &str = "witty.command_block.latest";
pub const COMMAND_BLOCK_SELECT_PREVIOUS_COMMAND_ID: &str = "witty.command_block.previous";
pub const COMMAND_BLOCK_SELECT_NEXT_COMMAND_ID: &str = "witty.command_block.next";
pub const COMMAND_BLOCK_CLEAR_SELECTION_COMMAND_ID: &str = "witty.command_block.clear";
pub const COMMAND_BLOCK_COPY_COMMAND_ID: &str = "witty.command_block.copy_command";
pub const COMMAND_BLOCK_COPY_OUTPUT_ID: &str = "witty.command_block.copy_output";
pub const COMMAND_BLOCK_TOGGLE_FOLD_COMMAND_ID: &str = "witty.command_block.toggle_fold";
pub const SELECTED_COMMAND_BLOCK_BACKGROUND: Rgba = Rgba::with_alpha(42, 83, 92, 72);
pub const SELECTED_COMMAND_BLOCK_GUTTER: Rgba = Rgba::rgb(93, 214, 176);
pub const HOVERED_COMMAND_BLOCK_BACKGROUND: Rgba = Rgba::with_alpha(80, 105, 118, 44);
pub const HOVERED_COMMAND_BLOCK_GUTTER: Rgba = Rgba::with_alpha(214, 226, 232, 176);
pub const COMMAND_BLOCK_GUTTER_SUCCESS: Rgba = Rgba::with_alpha(83, 184, 137, 132);
pub const COMMAND_BLOCK_GUTTER_FAILURE: Rgba = Rgba::with_alpha(218, 86, 86, 150);
pub const COMMAND_BLOCK_GUTTER_UNKNOWN: Rgba = Rgba::with_alpha(142, 158, 170, 110);
pub const COMMAND_BLOCK_STATUS_LABEL_BACKGROUND: Rgba = Rgba::with_alpha(18, 22, 28, 220);
pub const COMMAND_BLOCK_STATUS_LABEL_SUCCESS_TEXT: Rgba = Rgba::rgb(118, 225, 170);
pub const COMMAND_BLOCK_STATUS_LABEL_FAILURE_TEXT: Rgba = Rgba::rgb(255, 142, 142);
pub const COMMAND_BLOCK_STATUS_LABEL_UNKNOWN_TEXT: Rgba = Rgba::rgb(170, 178, 185);
pub const COMMAND_BLOCK_FOLDED_ROW_MASK_BACKGROUND: Rgba = Rgba::BLACK;
pub const COMMAND_BLOCK_ACTION_MENU_BACKGROUND: Rgba = Rgba::rgb(18, 22, 28);
pub const COMMAND_BLOCK_ACTION_MENU_SELECTED: Rgba = Rgba::rgb(42, 76, 118);
pub const COMMAND_BLOCK_ACTION_MENU_TEXT: Rgba = Rgba::rgb(238, 242, 245);
pub const COMMAND_BLOCK_ACTION_MENU_MUTED_TEXT: Rgba = Rgba::rgb(150, 160, 165);

pub fn command_block_command_registrations() -> Vec<CommandRegistration> {
    [
        (
            COMMAND_BLOCK_ACTION_MENU_COMMAND_ID,
            "Command Block: Actions",
        ),
        (
            COMMAND_BLOCK_SELECT_LATEST_COMMAND_ID,
            "Command Block: Latest",
        ),
        (
            COMMAND_BLOCK_SELECT_PREVIOUS_COMMAND_ID,
            "Command Block: Previous",
        ),
        (COMMAND_BLOCK_SELECT_NEXT_COMMAND_ID, "Command Block: Next"),
        (
            COMMAND_BLOCK_CLEAR_SELECTION_COMMAND_ID,
            "Command Block: Clear",
        ),
        (COMMAND_BLOCK_COPY_OUTPUT_ID, "Command Block: Copy Output"),
        (COMMAND_BLOCK_COPY_COMMAND_ID, "Command Block: Copy Command"),
        (
            COMMAND_BLOCK_TOGGLE_FOLD_COMMAND_ID,
            "Command Block: Toggle Fold",
        ),
    ]
    .into_iter()
    .map(|(id, title)| CommandRegistration {
        id: id.to_owned(),
        title: title.to_owned(),
        source_plugin: "builtin".to_owned(),
    })
    .collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandBlockActionMenuItem {
    pub id: &'static str,
    pub title: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandBlockActionMenuVisibleItem {
    pub item: CommandBlockActionMenuItem,
    pub index: usize,
    pub selected: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommandBlockActionMenu {
    open: bool,
    selected: usize,
    block_id: Option<u64>,
}

impl CommandBlockActionMenu {
    pub fn open_for_selected_block(&mut self, shell_integration: &ShellIntegrationState) -> bool {
        let Some(block_id) = shell_integration.selected_block_id() else {
            self.close();
            return false;
        };

        self.open_for_block(block_id);
        true
    }

    pub fn open_for_block(&mut self, block_id: u64) {
        self.open = true;
        self.selected = self
            .selected
            .min(command_block_action_menu_items().len() - 1);
        self.block_id = Some(block_id);
    }

    pub fn close(&mut self) {
        self.open = false;
        self.selected = 0;
        self.block_id = None;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn block_id(&self) -> Option<u64> {
        self.open.then_some(self.block_id).flatten()
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.open.then_some(self.selected)
    }

    pub fn selected_item(&self) -> Option<CommandBlockActionMenuItem> {
        self.open
            .then(|| {
                command_block_action_menu_items()
                    .get(self.selected)
                    .copied()
            })
            .flatten()
    }

    pub fn selected_command_id(&self) -> Option<&'static str> {
        self.selected_item().map(|item| item.id)
    }

    pub fn move_selection(&mut self, delta: isize) {
        if !self.open {
            return;
        }
        let max_index = command_block_action_menu_items().len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, max_index) as usize;
    }

    pub fn confirm(&mut self) -> Option<&'static str> {
        let command_id = self.selected_command_id();
        self.close();
        command_id
    }

    pub fn visible_items(&self) -> Vec<CommandBlockActionMenuVisibleItem> {
        if !self.open {
            return Vec::new();
        }

        command_block_action_menu_items()
            .iter()
            .copied()
            .enumerate()
            .map(|(index, item)| CommandBlockActionMenuVisibleItem {
                item,
                index,
                selected: index == self.selected,
            })
            .collect()
    }
}

pub fn command_block_action_menu_items() -> &'static [CommandBlockActionMenuItem] {
    &[
        CommandBlockActionMenuItem {
            id: COMMAND_BLOCK_COPY_OUTPUT_ID,
            title: "Copy Output",
        },
        CommandBlockActionMenuItem {
            id: COMMAND_BLOCK_COPY_COMMAND_ID,
            title: "Copy Command",
        },
        CommandBlockActionMenuItem {
            id: COMMAND_BLOCK_CLEAR_SELECTION_COMMAND_ID,
            title: "Clear Selection",
        },
        CommandBlockActionMenuItem {
            id: COMMAND_BLOCK_TOGGLE_FOLD_COMMAND_ID,
            title: "Toggle Fold",
        },
    ]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandBlockCopyTarget {
    Command,
    Output,
}

pub fn command_block_copy_target(command_id: &str) -> Option<CommandBlockCopyTarget> {
    match command_id {
        COMMAND_BLOCK_COPY_COMMAND_ID => Some(CommandBlockCopyTarget::Command),
        COMMAND_BLOCK_COPY_OUTPUT_ID => Some(CommandBlockCopyTarget::Output),
        _ => None,
    }
}

pub fn selected_command_block_copy_text(
    terminal: &BasicTerminal,
    shell_integration: &ShellIntegrationState,
    target: CommandBlockCopyTarget,
) -> Option<String> {
    let ranges = shell_integration.selected_command_block_text_ranges()?;
    match target {
        CommandBlockCopyTarget::Command => terminal.text_for_range(ranges.command_text_range()),
        CommandBlockCopyTarget::Output => ranges
            .output_text_range()
            .and_then(|range| terminal.text_for_range(range))
            .map(normalize_command_block_output_copy_text),
    }
}

pub fn apply_command_block_command(
    shell_integration: &mut ShellIntegrationState,
    screen: TerminalScreen,
    command_id: &str,
) -> bool {
    match command_id {
        COMMAND_BLOCK_SELECT_LATEST_COMMAND_ID => {
            shell_integration.select_latest_completed_block_for_screen(screen);
            true
        }
        COMMAND_BLOCK_SELECT_PREVIOUS_COMMAND_ID => {
            shell_integration.select_previous_completed_block_for_screen(screen);
            true
        }
        COMMAND_BLOCK_SELECT_NEXT_COMMAND_ID => {
            shell_integration.select_next_completed_block_for_screen(screen);
            true
        }
        COMMAND_BLOCK_CLEAR_SELECTION_COMMAND_ID => {
            shell_integration.clear_selection();
            true
        }
        COMMAND_BLOCK_TOGGLE_FOLD_COMMAND_ID => shell_integration
            .toggle_selected_completed_block_folded_for_screen(screen)
            .is_some(),
        _ => false,
    }
}

fn normalize_command_block_output_copy_text(mut text: String) -> String {
    if text.starts_with('\n') {
        text.remove(0);
    }
    while text.ends_with('\n') {
        text.pop();
    }
    text
}

pub fn command_block_status_label(exit_code: Option<i32>) -> String {
    match exit_code {
        Some(0) => "ok".to_owned(),
        Some(code) => format!("exit {code}"),
        None => "done".to_owned(),
    }
}

pub fn command_block_status_label_with_duration(
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
) -> String {
    command_block_status_label_with_duration_and_fold_state(exit_code, duration_ms, false)
}

pub fn command_block_status_label_with_duration_and_fold_state(
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
    folded: bool,
) -> String {
    command_block_status_label_with_duration_and_folded_hidden_rows(
        exit_code,
        duration_ms,
        folded.then_some(0),
    )
}

pub fn command_block_status_label_with_duration_and_folded_hidden_rows(
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
    folded_hidden_rows: Option<u16>,
) -> String {
    let status = command_block_status_label(exit_code);
    let status = match duration_ms {
        Some(duration_ms) => format!("{status} {}", command_block_duration_label(duration_ms)),
        None => status,
    };
    match folded_hidden_rows {
        Some(0) => format!("folded {status}"),
        Some(1) => format!("folded 1 row {status}"),
        Some(rows) => format!("folded {rows} rows {status}"),
        None => status,
    }
}

fn command_block_duration_label(duration_ms: u64) -> String {
    if duration_ms < 1_000 {
        return format!("{duration_ms}ms");
    }

    if duration_ms < 10_000 {
        let tenths = duration_ms.saturating_add(50) / 100;
        let seconds = tenths / 10;
        let fraction = tenths % 10;
        if fraction == 0 {
            return format!("{seconds}s");
        }
        return format!("{seconds}.{fraction}s");
    }

    let total_seconds = duration_ms.saturating_add(500) / 1_000;
    if total_seconds < 60 {
        return format!("{total_seconds}s");
    }

    format!("{}m{:02}s", total_seconds / 60, total_seconds % 60)
}

pub fn apply_command_block_folded_row_mask_with_anchors(
    frame: &mut FramePlan,
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    visible_row_anchors: &[TerminalVisibleRowAnchor],
    metrics: CellMetrics,
    grid_size: GridSize,
) -> usize {
    if grid_size.rows == 0 || grid_size.cols == 0 {
        return 0;
    }

    let spans = shell_integration.folded_hidden_row_spans_intersecting_visible_rows(
        screen,
        visible_row_anchors,
        grid_size.rows,
    );
    let mut masked_rows = Vec::new();
    let mut rects = 0;

    for span in spans {
        if span.hidden_start_row >= grid_size.rows {
            continue;
        }
        let end_row = span.hidden_end_row.min(grid_size.rows.saturating_sub(1));
        if span.hidden_start_row > end_row {
            continue;
        }

        for row in span.hidden_start_row..=end_row {
            if masked_rows.contains(&row) {
                continue;
            }
            masked_rows.push(row);

            let origin = cell_origin(CellPoint::new(row, 0), metrics);
            let size = PixelSize {
                width: f32::from(grid_size.cols) * metrics.cell.width,
                height: metrics.cell.height,
            };
            let color = folded_row_mask_background_for_row(frame, row, metrics)
                .unwrap_or(COMMAND_BLOCK_FOLDED_ROW_MASK_BACKGROUND);

            frame
                .glyphs
                .retain(|glyph| !glyph_origin_inside(glyph, origin, size));
            frame.backgrounds.push(RectBatchItem {
                origin,
                size,
                color,
            });
            rects += 1;
        }
    }

    rects
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TerminalCommandBlockFoldedFrameRemapStats {
    pub hidden_rows: u16,
    pub removed_glyphs: usize,
    pub remapped_glyphs: usize,
    pub removed_rects: usize,
    pub remapped_rects: usize,
}

pub fn apply_command_block_folded_frame_remap_with_anchors(
    frame: &mut FramePlan,
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    visible_row_anchors: &[TerminalVisibleRowAnchor],
    metrics: CellMetrics,
    grid_size: GridSize,
) -> TerminalCommandBlockFoldedFrameRemapStats {
    if grid_size.rows == 0 || metrics.cell.height <= 0.0 || !metrics.cell.height.is_finite() {
        return TerminalCommandBlockFoldedFrameRemapStats::default();
    }

    let compact_rows =
        shell_integration.folded_compact_visual_rows(screen, visible_row_anchors, grid_size.rows);
    let hidden_rows = compact_rows.iter().filter(|row| row.hidden).count() as u16;
    if hidden_rows == 0 {
        return TerminalCommandBlockFoldedFrameRemapStats::default();
    }

    let mut stats = TerminalCommandBlockFoldedFrameRemapStats {
        hidden_rows,
        ..TerminalCommandBlockFoldedFrameRemapStats::default()
    };
    remap_folded_frame_glyphs(&mut frame.glyphs, &compact_rows, metrics, &mut stats);
    remap_folded_frame_rects(&mut frame.backgrounds, &compact_rows, metrics, &mut stats);
    remap_folded_frame_rects(&mut frame.selection, &compact_rows, metrics, &mut stats);
    remap_folded_frame_rects(
        &mut frame.search_highlights,
        &compact_rows,
        metrics,
        &mut stats,
    );
    remap_folded_frame_rects(
        &mut frame.hyperlink_hover,
        &compact_rows,
        metrics,
        &mut stats,
    );
    remap_folded_frame_rects(
        &mut frame.hyperlink_underlines,
        &compact_rows,
        metrics,
        &mut stats,
    );
    remap_folded_frame_rects(&mut frame.ime_preedit, &compact_rows, metrics, &mut stats);
    if let Some(mut cursor) = frame.cursor.take() {
        match remapped_folded_pixel_origin(cursor.origin, &compact_rows, metrics) {
            Some(origin) => {
                if origin != cursor.origin {
                    cursor.origin = origin;
                    stats.remapped_rects += 1;
                }
                frame.cursor = Some(cursor);
            }
            None => {
                stats.removed_rects += 1;
            }
        }
    }
    stats
}

pub fn command_block_folded_terminal_row_for_compact_visual_row_with_anchors(
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    visible_row_anchors: &[TerminalVisibleRowAnchor],
    compact_visual_row: u16,
    grid_size: GridSize,
) -> Option<u16> {
    if compact_visual_row >= grid_size.rows {
        return None;
    }

    let compact_rows =
        shell_integration.folded_compact_visual_rows(screen, visible_row_anchors, grid_size.rows);
    compact_rows
        .iter()
        .find(|row| !row.hidden && row.compact_row == Some(compact_visual_row))
        .map(|row| row.visible_row)
}

pub fn command_block_folded_visual_pixel_to_terminal_pixel_with_anchors(
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    visible_row_anchors: &[TerminalVisibleRowAnchor],
    compact_visual_point: PixelPoint,
    metrics: CellMetrics,
    grid_size: GridSize,
) -> Option<PixelPoint> {
    if grid_size.rows == 0 || metrics.cell.height <= 0.0 || !metrics.cell.height.is_finite() {
        return None;
    }
    let compact_visual_row = visible_row_for_pixel_origin(compact_visual_point, metrics)?;
    let terminal_row = command_block_folded_terminal_row_for_compact_visual_row_with_anchors(
        shell_integration,
        screen,
        visible_row_anchors,
        compact_visual_row,
        grid_size,
    )?;
    let compact_row_origin_y =
        metrics.padding.y + f32::from(compact_visual_row) * metrics.cell.height;
    let terminal_row_origin_y = metrics.padding.y + f32::from(terminal_row) * metrics.cell.height;
    Some(PixelPoint {
        x: compact_visual_point.x,
        y: terminal_row_origin_y + (compact_visual_point.y - compact_row_origin_y),
    })
}

pub fn apply_command_block_selection_overlay(
    frame: &mut FramePlan,
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    metrics: CellMetrics,
    grid_size: GridSize,
) -> usize {
    apply_command_block_selection_overlay_for_span(
        frame,
        shell_integration
            .selected_completed_block()
            .filter(|block| block.screen == screen)
            .map(TerminalCommandBlock::chrome_row_span),
        metrics,
        grid_size,
    )
}

pub fn apply_command_block_selection_overlay_with_anchors(
    frame: &mut FramePlan,
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    visible_row_anchors: &[TerminalVisibleRowAnchor],
    metrics: CellMetrics,
    grid_size: GridSize,
) -> usize {
    if grid_size.rows == 0 || grid_size.cols == 0 {
        return 0;
    }
    let Some(block) = shell_integration
        .selected_completed_block()
        .filter(|block| block.screen == screen)
    else {
        return 0;
    };

    apply_command_block_selection_overlay_for_span(
        frame,
        block.visible_chrome_row_span(visible_row_anchors),
        metrics,
        grid_size,
    )
}

pub fn apply_command_block_status_label_overlay_with_anchors(
    frame: &mut FramePlan,
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    visible_row_anchors: &[TerminalVisibleRowAnchor],
    hovered_block_id: Option<u64>,
    metrics: CellMetrics,
    grid_size: GridSize,
) -> usize {
    if grid_size.rows == 0 || grid_size.cols == 0 {
        return 0;
    }

    let selected_block_id = shell_integration.selected_block_id();
    let mut rects = 0;
    if let Some(id) = selected_block_id {
        rects += apply_command_block_status_label_for_block(
            frame,
            shell_integration,
            screen,
            visible_row_anchors,
            id,
            metrics,
            grid_size,
        );
    }
    if let Some(hovered_block_id) = hovered_block_id.filter(|id| Some(*id) != selected_block_id) {
        rects += apply_command_block_status_label_for_block(
            frame,
            shell_integration,
            screen,
            visible_row_anchors,
            hovered_block_id,
            metrics,
            grid_size,
        );
    }

    rects
}

fn apply_command_block_status_label_for_block(
    frame: &mut FramePlan,
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    visible_row_anchors: &[TerminalVisibleRowAnchor],
    block_id: u64,
    metrics: CellMetrics,
    grid_size: GridSize,
) -> usize {
    let Some(block) = shell_integration
        .completed_block_by_id(block_id)
        .filter(|block| block.screen == screen)
    else {
        return 0;
    };
    let Some(span) = block.visible_row_span(visible_row_anchors) else {
        return 0;
    };

    let label = if block.folded {
        let folded_hidden_rows = block
            .folded_hidden_visible_row_span(visible_row_anchors)
            .map(|span| span.hidden_row_count())
            .unwrap_or(0);
        command_block_status_label_with_duration_and_folded_hidden_rows(
            block.exit_code,
            block.duration_ms,
            Some(folded_hidden_rows),
        )
    } else {
        command_block_status_label_with_duration(block.exit_code, block.duration_ms)
    };
    let label_cols = (label.chars().count() as u16 + 2).clamp(1, grid_size.cols);
    let start_col = grid_size.cols.saturating_sub(label_cols);
    let row = span.start_row.min(grid_size.rows.saturating_sub(1));
    let origin = cell_origin(CellPoint::new(row, start_col), metrics);
    let size = PixelSize {
        width: f32::from(label_cols) * metrics.cell.width,
        height: metrics.cell.height,
    };

    frame
        .glyphs
        .retain(|glyph| !glyph_origin_inside(glyph, origin, size));
    frame.backgrounds.push(RectBatchItem {
        origin,
        size,
        color: COMMAND_BLOCK_STATUS_LABEL_BACKGROUND,
    });

    let text_width = label_cols.saturating_sub(2);
    if text_width > 0 && start_col.saturating_add(1) < grid_size.cols {
        frame.glyphs.push(GlyphBatchItem {
            origin: cell_origin(CellPoint::new(row, start_col.saturating_add(1)), metrics),
            text: truncate_ascii_cells(&label, text_width),
            color: command_block_status_label_text_color(block.exit_code),
            style_flags: CellFlags::default(),
        });
    }

    1
}

fn command_block_status_label_text_color(exit_code: Option<i32>) -> Rgba {
    match exit_code {
        Some(0) => COMMAND_BLOCK_STATUS_LABEL_SUCCESS_TEXT,
        Some(_) => COMMAND_BLOCK_STATUS_LABEL_FAILURE_TEXT,
        None => COMMAND_BLOCK_STATUS_LABEL_UNKNOWN_TEXT,
    }
}

pub fn apply_command_block_gutter_overlay_with_anchors(
    frame: &mut FramePlan,
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    visible_row_anchors: &[TerminalVisibleRowAnchor],
    metrics: CellMetrics,
    grid_size: GridSize,
) -> usize {
    if grid_size.rows == 0 || grid_size.cols == 0 {
        return 0;
    }

    let selected_id = shell_integration.selected_block_id();
    let mut rects = 0;
    for block in shell_integration.completed_blocks_for_screen(screen) {
        if Some(block.id) == selected_id {
            continue;
        }
        let Some(span) = block.visible_chrome_row_span(visible_row_anchors) else {
            continue;
        };
        rects += apply_command_block_gutter_span(frame, &span, metrics, grid_size);
    }
    rects
}

pub fn apply_command_block_gutter_hover_overlay_with_anchors(
    frame: &mut FramePlan,
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    visible_row_anchors: &[TerminalVisibleRowAnchor],
    hovered_block_id: Option<u64>,
    metrics: CellMetrics,
    grid_size: GridSize,
) -> usize {
    if grid_size.rows == 0 || grid_size.cols == 0 {
        return 0;
    }

    let Some(hovered_block_id) = hovered_block_id else {
        return 0;
    };
    if Some(hovered_block_id) == shell_integration.selected_block_id() {
        return 0;
    }

    let Some(block) = shell_integration
        .completed_blocks_for_screen(screen)
        .find(|block| block.id == hovered_block_id)
    else {
        return 0;
    };

    apply_command_block_gutter_hover_span(
        frame,
        block.visible_chrome_row_span(visible_row_anchors),
        metrics,
        grid_size,
    )
}

pub fn apply_command_block_action_menu_overlay(
    frame: &mut FramePlan,
    menu: &CommandBlockActionMenu,
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    visible_row_anchors: &[TerminalVisibleRowAnchor],
    metrics: CellMetrics,
    grid_size: GridSize,
) -> usize {
    if !menu.is_open() || grid_size.rows == 0 || grid_size.cols == 0 {
        return 0;
    }
    let Some(block_id) = menu.block_id() else {
        return 0;
    };
    let Some(block) = shell_integration
        .completed_block_by_id(block_id)
        .filter(|block| block.screen == screen)
    else {
        return 0;
    };
    let Some(span) = block.visible_row_span(visible_row_anchors) else {
        return 0;
    };
    let Some(panel) = command_block_action_menu_panel(span, grid_size) else {
        return 0;
    };

    let panel_origin = cell_origin(panel.start, metrics);
    let panel_size = PixelSize {
        width: f32::from(panel.cols) * metrics.cell.width,
        height: f32::from(panel.rows) * metrics.cell.height,
    };

    frame
        .glyphs
        .retain(|glyph| !glyph_origin_inside(glyph, panel_origin, panel_size));

    frame.backgrounds.push(RectBatchItem {
        origin: panel_origin,
        size: panel_size,
        color: COMMAND_BLOCK_ACTION_MENU_BACKGROUND,
    });
    let mut rects = 1;

    push_command_block_action_menu_text(
        frame,
        panel,
        metrics,
        0,
        1,
        "Block Actions",
        COMMAND_BLOCK_ACTION_MENU_TEXT,
    );

    for visible in menu.visible_items() {
        let row = visible.index as u16 + 1;
        if row >= panel.rows {
            continue;
        }
        if visible.selected {
            frame.backgrounds.push(RectBatchItem {
                origin: cell_origin(
                    CellPoint::new(panel.start.row + row, panel.start.col),
                    metrics,
                ),
                size: PixelSize {
                    width: f32::from(panel.cols) * metrics.cell.width,
                    height: metrics.cell.height,
                },
                color: COMMAND_BLOCK_ACTION_MENU_SELECTED,
            });
            rects += 1;
        }
        let marker = if visible.selected { ">" } else { " " };
        let text = truncate_ascii_cells(
            &format!("{marker} {}", visible.item.title),
            panel.cols.saturating_sub(2),
        );
        push_command_block_action_menu_text(
            frame,
            panel,
            metrics,
            row,
            1,
            &text,
            COMMAND_BLOCK_ACTION_MENU_TEXT,
        );
    }

    rects
}

pub fn command_block_gutter_hit_test_with_anchors(
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    visible_row_anchors: &[TerminalVisibleRowAnchor],
    point: PixelPoint,
    metrics: CellMetrics,
    grid_size: GridSize,
) -> Option<TerminalCommandBlockGutterHit> {
    if grid_size.rows == 0 || grid_size.cols == 0 || !point_is_inside_gutter(point, metrics) {
        return None;
    }
    let visible_row = visible_row_for_gutter_hit(point, metrics, grid_size)?;
    let selected_id = shell_integration.selected_block_id();

    shell_integration
        .completed
        .iter()
        .rev()
        .filter(|block| block.screen == screen)
        .filter_map(|block| {
            let span = block.visible_chrome_row_span(visible_row_anchors)?;
            (visible_row >= span.start_row && visible_row <= span.end_row).then_some((block, span))
        })
        .map(|(block, span)| TerminalCommandBlockGutterHit {
            id: block.id,
            screen: block.screen,
            visible_row,
            start_row: span.start_row,
            end_row: span.end_row,
            selected: Some(block.id) == selected_id,
            exit_code: block.exit_code,
        })
        .next()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CommandBlockActionMenuPanel {
    start: CellPoint,
    cols: u16,
    rows: u16,
}

fn command_block_action_menu_panel(
    span: TerminalCommandBlockRowSpan,
    grid_size: GridSize,
) -> Option<CommandBlockActionMenuPanel> {
    if grid_size.rows == 0 || grid_size.cols == 0 {
        return None;
    }

    let rows = (command_block_action_menu_items().len() as u16 + 1).min(grid_size.rows);
    let cols = grid_size.cols.clamp(1, 32);
    let start_col = if grid_size.cols > cols + 1 { 1 } else { 0 };
    let preferred_row = span.start_row.min(grid_size.rows.saturating_sub(1));
    let start_row = preferred_row.min(grid_size.rows.saturating_sub(rows));

    Some(CommandBlockActionMenuPanel {
        start: CellPoint::new(start_row, start_col),
        cols,
        rows,
    })
}

fn push_command_block_action_menu_text(
    frame: &mut FramePlan,
    panel: CommandBlockActionMenuPanel,
    metrics: CellMetrics,
    row_offset: u16,
    col_offset: u16,
    text: &str,
    color: Rgba,
) {
    if row_offset >= panel.rows || col_offset >= panel.cols {
        return;
    }

    frame.glyphs.push(GlyphBatchItem {
        origin: cell_origin(
            CellPoint::new(panel.start.row + row_offset, panel.start.col + col_offset),
            metrics,
        ),
        text: text.to_owned(),
        color,
        style_flags: CellFlags::default(),
    });
}

fn folded_row_mask_background_for_row(
    frame: &FramePlan,
    row: u16,
    metrics: CellMetrics,
) -> Option<Rgba> {
    let origin = cell_origin(CellPoint::new(row, 0), metrics);
    frame
        .backgrounds
        .iter()
        .rev()
        .find(|background| point_inside_rect(origin, background.origin, background.size))
        .map(|background| background.color)
}

fn glyph_origin_inside(glyph: &GlyphBatchItem, origin: PixelPoint, size: PixelSize) -> bool {
    point_inside_rect(glyph.origin, origin, size)
}

fn remap_folded_frame_glyphs(
    glyphs: &mut Vec<GlyphBatchItem>,
    compact_rows: &[TerminalCommandBlockFoldedCompactVisualRow],
    metrics: CellMetrics,
    stats: &mut TerminalCommandBlockFoldedFrameRemapStats,
) {
    glyphs.retain_mut(|glyph| {
        match remapped_folded_pixel_origin(glyph.origin, compact_rows, metrics) {
            Some(origin) => {
                if origin != glyph.origin {
                    glyph.origin = origin;
                    stats.remapped_glyphs += 1;
                }
                true
            }
            None => {
                stats.removed_glyphs += 1;
                false
            }
        }
    });
}

fn remap_folded_frame_rects(
    rects: &mut Vec<RectBatchItem>,
    compact_rows: &[TerminalCommandBlockFoldedCompactVisualRow],
    metrics: CellMetrics,
    stats: &mut TerminalCommandBlockFoldedFrameRemapStats,
) {
    rects.retain_mut(|rect| {
        match remapped_folded_pixel_origin(rect.origin, compact_rows, metrics) {
            Some(origin) => {
                if origin != rect.origin {
                    rect.origin = origin;
                    stats.remapped_rects += 1;
                }
                true
            }
            None => {
                stats.removed_rects += 1;
                false
            }
        }
    });
}

fn remapped_folded_pixel_origin(
    origin: PixelPoint,
    compact_rows: &[TerminalCommandBlockFoldedCompactVisualRow],
    metrics: CellMetrics,
) -> Option<PixelPoint> {
    let Some(visible_row) = visible_row_for_pixel_origin(origin, metrics) else {
        return Some(origin);
    };
    let Some(row) = compact_rows.get(usize::from(visible_row)) else {
        return Some(origin);
    };
    if row.hidden {
        return None;
    }

    let Some(compact_row) = row.compact_row else {
        return Some(origin);
    };
    let visible_row_origin_y = metrics.padding.y + f32::from(visible_row) * metrics.cell.height;
    let compact_row_origin_y = metrics.padding.y + f32::from(compact_row) * metrics.cell.height;
    Some(PixelPoint {
        x: origin.x,
        y: compact_row_origin_y + (origin.y - visible_row_origin_y),
    })
}

fn visible_row_for_pixel_origin(origin: PixelPoint, metrics: CellMetrics) -> Option<u16> {
    if !origin.y.is_finite() || !metrics.padding.y.is_finite() || !metrics.cell.height.is_finite() {
        return None;
    }
    if metrics.cell.height <= 0.0 || origin.y < metrics.padding.y {
        return None;
    }

    let row = ((origin.y - metrics.padding.y) / metrics.cell.height).floor();
    if row < 0.0 || row > f32::from(u16::MAX) {
        return None;
    }
    Some(row as u16)
}

fn point_inside_rect(point: PixelPoint, origin: PixelPoint, size: PixelSize) -> bool {
    point.x >= origin.x
        && point.x < origin.x + size.width
        && point.y >= origin.y
        && point.y < origin.y + size.height
}

fn truncate_ascii_cells(text: &str, width: u16) -> String {
    text.chars().take(usize::from(width)).collect()
}

fn apply_command_block_selection_overlay_for_span(
    frame: &mut FramePlan,
    span: Option<TerminalCommandBlockRowSpan>,
    metrics: CellMetrics,
    grid_size: GridSize,
) -> usize {
    if grid_size.rows == 0 || grid_size.cols == 0 {
        return 0;
    }
    let Some(span) = span else {
        return 0;
    };
    if span.start_row >= grid_size.rows {
        return 0;
    }

    let start_row = span.start_row;
    let end_row = span.end_row.min(grid_size.rows.saturating_sub(1));
    let mut rects = 0;
    for row in start_row..=end_row {
        let origin = cell_origin(CellPoint::new(row, 0), metrics);
        frame.backgrounds.push(RectBatchItem {
            origin,
            size: PixelSize {
                width: f32::from(grid_size.cols) * metrics.cell.width,
                height: metrics.cell.height,
            },
            color: SELECTED_COMMAND_BLOCK_BACKGROUND,
        });
        rects += 1;

        frame.backgrounds.push(RectBatchItem {
            origin,
            size: PixelSize {
                width: metrics.cell.width.clamp(1.0, 3.0),
                height: metrics.cell.height,
            },
            color: SELECTED_COMMAND_BLOCK_GUTTER,
        });
        rects += 1;
    }
    rects
}

fn apply_command_block_gutter_hover_span(
    frame: &mut FramePlan,
    span: Option<TerminalCommandBlockRowSpan>,
    metrics: CellMetrics,
    grid_size: GridSize,
) -> usize {
    let Some(span) = span else {
        return 0;
    };
    if span.start_row >= grid_size.rows {
        return 0;
    }

    let end_row = span.end_row.min(grid_size.rows.saturating_sub(1));
    let mut rects = 0;
    for row in span.start_row..=end_row {
        let origin = cell_origin(CellPoint::new(row, 0), metrics);
        frame.backgrounds.push(RectBatchItem {
            origin,
            size: PixelSize {
                width: f32::from(grid_size.cols) * metrics.cell.width,
                height: metrics.cell.height,
            },
            color: HOVERED_COMMAND_BLOCK_BACKGROUND,
        });
        rects += 1;

        frame.backgrounds.push(RectBatchItem {
            origin,
            size: PixelSize {
                width: metrics.cell.width.clamp(1.0, 3.0),
                height: metrics.cell.height,
            },
            color: HOVERED_COMMAND_BLOCK_GUTTER,
        });
        rects += 1;
    }
    rects
}

fn apply_command_block_gutter_span(
    frame: &mut FramePlan,
    span: &TerminalCommandBlockRowSpan,
    metrics: CellMetrics,
    grid_size: GridSize,
) -> usize {
    if span.start_row >= grid_size.rows {
        return 0;
    }

    let end_row = span.end_row.min(grid_size.rows.saturating_sub(1));
    let color = command_block_gutter_color(span.exit_code);
    let mut rects = 0;
    for row in span.start_row..=end_row {
        let origin = cell_origin(CellPoint::new(row, 0), metrics);
        frame.backgrounds.push(RectBatchItem {
            origin,
            size: PixelSize {
                width: metrics.cell.width.clamp(1.0, 2.0),
                height: metrics.cell.height,
            },
            color,
        });
        rects += 1;
    }
    rects
}

fn command_block_gutter_color(exit_code: Option<i32>) -> Rgba {
    match exit_code {
        Some(0) => COMMAND_BLOCK_GUTTER_SUCCESS,
        Some(_) => COMMAND_BLOCK_GUTTER_FAILURE,
        None => COMMAND_BLOCK_GUTTER_UNKNOWN,
    }
}

fn point_is_inside_gutter(point: PixelPoint, metrics: CellMetrics) -> bool {
    if !point.x.is_finite() || !metrics.padding.x.is_finite() || !metrics.cell.width.is_finite() {
        return false;
    }
    if metrics.cell.width <= 0.0 {
        return false;
    }

    point.x >= metrics.padding.x && point.x < metrics.padding.x + metrics.cell.width
}

fn visible_row_for_gutter_hit(
    point: PixelPoint,
    metrics: CellMetrics,
    grid_size: GridSize,
) -> Option<u16> {
    if !point.y.is_finite() || !metrics.padding.y.is_finite() || !metrics.cell.height.is_finite() {
        return None;
    }
    if metrics.cell.height <= 0.0 {
        return None;
    }

    let relative_y = point.y - metrics.padding.y;
    if relative_y < 0.0 {
        return None;
    }
    let row = (relative_y / metrics.cell.height).floor();
    if row < 0.0 || row >= f32::from(grid_size.rows) {
        return None;
    }
    Some(row as u16)
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ShellIntegrationState {
    next_block_id: u64,
    pending: Option<PendingCommandBlock>,
    completed: Vec<TerminalCommandBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current_directory: Option<TerminalCurrentDirectory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selected_block_id: Option<u64>,
}

impl ShellIntegrationState {
    pub fn apply_event(&mut self, event: TerminalShellIntegrationEvent) {
        self.apply_event_observed_at_ms(event, None);
    }

    pub fn apply_event_at_ms(&mut self, event: TerminalShellIntegrationEvent, observed_at_ms: u64) {
        self.apply_event_observed_at_ms(event, Some(observed_at_ms));
    }

    pub fn apply_current_directory(&mut self, directory: TerminalCurrentDirectory) {
        if let Some(pending) = self
            .pending
            .as_mut()
            .filter(|pending| pending.screen == directory.screen)
        {
            pending.current_directory = Some(directory.clone());
        }
        self.current_directory = Some(directory);
    }

    pub fn current_directory(&self) -> Option<&TerminalCurrentDirectory> {
        self.current_directory.as_ref()
    }

    pub fn current_directory_for_screen(
        &self,
        screen: TerminalScreen,
    ) -> Option<&TerminalCurrentDirectory> {
        self.current_directory
            .as_ref()
            .filter(|directory| directory.screen == screen)
    }

    pub fn command_invocation_context_for_screen(
        &self,
        screen: TerminalScreen,
    ) -> CommandInvocationContext {
        CommandInvocationContext {
            current_directory: self
                .current_directory_for_screen(screen)
                .map(PluginCurrentDirectory::from),
            selected_command_block: self
                .selected_completed_block()
                .filter(|block| block.screen == screen)
                .map(plugin_command_block_from_terminal_block),
        }
    }

    fn apply_event_observed_at_ms(
        &mut self,
        event: TerminalShellIntegrationEvent,
        observed_at_ms: Option<u64>,
    ) {
        match event.marker {
            TerminalShellIntegrationMarker::PromptStart => self.start_prompt(event, observed_at_ms),
            TerminalShellIntegrationMarker::CommandStart => {
                self.mark_command_start(event, observed_at_ms);
            }
            TerminalShellIntegrationMarker::OutputStart => {
                self.mark_output_start(event, observed_at_ms);
            }
            TerminalShellIntegrationMarker::CommandFinished => {
                self.finish_command(event, observed_at_ms);
            }
        }
    }

    pub fn pending_block(&self) -> Option<&PendingCommandBlock> {
        self.pending.as_ref()
    }

    pub fn completed_blocks(&self) -> &[TerminalCommandBlock] {
        &self.completed
    }

    pub fn completed_len(&self) -> usize {
        self.completed.len()
    }

    pub fn completed_len_for_screen(&self, screen: TerminalScreen) -> usize {
        self.completed_blocks_for_screen(screen).count()
    }

    pub fn last_completed_block(&self) -> Option<&TerminalCommandBlock> {
        self.completed.last()
    }

    pub fn selected_block_id(&self) -> Option<u64> {
        self.selected_block_id
    }

    pub fn selected_completed_block(&self) -> Option<&TerminalCommandBlock> {
        self.selected_block_id
            .and_then(|id| self.completed_block_by_id(id))
    }

    pub fn is_completed_block_folded(&self, id: u64) -> bool {
        self.completed_block_by_id(id)
            .map(|block| block.folded)
            .unwrap_or(false)
    }

    pub fn set_completed_block_folded(&mut self, id: u64, folded: bool) -> bool {
        let Some(block) = self.completed_block_by_id_mut(id) else {
            return false;
        };
        block.folded = folded;
        true
    }

    pub fn toggle_completed_block_folded(&mut self, id: u64) -> Option<bool> {
        let block = self.completed_block_by_id_mut(id)?;
        block.folded = !block.folded;
        Some(block.folded)
    }

    pub fn toggle_selected_completed_block_folded_for_screen(
        &mut self,
        screen: TerminalScreen,
    ) -> Option<bool> {
        let id = self.selected_block_id?;
        if self.completed_block_by_id(id)?.screen != screen {
            return None;
        }
        self.toggle_completed_block_folded(id)
    }

    pub fn selected_command_block_text_ranges(&self) -> Option<TerminalCommandBlockTextRanges> {
        self.selected_completed_block()
            .map(TerminalCommandBlock::text_ranges)
    }

    pub fn completed_block_by_id(&self, id: u64) -> Option<&TerminalCommandBlock> {
        self.completed.iter().find(|block| block.id == id)
    }

    fn completed_block_by_id_mut(&mut self, id: u64) -> Option<&mut TerminalCommandBlock> {
        self.completed.iter_mut().find(|block| block.id == id)
    }

    pub fn completed_blocks_for_screen(
        &self,
        screen: TerminalScreen,
    ) -> impl Iterator<Item = &TerminalCommandBlock> {
        self.completed
            .iter()
            .filter(move |block| block.screen == screen)
    }

    pub fn completed_blocks_intersecting_rows(
        &self,
        screen: TerminalScreen,
        start_row: u16,
        end_row_exclusive: u16,
    ) -> Vec<TerminalCommandBlock> {
        self.completed_blocks_for_screen(screen)
            .filter(|block| block.intersects_rows(start_row, end_row_exclusive))
            .cloned()
            .collect()
    }

    pub fn completed_block_row_spans_intersecting_rows(
        &self,
        screen: TerminalScreen,
        start_row: u16,
        end_row_exclusive: u16,
    ) -> Vec<TerminalCommandBlockRowSpan> {
        self.completed_blocks_for_screen(screen)
            .filter(|block| block.intersects_rows(start_row, end_row_exclusive))
            .map(TerminalCommandBlock::row_span)
            .collect()
    }

    pub fn completed_blocks_intersecting_visible_rows(
        &self,
        screen: TerminalScreen,
        visible_row_anchors: &[TerminalVisibleRowAnchor],
        fallback_rows: u16,
    ) -> Vec<TerminalCommandBlock> {
        self.completed_blocks_for_screen(screen)
            .filter(|block| {
                block
                    .visible_row_span(visible_row_anchors)
                    .is_some_and(|span| span.start_row < fallback_rows)
            })
            .cloned()
            .collect()
    }

    pub fn completed_block_row_spans_intersecting_visible_rows(
        &self,
        screen: TerminalScreen,
        visible_row_anchors: &[TerminalVisibleRowAnchor],
        fallback_rows: u16,
    ) -> Vec<TerminalCommandBlockRowSpan> {
        self.completed_blocks_for_screen(screen)
            .filter_map(|block| block.visible_row_span(visible_row_anchors))
            .filter(|span| span.start_row < fallback_rows)
            .collect()
    }

    pub fn folded_hidden_row_spans_intersecting_visible_rows(
        &self,
        screen: TerminalScreen,
        visible_row_anchors: &[TerminalVisibleRowAnchor],
        fallback_rows: u16,
    ) -> Vec<TerminalCommandBlockFoldedHiddenRowSpan> {
        self.completed_blocks_for_screen(screen)
            .filter_map(|block| block.folded_hidden_visible_row_span(visible_row_anchors))
            .filter(|span| {
                span.summary_row < fallback_rows || span.hidden_start_row < fallback_rows
            })
            .collect()
    }

    pub fn folded_compact_visual_rows(
        &self,
        screen: TerminalScreen,
        visible_row_anchors: &[TerminalVisibleRowAnchor],
        fallback_rows: u16,
    ) -> Vec<TerminalCommandBlockFoldedCompactVisualRow> {
        let mut hidden_spans = self.folded_hidden_row_spans_intersecting_visible_rows(
            screen,
            visible_row_anchors,
            fallback_rows,
        );
        hidden_spans.sort_by_key(|span| (span.hidden_start_row, span.hidden_end_row, span.id));

        let mut hidden_rows_before = 0u16;
        (0..fallback_rows)
            .map(|visible_row| {
                let hidden_by_block_id = hidden_spans
                    .iter()
                    .find(|span| {
                        visible_row >= span.hidden_start_row && visible_row <= span.hidden_end_row
                    })
                    .map(|span| span.id);
                let hidden = hidden_by_block_id.is_some();
                let compact_row =
                    (!hidden).then_some(visible_row.saturating_sub(hidden_rows_before));
                let row = TerminalCommandBlockFoldedCompactVisualRow {
                    screen,
                    visible_row,
                    compact_row,
                    hidden,
                    hidden_rows_before,
                    hidden_by_block_id,
                };
                if hidden {
                    hidden_rows_before = hidden_rows_before.saturating_add(1);
                }
                row
            })
            .collect()
    }

    pub fn select_completed_block(&mut self, id: u64) -> Option<&TerminalCommandBlock> {
        if self.completed.iter().any(|block| block.id == id) {
            self.selected_block_id = Some(id);
        }
        self.selected_completed_block()
    }

    pub fn select_latest_completed_block_for_screen(
        &mut self,
        screen: TerminalScreen,
    ) -> Option<&TerminalCommandBlock> {
        let id = self
            .completed_blocks_for_screen(screen)
            .last()
            .map(|block| block.id)?;
        self.selected_block_id = Some(id);
        self.selected_completed_block()
    }

    pub fn select_previous_completed_block_for_screen(
        &mut self,
        screen: TerminalScreen,
    ) -> Option<&TerminalCommandBlock> {
        if let Some(id) = self.previous_completed_block_id_for_screen(screen) {
            self.selected_block_id = Some(id);
        }
        self.selected_completed_block()
    }

    pub fn select_next_completed_block_for_screen(
        &mut self,
        screen: TerminalScreen,
    ) -> Option<&TerminalCommandBlock> {
        if let Some(id) = self.next_completed_block_id_for_screen(screen) {
            self.selected_block_id = Some(id);
        }
        self.selected_completed_block()
    }

    pub fn clear_selection(&mut self) {
        self.selected_block_id = None;
    }

    pub fn clear(&mut self) {
        self.pending = None;
        self.completed.clear();
        self.selected_block_id = None;
    }

    fn start_prompt(&mut self, event: TerminalShellIntegrationEvent, observed_at_ms: Option<u64>) {
        let current_directory = self.current_directory_for_screen(event.screen).cloned();
        let block = PendingCommandBlock {
            id: self.allocate_block_id(),
            screen: event.screen,
            current_directory,
            prompt_start: event.point,
            prompt_anchor: event.anchor,
            command_start: None,
            command_anchor: None,
            output_start: None,
            output_anchor: None,
            started_at_ms: observed_at_ms,
        };
        self.pending = Some(block);
    }

    fn mark_command_start(
        &mut self,
        event: TerminalShellIntegrationEvent,
        observed_at_ms: Option<u64>,
    ) {
        let pending = self.pending_for_event(event, observed_at_ms);
        pending.command_start = Some(event.point);
        pending.command_anchor = event.anchor;
        if observed_at_ms.is_some() {
            pending.started_at_ms = observed_at_ms;
        }
    }

    fn mark_output_start(
        &mut self,
        event: TerminalShellIntegrationEvent,
        observed_at_ms: Option<u64>,
    ) {
        let pending = self.pending_for_event(event, observed_at_ms);
        if pending.command_start.is_none() {
            pending.command_start = Some(event.point);
            pending.command_anchor = event.anchor;
            if observed_at_ms.is_some() {
                pending.started_at_ms = observed_at_ms;
            }
        }
        pending.output_start = Some(event.point);
        pending.output_anchor = event.anchor;
    }

    fn finish_command(
        &mut self,
        event: TerminalShellIntegrationEvent,
        observed_at_ms: Option<u64>,
    ) {
        let fallback_current_directory = self.current_directory_for_screen(event.screen).cloned();
        let pending = self.pending.take().unwrap_or_else(|| PendingCommandBlock {
            id: self.allocate_block_id(),
            screen: event.screen,
            current_directory: fallback_current_directory.clone(),
            prompt_start: event.point,
            prompt_anchor: event.anchor,
            command_start: None,
            command_anchor: None,
            output_start: None,
            output_anchor: None,
            started_at_ms: observed_at_ms,
        });
        let started_at_ms = pending.started_at_ms.or(observed_at_ms);
        let finished_at_ms = observed_at_ms;
        let duration_ms = started_at_ms
            .zip(finished_at_ms)
            .map(|(started, finished)| finished.saturating_sub(started));
        self.completed.push(TerminalCommandBlock {
            id: pending.id,
            screen: pending.screen,
            current_directory: pending.current_directory.or(fallback_current_directory),
            prompt_start: pending.prompt_start,
            prompt_anchor: pending.prompt_anchor,
            command_start: pending.command_start,
            command_anchor: pending.command_anchor,
            output_start: pending.output_start,
            output_anchor: pending.output_anchor,
            finished_at: event.point,
            finished_anchor: event.anchor,
            exit_code: event.exit_code,
            started_at_ms,
            finished_at_ms,
            duration_ms,
            folded: false,
        });
    }

    fn pending_for_event(
        &mut self,
        event: TerminalShellIntegrationEvent,
        observed_at_ms: Option<u64>,
    ) -> &mut PendingCommandBlock {
        if self.pending.is_none() {
            let id = self.allocate_block_id();
            let current_directory = self.current_directory_for_screen(event.screen).cloned();
            self.pending = Some(PendingCommandBlock {
                id,
                screen: event.screen,
                current_directory,
                prompt_start: event.point,
                prompt_anchor: event.anchor,
                command_start: None,
                command_anchor: None,
                output_start: None,
                output_anchor: None,
                started_at_ms: observed_at_ms,
            });
        }
        self.pending
            .as_mut()
            .expect("pending block was just created")
    }

    fn allocate_block_id(&mut self) -> u64 {
        let id = self.next_block_id;
        self.next_block_id = self.next_block_id.saturating_add(1);
        id
    }

    fn previous_completed_block_id_for_screen(&self, screen: TerminalScreen) -> Option<u64> {
        let Some(selected_id) = self.selected_block_id else {
            return self
                .completed_blocks_for_screen(screen)
                .last()
                .map(|block| block.id);
        };
        let Some(selected_index) = self.completed_index_by_id(selected_id) else {
            return self
                .completed_blocks_for_screen(screen)
                .last()
                .map(|block| block.id);
        };
        if self.completed[selected_index].screen != screen {
            return self
                .completed_blocks_for_screen(screen)
                .last()
                .map(|block| block.id);
        }

        self.completed[..selected_index]
            .iter()
            .rev()
            .find(|block| block.screen == screen)
            .map(|block| block.id)
    }

    fn next_completed_block_id_for_screen(&self, screen: TerminalScreen) -> Option<u64> {
        let Some(selected_id) = self.selected_block_id else {
            return self
                .completed_blocks_for_screen(screen)
                .next()
                .map(|block| block.id);
        };
        let Some(selected_index) = self.completed_index_by_id(selected_id) else {
            return self
                .completed_blocks_for_screen(screen)
                .next()
                .map(|block| block.id);
        };
        if self.completed[selected_index].screen != screen {
            return self
                .completed_blocks_for_screen(screen)
                .next()
                .map(|block| block.id);
        }

        self.completed[selected_index.saturating_add(1)..]
            .iter()
            .find(|block| block.screen == screen)
            .map(|block| block.id)
    }

    fn completed_index_by_id(&self, id: u64) -> Option<usize> {
        self.completed.iter().position(|block| block.id == id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PendingCommandBlock {
    pub id: u64,
    pub screen: TerminalScreen,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_directory: Option<TerminalCurrentDirectory>,
    pub prompt_start: CellPoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_anchor: Option<TerminalPointAnchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_start: Option<CellPoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_anchor: Option<TerminalPointAnchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_start: Option<CellPoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_anchor: Option<TerminalPointAnchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalCommandBlock {
    pub id: u64,
    pub screen: TerminalScreen,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_directory: Option<TerminalCurrentDirectory>,
    pub prompt_start: CellPoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_anchor: Option<TerminalPointAnchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_start: Option<CellPoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_anchor: Option<TerminalPointAnchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_start: Option<CellPoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_anchor: Option<TerminalPointAnchor>,
    pub finished_at: CellPoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_anchor: Option<TerminalPointAnchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub folded: bool,
}

fn plugin_command_block_from_terminal_block(block: &TerminalCommandBlock) -> PluginCommandBlock {
    let ranges = block.text_ranges();
    PluginCommandBlock {
        id: block.id,
        command_range: plugin_command_block_text_range_from_terminal_range(&ranges.command),
        output_range: ranges
            .output
            .as_ref()
            .map(plugin_command_block_text_range_from_terminal_range),
        exit_code: block.exit_code,
        started_at_ms: block.started_at_ms,
        finished_at_ms: block.finished_at_ms,
        duration_ms: block.duration_ms,
        current_directory: block
            .current_directory
            .as_ref()
            .map(PluginCurrentDirectory::from),
    }
}

fn plugin_command_block_text_range_from_terminal_range(
    range: &TerminalCommandBlockTextRange,
) -> PluginCommandBlockTextRange {
    PluginCommandBlockTextRange {
        start: range.start,
        end_exclusive: range.end_exclusive,
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl TerminalCommandBlock {
    pub fn start_row(&self) -> u16 {
        self.points()
            .into_iter()
            .map(|point| point.row)
            .min()
            .unwrap_or(self.prompt_start.row)
    }

    pub fn end_row(&self) -> u16 {
        self.points()
            .into_iter()
            .map(|point| point.row)
            .max()
            .unwrap_or(self.finished_at.row)
    }

    pub fn row_span(&self) -> TerminalCommandBlockRowSpan {
        TerminalCommandBlockRowSpan {
            id: self.id,
            screen: self.screen,
            start_row: self.start_row(),
            end_row: self.end_row(),
            exit_code: self.exit_code,
        }
    }

    pub fn chrome_row_span(&self) -> TerminalCommandBlockRowSpan {
        self.chrome_row_span_for_span(self.row_span())
    }

    pub fn intersects_rows(&self, start_row: u16, end_row_exclusive: u16) -> bool {
        if start_row >= end_row_exclusive {
            return false;
        }
        let end_row = end_row_exclusive.saturating_sub(1);
        self.start_row() <= end_row && self.end_row() >= start_row
    }

    pub fn anchor_row_span(&self) -> Option<TerminalCommandBlockAnchorRowSpan> {
        let mut rows = self.anchor_points().into_iter().filter_map(|anchor| {
            let anchor = anchor?;
            (anchor.row.screen == self.screen).then_some(anchor.row.row)
        });
        let first = rows.next()?;
        let (start_anchor, end_anchor) = rows.fold((first, first), |(start, end), row| {
            (start.min(row), end.max(row))
        });
        Some(TerminalCommandBlockAnchorRowSpan {
            id: self.id,
            screen: self.screen,
            start_anchor,
            end_anchor,
            exit_code: self.exit_code,
        })
    }

    pub fn visible_row_span(
        &self,
        visible_row_anchors: &[TerminalVisibleRowAnchor],
    ) -> Option<TerminalCommandBlockRowSpan> {
        let Some(anchor_span) = self.anchor_row_span() else {
            return Some(self.row_span());
        };

        let mut visible_rows = visible_row_anchors
            .iter()
            .filter(|visible| {
                visible.anchor.screen == anchor_span.screen
                    && visible.anchor.row >= anchor_span.start_anchor
                    && visible.anchor.row <= anchor_span.end_anchor
            })
            .map(|visible| visible.visible_row);
        let first = visible_rows.next()?;
        let (start_row, end_row) = visible_rows.fold((first, first), |(start, end), row| {
            (start.min(row), end.max(row))
        });
        Some(TerminalCommandBlockRowSpan {
            id: self.id,
            screen: self.screen,
            start_row,
            end_row,
            exit_code: self.exit_code,
        })
    }

    pub fn visible_chrome_row_span(
        &self,
        visible_row_anchors: &[TerminalVisibleRowAnchor],
    ) -> Option<TerminalCommandBlockRowSpan> {
        self.visible_row_span(visible_row_anchors)
            .map(|span| self.chrome_row_span_for_span(span))
    }

    pub fn folded_hidden_visible_row_span(
        &self,
        visible_row_anchors: &[TerminalVisibleRowAnchor],
    ) -> Option<TerminalCommandBlockFoldedHiddenRowSpan> {
        if !self.folded {
            return None;
        }
        let span = self.visible_row_span(visible_row_anchors)?;
        if span.start_row >= span.end_row {
            return None;
        }

        Some(TerminalCommandBlockFoldedHiddenRowSpan {
            id: self.id,
            screen: self.screen,
            summary_row: span.start_row,
            hidden_start_row: span.start_row.saturating_add(1),
            hidden_end_row: span.end_row,
            exit_code: self.exit_code,
        })
    }

    pub fn text_ranges(&self) -> TerminalCommandBlockTextRanges {
        let command_start = self.command_start.unwrap_or(self.prompt_start);
        let command_start_anchor = self.command_anchor.or(self.prompt_anchor);
        let command_end = self.output_start.unwrap_or(self.finished_at);
        let command_end_anchor = self.output_anchor.or(self.finished_anchor);
        let command = TerminalCommandBlockTextRange {
            start: command_start,
            end_exclusive: command_end,
            start_anchor: command_start_anchor,
            end_exclusive_anchor: command_end_anchor,
        };
        let output = self
            .output_start
            .map(|output_start| TerminalCommandBlockTextRange {
                start: output_start,
                end_exclusive: self.finished_at,
                start_anchor: self.output_anchor,
                end_exclusive_anchor: self.finished_anchor,
            });

        TerminalCommandBlockTextRanges {
            id: self.id,
            screen: self.screen,
            command,
            output,
            exit_code: self.exit_code,
        }
    }

    fn points(&self) -> [CellPoint; 4] {
        [
            self.prompt_start,
            self.command_start.unwrap_or(self.prompt_start),
            self.output_start.unwrap_or(self.prompt_start),
            self.finished_at,
        ]
    }

    fn anchor_points(&self) -> [Option<TerminalPointAnchor>; 4] {
        [
            self.prompt_anchor,
            self.command_anchor,
            self.output_anchor,
            self.finished_anchor,
        ]
    }

    fn chrome_row_span_for_span(
        &self,
        span: TerminalCommandBlockRowSpan,
    ) -> TerminalCommandBlockRowSpan {
        if self.folded && span.start_row < span.end_row {
            TerminalCommandBlockRowSpan {
                end_row: span.start_row,
                ..span
            }
        } else {
            span
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalCommandBlockTextRanges {
    pub id: u64,
    pub screen: TerminalScreen,
    pub command: TerminalCommandBlockTextRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<TerminalCommandBlockTextRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

impl TerminalCommandBlockTextRanges {
    pub fn command_text_range(&self) -> TerminalTextRange {
        self.command.to_terminal_text_range(self.screen)
    }

    pub fn output_text_range(&self) -> Option<TerminalTextRange> {
        self.output
            .as_ref()
            .map(|range| range.to_terminal_text_range(self.screen))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalCommandBlockTextRange {
    pub start: CellPoint,
    pub end_exclusive: CellPoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_anchor: Option<TerminalPointAnchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_exclusive_anchor: Option<TerminalPointAnchor>,
}

impl TerminalCommandBlockTextRange {
    pub fn to_terminal_text_range(&self, screen: TerminalScreen) -> TerminalTextRange {
        TerminalTextRange {
            screen,
            start: self.start,
            end_exclusive: self.end_exclusive,
            start_anchor: self.start_anchor,
            end_exclusive_anchor: self.end_exclusive_anchor,
        }
    }
}

fn cell_origin(point: CellPoint, metrics: CellMetrics) -> PixelPoint {
    PixelPoint {
        x: metrics.padding.x + f32::from(point.col) * metrics.cell.width,
        y: metrics.padding.y + f32::from(point.row) * metrics.cell.height,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalCommandBlockRowSpan {
    pub id: u64,
    pub screen: TerminalScreen,
    pub start_row: u16,
    pub end_row: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalCommandBlockAnchorRowSpan {
    pub id: u64,
    pub screen: TerminalScreen,
    pub start_anchor: u64,
    pub end_anchor: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalCommandBlockFoldedHiddenRowSpan {
    pub id: u64,
    pub screen: TerminalScreen,
    pub summary_row: u16,
    pub hidden_start_row: u16,
    pub hidden_end_row: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

impl TerminalCommandBlockFoldedHiddenRowSpan {
    pub fn hidden_row_count(&self) -> u16 {
        self.hidden_end_row
            .saturating_sub(self.hidden_start_row)
            .saturating_add(1)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalCommandBlockFoldedCompactVisualRow {
    pub screen: TerminalScreen,
    pub visible_row: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_row: Option<u16>,
    pub hidden: bool,
    pub hidden_rows_before: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden_by_block_id: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalCommandBlockGutterHit {
    pub id: u64,
    pub screen: TerminalScreen,
    pub visible_row: u16,
    pub start_row: u16,
    pub end_row: u16,
    pub selected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(
        marker: TerminalShellIntegrationMarker,
        row: u16,
        col: u16,
        exit_code: Option<i32>,
    ) -> TerminalShellIntegrationEvent {
        event_on_screen(marker, TerminalScreen::Main, row, col, exit_code)
    }

    fn event_on_screen(
        marker: TerminalShellIntegrationMarker,
        screen: TerminalScreen,
        row: u16,
        col: u16,
        exit_code: Option<i32>,
    ) -> TerminalShellIntegrationEvent {
        TerminalShellIntegrationEvent {
            marker,
            screen,
            point: CellPoint::new(row, col),
            anchor: None,
            exit_code,
        }
    }

    fn event_with_anchor(
        marker: TerminalShellIntegrationMarker,
        screen: TerminalScreen,
        row: u16,
        col: u16,
        anchor_row: u64,
        exit_code: Option<i32>,
    ) -> TerminalShellIntegrationEvent {
        TerminalShellIntegrationEvent {
            marker,
            screen,
            point: CellPoint::new(row, col),
            anchor: Some(point_anchor(screen, anchor_row, col)),
            exit_code,
        }
    }

    fn point_anchor(screen: TerminalScreen, row: u64, col: u16) -> TerminalPointAnchor {
        TerminalPointAnchor {
            row: witty_core::TerminalRowAnchor { screen, row },
            col,
        }
    }

    fn visible_anchor(
        screen: TerminalScreen,
        row: u64,
        visible_row: u16,
    ) -> TerminalVisibleRowAnchor {
        TerminalVisibleRowAnchor {
            visible_row,
            anchor: witty_core::TerminalRowAnchor { screen, row },
        }
    }

    fn current_directory(path: &str) -> TerminalCurrentDirectory {
        TerminalCurrentDirectory {
            uri: format!("file://localhost{path}"),
            host: Some("localhost".to_owned()),
            path: path.to_owned(),
            screen: TerminalScreen::Main,
            point: CellPoint::new(0, 0),
            anchor: Some(point_anchor(TerminalScreen::Main, 0, 0)),
        }
    }

    #[test]
    fn current_directory_tracks_latest_osc7_action() {
        let mut state = ShellIntegrationState::default();
        let first = current_directory("/home/mingxu/one");
        let second = current_directory("/home/mingxu/two");

        state.apply_current_directory(first);
        state.apply_current_directory(second.clone());

        assert_eq!(state.current_directory(), Some(&second));
    }

    #[test]
    fn current_directory_is_attached_to_command_blocks() {
        let mut state = ShellIntegrationState::default();

        state.apply_current_directory(current_directory("/home/mingxu/project"));
        state.apply_event(event(
            TerminalShellIntegrationMarker::PromptStart,
            0,
            0,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandStart,
            0,
            2,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandFinished,
            1,
            0,
            Some(0),
        ));

        assert_eq!(
            state.completed_blocks()[0]
                .current_directory
                .as_ref()
                .map(|directory| directory.path.as_str()),
            Some("/home/mingxu/project")
        );
    }

    #[test]
    fn current_directory_updates_pending_command_block() {
        let mut state = ShellIntegrationState::default();

        state.apply_event(event(
            TerminalShellIntegrationMarker::PromptStart,
            0,
            0,
            None,
        ));
        state.apply_current_directory(current_directory("/home/mingxu/later"));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandFinished,
            1,
            0,
            Some(0),
        ));

        assert_eq!(
            state.completed_blocks()[0]
                .current_directory
                .as_ref()
                .map(|directory| directory.path.as_str()),
            Some("/home/mingxu/later")
        );
    }

    #[test]
    fn command_invocation_context_includes_selected_command_block_metadata() {
        let mut state = ShellIntegrationState::default();

        state.apply_current_directory(current_directory("/home/mingxu/block"));
        state.apply_event_at_ms(
            event(TerminalShellIntegrationMarker::PromptStart, 0, 0, None),
            10,
        );
        state.apply_event_at_ms(
            event(TerminalShellIntegrationMarker::CommandStart, 0, 2, None),
            100,
        );
        state.apply_event_at_ms(
            event(TerminalShellIntegrationMarker::OutputStart, 0, 8, None),
            125,
        );
        state.apply_event_at_ms(
            event(
                TerminalShellIntegrationMarker::CommandFinished,
                2,
                0,
                Some(2),
            ),
            350,
        );
        state.select_latest_completed_block_for_screen(TerminalScreen::Main);
        state.apply_current_directory(current_directory("/home/mingxu/latest"));

        let context = state.command_invocation_context_for_screen(TerminalScreen::Main);
        assert_eq!(
            context
                .current_directory
                .as_ref()
                .map(|directory| directory.path.as_str()),
            Some("/home/mingxu/latest")
        );
        let block = context
            .selected_command_block
            .expect("selected block should be available");
        assert_eq!(block.id, 0);
        assert_eq!(block.command_range.start, CellPoint::new(0, 2));
        assert_eq!(block.command_range.end_exclusive, CellPoint::new(0, 8));
        assert_eq!(
            block
                .output_range
                .as_ref()
                .map(|range| (range.start, range.end_exclusive)),
            Some((CellPoint::new(0, 8), CellPoint::new(2, 0)))
        );
        assert_eq!(block.exit_code, Some(2));
        assert_eq!(block.started_at_ms, Some(100));
        assert_eq!(block.finished_at_ms, Some(350));
        assert_eq!(block.duration_ms, Some(250));
        assert_eq!(
            block
                .current_directory
                .as_ref()
                .map(|directory| directory.path.as_str()),
            Some("/home/mingxu/block")
        );

        let alternate_context =
            state.command_invocation_context_for_screen(TerminalScreen::Alternate);
        assert!(alternate_context.current_directory.is_none());
        assert!(alternate_context.selected_command_block.is_none());
    }

    #[test]
    fn osc133_events_build_completed_command_block() {
        let mut state = ShellIntegrationState::default();

        state.apply_event(event(
            TerminalShellIntegrationMarker::PromptStart,
            0,
            0,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandStart,
            0,
            2,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::OutputStart,
            0,
            9,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandFinished,
            2,
            4,
            Some(7),
        ));

        assert_eq!(state.pending_block(), None);
        assert_eq!(
            state.completed_blocks(),
            &[TerminalCommandBlock {
                id: 0,
                screen: TerminalScreen::Main,
                current_directory: None,
                prompt_start: CellPoint::new(0, 0),
                prompt_anchor: None,
                command_start: Some(CellPoint::new(0, 2)),
                command_anchor: None,
                output_start: Some(CellPoint::new(0, 9)),
                output_anchor: None,
                started_at_ms: None,
                finished_at: CellPoint::new(2, 4),
                finished_anchor: None,
                exit_code: Some(7),
                finished_at_ms: None,
                duration_ms: None,
                folded: false,
            }]
        );
    }

    #[test]
    fn osc133_timing_metadata_tracks_command_duration() {
        let mut state = ShellIntegrationState::default();

        state.apply_event_at_ms(
            event(TerminalShellIntegrationMarker::PromptStart, 0, 0, None),
            10,
        );
        state.apply_event_at_ms(
            event(TerminalShellIntegrationMarker::CommandStart, 0, 2, None),
            25,
        );
        state.apply_event_at_ms(
            event(TerminalShellIntegrationMarker::OutputStart, 0, 9, None),
            40,
        );
        state.apply_event_at_ms(
            event(
                TerminalShellIntegrationMarker::CommandFinished,
                2,
                4,
                Some(0),
            ),
            225,
        );

        let block = state.last_completed_block().expect("completed block");
        assert_eq!(block.started_at_ms, Some(25));
        assert_eq!(block.finished_at_ms, Some(225));
        assert_eq!(block.duration_ms, Some(200));
    }

    #[test]
    fn osc133_timing_metadata_falls_back_to_prompt_start() {
        let mut state = ShellIntegrationState::default();

        state.apply_event_at_ms(
            event(TerminalShellIntegrationMarker::PromptStart, 0, 0, None),
            10,
        );
        state.apply_event_at_ms(
            event(
                TerminalShellIntegrationMarker::CommandFinished,
                1,
                4,
                Some(0),
            ),
            30,
        );

        let block = state.last_completed_block().expect("completed block");
        assert_eq!(block.started_at_ms, Some(10));
        assert_eq!(block.finished_at_ms, Some(30));
        assert_eq!(block.duration_ms, Some(20));
    }

    #[test]
    fn command_start_without_prompt_creates_pending_block() {
        let mut state = ShellIntegrationState::default();

        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandStart,
            3,
            5,
            None,
        ));

        assert_eq!(
            state.pending_block(),
            Some(&PendingCommandBlock {
                id: 0,
                screen: TerminalScreen::Main,
                current_directory: None,
                prompt_start: CellPoint::new(3, 5),
                prompt_anchor: None,
                command_start: Some(CellPoint::new(3, 5)),
                command_anchor: None,
                output_start: None,
                output_anchor: None,
                started_at_ms: None,
            })
        );
    }

    #[test]
    fn command_finished_without_prompt_creates_completed_block() {
        let mut state = ShellIntegrationState::default();

        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandFinished,
            1,
            4,
            None,
        ));

        assert_eq!(
            state.completed_blocks(),
            &[TerminalCommandBlock {
                id: 0,
                screen: TerminalScreen::Main,
                current_directory: None,
                prompt_start: CellPoint::new(1, 4),
                prompt_anchor: None,
                command_start: None,
                command_anchor: None,
                output_start: None,
                output_anchor: None,
                started_at_ms: None,
                finished_at: CellPoint::new(1, 4),
                finished_anchor: None,
                exit_code: None,
                finished_at_ms: None,
                duration_ms: None,
                folded: false,
            }]
        );
    }

    #[test]
    fn command_block_row_span_uses_all_marker_rows() {
        let block = TerminalCommandBlock {
            id: 3,
            screen: TerminalScreen::Main,
            current_directory: None,
            prompt_start: CellPoint::new(5, 1),
            prompt_anchor: None,
            command_start: Some(CellPoint::new(5, 3)),
            command_anchor: None,
            output_start: Some(CellPoint::new(7, 0)),
            output_anchor: None,
            finished_at: CellPoint::new(6, 4),
            finished_anchor: None,
            exit_code: Some(2),
            started_at_ms: None,
            finished_at_ms: None,
            duration_ms: None,
            folded: false,
        };

        assert_eq!(block.start_row(), 5);
        assert_eq!(block.end_row(), 7);
        assert_eq!(
            block.row_span(),
            TerminalCommandBlockRowSpan {
                id: 3,
                screen: TerminalScreen::Main,
                start_row: 5,
                end_row: 7,
                exit_code: Some(2),
            }
        );
        assert!(block.intersects_rows(0, 6));
        assert!(block.intersects_rows(7, 8));
        assert!(!block.intersects_rows(0, 5));
        assert!(!block.intersects_rows(8, 9));
        assert!(!block.intersects_rows(5, 5));
    }

    #[test]
    fn command_block_anchor_span_maps_to_visible_viewport_rows() {
        let block = TerminalCommandBlock {
            id: 4,
            screen: TerminalScreen::Main,
            current_directory: None,
            prompt_start: CellPoint::new(0, 0),
            prompt_anchor: Some(point_anchor(TerminalScreen::Main, 40, 0)),
            command_start: Some(CellPoint::new(0, 2)),
            command_anchor: Some(point_anchor(TerminalScreen::Main, 40, 2)),
            output_start: Some(CellPoint::new(1, 0)),
            output_anchor: Some(point_anchor(TerminalScreen::Main, 41, 0)),
            finished_at: CellPoint::new(2, 0),
            finished_anchor: Some(point_anchor(TerminalScreen::Main, 42, 0)),
            exit_code: Some(0),
            started_at_ms: None,
            finished_at_ms: None,
            duration_ms: None,
            folded: false,
        };

        assert_eq!(
            block.anchor_row_span(),
            Some(TerminalCommandBlockAnchorRowSpan {
                id: 4,
                screen: TerminalScreen::Main,
                start_anchor: 40,
                end_anchor: 42,
                exit_code: Some(0),
            })
        );
        assert_eq!(
            block.visible_row_span(&[
                visible_anchor(TerminalScreen::Main, 39, 0),
                visible_anchor(TerminalScreen::Main, 40, 1),
                visible_anchor(TerminalScreen::Main, 41, 2),
            ]),
            Some(TerminalCommandBlockRowSpan {
                id: 4,
                screen: TerminalScreen::Main,
                start_row: 1,
                end_row: 2,
                exit_code: Some(0),
            })
        );
        assert_eq!(
            block.visible_row_span(&[visible_anchor(TerminalScreen::Main, 43, 0)]),
            None
        );
    }

    #[test]
    fn folded_command_block_hidden_span_keeps_first_visible_row() {
        let mut block = TerminalCommandBlock {
            id: 5,
            screen: TerminalScreen::Main,
            current_directory: None,
            prompt_start: CellPoint::new(0, 0),
            prompt_anchor: Some(point_anchor(TerminalScreen::Main, 40, 0)),
            command_start: Some(CellPoint::new(0, 2)),
            command_anchor: Some(point_anchor(TerminalScreen::Main, 40, 2)),
            output_start: Some(CellPoint::new(1, 0)),
            output_anchor: Some(point_anchor(TerminalScreen::Main, 41, 0)),
            finished_at: CellPoint::new(2, 0),
            finished_anchor: Some(point_anchor(TerminalScreen::Main, 42, 0)),
            exit_code: Some(0),
            started_at_ms: None,
            finished_at_ms: None,
            duration_ms: None,
            folded: false,
        };
        let visible = [
            visible_anchor(TerminalScreen::Main, 40, 0),
            visible_anchor(TerminalScreen::Main, 41, 1),
            visible_anchor(TerminalScreen::Main, 42, 2),
        ];

        assert_eq!(block.folded_hidden_visible_row_span(&visible), None);
        block.folded = true;
        assert_eq!(
            block.folded_hidden_visible_row_span(&visible),
            Some(TerminalCommandBlockFoldedHiddenRowSpan {
                id: 5,
                screen: TerminalScreen::Main,
                summary_row: 0,
                hidden_start_row: 1,
                hidden_end_row: 2,
                exit_code: Some(0),
            })
        );
        assert_eq!(
            block.folded_hidden_visible_row_span(&[visible_anchor(TerminalScreen::Main, 40, 0)]),
            None
        );
    }

    #[test]
    fn folded_command_block_chrome_span_keeps_summary_row_only() {
        let mut block = TerminalCommandBlock {
            id: 6,
            screen: TerminalScreen::Main,
            current_directory: None,
            prompt_start: CellPoint::new(0, 0),
            prompt_anchor: Some(point_anchor(TerminalScreen::Main, 40, 0)),
            command_start: Some(CellPoint::new(0, 2)),
            command_anchor: Some(point_anchor(TerminalScreen::Main, 40, 2)),
            output_start: Some(CellPoint::new(1, 0)),
            output_anchor: Some(point_anchor(TerminalScreen::Main, 41, 0)),
            finished_at: CellPoint::new(2, 0),
            finished_anchor: Some(point_anchor(TerminalScreen::Main, 42, 0)),
            exit_code: Some(0),
            started_at_ms: None,
            finished_at_ms: None,
            duration_ms: None,
            folded: false,
        };
        let visible = [
            visible_anchor(TerminalScreen::Main, 40, 0),
            visible_anchor(TerminalScreen::Main, 41, 1),
            visible_anchor(TerminalScreen::Main, 42, 2),
        ];

        assert_eq!(
            block.visible_chrome_row_span(&visible),
            Some(TerminalCommandBlockRowSpan {
                id: 6,
                screen: TerminalScreen::Main,
                start_row: 0,
                end_row: 2,
                exit_code: Some(0),
            })
        );

        block.folded = true;
        assert_eq!(
            block.chrome_row_span(),
            TerminalCommandBlockRowSpan {
                id: 6,
                screen: TerminalScreen::Main,
                start_row: 0,
                end_row: 0,
                exit_code: Some(0),
            }
        );
        assert_eq!(
            block.visible_chrome_row_span(&visible),
            Some(TerminalCommandBlockRowSpan {
                id: 6,
                screen: TerminalScreen::Main,
                start_row: 0,
                end_row: 0,
                exit_code: Some(0),
            })
        );
    }

    #[test]
    fn state_reports_folded_hidden_spans_for_visible_rows() {
        let mut state = ShellIntegrationState::default();

        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::PromptStart,
            TerminalScreen::Main,
            0,
            0,
            40,
            None,
        ));
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::CommandFinished,
            TerminalScreen::Main,
            2,
            0,
            42,
            Some(0),
        ));
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::PromptStart,
            TerminalScreen::Main,
            3,
            0,
            43,
            None,
        ));
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::CommandFinished,
            TerminalScreen::Main,
            3,
            3,
            43,
            Some(1),
        ));

        assert!(state.set_completed_block_folded(0, true));
        assert!(state.set_completed_block_folded(1, true));
        assert_eq!(
            state.folded_hidden_row_spans_intersecting_visible_rows(
                TerminalScreen::Main,
                &[
                    visible_anchor(TerminalScreen::Main, 40, 0),
                    visible_anchor(TerminalScreen::Main, 41, 1),
                    visible_anchor(TerminalScreen::Main, 42, 2),
                    visible_anchor(TerminalScreen::Main, 43, 3),
                ],
                4,
            ),
            vec![TerminalCommandBlockFoldedHiddenRowSpan {
                id: 0,
                screen: TerminalScreen::Main,
                summary_row: 0,
                hidden_start_row: 1,
                hidden_end_row: 2,
                exit_code: Some(0),
            }]
        );
    }

    #[test]
    fn state_reports_folded_compact_visual_rows() {
        let mut state = ShellIntegrationState::default();

        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::PromptStart,
            TerminalScreen::Main,
            0,
            0,
            40,
            None,
        ));
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::CommandFinished,
            TerminalScreen::Main,
            2,
            0,
            42,
            Some(0),
        ));
        assert!(state.set_completed_block_folded(0, true));

        let compact_rows = state.folded_compact_visual_rows(
            TerminalScreen::Main,
            &[
                visible_anchor(TerminalScreen::Main, 40, 0),
                visible_anchor(TerminalScreen::Main, 41, 1),
                visible_anchor(TerminalScreen::Main, 42, 2),
            ],
            5,
        );

        assert_eq!(
            compact_rows,
            vec![
                TerminalCommandBlockFoldedCompactVisualRow {
                    screen: TerminalScreen::Main,
                    visible_row: 0,
                    compact_row: Some(0),
                    hidden: false,
                    hidden_rows_before: 0,
                    hidden_by_block_id: None,
                },
                TerminalCommandBlockFoldedCompactVisualRow {
                    screen: TerminalScreen::Main,
                    visible_row: 1,
                    compact_row: None,
                    hidden: true,
                    hidden_rows_before: 0,
                    hidden_by_block_id: Some(0),
                },
                TerminalCommandBlockFoldedCompactVisualRow {
                    screen: TerminalScreen::Main,
                    visible_row: 2,
                    compact_row: None,
                    hidden: true,
                    hidden_rows_before: 1,
                    hidden_by_block_id: Some(0),
                },
                TerminalCommandBlockFoldedCompactVisualRow {
                    screen: TerminalScreen::Main,
                    visible_row: 3,
                    compact_row: Some(1),
                    hidden: false,
                    hidden_rows_before: 2,
                    hidden_by_block_id: None,
                },
                TerminalCommandBlockFoldedCompactVisualRow {
                    screen: TerminalScreen::Main,
                    visible_row: 4,
                    compact_row: Some(2),
                    hidden: false,
                    hidden_rows_before: 2,
                    hidden_by_block_id: None,
                },
            ]
        );

        assert!(state.set_completed_block_folded(0, false));
        assert_eq!(
            state
                .folded_compact_visual_rows(TerminalScreen::Main, &[], 3)
                .iter()
                .map(|row| row.compact_row)
                .collect::<Vec<_>>(),
            vec![Some(0), Some(1), Some(2)]
        );
    }

    #[test]
    fn folded_frame_remap_removes_hidden_rows_and_moves_following_primitives() {
        let mut state = ShellIntegrationState::default();

        state.apply_event(event(
            TerminalShellIntegrationMarker::PromptStart,
            0,
            0,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandFinished,
            2,
            0,
            Some(0),
        ));
        assert!(state.set_completed_block_folded(0, true));

        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        let row_size = PixelSize {
            width: 80.0,
            height: 20.0,
        };
        let mut frame = FramePlan::default();
        frame.glyphs.push(GlyphBatchItem {
            origin: cell_origin(CellPoint::new(0, 0), metrics),
            text: "summary".to_owned(),
            color: Rgba::WHITE,
            style_flags: CellFlags::default(),
        });
        frame.glyphs.push(GlyphBatchItem {
            origin: cell_origin(CellPoint::new(1, 0), metrics),
            text: "hidden".to_owned(),
            color: Rgba::WHITE,
            style_flags: CellFlags::default(),
        });
        frame.glyphs.push(GlyphBatchItem {
            origin: cell_origin(CellPoint::new(3, 0), metrics),
            text: "after".to_owned(),
            color: Rgba::WHITE,
            style_flags: CellFlags::default(),
        });
        frame.backgrounds.push(RectBatchItem {
            origin: cell_origin(CellPoint::new(1, 0), metrics),
            size: row_size,
            color: Rgba::rgb(10, 10, 10),
        });
        frame.backgrounds.push(RectBatchItem {
            origin: cell_origin(CellPoint::new(3, 0), metrics),
            size: row_size,
            color: Rgba::rgb(20, 20, 20),
        });
        frame.selection.push(RectBatchItem {
            origin: cell_origin(CellPoint::new(2, 0), metrics),
            size: row_size,
            color: Rgba::rgb(30, 30, 30),
        });
        frame.search_highlights.push(RectBatchItem {
            origin: cell_origin(CellPoint::new(4, 0), metrics),
            size: row_size,
            color: Rgba::rgb(40, 40, 40),
        });
        frame.hyperlink_hover.push(RectBatchItem {
            origin: cell_origin(CellPoint::new(1, 0), metrics),
            size: row_size,
            color: Rgba::rgb(50, 50, 50),
        });
        frame.hyperlink_underlines.push(RectBatchItem {
            origin: PixelPoint { x: 0.0, y: 98.0 },
            size: PixelSize {
                width: 80.0,
                height: 2.0,
            },
            color: Rgba::rgb(60, 60, 60),
        });
        frame.ime_preedit.push(RectBatchItem {
            origin: cell_origin(CellPoint::new(3, 0), metrics),
            size: row_size,
            color: Rgba::rgb(70, 70, 70),
        });
        frame.cursor = Some(RectBatchItem {
            origin: cell_origin(CellPoint::new(4, 0), metrics),
            size: metrics.cell,
            color: Rgba::WHITE,
        });

        let stats = apply_command_block_folded_frame_remap_with_anchors(
            &mut frame,
            &state,
            TerminalScreen::Main,
            &[],
            metrics,
            GridSize::new(5, 8),
        );

        assert_eq!(
            stats,
            TerminalCommandBlockFoldedFrameRemapStats {
                hidden_rows: 2,
                removed_glyphs: 1,
                remapped_glyphs: 1,
                removed_rects: 3,
                remapped_rects: 5,
            }
        );
        assert!(frame.glyphs.iter().any(
            |glyph| glyph.text == "summary" && glyph.origin == (PixelPoint { x: 0.0, y: 0.0 })
        ));
        assert!(
            frame
                .glyphs
                .iter()
                .any(|glyph| glyph.text == "after"
                    && glyph.origin == (PixelPoint { x: 0.0, y: 20.0 }))
        );
        assert!(!frame.glyphs.iter().any(|glyph| glyph.text == "hidden"));
        assert_eq!(frame.backgrounds.len(), 1);
        assert_eq!(frame.backgrounds[0].origin, PixelPoint { x: 0.0, y: 20.0 });
        assert!(frame.selection.is_empty());
        assert_eq!(
            frame.search_highlights[0].origin,
            PixelPoint { x: 0.0, y: 40.0 }
        );
        assert!(frame.hyperlink_hover.is_empty());
        assert_eq!(
            frame.hyperlink_underlines[0].origin,
            PixelPoint { x: 0.0, y: 58.0 }
        );
        assert_eq!(frame.ime_preedit[0].origin, PixelPoint { x: 0.0, y: 20.0 });
        assert_eq!(
            frame.cursor.as_ref().map(|cursor| cursor.origin),
            Some(PixelPoint { x: 0.0, y: 40.0 })
        );
    }

    #[test]
    fn folded_coordinate_remap_maps_compact_visual_points_back_to_terminal_points() {
        let mut state = ShellIntegrationState::default();

        state.apply_event(event(
            TerminalShellIntegrationMarker::PromptStart,
            0,
            0,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandFinished,
            2,
            0,
            Some(0),
        ));
        assert!(state.set_completed_block_folded(0, true));

        let grid_size = GridSize::new(5, 8);
        assert_eq!(
            command_block_folded_terminal_row_for_compact_visual_row_with_anchors(
                &state,
                TerminalScreen::Main,
                &[],
                0,
                grid_size,
            ),
            Some(0)
        );
        assert_eq!(
            command_block_folded_terminal_row_for_compact_visual_row_with_anchors(
                &state,
                TerminalScreen::Main,
                &[],
                1,
                grid_size,
            ),
            Some(3)
        );
        assert_eq!(
            command_block_folded_terminal_row_for_compact_visual_row_with_anchors(
                &state,
                TerminalScreen::Main,
                &[],
                2,
                grid_size,
            ),
            Some(4)
        );
        assert_eq!(
            command_block_folded_terminal_row_for_compact_visual_row_with_anchors(
                &state,
                TerminalScreen::Main,
                &[],
                3,
                grid_size,
            ),
            None
        );

        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        assert_eq!(
            command_block_folded_visual_pixel_to_terminal_pixel_with_anchors(
                &state,
                TerminalScreen::Main,
                &[],
                PixelPoint { x: 15.0, y: 25.0 },
                metrics,
                grid_size,
            ),
            Some(PixelPoint { x: 15.0, y: 65.0 })
        );
        assert_eq!(
            command_block_folded_visual_pixel_to_terminal_pixel_with_anchors(
                &state,
                TerminalScreen::Main,
                &[],
                PixelPoint { x: 15.0, y: 65.0 },
                metrics,
                grid_size,
            ),
            None
        );

        assert!(state.set_completed_block_folded(0, false));
        assert_eq!(
            command_block_folded_terminal_row_for_compact_visual_row_with_anchors(
                &state,
                TerminalScreen::Main,
                &[],
                2,
                grid_size,
            ),
            Some(2)
        );
        assert_eq!(
            command_block_folded_visual_pixel_to_terminal_pixel_with_anchors(
                &state,
                TerminalScreen::Main,
                &[],
                PixelPoint { x: 15.0, y: 45.0 },
                metrics,
                grid_size,
            ),
            Some(PixelPoint { x: 15.0, y: 45.0 })
        );
    }

    #[test]
    fn folded_command_block_row_mask_hides_hidden_row_glyphs() {
        let mut state = ShellIntegrationState::default();

        state.apply_event(event(
            TerminalShellIntegrationMarker::PromptStart,
            0,
            0,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandFinished,
            2,
            4,
            Some(0),
        ));
        assert!(state.set_completed_block_folded(0, true));

        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        let row_background = Rgba::rgb(12, 14, 16);
        let mut frame = FramePlan::default();
        frame.backgrounds.push(RectBatchItem {
            origin: cell_origin(CellPoint::new(1, 0), metrics),
            size: PixelSize {
                width: 80.0,
                height: 20.0,
            },
            color: row_background,
        });
        frame.glyphs.push(GlyphBatchItem {
            origin: cell_origin(CellPoint::new(0, 0), metrics),
            text: "summary".to_owned(),
            color: Rgba::WHITE,
            style_flags: CellFlags::default(),
        });
        frame.glyphs.push(GlyphBatchItem {
            origin: cell_origin(CellPoint::new(1, 2), metrics),
            text: "hidden-one".to_owned(),
            color: Rgba::WHITE,
            style_flags: CellFlags::default(),
        });
        frame.glyphs.push(GlyphBatchItem {
            origin: cell_origin(CellPoint::new(2, 0), metrics),
            text: "hidden-two".to_owned(),
            color: Rgba::WHITE,
            style_flags: CellFlags::default(),
        });
        frame.glyphs.push(GlyphBatchItem {
            origin: cell_origin(CellPoint::new(3, 0), metrics),
            text: "outside".to_owned(),
            color: Rgba::WHITE,
            style_flags: CellFlags::default(),
        });

        let rects = apply_command_block_folded_row_mask_with_anchors(
            &mut frame,
            &state,
            TerminalScreen::Main,
            &[],
            metrics,
            GridSize::new(4, 8),
        );

        assert_eq!(rects, 2);
        assert!(frame.glyphs.iter().any(|glyph| glyph.text == "summary"));
        assert!(frame.glyphs.iter().any(|glyph| glyph.text == "outside"));
        assert!(!frame.glyphs.iter().any(|glyph| glyph.text == "hidden-one"));
        assert!(!frame.glyphs.iter().any(|glyph| glyph.text == "hidden-two"));
        assert_eq!(frame.backgrounds.len(), 3);
        assert_eq!(frame.backgrounds[1].origin, PixelPoint { x: 0.0, y: 20.0 });
        assert_eq!(frame.backgrounds[1].color, row_background);
        assert_eq!(frame.backgrounds[2].origin, PixelPoint { x: 0.0, y: 40.0 });
        assert_eq!(
            frame.backgrounds[2].color,
            COMMAND_BLOCK_FOLDED_ROW_MASK_BACKGROUND
        );

        assert!(state.set_completed_block_folded(0, false));
        let mut unfolded_frame = FramePlan::default();
        unfolded_frame.glyphs.push(GlyphBatchItem {
            origin: cell_origin(CellPoint::new(1, 0), metrics),
            text: "visible".to_owned(),
            color: Rgba::WHITE,
            style_flags: CellFlags::default(),
        });
        assert_eq!(
            apply_command_block_folded_row_mask_with_anchors(
                &mut unfolded_frame,
                &state,
                TerminalScreen::Main,
                &[],
                metrics,
                GridSize::new(4, 8),
            ),
            0
        );
        assert!(unfolded_frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text == "visible"));
    }

    #[test]
    fn folded_command_block_chrome_overlays_use_summary_row_only() {
        let mut state = ShellIntegrationState::default();

        state.apply_event(event(
            TerminalShellIntegrationMarker::PromptStart,
            0,
            0,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::OutputStart,
            1,
            0,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandFinished,
            2,
            4,
            Some(0),
        ));
        assert!(state.set_completed_block_folded(0, true));

        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };

        state.select_completed_block(0);
        let mut selection_frame = FramePlan::default();
        let selection_rects = apply_command_block_selection_overlay(
            &mut selection_frame,
            &state,
            TerminalScreen::Main,
            metrics,
            GridSize::new(4, 8),
        );
        assert_eq!(selection_rects, 2);
        assert_eq!(selection_frame.backgrounds.len(), 2);
        assert_eq!(
            selection_frame.backgrounds[0].origin,
            PixelPoint { x: 0.0, y: 0.0 }
        );

        state.clear_selection();
        let mut gutter_frame = FramePlan::default();
        let gutter_rects = apply_command_block_gutter_overlay_with_anchors(
            &mut gutter_frame,
            &state,
            TerminalScreen::Main,
            &[],
            metrics,
            GridSize::new(4, 8),
        );
        assert_eq!(gutter_rects, 1);
        assert_eq!(
            gutter_frame.backgrounds[0].origin,
            PixelPoint { x: 0.0, y: 0.0 }
        );

        let mut hover_frame = FramePlan::default();
        let hover_rects = apply_command_block_gutter_hover_overlay_with_anchors(
            &mut hover_frame,
            &state,
            TerminalScreen::Main,
            &[],
            Some(0),
            metrics,
            GridSize::new(4, 8),
        );
        assert_eq!(hover_rects, 2);
        assert_eq!(
            hover_frame.backgrounds[0].origin,
            PixelPoint { x: 0.0, y: 0.0 }
        );
        assert_eq!(
            hover_frame.backgrounds[1].origin,
            PixelPoint { x: 0.0, y: 0.0 }
        );

        assert_eq!(
            command_block_gutter_hit_test_with_anchors(
                &state,
                TerminalScreen::Main,
                &[],
                PixelPoint { x: 5.0, y: 10.0 },
                metrics,
                GridSize::new(4, 8),
            ),
            Some(TerminalCommandBlockGutterHit {
                id: 0,
                screen: TerminalScreen::Main,
                visible_row: 0,
                start_row: 0,
                end_row: 0,
                selected: false,
                exit_code: Some(0),
            })
        );
        assert_eq!(
            command_block_gutter_hit_test_with_anchors(
                &state,
                TerminalScreen::Main,
                &[],
                PixelPoint { x: 5.0, y: 30.0 },
                metrics,
                GridSize::new(4, 8),
            ),
            None
        );
    }

    #[test]
    fn state_filters_completed_blocks_by_screen_and_rows() {
        let mut state = ShellIntegrationState::default();

        state.apply_event(event(
            TerminalShellIntegrationMarker::PromptStart,
            0,
            0,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandFinished,
            1,
            0,
            Some(0),
        ));
        state.apply_event(event_on_screen(
            TerminalShellIntegrationMarker::PromptStart,
            TerminalScreen::Alternate,
            2,
            0,
            None,
        ));
        state.apply_event(event_on_screen(
            TerminalShellIntegrationMarker::CommandFinished,
            TerminalScreen::Alternate,
            4,
            0,
            Some(1),
        ));

        assert_eq!(state.completed_len(), 2);
        assert_eq!(state.completed_len_for_screen(TerminalScreen::Main), 1);
        assert_eq!(state.completed_len_for_screen(TerminalScreen::Alternate), 1);
        assert_eq!(state.last_completed_block().map(|block| block.id), Some(1));
        assert_eq!(
            state.completed_block_by_id(0).map(|block| block.exit_code),
            Some(Some(0))
        );
        assert_eq!(state.completed_block_by_id(99), None);

        let main_blocks = state.completed_blocks_intersecting_rows(TerminalScreen::Main, 0, 2);
        assert_eq!(main_blocks.len(), 1);
        assert_eq!(main_blocks[0].id, 0);
        assert!(state
            .completed_blocks_intersecting_rows(TerminalScreen::Main, 2, 4)
            .is_empty());

        assert_eq!(
            state.completed_block_row_spans_intersecting_rows(TerminalScreen::Alternate, 3, 5,),
            vec![TerminalCommandBlockRowSpan {
                id: 1,
                screen: TerminalScreen::Alternate,
                start_row: 2,
                end_row: 4,
                exit_code: Some(1),
            }]
        );
    }

    #[test]
    fn state_navigates_completed_blocks_on_active_screen() {
        let mut state = ShellIntegrationState::default();

        for row in 0..3 {
            state.apply_event(event(
                TerminalShellIntegrationMarker::PromptStart,
                row,
                0,
                None,
            ));
            state.apply_event(event(
                TerminalShellIntegrationMarker::CommandFinished,
                row,
                4,
                Some(i32::from(row)),
            ));
        }

        assert_eq!(state.selected_block_id(), None);
        assert_eq!(
            state
                .select_previous_completed_block_for_screen(TerminalScreen::Main)
                .map(|block| block.id),
            Some(2)
        );
        assert_eq!(
            state
                .select_previous_completed_block_for_screen(TerminalScreen::Main)
                .map(|block| block.id),
            Some(1)
        );
        assert_eq!(
            state
                .select_previous_completed_block_for_screen(TerminalScreen::Main)
                .map(|block| block.id),
            Some(0)
        );
        assert_eq!(
            state
                .select_previous_completed_block_for_screen(TerminalScreen::Main)
                .map(|block| block.id),
            Some(0)
        );
        assert_eq!(
            state
                .select_next_completed_block_for_screen(TerminalScreen::Main)
                .map(|block| block.id),
            Some(1)
        );
        assert_eq!(
            state
                .select_latest_completed_block_for_screen(TerminalScreen::Main)
                .map(|block| block.id),
            Some(2)
        );
        assert_eq!(
            state.select_completed_block(1).map(|block| block.id),
            Some(1)
        );
        assert_eq!(
            state.select_completed_block(99).map(|block| block.id),
            Some(1)
        );

        state.clear_selection();
        assert_eq!(state.selected_completed_block(), None);
        assert_eq!(
            state
                .select_next_completed_block_for_screen(TerminalScreen::Main)
                .map(|block| block.id),
            Some(0)
        );

        state.clear();
        assert_eq!(state.selected_block_id(), None);
        assert!(state.completed_blocks().is_empty());
    }

    #[test]
    fn selected_command_block_text_ranges_split_command_and_output() {
        let mut state = ShellIntegrationState::default();
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::PromptStart,
            TerminalScreen::Main,
            0,
            0,
            40,
            None,
        ));
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::CommandStart,
            TerminalScreen::Main,
            0,
            2,
            40,
            None,
        ));
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::OutputStart,
            TerminalScreen::Main,
            0,
            9,
            40,
            None,
        ));
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::CommandFinished,
            TerminalScreen::Main,
            2,
            4,
            42,
            Some(0),
        ));
        state.select_latest_completed_block_for_screen(TerminalScreen::Main);

        let ranges = state.selected_command_block_text_ranges();
        assert_eq!(
            ranges,
            Some(TerminalCommandBlockTextRanges {
                id: 0,
                screen: TerminalScreen::Main,
                command: TerminalCommandBlockTextRange {
                    start: CellPoint::new(0, 2),
                    end_exclusive: CellPoint::new(0, 9),
                    start_anchor: Some(point_anchor(TerminalScreen::Main, 40, 2)),
                    end_exclusive_anchor: Some(point_anchor(TerminalScreen::Main, 40, 9)),
                },
                output: Some(TerminalCommandBlockTextRange {
                    start: CellPoint::new(0, 9),
                    end_exclusive: CellPoint::new(2, 4),
                    start_anchor: Some(point_anchor(TerminalScreen::Main, 40, 9)),
                    end_exclusive_anchor: Some(point_anchor(TerminalScreen::Main, 42, 4)),
                }),
                exit_code: Some(0),
            })
        );
        let ranges = ranges.unwrap();
        assert_eq!(
            ranges.command_text_range(),
            TerminalTextRange {
                screen: TerminalScreen::Main,
                start: CellPoint::new(0, 2),
                end_exclusive: CellPoint::new(0, 9),
                start_anchor: Some(point_anchor(TerminalScreen::Main, 40, 2)),
                end_exclusive_anchor: Some(point_anchor(TerminalScreen::Main, 40, 9)),
            }
        );
        assert_eq!(
            ranges.output_text_range(),
            Some(TerminalTextRange {
                screen: TerminalScreen::Main,
                start: CellPoint::new(0, 9),
                end_exclusive: CellPoint::new(2, 4),
                start_anchor: Some(point_anchor(TerminalScreen::Main, 40, 9)),
                end_exclusive_anchor: Some(point_anchor(TerminalScreen::Main, 42, 4)),
            })
        );
    }

    #[test]
    fn command_block_command_registrations_are_builtin_commands() {
        let commands = command_block_command_registrations();

        assert_eq!(commands.len(), 8);
        assert_eq!(commands[0].id, COMMAND_BLOCK_ACTION_MENU_COMMAND_ID);
        assert_eq!(commands[0].title, "Command Block: Actions");
        assert_eq!(commands[1].id, COMMAND_BLOCK_SELECT_LATEST_COMMAND_ID);
        assert_eq!(commands[1].title, "Command Block: Latest");
        assert_eq!(commands[5].id, COMMAND_BLOCK_COPY_OUTPUT_ID);
        assert_eq!(commands[6].id, COMMAND_BLOCK_COPY_COMMAND_ID);
        assert_eq!(commands[7].id, COMMAND_BLOCK_TOGGLE_FOLD_COMMAND_ID);
        assert_eq!(commands[7].title, "Command Block: Toggle Fold");
        assert!(commands
            .iter()
            .all(|command| command.source_plugin == "builtin"));
    }

    #[test]
    fn selected_command_block_copy_text_extracts_command_and_output() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 16));
        terminal.feed(b"$ echo\r\nok\r\n");

        let mut state = ShellIntegrationState::default();
        state.apply_event(event(
            TerminalShellIntegrationMarker::PromptStart,
            0,
            0,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandStart,
            0,
            1,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::OutputStart,
            0,
            6,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandFinished,
            2,
            0,
            Some(0),
        ));
        state.select_latest_completed_block_for_screen(TerminalScreen::Main);

        assert_eq!(
            selected_command_block_copy_text(&terminal, &state, CommandBlockCopyTarget::Command),
            Some(" echo".to_owned())
        );
        assert_eq!(
            selected_command_block_copy_text(&terminal, &state, CommandBlockCopyTarget::Output),
            Some("ok".to_owned())
        );
    }

    #[test]
    fn command_block_action_menu_tracks_selected_block_and_confirms_actions() {
        let mut state = ShellIntegrationState::default();
        state.apply_event(event(
            TerminalShellIntegrationMarker::PromptStart,
            0,
            0,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandFinished,
            1,
            2,
            Some(0),
        ));
        state.select_latest_completed_block_for_screen(TerminalScreen::Main);

        let mut menu = CommandBlockActionMenu::default();
        assert!(menu.open_for_selected_block(&state));
        assert!(menu.is_open());
        assert_eq!(menu.block_id(), Some(0));
        assert_eq!(
            menu.selected_command_id(),
            Some(COMMAND_BLOCK_COPY_OUTPUT_ID)
        );

        menu.move_selection(1);
        assert_eq!(menu.selected_index(), Some(1));
        assert_eq!(
            menu.selected_command_id(),
            Some(COMMAND_BLOCK_COPY_COMMAND_ID)
        );
        assert_eq!(menu.confirm(), Some(COMMAND_BLOCK_COPY_COMMAND_ID));
        assert!(!menu.is_open());
    }

    #[test]
    fn command_block_action_menu_overlay_renders_near_selected_block() {
        let mut state = ShellIntegrationState::default();
        state.apply_event(event(
            TerminalShellIntegrationMarker::PromptStart,
            1,
            0,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandFinished,
            2,
            4,
            Some(0),
        ));
        state.select_latest_completed_block_for_screen(TerminalScreen::Main);

        let mut menu = CommandBlockActionMenu::default();
        assert!(menu.open_for_selected_block(&state));
        let mut frame = FramePlan::default();
        frame.glyphs.push(GlyphBatchItem {
            origin: cell_origin(CellPoint::new(1, 1), CellMetrics::default()),
            text: "under".to_owned(),
            color: Rgba::rgb(255, 255, 255),
            style_flags: CellFlags::default(),
        });

        let rects = apply_command_block_action_menu_overlay(
            &mut frame,
            &menu,
            &state,
            TerminalScreen::Main,
            &[],
            CellMetrics::default(),
            GridSize::new(8, 40),
        );

        assert_eq!(rects, 2);
        assert_eq!(frame.backgrounds.len(), 2);
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text == "Block Actions"));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text == "> Copy Output"));
        assert!(!frame.glyphs.iter().any(|glyph| glyph.text == "under"));
    }

    #[test]
    fn command_block_status_label_formats_exit_status() {
        assert_eq!(command_block_status_label(Some(0)), "ok");
        assert_eq!(command_block_status_label(Some(127)), "exit 127");
        assert_eq!(command_block_status_label(None), "done");
    }

    #[test]
    fn command_block_status_label_formats_duration_metadata() {
        assert_eq!(
            command_block_status_label_with_duration(Some(0), Some(42)),
            "ok 42ms"
        );
        assert_eq!(
            command_block_status_label_with_duration(Some(0), Some(1_250)),
            "ok 1.3s"
        );
        assert_eq!(
            command_block_status_label_with_duration(Some(2), Some(12_400)),
            "exit 2 12s"
        );
        assert_eq!(
            command_block_status_label_with_duration(None, Some(65_000)),
            "done 1m05s"
        );
        assert_eq!(command_block_status_label_with_duration(None, None), "done");
        assert_eq!(
            command_block_status_label_with_duration_and_fold_state(Some(0), Some(1_250), true),
            "folded ok 1.3s"
        );
        assert_eq!(
            command_block_status_label_with_duration_and_folded_hidden_rows(
                Some(0),
                Some(1_250),
                Some(1),
            ),
            "folded 1 row ok 1.3s"
        );
        assert_eq!(
            command_block_status_label_with_duration_and_folded_hidden_rows(
                Some(2),
                Some(12_400),
                Some(3),
            ),
            "folded 3 rows exit 2 12s"
        );
    }

    #[test]
    fn command_block_status_label_overlay_renders_selected_and_hovered_blocks() {
        let mut state = ShellIntegrationState::default();
        state.apply_event(event(
            TerminalShellIntegrationMarker::PromptStart,
            0,
            0,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandFinished,
            1,
            4,
            Some(0),
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::PromptStart,
            3,
            0,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandFinished,
            4,
            4,
            Some(2),
        ));
        state.select_completed_block(0);

        let mut frame = FramePlan::default();
        frame.glyphs.push(GlyphBatchItem {
            origin: cell_origin(CellPoint::new(0, 21), CellMetrics::default()),
            text: "under".to_owned(),
            color: Rgba::rgb(255, 255, 255),
            style_flags: CellFlags::default(),
        });

        let rects = apply_command_block_status_label_overlay_with_anchors(
            &mut frame,
            &state,
            TerminalScreen::Main,
            &[],
            Some(1),
            CellMetrics::default(),
            GridSize::new(8, 24),
        );

        assert_eq!(rects, 2);
        assert_eq!(frame.backgrounds.len(), 2);
        assert!(frame.glyphs.iter().any(|glyph| glyph.text == "ok"));
        assert!(frame.glyphs.iter().any(|glyph| glyph.text == "exit 2"));
        assert!(!frame.glyphs.iter().any(|glyph| glyph.text == "under"));
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.color == COMMAND_BLOCK_STATUS_LABEL_FAILURE_TEXT));
    }

    #[test]
    fn command_block_status_label_overlay_includes_duration_when_available() {
        let mut state = ShellIntegrationState::default();
        state.apply_event_at_ms(
            event(TerminalShellIntegrationMarker::PromptStart, 0, 0, None),
            100,
        );
        state.apply_event_at_ms(
            event(TerminalShellIntegrationMarker::CommandStart, 0, 2, None),
            120,
        );
        state.apply_event_at_ms(
            event(
                TerminalShellIntegrationMarker::CommandFinished,
                1,
                4,
                Some(0),
            ),
            1_420,
        );
        state.select_completed_block(0);

        let mut frame = FramePlan::default();
        let rects = apply_command_block_status_label_overlay_with_anchors(
            &mut frame,
            &state,
            TerminalScreen::Main,
            &[],
            None,
            CellMetrics::default(),
            GridSize::new(4, 24),
        );

        assert_eq!(rects, 1);
        assert!(frame.glyphs.iter().any(|glyph| glyph.text == "ok 1.3s"));
    }

    #[test]
    fn command_block_status_label_overlay_marks_folded_blocks() {
        let mut state = ShellIntegrationState::default();
        state.apply_event(event(
            TerminalShellIntegrationMarker::PromptStart,
            0,
            0,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandFinished,
            1,
            4,
            Some(0),
        ));
        state.select_completed_block(0);
        assert!(state.set_completed_block_folded(0, true));

        let mut frame = FramePlan::default();
        let rects = apply_command_block_status_label_overlay_with_anchors(
            &mut frame,
            &state,
            TerminalScreen::Main,
            &[],
            None,
            CellMetrics::default(),
            GridSize::new(4, 24),
        );

        assert_eq!(rects, 1);
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text == "folded 1 row ok"));
    }

    #[test]
    fn command_block_commands_update_selection() {
        let mut state = ShellIntegrationState::default();
        for row in 0..2 {
            state.apply_event(event(
                TerminalShellIntegrationMarker::PromptStart,
                row,
                0,
                None,
            ));
            state.apply_event(event(
                TerminalShellIntegrationMarker::CommandFinished,
                row,
                2,
                None,
            ));
        }

        assert!(apply_command_block_command(
            &mut state,
            TerminalScreen::Main,
            COMMAND_BLOCK_SELECT_LATEST_COMMAND_ID,
        ));
        assert_eq!(state.selected_block_id(), Some(1));
        assert!(apply_command_block_command(
            &mut state,
            TerminalScreen::Main,
            COMMAND_BLOCK_SELECT_PREVIOUS_COMMAND_ID,
        ));
        assert_eq!(state.selected_block_id(), Some(0));
        assert!(apply_command_block_command(
            &mut state,
            TerminalScreen::Main,
            COMMAND_BLOCK_SELECT_NEXT_COMMAND_ID,
        ));
        assert_eq!(state.selected_block_id(), Some(1));
        assert!(apply_command_block_command(
            &mut state,
            TerminalScreen::Main,
            COMMAND_BLOCK_TOGGLE_FOLD_COMMAND_ID,
        ));
        assert!(state.is_completed_block_folded(1));
        assert!(apply_command_block_command(
            &mut state,
            TerminalScreen::Main,
            COMMAND_BLOCK_TOGGLE_FOLD_COMMAND_ID,
        ));
        assert!(!state.is_completed_block_folded(1));
        assert!(apply_command_block_command(
            &mut state,
            TerminalScreen::Main,
            COMMAND_BLOCK_CLEAR_SELECTION_COMMAND_ID,
        ));
        assert_eq!(state.selected_block_id(), None);
        assert!(!apply_command_block_command(
            &mut state,
            TerminalScreen::Main,
            COMMAND_BLOCK_TOGGLE_FOLD_COMMAND_ID,
        ));
        assert!(!apply_command_block_command(
            &mut state,
            TerminalScreen::Main,
            "witty.unknown",
        ));
    }

    #[test]
    fn selected_command_block_overlay_marks_visible_rows() {
        let mut state = ShellIntegrationState::default();
        state.apply_event(event(
            TerminalShellIntegrationMarker::PromptStart,
            0,
            0,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandStart,
            0,
            2,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::OutputStart,
            1,
            0,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandFinished,
            2,
            4,
            Some(0),
        ));
        state.select_latest_completed_block_for_screen(TerminalScreen::Main);

        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        let mut frame = FramePlan::default();
        let rects = apply_command_block_selection_overlay(
            &mut frame,
            &state,
            TerminalScreen::Main,
            metrics,
            GridSize::new(2, 8),
        );

        assert_eq!(rects, 4);
        assert_eq!(frame.backgrounds.len(), 4);
        assert_eq!(frame.backgrounds[0].origin, PixelPoint { x: 0.0, y: 0.0 });
        assert_eq!(
            frame.backgrounds[0].size,
            PixelSize {
                width: 80.0,
                height: 20.0,
            }
        );
        assert_eq!(
            frame.backgrounds[0].color,
            SELECTED_COMMAND_BLOCK_BACKGROUND
        );
        assert_eq!(frame.backgrounds[1].origin, PixelPoint { x: 0.0, y: 0.0 });
        assert_eq!(
            frame.backgrounds[1].size,
            PixelSize {
                width: 3.0,
                height: 20.0,
            }
        );
        assert_eq!(frame.backgrounds[1].color, SELECTED_COMMAND_BLOCK_GUTTER);
        assert_eq!(frame.backgrounds[2].origin, PixelPoint { x: 0.0, y: 20.0 });

        let rects_for_alternate = apply_command_block_selection_overlay(
            &mut frame,
            &state,
            TerminalScreen::Alternate,
            metrics,
            GridSize::new(2, 8),
        );
        assert_eq!(rects_for_alternate, 0);
        assert_eq!(frame.backgrounds.len(), 4);
    }

    #[test]
    fn selected_command_block_overlay_with_anchors_uses_viewport_rows() {
        let mut state = ShellIntegrationState::default();
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::PromptStart,
            TerminalScreen::Main,
            0,
            0,
            40,
            None,
        ));
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::OutputStart,
            TerminalScreen::Main,
            1,
            0,
            41,
            None,
        ));
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::CommandFinished,
            TerminalScreen::Main,
            2,
            4,
            42,
            Some(0),
        ));
        state.select_latest_completed_block_for_screen(TerminalScreen::Main);

        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        let mut frame = FramePlan::default();
        let rects = apply_command_block_selection_overlay_with_anchors(
            &mut frame,
            &state,
            TerminalScreen::Main,
            &[
                visible_anchor(TerminalScreen::Main, 39, 0),
                visible_anchor(TerminalScreen::Main, 40, 1),
                visible_anchor(TerminalScreen::Main, 41, 2),
            ],
            metrics,
            GridSize::new(3, 8),
        );

        assert_eq!(rects, 4);
        assert_eq!(frame.backgrounds[0].origin, PixelPoint { x: 0.0, y: 20.0 });
        assert_eq!(frame.backgrounds[2].origin, PixelPoint { x: 0.0, y: 40.0 });
    }

    #[test]
    fn command_block_gutter_overlay_marks_visible_unselected_blocks() {
        let mut state = ShellIntegrationState::default();
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::PromptStart,
            TerminalScreen::Main,
            0,
            0,
            30,
            None,
        ));
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::CommandFinished,
            TerminalScreen::Main,
            1,
            4,
            31,
            Some(0),
        ));
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::PromptStart,
            TerminalScreen::Main,
            2,
            0,
            40,
            None,
        ));
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::OutputStart,
            TerminalScreen::Main,
            3,
            0,
            41,
            None,
        ));
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::CommandFinished,
            TerminalScreen::Main,
            4,
            3,
            42,
            Some(2),
        ));
        state.select_latest_completed_block_for_screen(TerminalScreen::Main);

        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        let visible_row_anchors = [
            visible_anchor(TerminalScreen::Main, 30, 0),
            visible_anchor(TerminalScreen::Main, 31, 1),
            visible_anchor(TerminalScreen::Main, 40, 3),
            visible_anchor(TerminalScreen::Main, 41, 4),
            visible_anchor(TerminalScreen::Main, 42, 5),
        ];
        let mut frame = FramePlan::default();
        let rects = apply_command_block_gutter_overlay_with_anchors(
            &mut frame,
            &state,
            TerminalScreen::Main,
            &visible_row_anchors,
            metrics,
            GridSize::new(5, 8),
        );

        assert_eq!(rects, 2);
        assert_eq!(frame.backgrounds.len(), 2);
        assert_eq!(frame.backgrounds[0].origin, PixelPoint { x: 0.0, y: 0.0 });
        assert_eq!(
            frame.backgrounds[0].size,
            PixelSize {
                width: 2.0,
                height: 20.0,
            }
        );
        assert_eq!(frame.backgrounds[0].color, COMMAND_BLOCK_GUTTER_SUCCESS);
        assert_eq!(frame.backgrounds[1].color, COMMAND_BLOCK_GUTTER_SUCCESS);

        state.clear_selection();
        let mut frame = FramePlan::default();
        let rects = apply_command_block_gutter_overlay_with_anchors(
            &mut frame,
            &state,
            TerminalScreen::Main,
            &visible_row_anchors,
            metrics,
            GridSize::new(5, 8),
        );

        assert_eq!(rects, 4);
        assert_eq!(frame.backgrounds[0].color, COMMAND_BLOCK_GUTTER_SUCCESS);
        assert_eq!(frame.backgrounds[1].color, COMMAND_BLOCK_GUTTER_SUCCESS);
        assert_eq!(frame.backgrounds[2].origin, PixelPoint { x: 0.0, y: 60.0 });
        assert_eq!(frame.backgrounds[2].color, COMMAND_BLOCK_GUTTER_FAILURE);
        assert_eq!(frame.backgrounds[3].origin, PixelPoint { x: 0.0, y: 80.0 });
        assert_eq!(frame.backgrounds[3].color, COMMAND_BLOCK_GUTTER_FAILURE);
    }

    #[test]
    fn command_block_gutter_hover_overlay_marks_hovered_unselected_block() {
        let mut state = ShellIntegrationState::default();
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::PromptStart,
            TerminalScreen::Main,
            0,
            0,
            30,
            None,
        ));
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::CommandFinished,
            TerminalScreen::Main,
            1,
            4,
            31,
            Some(0),
        ));
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::PromptStart,
            TerminalScreen::Main,
            2,
            0,
            40,
            None,
        ));
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::CommandFinished,
            TerminalScreen::Main,
            3,
            4,
            41,
            Some(1),
        ));
        state.select_latest_completed_block_for_screen(TerminalScreen::Main);

        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        let visible_row_anchors = [
            visible_anchor(TerminalScreen::Main, 30, 0),
            visible_anchor(TerminalScreen::Main, 31, 1),
            visible_anchor(TerminalScreen::Main, 40, 3),
            visible_anchor(TerminalScreen::Main, 41, 4),
        ];
        let mut frame = FramePlan::default();
        let rects = apply_command_block_gutter_hover_overlay_with_anchors(
            &mut frame,
            &state,
            TerminalScreen::Main,
            &visible_row_anchors,
            Some(0),
            metrics,
            GridSize::new(5, 8),
        );

        assert_eq!(rects, 4);
        assert_eq!(frame.backgrounds.len(), 4);
        assert_eq!(frame.backgrounds[0].origin, PixelPoint { x: 0.0, y: 0.0 });
        assert_eq!(
            frame.backgrounds[0].size,
            PixelSize {
                width: 80.0,
                height: 20.0,
            }
        );
        assert_eq!(frame.backgrounds[0].color, HOVERED_COMMAND_BLOCK_BACKGROUND);
        assert_eq!(frame.backgrounds[1].color, HOVERED_COMMAND_BLOCK_GUTTER);
        assert_eq!(frame.backgrounds[2].origin, PixelPoint { x: 0.0, y: 20.0 });
        assert_eq!(frame.backgrounds[3].color, HOVERED_COMMAND_BLOCK_GUTTER);

        let mut selected_frame = FramePlan::default();
        let selected_rects = apply_command_block_gutter_hover_overlay_with_anchors(
            &mut selected_frame,
            &state,
            TerminalScreen::Main,
            &visible_row_anchors,
            Some(1),
            metrics,
            GridSize::new(5, 8),
        );
        assert_eq!(selected_rects, 0);
        assert!(selected_frame.backgrounds.is_empty());
    }

    #[test]
    fn command_block_gutter_hit_test_uses_anchors_and_selected_state() {
        let mut state = ShellIntegrationState::default();
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::PromptStart,
            TerminalScreen::Main,
            0,
            0,
            30,
            None,
        ));
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::CommandFinished,
            TerminalScreen::Main,
            1,
            4,
            31,
            Some(0),
        ));
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::PromptStart,
            TerminalScreen::Main,
            2,
            0,
            40,
            None,
        ));
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::OutputStart,
            TerminalScreen::Main,
            3,
            0,
            41,
            None,
        ));
        state.apply_event(event_with_anchor(
            TerminalShellIntegrationMarker::CommandFinished,
            TerminalScreen::Main,
            4,
            3,
            42,
            Some(2),
        ));
        state.select_latest_completed_block_for_screen(TerminalScreen::Main);

        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        let visible_row_anchors = [
            visible_anchor(TerminalScreen::Main, 30, 0),
            visible_anchor(TerminalScreen::Main, 31, 1),
            visible_anchor(TerminalScreen::Main, 40, 3),
            visible_anchor(TerminalScreen::Main, 41, 4),
            visible_anchor(TerminalScreen::Main, 42, 5),
        ];

        assert_eq!(
            command_block_gutter_hit_test_with_anchors(
                &state,
                TerminalScreen::Main,
                &visible_row_anchors,
                PixelPoint { x: 5.0, y: 10.0 },
                metrics,
                GridSize::new(6, 8),
            ),
            Some(TerminalCommandBlockGutterHit {
                id: 0,
                screen: TerminalScreen::Main,
                visible_row: 0,
                start_row: 0,
                end_row: 1,
                selected: false,
                exit_code: Some(0),
            })
        );
        assert_eq!(
            command_block_gutter_hit_test_with_anchors(
                &state,
                TerminalScreen::Main,
                &visible_row_anchors,
                PixelPoint { x: 5.0, y: 70.0 },
                metrics,
                GridSize::new(6, 8),
            ),
            Some(TerminalCommandBlockGutterHit {
                id: 1,
                screen: TerminalScreen::Main,
                visible_row: 3,
                start_row: 3,
                end_row: 5,
                selected: true,
                exit_code: Some(2),
            })
        );
        assert_eq!(
            command_block_gutter_hit_test_with_anchors(
                &state,
                TerminalScreen::Main,
                &visible_row_anchors,
                PixelPoint { x: 10.0, y: 10.0 },
                metrics,
                GridSize::new(6, 8),
            ),
            None
        );
        assert_eq!(
            command_block_gutter_hit_test_with_anchors(
                &state,
                TerminalScreen::Alternate,
                &visible_row_anchors,
                PixelPoint { x: 5.0, y: 10.0 },
                metrics,
                GridSize::new(6, 8),
            ),
            None
        );
    }

    #[test]
    fn navigation_is_screen_scoped() {
        let mut state = ShellIntegrationState::default();

        state.apply_event(event(
            TerminalShellIntegrationMarker::PromptStart,
            0,
            0,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandFinished,
            0,
            2,
            None,
        ));
        state.apply_event(event_on_screen(
            TerminalShellIntegrationMarker::PromptStart,
            TerminalScreen::Alternate,
            1,
            0,
            None,
        ));
        state.apply_event(event_on_screen(
            TerminalShellIntegrationMarker::CommandFinished,
            TerminalScreen::Alternate,
            1,
            2,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::PromptStart,
            2,
            0,
            None,
        ));
        state.apply_event(event(
            TerminalShellIntegrationMarker::CommandFinished,
            2,
            2,
            None,
        ));

        assert_eq!(
            state.select_completed_block(1).map(|block| block.id),
            Some(1)
        );
        assert_eq!(
            state
                .select_previous_completed_block_for_screen(TerminalScreen::Main)
                .map(|block| block.id),
            Some(2)
        );

        assert_eq!(
            state.select_completed_block(1).map(|block| block.id),
            Some(1)
        );
        assert_eq!(
            state
                .select_next_completed_block_for_screen(TerminalScreen::Main)
                .map(|block| block.id),
            Some(0)
        );
    }
}

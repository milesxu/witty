//! Browser-facing entry points for the Witty prototype.
//!
//! This crate starts deliberately small: it proves that the browser build can
//! reuse the terminal core and renderer frame-planning layers without pulling
//! in native PTY, native clipboard, or Wasmtime host dependencies.

use anyhow::Result;
#[cfg(target_arch = "wasm32")]
use witty_core::paste_payload;
#[cfg(target_arch = "wasm32")]
use witty_core::HyperlinkId;
#[cfg(any(test, target_arch = "wasm32"))]
use witty_core::RenderSnapshot;
#[cfg(test)]
use witty_core::TerminalClipboardWrite;
#[cfg(any(test, target_arch = "wasm32"))]
use witty_core::TerminalHostAction;
#[cfg(any(test, target_arch = "wasm32"))]
use witty_core::{
    encode_terminal_focus_event, encode_terminal_mouse_event, terminal_char_width,
    validate_external_url, CellFlags, CellPoint, CellRange, CursorState, FocusEventKind,
    MouseButtonCode, MouseEventKind, MouseModifiers, PixelMousePosition, Rgba, TerminalHyperlink,
    TerminalMouseEvent, TerminalMouseModes, TerminalScreen,
};
use witty_core::{
    encode_terminal_key_input as encode_core_terminal_key_input, BasicTerminal, GridSize,
    TerminalInputModes, TerminalKey as CoreTerminalKey,
    TerminalKeyEventType as CoreTerminalKeyEventType, TerminalKeyInput as CoreTerminalKeyInput,
    TerminalKeyModifiers as CoreTerminalKeyModifiers, TerminalKeypadKey as CoreTerminalKeypadKey,
    TerminalModifierKey as CoreTerminalModifierKey, TerminalNamedKey,
    KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES, KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES,
    KITTY_KEYBOARD_REPORT_ALTERNATE_KEYS, KITTY_KEYBOARD_REPORT_ASSOCIATED_TEXT,
    KITTY_KEYBOARD_REPORT_EVENT_TYPES,
};
#[cfg(test)]
use witty_core::{TerminalRowAnchor, TerminalVisibleRowAnchor};
use witty_plugin_api::{
    CommandRegistration, NetworkPermission, PluginAction, PluginEvent, PluginManifest,
    PluginPermissions, PluginRuntime, TerminalReadPermission, TerminalWritePermission,
    VaultPermission,
};
#[cfg(target_arch = "wasm32")]
use witty_render_wgpu::WgpuRectRenderer;
use witty_render_wgpu::{CellMetrics, RetainedFramePlanner};
#[cfg(any(test, target_arch = "wasm32"))]
use witty_render_wgpu::{FramePlan, GlyphBatchItem, PixelPoint, PixelSize, RectBatchItem};
#[cfg(any(test, target_arch = "wasm32"))]
use witty_render_wgpu::{FrameStats, RendererCacheStats, RendererTimingStats};
#[cfg(target_arch = "wasm32")]
use witty_transport::BrowserGatewayServerMessage;
use witty_transport::{BrowserGatewayTransport, MockTransport, TransportEvent};
#[cfg(target_arch = "wasm32")]
use witty_ui::apply_command_block_folded_frame_remap_with_anchors;
#[cfg(test)]
use witty_ui::apply_command_block_folded_row_mask_with_anchors;
#[cfg(target_arch = "wasm32")]
use witty_ui::apply_command_block_gutter_hover_overlay_with_anchors;
#[cfg(target_arch = "wasm32")]
use witty_ui::apply_command_block_gutter_overlay_with_anchors;
#[cfg(test)]
use witty_ui::apply_command_block_selection_overlay;
#[cfg(target_arch = "wasm32")]
use witty_ui::apply_command_block_selection_overlay_with_anchors;
#[cfg(target_arch = "wasm32")]
use witty_ui::apply_ime_preedit_overlay;
#[cfg(any(test, target_arch = "wasm32"))]
use witty_ui::COMMAND_BLOCK_TOGGLE_FOLD_COMMAND_ID;
#[cfg(any(test, target_arch = "wasm32"))]
use witty_ui::SEARCH_OPEN_COMMAND_ID;
#[cfg(any(test, target_arch = "wasm32"))]
use witty_ui::{
    apply_command_block_action_menu_overlay, apply_command_block_command,
    apply_command_block_status_label_overlay_with_anchors, command_block_command_registrations,
    command_block_copy_target, command_block_folded_visual_pixel_to_terminal_pixel_with_anchors,
    selected_command_block_copy_text, CommandBlockActionMenu, ShellIntegrationState,
};
#[cfg(any(test, target_arch = "wasm32"))]
use witty_ui::{command_block_gutter_hit_test_with_anchors, TerminalCommandBlockGutterHit};
#[cfg(any(test, target_arch = "wasm32"))]
use witty_ui::{search_command_registrations, CommandPalette};
use witty_ui::{BuiltInPlugin, ImeComposition, TerminalApp, TerminalSearch};
#[cfg(any(test, target_arch = "wasm32"))]
use witty_ui::{COMMAND_BLOCK_ACTION_MENU_COMMAND_ID, COMMAND_BLOCK_CLEAR_SELECTION_COMMAND_ID};
#[cfg(test)]
use witty_ui::{
    COMMAND_BLOCK_COPY_COMMAND_ID, COMMAND_BLOCK_COPY_OUTPUT_ID,
    COMMAND_BLOCK_SELECT_LATEST_COMMAND_ID, SELECTED_COMMAND_BLOCK_BACKGROUND,
    SELECTED_COMMAND_BLOCK_GUTTER,
};
#[cfg(target_arch = "wasm32")]
use witty_ui::{SEARCH_CLOSE_COMMAND_ID, SEARCH_NEXT_COMMAND_ID, SEARCH_PREVIOUS_COMMAND_ID};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::{prelude::*, JsCast};

#[cfg(any(test, target_arch = "wasm32"))]
const DEFAULT_BROWSER_TITLE: &str = "Witty";

const BROWSER_SEARCH_SCROLL_BUFFER_ROWS: u16 = 1;
#[cfg(any(test, target_arch = "wasm32"))]
const BROWSER_COMMAND_PALETTE_MAX_COLS: u16 = 56;
#[cfg(any(test, target_arch = "wasm32"))]
const BROWSER_COMMAND_PALETTE_MAX_VISIBLE_ITEMS: usize = 3;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebReplayReport {
    pub frames: u32,
    pub visible_rows: u16,
    pub visible_cols: u16,
    pub first_rebuilt_rows: usize,
    pub second_reused_rows: usize,
    pub second_rebuilt_rows: usize,
    pub second_glyph_runs: usize,
    pub second_glyph_chars: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebSessionSmokeReport {
    pub commands: usize,
    pub frame_glyph_runs: usize,
    pub frame_glyph_chars: usize,
    pub frame_glyph_prepare_batches: usize,
    pub frame_max_glyph_run_chars: usize,
    pub written_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserGatewaySmokeReport {
    pub outbound_bytes: usize,
    pub drained_bytes: usize,
    pub output_bytes: usize,
    pub resized: GridSize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserKeyInputReport {
    pub key: String,
    pub text: String,
    pub control: bool,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrowserCanvasResizeReport {
    pub backing_width: u32,
    pub backing_height: u32,
    pub device_pixel_ratio: f64,
    pub grid: GridSize,
    pub metrics: CellMetrics,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserSearchSmokeReport {
    pub query: String,
    pub match_count: usize,
    pub active_index: Option<usize>,
    pub visible_highlights: usize,
    pub active_visible: bool,
    pub status: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowserCommandPaletteSmokeReport {
    pub query: String,
    pub filtered_count: usize,
    pub selected_index: Option<usize>,
    pub selected_command_id: String,
    pub visible_item_command_ids: Vec<String>,
    pub status: String,
    pub overlay_glyphs: usize,
    pub overlay_backgrounds: usize,
}

pub fn mock_replay_report() -> WebReplayReport {
    let size = GridSize::new(4, 16);
    let mut terminal = BasicTerminal::new(size);
    let mut planner = RetainedFramePlanner::new(CellMetrics::default());

    let first = planner.plan(&terminal.take_snapshot());

    terminal.feed(b"Witty web\r\nmock replay");
    let second = planner.plan(&terminal.take_snapshot());

    WebReplayReport {
        frames: 2,
        visible_rows: second.stats.visible_rows,
        visible_cols: second.stats.visible_cols,
        first_rebuilt_rows: first.stats.rebuilt_rows,
        second_reused_rows: second.stats.reused_rows,
        second_rebuilt_rows: second.stats.rebuilt_rows,
        second_glyph_runs: second.stats.glyph_runs,
        second_glyph_chars: second.stats.glyph_chars,
    }
}

pub fn browser_session_smoke_report() -> WebSessionSmokeReport {
    let size = GridSize::new(4, 16);
    let transport = MockTransport::new(size);
    let mut app = TerminalApp::new(transport, size);
    app.install_builtin_plugin(WebEchoPlugin)
        .expect("web echo plugin should install");

    let mut terminal = BasicTerminal::new(size);
    terminal.feed(b"websession");
    app.set_snapshot(terminal.take_snapshot());
    let frame = app.frame_plan();

    app.invoke_command("web.echo", serde_json::Value::Null)
        .expect("web echo command should dispatch");

    WebSessionSmokeReport {
        commands: app.commands().len(),
        frame_glyph_runs: frame.stats.glyph_runs,
        frame_glyph_chars: frame.stats.glyph_chars,
        frame_glyph_prepare_batches: frame.stats.glyph_prepare_batches,
        frame_max_glyph_run_chars: frame.stats.max_glyph_run_chars,
        written_bytes: app.transport().written().len(),
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_frame_stats_json(
    stats: FrameStats,
    renderer_cache: RendererCacheStats,
    renderer_timing: RendererTimingStats,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&serde_json::json!({
        "visibleRows": stats.visible_rows,
        "visibleCols": stats.visible_cols,
        "backgroundRuns": stats.background_runs,
        "glyphRuns": stats.glyph_runs,
        "glyphChars": stats.glyph_chars,
        "glyphPrepareBatches": stats.glyph_prepare_batches,
        "maxGlyphRunChars": stats.max_glyph_run_chars,
        "selectionRects": stats.selection_rects,
        "searchHighlightRects": stats.search_highlight_rects,
        "textDecorationRects": stats.text_decoration_rects,
        "cursorVisible": stats.cursor_visible,
        "rectVertices": stats.rect_vertices,
        "rectVertexCapacity": stats.rect_vertex_capacity,
        "rendererTextBuffersReused": renderer_cache.text_buffers_reused,
        "rendererTextBuffersRebuilt": renderer_cache.text_buffers_rebuilt,
        "rendererTextBuffersRetired": renderer_cache.text_buffers_retired,
        "rendererTextBufferCount": renderer_cache.text_buffer_count,
        "rendererTextRendererCount": renderer_cache.text_renderer_count,
        "rendererRectVertexCapacity": renderer_cache.rect_vertex_capacity,
        "rendererCpuPrepareUs": renderer_timing.cpu_prepare_us,
        "rendererTextBufferSyncUs": renderer_timing.text_buffer_sync_us,
        "rendererGlyphPrepareUs": renderer_timing.glyph_prepare_us,
        "rendererRectVertexSyncUs": renderer_timing.rect_vertex_sync_us,
        "fullDamage": stats.full_damage,
        "damageRegions": stats.damage_regions,
        "reusedRows": stats.reused_rows,
        "rebuiltRows": stats.rebuilt_rows,
    }))
}

pub fn browser_key_input_report(
    key: impl Into<String>,
    text: impl Into<String>,
    control: bool,
) -> BrowserKeyInputReport {
    let key = key.into();
    let text = text.into();
    let bytes = encode_browser_key_input(&key, &text, control, TerminalInputModes::default())
        .unwrap_or_default();

    BrowserKeyInputReport {
        key,
        text,
        control,
        bytes,
    }
}

pub fn browser_keyboard_protocol_diagnostic_report_json(
    key: impl Into<String>,
    text: impl Into<String>,
    control: bool,
    code: impl Into<String>,
    location: u32,
    modifier_mask: u8,
    event_type: u8,
) -> String {
    let key = key.into();
    let text = text.into();
    let code = code.into();
    browser_keyboard_protocol_diagnostic_json_line(
        &key,
        &text,
        BrowserKeyModifiers::from_browser_mask(control, modifier_mask),
        &code,
        location,
        BrowserKeyEventType::from_browser_event_type(event_type),
    )
}

pub fn browser_gateway_smoke_report() -> BrowserGatewaySmokeReport {
    let size = GridSize::new(4, 16);
    let mut app = TerminalApp::new(BrowserGatewayTransport::new(size), size);

    app.write_input(b"xy\r")
        .expect("browser gateway should record outbound input");
    app.transport_mut().push_output(b"remote ok".to_vec());
    app.resize_transport(GridSize::new(8, 32))
        .expect("browser gateway should resize");

    let outbound_bytes = app.transport().outbound().len();
    let drained_bytes = app.transport_mut().drain_outbound().len();
    let output_bytes = match app
        .poll_transport()
        .expect("browser gateway event poll should succeed")
    {
        Some(TransportEvent::Output(bytes)) => bytes.len(),
        other => panic!("expected browser gateway output, got {other:?}"),
    };

    BrowserGatewaySmokeReport {
        outbound_bytes,
        drained_bytes,
        output_bytes,
        resized: app.transport().size(),
    }
}

pub fn browser_canvas_resize_report(
    css_width: f64,
    css_height: f64,
    device_pixel_ratio: f64,
) -> BrowserCanvasResizeReport {
    browser_canvas_sizing(css_width, css_height, device_pixel_ratio).report()
}

pub fn browser_search_smoke_report() -> BrowserSearchSmokeReport {
    let size = GridSize::new(3, 32);
    let mut terminal = BasicTerminal::new(size);
    terminal.feed(b"alpha one\r\nbeta row\r\nalpha two");

    let mut search = TerminalSearch::default();
    search.open(&terminal.search_text_rows(), None);
    search.input_text(&terminal.search_text_rows(), "alpha");
    if let Some(active) = search.active_match() {
        terminal.scroll_to_search_match(active.row, BROWSER_SEARCH_SCROLL_BUFFER_ROWS);
    }
    let highlights = terminal.visible_search_highlights(search.matches(), search.active_match());

    BrowserSearchSmokeReport {
        query: search.query().to_owned(),
        match_count: search.match_count(),
        active_index: search.active_index(),
        visible_highlights: highlights.len(),
        active_visible: highlights.iter().any(|highlight| highlight.active),
        status: browser_search_status_text(&search),
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
pub fn browser_command_palette_smoke_report() -> BrowserCommandPaletteSmokeReport {
    let size = GridSize::new(8, 48);
    let commands = browser_default_command_registrations();
    let mut palette = CommandPalette::default();
    palette.open(&commands);
    palette.input_text("sea");

    let mut frame = browser_entry_frame();
    let base_backgrounds = frame.backgrounds.len();
    apply_browser_command_palette_overlay(
        &mut frame,
        &palette,
        None,
        &commands,
        CellMetrics::default(),
        size,
    );

    BrowserCommandPaletteSmokeReport {
        query: palette.query().to_owned(),
        filtered_count: palette.filtered_count(),
        selected_index: palette.selected_index(),
        selected_command_id: palette
            .selected_command()
            .map(|command| command.id.clone())
            .unwrap_or_default(),
        visible_item_command_ids: palette
            .visible_items(BROWSER_COMMAND_PALETTE_MAX_VISIBLE_ITEMS)
            .into_iter()
            .map(|item| item.command.id.clone())
            .collect(),
        status: browser_command_palette_status_text(&palette),
        overlay_glyphs: frame.glyphs.len(),
        overlay_backgrounds: frame.backgrounds.len().saturating_sub(base_backgrounds),
    }
}

#[cfg(test)]
fn drain_clipboard_write_actions_json(terminal: &mut BasicTerminal) -> Result<String> {
    let mut shell_integration = ShellIntegrationState::default();
    drain_browser_host_actions_json(terminal, &mut shell_integration, |_| Ok(()))
}

#[cfg(test)]
fn drain_browser_host_actions_json(
    terminal: &mut BasicTerminal,
    shell_integration: &mut ShellIntegrationState,
    mut write_reply: impl FnMut(&[u8]) -> Result<()>,
) -> Result<String> {
    drain_browser_host_actions_json_at_ms(terminal, shell_integration, None, &mut write_reply)
}

#[cfg(any(test, target_arch = "wasm32"))]
fn drain_browser_host_actions_json_at_ms(
    terminal: &mut BasicTerminal,
    shell_integration: &mut ShellIntegrationState,
    observed_at_ms: Option<u64>,
    mut write_reply: impl FnMut(&[u8]) -> Result<()>,
) -> Result<String> {
    let mut writes = Vec::new();
    for action in terminal.drain_host_actions() {
        match action {
            TerminalHostAction::ClipboardWrite(write) => writes.push(write),
            TerminalHostAction::TerminalReply(reply) => write_reply(&reply.bytes)?,
            TerminalHostAction::ShellIntegration(event) => {
                if let Some(observed_at_ms) = observed_at_ms {
                    shell_integration.apply_event_at_ms(event, observed_at_ms);
                } else {
                    shell_integration.apply_event(event);
                }
            }
            TerminalHostAction::CurrentDirectory(directory) => {
                shell_integration.apply_current_directory(directory);
            }
            TerminalHostAction::Bell => {}
        }
    }

    serde_json::to_string(&writes).map_err(Into::into)
}

#[cfg(target_arch = "wasm32")]
fn browser_now_ms() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map(|performance| performance.now())
        .unwrap_or(0.0)
}

#[cfg(target_arch = "wasm32")]
fn elapsed_ms_since(started_at_ms: f64) -> u64 {
    let elapsed = browser_now_ms() - started_at_ms;
    if !elapsed.is_finite() || elapsed <= 0.0 {
        return 0;
    }
    elapsed.min(u64::MAX as f64) as u64
}

#[cfg(any(test, target_arch = "wasm32"))]
fn terminal_screen_name(screen: TerminalScreen) -> &'static str {
    match screen {
        TerminalScreen::Main => "main",
        TerminalScreen::Alternate => "alternate",
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_completed_command_blocks_json_text(
    shell_integration: &ShellIntegrationState,
) -> std::result::Result<String, serde_json::Error> {
    serde_json::to_string(shell_integration.completed_blocks())
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_completed_command_blocks_for_screen_json_text(
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
) -> std::result::Result<String, serde_json::Error> {
    let blocks = shell_integration
        .completed_blocks_for_screen(screen)
        .cloned()
        .collect::<Vec<_>>();
    serde_json::to_string(&blocks)
}

#[cfg(test)]
fn browser_visible_command_blocks_json_text(
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    rows: u16,
) -> std::result::Result<String, serde_json::Error> {
    serde_json::to_string(&shell_integration.completed_blocks_intersecting_rows(screen, 0, rows))
}

#[cfg(target_arch = "wasm32")]
fn browser_visible_command_blocks_json_text_with_anchors(
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    visible_row_anchors: &[witty_core::TerminalVisibleRowAnchor],
    rows: u16,
) -> std::result::Result<String, serde_json::Error> {
    serde_json::to_string(
        &shell_integration.completed_blocks_intersecting_visible_rows(
            screen,
            visible_row_anchors,
            rows,
        ),
    )
}

#[cfg(test)]
fn browser_visible_command_block_row_spans_json_text(
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    rows: u16,
) -> std::result::Result<String, serde_json::Error> {
    serde_json::to_string(
        &shell_integration.completed_block_row_spans_intersecting_rows(screen, 0, rows),
    )
}

#[cfg(target_arch = "wasm32")]
fn browser_visible_command_block_row_spans_json_text_with_anchors(
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    visible_row_anchors: &[witty_core::TerminalVisibleRowAnchor],
    rows: u16,
) -> std::result::Result<String, serde_json::Error> {
    serde_json::to_string(
        &shell_integration.completed_block_row_spans_intersecting_visible_rows(
            screen,
            visible_row_anchors,
            rows,
        ),
    )
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_folded_command_block_hidden_row_spans_json_text(
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    visible_row_anchors: &[witty_core::TerminalVisibleRowAnchor],
    rows: u16,
) -> std::result::Result<String, serde_json::Error> {
    serde_json::to_string(
        &shell_integration.folded_hidden_row_spans_intersecting_visible_rows(
            screen,
            visible_row_anchors,
            rows,
        ),
    )
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_folded_command_block_compact_rows_json_text(
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    visible_row_anchors: &[witty_core::TerminalVisibleRowAnchor],
    rows: u16,
) -> std::result::Result<String, serde_json::Error> {
    serde_json::to_string(&shell_integration.folded_compact_visual_rows(
        screen,
        visible_row_anchors,
        rows,
    ))
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct BrowserHitTestViewport {
    device_pixel_ratio: f64,
    metrics: CellMetrics,
    size: GridSize,
}

#[cfg(test)]
fn browser_command_block_gutter_hit_json_text(
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    visible_row_anchors: &[witty_core::TerminalVisibleRowAnchor],
    offset_x: f64,
    offset_y: f64,
    viewport: BrowserHitTestViewport,
) -> String {
    let hit = browser_command_block_gutter_hit(
        shell_integration,
        screen,
        visible_row_anchors,
        offset_x,
        offset_y,
        viewport,
    );

    match hit {
        Some(hit) => browser_command_block_gutter_hit_json_for_hit(Some(hit)),
        None => browser_command_block_gutter_hit_json_for_hit(None),
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_command_block_gutter_hit_json_for_hit(
    hit: Option<TerminalCommandBlockGutterHit>,
) -> String {
    match hit {
        Some(hit) => serde_json::json!({
            "hit": true,
            "id": hit.id,
            "screen": terminal_screen_name(hit.screen),
            "visibleRow": hit.visible_row,
            "startRow": hit.start_row,
            "endRow": hit.end_row,
            "selected": hit.selected,
            "exitCode": hit.exit_code,
        })
        .to_string(),
        None => serde_json::json!({
            "hit": false,
            "id": serde_json::Value::Null,
            "screen": "",
            "visibleRow": serde_json::Value::Null,
            "startRow": serde_json::Value::Null,
            "endRow": serde_json::Value::Null,
            "selected": false,
            "exitCode": serde_json::Value::Null,
        })
        .to_string(),
    }
}

#[cfg(test)]
fn browser_command_block_gutter_hit(
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    visible_row_anchors: &[witty_core::TerminalVisibleRowAnchor],
    offset_x: f64,
    offset_y: f64,
    viewport: BrowserHitTestViewport,
) -> Option<TerminalCommandBlockGutterHit> {
    let visual_point =
        browser_pixel_point_for_offset(offset_x, offset_y, viewport.device_pixel_ratio)?;
    let terminal_point = command_block_folded_visual_pixel_to_terminal_pixel_with_anchors(
        shell_integration,
        screen,
        visible_row_anchors,
        visual_point,
        viewport.metrics,
        viewport.size,
    )?;
    command_block_gutter_hit_test_with_anchors(
        shell_integration,
        screen,
        visible_row_anchors,
        terminal_point,
        viewport.metrics,
        viewport.size,
    )
}

#[cfg(test)]
fn browser_command_block_gutter_hover_id(
    shell_integration: &ShellIntegrationState,
    screen: TerminalScreen,
    visible_row_anchors: &[witty_core::TerminalVisibleRowAnchor],
    offset_x: f64,
    offset_y: f64,
    viewport: BrowserHitTestViewport,
) -> Option<u64> {
    browser_command_block_gutter_hit(
        shell_integration,
        screen,
        visible_row_anchors,
        offset_x,
        offset_y,
        viewport,
    )
    .map(|hit| hit.id)
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_pixel_point_for_offset(
    offset_x: f64,
    offset_y: f64,
    device_pixel_ratio: f64,
) -> Option<PixelPoint> {
    Some(PixelPoint {
        x: browser_backing_axis(offset_x, device_pixel_ratio)? as f32,
        y: browser_backing_axis(offset_y, device_pixel_ratio)? as f32,
    })
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_last_completed_command_block_json_text(
    shell_integration: &ShellIntegrationState,
) -> std::result::Result<String, serde_json::Error> {
    serde_json::to_string(&shell_integration.last_completed_block())
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_selected_command_block_json_text(
    shell_integration: &ShellIntegrationState,
) -> std::result::Result<String, serde_json::Error> {
    serde_json::to_string(&shell_integration.selected_completed_block())
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_selected_command_block_text_ranges_json_text(
    shell_integration: &ShellIntegrationState,
) -> std::result::Result<String, serde_json::Error> {
    serde_json::to_string(&shell_integration.selected_command_block_text_ranges())
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_selected_command_block_text_json_text(
    terminal: &BasicTerminal,
    shell_integration: &ShellIntegrationState,
) -> std::result::Result<String, serde_json::Error> {
    let selected = shell_integration
        .selected_command_block_text_ranges()
        .map(|ranges| {
            serde_json::json!({
                "id": ranges.id,
                "screen": ranges.screen,
                "command": terminal.text_for_range(ranges.command_text_range()),
                "output": ranges
                .output_text_range()
                .and_then(|range| terminal.text_for_range(range)),
                "exit_code": ranges.exit_code,
            })
        });
    serde_json::to_string(&selected)
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_command_block_copy_text(
    terminal: &BasicTerminal,
    shell_integration: &ShellIntegrationState,
    command_id: &str,
) -> Option<String> {
    let target = command_block_copy_target(command_id)?;
    selected_command_block_copy_text(terminal, shell_integration, target)
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_select_latest_command_block_for_screen_json_text(
    shell_integration: &mut ShellIntegrationState,
    screen: TerminalScreen,
) -> std::result::Result<String, serde_json::Error> {
    let selected = shell_integration.select_latest_completed_block_for_screen(screen);
    serde_json::to_string(&selected)
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_select_previous_command_block_for_screen_json_text(
    shell_integration: &mut ShellIntegrationState,
    screen: TerminalScreen,
) -> std::result::Result<String, serde_json::Error> {
    let selected = shell_integration.select_previous_completed_block_for_screen(screen);
    serde_json::to_string(&selected)
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_select_next_command_block_for_screen_json_text(
    shell_integration: &mut ShellIntegrationState,
    screen: TerminalScreen,
) -> std::result::Result<String, serde_json::Error> {
    let selected = shell_integration.select_next_completed_block_for_screen(screen);
    serde_json::to_string(&selected)
}

#[cfg(test)]
fn browser_select_command_block_gutter_hit_json_text(
    shell_integration: &mut ShellIntegrationState,
    screen: TerminalScreen,
    visible_row_anchors: &[witty_core::TerminalVisibleRowAnchor],
    offset_x: f64,
    offset_y: f64,
    viewport: BrowserHitTestViewport,
) -> std::result::Result<String, serde_json::Error> {
    let hit = browser_command_block_gutter_hit(
        shell_integration,
        screen,
        visible_row_anchors,
        offset_x,
        offset_y,
        viewport,
    );
    browser_select_command_block_gutter_hit_json_for_hit(shell_integration, hit)
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_select_command_block_gutter_hit_json_for_hit(
    shell_integration: &mut ShellIntegrationState,
    hit: Option<TerminalCommandBlockGutterHit>,
) -> std::result::Result<String, serde_json::Error> {
    let selected = hit.and_then(|hit| shell_integration.select_completed_block(hit.id));
    serde_json::to_string(&selected)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn witty_web_mock_replay_glyph_chars() -> u32 {
    mock_replay_report().second_glyph_chars as u32
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn witty_web_session_written_bytes() -> u32 {
    browser_session_smoke_report().written_bytes as u32
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn witty_browser_keyboard_protocol_diagnostic_report_json(
    key: String,
    text: String,
    control: bool,
    code: String,
    location: u32,
    modifier_mask: u8,
    event_type: u8,
) -> String {
    browser_keyboard_protocol_diagnostic_report_json(
        key,
        text,
        control,
        code,
        location,
        modifier_mask,
        event_type,
    )
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub async fn witty_start_canvas(canvas_id: String) -> Result<(), JsValue> {
    witty_start_canvas_inner(canvas_id, None).await
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub async fn witty_start_canvas_with_font_data(
    canvas_id: String,
    font_data: Vec<u8>,
) -> Result<(), JsValue> {
    witty_start_canvas_inner(canvas_id, Some(font_data)).await
}

#[cfg(target_arch = "wasm32")]
async fn witty_start_canvas_inner(
    canvas_id: String,
    font_data: Option<Vec<u8>>,
) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();

    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window is unavailable"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("document is unavailable"))?;
    let element = document
        .get_element_by_id(&canvas_id)
        .ok_or_else(|| JsValue::from_str("canvas element was not found"))?;
    let canvas: web_sys::HtmlCanvasElement = element
        .dyn_into()
        .map_err(|_| JsValue::from_str("element is not an HTML canvas"))?;
    let width = canvas.width().max(1);
    let height = canvas.height().max(1);

    let mut renderer = match font_data {
        Some(font_data) => {
            WgpuRectRenderer::new_for_canvas_with_font_data(canvas, width, height, font_data).await
        }
        None => WgpuRectRenderer::new_for_canvas(canvas, width, height).await,
    }
    .map_err(js_error)?;
    let frame = browser_entry_frame();
    renderer.render(&frame).map_err(js_error)?;
    Ok(())
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct WittyWebSession {
    renderer: WgpuRectRenderer,
    last_frame_stats: FrameStats,
    last_renderer_cache_stats: RendererCacheStats,
    last_renderer_timing_stats: RendererTimingStats,
    app: TerminalApp<BrowserGatewayTransport>,
    terminal: BasicTerminal,
    terminal_search: TerminalSearch,
    command_palette: CommandPalette,
    command_block_action_menu: CommandBlockActionMenu,
    shell_integration: ShellIntegrationState,
    ime_composition: ImeComposition,
    mouse_report: BrowserMouseReportState,
    local_selection: BrowserLocalSelectionState,
    hovered_hyperlink: Option<HyperlinkId>,
    hovered_command_block_id: Option<u64>,
    started_at_ms: f64,
    size: GridSize,
    metrics: CellMetrics,
    backing_width: u32,
    backing_height: u32,
    device_pixel_ratio: f64,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BrowserTextInputTarget {
    Terminal,
    Search,
    CommandPalette,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub async fn witty_create_session(
    canvas_id: String,
    font_data: Vec<u8>,
    css_width: f64,
    css_height: f64,
    device_pixel_ratio: f64,
) -> Result<WittyWebSession, JsValue> {
    console_error_panic_hook::set_once();

    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window is unavailable"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("document is unavailable"))?;
    let element = document
        .get_element_by_id(&canvas_id)
        .ok_or_else(|| JsValue::from_str("canvas element was not found"))?;
    let canvas: web_sys::HtmlCanvasElement = element
        .dyn_into()
        .map_err(|_| JsValue::from_str("element is not an HTML canvas"))?;
    let sizing = browser_canvas_sizing(css_width, css_height, device_pixel_ratio);
    canvas.set_width(sizing.backing_width);
    canvas.set_height(sizing.backing_height);
    let renderer = WgpuRectRenderer::new_for_canvas_with_font_data(
        canvas,
        sizing.backing_width,
        sizing.backing_height,
        font_data,
    )
    .await
    .map_err(js_error)?;
    let transport = BrowserGatewayTransport::new(sizing.grid);
    let mut app = TerminalApp::new(transport, sizing.grid);
    app.install_builtin_plugin(BrowserBuiltInCommandsPlugin)
        .map_err(js_error)?;
    for command in search_command_registrations() {
        app.register_command(command).map_err(js_error)?;
    }
    app.install_builtin_plugin(WebEchoPlugin)
        .map_err(js_error)?;
    for command in command_block_command_registrations() {
        app.register_command(command).map_err(js_error)?;
    }
    app.set_cell_metrics(sizing.metrics);
    let mut terminal = BasicTerminal::new(sizing.grid);
    terminal.feed(b"Witty web\r\nbrowser input ready\r\n> ");

    let mut session = WittyWebSession {
        renderer,
        last_frame_stats: FrameStats::default(),
        last_renderer_cache_stats: RendererCacheStats::default(),
        last_renderer_timing_stats: RendererTimingStats::default(),
        app,
        terminal,
        terminal_search: TerminalSearch::default(),
        command_palette: CommandPalette::default(),
        command_block_action_menu: CommandBlockActionMenu::default(),
        shell_integration: ShellIntegrationState::default(),
        ime_composition: ImeComposition::default(),
        mouse_report: BrowserMouseReportState::default(),
        local_selection: BrowserLocalSelectionState::default(),
        hovered_hyperlink: None,
        hovered_command_block_id: None,
        started_at_ms: browser_now_ms(),
        size: sizing.grid,
        metrics: sizing.metrics,
        backing_width: sizing.backing_width,
        backing_height: sizing.backing_height,
        device_pixel_ratio: sizing.device_pixel_ratio,
    };
    session.render_current_frame()?;
    Ok(session)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WittyWebSession {
    pub fn handle_key(
        &mut self,
        key: String,
        text: String,
        control: bool,
        code: String,
        location: u32,
        modifier_mask: u8,
        event_type: u8,
    ) -> Result<bool, JsValue> {
        let Some(bytes) = encode_browser_key_input_with_metadata_and_event_type(
            &key,
            &text,
            BrowserKeyModifiers::from_browser_mask(control, modifier_mask),
            &code,
            location,
            BrowserKeyEventType::from_browser_event_type(event_type),
            self.terminal.input_modes(),
        ) else {
            return Ok(false);
        };

        self.app.write_input(&bytes).map_err(js_error)?;
        let echo = browser_echo_bytes(&bytes);
        if !echo.is_empty() {
            self.terminal.feed(&echo);
            self.refresh_search_after_terminal_change();
            self.render_current_frame()?;
        }
        Ok(true)
    }

    pub fn set_ime_preedit(
        &mut self,
        text: String,
        caret_start: i32,
        caret_end: i32,
    ) -> Result<bool, JsValue> {
        let caret = browser_ime_caret(&text, caret_start, caret_end);
        let changed = apply_browser_ime_preedit(&mut self.ime_composition, text, caret);
        if changed {
            self.render_current_frame()?;
        }
        Ok(changed)
    }

    pub fn commit_ime_text(&mut self, text: String) -> Result<bool, JsValue> {
        let target = self.text_input_target();
        let mut committed = Vec::new();
        let result = apply_browser_ime_commit(&mut self.ime_composition, text);

        if let Some(text) = result.committed_text.as_deref() {
            match target {
                BrowserTextInputTarget::Terminal => {
                    let bytes = text.as_bytes();
                    self.app.write_input(bytes).map_err(js_error)?;
                    committed.extend_from_slice(bytes);
                }
                BrowserTextInputTarget::Search => {
                    self.terminal_search
                        .input_text(&self.terminal.search_text_rows(), text);
                    self.scroll_to_active_search_match();
                }
                BrowserTextInputTarget::CommandPalette => {
                    self.command_palette.input_text(text);
                }
            }
        }

        if !committed.is_empty() {
            let echo = browser_echo_bytes(&committed);
            if !echo.is_empty() {
                self.terminal.feed(&echo);
                self.refresh_search_after_terminal_change();
            }
        }
        if result.changed || result.committed_text.is_some() {
            self.render_current_frame()?;
        }
        Ok(result.committed_text.is_some())
    }

    pub fn clear_ime_preedit(&mut self) -> Result<bool, JsValue> {
        let changed = clear_browser_ime_preedit(&mut self.ime_composition);
        if changed {
            self.render_current_frame()?;
        }
        Ok(changed)
    }

    pub fn ime_is_active(&self) -> bool {
        self.ime_composition.is_active()
    }

    pub fn ime_preedit(&self) -> String {
        self.ime_composition.preedit().to_owned()
    }

    pub fn ime_target(&self) -> String {
        match self.text_input_target() {
            BrowserTextInputTarget::Terminal => "terminal",
            BrowserTextInputTarget::Search => "search",
            BrowserTextInputTarget::CommandPalette => "palette",
        }
        .to_owned()
    }

    pub fn ime_state_json(&self) -> Result<String, JsValue> {
        serde_json::to_string(&serde_json::json!({
            "active": self.ime_composition.is_active(),
            "preedit": self.ime_composition.preedit(),
            "target": self.ime_target(),
            "writtenBytes": self.written_bytes(),
        }))
        .map_err(js_json_error)
    }

    pub fn ime_cursor_rect_json(&self) -> Result<String, JsValue> {
        let rect = browser_cursor_css_rect(
            self.active_ime_cursor(),
            self.metrics,
            self.device_pixel_ratio,
        );
        serde_json::to_string(&serde_json::json!({
            "left": rect.left,
            "top": rect.top,
            "width": rect.width,
            "height": rect.height,
            "target": self.ime_target(),
        }))
        .map_err(js_json_error)
    }

    pub fn ime_cursor_left_css(&self) -> f64 {
        browser_cursor_css_rect(
            self.active_ime_cursor(),
            self.metrics,
            self.device_pixel_ratio,
        )
        .left
    }

    pub fn ime_cursor_top_css(&self) -> f64 {
        browser_cursor_css_rect(
            self.active_ime_cursor(),
            self.metrics,
            self.device_pixel_ratio,
        )
        .top
    }

    pub fn ime_cursor_width_css(&self) -> f64 {
        browser_cursor_css_rect(
            self.active_ime_cursor(),
            self.metrics,
            self.device_pixel_ratio,
        )
        .width
    }

    pub fn ime_cursor_height_css(&self) -> f64 {
        browser_cursor_css_rect(
            self.active_ime_cursor(),
            self.metrics,
            self.device_pixel_ratio,
        )
        .height
    }

    pub fn open_search(&mut self) -> Result<String, JsValue> {
        self.ime_composition.clear_preedit();
        self.command_palette.close();
        self.command_block_action_menu.close();
        let selected_text = self.terminal.selected_text();
        self.terminal_search
            .open(&self.terminal.search_text_rows(), selected_text.as_deref());
        self.scroll_to_active_search_match();
        self.render_current_frame()?;
        Ok(self.search_status_text())
    }

    pub fn close_search(&mut self) -> Result<String, JsValue> {
        self.ime_composition.clear_preedit();
        self.terminal_search.close();
        self.render_current_frame()?;
        Ok(self.search_status_text())
    }

    pub fn search_input_text(&mut self, text: String) -> Result<String, JsValue> {
        self.terminal_search
            .input_text(&self.terminal.search_text_rows(), &text);
        self.scroll_to_active_search_match();
        self.render_current_frame()?;
        Ok(self.search_status_text())
    }

    pub fn search_backspace(&mut self) -> Result<String, JsValue> {
        self.terminal_search
            .backspace(&self.terminal.search_text_rows());
        self.scroll_to_active_search_match();
        self.render_current_frame()?;
        Ok(self.search_status_text())
    }

    pub fn toggle_search_case_sensitive(&mut self) -> Result<String, JsValue> {
        self.terminal_search
            .toggle_case_sensitive(&self.terminal.search_text_rows());
        self.scroll_to_active_search_match();
        self.render_current_frame()?;
        Ok(self.search_status_text())
    }

    pub fn toggle_search_regex(&mut self) -> Result<String, JsValue> {
        self.terminal_search
            .toggle_regex(&self.terminal.search_text_rows());
        self.scroll_to_active_search_match();
        self.render_current_frame()?;
        Ok(self.search_status_text())
    }

    pub fn toggle_search_whole_word(&mut self) -> Result<String, JsValue> {
        self.terminal_search
            .toggle_whole_word(&self.terminal.search_text_rows());
        self.scroll_to_active_search_match();
        self.render_current_frame()?;
        Ok(self.search_status_text())
    }

    pub fn toggle_search_normalize_nfc(&mut self) -> Result<String, JsValue> {
        self.terminal_search
            .toggle_normalize_nfc(&self.terminal.search_text_rows());
        self.scroll_to_active_search_match();
        self.render_current_frame()?;
        Ok(self.search_status_text())
    }

    pub fn search_next(&mut self) -> Result<String, JsValue> {
        self.terminal_search.next_match();
        self.scroll_to_active_search_match();
        self.render_current_frame()?;
        Ok(self.search_status_text())
    }

    pub fn search_previous(&mut self) -> Result<String, JsValue> {
        self.terminal_search.previous_match();
        self.scroll_to_active_search_match();
        self.render_current_frame()?;
        Ok(self.search_status_text())
    }

    pub fn search_history_previous(&mut self) -> Result<String, JsValue> {
        self.terminal_search
            .previous_history_query(&self.terminal.search_text_rows());
        self.scroll_to_active_search_match();
        self.render_current_frame()?;
        Ok(self.search_status_text())
    }

    pub fn search_history_next(&mut self) -> Result<String, JsValue> {
        self.terminal_search
            .next_history_query(&self.terminal.search_text_rows());
        self.scroll_to_active_search_match();
        self.render_current_frame()?;
        Ok(self.search_status_text())
    }

    pub fn search_is_open(&self) -> bool {
        self.terminal_search.is_open()
    }

    pub fn search_query(&self) -> String {
        self.terminal_search.query().to_owned()
    }

    pub fn search_match_count(&self) -> u32 {
        self.terminal_search.match_count() as u32
    }

    pub fn search_active_index(&self) -> i32 {
        self.terminal_search
            .active_index()
            .map(|index| index as i32)
            .unwrap_or(-1)
    }

    pub fn search_case_sensitive(&self) -> bool {
        self.terminal_search.options().case_sensitive
    }

    pub fn search_regex_enabled(&self) -> bool {
        self.terminal_search.options().regex
    }

    pub fn search_whole_word_enabled(&self) -> bool {
        self.terminal_search.options().whole_word
    }

    pub fn search_normalize_nfc_enabled(&self) -> bool {
        self.terminal_search.options().normalize_nfc
    }

    pub fn search_error_text(&self) -> String {
        self.terminal_search.error_text().unwrap_or_default()
    }

    pub fn search_status_text(&self) -> String {
        browser_search_status_text_with_ime(
            &self.terminal_search,
            (self.text_input_target() == BrowserTextInputTarget::Search)
                .then_some(&self.ime_composition),
        )
    }

    pub fn search_visible_highlight_count(&self) -> u32 {
        self.visible_search_highlights().len() as u32
    }

    pub fn search_active_visible(&self) -> bool {
        self.visible_search_highlights()
            .iter()
            .any(|highlight| highlight.active)
    }

    pub fn open_command_palette(&mut self) -> Result<String, JsValue> {
        self.ime_composition.clear_preedit();
        self.terminal_search.close();
        self.command_block_action_menu.close();
        self.command_palette.open(self.app.commands());
        self.render_current_frame()?;
        Ok(self.command_palette_status_text())
    }

    pub fn close_command_palette(&mut self) -> Result<String, JsValue> {
        self.ime_composition.clear_preedit();
        self.command_palette.close();
        self.render_current_frame()?;
        Ok(self.command_palette_status_text())
    }

    pub fn command_palette_input_text(&mut self, text: String) -> Result<String, JsValue> {
        self.command_palette.input_text(&text);
        self.render_current_frame()?;
        Ok(self.command_palette_status_text())
    }

    pub fn command_palette_backspace(&mut self) -> Result<String, JsValue> {
        self.command_palette.backspace();
        self.render_current_frame()?;
        Ok(self.command_palette_status_text())
    }

    pub fn command_palette_move_selection(&mut self, delta: i32) -> Result<String, JsValue> {
        self.command_palette.move_selection(delta as isize);
        self.render_current_frame()?;
        Ok(self.command_palette_status_text())
    }

    pub fn confirm_command_palette(&mut self) -> Result<String, JsValue> {
        self.ime_composition.clear_preedit();
        let command_id = self.command_palette.confirm().unwrap_or_default();
        if command_id.is_empty() {
            self.render_current_frame()?;
            return Ok(command_id);
        }

        self.invoke_browser_command(&command_id)?;
        Ok(command_id)
    }

    pub fn invoke_command_shortcut(&mut self, key: String) -> Result<String, JsValue> {
        let Some(command_id) = browser_command_shortcut_for_key(&key, self.app.commands()) else {
            return Ok(String::new());
        };

        self.ime_composition.clear_preedit();
        self.command_palette.close();
        self.terminal_search.close();
        self.command_block_action_menu.close();
        self.invoke_browser_command(&command_id)?;
        Ok(command_id)
    }

    pub fn command_palette_is_open(&self) -> bool {
        self.command_palette.is_open()
    }

    pub fn command_palette_query(&self) -> String {
        self.command_palette.query().to_owned()
    }

    pub fn command_palette_filtered_count(&self) -> u32 {
        self.command_palette.filtered_count() as u32
    }

    pub fn command_palette_selected_id(&self) -> String {
        self.command_palette
            .selected_command()
            .map(|command| command.id.clone())
            .unwrap_or_default()
    }

    pub fn command_palette_selected_index(&self) -> i32 {
        self.command_palette
            .selected_index()
            .map(|index| index as i32)
            .unwrap_or(-1)
    }

    pub fn command_palette_status_text(&self) -> String {
        browser_command_palette_status_text(&self.command_palette)
    }

    pub fn command_palette_visible_items_json(&self, limit: u32) -> Result<String, JsValue> {
        browser_command_palette_visible_items_json_text(&self.command_palette, limit as usize)
            .map_err(js_json_error)
    }

    pub fn open_command_block_action_menu(&mut self) -> Result<String, JsValue> {
        self.ime_composition.clear_preedit();
        self.command_palette.close();
        self.terminal_search.close();
        self.command_block_action_menu
            .open_for_selected_block(&self.shell_integration);
        self.render_current_frame()?;
        Ok(self.command_block_action_menu_status_text())
    }

    pub fn close_command_block_action_menu(&mut self) -> Result<String, JsValue> {
        self.ime_composition.clear_preedit();
        self.command_block_action_menu.close();
        self.render_current_frame()?;
        Ok(self.command_block_action_menu_status_text())
    }

    pub fn command_block_action_menu_move_selection(
        &mut self,
        delta: i32,
    ) -> Result<String, JsValue> {
        self.command_block_action_menu
            .move_selection(delta as isize);
        self.render_current_frame()?;
        Ok(self.command_block_action_menu_status_text())
    }

    pub fn confirm_command_block_action_menu(&mut self) -> Result<String, JsValue> {
        self.ime_composition.clear_preedit();
        let command_id = self
            .command_block_action_menu
            .confirm()
            .unwrap_or_default()
            .to_owned();
        if command_id.is_empty() {
            self.render_current_frame()?;
            return Ok(command_id);
        }

        self.invoke_browser_command(&command_id)?;
        Ok(command_id)
    }

    pub fn command_block_action_menu_is_open(&self) -> bool {
        self.command_block_action_menu.is_open()
    }

    pub fn command_block_action_menu_status_text(&self) -> String {
        browser_command_block_action_menu_status_text(&self.command_block_action_menu)
    }

    pub fn command_block_action_menu_selected_id(&self) -> String {
        self.command_block_action_menu
            .selected_command_id()
            .unwrap_or_default()
            .to_owned()
    }

    pub fn command_block_action_menu_selected_index(&self) -> i32 {
        self.command_block_action_menu
            .selected_index()
            .map(|index| index as i32)
            .unwrap_or(-1)
    }

    pub fn command_block_action_menu_visible_items_json(&self) -> Result<String, JsValue> {
        browser_command_block_action_menu_visible_items_json_text(&self.command_block_action_menu)
            .map_err(js_json_error)
    }

    pub fn paste_text(&mut self, text: String) -> Result<bool, JsValue> {
        if text.is_empty() {
            return Ok(false);
        }

        let payload = paste_payload(&text, self.terminal.bracketed_paste_enabled());
        self.app.write_input(&payload).map_err(js_error)?;
        Ok(true)
    }

    pub fn update_hyperlink_hover(
        &mut self,
        offset_x: f64,
        offset_y: f64,
    ) -> Result<bool, JsValue> {
        let snapshot = self.terminal.snapshot();
        let hovered = self
            .terminal_cell_point_for_offset(offset_x, offset_y)
            .and_then(|point| snapshot.hyperlink_id_at(point));
        if self.hovered_hyperlink == hovered {
            return Ok(false);
        }

        self.hovered_hyperlink = hovered;
        self.render_current_frame()?;
        Ok(true)
    }

    pub fn update_command_block_gutter_hover(
        &mut self,
        offset_x: f64,
        offset_y: f64,
    ) -> Result<bool, JsValue> {
        let hovered = self
            .terminal_pixel_point_for_offset(offset_x, offset_y)
            .and_then(|point| {
                command_block_gutter_hit_test_with_anchors(
                    &self.shell_integration,
                    self.terminal.active_screen(),
                    &self.terminal.visible_row_anchors(),
                    point,
                    self.metrics,
                    self.size,
                )
                .map(|hit| hit.id)
            });
        if self.hovered_command_block_id == hovered {
            return Ok(false);
        }

        self.hovered_command_block_id = hovered;
        self.render_current_frame()?;
        Ok(true)
    }

    pub fn select_command_block_gutter_hit_json(
        &mut self,
        offset_x: f64,
        offset_y: f64,
    ) -> Result<String, JsValue> {
        let visible_row_anchors = self.terminal.visible_row_anchors();
        let hit = self
            .terminal_pixel_point_for_offset(offset_x, offset_y)
            .and_then(|point| {
                command_block_gutter_hit_test_with_anchors(
                    &self.shell_integration,
                    self.terminal.active_screen(),
                    &visible_row_anchors,
                    point,
                    self.metrics,
                    self.size,
                )
            });
        let json =
            browser_select_command_block_gutter_hit_json_for_hit(&mut self.shell_integration, hit)
                .map_err(js_json_error)?;
        let selected: serde_json::Value = serde_json::from_str(&json).map_err(js_json_error)?;
        if let Some(id) = selected.get("id").and_then(serde_json::Value::as_u64) {
            self.hovered_command_block_id = Some(id);
            self.render_current_frame()?;
        }
        Ok(json)
    }

    pub fn hyperlink_activation_target_json(
        &self,
        offset_x: f64,
        offset_y: f64,
    ) -> Result<String, JsValue> {
        let snapshot = self.terminal.snapshot();
        let target = self
            .terminal_cell_point_for_offset(offset_x, offset_y)
            .map(|point| browser_hyperlink_activation_target_for_point(&snapshot, point))
            .unwrap_or_else(BrowserHyperlinkActivationTarget::no_hit);
        Ok(target.to_json())
    }

    pub fn handle_mouse(
        &mut self,
        kind: String,
        button: i16,
        buttons: u16,
        offset_x: f64,
        offset_y: f64,
        delta_y: f64,
        shift: bool,
        alt: bool,
        control: bool,
        delta_mode: u32,
    ) -> Result<bool, JsValue> {
        let Some(kind) = BrowserMouseEventKind::from_browser_kind(&kind) else {
            return Ok(false);
        };
        let input = BrowserMouseInput {
            kind,
            button,
            buttons,
            offset_x,
            offset_y,
            delta_y,
            modifiers: MouseModifiers {
                shift,
                alt,
                control,
            },
        };
        if kind == BrowserMouseEventKind::LocalWheel {
            return self.handle_local_wheel(input.delta_y, delta_mode);
        }

        let Some(cell) = self.terminal_cell_point_for_offset(offset_x, offset_y) else {
            return Ok(false);
        };
        let pixel = self.terminal_pixel_mouse_position_for_offset(offset_x, offset_y);
        let Some(bytes) = self.mouse_report.encode_resolved(
            input,
            self.terminal.input_modes().mouse,
            cell,
            pixel,
        ) else {
            return Ok(false);
        };

        self.app.write_input(&bytes).map_err(js_error)?;
        Ok(true)
    }

    fn handle_local_wheel(&mut self, delta_y: f64, delta_mode: u32) -> Result<bool, JsValue> {
        let lines = browser_scroll_lines_for_wheel_delta(
            delta_y,
            delta_mode,
            self.metrics,
            self.size,
            self.device_pixel_ratio,
        );
        if lines == 0 {
            return Ok(false);
        }

        let previous_offset = self.terminal.viewport_offset();
        self.terminal.scroll_viewport_lines(lines);
        if self.terminal.viewport_offset() != previous_offset {
            self.render_current_frame()?;
        }

        Ok(true)
    }

    pub fn begin_local_selection(
        &mut self,
        offset_x: f64,
        offset_y: f64,
        click_count: u32,
    ) -> Result<bool, JsValue> {
        let Some(point) = self.terminal_cell_point_for_offset(offset_x, offset_y) else {
            return Ok(false);
        };
        self.local_selection
            .begin(&mut self.terminal, point, click_count);
        self.render_current_frame()?;
        Ok(true)
    }

    pub fn update_local_selection(
        &mut self,
        offset_x: f64,
        offset_y: f64,
    ) -> Result<bool, JsValue> {
        let Some(point) = self.terminal_cell_point_for_offset(offset_x, offset_y) else {
            return Ok(false);
        };
        if !self.local_selection.update(&mut self.terminal, point) {
            return Ok(false);
        }

        self.render_current_frame()?;
        Ok(true)
    }

    pub fn end_local_selection(&mut self) -> Result<bool, JsValue> {
        Ok(self.local_selection.end())
    }

    pub fn mouse_reporting_active(&self) -> bool {
        self.terminal.input_modes().mouse.reports_mouse()
    }

    pub fn handle_focus(&mut self, focused: bool) -> Result<bool, JsValue> {
        let kind = if focused {
            FocusEventKind::In
        } else {
            FocusEventKind::Out
        };
        let Some(bytes) = encode_terminal_focus_event(kind, self.terminal.input_modes().mouse)
        else {
            return Ok(false);
        };

        self.app.write_input(&bytes).map_err(js_error)?;
        Ok(true)
    }

    pub fn written_bytes(&self) -> u32 {
        self.app.transport().outbound().len() as u32
    }

    pub fn max_scrollback_lines(&self) -> u32 {
        self.terminal.max_scrollback_lines() as u32
    }

    pub fn scrollback_line_count(&self) -> u32 {
        self.terminal.scrollback_line_count() as u32
    }

    pub fn viewport_offset(&self) -> u32 {
        self.terminal.viewport_offset() as u32
    }

    pub fn set_scrollback_lines(&mut self, max_lines: u32) -> Result<(), JsValue> {
        self.terminal.set_max_scrollback_lines(max_lines as usize);
        self.refresh_search_after_terminal_change();
        self.render_current_frame()
    }

    pub fn written_text(&self) -> String {
        String::from_utf8_lossy(self.app.transport().outbound()).into_owned()
    }

    pub fn drain_outbound_text(&mut self) -> String {
        String::from_utf8_lossy(&self.app.transport_mut().drain_outbound()).into_owned()
    }

    pub fn drain_outbound_message_json(&mut self) -> Result<Option<String>, JsValue> {
        let Some(message) = self.app.transport_mut().drain_outbound_message() else {
            return Ok(None);
        };

        message.to_json().map(Some).map_err(js_json_error)
    }

    pub fn drain_clipboard_write_actions_json(&mut self) -> Result<String, JsValue> {
        let app = &mut self.app;
        let shell_integration = &mut self.shell_integration;
        let observed_at_ms = elapsed_ms_since(self.started_at_ms);
        drain_browser_host_actions_json_at_ms(
            &mut self.terminal,
            shell_integration,
            Some(observed_at_ms),
            |bytes| app.write_input(bytes),
        )
        .map_err(js_error)
    }

    pub fn active_screen(&self) -> String {
        terminal_screen_name(self.terminal.active_screen()).to_owned()
    }

    pub fn completed_command_blocks_json(&self) -> Result<String, JsValue> {
        browser_completed_command_blocks_json_text(&self.shell_integration).map_err(js_json_error)
    }

    pub fn completed_command_block_count(&self) -> u32 {
        self.shell_integration.completed_len() as u32
    }

    pub fn completed_command_blocks_for_active_screen_json(&self) -> Result<String, JsValue> {
        browser_completed_command_blocks_for_screen_json_text(
            &self.shell_integration,
            self.terminal.active_screen(),
        )
        .map_err(js_json_error)
    }

    pub fn completed_command_block_count_for_active_screen(&self) -> u32 {
        self.shell_integration
            .completed_len_for_screen(self.terminal.active_screen()) as u32
    }

    pub fn visible_command_blocks_json(&self) -> Result<String, JsValue> {
        browser_visible_command_blocks_json_text_with_anchors(
            &self.shell_integration,
            self.terminal.active_screen(),
            &self.terminal.visible_row_anchors(),
            self.size.rows,
        )
        .map_err(js_json_error)
    }

    pub fn visible_command_block_row_spans_json(&self) -> Result<String, JsValue> {
        browser_visible_command_block_row_spans_json_text_with_anchors(
            &self.shell_integration,
            self.terminal.active_screen(),
            &self.terminal.visible_row_anchors(),
            self.size.rows,
        )
        .map_err(js_json_error)
    }

    pub fn folded_command_block_hidden_row_spans_json(&self) -> Result<String, JsValue> {
        browser_folded_command_block_hidden_row_spans_json_text(
            &self.shell_integration,
            self.terminal.active_screen(),
            &self.terminal.visible_row_anchors(),
            self.size.rows,
        )
        .map_err(js_json_error)
    }

    pub fn folded_command_block_compact_rows_json(&self) -> Result<String, JsValue> {
        browser_folded_command_block_compact_rows_json_text(
            &self.shell_integration,
            self.terminal.active_screen(),
            &self.terminal.visible_row_anchors(),
            self.size.rows,
        )
        .map_err(js_json_error)
    }

    pub fn command_block_gutter_hit_json(&self, offset_x: f64, offset_y: f64) -> String {
        let hit = self
            .terminal_pixel_point_for_offset(offset_x, offset_y)
            .and_then(|point| {
                command_block_gutter_hit_test_with_anchors(
                    &self.shell_integration,
                    self.terminal.active_screen(),
                    &self.terminal.visible_row_anchors(),
                    point,
                    self.metrics,
                    self.size,
                )
            });
        browser_command_block_gutter_hit_json_for_hit(hit)
    }

    pub fn last_completed_command_block_json(&self) -> Result<String, JsValue> {
        browser_last_completed_command_block_json_text(&self.shell_integration)
            .map_err(js_json_error)
    }

    pub fn selected_command_block_json(&self) -> Result<String, JsValue> {
        browser_selected_command_block_json_text(&self.shell_integration).map_err(js_json_error)
    }

    pub fn selected_command_block_text_ranges_json(&self) -> Result<String, JsValue> {
        browser_selected_command_block_text_ranges_json_text(&self.shell_integration)
            .map_err(js_json_error)
    }

    pub fn selected_command_block_text_json(&self) -> Result<String, JsValue> {
        browser_selected_command_block_text_json_text(&self.terminal, &self.shell_integration)
            .map_err(js_json_error)
    }

    pub fn command_block_copy_text(&self, command_id: String) -> String {
        browser_command_block_copy_text(&self.terminal, &self.shell_integration, &command_id)
            .unwrap_or_default()
    }

    pub fn select_latest_command_block_for_active_screen_json(
        &mut self,
    ) -> Result<String, JsValue> {
        let json = browser_select_latest_command_block_for_screen_json_text(
            &mut self.shell_integration,
            self.terminal.active_screen(),
        )
        .map_err(js_json_error)?;
        self.render_current_frame()?;
        Ok(json)
    }

    pub fn select_previous_command_block_for_active_screen_json(
        &mut self,
    ) -> Result<String, JsValue> {
        let json = browser_select_previous_command_block_for_screen_json_text(
            &mut self.shell_integration,
            self.terminal.active_screen(),
        )
        .map_err(js_json_error)?;
        self.render_current_frame()?;
        Ok(json)
    }

    pub fn select_next_command_block_for_active_screen_json(&mut self) -> Result<String, JsValue> {
        let json = browser_select_next_command_block_for_screen_json_text(
            &mut self.shell_integration,
            self.terminal.active_screen(),
        )
        .map_err(js_json_error)?;
        self.render_current_frame()?;
        Ok(json)
    }

    pub fn toggle_selected_command_block_fold_json(&mut self) -> Result<String, JsValue> {
        apply_command_block_command(
            &mut self.shell_integration,
            self.terminal.active_screen(),
            COMMAND_BLOCK_TOGGLE_FOLD_COMMAND_ID,
        );
        let json = browser_selected_command_block_json_text(&self.shell_integration)
            .map_err(js_json_error)?;
        self.render_current_frame()?;
        Ok(json)
    }

    pub fn clear_selected_command_block(&mut self) -> Result<(), JsValue> {
        self.shell_integration.clear_selection();
        self.render_current_frame()
    }

    pub fn resize_message_json(&self) -> Result<String, JsValue> {
        self.app
            .transport()
            .resize_message()
            .to_json()
            .map_err(js_json_error)
    }

    pub fn push_gateway_output(&mut self, text: String) -> Result<(), JsValue> {
        self.app.transport_mut().push_output(text.into_bytes());
        self.poll_gateway_events()
    }

    pub fn push_gateway_message_json(&mut self, json: String) -> Result<(), JsValue> {
        let message = BrowserGatewayServerMessage::from_json(&json).map_err(js_json_error)?;
        self.app.transport_mut().push_server_message(message);
        self.poll_gateway_events()
    }

    pub fn resize(
        &mut self,
        css_width: f64,
        css_height: f64,
        device_pixel_ratio: f64,
    ) -> Result<bool, JsValue> {
        let sizing = browser_canvas_sizing(css_width, css_height, device_pixel_ratio);
        let grid_changed = sizing.grid != self.size;

        self.backing_width = sizing.backing_width;
        self.backing_height = sizing.backing_height;
        self.device_pixel_ratio = sizing.device_pixel_ratio;
        self.metrics = sizing.metrics;
        self.renderer
            .resize(sizing.backing_width, sizing.backing_height);
        self.app.set_cell_metrics(sizing.metrics);

        if grid_changed {
            self.size = sizing.grid;
            self.terminal.resize(sizing.grid);
            self.app.resize_transport(sizing.grid).map_err(js_error)?;
            self.refresh_search_after_terminal_change();
        }

        self.render_current_frame()?;
        Ok(grid_changed)
    }

    pub fn grid_rows(&self) -> u32 {
        u32::from(self.size.rows)
    }

    pub fn grid_cols(&self) -> u32 {
        u32::from(self.size.cols)
    }

    pub fn backing_width(&self) -> u32 {
        self.backing_width
    }

    pub fn backing_height(&self) -> u32 {
        self.backing_height
    }

    pub fn device_pixel_ratio(&self) -> f64 {
        self.device_pixel_ratio
    }

    pub fn transport_grid_text(&self) -> String {
        let size = self.app.transport().size();
        format!("{}x{}", size.rows, size.cols)
    }

    pub fn title(&self) -> String {
        browser_document_title(self.app.title())
    }

    pub fn screen_text(&self) -> String {
        render_snapshot_text(&self.terminal.snapshot())
    }

    pub fn frame_stats_json(&self) -> Result<String, JsValue> {
        browser_frame_stats_json(
            self.last_frame_stats,
            self.last_renderer_cache_stats,
            self.last_renderer_timing_stats,
        )
        .map_err(js_json_error)
    }

    pub fn synchronized_output_enabled(&self) -> bool {
        self.terminal.synchronized_output_enabled()
    }

    pub fn flush_synchronized_output(&mut self) -> Result<bool, JsValue> {
        if !self.terminal.synchronized_output_enabled() {
            return Ok(false);
        }

        self.render_current_frame()?;
        Ok(true)
    }

    pub fn selected_text(&self) -> String {
        self.terminal.selected_text().unwrap_or_default()
    }

    pub fn clear_selection(&mut self) -> Result<bool, JsValue> {
        if self.terminal.snapshot().selection.is_none() {
            return Ok(false);
        }

        self.terminal.set_selection(None);
        self.render_current_frame()?;
        Ok(true)
    }

    pub fn selection_range_text(&self) -> String {
        let Some(selection) = self.terminal.snapshot().selection else {
            return String::new();
        };
        format!(
            "{}:{}-{}:{}",
            selection.start.row, selection.start.col, selection.end.row, selection.end.col
        )
    }
}

#[cfg(target_arch = "wasm32")]
impl WittyWebSession {
    fn render_current_frame(&mut self) -> Result<(), JsValue> {
        let visible_row_anchors = self.terminal.visible_row_anchors();
        let mut snapshot = self.terminal.take_snapshot();
        snapshot.search_highlights = self.visible_search_highlights();
        snapshot.hovered_hyperlink = self.hovered_hyperlink;
        let cursor = snapshot.cursor;
        self.app.set_snapshot(snapshot);
        sync_browser_document_title(self.app.title())?;
        let mut frame = self.app.frame_plan();
        let reused_rows = frame.stats.reused_rows;
        let rebuilt_rows = frame.stats.rebuilt_rows;
        apply_command_block_gutter_overlay_with_anchors(
            &mut frame,
            &self.shell_integration,
            self.terminal.active_screen(),
            &visible_row_anchors,
            self.metrics,
            self.size,
        );
        apply_command_block_gutter_hover_overlay_with_anchors(
            &mut frame,
            &self.shell_integration,
            self.terminal.active_screen(),
            &visible_row_anchors,
            self.hovered_command_block_id,
            self.metrics,
            self.size,
        );
        apply_command_block_selection_overlay_with_anchors(
            &mut frame,
            &self.shell_integration,
            self.terminal.active_screen(),
            &visible_row_anchors,
            self.metrics,
            self.size,
        );
        apply_command_block_status_label_overlay_with_anchors(
            &mut frame,
            &self.shell_integration,
            self.terminal.active_screen(),
            &visible_row_anchors,
            self.hovered_command_block_id,
            self.metrics,
            self.size,
        );
        apply_command_block_action_menu_overlay(
            &mut frame,
            &self.command_block_action_menu,
            &self.shell_integration,
            self.terminal.active_screen(),
            &visible_row_anchors,
            self.metrics,
            self.size,
        );
        if self.text_input_target() == BrowserTextInputTarget::Terminal {
            apply_ime_preedit_overlay(
                &mut frame,
                &self.ime_composition,
                cursor,
                self.metrics,
                self.size,
            );
        }
        apply_command_block_folded_frame_remap_with_anchors(
            &mut frame,
            &self.shell_integration,
            self.terminal.active_screen(),
            &visible_row_anchors,
            self.metrics,
            self.size,
        );
        apply_browser_command_palette_overlay(
            &mut frame,
            &self.command_palette,
            (self.text_input_target() == BrowserTextInputTarget::CommandPalette)
                .then_some(&self.ime_composition),
            self.app.commands(),
            self.metrics,
            self.size,
        );
        frame.refresh_stats_with_rows(self.size.rows, self.size.cols, reused_rows, rebuilt_rows);
        self.last_frame_stats = frame.stats;
        self.renderer.render(&frame).map_err(js_error)?;
        self.last_renderer_cache_stats = self.renderer.cache_stats();
        self.last_renderer_timing_stats = self.renderer.timing_stats();
        Ok(())
    }

    fn text_input_target(&self) -> BrowserTextInputTarget {
        if self.command_palette.is_open() {
            BrowserTextInputTarget::CommandPalette
        } else if self.terminal_search.is_open() {
            BrowserTextInputTarget::Search
        } else {
            BrowserTextInputTarget::Terminal
        }
    }

    fn terminal_pixel_point_for_offset(&self, offset_x: f64, offset_y: f64) -> Option<PixelPoint> {
        let visual_point =
            browser_pixel_point_for_offset(offset_x, offset_y, self.device_pixel_ratio)?;
        command_block_folded_visual_pixel_to_terminal_pixel_with_anchors(
            &self.shell_integration,
            self.terminal.active_screen(),
            &self.terminal.visible_row_anchors(),
            visual_point,
            self.metrics,
            self.size,
        )
    }

    fn terminal_cell_point_for_offset(&self, offset_x: f64, offset_y: f64) -> Option<CellPoint> {
        self.terminal_pixel_point_for_offset(offset_x, offset_y)
            .map(|point| browser_cell_point_for_pixel_point(point, self.metrics, self.size))
    }

    fn terminal_pixel_mouse_position_for_offset(
        &self,
        offset_x: f64,
        offset_y: f64,
    ) -> Option<PixelMousePosition> {
        self.terminal_pixel_point_for_offset(offset_x, offset_y)
            .and_then(browser_pixel_position_for_pixel_point)
    }

    fn compact_visual_cell_for_terminal_cell(&self, point: CellPoint) -> Option<CellPoint> {
        let compact_rows = self.shell_integration.folded_compact_visual_rows(
            self.terminal.active_screen(),
            &self.terminal.visible_row_anchors(),
            self.size.rows,
        );
        let row = compact_rows.get(usize::from(point.row))?;
        let compact_row = row.compact_row?;
        Some(CellPoint::new(compact_row, point.col))
    }

    fn active_ime_cursor(&self) -> CursorState {
        let cursor = self.terminal.snapshot().cursor;
        match self.text_input_target() {
            BrowserTextInputTarget::Terminal => {
                let position = browser_terminal_ime_cursor_cell(
                    cursor.position,
                    &self.ime_composition,
                    self.size,
                );
                CursorState {
                    position: self
                        .compact_visual_cell_for_terminal_cell(position)
                        .unwrap_or(position),
                    ..cursor
                }
            }
            BrowserTextInputTarget::Search => CursorState {
                position: browser_search_ime_cursor_cell(
                    &self.terminal_search,
                    &self.ime_composition,
                    self.size,
                ),
                ..cursor
            },
            BrowserTextInputTarget::CommandPalette => CursorState {
                position: browser_command_palette_ime_cursor_cell(
                    &self.command_palette,
                    &self.ime_composition,
                    self.size,
                )
                .unwrap_or(cursor.position),
                ..cursor
            },
        }
    }

    fn invoke_browser_command(&mut self, command_id: &str) -> Result<(), JsValue> {
        if apply_browser_search_command(&mut self.terminal, &mut self.terminal_search, command_id) {
            self.command_block_action_menu.close();
            self.render_current_frame()?;
            return Ok(());
        }
        if command_id == COMMAND_BLOCK_ACTION_MENU_COMMAND_ID {
            self.command_block_action_menu
                .open_for_selected_block(&self.shell_integration);
            self.render_current_frame()?;
            return Ok(());
        }
        if command_block_copy_target(command_id).is_some() {
            self.render_current_frame()?;
            return Ok(());
        }
        if apply_command_block_command(
            &mut self.shell_integration,
            self.terminal.active_screen(),
            command_id,
        ) {
            if command_id == COMMAND_BLOCK_CLEAR_SELECTION_COMMAND_ID {
                self.command_block_action_menu.close();
            }
            self.render_current_frame()?;
            return Ok(());
        }

        let context = self
            .shell_integration
            .command_invocation_context_for_screen(self.terminal.active_screen());
        match self
            .app
            .invoke_command_with_context(command_id, serde_json::Value::Null, context)
            .map_err(js_error)
        {
            Ok(actions) => {
                if self.feed_browser_command_feedback(&actions) {
                    self.refresh_search_after_terminal_change();
                }
                self.render_current_frame()
            }
            Err(error) => {
                let message = format!(
                    "\r\n[command failed: {}]\r\n",
                    error.as_string().unwrap_or_default()
                );
                self.terminal.feed(message.as_bytes());
                self.refresh_search_after_terminal_change();
                self.render_current_frame()
            }
        }
    }

    fn feed_browser_command_feedback(&mut self, actions: &[PluginAction]) -> bool {
        let mut changed = false;
        for action in actions {
            match action {
                PluginAction::ShowMessage { message } => {
                    let message = format!("\r\n[plugin message: {message}]\r\n");
                    self.terminal.feed(message.as_bytes());
                    changed = true;
                }
                PluginAction::RegisterCommand(_)
                | PluginAction::WriteTerminal { .. }
                | PluginAction::RenderOverlay(_) => {}
            }
        }
        changed
    }

    fn visible_search_highlights(&self) -> Vec<witty_core::SearchHighlight> {
        if !self.terminal_search.is_open() {
            return Vec::new();
        }

        self.terminal.visible_search_highlights(
            self.terminal_search.matches(),
            self.terminal_search.active_match(),
        )
    }

    fn refresh_search_after_terminal_change(&mut self) {
        if !self.terminal_search.is_open() {
            return;
        }

        self.terminal_search
            .rebuild(&self.terminal.search_text_rows());
        self.scroll_to_active_search_match();
    }

    fn scroll_to_active_search_match(&mut self) {
        let Some(active) = self.terminal_search.active_match() else {
            return;
        };

        self.terminal
            .scroll_to_search_match(active.row, BROWSER_SEARCH_SCROLL_BUFFER_ROWS);
    }

    fn poll_gateway_events(&mut self) -> Result<(), JsValue> {
        let mut changed = false;
        loop {
            match self.app.poll_transport().map_err(js_error)? {
                Some(TransportEvent::Output(bytes)) => {
                    self.terminal.feed(&bytes);
                    changed = true;
                }
                Some(TransportEvent::Exit { code }) => {
                    let message = format!("\r\n[gateway exited: {code:?}]\r\n");
                    self.terminal.feed(message.as_bytes());
                    changed = true;
                }
                Some(TransportEvent::Error(message)) => {
                    let message = format!("\r\n[gateway error: {message}]\r\n");
                    self.terminal.feed(message.as_bytes());
                    changed = true;
                }
                None => break,
            }
        }

        if changed {
            self.refresh_search_after_terminal_change();
            if !self.terminal.synchronized_output_enabled() {
                self.render_current_frame()?;
            }
        }
        Ok(())
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_document_title(title: Option<&str>) -> String {
    title
        .filter(|title| !title.is_empty())
        .unwrap_or(DEFAULT_BROWSER_TITLE)
        .to_owned()
}

fn browser_search_status_text(search: &TerminalSearch) -> String {
    browser_search_status_text_with_ime(search, None)
}

fn browser_search_status_text_with_ime(
    search: &TerminalSearch,
    ime: Option<&ImeComposition>,
) -> String {
    if !search.is_open() {
        return "closed".to_owned();
    }

    let query = browser_search_display_query(search, ime);
    format!(
        "Find: {} {} {}",
        query,
        browser_search_options_label(search),
        browser_search_count_label(search)
    )
}

fn browser_search_display_query(search: &TerminalSearch, ime: Option<&ImeComposition>) -> String {
    let mut query = search.query().to_owned();
    if let Some(ime) = ime.filter(|ime| ime.is_active()) {
        query.push_str(ime.preedit());
    }
    query
}

fn browser_search_count_label(search: &TerminalSearch) -> String {
    if let Some(error) = search.error_text() {
        return error;
    }

    if search.query().is_empty() {
        return "0/0".to_owned();
    }

    if search.match_count() == 0 {
        return "No results".to_owned();
    }

    let active = search.active_index().map(|index| index + 1).unwrap_or(0);
    format!("{active}/{}", search.match_count())
}

fn browser_search_options_label(search: &TerminalSearch) -> String {
    let options = search.options();
    let case = if options.case_sensitive { "Aa" } else { "aa" };
    let pattern = if options.regex { ".*" } else { "lit" };
    let scope = if options.whole_word { "word" } else { "part" };
    let normalization = if options.normalize_nfc { "nfc" } else { "raw" };
    format!("[{case} {pattern} {scope} {normalization}]")
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_command_palette_status_text(palette: &CommandPalette) -> String {
    if !palette.is_open() {
        return "closed".to_owned();
    }

    let selected = palette
        .selected_command()
        .map(|command| command.id.as_str())
        .unwrap_or("none");
    let selected_position = palette
        .selected_index()
        .map(|index| index.saturating_add(1))
        .unwrap_or(0);
    let query = if palette.query().is_empty() {
        String::new()
    } else {
        format!("{} ", palette.query())
    };
    format!(
        "Command Palette: {query}{}/{} {}",
        selected_position,
        palette.filtered_count(),
        selected
    )
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_command_palette_visible_items_json_text(
    palette: &CommandPalette,
    limit: usize,
) -> serde_json::Result<String> {
    let items = palette
        .visible_items(limit)
        .into_iter()
        .map(|item| {
            serde_json::json!({
                "id": item.command.id.as_str(),
                "title": item.command.title.as_str(),
                "sourcePlugin": item.command.source_plugin.as_str(),
                "filteredIndex": item.filtered_index,
                "position": item.filtered_index + 1,
                "selected": item.selected,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&items)
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_command_block_action_menu_status_text(menu: &CommandBlockActionMenu) -> String {
    if !menu.is_open() {
        return "Command Block Actions: closed".to_owned();
    }

    let count = menu.visible_items().len();
    let position = menu
        .selected_index()
        .map(|index| index + 1)
        .unwrap_or_default();
    let command_id = menu.selected_command_id().unwrap_or("");
    format!("Command Block Actions: {position}/{count} {command_id}")
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_command_block_action_menu_visible_items_json_text(
    menu: &CommandBlockActionMenu,
) -> serde_json::Result<String> {
    let items = menu
        .visible_items()
        .into_iter()
        .map(|item| {
            serde_json::json!({
                "id": item.item.id,
                "title": item.item.title,
                "index": item.index,
                "position": item.index + 1,
                "selected": item.selected,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&items)
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_default_command_registrations() -> Vec<CommandRegistration> {
    let mut commands = Vec::new();
    commands.push(browser_about_command_registration());
    commands.extend(search_command_registrations());
    commands.push(browser_web_echo_command_registration());
    commands.extend(command_block_command_registrations());
    commands
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_about_command_registration() -> CommandRegistration {
    CommandRegistration {
        id: "witty.about".to_owned(),
        title: "About Witty".to_owned(),
        source_plugin: "builtin".to_owned(),
    }
}

fn browser_web_echo_command_registration() -> CommandRegistration {
    CommandRegistration {
        id: "web.echo".to_owned(),
        title: "Web Echo".to_owned(),
        source_plugin: "web".to_owned(),
    }
}

#[cfg(target_arch = "wasm32")]
fn apply_browser_search_command(
    terminal: &mut BasicTerminal,
    search: &mut TerminalSearch,
    command_id: &str,
) -> bool {
    match command_id {
        SEARCH_OPEN_COMMAND_ID => {
            let selected_text = terminal.selected_text();
            search.open(&terminal.search_text_rows(), selected_text.as_deref());
            scroll_browser_terminal_to_active_search_match(terminal, search);
            true
        }
        SEARCH_CLOSE_COMMAND_ID => {
            search.close();
            true
        }
        SEARCH_NEXT_COMMAND_ID => {
            search.repeat_next(&terminal.search_text_rows());
            scroll_browser_terminal_to_active_search_match(terminal, search);
            true
        }
        SEARCH_PREVIOUS_COMMAND_ID => {
            search.repeat_previous(&terminal.search_text_rows());
            scroll_browser_terminal_to_active_search_match(terminal, search);
            true
        }
        _ => false,
    }
}

#[cfg(target_arch = "wasm32")]
fn scroll_browser_terminal_to_active_search_match(
    terminal: &mut BasicTerminal,
    search: &TerminalSearch,
) {
    let Some(active) = search.active_match() else {
        return;
    };

    terminal.scroll_to_search_match(active.row, BROWSER_SEARCH_SCROLL_BUFFER_ROWS);
}

#[cfg(any(test, target_arch = "wasm32"))]
fn apply_browser_command_palette_overlay(
    frame: &mut FramePlan,
    palette: &CommandPalette,
    ime: Option<&ImeComposition>,
    commands: &[CommandRegistration],
    metrics: CellMetrics,
    grid_size: GridSize,
) {
    if !palette.is_open() {
        return;
    }

    let Some(panel) = browser_palette_panel(grid_size, palette.filtered_count()) else {
        return;
    };
    let panel_origin = browser_cell_origin(panel.start, metrics);
    let panel_size = PixelSize {
        width: f32::from(panel.cols) * metrics.cell.width,
        height: f32::from(panel.rows) * metrics.cell.height,
    };

    frame.glyphs.clear();
    frame.selection.clear();
    frame.cursor = None;

    frame.backgrounds.push(RectBatchItem {
        origin: panel_origin,
        size: panel_size,
        color: Rgba::rgb(18, 22, 28),
    });

    browser_push_palette_text(
        frame,
        panel,
        metrics,
        0,
        1,
        &browser_palette_title_with_ime(palette.query(), ime, panel.cols.saturating_sub(2)),
        Rgba::rgb(220, 230, 235),
    );

    let items = palette.visible_items(panel.item_rows);
    if items.is_empty() && panel.item_rows > 0 {
        browser_push_palette_text(
            frame,
            panel,
            metrics,
            1,
            1,
            "No matching commands",
            Rgba::rgb(150, 160, 165),
        );
        return;
    }

    for (offset, item) in items.iter().enumerate() {
        let row = offset as u16 + 1;
        if item.selected {
            frame.backgrounds.push(RectBatchItem {
                origin: browser_cell_origin(
                    CellPoint::new(panel.start.row + row, panel.start.col),
                    metrics,
                ),
                size: PixelSize {
                    width: f32::from(panel.cols) * metrics.cell.width,
                    height: metrics.cell.height,
                },
                color: Rgba::rgb(42, 76, 118),
            });
        }

        let text = browser_palette_item_text(
            item.command,
            item.selected,
            commands,
            panel.cols.saturating_sub(2),
        );
        browser_push_palette_text(
            frame,
            panel,
            metrics,
            row,
            1,
            &text,
            Rgba::rgb(238, 242, 245),
        );
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BrowserPalettePanel {
    start: CellPoint,
    cols: u16,
    rows: u16,
    item_rows: usize,
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_palette_panel(
    grid_size: GridSize,
    filtered_count: usize,
) -> Option<BrowserPalettePanel> {
    if grid_size.rows == 0 || grid_size.cols == 0 {
        return None;
    }

    let cols = grid_size.cols;
    let panel_cols = if cols > 80 {
        BROWSER_COMMAND_PALETTE_MAX_COLS
    } else {
        cols.saturating_sub(4).max(1)
    };
    let start_col = cols.saturating_sub(panel_cols) / 2;
    let max_item_rows = usize::from(grid_size.rows.saturating_sub(1))
        .min(BROWSER_COMMAND_PALETTE_MAX_VISIBLE_ITEMS);
    let item_rows = if max_item_rows == 0 {
        0
    } else {
        filtered_count.min(max_item_rows).max(1)
    };
    let panel_rows = u16::try_from(item_rows + 1)
        .unwrap_or(u16::MAX)
        .min(grid_size.rows);
    let start_row = if grid_size.rows > panel_rows + 4 {
        2
    } else {
        grid_size.rows.saturating_sub(panel_rows) / 2
    };

    Some(BrowserPalettePanel {
        start: CellPoint::new(start_row, start_col),
        cols: panel_cols,
        rows: panel_rows,
        item_rows,
    })
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_palette_item_text(
    command: &CommandRegistration,
    selected: bool,
    commands: &[CommandRegistration],
    width: u16,
) -> String {
    let marker = if selected { ">" } else { " " };
    let base = format!("{marker} {}  {}", command.title, command.id);
    let text = match browser_shortcut_label_for_command(command, commands) {
        Some(shortcut) => format!("{base}  {shortcut}"),
        None => base,
    };

    browser_truncate_cells(&text, width)
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_shortcut_label_for_command(
    command: &CommandRegistration,
    commands: &[CommandRegistration],
) -> Option<&'static str> {
    if command.id == "witty.about" && browser_has_command(commands, "witty.about") {
        return Some("F1");
    }

    let first_external = commands
        .iter()
        .find(|candidate| candidate.source_plugin != "builtin")?;
    (first_external.id == command.id).then_some("F2")
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_command_shortcut_for_key(key: &str, commands: &[CommandRegistration]) -> Option<String> {
    match key {
        "F1" if browser_has_command(commands, "witty.about") => Some("witty.about".to_owned()),
        "F2" => commands
            .iter()
            .find(|command| command.source_plugin != "builtin")
            .map(|command| command.id.clone()),
        _ => None,
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_has_command(commands: &[CommandRegistration], command_id: &str) -> bool {
    commands.iter().any(|command| command.id == command_id)
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_push_palette_text(
    frame: &mut FramePlan,
    panel: BrowserPalettePanel,
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
        origin: browser_cell_origin(
            CellPoint::new(panel.start.row + row_offset, panel.start.col + col_offset),
            metrics,
        ),
        text: text.to_owned(),
        color,
        style_flags: CellFlags::default(),
    });
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_command_palette_ime_cursor_cell(
    palette: &CommandPalette,
    composition: &ImeComposition,
    grid_size: GridSize,
) -> Option<CellPoint> {
    let panel = browser_palette_panel(grid_size, palette.filtered_count())?;
    let visible_width = browser_text_cell_width("Command Palette  ")
        .saturating_add(browser_text_cell_width(palette.query()))
        .saturating_add(browser_ime_preedit_caret_cell_width(composition));
    let available_width = panel.cols.saturating_sub(2);
    let col = panel
        .start
        .col
        .saturating_add(1)
        .saturating_add(visible_width.min(available_width))
        .min(grid_size.cols.saturating_sub(1));

    Some(CellPoint::new(panel.start.row, col))
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_palette_title_with_ime(query: &str, ime: Option<&ImeComposition>, width: u16) -> String {
    let query = browser_palette_display_query(query, ime);
    let title = if query.is_empty() {
        "Command Palette".to_owned()
    } else {
        format!("Command Palette  {query}")
    };

    browser_truncate_cells(&title, width)
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_palette_display_query(query: &str, ime: Option<&ImeComposition>) -> String {
    let mut display = query.to_owned();
    if let Some(ime) = ime.filter(|ime| ime.is_active()) {
        display.push_str(ime.preedit());
    }
    display
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_cell_origin(point: CellPoint, metrics: CellMetrics) -> PixelPoint {
    PixelPoint {
        x: metrics.padding.x + f32::from(point.col) * metrics.cell.width,
        y: metrics.padding.y + f32::from(point.row) * metrics.cell.height,
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_truncate_cells(text: &str, width: u16) -> String {
    if browser_text_cell_width(text) <= width {
        return text.to_owned();
    }
    if width <= 3 {
        return ".".repeat(usize::from(width));
    }

    let target_width = width.saturating_sub(3);
    let mut output = String::new();
    let mut current_width = 0u16;
    for ch in text.chars() {
        let ch_width = u16::from(terminal_char_width(ch));
        if current_width.saturating_add(ch_width) > target_width {
            break;
        }
        output.push(ch);
        current_width = current_width.saturating_add(ch_width);
    }
    output.push_str("...");
    output
}

#[cfg(any(test, target_arch = "wasm32"))]
fn render_snapshot_text(snapshot: &RenderSnapshot) -> String {
    snapshot
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(target_arch = "wasm32")]
fn sync_browser_document_title(title: Option<&str>) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("window is unavailable"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("document is unavailable"))?;
    document.set_title(&browser_document_title(title));
    Ok(())
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_entry_frame() -> FramePlan {
    let size = GridSize::new(4, 16);
    let mut terminal = BasicTerminal::new(size);
    let transport = MockTransport::new(size);
    let mut app = TerminalApp::new(transport, size);

    terminal.feed(b"Witty web\r\ncanvas ready");
    app.set_snapshot(terminal.take_snapshot());
    app.frame_plan()
}

#[cfg(target_arch = "wasm32")]
fn js_error(error: anyhow::Error) -> JsValue {
    JsValue::from_str(&format!("{error:#}"))
}

#[cfg(target_arch = "wasm32")]
fn js_json_error(error: serde_json::Error) -> JsValue {
    JsValue::from_str(&format!("{error:#}"))
}

fn encode_browser_key_input(
    key: &str,
    text: &str,
    control: bool,
    modes: TerminalInputModes,
) -> Option<Vec<u8>> {
    encode_browser_key_input_with_metadata(
        key,
        text,
        BrowserKeyModifiers {
            control,
            ..BrowserKeyModifiers::default()
        },
        "",
        0,
        modes,
    )
}

fn encode_browser_key_input_with_metadata(
    key: &str,
    text: &str,
    modifiers: BrowserKeyModifiers,
    code: &str,
    location: u32,
    modes: TerminalInputModes,
) -> Option<Vec<u8>> {
    encode_browser_key_input_with_metadata_and_event_type(
        key,
        text,
        modifiers,
        code,
        location,
        BrowserKeyEventType::Press,
        modes,
    )
}

fn encode_browser_key_input_with_metadata_and_event_type(
    key: &str,
    text: &str,
    modifiers: BrowserKeyModifiers,
    code: &str,
    location: u32,
    event_type: BrowserKeyEventType,
    modes: TerminalInputModes,
) -> Option<Vec<u8>> {
    encode_browser_terminal_key_input(
        browser_terminal_key_input_from_event(key, text, modifiers, code, location, event_type),
        modes,
    )
}

fn browser_terminal_key_input_from_event<'a>(
    key: &'a str,
    text: &'a str,
    modifiers: BrowserKeyModifiers,
    code: &'a str,
    location: u32,
    event_type: BrowserKeyEventType,
) -> BrowserTerminalKeyInput<'a> {
    let modifier_key = browser_modifier_key_from_event(key, code, location);
    BrowserTerminalKeyInput {
        key,
        text,
        modifiers: modifiers.with_modifier_key_event_state(modifier_key, event_type),
        keypad_key: browser_keypad_key_from_event(key, text, code, location),
        base_layout_key: browser_base_layout_key_from_code(code),
        modifier_key,
        event_type,
    }
}

fn browser_keyboard_protocol_diagnostic_json_line(
    key: &str,
    text: &str,
    modifiers: BrowserKeyModifiers,
    code: &str,
    location: u32,
    event_type: BrowserKeyEventType,
) -> String {
    let input =
        browser_terminal_key_input_from_event(key, text, modifiers, code, location, event_type);
    let kitty_disambiguate_modes = TerminalInputModes {
        kitty_keyboard_flags: KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES,
        ..TerminalInputModes::default()
    };
    let kitty_all_feature_modes = TerminalInputModes {
        kitty_keyboard_flags: KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES
            | KITTY_KEYBOARD_REPORT_ALTERNATE_KEYS
            | KITTY_KEYBOARD_REPORT_ASSOCIATED_TEXT
            | KITTY_KEYBOARD_REPORT_EVENT_TYPES,
        ..TerminalInputModes::default()
    };

    serde_json::json!({
        "key": key,
        "text": text,
        "code": code,
        "location": location,
        "eventType": format!("{:?}", event_type),
        "inputModifiers": browser_key_modifiers_json(modifiers),
        "witty": {
            "modifiers": browser_key_modifiers_json(input.modifiers),
            "modifierKey": debug_option_json(input.modifier_key),
            "keypadKey": debug_option_json(input.keypad_key),
            "baseLayoutKey": input.base_layout_key.map(|ch| ch.to_string()),
        },
        "encoded": {
            "legacy": diagnostic_bytes_json(encode_browser_terminal_key_input(
                input,
                TerminalInputModes::default(),
            )),
            "kittyDisambiguate": diagnostic_bytes_json(encode_browser_terminal_key_input(
                input,
                kitty_disambiguate_modes,
            )),
            "kittyAllFeatures": diagnostic_bytes_json(encode_browser_terminal_key_input(
                input,
                kitty_all_feature_modes,
            )),
        },
    })
    .to_string()
}

fn browser_key_modifiers_json(modifiers: BrowserKeyModifiers) -> serde_json::Value {
    serde_json::json!({
        "control": modifiers.control,
        "shift": modifiers.shift,
        "alt": modifiers.alt,
        "meta": modifiers.meta,
        "hyper": modifiers.hyper,
    })
}

fn debug_option_json<T: std::fmt::Debug>(value: Option<T>) -> serde_json::Value {
    value
        .map(|value| serde_json::Value::String(format!("{value:?}")))
        .unwrap_or(serde_json::Value::Null)
}

fn diagnostic_bytes_json(bytes: Option<Vec<u8>>) -> serde_json::Value {
    bytes
        .as_deref()
        .map(|bytes| {
            serde_json::json!({
                "hex": diagnostic_bytes_hex(bytes),
                "escaped": diagnostic_escaped_bytes(bytes),
            })
        })
        .unwrap_or(serde_json::Value::Null)
}

fn diagnostic_bytes_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn diagnostic_escaped_bytes(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| match byte {
            b'\n' => "\\n".to_owned(),
            b'\r' => "\\r".to_owned(),
            b'\t' => "\\t".to_owned(),
            0x1b => "\\x1b".to_owned(),
            0x20..=0x7e => char::from(*byte).to_string(),
            _ => format!("\\x{byte:02x}"),
        })
        .collect()
}

#[cfg_attr(not(any(test, target_arch = "wasm32")), allow(dead_code))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum BrowserKeyEventType {
    #[default]
    Press,
    Repeat,
    Release,
}

impl BrowserKeyEventType {
    fn from_browser_event_type(value: u8) -> Self {
        match value {
            2 => Self::Repeat,
            3 => Self::Release,
            _ => Self::Press,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct BrowserKeyModifiers {
    control: bool,
    shift: bool,
    alt: bool,
    meta: bool,
    hyper: bool,
}

impl BrowserKeyModifiers {
    fn from_browser_mask(control: bool, mask: u8) -> Self {
        Self {
            control,
            shift: mask & 0b001 != 0,
            alt: mask & 0b010 != 0,
            meta: mask & 0b100 != 0,
            hyper: false,
        }
    }

    fn with_modifier_key_event_state(
        mut self,
        modifier_key: Option<BrowserModifierKey>,
        event_type: BrowserKeyEventType,
    ) -> Self {
        let Some(modifier_key) = modifier_key else {
            return self;
        };
        let active = event_type != BrowserKeyEventType::Release;
        match modifier_key {
            BrowserModifierKey::LeftShift | BrowserModifierKey::RightShift => self.shift = active,
            BrowserModifierKey::LeftAlt | BrowserModifierKey::RightAlt => self.alt = active,
            BrowserModifierKey::LeftControl | BrowserModifierKey::RightControl => {
                self.control = active
            }
            BrowserModifierKey::LeftSuper | BrowserModifierKey::RightSuper => self.meta = active,
            BrowserModifierKey::LeftHyper | BrowserModifierKey::RightHyper => self.hyper = active,
        }
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BrowserModifierKey {
    LeftShift,
    RightShift,
    LeftAlt,
    RightAlt,
    LeftControl,
    RightControl,
    LeftSuper,
    RightSuper,
    LeftHyper,
    RightHyper,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BrowserKeypadKey {
    Digit(u8),
    Decimal,
    Comma,
    Add,
    Subtract,
    Multiply,
    Divide,
    Enter,
    Equal,
    Left,
    Right,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Insert,
    Delete,
    Begin,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BrowserTerminalKeyInput<'a> {
    key: &'a str,
    text: &'a str,
    modifiers: BrowserKeyModifiers,
    keypad_key: Option<BrowserKeypadKey>,
    base_layout_key: Option<char>,
    modifier_key: Option<BrowserModifierKey>,
    event_type: BrowserKeyEventType,
}

fn encode_browser_terminal_key_input(
    input: BrowserTerminalKeyInput<'_>,
    modes: TerminalInputModes,
) -> Option<Vec<u8>> {
    encode_core_terminal_key_input(core_terminal_key_input_from_browser(input), modes)
}

fn core_terminal_key_input_from_browser(
    input: BrowserTerminalKeyInput<'_>,
) -> CoreTerminalKeyInput<'_> {
    CoreTerminalKeyInput {
        key: core_terminal_key_from_browser(input.key),
        text: (!input.text.is_empty()).then_some(input.text),
        modifiers: core_terminal_key_modifiers_from_browser(input.modifiers),
        keypad_key: input.keypad_key.map(core_terminal_keypad_key_from_browser),
        base_layout_key: input.base_layout_key,
        modifier_key: input
            .modifier_key
            .map(core_terminal_modifier_key_from_browser),
        event_type: core_terminal_key_event_type_from_browser(input.event_type),
    }
}

fn core_terminal_key_event_type_from_browser(
    event_type: BrowserKeyEventType,
) -> CoreTerminalKeyEventType {
    match event_type {
        BrowserKeyEventType::Press => CoreTerminalKeyEventType::Press,
        BrowserKeyEventType::Repeat => CoreTerminalKeyEventType::Repeat,
        BrowserKeyEventType::Release => CoreTerminalKeyEventType::Release,
    }
}

fn core_terminal_key_modifiers_from_browser(
    modifiers: BrowserKeyModifiers,
) -> CoreTerminalKeyModifiers {
    CoreTerminalKeyModifiers {
        control: modifiers.control,
        shift: modifiers.shift,
        alt: modifiers.alt,
        meta: modifiers.meta,
        hyper: modifiers.hyper,
        kitty_meta: false,
    }
}

fn core_terminal_modifier_key_from_browser(
    modifier_key: BrowserModifierKey,
) -> CoreTerminalModifierKey {
    match modifier_key {
        BrowserModifierKey::LeftShift => CoreTerminalModifierKey::LeftShift,
        BrowserModifierKey::RightShift => CoreTerminalModifierKey::RightShift,
        BrowserModifierKey::LeftAlt => CoreTerminalModifierKey::LeftAlt,
        BrowserModifierKey::RightAlt => CoreTerminalModifierKey::RightAlt,
        BrowserModifierKey::LeftControl => CoreTerminalModifierKey::LeftControl,
        BrowserModifierKey::RightControl => CoreTerminalModifierKey::RightControl,
        BrowserModifierKey::LeftSuper => CoreTerminalModifierKey::LeftSuper,
        BrowserModifierKey::RightSuper => CoreTerminalModifierKey::RightSuper,
        BrowserModifierKey::LeftHyper => CoreTerminalModifierKey::LeftHyper,
        BrowserModifierKey::RightHyper => CoreTerminalModifierKey::RightHyper,
    }
}

fn core_terminal_keypad_key_from_browser(keypad_key: BrowserKeypadKey) -> CoreTerminalKeypadKey {
    match keypad_key {
        BrowserKeypadKey::Digit(value) => CoreTerminalKeypadKey::Digit(value),
        BrowserKeypadKey::Decimal => CoreTerminalKeypadKey::Decimal,
        BrowserKeypadKey::Comma => CoreTerminalKeypadKey::Comma,
        BrowserKeypadKey::Add => CoreTerminalKeypadKey::Add,
        BrowserKeypadKey::Subtract => CoreTerminalKeypadKey::Subtract,
        BrowserKeypadKey::Multiply => CoreTerminalKeypadKey::Multiply,
        BrowserKeypadKey::Divide => CoreTerminalKeypadKey::Divide,
        BrowserKeypadKey::Enter => CoreTerminalKeypadKey::Enter,
        BrowserKeypadKey::Equal => CoreTerminalKeypadKey::Equal,
        BrowserKeypadKey::Left => CoreTerminalKeypadKey::Left,
        BrowserKeypadKey::Right => CoreTerminalKeypadKey::Right,
        BrowserKeypadKey::Up => CoreTerminalKeypadKey::Up,
        BrowserKeypadKey::Down => CoreTerminalKeypadKey::Down,
        BrowserKeypadKey::PageUp => CoreTerminalKeypadKey::PageUp,
        BrowserKeypadKey::PageDown => CoreTerminalKeypadKey::PageDown,
        BrowserKeypadKey::Home => CoreTerminalKeypadKey::Home,
        BrowserKeypadKey::End => CoreTerminalKeypadKey::End,
        BrowserKeypadKey::Insert => CoreTerminalKeypadKey::Insert,
        BrowserKeypadKey::Delete => CoreTerminalKeypadKey::Delete,
        BrowserKeypadKey::Begin => CoreTerminalKeypadKey::Begin,
    }
}

fn core_terminal_key_from_browser(key: &str) -> CoreTerminalKey<'_> {
    if let Some(named_key) = core_terminal_named_key_from_browser(key) {
        return CoreTerminalKey::Named(named_key);
    }
    let mut chars = key.chars();
    if chars.next().is_some() && chars.next().is_none() {
        CoreTerminalKey::Character(key)
    } else {
        CoreTerminalKey::Unidentified
    }
}

fn core_terminal_named_key_from_browser(key: &str) -> Option<TerminalNamedKey> {
    match key {
        "Enter" => Some(TerminalNamedKey::Enter),
        "Tab" => Some(TerminalNamedKey::Tab),
        "Backspace" => Some(TerminalNamedKey::Backspace),
        "Escape" => Some(TerminalNamedKey::Escape),
        "ArrowUp" => Some(TerminalNamedKey::ArrowUp),
        "ArrowDown" => Some(TerminalNamedKey::ArrowDown),
        "ArrowRight" => Some(TerminalNamedKey::ArrowRight),
        "ArrowLeft" => Some(TerminalNamedKey::ArrowLeft),
        "Home" => Some(TerminalNamedKey::Home),
        "End" => Some(TerminalNamedKey::End),
        "Insert" => Some(TerminalNamedKey::Insert),
        "PageUp" => Some(TerminalNamedKey::PageUp),
        "PageDown" => Some(TerminalNamedKey::PageDown),
        "Delete" => Some(TerminalNamedKey::Delete),
        "F1" => Some(TerminalNamedKey::F1),
        "F2" => Some(TerminalNamedKey::F2),
        "F3" => Some(TerminalNamedKey::F3),
        "F4" => Some(TerminalNamedKey::F4),
        "F5" => Some(TerminalNamedKey::F5),
        "F6" => Some(TerminalNamedKey::F6),
        "F7" => Some(TerminalNamedKey::F7),
        "F8" => Some(TerminalNamedKey::F8),
        "F9" => Some(TerminalNamedKey::F9),
        "F10" => Some(TerminalNamedKey::F10),
        "F11" => Some(TerminalNamedKey::F11),
        "F12" => Some(TerminalNamedKey::F12),
        "F13" => Some(TerminalNamedKey::F13),
        "F14" => Some(TerminalNamedKey::F14),
        "F15" => Some(TerminalNamedKey::F15),
        "F16" => Some(TerminalNamedKey::F16),
        "F17" => Some(TerminalNamedKey::F17),
        "F18" => Some(TerminalNamedKey::F18),
        "F19" => Some(TerminalNamedKey::F19),
        "F20" => Some(TerminalNamedKey::F20),
        "F21" => Some(TerminalNamedKey::F21),
        "F22" => Some(TerminalNamedKey::F22),
        "F23" => Some(TerminalNamedKey::F23),
        "F24" => Some(TerminalNamedKey::F24),
        "F25" => Some(TerminalNamedKey::F25),
        "F26" => Some(TerminalNamedKey::F26),
        "F27" => Some(TerminalNamedKey::F27),
        "F28" => Some(TerminalNamedKey::F28),
        "F29" => Some(TerminalNamedKey::F29),
        "F30" => Some(TerminalNamedKey::F30),
        "F31" => Some(TerminalNamedKey::F31),
        "F32" => Some(TerminalNamedKey::F32),
        "F33" => Some(TerminalNamedKey::F33),
        "F34" => Some(TerminalNamedKey::F34),
        "F35" => Some(TerminalNamedKey::F35),
        "CapsLock" => Some(TerminalNamedKey::CapsLock),
        "ScrollLock" => Some(TerminalNamedKey::ScrollLock),
        "NumLock" => Some(TerminalNamedKey::NumLock),
        "PrintScreen" => Some(TerminalNamedKey::PrintScreen),
        "Pause" => Some(TerminalNamedKey::Pause),
        "ContextMenu" => Some(TerminalNamedKey::ContextMenu),
        "MediaPlay" => Some(TerminalNamedKey::MediaPlay),
        "MediaPause" => Some(TerminalNamedKey::MediaPause),
        "MediaPlayPause" => Some(TerminalNamedKey::MediaPlayPause),
        "MediaStop" => Some(TerminalNamedKey::MediaStop),
        "MediaFastForward" => Some(TerminalNamedKey::MediaFastForward),
        "MediaRewind" => Some(TerminalNamedKey::MediaRewind),
        "MediaTrackNext" => Some(TerminalNamedKey::MediaTrackNext),
        "MediaTrackPrevious" => Some(TerminalNamedKey::MediaTrackPrevious),
        "MediaRecord" => Some(TerminalNamedKey::MediaRecord),
        "AudioVolumeDown" => Some(TerminalNamedKey::AudioVolumeDown),
        "AudioVolumeUp" => Some(TerminalNamedKey::AudioVolumeUp),
        "AudioVolumeMute" => Some(TerminalNamedKey::AudioVolumeMute),
        "AltGraph" => Some(TerminalNamedKey::AltGraph),
        _ => None,
    }
}

fn browser_modifier_key_from_event(
    key: &str,
    code: &str,
    location: u32,
) -> Option<BrowserModifierKey> {
    if let Some(modifier_key) = browser_modifier_key_from_code(code) {
        return Some(modifier_key);
    }

    browser_modifier_key_from_location(key, location)
}

fn browser_modifier_key_from_code(code: &str) -> Option<BrowserModifierKey> {
    match code {
        "ShiftLeft" => Some(BrowserModifierKey::LeftShift),
        "ShiftRight" => Some(BrowserModifierKey::RightShift),
        "AltLeft" => Some(BrowserModifierKey::LeftAlt),
        "AltRight" => Some(BrowserModifierKey::RightAlt),
        "ControlLeft" => Some(BrowserModifierKey::LeftControl),
        "ControlRight" => Some(BrowserModifierKey::RightControl),
        "MetaLeft" => Some(BrowserModifierKey::LeftSuper),
        "MetaRight" => Some(BrowserModifierKey::RightSuper),
        "HyperLeft" => Some(BrowserModifierKey::LeftHyper),
        "HyperRight" => Some(BrowserModifierKey::RightHyper),
        _ => None,
    }
}

fn browser_modifier_key_from_location(key: &str, location: u32) -> Option<BrowserModifierKey> {
    match (key, location) {
        ("Shift", 1) => Some(BrowserModifierKey::LeftShift),
        ("Shift", 2) => Some(BrowserModifierKey::RightShift),
        ("Alt", 1) => Some(BrowserModifierKey::LeftAlt),
        ("Alt", 2) => Some(BrowserModifierKey::RightAlt),
        ("Control", 1) => Some(BrowserModifierKey::LeftControl),
        ("Control", 2) => Some(BrowserModifierKey::RightControl),
        ("Meta", 1) | ("Super", 1) => Some(BrowserModifierKey::LeftSuper),
        ("Meta", 2) | ("Super", 2) => Some(BrowserModifierKey::RightSuper),
        ("Hyper", 1) => Some(BrowserModifierKey::LeftHyper),
        ("Hyper", 2) => Some(BrowserModifierKey::RightHyper),
        _ => None,
    }
}

fn browser_base_layout_key_from_code(code: &str) -> Option<char> {
    match code {
        "KeyA" => Some('a'),
        "KeyB" => Some('b'),
        "KeyC" => Some('c'),
        "KeyD" => Some('d'),
        "KeyE" => Some('e'),
        "KeyF" => Some('f'),
        "KeyG" => Some('g'),
        "KeyH" => Some('h'),
        "KeyI" => Some('i'),
        "KeyJ" => Some('j'),
        "KeyK" => Some('k'),
        "KeyL" => Some('l'),
        "KeyM" => Some('m'),
        "KeyN" => Some('n'),
        "KeyO" => Some('o'),
        "KeyP" => Some('p'),
        "KeyQ" => Some('q'),
        "KeyR" => Some('r'),
        "KeyS" => Some('s'),
        "KeyT" => Some('t'),
        "KeyU" => Some('u'),
        "KeyV" => Some('v'),
        "KeyW" => Some('w'),
        "KeyX" => Some('x'),
        "KeyY" => Some('y'),
        "KeyZ" => Some('z'),
        "Digit0" => Some('0'),
        "Digit1" => Some('1'),
        "Digit2" => Some('2'),
        "Digit3" => Some('3'),
        "Digit4" => Some('4'),
        "Digit5" => Some('5'),
        "Digit6" => Some('6'),
        "Digit7" => Some('7'),
        "Digit8" => Some('8'),
        "Digit9" => Some('9'),
        "Backquote" => Some('`'),
        "Backslash" => Some('\\'),
        "BracketLeft" => Some('['),
        "BracketRight" => Some(']'),
        "Comma" => Some(','),
        "Equal" => Some('='),
        "IntlBackslash" => Some('\\'),
        "Minus" => Some('-'),
        "Period" => Some('.'),
        "Quote" => Some('\''),
        "Semicolon" => Some(';'),
        "Slash" => Some('/'),
        "Space" => Some(' '),
        _ => None,
    }
}

fn browser_keypad_key_from_event(
    key: &str,
    text: &str,
    code: &str,
    location: u32,
) -> Option<BrowserKeypadKey> {
    if let Some(keypad_key) = browser_keypad_navigation_key(key) {
        if location == 3 || browser_is_numpad_code(code) {
            return Some(keypad_key);
        }
    }

    if let Some(keypad_key) = browser_keypad_key_from_code(code) {
        return Some(keypad_key);
    }

    if location != 3 {
        return None;
    }

    browser_keypad_key_from_text(key).or_else(|| browser_keypad_key_from_text(text))
}

fn browser_keypad_navigation_key(key: &str) -> Option<BrowserKeypadKey> {
    match key {
        "ArrowLeft" => Some(BrowserKeypadKey::Left),
        "ArrowRight" => Some(BrowserKeypadKey::Right),
        "ArrowUp" => Some(BrowserKeypadKey::Up),
        "ArrowDown" => Some(BrowserKeypadKey::Down),
        "PageUp" => Some(BrowserKeypadKey::PageUp),
        "PageDown" => Some(BrowserKeypadKey::PageDown),
        "Home" => Some(BrowserKeypadKey::Home),
        "End" => Some(BrowserKeypadKey::End),
        "Insert" => Some(BrowserKeypadKey::Insert),
        "Delete" => Some(BrowserKeypadKey::Delete),
        "Clear" => Some(BrowserKeypadKey::Begin),
        _ => None,
    }
}

fn browser_is_numpad_code(code: &str) -> bool {
    browser_keypad_key_from_code(code).is_some()
        || matches!(
            code,
            "NumpadBackspace"
                | "NumpadClear"
                | "NumpadClearEntry"
                | "NumpadHash"
                | "NumpadMemoryAdd"
                | "NumpadMemoryClear"
                | "NumpadMemoryRecall"
                | "NumpadMemoryStore"
                | "NumpadMemorySubtract"
                | "NumpadParenLeft"
                | "NumpadParenRight"
                | "NumpadStar"
        )
}

fn browser_keypad_key_from_code(code: &str) -> Option<BrowserKeypadKey> {
    match code {
        "Numpad0" => Some(BrowserKeypadKey::Digit(0)),
        "Numpad1" => Some(BrowserKeypadKey::Digit(1)),
        "Numpad2" => Some(BrowserKeypadKey::Digit(2)),
        "Numpad3" => Some(BrowserKeypadKey::Digit(3)),
        "Numpad4" => Some(BrowserKeypadKey::Digit(4)),
        "Numpad5" => Some(BrowserKeypadKey::Digit(5)),
        "Numpad6" => Some(BrowserKeypadKey::Digit(6)),
        "Numpad7" => Some(BrowserKeypadKey::Digit(7)),
        "Numpad8" => Some(BrowserKeypadKey::Digit(8)),
        "Numpad9" => Some(BrowserKeypadKey::Digit(9)),
        "NumpadDecimal" => Some(BrowserKeypadKey::Decimal),
        "NumpadComma" => Some(BrowserKeypadKey::Comma),
        "NumpadAdd" => Some(BrowserKeypadKey::Add),
        "NumpadSubtract" => Some(BrowserKeypadKey::Subtract),
        "NumpadMultiply" => Some(BrowserKeypadKey::Multiply),
        "NumpadDivide" => Some(BrowserKeypadKey::Divide),
        "NumpadEnter" => Some(BrowserKeypadKey::Enter),
        "NumpadEqual" => Some(BrowserKeypadKey::Equal),
        _ => None,
    }
}

fn browser_keypad_key_from_text(text: &str) -> Option<BrowserKeypadKey> {
    match text {
        "0" => Some(BrowserKeypadKey::Digit(0)),
        "1" => Some(BrowserKeypadKey::Digit(1)),
        "2" => Some(BrowserKeypadKey::Digit(2)),
        "3" => Some(BrowserKeypadKey::Digit(3)),
        "4" => Some(BrowserKeypadKey::Digit(4)),
        "5" => Some(BrowserKeypadKey::Digit(5)),
        "6" => Some(BrowserKeypadKey::Digit(6)),
        "7" => Some(BrowserKeypadKey::Digit(7)),
        "8" => Some(BrowserKeypadKey::Digit(8)),
        "9" => Some(BrowserKeypadKey::Digit(9)),
        "." | "Decimal" => Some(BrowserKeypadKey::Decimal),
        "," | "Separator" => Some(BrowserKeypadKey::Comma),
        "+" => Some(BrowserKeypadKey::Add),
        "-" => Some(BrowserKeypadKey::Subtract),
        "*" => Some(BrowserKeypadKey::Multiply),
        "/" => Some(BrowserKeypadKey::Divide),
        "Enter" => Some(BrowserKeypadKey::Enter),
        "=" => Some(BrowserKeypadKey::Equal),
        _ => None,
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Debug, Eq, PartialEq)]
struct BrowserHyperlinkActivationTarget {
    hit: bool,
    allowed: bool,
    uri: Option<String>,
    reason: Option<String>,
}

#[cfg(any(test, target_arch = "wasm32"))]
impl BrowserHyperlinkActivationTarget {
    fn no_hit() -> Self {
        Self {
            hit: false,
            allowed: false,
            uri: None,
            reason: None,
        }
    }

    fn allowed(uri: String) -> Self {
        Self {
            hit: true,
            allowed: true,
            uri: Some(uri),
            reason: None,
        }
    }

    fn blocked(reason: String) -> Self {
        Self {
            hit: true,
            allowed: false,
            uri: None,
            reason: Some(reason),
        }
    }

    fn to_json(&self) -> String {
        serde_json::json!({
            "hit": self.hit,
            "allowed": self.allowed,
            "uri": self.uri.as_deref().unwrap_or_default(),
            "reason": self.reason.as_deref().unwrap_or_default(),
        })
        .to_string()
    }
}

#[cfg(test)]
fn browser_hyperlink_activation_target(
    snapshot: &RenderSnapshot,
    offset_x: f64,
    offset_y: f64,
    device_pixel_ratio: f64,
    metrics: CellMetrics,
    size: GridSize,
) -> BrowserHyperlinkActivationTarget {
    let Some(link) = browser_hyperlink_for_offset(
        snapshot,
        offset_x,
        offset_y,
        device_pixel_ratio,
        metrics,
        size,
    ) else {
        return BrowserHyperlinkActivationTarget::no_hit();
    };

    browser_hyperlink_activation_target_for_link(link)
}

#[cfg(target_arch = "wasm32")]
fn browser_hyperlink_activation_target_for_point(
    snapshot: &RenderSnapshot,
    point: CellPoint,
) -> BrowserHyperlinkActivationTarget {
    let Some(link) = snapshot.hyperlink_at(point) else {
        return BrowserHyperlinkActivationTarget::no_hit();
    };

    browser_hyperlink_activation_target_for_link(link)
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_hyperlink_activation_target_for_link(
    link: &TerminalHyperlink,
) -> BrowserHyperlinkActivationTarget {
    match validate_external_url(&link.uri) {
        Ok(()) => BrowserHyperlinkActivationTarget::allowed(link.uri.clone()),
        Err(error) => BrowserHyperlinkActivationTarget::blocked(error.to_string()),
    }
}

#[cfg(test)]
fn browser_hyperlink_for_offset(
    snapshot: &RenderSnapshot,
    offset_x: f64,
    offset_y: f64,
    device_pixel_ratio: f64,
    metrics: CellMetrics,
    size: GridSize,
) -> Option<&TerminalHyperlink> {
    let point =
        browser_cell_point_for_offset(offset_x, offset_y, device_pixel_ratio, metrics, size);
    snapshot.hyperlink_at(point)
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BrowserMouseEventKind {
    PointerDown,
    PointerUp,
    PointerMove,
    Wheel,
    LocalWheel,
}

#[cfg(any(test, target_arch = "wasm32"))]
impl BrowserMouseEventKind {
    fn from_browser_kind(kind: &str) -> Option<Self> {
        match kind {
            "pointerdown" | "press" => Some(Self::PointerDown),
            "pointerup" | "release" => Some(Self::PointerUp),
            "pointermove" | "motion" => Some(Self::PointerMove),
            "wheel" => Some(Self::Wheel),
            "localwheel" => Some(Self::LocalWheel),
            _ => None,
        }
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, PartialEq)]
struct BrowserMouseInput {
    kind: BrowserMouseEventKind,
    button: i16,
    buttons: u16,
    offset_x: f64,
    offset_y: f64,
    delta_y: f64,
    modifiers: MouseModifiers,
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct BrowserMouseReportState {
    pressed_button: Option<MouseButtonCode>,
    last_reported_cell: Option<CellPoint>,
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct BrowserLocalSelectionState {
    anchor: Option<CellPoint>,
}

#[cfg(any(test, target_arch = "wasm32"))]
impl BrowserLocalSelectionState {
    fn begin(&mut self, terminal: &mut BasicTerminal, point: CellPoint, click_count: u32) {
        let is_word_selection = click_count >= 2;
        let range = if is_word_selection {
            terminal
                .word_range_at(point)
                .unwrap_or_else(|| browser_collapsed_range(point))
        } else {
            browser_collapsed_range(point)
        };

        self.anchor = (!is_word_selection).then_some(point);
        terminal.set_selection(Some(range));
    }

    fn update(&mut self, terminal: &mut BasicTerminal, point: CellPoint) -> bool {
        let Some(anchor) = self.anchor else {
            return false;
        };

        terminal.set_selection(Some(browser_ordered_cell_range(anchor, point)));
        true
    }

    fn end(&mut self) -> bool {
        let was_active = self.anchor.is_some();
        self.anchor = None;
        was_active
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
impl BrowserMouseReportState {
    #[cfg(test)]
    fn encode(
        &mut self,
        input: BrowserMouseInput,
        modes: TerminalMouseModes,
        metrics: CellMetrics,
        size: GridSize,
        device_pixel_ratio: f64,
    ) -> Option<Vec<u8>> {
        if !modes.reports_mouse() {
            return None;
        }

        let cell = browser_cell_point_for_offset(
            input.offset_x,
            input.offset_y,
            device_pixel_ratio,
            metrics,
            size,
        );
        let pixel =
            browser_pixel_position_for_offset(input.offset_x, input.offset_y, device_pixel_ratio);

        self.encode_resolved(input, modes, cell, pixel)
    }

    fn encode_resolved(
        &mut self,
        input: BrowserMouseInput,
        modes: TerminalMouseModes,
        cell: CellPoint,
        pixel: Option<PixelMousePosition>,
    ) -> Option<Vec<u8>> {
        if !modes.reports_mouse() {
            return None;
        }

        match input.kind {
            BrowserMouseEventKind::PointerDown => {
                let button = browser_mouse_button_code(input.button)?;
                self.pressed_button = Some(button);
                self.encode_event(
                    MouseEventKind::Press,
                    button,
                    cell,
                    pixel,
                    input.modifiers,
                    modes,
                )
            }
            BrowserMouseEventKind::PointerUp => {
                let button = browser_mouse_button_code(input.button)?;
                let bytes = self.encode_event(
                    MouseEventKind::Release,
                    button,
                    cell,
                    pixel,
                    input.modifiers,
                    modes,
                );
                if self.pressed_button == Some(button) {
                    self.pressed_button = None;
                }
                bytes
            }
            BrowserMouseEventKind::PointerMove => {
                if self.last_reported_cell == Some(cell) {
                    return None;
                }
                let button = self
                    .pressed_button
                    .filter(|button| browser_buttons_include(input.buttons, *button))
                    .or_else(|| browser_active_button_from_buttons(input.buttons))
                    .unwrap_or(MouseButtonCode::None);
                self.encode_event(
                    MouseEventKind::Motion,
                    button,
                    cell,
                    pixel,
                    input.modifiers,
                    modes,
                )
            }
            BrowserMouseEventKind::Wheel => {
                let button = if input.delta_y < 0.0 {
                    MouseButtonCode::WheelUp
                } else if input.delta_y > 0.0 {
                    MouseButtonCode::WheelDown
                } else {
                    return None;
                };
                self.encode_event(
                    MouseEventKind::Wheel,
                    button,
                    cell,
                    pixel,
                    input.modifiers,
                    modes,
                )
            }
            BrowserMouseEventKind::LocalWheel => None,
        }
    }

    fn encode_event(
        &mut self,
        kind: MouseEventKind,
        button: MouseButtonCode,
        cell: CellPoint,
        pixel: Option<PixelMousePosition>,
        modifiers: MouseModifiers,
        modes: TerminalMouseModes,
    ) -> Option<Vec<u8>> {
        let bytes = encode_terminal_mouse_event(
            TerminalMouseEvent {
                kind,
                button,
                cell,
                pixel,
                modifiers,
            },
            modes,
        )?;
        self.last_reported_cell = Some(cell);
        Some(bytes)
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_collapsed_range(point: CellPoint) -> CellRange {
    CellRange {
        start: point,
        end: point,
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_ordered_cell_range(anchor: CellPoint, current: CellPoint) -> CellRange {
    if (current.row, current.col) < (anchor.row, anchor.col) {
        CellRange {
            start: current,
            end: anchor,
        }
    } else {
        CellRange {
            start: anchor,
            end: current,
        }
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_mouse_button_code(button: i16) -> Option<MouseButtonCode> {
    match button {
        0 => Some(MouseButtonCode::Left),
        1 => Some(MouseButtonCode::Middle),
        2 => Some(MouseButtonCode::Right),
        _ => None,
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_active_button_from_buttons(buttons: u16) -> Option<MouseButtonCode> {
    if browser_buttons_include(buttons, MouseButtonCode::Left) {
        Some(MouseButtonCode::Left)
    } else if browser_buttons_include(buttons, MouseButtonCode::Middle) {
        Some(MouseButtonCode::Middle)
    } else if browser_buttons_include(buttons, MouseButtonCode::Right) {
        Some(MouseButtonCode::Right)
    } else {
        None
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_buttons_include(buttons: u16, button: MouseButtonCode) -> bool {
    let mask = match button {
        MouseButtonCode::Left => 0b001,
        MouseButtonCode::Right => 0b010,
        MouseButtonCode::Middle => 0b100,
        MouseButtonCode::None | MouseButtonCode::WheelUp | MouseButtonCode::WheelDown => 0,
    };
    mask != 0 && buttons & mask != 0
}

#[cfg(test)]
fn browser_cell_point_for_offset(
    offset_x: f64,
    offset_y: f64,
    device_pixel_ratio: f64,
    metrics: CellMetrics,
    size: GridSize,
) -> CellPoint {
    let x = browser_backing_axis(offset_x, device_pixel_ratio).unwrap_or(0.0);
    let y = browser_backing_axis(offset_y, device_pixel_ratio).unwrap_or(0.0);

    browser_cell_point_for_pixel_point(
        PixelPoint {
            x: x as f32,
            y: y as f32,
        },
        metrics,
        size,
    )
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_cell_point_for_pixel_point(
    point: PixelPoint,
    metrics: CellMetrics,
    size: GridSize,
) -> CellPoint {
    let max_row = size.rows.saturating_sub(1);
    let max_col = size.cols.saturating_sub(1);
    CellPoint::new(
        browser_cell_axis(
            f64::from(point.y),
            metrics.padding.y,
            metrics.cell.height,
            max_row,
        ),
        browser_cell_axis(
            f64::from(point.x),
            metrics.padding.x,
            metrics.cell.width,
            max_col,
        ),
    )
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_cell_axis(position: f64, padding: f32, cell_extent: f32, max_index: u16) -> u16 {
    if cell_extent <= 0.0 || !position.is_finite() {
        return 0;
    }

    ((position - f64::from(padding)) / f64::from(cell_extent))
        .floor()
        .clamp(0.0, f64::from(max_index)) as u16
}

#[cfg(test)]
fn browser_pixel_position_for_offset(
    offset_x: f64,
    offset_y: f64,
    device_pixel_ratio: f64,
) -> Option<PixelMousePosition> {
    browser_pixel_position_for_pixel_point(PixelPoint {
        x: browser_backing_axis(offset_x, device_pixel_ratio)? as f32,
        y: browser_backing_axis(offset_y, device_pixel_ratio)? as f32,
    })
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_pixel_position_for_pixel_point(point: PixelPoint) -> Option<PixelMousePosition> {
    Some(PixelMousePosition::new(
        f64::from(point.x).floor().clamp(0.0, f64::from(u16::MAX)) as u16,
        f64::from(point.y).floor().clamp(0.0, f64::from(u16::MAX)) as u16,
    ))
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_backing_axis(offset: f64, device_pixel_ratio: f64) -> Option<f64> {
    if !offset.is_finite() {
        return None;
    }
    Some(offset * sane_device_pixel_ratio(device_pixel_ratio))
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_scroll_lines_for_wheel_delta(
    delta_y: f64,
    delta_mode: u32,
    metrics: CellMetrics,
    size: GridSize,
    device_pixel_ratio: f64,
) -> i16 {
    match delta_mode {
        1 => browser_rounded_scroll_lines(-delta_y),
        2 => browser_rounded_scroll_lines(-delta_y * f64::from(size.rows.max(1))),
        _ => {
            let cell_height = browser_css_cell_height(metrics, device_pixel_ratio);
            browser_rounded_scroll_lines(-delta_y / cell_height)
        }
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_css_cell_height(metrics: CellMetrics, device_pixel_ratio: f64) -> f64 {
    let cell_height = f64::from(metrics.cell.height) / sane_device_pixel_ratio(device_pixel_ratio);
    if cell_height.is_finite() && cell_height > 0.0 {
        cell_height
    } else {
        f64::from(CellMetrics::default().cell.height)
    }
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_rounded_scroll_lines(value: f64) -> i16 {
    if !value.is_finite() {
        return 0;
    }

    let rounded = value.round();
    let effective = if rounded == 0.0 && value != 0.0 {
        value.signum()
    } else {
        rounded
    };

    effective.clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_echo_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut echo = Vec::new();
    for byte in bytes {
        match byte {
            b'\r' => echo.extend_from_slice(b"\r\n> "),
            b'\t' => echo.push(b'\t'),
            0x20..=0x7e => echo.push(*byte),
            _ => {}
        }
    }
    echo
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct BrowserImeCommitResult {
    changed: bool,
    committed_text: Option<String>,
}

#[cfg(any(test, target_arch = "wasm32"))]
fn apply_browser_ime_preedit(
    composition: &mut ImeComposition,
    text: String,
    caret: Option<(usize, usize)>,
) -> bool {
    let before = (
        composition.is_enabled(),
        composition.preedit().to_owned(),
        composition.caret(),
    );
    composition.set_preedit(text, caret);
    let after = (
        composition.is_enabled(),
        composition.preedit().to_owned(),
        composition.caret(),
    );
    before != after
}

#[cfg(any(test, target_arch = "wasm32"))]
fn apply_browser_ime_commit(
    composition: &mut ImeComposition,
    text: String,
) -> BrowserImeCommitResult {
    let was_active = composition.is_active();
    let committed_text = composition.commit_text(text);
    BrowserImeCommitResult {
        changed: was_active || committed_text.is_some(),
        committed_text,
    }
}

#[cfg(target_arch = "wasm32")]
fn clear_browser_ime_preedit(composition: &mut ImeComposition) -> bool {
    let changed = composition.is_active();
    composition.clear_preedit();
    changed
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_ime_caret(text: &str, caret_start: i32, caret_end: i32) -> Option<(usize, usize)> {
    if text.is_empty() || caret_start < 0 || caret_end < 0 {
        return None;
    }

    Some((caret_start as usize, caret_end as usize))
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_search_ime_cursor_cell(
    search: &TerminalSearch,
    composition: &ImeComposition,
    grid_size: GridSize,
) -> CellPoint {
    let row = grid_size.rows.saturating_sub(1);
    let visible_width = browser_text_cell_width("Find: ")
        .saturating_add(browser_text_cell_width(search.query()))
        .saturating_add(browser_ime_preedit_caret_cell_width(composition));
    let col = 1u16
        .saturating_add(visible_width)
        .min(grid_size.cols.saturating_sub(1));

    CellPoint::new(row, col)
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_terminal_ime_cursor_cell(
    terminal_cursor: CellPoint,
    composition: &ImeComposition,
    grid_size: GridSize,
) -> CellPoint {
    let row = terminal_cursor.row.min(grid_size.rows.saturating_sub(1));
    let col = terminal_cursor
        .col
        .saturating_add(composition.preedit_caret_cell_width())
        .min(grid_size.cols.saturating_sub(1));

    CellPoint::new(row, col)
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_ime_preedit_caret_cell_width(composition: &ImeComposition) -> u16 {
    composition.preedit_caret_cell_width()
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_text_cell_width(text: &str) -> u16 {
    text.chars().fold(0u16, |width, ch| {
        width.saturating_add(u16::from(terminal_char_width(ch)))
    })
}

#[cfg(any(test, target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, PartialEq)]
struct BrowserCursorCssRect {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

#[cfg(any(test, target_arch = "wasm32"))]
fn browser_cursor_css_rect(
    cursor: CursorState,
    metrics: CellMetrics,
    device_pixel_ratio: f64,
) -> BrowserCursorCssRect {
    let device_pixel_ratio = sane_device_pixel_ratio(device_pixel_ratio);
    BrowserCursorCssRect {
        left: f64::from(metrics.padding.x + f32::from(cursor.position.col) * metrics.cell.width)
            / device_pixel_ratio,
        top: f64::from(metrics.padding.y + f32::from(cursor.position.row) * metrics.cell.height)
            / device_pixel_ratio,
        width: f64::from(metrics.cell.width.max(1.0)) / device_pixel_ratio,
        height: f64::from(metrics.cell.height.max(1.0)) / device_pixel_ratio,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct BrowserCanvasSizing {
    backing_width: u32,
    backing_height: u32,
    device_pixel_ratio: f64,
    grid: GridSize,
    metrics: CellMetrics,
}

impl BrowserCanvasSizing {
    fn report(self) -> BrowserCanvasResizeReport {
        BrowserCanvasResizeReport {
            backing_width: self.backing_width,
            backing_height: self.backing_height,
            device_pixel_ratio: self.device_pixel_ratio,
            grid: self.grid,
            metrics: self.metrics,
        }
    }
}

fn browser_canvas_sizing(
    css_width: f64,
    css_height: f64,
    device_pixel_ratio: f64,
) -> BrowserCanvasSizing {
    let device_pixel_ratio = sane_device_pixel_ratio(device_pixel_ratio);
    let backing_width = backing_extent(css_width, device_pixel_ratio);
    let backing_height = backing_extent(css_height, device_pixel_ratio);
    let metrics = scaled_cell_metrics(device_pixel_ratio as f32);
    let grid = grid_size_for_backing_extent(backing_width, backing_height, metrics);

    BrowserCanvasSizing {
        backing_width,
        backing_height,
        device_pixel_ratio,
        grid,
        metrics,
    }
}

fn sane_device_pixel_ratio(device_pixel_ratio: f64) -> f64 {
    if device_pixel_ratio.is_finite() && device_pixel_ratio > 0.0 {
        device_pixel_ratio
    } else {
        1.0
    }
}

fn backing_extent(css_extent: f64, device_pixel_ratio: f64) -> u32 {
    (css_extent.max(1.0) * device_pixel_ratio)
        .ceil()
        .clamp(1.0, f64::from(u32::MAX)) as u32
}

fn scaled_cell_metrics(device_pixel_ratio: f32) -> CellMetrics {
    let base = CellMetrics::default();
    CellMetrics {
        cell: witty_render_wgpu::PixelSize {
            width: base.cell.width * device_pixel_ratio,
            height: base.cell.height * device_pixel_ratio,
        },
        padding: witty_render_wgpu::PixelPoint {
            x: base.padding.x * device_pixel_ratio,
            y: base.padding.y * device_pixel_ratio,
        },
    }
}

fn grid_size_for_backing_extent(width: u32, height: u32, metrics: CellMetrics) -> GridSize {
    GridSize::new(
        grid_axis_cells(height as f32, metrics.padding.y, metrics.cell.height),
        grid_axis_cells(width as f32, metrics.padding.x, metrics.cell.width),
    )
}

fn grid_axis_cells(extent: f32, padding: f32, cell_extent: f32) -> u16 {
    let available = (extent - padding * 2.0).max(cell_extent);
    (available / cell_extent)
        .floor()
        .clamp(1.0, f32::from(u16::MAX)) as u16
}

#[cfg(target_arch = "wasm32")]
struct BrowserBuiltInCommandsPlugin;

#[cfg(target_arch = "wasm32")]
impl BuiltInPlugin for BrowserBuiltInCommandsPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "builtin".to_owned(),
            name: "Built-in Commands".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            runtime: PluginRuntime::BuiltIn,
            permissions: PluginPermissions::default(),
        }
    }

    fn commands(&self) -> Vec<CommandRegistration> {
        vec![browser_about_command_registration()]
    }

    fn handle_event(&mut self, event: &PluginEvent) -> Result<Vec<PluginAction>> {
        let PluginEvent::CommandInvoked(invocation) = event else {
            return Ok(Vec::new());
        };
        if invocation.command_id != "witty.about" {
            return Ok(Vec::new());
        }

        Ok(vec![PluginAction::ShowMessage {
            message: "Witty Rust/wgpu browser prototype".to_owned(),
        }])
    }
}

struct WebEchoPlugin;

impl BuiltInPlugin for WebEchoPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            id: "web".to_owned(),
            name: "Web Echo".to_owned(),
            version: "0.1.0".to_owned(),
            runtime: PluginRuntime::BuiltIn,
            permissions: PluginPermissions {
                terminal_read: TerminalReadPermission::None,
                terminal_write: TerminalWritePermission::AllowSession,
                profile_read: false,
                profile_write: false,
                vault: VaultPermission::Deny,
                network: NetworkPermission::Deny,
            },
        }
    }

    fn commands(&self) -> Vec<CommandRegistration> {
        vec![browser_web_echo_command_registration()]
    }

    fn handle_event(&mut self, event: &PluginEvent) -> Result<Vec<PluginAction>> {
        if !matches!(event, PluginEvent::CommandInvoked(invocation) if invocation.command_id == "web.echo")
        {
            return Ok(Vec::new());
        }

        Ok(vec![PluginAction::WriteTerminal {
            bytes: b"echo from web\n".to_vec(),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use witty_core::{MouseEncodingMode, MouseTrackingMode};

    #[test]
    fn mock_replay_reuses_shared_terminal_and_frame_planner() {
        let report = mock_replay_report();

        assert_eq!(
            report,
            WebReplayReport {
                frames: 2,
                visible_rows: 4,
                visible_cols: 16,
                first_rebuilt_rows: 4,
                second_reused_rows: 2,
                second_rebuilt_rows: 2,
                second_glyph_runs: 4,
                second_glyph_chars: 22,
            }
        );
    }

    #[test]
    fn wasm_export_reports_mock_replay_glyph_chars() {
        assert_eq!(witty_web_mock_replay_glyph_chars(), 22);
    }

    #[test]
    fn browser_session_smoke_uses_terminal_app_with_mock_transport() {
        assert_eq!(
            browser_session_smoke_report(),
            WebSessionSmokeReport {
                commands: 1,
                frame_glyph_runs: 1,
                frame_glyph_chars: 10,
                frame_glyph_prepare_batches: 1,
                frame_max_glyph_run_chars: 10,
                written_bytes: 14,
            }
        );
    }

    #[test]
    fn browser_frame_stats_json_includes_glyph_run_budget() {
        let stats = FrameStats {
            visible_rows: 4,
            visible_cols: 16,
            glyph_runs: 2,
            glyph_chars: 130,
            glyph_prepare_batches: 2,
            max_glyph_run_chars: 120,
            rect_vertices: 18,
            rect_vertex_capacity: 32,
            rebuilt_rows: 4,
            ..FrameStats::default()
        };

        let renderer_cache = RendererCacheStats {
            text_buffers_reused: 1,
            text_buffers_rebuilt: 1,
            text_buffers_retired: 0,
            text_buffer_count: 2,
            text_renderer_count: 1,
            rect_vertex_capacity: 64,
        };
        let renderer_timing = RendererTimingStats {
            cpu_prepare_us: 15,
            text_buffer_sync_us: 3,
            glyph_prepare_us: 10,
            rect_vertex_sync_us: 2,
        };

        let json = browser_frame_stats_json(stats, renderer_cache, renderer_timing).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(value["visibleRows"], 4);
        assert_eq!(value["glyphRuns"], 2);
        assert_eq!(value["glyphChars"], 130);
        assert_eq!(value["glyphPrepareBatches"], 2);
        assert_eq!(value["maxGlyphRunChars"], 120);
        assert_eq!(value["textDecorationRects"], 0);
        assert_eq!(value["rectVertices"], 18);
        assert_eq!(value["rectVertexCapacity"], 32);
        assert_eq!(value["rendererTextBuffersReused"], 1);
        assert_eq!(value["rendererTextBuffersRebuilt"], 1);
        assert_eq!(value["rendererTextBuffersRetired"], 0);
        assert_eq!(value["rendererTextBufferCount"], 2);
        assert_eq!(value["rendererTextRendererCount"], 1);
        assert_eq!(value["rendererRectVertexCapacity"], 64);
        assert_eq!(value["rendererCpuPrepareUs"], 15);
        assert_eq!(value["rendererTextBufferSyncUs"], 3);
        assert_eq!(value["rendererGlyphPrepareUs"], 10);
        assert_eq!(value["rendererRectVertexSyncUs"], 2);
        assert_eq!(value["rebuiltRows"], 4);
    }

    #[test]
    fn wasm_export_reports_browser_session_written_bytes() {
        assert_eq!(witty_web_session_written_bytes(), 14);
    }

    #[test]
    fn browser_gateway_smoke_splits_outbound_and_remote_output() {
        assert_eq!(
            browser_gateway_smoke_report(),
            BrowserGatewaySmokeReport {
                outbound_bytes: 3,
                drained_bytes: 3,
                output_bytes: 9,
                resized: GridSize::new(8, 32),
            }
        );
    }

    #[test]
    fn browser_osc52_clipboard_actions_json_drains_actions_without_screen_text() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 32));
        terminal.feed(b"before\r\n\x1b]52;c;YnJvd3NlciBvc2M1Mg==\x07after");

        let json = drain_clipboard_write_actions_json(&mut terminal).unwrap();
        let writes: Vec<TerminalClipboardWrite> = serde_json::from_str(&json).unwrap();

        assert_eq!(
            writes,
            vec![TerminalClipboardWrite {
                selection: witty_core::TerminalClipboardSelection::Clipboard,
                text: "browser osc52".to_owned(),
                decoded_bytes: "browser osc52".len(),
            }]
        );
        assert_eq!(
            drain_clipboard_write_actions_json(&mut terminal).unwrap(),
            "[]"
        );
        let screen_text = render_snapshot_text(&terminal.snapshot());
        assert!(screen_text.contains("before"));
        assert!(screen_text.contains("after"));
        assert!(!screen_text.contains("browser osc52"));
    }

    #[test]
    fn browser_host_actions_forward_terminal_replies_and_return_clipboard_writes() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 32));
        let mut shell_integration = ShellIntegrationState::default();
        let mut replies = Vec::new();
        terminal.feed(
            b"\x1b[c\x1b[2;4H\x1b[6n\x1b[?1h\x1b[?1$p\
              \x1b[18t\
              \x1b]52;c;YnJvd3NlciBvc2M1Mg==\x07",
        );

        let json =
            drain_browser_host_actions_json(&mut terminal, &mut shell_integration, |bytes| {
                replies.extend_from_slice(bytes);
                Ok(())
            })
            .unwrap();
        let writes: Vec<TerminalClipboardWrite> = serde_json::from_str(&json).unwrap();

        assert_eq!(replies, b"\x1b[?1;2c\x1b[2;4R\x1b[?1;1$y\x1b[8;3;32t");
        assert_eq!(
            writes,
            vec![TerminalClipboardWrite {
                selection: witty_core::TerminalClipboardSelection::Clipboard,
                text: "browser osc52".to_owned(),
                decoded_bytes: "browser osc52".len(),
            }]
        );
        assert_eq!(
            drain_browser_host_actions_json(&mut terminal, &mut shell_integration, |_| Ok(()))
                .unwrap(),
            "[]"
        );
    }

    #[test]
    fn browser_bell_host_action_is_not_clipboard_or_terminal_reply() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 16));
        let mut shell_integration = ShellIntegrationState::default();
        let mut replies = Vec::new();
        terminal.feed(b"before\x07after");

        let json =
            drain_browser_host_actions_json(&mut terminal, &mut shell_integration, |bytes| {
                replies.extend_from_slice(bytes);
                Ok(())
            })
            .unwrap();

        assert_eq!(json, "[]");
        assert!(replies.is_empty());
        assert_eq!(shell_integration.completed_len(), 0);
        assert!(render_snapshot_text(&terminal.snapshot()).contains("beforeafter"));
    }

    #[test]
    fn browser_host_actions_update_shell_integration_blocks() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 32));
        let mut shell_integration = ShellIntegrationState::default();
        terminal.feed(
            b"\x1b]133;A\x1b\\$\x1b]133;B\x1b\\ echo\x1b]133;C\x1b\\\r\nok\x1b]133;D;0\x1b\\",
        );

        let json =
            drain_browser_host_actions_json(&mut terminal, &mut shell_integration, |_| Ok(()))
                .unwrap();

        assert_eq!(json, "[]");
        assert_eq!(shell_integration.completed_len(), 1);
        let block = &shell_integration.completed_blocks()[0];
        assert_eq!(block.command_start, Some(CellPoint::new(0, 1)));
        assert_eq!(block.output_start, Some(CellPoint::new(0, 6)));
        assert_eq!(block.finished_at, CellPoint::new(1, 2));
        assert_eq!(block.exit_code, Some(0));
        assert_eq!(terminal.active_screen(), TerminalScreen::Main);
        assert_eq!(terminal_screen_name(terminal.active_screen()), "main");
        assert!(
            browser_completed_command_blocks_json_text(&shell_integration)
                .unwrap()
                .contains("\"id\":0")
        );

        let active_screen_json = browser_completed_command_blocks_for_screen_json_text(
            &shell_integration,
            terminal.active_screen(),
        )
        .unwrap();
        let active_screen_blocks: serde_json::Value =
            serde_json::from_str(&active_screen_json).unwrap();
        assert_eq!(active_screen_blocks[0]["id"], 0);

        let visible_json = browser_visible_command_blocks_json_text(
            &shell_integration,
            terminal.active_screen(),
            2,
        )
        .unwrap();
        let visible: serde_json::Value = serde_json::from_str(&visible_json).unwrap();
        assert_eq!(visible[0]["id"], 0);
        assert_eq!(visible[0]["screen"], "main");

        let spans_json = browser_visible_command_block_row_spans_json_text(
            &shell_integration,
            terminal.active_screen(),
            2,
        )
        .unwrap();
        let spans: serde_json::Value = serde_json::from_str(&spans_json).unwrap();
        assert_eq!(spans[0]["id"], 0);
        assert_eq!(spans[0]["start_row"], 0);
        assert_eq!(spans[0]["end_row"], 1);
        assert!(
            browser_last_completed_command_block_json_text(&shell_integration)
                .unwrap()
                .contains("\"id\":0")
        );
        assert_eq!(
            browser_selected_command_block_json_text(&shell_integration).unwrap(),
            "null"
        );
        let selected_json = browser_select_latest_command_block_for_screen_json_text(
            &mut shell_integration,
            terminal.active_screen(),
        )
        .unwrap();
        let selected: serde_json::Value = serde_json::from_str(&selected_json).unwrap();
        assert_eq!(selected["id"], 0);
        let selected_text: serde_json::Value = serde_json::from_str(
            &browser_selected_command_block_text_json_text(&terminal, &shell_integration).unwrap(),
        )
        .unwrap();
        assert_eq!(selected_text["id"], 0);
        assert_eq!(selected_text["command"], " echo");
        assert_eq!(selected_text["output"], "\nok");
        assert_eq!(
            browser_command_block_copy_text(
                &terminal,
                &shell_integration,
                COMMAND_BLOCK_COPY_COMMAND_ID
            ),
            Some(" echo".to_owned())
        );
        assert_eq!(
            browser_command_block_copy_text(
                &terminal,
                &shell_integration,
                COMMAND_BLOCK_COPY_OUTPUT_ID
            ),
            Some("ok".to_owned())
        );
        assert_eq!(
            browser_select_previous_command_block_for_screen_json_text(
                &mut shell_integration,
                terminal.active_screen(),
            )
            .unwrap(),
            selected_json
        );
        assert_eq!(
            browser_select_next_command_block_for_screen_json_text(
                &mut shell_integration,
                terminal.active_screen(),
            )
            .unwrap(),
            selected_json
        );
    }

    #[test]
    fn browser_host_actions_update_current_directory_without_clipboard_json() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 32));
        let mut shell_integration = ShellIntegrationState::default();
        terminal.feed(b"A\x1b]7;file://localhost/home/mingxu/browser\x1b\\B");

        let json =
            drain_browser_host_actions_json(&mut terminal, &mut shell_integration, |_| Ok(()))
                .unwrap();

        assert_eq!(json, "[]");
        assert_eq!(
            shell_integration
                .current_directory()
                .map(|dir| dir.path.as_str()),
            Some("/home/mingxu/browser")
        );
        assert!(render_snapshot_text(&terminal.snapshot()).contains("AB"));
    }

    #[test]
    fn browser_shell_integration_json_exposes_timing_metadata() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 32));
        let mut shell_integration = ShellIntegrationState::default();

        terminal.feed(b"\x1b]133;A\x1b\\$\x1b]133;B\x1b\\ echo\x1b]133;C\x1b\\\r\nok");
        drain_browser_host_actions_json_at_ms(
            &mut terminal,
            &mut shell_integration,
            Some(100),
            |_| Ok(()),
        )
        .unwrap();
        terminal.feed(b"\x1b]133;D;0\x1b\\");
        drain_browser_host_actions_json_at_ms(
            &mut terminal,
            &mut shell_integration,
            Some(325),
            |_| Ok(()),
        )
        .unwrap();

        let json = browser_completed_command_blocks_json_text(&shell_integration).unwrap();
        let blocks: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(blocks[0]["started_at_ms"], 100);
        assert_eq!(blocks[0]["finished_at_ms"], 325);
        assert_eq!(blocks[0]["duration_ms"], 225);
    }

    #[test]
    fn browser_command_block_commands_update_selection() {
        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_event(witty_core::TerminalShellIntegrationEvent {
            marker: witty_core::TerminalShellIntegrationMarker::PromptStart,
            screen: TerminalScreen::Main,
            point: CellPoint::new(0, 0),
            anchor: None,
            exit_code: None,
        });
        shell_integration.apply_event(witty_core::TerminalShellIntegrationEvent {
            marker: witty_core::TerminalShellIntegrationMarker::CommandFinished,
            screen: TerminalScreen::Main,
            point: CellPoint::new(0, 2),
            anchor: None,
            exit_code: Some(0),
        });

        assert!(apply_command_block_command(
            &mut shell_integration,
            TerminalScreen::Main,
            COMMAND_BLOCK_SELECT_LATEST_COMMAND_ID,
        ));
        assert_eq!(shell_integration.selected_block_id(), Some(0));
        assert!(apply_command_block_command(
            &mut shell_integration,
            TerminalScreen::Main,
            COMMAND_BLOCK_TOGGLE_FOLD_COMMAND_ID,
        ));
        assert!(shell_integration.is_completed_block_folded(0));
        let selected_json =
            browser_selected_command_block_json_text(&shell_integration).expect("valid JSON");
        let selected: serde_json::Value = serde_json::from_str(&selected_json).unwrap();
        assert_eq!(selected["folded"], true);
        assert!(apply_command_block_command(
            &mut shell_integration,
            TerminalScreen::Main,
            COMMAND_BLOCK_TOGGLE_FOLD_COMMAND_ID,
        ));
        assert!(!shell_integration.is_completed_block_folded(0));
        assert!(apply_command_block_command(
            &mut shell_integration,
            TerminalScreen::Main,
            COMMAND_BLOCK_CLEAR_SELECTION_COMMAND_ID,
        ));
        assert_eq!(shell_integration.selected_block_id(), None);
        assert!(!apply_command_block_command(
            &mut shell_integration,
            TerminalScreen::Main,
            "witty.unknown",
        ));
    }

    #[test]
    fn browser_folded_command_block_hidden_row_spans_json_tracks_visible_plan() {
        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_event(witty_core::TerminalShellIntegrationEvent {
            marker: witty_core::TerminalShellIntegrationMarker::PromptStart,
            screen: TerminalScreen::Main,
            point: CellPoint::new(0, 0),
            anchor: Some(witty_core::TerminalPointAnchor {
                row: TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 40,
                },
                col: 0,
            }),
            exit_code: None,
        });
        shell_integration.apply_event(witty_core::TerminalShellIntegrationEvent {
            marker: witty_core::TerminalShellIntegrationMarker::CommandFinished,
            screen: TerminalScreen::Main,
            point: CellPoint::new(2, 0),
            anchor: Some(witty_core::TerminalPointAnchor {
                row: TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 42,
                },
                col: 0,
            }),
            exit_code: Some(0),
        });
        let visible = [
            TerminalVisibleRowAnchor {
                visible_row: 0,
                anchor: TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 40,
                },
            },
            TerminalVisibleRowAnchor {
                visible_row: 1,
                anchor: TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 41,
                },
            },
            TerminalVisibleRowAnchor {
                visible_row: 2,
                anchor: TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 42,
                },
            },
        ];

        let json = browser_folded_command_block_hidden_row_spans_json_text(
            &shell_integration,
            TerminalScreen::Main,
            &visible,
            4,
        )
        .unwrap();
        let spans: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(spans.as_array().unwrap().len(), 0);

        assert!(shell_integration.set_completed_block_folded(0, true));
        let json = browser_folded_command_block_hidden_row_spans_json_text(
            &shell_integration,
            TerminalScreen::Main,
            &visible,
            4,
        )
        .unwrap();
        let spans: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(spans[0]["id"], 0);
        assert_eq!(spans[0]["screen"], "main");
        assert_eq!(spans[0]["summary_row"], 0);
        assert_eq!(spans[0]["hidden_start_row"], 1);
        assert_eq!(spans[0]["hidden_end_row"], 2);
        assert_eq!(spans[0]["exit_code"], 0);

        let json = browser_folded_command_block_compact_rows_json_text(
            &shell_integration,
            TerminalScreen::Main,
            &visible,
            4,
        )
        .unwrap();
        let compact_rows: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(compact_rows.as_array().unwrap().len(), 4);
        assert_eq!(compact_rows[0]["visible_row"], 0);
        assert_eq!(compact_rows[0]["compact_row"], 0);
        assert_eq!(compact_rows[0]["hidden"], false);
        assert_eq!(compact_rows[1]["visible_row"], 1);
        assert_eq!(compact_rows[1]["hidden"], true);
        assert_eq!(compact_rows[1]["compact_row"], serde_json::Value::Null);
        assert_eq!(compact_rows[1]["hidden_by_block_id"], 0);
        assert_eq!(compact_rows[2]["hidden_rows_before"], 1);
        assert_eq!(compact_rows[3]["visible_row"], 3);
        assert_eq!(compact_rows[3]["compact_row"], 1);
        assert_eq!(compact_rows[3]["hidden_rows_before"], 2);
    }

    #[test]
    fn browser_folded_command_block_row_mask_uses_visible_plan() {
        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_event(witty_core::TerminalShellIntegrationEvent {
            marker: witty_core::TerminalShellIntegrationMarker::PromptStart,
            screen: TerminalScreen::Main,
            point: CellPoint::new(0, 0),
            anchor: Some(witty_core::TerminalPointAnchor {
                row: TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 40,
                },
                col: 0,
            }),
            exit_code: None,
        });
        shell_integration.apply_event(witty_core::TerminalShellIntegrationEvent {
            marker: witty_core::TerminalShellIntegrationMarker::CommandFinished,
            screen: TerminalScreen::Main,
            point: CellPoint::new(2, 0),
            anchor: Some(witty_core::TerminalPointAnchor {
                row: TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 42,
                },
                col: 0,
            }),
            exit_code: Some(0),
        });
        assert!(shell_integration.set_completed_block_folded(0, true));

        let visible = [
            TerminalVisibleRowAnchor {
                visible_row: 0,
                anchor: TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 40,
                },
            },
            TerminalVisibleRowAnchor {
                visible_row: 1,
                anchor: TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 41,
                },
            },
            TerminalVisibleRowAnchor {
                visible_row: 2,
                anchor: TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 42,
                },
            },
        ];
        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        let mut frame = FramePlan::default();
        frame.glyphs.push(GlyphBatchItem {
            origin: PixelPoint { x: 0.0, y: 0.0 },
            text: "summary".to_owned(),
            color: Rgba::WHITE,
            style_flags: CellFlags::default(),
        });
        frame.glyphs.push(GlyphBatchItem {
            origin: PixelPoint { x: 0.0, y: 20.0 },
            text: "hidden".to_owned(),
            color: Rgba::WHITE,
            style_flags: CellFlags::default(),
        });

        let rects = apply_command_block_folded_row_mask_with_anchors(
            &mut frame,
            &shell_integration,
            TerminalScreen::Main,
            &visible,
            metrics,
            GridSize::new(3, 16),
        );

        assert_eq!(rects, 2);
        assert!(frame.glyphs.iter().any(|glyph| glyph.text == "summary"));
        assert!(!frame.glyphs.iter().any(|glyph| glyph.text == "hidden"));
        assert_eq!(frame.backgrounds.len(), 2);
        assert_eq!(frame.backgrounds[0].origin, PixelPoint { x: 0.0, y: 20.0 });
        assert_eq!(frame.backgrounds[1].origin, PixelPoint { x: 0.0, y: 40.0 });
    }

    #[test]
    fn browser_command_block_gutter_hit_json_reports_visible_block() {
        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_event(witty_core::TerminalShellIntegrationEvent {
            marker: witty_core::TerminalShellIntegrationMarker::PromptStart,
            screen: TerminalScreen::Main,
            point: CellPoint::new(0, 0),
            anchor: Some(witty_core::TerminalPointAnchor {
                row: witty_core::TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 30,
                },
                col: 0,
            }),
            exit_code: None,
        });
        shell_integration.apply_event(witty_core::TerminalShellIntegrationEvent {
            marker: witty_core::TerminalShellIntegrationMarker::CommandFinished,
            screen: TerminalScreen::Main,
            point: CellPoint::new(1, 2),
            anchor: Some(witty_core::TerminalPointAnchor {
                row: witty_core::TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 31,
                },
                col: 2,
            }),
            exit_code: Some(0),
        });
        shell_integration.select_latest_completed_block_for_screen(TerminalScreen::Main);

        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        let visible_row_anchors = [
            witty_core::TerminalVisibleRowAnchor {
                visible_row: 3,
                anchor: witty_core::TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 30,
                },
            },
            witty_core::TerminalVisibleRowAnchor {
                visible_row: 4,
                anchor: witty_core::TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 31,
                },
            },
        ];

        let hit: serde_json::Value =
            serde_json::from_str(&browser_command_block_gutter_hit_json_text(
                &shell_integration,
                TerminalScreen::Main,
                &visible_row_anchors,
                2.0,
                65.0,
                BrowserHitTestViewport {
                    device_pixel_ratio: 1.0,
                    metrics,
                    size: GridSize::new(6, 8),
                },
            ))
            .unwrap();
        assert_eq!(hit["hit"], true);
        assert_eq!(hit["id"], 0);
        assert_eq!(hit["screen"], "main");
        assert_eq!(hit["visibleRow"], 3);
        assert_eq!(hit["startRow"], 3);
        assert_eq!(hit["endRow"], 4);
        assert_eq!(hit["selected"], true);
        assert_eq!(hit["exitCode"], 0);
        let text_ranges: serde_json::Value = serde_json::from_str(
            &browser_selected_command_block_text_ranges_json_text(&shell_integration).unwrap(),
        )
        .unwrap();
        assert_eq!(text_ranges["id"], 0);
        assert_eq!(text_ranges["command"]["start"]["row"], 0);
        assert_eq!(text_ranges["command"]["end_exclusive"]["row"], 1);
        assert_eq!(text_ranges["output"], serde_json::Value::Null);
        assert_eq!(
            browser_command_block_gutter_hover_id(
                &shell_integration,
                TerminalScreen::Main,
                &visible_row_anchors,
                2.0,
                65.0,
                BrowserHitTestViewport {
                    device_pixel_ratio: 1.0,
                    metrics,
                    size: GridSize::new(6, 8),
                },
            ),
            Some(0)
        );

        let miss: serde_json::Value =
            serde_json::from_str(&browser_command_block_gutter_hit_json_text(
                &shell_integration,
                TerminalScreen::Main,
                &visible_row_anchors,
                10.0,
                65.0,
                BrowserHitTestViewport {
                    device_pixel_ratio: 1.0,
                    metrics,
                    size: GridSize::new(6, 8),
                },
            ))
            .unwrap();
        assert_eq!(miss["hit"], false);
        assert_eq!(miss["id"], serde_json::Value::Null);
        assert_eq!(
            browser_command_block_gutter_hover_id(
                &shell_integration,
                TerminalScreen::Main,
                &visible_row_anchors,
                10.0,
                65.0,
                BrowserHitTestViewport {
                    device_pixel_ratio: 1.0,
                    metrics,
                    size: GridSize::new(6, 8),
                },
            ),
            None
        );

        shell_integration.clear_selection();
        let selected_json = browser_select_command_block_gutter_hit_json_text(
            &mut shell_integration,
            TerminalScreen::Main,
            &visible_row_anchors,
            2.0,
            65.0,
            BrowserHitTestViewport {
                device_pixel_ratio: 1.0,
                metrics,
                size: GridSize::new(6, 8),
            },
        )
        .unwrap();
        let selected: serde_json::Value = serde_json::from_str(&selected_json).unwrap();
        assert_eq!(selected["id"], 0);
        assert_eq!(shell_integration.selected_block_id(), Some(0));

        let miss_selection_json = browser_select_command_block_gutter_hit_json_text(
            &mut shell_integration,
            TerminalScreen::Main,
            &visible_row_anchors,
            10.0,
            65.0,
            BrowserHitTestViewport {
                device_pixel_ratio: 1.0,
                metrics,
                size: GridSize::new(6, 8),
            },
        )
        .unwrap();
        let miss_selection: serde_json::Value = serde_json::from_str(&miss_selection_json).unwrap();
        assert_eq!(miss_selection, serde_json::Value::Null);
        assert_eq!(shell_integration.selected_block_id(), Some(0));
    }

    #[test]
    fn browser_command_block_gutter_hit_json_remaps_folded_compact_rows() {
        let mut shell_integration = ShellIntegrationState::default();
        shell_integration.apply_event(witty_core::TerminalShellIntegrationEvent {
            marker: witty_core::TerminalShellIntegrationMarker::PromptStart,
            screen: TerminalScreen::Main,
            point: CellPoint::new(0, 0),
            anchor: Some(witty_core::TerminalPointAnchor {
                row: TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 30,
                },
                col: 0,
            }),
            exit_code: None,
        });
        shell_integration.apply_event(witty_core::TerminalShellIntegrationEvent {
            marker: witty_core::TerminalShellIntegrationMarker::CommandFinished,
            screen: TerminalScreen::Main,
            point: CellPoint::new(2, 0),
            anchor: Some(witty_core::TerminalPointAnchor {
                row: TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 32,
                },
                col: 0,
            }),
            exit_code: Some(0),
        });
        shell_integration.apply_event(witty_core::TerminalShellIntegrationEvent {
            marker: witty_core::TerminalShellIntegrationMarker::PromptStart,
            screen: TerminalScreen::Main,
            point: CellPoint::new(3, 0),
            anchor: Some(witty_core::TerminalPointAnchor {
                row: TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 33,
                },
                col: 0,
            }),
            exit_code: None,
        });
        shell_integration.apply_event(witty_core::TerminalShellIntegrationEvent {
            marker: witty_core::TerminalShellIntegrationMarker::CommandFinished,
            screen: TerminalScreen::Main,
            point: CellPoint::new(3, 2),
            anchor: Some(witty_core::TerminalPointAnchor {
                row: TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 33,
                },
                col: 2,
            }),
            exit_code: Some(0),
        });
        assert!(shell_integration.set_completed_block_folded(0, true));

        let visible_row_anchors = [
            TerminalVisibleRowAnchor {
                visible_row: 0,
                anchor: TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 30,
                },
            },
            TerminalVisibleRowAnchor {
                visible_row: 1,
                anchor: TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 31,
                },
            },
            TerminalVisibleRowAnchor {
                visible_row: 2,
                anchor: TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 32,
                },
            },
            TerminalVisibleRowAnchor {
                visible_row: 3,
                anchor: TerminalRowAnchor {
                    screen: TerminalScreen::Main,
                    row: 33,
                },
            },
        ];
        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        let viewport = BrowserHitTestViewport {
            device_pixel_ratio: 1.0,
            metrics,
            size: GridSize::new(5, 8),
        };

        let hit: serde_json::Value =
            serde_json::from_str(&browser_command_block_gutter_hit_json_text(
                &shell_integration,
                TerminalScreen::Main,
                &visible_row_anchors,
                2.0,
                25.0,
                viewport,
            ))
            .unwrap();
        assert_eq!(hit["hit"], true);
        assert_eq!(hit["id"], 1);
        assert_eq!(hit["visibleRow"], 3);

        let selected_json = browser_select_command_block_gutter_hit_json_text(
            &mut shell_integration,
            TerminalScreen::Main,
            &visible_row_anchors,
            2.0,
            25.0,
            viewport,
        )
        .unwrap();
        let selected: serde_json::Value = serde_json::from_str(&selected_json).unwrap();
        assert_eq!(selected["id"], 1);
        assert_eq!(shell_integration.selected_block_id(), Some(1));

        let compact_blank: serde_json::Value =
            serde_json::from_str(&browser_command_block_gutter_hit_json_text(
                &shell_integration,
                TerminalScreen::Main,
                &visible_row_anchors,
                2.0,
                75.0,
                viewport,
            ))
            .unwrap();
        assert_eq!(compact_blank["hit"], false);
    }

    #[test]
    fn browser_selected_command_block_overlay_marks_visible_rows() {
        let mut terminal = BasicTerminal::new(GridSize::new(4, 32));
        let mut shell_integration = ShellIntegrationState::default();
        terminal.feed(
            b"\x1b]133;A\x1b\\$\x1b]133;B\x1b\\ echo\x1b]133;C\x1b\\\r\none\r\ntwo\x1b]133;D;0\x1b\\",
        );
        drain_browser_host_actions_json(&mut terminal, &mut shell_integration, |_| Ok(())).unwrap();
        shell_integration.select_latest_completed_block_for_screen(TerminalScreen::Main);

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
            &shell_integration,
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
            &shell_integration,
            TerminalScreen::Alternate,
            metrics,
            GridSize::new(2, 8),
        );
        assert_eq!(rects_for_alternate, 0);
        assert_eq!(frame.backgrounds.len(), 4);
    }

    #[test]
    fn browser_command_block_status_label_overlay_marks_selected_block() {
        let mut terminal = BasicTerminal::new(GridSize::new(4, 32));
        let mut shell_integration = ShellIntegrationState::default();
        terminal.feed(
            b"\x1b]133;A\x1b\\$\x1b]133;B\x1b\\ false\x1b]133;C\x1b\\\r\nbad\x1b]133;D;2\x1b\\",
        );
        drain_browser_host_actions_json(&mut terminal, &mut shell_integration, |_| Ok(())).unwrap();
        shell_integration.select_latest_completed_block_for_screen(TerminalScreen::Main);

        let mut frame = FramePlan::default();
        let rects = apply_command_block_status_label_overlay_with_anchors(
            &mut frame,
            &shell_integration,
            TerminalScreen::Main,
            &terminal.visible_row_anchors(),
            None,
            CellMetrics::default(),
            GridSize::new(4, 32),
        );

        assert_eq!(rects, 1);
        assert_eq!(frame.backgrounds.len(), 1);
        assert!(frame.glyphs.iter().any(|glyph| glyph.text == "exit 2"));
    }

    #[test]
    fn browser_command_block_status_label_overlay_includes_duration() {
        let mut terminal = BasicTerminal::new(GridSize::new(4, 32));
        let mut shell_integration = ShellIntegrationState::default();

        terminal.feed(b"\x1b]133;A\x1b\\$\x1b]133;B\x1b\\ sleep\x1b]133;C\x1b\\\r\n");
        drain_browser_host_actions_json_at_ms(
            &mut terminal,
            &mut shell_integration,
            Some(100),
            |_| Ok(()),
        )
        .unwrap();
        terminal.feed(b"done\x1b]133;D;0\x1b\\");
        drain_browser_host_actions_json_at_ms(
            &mut terminal,
            &mut shell_integration,
            Some(1_650),
            |_| Ok(()),
        )
        .unwrap();
        shell_integration.select_latest_completed_block_for_screen(TerminalScreen::Main);

        let mut frame = FramePlan::default();
        let rects = apply_command_block_status_label_overlay_with_anchors(
            &mut frame,
            &shell_integration,
            TerminalScreen::Main,
            &terminal.visible_row_anchors(),
            None,
            CellMetrics::default(),
            GridSize::new(4, 32),
        );

        assert_eq!(rects, 1);
        assert!(frame.glyphs.iter().any(|glyph| glyph.text == "ok 1.6s"));

        assert!(shell_integration.set_completed_block_folded(0, true));
        let mut frame = FramePlan::default();
        let rects = apply_command_block_status_label_overlay_with_anchors(
            &mut frame,
            &shell_integration,
            TerminalScreen::Main,
            &terminal.visible_row_anchors(),
            None,
            CellMetrics::default(),
            GridSize::new(4, 32),
        );

        assert_eq!(rects, 1);
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text == "folded 1 row ok 1.6s"));
    }

    #[test]
    fn browser_search_smoke_tracks_status_and_visible_highlights() {
        assert_eq!(
            browser_search_smoke_report(),
            BrowserSearchSmokeReport {
                query: "alpha".to_owned(),
                match_count: 2,
                active_index: Some(0),
                visible_highlights: 2,
                active_visible: true,
                status: "Find: alpha [aa lit part raw] 1/2".to_owned(),
            }
        );
    }

    #[test]
    fn browser_search_status_reports_zero_and_invalid_regex_states() {
        let rows = vec![witty_core::SearchTextRow {
            id: witty_core::SearchRowId::screen(0),
            visible_row: Some(0),
            text: "alpha beta".to_owned(),
            columns: Vec::new(),
        }];
        let mut search = TerminalSearch::default();

        search.open(&rows, None);
        assert_eq!(
            browser_search_status_text(&search),
            "Find:  [aa lit part raw] 0/0"
        );

        search.input_text(&rows, "missing");
        assert_eq!(
            browser_search_status_text(&search),
            "Find: missing [aa lit part raw] No results"
        );

        search.set_query(&rows, "[");
        search.toggle_regex(&rows);
        assert!(browser_search_status_text(&search).contains("invalid regex"));
        assert!(browser_search_status_text(&search).contains("[aa .* part raw]"));
    }

    #[test]
    fn browser_command_palette_smoke_filters_and_draws_overlay() {
        let report = browser_command_palette_smoke_report();

        assert_eq!(report.query, "sea");
        assert_eq!(report.filtered_count, 4);
        assert_eq!(report.selected_index, Some(0));
        assert_eq!(report.selected_command_id, SEARCH_OPEN_COMMAND_ID);
        assert_eq!(
            report.visible_item_command_ids,
            vec![
                SEARCH_OPEN_COMMAND_ID.to_owned(),
                "witty.search.close".to_owned(),
                "witty.search.next".to_owned(),
            ]
        );
        assert!(report.status.contains("1/4"));
        assert!(report.status.contains("Command Palette: sea"));
        assert!(report.overlay_glyphs >= 2);
        assert!(report.overlay_backgrounds >= 2);
    }

    #[test]
    fn browser_command_palette_status_and_json_track_visible_window() {
        let commands = browser_default_command_registrations();
        let mut palette = CommandPalette::default();

        palette.open(&commands);
        palette.move_selection(5);

        assert_eq!(palette.selected_index(), Some(5));
        assert_eq!(palette.selected_command().unwrap().id, "web.echo");
        assert_eq!(
            browser_command_palette_status_text(&palette),
            "Command Palette: 6/14 web.echo"
        );

        let items =
            browser_command_palette_visible_items_json_text(&palette, 3).expect("valid JSON");
        let items = serde_json::from_str::<Vec<serde_json::Value>>(&items).expect("valid items");

        assert_eq!(items.len(), 3);
        assert_eq!(items[0]["id"].as_str(), Some("witty.search.next"));
        assert_eq!(items[0]["filteredIndex"].as_u64(), Some(3));
        assert_eq!(items[0]["position"].as_u64(), Some(4));
        assert_eq!(items[2]["id"].as_str(), Some("web.echo"));
        assert_eq!(items[2]["filteredIndex"].as_u64(), Some(5));
        assert_eq!(items[2]["position"].as_u64(), Some(6));
        assert_eq!(items[2]["selected"].as_bool(), Some(true));
    }

    #[test]
    fn browser_command_shortcuts_pick_builtin_and_first_external_command() {
        let commands = browser_default_command_registrations();

        assert_eq!(
            browser_command_shortcut_for_key("F1", &commands),
            Some("witty.about".to_owned())
        );
        assert_eq!(
            browser_command_shortcut_for_key("F2", &commands),
            Some("web.echo".to_owned())
        );
        assert_eq!(browser_command_shortcut_for_key("F3", &commands), None);
    }

    #[test]
    fn browser_command_palette_includes_command_block_commands() {
        let commands = browser_default_command_registrations();
        let mut palette = CommandPalette::default();

        palette.open(&commands);
        palette.input_text("block");

        assert_eq!(palette.filtered_count(), 8);

        let items =
            browser_command_palette_visible_items_json_text(&palette, 8).expect("valid JSON");
        let items = serde_json::from_str::<Vec<serde_json::Value>>(&items).expect("valid items");
        assert_eq!(items.len(), 8);
        assert!(items
            .iter()
            .any(|item| item["id"].as_str() == Some(COMMAND_BLOCK_ACTION_MENU_COMMAND_ID)));
        assert!(items
            .iter()
            .any(|item| item["id"].as_str() == Some(COMMAND_BLOCK_SELECT_LATEST_COMMAND_ID)));
        assert!(items
            .iter()
            .any(|item| item["id"].as_str() == Some(COMMAND_BLOCK_CLEAR_SELECTION_COMMAND_ID)));
        assert!(items
            .iter()
            .any(|item| item["id"].as_str() == Some(COMMAND_BLOCK_COPY_COMMAND_ID)));
        assert!(items
            .iter()
            .any(|item| item["id"].as_str() == Some(COMMAND_BLOCK_COPY_OUTPUT_ID)));
        assert!(items
            .iter()
            .any(|item| item["id"].as_str() == Some(COMMAND_BLOCK_TOGGLE_FOLD_COMMAND_ID)));

        palette.open(&commands);
        palette.input_text("latest");

        assert_eq!(palette.filtered_count(), 1);
        assert_eq!(
            palette
                .selected_command()
                .map(|command| command.id.as_str()),
            Some(COMMAND_BLOCK_SELECT_LATEST_COMMAND_ID)
        );
    }

    #[test]
    fn browser_command_block_action_menu_status_json_and_overlay() {
        let mut terminal = BasicTerminal::new(GridSize::new(4, 32));
        let mut shell_integration = ShellIntegrationState::default();
        terminal.feed(
            b"\x1b]133;A\x1b\\$\x1b]133;B\x1b\\ echo\x1b]133;C\x1b\\\r\none\r\ntwo\x1b]133;D;0\x1b\\",
        );
        drain_browser_host_actions_json(&mut terminal, &mut shell_integration, |_| Ok(())).unwrap();
        shell_integration.select_latest_completed_block_for_screen(TerminalScreen::Main);

        let mut menu = CommandBlockActionMenu::default();
        assert!(menu.open_for_selected_block(&shell_integration));
        assert_eq!(
            browser_command_block_action_menu_status_text(&menu),
            "Command Block Actions: 1/4 witty.command_block.copy_output"
        );

        let items =
            browser_command_block_action_menu_visible_items_json_text(&menu).expect("valid JSON");
        let items = serde_json::from_str::<Vec<serde_json::Value>>(&items).expect("valid items");
        assert_eq!(items.len(), 4);
        assert_eq!(items[0]["id"].as_str(), Some(COMMAND_BLOCK_COPY_OUTPUT_ID));
        assert_eq!(items[0]["selected"].as_bool(), Some(true));

        menu.move_selection(1);
        assert_eq!(
            menu.selected_command_id(),
            Some(COMMAND_BLOCK_COPY_COMMAND_ID)
        );
        assert_eq!(
            browser_command_block_action_menu_status_text(&menu),
            "Command Block Actions: 2/4 witty.command_block.copy_command"
        );

        let mut frame = browser_entry_frame();
        let base_backgrounds = frame.backgrounds.len();
        let rects = apply_command_block_action_menu_overlay(
            &mut frame,
            &menu,
            &shell_integration,
            TerminalScreen::Main,
            &terminal.visible_row_anchors(),
            CellMetrics::default(),
            GridSize::new(4, 32),
        );
        assert_eq!(rects, 2);
        assert!(frame.backgrounds.len() > base_backgrounds);
        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("Block Actions")));

        assert_eq!(menu.confirm(), Some(COMMAND_BLOCK_COPY_COMMAND_ID));
        assert!(!menu.is_open());
    }

    #[test]
    fn browser_command_palette_overlay_renders_preedit_and_positions_cursor() {
        let commands = browser_default_command_registrations();
        let mut palette = CommandPalette::default();
        palette.open(&commands);
        palette.input_text("se");
        let mut composition = ImeComposition::default();
        composition.set_preedit("中", Some(("中".len(), "中".len())));

        let mut frame = browser_entry_frame();
        apply_browser_command_palette_overlay(
            &mut frame,
            &palette,
            Some(&composition),
            &commands,
            CellMetrics::default(),
            GridSize::new(12, 80),
        );

        assert!(frame
            .glyphs
            .iter()
            .any(|glyph| glyph.text.contains("Command Palette  se中")));
        assert_eq!(palette.query(), "se");
        assert_eq!(
            browser_command_palette_ime_cursor_cell(&palette, &composition, GridSize::new(12, 80)),
            Some(CellPoint::new(2, 24))
        );
    }

    #[test]
    fn browser_search_history_keeps_queries_in_ui_state() {
        let rows = vec![witty_core::SearchTextRow {
            id: witty_core::SearchRowId::screen(0),
            visible_row: Some(0),
            text: "alpha beta gamma".to_owned(),
            columns: Vec::new(),
        }];
        let mut search = TerminalSearch::default();

        search.open(&rows, Some("alpha"));
        search.close();
        search.open(&rows, Some("beta"));
        search.close();
        search.open(&rows, None);

        assert_eq!(
            browser_search_status_text(&search),
            "Find: beta [aa lit part raw] 1/1"
        );
        assert_eq!(search.previous_history_query(&rows), Some("beta"));
        assert_eq!(search.previous_history_query(&rows), Some("alpha"));
        assert_eq!(
            browser_search_status_text(&search),
            "Find: alpha [aa lit part raw] 1/1"
        );
    }

    #[test]
    fn browser_document_title_uses_terminal_title_and_default() {
        let size = GridSize::new(4, 16);
        let mut terminal = BasicTerminal::new(size);
        let mut app = TerminalApp::new(BrowserGatewayTransport::new(size), size);

        assert_eq!(browser_document_title(app.title()), DEFAULT_BROWSER_TITLE);

        terminal.feed(b"\x1b]2;web shell\x07");
        app.set_snapshot(terminal.take_snapshot());

        assert_eq!(browser_document_title(app.title()), "web shell");
        assert_eq!(browser_document_title(Some("")), DEFAULT_BROWSER_TITLE);
    }

    #[test]
    fn browser_visible_text_tracks_alternate_screen_switches() {
        let mut terminal = BasicTerminal::new(GridSize::new(4, 32));

        terminal.feed(b"main screen\r\n\x1b[?1049halternate app");
        let alternate = render_snapshot_text(&terminal.snapshot());
        assert!(alternate.contains("alternate app"));
        assert!(!alternate.contains("main screen"));

        terminal.feed(b"\x1b[?1049lrestored main");
        let restored = render_snapshot_text(&terminal.snapshot());
        assert!(restored.contains("main screen"));
        assert!(restored.contains("restored main"));
        assert!(!restored.contains("alternate app"));
    }

    #[test]
    fn browser_canvas_resize_scales_backing_store_and_keeps_logical_grid() {
        let report = browser_canvas_resize_report(450.0, 180.0, 2.0);

        assert_eq!(report.backing_width, 900);
        assert_eq!(report.backing_height, 360);
        assert_eq!(report.device_pixel_ratio, 2.0);
        assert_eq!(report.metrics.cell.width, 18.0);
        assert_eq!(report.metrics.cell.height, 36.0);
        assert_eq!(report.grid, GridSize::new(9, 48));
    }

    #[test]
    fn browser_canvas_resize_sanitizes_invalid_dimensions() {
        let report = browser_canvas_resize_report(0.0, -10.0, f64::NAN);

        assert_eq!(report.backing_width, 1);
        assert_eq!(report.backing_height, 1);
        assert_eq!(report.device_pixel_ratio, 1.0);
        assert_eq!(report.grid, GridSize::new(1, 1));
    }

    #[test]
    fn browser_scroll_lines_for_wheel_delta_matches_browser_sign_and_modes() {
        let grid = GridSize::new(24, 80);
        let metrics = CellMetrics::default();

        assert_eq!(
            browser_scroll_lines_for_wheel_delta(-120.0, 0, metrics, grid, 1.0),
            7
        );
        assert_eq!(
            browser_scroll_lines_for_wheel_delta(120.0, 0, metrics, grid, 1.0),
            -7
        );
        assert_eq!(
            browser_scroll_lines_for_wheel_delta(-3.0, 1, metrics, grid, 1.0),
            3
        );
        assert_eq!(
            browser_scroll_lines_for_wheel_delta(1.0, 2, metrics, grid, 1.0),
            -24
        );
        assert_eq!(
            browser_scroll_lines_for_wheel_delta(-1.0, 0, metrics, grid, 1.0),
            1
        );
        assert_eq!(
            browser_scroll_lines_for_wheel_delta(f64::NAN, 0, metrics, grid, 1.0),
            0
        );
    }

    #[test]
    fn browser_scroll_lines_for_wheel_delta_uses_css_pixels_with_dpr() {
        let grid = GridSize::new(24, 80);
        let metrics = scaled_cell_metrics(2.0);

        assert_eq!(
            browser_scroll_lines_for_wheel_delta(-18.0, 0, metrics, grid, 2.0),
            1
        );
    }

    fn browser_mouse_modes(tracking: MouseTrackingMode) -> TerminalMouseModes {
        TerminalMouseModes {
            tracking,
            encoding: MouseEncodingMode::Sgr,
            focus_events: false,
            alternate_scroll: false,
        }
    }

    fn browser_mouse_input(kind: BrowserMouseEventKind, col: u16, row: u16) -> BrowserMouseInput {
        BrowserMouseInput {
            kind,
            button: if kind == BrowserMouseEventKind::PointerMove {
                -1
            } else {
                0
            },
            buttons: 0,
            offset_x: 8.0 + f64::from(col) * 9.0 + 1.0,
            offset_y: 8.0 + f64::from(row) * 18.0 + 1.0,
            delta_y: 0.0,
            modifiers: MouseModifiers::NONE,
        }
    }

    #[test]
    fn browser_mouse_coordinates_scale_css_offsets_to_backing_store() {
        let metrics = scaled_cell_metrics(2.0);
        let point = browser_cell_point_for_offset(27.0, 63.0, 2.0, metrics, GridSize::new(10, 10));
        let pixel = browser_pixel_position_for_offset(27.0, 63.0, 2.0);

        assert_eq!(point, CellPoint::new(3, 2));
        assert_eq!(pixel, Some(PixelMousePosition::new(54, 126)));
    }

    #[test]
    fn browser_hyperlink_activation_target_applies_shared_url_policy() {
        let metrics = CellMetrics::default();
        let size = GridSize::new(1, 8);
        let mut snapshot = RenderSnapshot::from_plain_lines(&["ab cd"]);
        snapshot.rows[0].cells[0].hyperlink = Some(1);
        snapshot.rows[0].cells[1].hyperlink = Some(1);
        snapshot.rows[0].cells[3].hyperlink = Some(2);
        snapshot.rows[0].cells[4].hyperlink = Some(2);
        snapshot.hyperlinks = vec![
            TerminalHyperlink {
                id: 1,
                uri: "https://example.com".to_owned(),
                osc8_id: None,
            },
            TerminalHyperlink {
                id: 2,
                uri: "file:///tmp/example".to_owned(),
                osc8_id: None,
            },
        ];

        let allowed = browser_hyperlink_activation_target(&snapshot, 9.0, 9.0, 1.0, metrics, size);
        assert_eq!(
            allowed,
            BrowserHyperlinkActivationTarget::allowed("https://example.com".to_owned())
        );

        let blocked = browser_hyperlink_activation_target(&snapshot, 36.0, 9.0, 1.0, metrics, size);
        assert!(blocked.hit);
        assert!(!blocked.allowed);
        assert!(blocked.reason.unwrap().contains("not allowed"));

        let no_hit = browser_hyperlink_activation_target(&snapshot, 27.0, 9.0, 1.0, metrics, size);
        assert_eq!(no_hit, BrowserHyperlinkActivationTarget::no_hit());

        let json: serde_json::Value = serde_json::from_str(&allowed.to_json()).unwrap();
        assert_eq!(json["hit"], true);
        assert_eq!(json["allowed"], true);
        assert_eq!(json["uri"], "https://example.com");
    }

    #[test]
    fn browser_focus_encoder_uses_mouse_focus_mode_flag() {
        let disabled = TerminalMouseModes::default();
        let enabled = TerminalMouseModes {
            focus_events: true,
            ..TerminalMouseModes::default()
        };

        assert_eq!(
            encode_terminal_focus_event(FocusEventKind::In, disabled),
            None
        );
        assert_eq!(
            encode_terminal_focus_event(FocusEventKind::In, enabled),
            Some(b"\x1b[I".to_vec())
        );
        assert_eq!(
            encode_terminal_focus_event(FocusEventKind::Out, enabled),
            Some(b"\x1b[O".to_vec())
        );
    }

    #[test]
    fn browser_mouse_kind_parser_accepts_dom_and_neutral_names() {
        assert_eq!(
            BrowserMouseEventKind::from_browser_kind("pointerdown"),
            Some(BrowserMouseEventKind::PointerDown)
        );
        assert_eq!(
            BrowserMouseEventKind::from_browser_kind("release"),
            Some(BrowserMouseEventKind::PointerUp)
        );
        assert_eq!(
            BrowserMouseEventKind::from_browser_kind("wheel"),
            Some(BrowserMouseEventKind::Wheel)
        );
        assert_eq!(BrowserMouseEventKind::from_browser_kind("click"), None);
    }

    #[test]
    fn browser_mouse_reporter_ignores_disabled_reporting() {
        let mut reporter = BrowserMouseReportState::default();
        let input = browser_mouse_input(BrowserMouseEventKind::PointerDown, 2, 3);

        assert_eq!(
            reporter.encode(
                input,
                TerminalMouseModes::default(),
                CellMetrics::default(),
                GridSize::new(10, 10),
                1.0,
            ),
            None
        );
        assert_eq!(reporter, BrowserMouseReportState::default());
    }

    #[test]
    fn browser_local_selection_tracks_shift_drag_without_mouse_bytes() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 16));
        let mut selection = BrowserLocalSelectionState::default();
        terminal.feed(b"abcdef\r\nsecond");

        selection.begin(&mut terminal, CellPoint::new(0, 1), 1);
        assert_eq!(
            terminal.snapshot().selection,
            Some(CellRange {
                start: CellPoint::new(0, 1),
                end: CellPoint::new(0, 1),
            })
        );
        assert!(selection.update(&mut terminal, CellPoint::new(0, 4)));
        assert_eq!(terminal.selected_text().as_deref(), Some("bcde"));
        assert!(selection.end());
        assert!(!selection.update(&mut terminal, CellPoint::new(0, 5)));
    }

    #[test]
    fn browser_local_selection_double_click_selects_word() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 20));
        let mut selection = BrowserLocalSelectionState::default();
        terminal.feed(b"cat src/main.rs");

        selection.begin(&mut terminal, CellPoint::new(0, 8), 2);

        assert_eq!(terminal.selected_text().as_deref(), Some("src/main.rs"));
        assert!(!selection.update(&mut terminal, CellPoint::new(0, 10)));
    }

    #[test]
    fn browser_mouse_reporter_encodes_press_release_and_wheel() {
        let mut reporter = BrowserMouseReportState::default();
        let modes = browser_mouse_modes(MouseTrackingMode::Normal);
        let grid = GridSize::new(10, 10);

        assert_eq!(
            reporter.encode(
                browser_mouse_input(BrowserMouseEventKind::PointerDown, 2, 3),
                modes,
                CellMetrics::default(),
                grid,
                1.0,
            ),
            Some(b"\x1b[<0;3;4M".to_vec())
        );
        assert_eq!(
            reporter.encode(
                browser_mouse_input(BrowserMouseEventKind::PointerUp, 2, 3),
                modes,
                CellMetrics::default(),
                grid,
                1.0,
            ),
            Some(b"\x1b[<0;3;4m".to_vec())
        );

        let mut wheel = browser_mouse_input(BrowserMouseEventKind::Wheel, 3, 3);
        wheel.delta_y = -120.0;
        wheel.modifiers = MouseModifiers {
            control: true,
            ..MouseModifiers::NONE
        };
        assert_eq!(
            reporter.encode(wheel, modes, CellMetrics::default(), grid, 1.0),
            Some(b"\x1b[<80;4;4M".to_vec())
        );
    }

    #[test]
    fn browser_button_event_mouse_reports_drag_only_with_button_and_cell_change() {
        let mut reporter = BrowserMouseReportState::default();
        let modes = browser_mouse_modes(MouseTrackingMode::ButtonEvent);
        let grid = GridSize::new(10, 10);

        let mut motion = browser_mouse_input(BrowserMouseEventKind::PointerMove, 2, 3);
        motion.buttons = 0;
        assert_eq!(
            reporter.encode(motion, modes, CellMetrics::default(), grid, 1.0),
            None
        );
        let mut press = browser_mouse_input(BrowserMouseEventKind::PointerDown, 2, 3);
        press.buttons = 1;
        assert_eq!(
            reporter.encode(press, modes, CellMetrics::default(), grid, 1.0),
            Some(b"\x1b[<0;3;4M".to_vec())
        );

        let mut duplicate_motion = browser_mouse_input(BrowserMouseEventKind::PointerMove, 2, 3);
        duplicate_motion.buttons = 1;
        assert_eq!(
            reporter.encode(duplicate_motion, modes, CellMetrics::default(), grid, 1.0),
            None
        );

        let mut moved = browser_mouse_input(BrowserMouseEventKind::PointerMove, 3, 3);
        moved.buttons = 1;
        assert_eq!(
            reporter.encode(moved, modes, CellMetrics::default(), grid, 1.0),
            Some(b"\x1b[<32;4;4M".to_vec())
        );
    }

    #[test]
    fn browser_any_event_mouse_reports_motion_without_pressed_button() {
        let mut reporter = BrowserMouseReportState::default();
        let modes = browser_mouse_modes(MouseTrackingMode::AnyEvent);
        let input = browser_mouse_input(BrowserMouseEventKind::PointerMove, 0, 1);

        assert_eq!(
            reporter.encode(
                input,
                modes,
                CellMetrics::default(),
                GridSize::new(10, 10),
                1.0,
            ),
            Some(b"\x1b[<35;1;2M".to_vec())
        );
    }

    #[test]
    fn browser_mouse_reporter_passes_pixel_position_to_sgr_pixel_encoder() {
        let mut reporter = BrowserMouseReportState::default();
        let modes = TerminalMouseModes {
            tracking: MouseTrackingMode::Normal,
            encoding: MouseEncodingMode::SgrPixels,
            focus_events: false,
            alternate_scroll: false,
        };

        assert_eq!(
            reporter.encode(
                browser_mouse_input(BrowserMouseEventKind::PointerDown, 2, 3),
                modes,
                scaled_cell_metrics(2.0),
                GridSize::new(10, 10),
                2.0,
            ),
            Some(b"\x1b[<0;55;127M".to_vec())
        );
    }

    #[test]
    fn browser_key_input_encodes_text_enter_arrows_and_control() {
        assert_eq!(
            browser_key_input_report("x", "x", false),
            BrowserKeyInputReport {
                key: "x".to_owned(),
                text: "x".to_owned(),
                control: false,
                bytes: b"x".to_vec(),
            }
        );
        assert_eq!(
            browser_key_input_report("Enter", "", false).bytes,
            b"\r".to_vec()
        );
        assert_eq!(
            browser_key_input_report("ArrowUp", "", false).bytes,
            b"\x1b[A".to_vec()
        );
        assert_eq!(browser_key_input_report("c", "", true).bytes, vec![0x03]);
    }

    #[test]
    fn browser_keyboard_protocol_diagnostics_report_key_metadata_and_bytes() {
        let report = browser_keyboard_protocol_diagnostic_report_json(
            "1", "1", false, "Numpad1", 3, 0, 1,
        );
        let value: serde_json::Value = serde_json::from_str(&report).unwrap();

        assert_eq!(value["key"], "1");
        assert_eq!(value["code"], "Numpad1");
        assert_eq!(value["location"], 3);
        assert_eq!(value["witty"]["keypadKey"], "Digit(1)");
        assert_eq!(value["encoded"]["legacy"]["hex"], "31");
        assert_eq!(
            value["encoded"]["kittyDisambiguate"]["escaped"],
            "\\x1b[57400u"
        );
    }

    #[test]
    fn browser_key_input_uses_backarrow_key_mode() {
        let backarrow_modes = TerminalInputModes {
            backarrow_sends_backspace: true,
            ..TerminalInputModes::default()
        };

        assert_eq!(
            encode_browser_key_input("Backspace", "", false, TerminalInputModes::default()),
            Some(b"\x7f".to_vec())
        );
        assert_eq!(
            encode_browser_key_input("Backspace", "", false, backarrow_modes),
            Some(b"\x08".to_vec())
        );
    }

    #[test]
    fn browser_key_input_respects_keyboard_action_mode() {
        let locked_modes = TerminalInputModes {
            keyboard_locked: true,
            ..TerminalInputModes::default()
        };

        assert_eq!(
            encode_browser_key_input("x", "x", false, locked_modes),
            None
        );
        assert_eq!(
            encode_browser_key_input("Enter", "", false, locked_modes),
            None
        );
        assert_eq!(
            encode_browser_key_input("ArrowUp", "", false, locked_modes),
            None
        );
    }

    #[test]
    fn browser_key_input_uses_application_cursor_key_sequences() {
        let modes = TerminalInputModes {
            application_cursor_keys: true,
            application_keypad: false,
            ..TerminalInputModes::default()
        };

        assert_eq!(
            encode_browser_key_input("ArrowUp", "", false, modes),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            encode_browser_key_input("ArrowDown", "", false, modes),
            Some(b"\x1bOB".to_vec())
        );
        assert_eq!(
            encode_browser_key_input("ArrowRight", "", false, modes),
            Some(b"\x1bOC".to_vec())
        );
        assert_eq!(
            encode_browser_key_input("ArrowLeft", "", false, modes),
            Some(b"\x1bOD".to_vec())
        );
        assert_eq!(
            encode_browser_key_input("Home", "", false, modes),
            Some(b"\x1bOH".to_vec())
        );
        assert_eq!(
            encode_browser_key_input("End", "", false, modes),
            Some(b"\x1bOF".to_vec())
        );
    }

    #[test]
    fn browser_key_input_handles_xterm_navigation_and_function_keys() {
        let cases = [
            ("Home", b"\x1b[H".as_slice()),
            ("End", b"\x1b[F".as_slice()),
            ("Insert", b"\x1b[2~".as_slice()),
            ("Delete", b"\x1b[3~".as_slice()),
            ("PageUp", b"\x1b[5~".as_slice()),
            ("PageDown", b"\x1b[6~".as_slice()),
            ("F1", b"\x1bOP".as_slice()),
            ("F2", b"\x1bOQ".as_slice()),
            ("F3", b"\x1bOR".as_slice()),
            ("F4", b"\x1bOS".as_slice()),
            ("F5", b"\x1b[15~".as_slice()),
            ("F6", b"\x1b[17~".as_slice()),
            ("F7", b"\x1b[18~".as_slice()),
            ("F8", b"\x1b[19~".as_slice()),
            ("F9", b"\x1b[20~".as_slice()),
            ("F10", b"\x1b[21~".as_slice()),
            ("F11", b"\x1b[23~".as_slice()),
            ("F12", b"\x1b[24~".as_slice()),
        ];

        for (key, expected) in cases {
            assert_eq!(
                encode_browser_key_input(key, "", false, TerminalInputModes::default()),
                Some(expected.to_vec())
            );
        }
    }

    #[test]
    fn browser_key_input_parameterizes_modified_navigation_and_function_keys() {
        let shift = BrowserKeyModifiers {
            shift: true,
            ..BrowserKeyModifiers::default()
        };
        let alt = BrowserKeyModifiers {
            alt: true,
            ..BrowserKeyModifiers::default()
        };
        let control = BrowserKeyModifiers {
            control: true,
            ..BrowserKeyModifiers::default()
        };
        let shift_control = BrowserKeyModifiers {
            shift: true,
            control: true,
            ..BrowserKeyModifiers::default()
        };
        let alt_control = BrowserKeyModifiers {
            alt: true,
            control: true,
            ..BrowserKeyModifiers::default()
        };
        let all = BrowserKeyModifiers {
            shift: true,
            alt: true,
            control: true,
            ..BrowserKeyModifiers::default()
        };
        let app_cursor_modes = TerminalInputModes {
            application_cursor_keys: true,
            application_keypad: false,
            ..TerminalInputModes::default()
        };

        let cases = [
            ("ArrowUp", shift, b"\x1b[1;2A".as_slice()),
            ("ArrowLeft", control, b"\x1b[1;5D".as_slice()),
            ("Home", alt, b"\x1b[1;3H".as_slice()),
            ("End", shift_control, b"\x1b[1;6F".as_slice()),
            ("Insert", shift, b"\x1b[2;2~".as_slice()),
            ("Delete", control, b"\x1b[3;5~".as_slice()),
            ("PageUp", alt_control, b"\x1b[5;7~".as_slice()),
            ("PageDown", all, b"\x1b[6;8~".as_slice()),
            ("F1", shift, b"\x1b[1;2P".as_slice()),
            ("F5", control, b"\x1b[15;5~".as_slice()),
        ];

        for (key, modifiers, expected) in cases {
            assert_eq!(
                encode_browser_key_input_with_metadata(key, "", modifiers, "", 0, app_cursor_modes,),
                Some(expected.to_vec())
            );
        }

        let meta_shift = BrowserKeyModifiers {
            shift: true,
            meta: true,
            ..BrowserKeyModifiers::default()
        };
        assert_eq!(
            encode_browser_key_input_with_metadata(
                "ArrowUp",
                "",
                meta_shift,
                "",
                0,
                TerminalInputModes::default(),
            ),
            Some(b"\x1b[A".to_vec())
        );
    }

    #[test]
    fn browser_key_input_uses_kitty_csi_u_when_disambiguation_is_enabled() {
        let modes = TerminalInputModes {
            kitty_keyboard_flags: KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES,
            ..TerminalInputModes::default()
        };
        let control = BrowserKeyModifiers {
            control: true,
            ..BrowserKeyModifiers::default()
        };
        let shift_control = BrowserKeyModifiers {
            shift: true,
            control: true,
            ..BrowserKeyModifiers::default()
        };
        let alt = BrowserKeyModifiers {
            alt: true,
            ..BrowserKeyModifiers::default()
        };
        let shift = BrowserKeyModifiers {
            shift: true,
            ..BrowserKeyModifiers::default()
        };

        assert_eq!(
            encode_browser_key_input_with_metadata("i", "i", control, "", 0, modes),
            Some(b"\x1b[105;5u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata("I", "I", shift_control, "", 0, modes),
            Some(b"\x1b[105;6u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata("a", "a", alt, "", 0, modes),
            Some(b"\x1b[97;3u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata("Escape", "", shift, "", 0, modes),
            Some(b"\x1b[27;2u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata("Enter", "", control, "", 0, modes),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata("Tab", "", shift, "", 0, modes),
            Some(b"\t".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata("Backspace", "", control, "", 0, modes),
            Some(b"\x7f".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata("ArrowUp", "", control, "", 0, modes),
            Some(b"\x1b[1;5A".to_vec())
        );
    }

    #[test]
    fn browser_key_input_uses_kitty_csi_u_when_report_all_keys_is_enabled() {
        let modes = TerminalInputModes {
            kitty_keyboard_flags: KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES,
            ..TerminalInputModes::default()
        };
        let shift = BrowserKeyModifiers {
            shift: true,
            ..BrowserKeyModifiers::default()
        };
        let control = BrowserKeyModifiers {
            control: true,
            ..BrowserKeyModifiers::default()
        };

        assert_eq!(
            encode_browser_key_input_with_metadata(
                "a",
                "a",
                BrowserKeyModifiers::default(),
                "",
                0,
                modes,
            ),
            Some(b"\x1b[97u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata("A", "A", shift, "", 0, modes),
            Some(b"\x1b[97;2u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata(
                "ab",
                "ab",
                BrowserKeyModifiers::default(),
                "",
                0,
                modes,
            ),
            Some(b"\x1b[0u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata("Enter", "", control, "", 0, modes),
            Some(b"\x1b[13;5u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata("Tab", "", shift, "", 0, modes),
            Some(b"\x1b[9;2u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata("Backspace", "", control, "", 0, modes),
            Some(b"\x1b[127;5u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata("ArrowUp", "", control, "", 0, modes),
            Some(b"\x1b[1;5A".to_vec())
        );
    }

    #[test]
    fn browser_key_input_reports_kitty_modifier_keys_when_all_keys_mode_is_enabled() {
        let all_keys_modes = TerminalInputModes {
            kitty_keyboard_flags: KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES,
            ..TerminalInputModes::default()
        };
        let event_type_modes = TerminalInputModes {
            kitty_keyboard_flags: KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES
                | KITTY_KEYBOARD_REPORT_EVENT_TYPES,
            ..TerminalInputModes::default()
        };

        assert_eq!(
            encode_browser_key_input_with_metadata(
                "Shift",
                "",
                BrowserKeyModifiers::default(),
                "ShiftLeft",
                1,
                all_keys_modes,
            ),
            Some(b"\x1b[57441;2u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata(
                "Control",
                "",
                BrowserKeyModifiers::default(),
                "ControlRight",
                2,
                all_keys_modes,
            ),
            Some(b"\x1b[57448;5u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata_and_event_type(
                "Meta",
                "",
                BrowserKeyModifiers::default(),
                "MetaRight",
                2,
                BrowserKeyEventType::Release,
                event_type_modes,
            ),
            Some(b"\x1b[57450;1:3u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata(
                "Hyper",
                "",
                BrowserKeyModifiers::default(),
                "HyperLeft",
                1,
                all_keys_modes,
            ),
            Some(b"\x1b[57445;17u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata(
                "Shift",
                "",
                BrowserKeyModifiers::default(),
                "ShiftLeft",
                1,
                TerminalInputModes {
                    kitty_keyboard_flags: KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES,
                    ..TerminalInputModes::default()
                },
            ),
            None
        );
    }

    #[test]
    fn browser_key_input_reports_kitty_associated_text_when_enabled() {
        let modes = TerminalInputModes {
            kitty_keyboard_flags: KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES
                | KITTY_KEYBOARD_REPORT_ASSOCIATED_TEXT,
            ..TerminalInputModes::default()
        };
        let shift = BrowserKeyModifiers {
            shift: true,
            ..BrowserKeyModifiers::default()
        };
        let alt = BrowserKeyModifiers {
            alt: true,
            ..BrowserKeyModifiers::default()
        };
        let control = BrowserKeyModifiers {
            control: true,
            ..BrowserKeyModifiers::default()
        };

        assert_eq!(
            encode_browser_key_input_with_metadata(
                "a",
                "a",
                BrowserKeyModifiers::default(),
                "",
                0,
                modes,
            ),
            Some(b"\x1b[97;;97u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata("A", "A", shift, "", 0, modes),
            Some(b"\x1b[97;2;65u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata("é", "é", alt, "", 0, modes),
            Some(b"\x1b[233;3;233u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata(
                "ab",
                "ab",
                BrowserKeyModifiers::default(),
                "",
                0,
                modes,
            ),
            Some(b"\x1b[0;;97:98u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata("i", "i", control, "", 0, modes),
            Some(b"\x1b[105;5u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata(
                "\u{7f}",
                "\u{7f}",
                BrowserKeyModifiers::default(),
                "",
                0,
                modes,
            ),
            Some(b"\x1b[127u".to_vec())
        );
    }

    #[test]
    fn browser_key_input_reports_kitty_alternate_keys_when_enabled() {
        let all_keys_modes = TerminalInputModes {
            kitty_keyboard_flags: KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES
                | KITTY_KEYBOARD_REPORT_ALTERNATE_KEYS,
            ..TerminalInputModes::default()
        };
        let disambiguate_modes = TerminalInputModes {
            kitty_keyboard_flags: KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES
                | KITTY_KEYBOARD_REPORT_ALTERNATE_KEYS,
            ..TerminalInputModes::default()
        };
        let shift = BrowserKeyModifiers {
            shift: true,
            ..BrowserKeyModifiers::default()
        };
        let shift_control = BrowserKeyModifiers {
            shift: true,
            control: true,
            ..BrowserKeyModifiers::default()
        };

        assert_eq!(
            encode_browser_key_input_with_metadata("A", "A", shift, "KeyA", 0, all_keys_modes),
            Some(b"\x1b[97:65;2u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata("+", "+", shift, "Equal", 0, all_keys_modes),
            Some(b"\x1b[61:43;2u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata(
                "é",
                "é",
                BrowserKeyModifiers::default(),
                "KeyE",
                0,
                all_keys_modes,
            ),
            Some(b"\x1b[233::101u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata(
                "I",
                "I",
                shift_control,
                "KeyI",
                0,
                disambiguate_modes,
            ),
            Some(b"\x1b[105:73;6u".to_vec())
        );
    }

    #[test]
    fn browser_key_input_reports_kitty_event_types_when_enabled() {
        let modes = TerminalInputModes {
            kitty_keyboard_flags: KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES
                | KITTY_KEYBOARD_REPORT_EVENT_TYPES,
            ..TerminalInputModes::default()
        };
        let control = BrowserKeyModifiers {
            control: true,
            ..BrowserKeyModifiers::default()
        };
        let shift = BrowserKeyModifiers {
            shift: true,
            ..BrowserKeyModifiers::default()
        };

        assert_eq!(
            encode_browser_key_input_with_metadata_and_event_type(
                "i",
                "i",
                control,
                "",
                0,
                BrowserKeyEventType::Press,
                modes,
            ),
            Some(b"\x1b[105;5:1u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata_and_event_type(
                "i",
                "i",
                control,
                "",
                0,
                BrowserKeyEventType::Repeat,
                modes,
            ),
            Some(b"\x1b[105;5:2u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata_and_event_type(
                "i",
                "",
                control,
                "",
                0,
                BrowserKeyEventType::Release,
                modes,
            ),
            Some(b"\x1b[105;5:3u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata_and_event_type(
                "Escape",
                "",
                shift,
                "",
                0,
                BrowserKeyEventType::Press,
                modes,
            ),
            Some(b"\x1b[27;2:1u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata_and_event_type(
                "Enter",
                "",
                control,
                "",
                0,
                BrowserKeyEventType::Release,
                modes,
            ),
            None
        );
    }

    #[test]
    fn browser_key_input_reports_kitty_event_types_for_all_keys_mode() {
        let modes = TerminalInputModes {
            kitty_keyboard_flags: KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES
                | KITTY_KEYBOARD_REPORT_EVENT_TYPES,
            ..TerminalInputModes::default()
        };
        let control = BrowserKeyModifiers {
            control: true,
            ..BrowserKeyModifiers::default()
        };

        assert_eq!(
            encode_browser_key_input_with_metadata_and_event_type(
                "a",
                "a",
                BrowserKeyModifiers::default(),
                "",
                0,
                BrowserKeyEventType::Press,
                modes,
            ),
            Some(b"\x1b[97;1:1u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata_and_event_type(
                "a",
                "a",
                BrowserKeyModifiers::default(),
                "",
                0,
                BrowserKeyEventType::Repeat,
                modes,
            ),
            Some(b"\x1b[97;1:2u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata_and_event_type(
                "a",
                "",
                BrowserKeyModifiers::default(),
                "",
                0,
                BrowserKeyEventType::Release,
                modes,
            ),
            Some(b"\x1b[97;1:3u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata_and_event_type(
                "Enter",
                "",
                control,
                "",
                0,
                BrowserKeyEventType::Release,
                modes,
            ),
            Some(b"\x1b[13;5:3u".to_vec())
        );
    }

    #[test]
    fn browser_key_input_reports_kitty_event_types_for_functional_keys() {
        let modes = TerminalInputModes {
            kitty_keyboard_flags: KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES
                | KITTY_KEYBOARD_REPORT_EVENT_TYPES,
            ..TerminalInputModes::default()
        };
        let control = BrowserKeyModifiers {
            control: true,
            ..BrowserKeyModifiers::default()
        };

        assert_eq!(
            encode_browser_key_input_with_metadata_and_event_type(
                "ArrowUp",
                "",
                BrowserKeyModifiers::default(),
                "",
                0,
                BrowserKeyEventType::Press,
                modes,
            ),
            Some(b"\x1b[1;1:1A".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata_and_event_type(
                "ArrowUp",
                "",
                control,
                "",
                0,
                BrowserKeyEventType::Repeat,
                modes,
            ),
            Some(b"\x1b[1;5:2A".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata_and_event_type(
                "F3",
                "",
                BrowserKeyModifiers::default(),
                "",
                0,
                BrowserKeyEventType::Release,
                modes,
            ),
            Some(b"\x1b[13;1:3~".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata_and_event_type(
                "F5",
                "",
                control,
                "",
                0,
                BrowserKeyEventType::Release,
                modes,
            ),
            Some(b"\x1b[15;5:3~".to_vec())
        );
    }

    #[test]
    fn browser_key_input_reports_kitty_pua_functional_keys_when_protocol_is_enabled() {
        let disambiguate_modes = TerminalInputModes {
            kitty_keyboard_flags: KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES,
            ..TerminalInputModes::default()
        };
        let event_type_modes = TerminalInputModes {
            kitty_keyboard_flags: KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES
                | KITTY_KEYBOARD_REPORT_EVENT_TYPES,
            ..TerminalInputModes::default()
        };

        assert_eq!(
            encode_browser_key_input_with_metadata(
                "F13",
                "",
                BrowserKeyModifiers::default(),
                "",
                0,
                disambiguate_modes,
            ),
            Some(b"\x1b[57376u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata(
                "CapsLock",
                "",
                BrowserKeyModifiers::default(),
                "",
                0,
                disambiguate_modes,
            ),
            Some(b"\x1b[57358u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata_and_event_type(
                "MediaTrackNext",
                "",
                BrowserKeyModifiers::default(),
                "",
                0,
                BrowserKeyEventType::Release,
                event_type_modes,
            ),
            Some(b"\x1b[57435;1:3u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata(
                "F13",
                "",
                BrowserKeyModifiers::default(),
                "",
                0,
                TerminalInputModes::default(),
            ),
            None
        );
    }

    #[test]
    fn browser_key_input_reports_kitty_meta_modified_functional_keys() {
        let modes = TerminalInputModes {
            kitty_keyboard_flags: KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES,
            ..TerminalInputModes::default()
        };
        let meta = BrowserKeyModifiers {
            meta: true,
            ..BrowserKeyModifiers::default()
        };

        assert_eq!(
            encode_browser_key_input_with_metadata("ArrowUp", "", meta, "", 0, modes),
            Some(b"\x1b[1;9A".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata("F3", "", meta, "", 0, modes),
            Some(b"\x1b[13;9~".to_vec())
        );
    }

    #[test]
    fn browser_key_input_combines_kitty_event_types_and_associated_text() {
        let modes = TerminalInputModes {
            kitty_keyboard_flags: KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES
                | KITTY_KEYBOARD_REPORT_EVENT_TYPES
                | KITTY_KEYBOARD_REPORT_ASSOCIATED_TEXT,
            ..TerminalInputModes::default()
        };
        let shift = BrowserKeyModifiers {
            shift: true,
            ..BrowserKeyModifiers::default()
        };

        assert_eq!(
            encode_browser_key_input_with_metadata_and_event_type(
                "a",
                "a",
                BrowserKeyModifiers::default(),
                "",
                0,
                BrowserKeyEventType::Press,
                modes,
            ),
            Some(b"\x1b[97;1:1;97u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata_and_event_type(
                "A",
                "A",
                shift,
                "",
                0,
                BrowserKeyEventType::Press,
                modes,
            ),
            Some(b"\x1b[97;2:1;65u".to_vec())
        );
    }

    #[test]
    fn browser_key_input_ignores_releases_until_kitty_event_reporting_is_enabled() {
        assert_eq!(
            encode_browser_key_input_with_metadata_and_event_type(
                "x",
                "",
                BrowserKeyModifiers::default(),
                "",
                0,
                BrowserKeyEventType::Release,
                TerminalInputModes::default(),
            ),
            None
        );
    }

    #[test]
    fn browser_key_input_keeps_legacy_control_characters_until_kitty_protocol_is_enabled() {
        assert_eq!(
            encode_browser_key_input("i", "i", true, TerminalInputModes::default()),
            Some(vec![0x09])
        );
    }

    #[test]
    fn browser_key_modifiers_decode_browser_mask() {
        assert_eq!(
            BrowserKeyModifiers::from_browser_mask(true, 0b111),
            BrowserKeyModifiers {
                control: true,
                shift: true,
                alt: true,
                meta: true,
                hyper: false,
            }
        );
        assert_eq!(
            BrowserKeyModifiers::from_browser_mask(false, 0b010),
            BrowserKeyModifiers {
                alt: true,
                ..BrowserKeyModifiers::default()
            }
        );
    }

    #[test]
    fn browser_modifier_key_mapper_uses_code_or_location() {
        assert_eq!(
            browser_modifier_key_from_code("ShiftLeft"),
            Some(BrowserModifierKey::LeftShift)
        );
        assert_eq!(
            browser_modifier_key_from_code("MetaRight"),
            Some(BrowserModifierKey::RightSuper)
        );
        assert_eq!(
            browser_modifier_key_from_code("HyperLeft"),
            Some(BrowserModifierKey::LeftHyper)
        );
        assert_eq!(browser_modifier_key_from_code("KeyA"), None);
        assert_eq!(
            browser_modifier_key_from_location("Alt", 1),
            Some(BrowserModifierKey::LeftAlt)
        );
        assert_eq!(
            browser_modifier_key_from_location("Control", 2),
            Some(BrowserModifierKey::RightControl)
        );
        assert_eq!(
            browser_modifier_key_from_location("Hyper", 2),
            Some(BrowserModifierKey::RightHyper)
        );
        assert_eq!(browser_modifier_key_from_location("Shift", 0), None);
    }

    #[test]
    fn browser_key_input_separates_top_row_and_keypad_digit_modes() {
        let modes = TerminalInputModes {
            application_cursor_keys: false,
            application_keypad: true,
            ..TerminalInputModes::default()
        };

        assert_eq!(
            encode_browser_key_input_with_metadata(
                "1",
                "1",
                BrowserKeyModifiers::default(),
                "Digit1",
                0,
                modes,
            ),
            Some(b"1".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata(
                "1",
                "1",
                BrowserKeyModifiers::default(),
                "Numpad1",
                3,
                TerminalInputModes::default(),
            ),
            Some(b"1".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata(
                "1",
                "1",
                BrowserKeyModifiers::default(),
                "Numpad1",
                3,
                modes,
            ),
            Some(b"\x1bOq".to_vec())
        );
    }

    #[test]
    fn browser_key_input_uses_application_keypad_sequences() {
        let modes = TerminalInputModes {
            application_cursor_keys: false,
            application_keypad: true,
            ..TerminalInputModes::default()
        };
        let cases = [
            ("0", "0", "Numpad0", b"\x1bOp".as_slice()),
            ("9", "9", "Numpad9", b"\x1bOy".as_slice()),
            (".", ".", "NumpadDecimal", b"\x1bOn".as_slice()),
            (",", ",", "NumpadComma", b"\x1bOl".as_slice()),
            ("+", "+", "NumpadAdd", b"\x1bOk".as_slice()),
            ("-", "-", "NumpadSubtract", b"\x1bOm".as_slice()),
            ("*", "*", "NumpadMultiply", b"\x1bOj".as_slice()),
            ("/", "/", "NumpadDivide", b"\x1bOo".as_slice()),
            ("Enter", "", "NumpadEnter", b"\x1bOM".as_slice()),
        ];

        for (key, text, code, expected) in cases {
            assert_eq!(
                encode_browser_key_input_with_metadata(
                    key,
                    text,
                    BrowserKeyModifiers::default(),
                    code,
                    3,
                    modes,
                ),
                Some(expected.to_vec())
            );
        }
    }

    #[test]
    fn browser_key_input_reports_kitty_keypad_keys_when_disambiguation_is_enabled() {
        let modes = TerminalInputModes {
            kitty_keyboard_flags: KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES,
            ..TerminalInputModes::default()
        };

        assert_eq!(
            encode_browser_key_input_with_metadata(
                "1",
                "1",
                BrowserKeyModifiers::default(),
                "Digit1",
                0,
                modes,
            ),
            Some(b"1".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata(
                "1",
                "1",
                BrowserKeyModifiers::default(),
                "Numpad1",
                3,
                modes,
            ),
            Some(b"\x1b[57400u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata(
                "Enter",
                "",
                BrowserKeyModifiers::default(),
                "NumpadEnter",
                3,
                modes,
            ),
            Some(b"\x1b[57414u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata(
                "ArrowLeft",
                "",
                BrowserKeyModifiers::default(),
                "Numpad4",
                3,
                modes,
            ),
            Some(b"\x1b[57417u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata(
                "ArrowLeft",
                "",
                BrowserKeyModifiers::default(),
                "Numpad4",
                3,
                TerminalInputModes::default(),
            ),
            Some(b"\x1b[D".to_vec())
        );
    }

    #[test]
    fn browser_key_input_reports_kitty_keypad_associated_text_and_event_types() {
        let associated_text_modes = TerminalInputModes {
            kitty_keyboard_flags: KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES
                | KITTY_KEYBOARD_REPORT_ASSOCIATED_TEXT,
            ..TerminalInputModes::default()
        };
        let event_type_modes = TerminalInputModes {
            kitty_keyboard_flags: KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES
                | KITTY_KEYBOARD_REPORT_EVENT_TYPES,
            ..TerminalInputModes::default()
        };

        assert_eq!(
            encode_browser_key_input_with_metadata(
                ".",
                ".",
                BrowserKeyModifiers::default(),
                "NumpadDecimal",
                3,
                associated_text_modes,
            ),
            Some(b"\x1b[57409;;46u".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata_and_event_type(
                "+",
                "",
                BrowserKeyModifiers::default(),
                "NumpadAdd",
                3,
                BrowserKeyEventType::Release,
                event_type_modes,
            ),
            Some(b"\x1b[57413;1:3u".to_vec())
        );
    }

    #[test]
    fn browser_key_input_keeps_main_enter_and_unsupported_keypad_fallbacks() {
        let modes = TerminalInputModes {
            application_cursor_keys: false,
            application_keypad: true,
            ..TerminalInputModes::default()
        };

        assert_eq!(
            encode_browser_key_input_with_metadata(
                "Enter",
                "",
                BrowserKeyModifiers::default(),
                "Enter",
                0,
                modes,
            ),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata(
                "=",
                "=",
                BrowserKeyModifiers::default(),
                "NumpadEqual",
                3,
                modes,
            ),
            Some(b"=".to_vec())
        );
        assert_eq!(
            encode_browser_key_input_with_metadata(
                "1",
                "1",
                BrowserKeyModifiers {
                    control: true,
                    ..BrowserKeyModifiers::default()
                },
                "Numpad1",
                3,
                modes,
            ),
            None
        );
    }

    #[test]
    fn browser_echo_makes_printable_input_visible() {
        assert_eq!(browser_echo_bytes(b"xy\r\x03"), b"xy\r\n> ".to_vec());
    }

    #[test]
    fn browser_ime_preedit_updates_state_without_writing_input() {
        let mut composition = ImeComposition::default();

        let changed =
            apply_browser_ime_preedit(&mut composition, "pinyin".to_owned(), Some((2, 4)));

        assert!(changed);
        assert!(composition.is_enabled());
        assert!(composition.is_active());
        assert_eq!(composition.preedit(), "pinyin");
        assert_eq!(composition.caret(), Some((2, 4)));
    }

    #[test]
    fn browser_ime_commit_clears_preedit_and_returns_commit_text() {
        let mut composition = ImeComposition::default();
        composition.set_preedit("ni".to_owned(), Some((2, 2)));

        let result = apply_browser_ime_commit(&mut composition, "你".to_owned());

        assert_eq!(
            result,
            BrowserImeCommitResult {
                changed: true,
                committed_text: Some("你".to_owned()),
            }
        );
        assert!(composition.is_enabled());
        assert!(!composition.is_active());
    }

    #[test]
    fn browser_ime_empty_commit_clears_preedit_without_commit_text() {
        let mut composition = ImeComposition::default();
        composition.set_preedit("kana".to_owned(), Some((4, 4)));

        let result = apply_browser_ime_commit(&mut composition, String::new());

        assert_eq!(
            result,
            BrowserImeCommitResult {
                changed: true,
                committed_text: None,
            }
        );
        assert!(composition.is_enabled());
        assert!(!composition.is_active());
    }

    #[test]
    fn browser_search_ime_commit_updates_query_without_gateway_input() {
        let mut composition = ImeComposition::default();
        composition.set_preedit("zhong".to_owned(), Some((5, 5)));
        let mut terminal = BasicTerminal::new(GridSize::new(3, 24));
        terminal.feed("alpha 中 beta".as_bytes());
        let mut search = TerminalSearch::default();
        search.open(&terminal.search_text_rows(), None);
        let gateway_input = Vec::<u8>::new();

        let result = apply_browser_ime_commit(&mut composition, "中".to_owned());
        if let Some(text) = result.committed_text.as_deref() {
            search.input_text(&terminal.search_text_rows(), text);
        }

        assert_eq!(
            result,
            BrowserImeCommitResult {
                changed: true,
                committed_text: Some("中".to_owned()),
            }
        );
        assert!(composition.is_enabled());
        assert!(!composition.is_active());
        assert_eq!(search.query(), "中");
        assert_eq!(search.match_count(), 1);
        assert!(gateway_input.is_empty());
    }

    #[test]
    fn browser_terminal_ime_commit_still_writes_utf8_once_after_routing() {
        let mut composition = ImeComposition::default();
        composition.set_preedit("ni".to_owned(), Some((2, 2)));
        let mut gateway_input = Vec::new();

        let result = apply_browser_ime_commit(&mut composition, "你".to_owned());
        if let Some(text) = result.committed_text.as_deref() {
            gateway_input.extend_from_slice(text.as_bytes());
        }

        assert_eq!(gateway_input, "你".as_bytes());
        assert_eq!(result.committed_text, Some("你".to_owned()));
    }

    #[test]
    fn browser_search_status_and_cursor_include_ime_preedit_without_mutating_query() {
        let rows = vec![witty_core::SearchTextRow {
            id: witty_core::SearchRowId::screen(0),
            visible_row: Some(0),
            text: "find 中".to_owned(),
            columns: Vec::new(),
        }];
        let mut search = TerminalSearch::default();
        search.open(&rows, Some("find "));
        let mut composition = ImeComposition::default();
        composition.set_preedit("中", Some(("中".len(), "中".len())));

        assert!(
            browser_search_status_text_with_ime(&search, Some(&composition))
                .contains("Find: find 中")
        );
        assert_eq!(
            browser_search_ime_cursor_cell(&search, &composition, GridSize::new(3, 40)),
            CellPoint::new(2, 1 + 6 + 5 + 2)
        );
        assert_eq!(search.query(), "find ");
    }

    #[test]
    fn browser_terminal_ime_cursor_tracks_preedit_caret_and_clamps_to_grid() {
        let mut composition = ImeComposition::default();
        composition.set_preedit("a你b", Some(("a你".len(), "a你".len())));

        assert_eq!(
            browser_terminal_ime_cursor_cell(
                CellPoint::new(1, 4),
                &composition,
                GridSize::new(3, 12)
            ),
            CellPoint::new(1, 7)
        );

        assert_eq!(
            browser_terminal_ime_cursor_cell(
                CellPoint::new(9, 10),
                &composition,
                GridSize::new(3, 12)
            ),
            CellPoint::new(2, 11)
        );
    }

    #[test]
    fn browser_ime_caret_and_cursor_css_rect_are_deterministic() {
        assert_eq!(browser_ime_caret("abc", 1, 3), Some((1, 3)));
        assert_eq!(browser_ime_caret("abc", -1, 3), None);
        assert_eq!(browser_ime_caret("", 0, 0), None);

        let rect = browser_cursor_css_rect(
            CursorState {
                position: CellPoint::new(2, 3),
                ..CursorState::default()
            },
            CellMetrics::default(),
            2.0,
        );

        assert_eq!(
            rect,
            BrowserCursorCssRect {
                left: 17.5,
                top: 22.0,
                width: 4.5,
                height: 9.0,
            }
        );
    }

    #[test]
    fn browser_entry_frame_uses_mock_terminal_snapshot() {
        let frame = browser_entry_frame();

        assert_eq!(frame.stats.visible_rows, 4);
        assert_eq!(frame.stats.visible_cols, 16);
        assert_eq!(frame.stats.glyph_runs, 4);
        assert_eq!(frame.stats.glyph_chars, 23);
    }
}

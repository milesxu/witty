//! Renderer-facing frame planning and the first static wgpu renderer.

use std::{borrow::Cow, collections::BTreeSet, fmt::Debug, ops::Range, sync::Arc};

use anyhow::{Context as _, Result};
use glyphon::{
    fontdb, Attrs, Buffer, Cache, Color, Family, FontSystem, Metrics, Resolution, Shaping, Style,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport, Weight,
};
use unicode_width::UnicodeWidthStr;
use witty_core::{
    BaselineShift, CellFlags, CellPoint, CellRange, CursorShape, CursorState, DamageRegion,
    GridSize, HyperlinkId, RenderCell, RenderRow, RenderSnapshot, Rgba, SearchHighlight,
    UnderlineStyle,
};

pub const DEFAULT_TERMINAL_FONT_SIZE: u16 = 14;
const DEFAULT_TERMINAL_LINE_HEIGHT: f32 = 18.0;
const DEFAULT_TERMINAL_CELL_WIDTH: f32 = 9.0;
const DEFAULT_TERMINAL_PADDING: PixelPoint = PixelPoint { x: 0.0, y: 0.0 };
const DEFAULT_SURFACE_CLEAR_COLOR: Rgba = Rgba::BLACK;
const BASELINE_SHIFT_FONT_SCALE: f32 = 0.72;
const MAX_GLYPHON_TEXT_AREA_CHARS: usize = 120;
const MAX_GLYPHON_RENDERER_CHARS: usize = 120;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelPoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelSize {
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellMetrics {
    pub cell: PixelSize,
    pub padding: PixelPoint,
}

impl CellMetrics {
    pub fn for_font_size(font_size: u16) -> Self {
        let scale = f32::from(font_size) / f32::from(DEFAULT_TERMINAL_FONT_SIZE);
        Self {
            cell: PixelSize {
                width: DEFAULT_TERMINAL_CELL_WIDTH * scale,
                height: DEFAULT_TERMINAL_LINE_HEIGHT * scale,
            },
            padding: DEFAULT_TERMINAL_PADDING,
        }
    }

    pub fn scale(self, scale_factor: f32) -> Self {
        let scale_factor = sane_font_scale_factor(scale_factor);
        Self {
            cell: PixelSize {
                width: self.cell.width * scale_factor,
                height: self.cell.height * scale_factor,
            },
            padding: PixelPoint {
                x: self.padding.x * scale_factor,
                y: self.padding.y * scale_factor,
            },
        }
    }
}

impl Default for CellMetrics {
    fn default() -> Self {
        Self::for_font_size(DEFAULT_TERMINAL_FONT_SIZE)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RectBatchItem {
    pub origin: PixelPoint,
    pub size: PixelSize,
    pub color: Rgba,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GlyphBatchItem {
    pub origin: PixelPoint,
    pub text: String,
    pub color: Rgba,
    pub style_flags: CellFlags,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FramePlan {
    pub backgrounds: Vec<RectBatchItem>,
    pub glyphs: Vec<GlyphBatchItem>,
    pub cursor: Option<RectBatchItem>,
    pub selection: Vec<RectBatchItem>,
    pub search_highlights: Vec<RectBatchItem>,
    pub hyperlink_hover: Vec<RectBatchItem>,
    pub hyperlink_underlines: Vec<RectBatchItem>,
    pub text_decorations: Vec<RectBatchItem>,
    pub ime_preedit: Vec<RectBatchItem>,
    pub damage: DamageRegion,
    pub stats: FrameStats,
}

impl FramePlan {
    pub fn refresh_stats(&mut self, visible_rows: u16, visible_cols: u16) {
        self.refresh_stats_with_rows(visible_rows, visible_cols, 0, visible_rows as usize);
    }

    pub fn refresh_stats_with_rows(
        &mut self,
        visible_rows: u16,
        visible_cols: u16,
        reused_rows: usize,
        rebuilt_rows: usize,
    ) {
        let rect_count = self.backgrounds.len()
            + self.search_highlights.len()
            + self.selection.len()
            + self.hyperlink_hover.len()
            + self.hyperlink_underlines.len()
            + self.text_decorations.len()
            + self.ime_preedit.len()
            + usize::from(self.cursor.is_some());
        self.stats = FrameStats {
            visible_rows,
            visible_cols,
            background_runs: self.backgrounds.len(),
            glyph_runs: self.glyphs.len(),
            glyph_chars: self
                .glyphs
                .iter()
                .map(|glyph| glyph.text.chars().count())
                .sum(),
            glyph_prepare_batches: glyph_prepare_batch_count(&self.glyphs),
            max_glyph_run_chars: self
                .glyphs
                .iter()
                .map(|glyph| glyph.text.chars().count())
                .max()
                .unwrap_or(0),
            selection_rects: self.selection.len(),
            search_highlight_rects: self.search_highlights.len(),
            hyperlink_hover_rects: self.hyperlink_hover.len(),
            hyperlink_underline_rects: self.hyperlink_underlines.len(),
            text_decoration_rects: self.text_decorations.len(),
            ime_preedit_rects: self.ime_preedit.len(),
            search_active_visible: self
                .search_highlights
                .iter()
                .any(|rect| rect.color == search_highlight_color(SearchHighlightKind::Active)),
            cursor_visible: self.cursor.is_some(),
            rect_vertices: rect_count * RECT_VERTICES,
            rect_vertex_capacity: rect_vertex_capacity_for_rect_count(rect_count),
            full_damage: self.damage.is_full(),
            damage_regions: self.damage.region_count(),
            reused_rows,
            rebuilt_rows,
        };
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameStats {
    pub visible_rows: u16,
    pub visible_cols: u16,
    pub background_runs: usize,
    pub glyph_runs: usize,
    pub glyph_chars: usize,
    pub glyph_prepare_batches: usize,
    pub max_glyph_run_chars: usize,
    pub selection_rects: usize,
    pub search_highlight_rects: usize,
    pub hyperlink_hover_rects: usize,
    pub hyperlink_underline_rects: usize,
    pub text_decoration_rects: usize,
    pub ime_preedit_rects: usize,
    pub search_active_visible: bool,
    pub cursor_visible: bool,
    pub rect_vertices: usize,
    pub rect_vertex_capacity: usize,
    pub full_damage: bool,
    pub damage_regions: usize,
    pub reused_rows: usize,
    pub rebuilt_rows: usize,
}

fn glyph_prepare_batch_count(glyphs: &[GlyphBatchItem]) -> usize {
    let mut batches = 0usize;
    let mut chars = 0usize;

    for glyph in glyphs {
        let glyph_chars = glyph.text.chars().count().max(1);
        if batches == 0 {
            batches = 1;
        } else if chars.saturating_add(glyph_chars) > MAX_GLYPHON_RENDERER_CHARS {
            batches += 1;
            chars = 0;
        }
        chars = chars.saturating_add(glyph_chars);
    }

    batches
}

fn rect_vertex_capacity_for_rect_count(rect_count: usize) -> usize {
    if rect_count == 0 {
        0
    } else {
        rect_vertex_capacity_for_len(rect_count * RECT_VERTICES)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FramePlanner {
    metrics: CellMetrics,
    blink_visible: bool,
}

impl FramePlanner {
    pub fn new(metrics: CellMetrics) -> Self {
        Self {
            metrics,
            blink_visible: true,
        }
    }

    pub fn with_blink_visible(mut self, visible: bool) -> Self {
        self.blink_visible = visible;
        self
    }

    pub fn plan(&self, snapshot: &RenderSnapshot) -> FramePlan {
        let mut frame = FramePlan::default();

        for row in &snapshot.rows {
            self.plan_row(row).extend_frame(&mut frame);
        }

        self.apply_dynamic_overlays(&mut frame, snapshot);
        frame.damage = snapshot.damage.clone();
        frame.refresh_stats_with_rows(
            snapshot.rows.len() as u16,
            snapshot.size.cols,
            0,
            snapshot.rows.len(),
        );
        frame
    }

    fn plan_row(&self, row: &RenderRow) -> PlannedRow {
        let mut planned = PlannedRow::default();
        let mut background_run = None;
        let mut glyph_run = None;
        let mut decoration_runs = TextDecorationRuns::default();

        for cell in &row.cells {
            self.extend_background_run(&mut planned, &mut background_run, cell);
            self.extend_glyph_run(&mut planned, &mut glyph_run, cell);
            self.extend_text_decoration_runs(&mut planned, &mut decoration_runs, cell);
        }

        self.flush_background_run(&mut planned, background_run);
        self.flush_glyph_run(&mut planned, glyph_run);
        self.flush_text_decoration_runs(&mut planned, decoration_runs);
        planned
    }

    fn apply_dynamic_overlays(&self, frame: &mut FramePlan, snapshot: &RenderSnapshot) {
        frame.search_highlights.extend(
            snapshot
                .search_highlights
                .iter()
                .flat_map(|highlight| self.search_highlight_rects(*highlight, snapshot.size.cols)),
        );

        if let Some(selection) = snapshot.selection {
            frame
                .selection
                .extend(self.selection_rects(selection, snapshot.size.cols));
        }

        let (hyperlink_underlines, hyperlink_hover) = self.hyperlink_overlay_rects(snapshot);
        frame.hyperlink_underlines.extend(hyperlink_underlines);
        frame.hyperlink_hover.extend(hyperlink_hover);

        if snapshot.cursor.visible {
            frame.cursor = Some(self.cursor_rect(snapshot.cursor, snapshot.cursor_color));
        }
    }

    fn cursor_rect(&self, cursor: CursorState, cursor_color: Option<Rgba>) -> RectBatchItem {
        let origin = self.cell_origin(cursor.position);
        let color = cursor_color.unwrap_or_else(|| Rgba::rgb(180, 180, 180));
        match cursor.shape {
            CursorShape::Block => RectBatchItem {
                origin,
                size: self.metrics.cell,
                color,
            },
            CursorShape::Bar => RectBatchItem {
                origin,
                size: PixelSize {
                    width: self.cursor_stem_width(),
                    height: self.metrics.cell.height,
                },
                color,
            },
            CursorShape::Underline => {
                let height = self.cursor_underline_height();
                RectBatchItem {
                    origin: PixelPoint {
                        x: origin.x,
                        y: origin.y + self.metrics.cell.height - height,
                    },
                    size: PixelSize {
                        width: self.metrics.cell.width,
                        height,
                    },
                    color,
                }
            }
        }
    }

    fn cursor_stem_width(&self) -> f32 {
        (self.metrics.cell.width * 0.18).clamp(1.0, self.metrics.cell.width)
    }

    fn cursor_underline_height(&self) -> f32 {
        (self.metrics.cell.height * 0.15).clamp(1.0, self.metrics.cell.height)
    }

    fn extend_background_run(
        &self,
        planned: &mut PlannedRow,
        current: &mut Option<BackgroundRunBuilder>,
        cell: &RenderCell,
    ) {
        match current {
            Some(run)
                if run.color == cell.style.background
                    && run.row == cell.point.row
                    && run.next_col == cell.point.col =>
            {
                run.cols = run.cols.saturating_add(u16::from(cell.width));
                run.next_col = run.next_col.saturating_add(u16::from(cell.width));
            }
            _ => {
                let old = current.take();
                self.flush_background_run(planned, old);
                *current = Some(BackgroundRunBuilder {
                    row: cell.point.row,
                    start_col: cell.point.col,
                    next_col: cell.point.col.saturating_add(u16::from(cell.width)),
                    cols: u16::from(cell.width),
                    color: cell.style.background,
                });
            }
        }
    }

    fn flush_background_run(&self, planned: &mut PlannedRow, run: Option<BackgroundRunBuilder>) {
        let Some(run) = run else {
            return;
        };

        planned.backgrounds.push(RectBatchItem {
            origin: self.cell_origin(CellPoint::new(run.row, run.start_col)),
            size: PixelSize {
                width: self.metrics.cell.width * f32::from(run.cols),
                height: self.metrics.cell.height,
            },
            color: run.color,
        });
    }

    fn extend_glyph_run(
        &self,
        planned: &mut PlannedRow,
        current: &mut Option<GlyphRunBuilder>,
        cell: &RenderCell,
    ) {
        if cell.style.flags.conceal
            || (cell.style.flags.blink && !self.blink_visible)
            || cell.text.trim().is_empty()
        {
            let old = current.take();
            self.flush_glyph_run(planned, old);
            return;
        }

        let cell_chars = cell.text.chars().count().max(1);
        let foreground = effective_foreground(cell);
        match current {
            Some(run)
                if run.foreground == foreground
                    && run.flags == cell.style.flags
                    && run.row == cell.point.row
                    && run.next_col == cell.point.col
                    && run.char_count.saturating_add(cell_chars) <= MAX_GLYPHON_TEXT_AREA_CHARS =>
            {
                run.text.push_str(&cell.text);
                run.char_count += cell_chars;
                run.next_col = run.next_col.saturating_add(u16::from(cell.width));
            }
            _ => {
                let old = current.take();
                self.flush_glyph_run(planned, old);
                *current = Some(GlyphRunBuilder {
                    row: cell.point.row,
                    start_col: cell.point.col,
                    next_col: cell.point.col.saturating_add(u16::from(cell.width)),
                    text: cell.text.clone(),
                    char_count: cell_chars,
                    foreground,
                    flags: cell.style.flags,
                });
            }
        }
    }

    fn flush_glyph_run(&self, planned: &mut PlannedRow, run: Option<GlyphRunBuilder>) {
        let Some(run) = run else {
            return;
        };

        planned.glyphs.push(GlyphBatchItem {
            origin: self.glyph_origin(CellPoint::new(run.row, run.start_col), run.flags),
            text: run.text,
            color: run.foreground,
            style_flags: run.flags,
        });
    }

    fn glyph_origin(&self, point: CellPoint, flags: CellFlags) -> PixelPoint {
        let mut origin = self.cell_origin(point);
        match flags.baseline_shift {
            BaselineShift::Normal => {}
            BaselineShift::Superscript => origin.y -= self.metrics.cell.height * 0.25,
            BaselineShift::Subscript => origin.y += self.metrics.cell.height * 0.18,
        }
        origin
    }

    fn extend_text_decoration_runs(
        &self,
        planned: &mut PlannedRow,
        current: &mut TextDecorationRuns,
        cell: &RenderCell,
    ) {
        if cell.style.flags.conceal || (cell.style.flags.blink && !self.blink_visible) {
            self.flush_text_decoration_run(planned, current.underline.take());
            self.flush_text_decoration_run(planned, current.strike.take());
            self.flush_text_decoration_run(planned, current.frame.take());
            self.flush_text_decoration_run(planned, current.encircle.take());
            self.flush_text_decoration_run(planned, current.overline.take());
            return;
        }

        let foreground = effective_foreground(cell);
        if let Some(kind) = underline_decoration_kind(cell.style.flags.underline_style)
            .filter(|_| cell.style.flags.underline)
        {
            let color = cell.style.underline_color.unwrap_or(foreground);
            self.extend_text_decoration_run(planned, &mut current.underline, kind, color, cell);
        } else {
            self.flush_text_decoration_run(planned, current.underline.take());
        }

        if cell.style.flags.strike {
            self.extend_text_decoration_run(
                planned,
                &mut current.strike,
                TextDecorationKind::Strikethrough,
                foreground,
                cell,
            );
        } else {
            self.flush_text_decoration_run(planned, current.strike.take());
        }

        if cell.style.flags.framed {
            self.extend_text_decoration_run(
                planned,
                &mut current.frame,
                TextDecorationKind::Frame,
                foreground,
                cell,
            );
        } else {
            self.flush_text_decoration_run(planned, current.frame.take());
        }

        if cell.style.flags.encircled {
            self.extend_text_decoration_run(
                planned,
                &mut current.encircle,
                TextDecorationKind::Encircle,
                foreground,
                cell,
            );
        } else {
            self.flush_text_decoration_run(planned, current.encircle.take());
        }

        if cell.style.flags.overline {
            self.extend_text_decoration_run(
                planned,
                &mut current.overline,
                TextDecorationKind::Overline,
                foreground,
                cell,
            );
        } else {
            self.flush_text_decoration_run(planned, current.overline.take());
        }
    }

    fn extend_text_decoration_run(
        &self,
        planned: &mut PlannedRow,
        current: &mut Option<TextDecorationRunBuilder>,
        kind: TextDecorationKind,
        color: Rgba,
        cell: &RenderCell,
    ) {
        let cell_width = u16::from(cell.width.max(1));
        match current {
            Some(run)
                if run.kind == kind
                    && run.color == color
                    && run.row == cell.point.row
                    && run.next_col == cell.point.col =>
            {
                run.cols = run.cols.saturating_add(cell_width);
                run.next_col = run.next_col.saturating_add(cell_width);
            }
            _ => {
                let old = current.take();
                self.flush_text_decoration_run(planned, old);
                *current = Some(TextDecorationRunBuilder {
                    kind,
                    row: cell.point.row,
                    start_col: cell.point.col,
                    next_col: cell.point.col.saturating_add(cell_width),
                    cols: cell_width,
                    color,
                });
            }
        }
    }

    fn flush_text_decoration_runs(&self, planned: &mut PlannedRow, runs: TextDecorationRuns) {
        self.flush_text_decoration_run(planned, runs.underline);
        self.flush_text_decoration_run(planned, runs.strike);
        self.flush_text_decoration_run(planned, runs.frame);
        self.flush_text_decoration_run(planned, runs.encircle);
        self.flush_text_decoration_run(planned, runs.overline);
    }

    fn flush_text_decoration_run(
        &self,
        planned: &mut PlannedRow,
        run: Option<TextDecorationRunBuilder>,
    ) {
        let Some(run) = run else {
            return;
        };

        let origin = self.cell_origin(CellPoint::new(run.row, run.start_col));
        let width = self.metrics.cell.width * f32::from(run.cols);
        let thickness = self.text_decoration_thickness();
        match run.kind {
            TextDecorationKind::SingleUnderline => {
                planned.text_decorations.push(RectBatchItem {
                    origin: PixelPoint {
                        x: origin.x,
                        y: origin.y + self.metrics.cell.height - thickness,
                    },
                    size: PixelSize {
                        width,
                        height: thickness,
                    },
                    color: run.color,
                });
            }
            TextDecorationKind::DoubleUnderline => {
                planned.text_decorations.push(RectBatchItem {
                    origin: PixelPoint {
                        x: origin.x,
                        y: origin.y + self.metrics.cell.height - thickness * 3.0,
                    },
                    size: PixelSize {
                        width,
                        height: thickness,
                    },
                    color: run.color,
                });
                planned.text_decorations.push(RectBatchItem {
                    origin: PixelPoint {
                        x: origin.x,
                        y: origin.y + self.metrics.cell.height - thickness,
                    },
                    size: PixelSize {
                        width,
                        height: thickness,
                    },
                    color: run.color,
                });
            }
            TextDecorationKind::DottedUnderline => {
                let dot_size = thickness.min(self.metrics.cell.width);
                self.push_cellwise_underline_segments(
                    planned, origin, run.cols, dot_size, run.color,
                );
            }
            TextDecorationKind::DashedUnderline => {
                let min_width = thickness.min(self.metrics.cell.width);
                let dash_width =
                    (self.metrics.cell.width * 0.62).clamp(min_width, self.metrics.cell.width);
                self.push_cellwise_underline_segments(
                    planned, origin, run.cols, dash_width, run.color,
                );
            }
            TextDecorationKind::CurlyUnderline => {
                self.push_curly_underline_segments(planned, origin, run.cols, run.color);
            }
            TextDecorationKind::Strikethrough => {
                planned.text_decorations.push(RectBatchItem {
                    origin: PixelPoint {
                        x: origin.x,
                        y: origin.y + (self.metrics.cell.height - thickness) * 0.5,
                    },
                    size: PixelSize {
                        width,
                        height: thickness,
                    },
                    color: run.color,
                });
            }
            TextDecorationKind::Frame => {
                self.push_frame_segments(planned, origin, width, run.color);
            }
            TextDecorationKind::Encircle => {
                self.push_encircle_segments(planned, origin, width, run.color);
            }
            TextDecorationKind::Overline => {
                planned.text_decorations.push(RectBatchItem {
                    origin,
                    size: PixelSize {
                        width,
                        height: thickness,
                    },
                    color: run.color,
                });
            }
        }
    }

    fn push_frame_segments(
        &self,
        planned: &mut PlannedRow,
        origin: PixelPoint,
        width: f32,
        color: Rgba,
    ) {
        let thickness = self.text_decoration_thickness();
        planned.text_decorations.push(RectBatchItem {
            origin,
            size: PixelSize {
                width,
                height: thickness,
            },
            color,
        });
        planned.text_decorations.push(RectBatchItem {
            origin: PixelPoint {
                x: origin.x,
                y: origin.y + self.metrics.cell.height - thickness,
            },
            size: PixelSize {
                width,
                height: thickness,
            },
            color,
        });
        planned.text_decorations.push(RectBatchItem {
            origin,
            size: PixelSize {
                width: thickness,
                height: self.metrics.cell.height,
            },
            color,
        });
        planned.text_decorations.push(RectBatchItem {
            origin: PixelPoint {
                x: origin.x + width - thickness,
                y: origin.y,
            },
            size: PixelSize {
                width: thickness,
                height: self.metrics.cell.height,
            },
            color,
        });
    }

    fn push_encircle_segments(
        &self,
        planned: &mut PlannedRow,
        origin: PixelPoint,
        width: f32,
        color: Rgba,
    ) {
        let thickness = self.text_decoration_thickness();
        let radius_x = (self.metrics.cell.width * 0.35).clamp(thickness, width * 0.5);
        let radius_y =
            (self.metrics.cell.height * 0.25).clamp(thickness, self.metrics.cell.height * 0.5);
        let horizontal_width = (width - radius_x * 2.0).max(thickness);
        let vertical_height = (self.metrics.cell.height - radius_y * 2.0).max(thickness);

        planned.text_decorations.push(RectBatchItem {
            origin: PixelPoint {
                x: origin.x + radius_x,
                y: origin.y,
            },
            size: PixelSize {
                width: horizontal_width,
                height: thickness,
            },
            color,
        });
        planned.text_decorations.push(RectBatchItem {
            origin: PixelPoint {
                x: origin.x + radius_x,
                y: origin.y + self.metrics.cell.height - thickness,
            },
            size: PixelSize {
                width: horizontal_width,
                height: thickness,
            },
            color,
        });
        planned.text_decorations.push(RectBatchItem {
            origin: PixelPoint {
                x: origin.x,
                y: origin.y + radius_y,
            },
            size: PixelSize {
                width: thickness,
                height: vertical_height,
            },
            color,
        });
        planned.text_decorations.push(RectBatchItem {
            origin: PixelPoint {
                x: origin.x + width - thickness,
                y: origin.y + radius_y,
            },
            size: PixelSize {
                width: thickness,
                height: vertical_height,
            },
            color,
        });
    }

    fn push_cellwise_underline_segments(
        &self,
        planned: &mut PlannedRow,
        origin: PixelPoint,
        cols: u16,
        segment_width: f32,
        color: Rgba,
    ) {
        let thickness = self.text_decoration_thickness();
        let y = origin.y + self.metrics.cell.height - thickness;
        for col in 0..cols {
            let cell_x = origin.x + f32::from(col) * self.metrics.cell.width;
            planned.text_decorations.push(RectBatchItem {
                origin: PixelPoint {
                    x: cell_x + (self.metrics.cell.width - segment_width) * 0.5,
                    y,
                },
                size: PixelSize {
                    width: segment_width,
                    height: thickness,
                },
                color,
            });
        }
    }

    fn push_curly_underline_segments(
        &self,
        planned: &mut PlannedRow,
        origin: PixelPoint,
        cols: u16,
        color: Rgba,
    ) {
        let thickness = self.text_decoration_thickness();
        let width = self.metrics.cell.width * f32::from(cols);
        let step = (self.metrics.cell.width * 0.5).max(thickness);
        let high_y = origin.y + self.metrics.cell.height - thickness * 3.0;
        let low_y = origin.y + self.metrics.cell.height - thickness;
        let segments = (width / step).ceil().max(1.0) as u16;

        for segment in 0..segments {
            let x = origin.x + f32::from(segment) * step;
            let segment_width = (origin.x + width - x).min(step).max(0.0);
            if segment_width <= 0.0 {
                continue;
            }

            let y = if segment % 2 == 0 { high_y } else { low_y };
            planned.text_decorations.push(RectBatchItem {
                origin: PixelPoint { x, y },
                size: PixelSize {
                    width: segment_width,
                    height: thickness,
                },
                color,
            });

            if segment > 0 {
                planned.text_decorations.push(RectBatchItem {
                    origin: PixelPoint {
                        x: x - thickness * 0.5,
                        y: high_y,
                    },
                    size: PixelSize {
                        width: thickness,
                        height: low_y - high_y + thickness,
                    },
                    color,
                });
            }
        }
    }

    fn text_decoration_thickness(&self) -> f32 {
        (self.metrics.cell.height * 0.08)
            .round()
            .clamp(1.0, self.metrics.cell.height)
    }

    fn selection_rects(&self, range: CellRange, cols: u16) -> Vec<RectBatchItem> {
        self.range_rects(range, cols, Rgba::rgb(75, 110, 175))
    }

    fn search_highlight_rects(&self, highlight: SearchHighlight, cols: u16) -> Vec<RectBatchItem> {
        let kind = if highlight.active {
            SearchHighlightKind::Active
        } else {
            SearchHighlightKind::Regular
        };
        self.range_rects(highlight.range, cols, search_highlight_color(kind))
    }

    fn hyperlink_overlay_rects(
        &self,
        snapshot: &RenderSnapshot,
    ) -> (Vec<RectBatchItem>, Vec<RectBatchItem>) {
        let mut underlines = Vec::new();
        let mut hover = Vec::new();
        for row in &snapshot.rows {
            let mut current = None;
            for cell in &row.cells {
                if let Some(id) = cell.hyperlink {
                    let old = self.extend_hyperlink_run(&mut current, id, cell);
                    self.flush_hyperlink_run(
                        &mut underlines,
                        &mut hover,
                        old,
                        snapshot.hovered_hyperlink,
                    );
                } else {
                    let old = current.take();
                    self.flush_hyperlink_run(
                        &mut underlines,
                        &mut hover,
                        old,
                        snapshot.hovered_hyperlink,
                    );
                }
            }

            self.flush_hyperlink_run(
                &mut underlines,
                &mut hover,
                current,
                snapshot.hovered_hyperlink,
            );
        }
        (underlines, hover)
    }

    fn extend_hyperlink_run(
        &self,
        current: &mut Option<HyperlinkRunBuilder>,
        id: HyperlinkId,
        cell: &RenderCell,
    ) -> Option<HyperlinkRunBuilder> {
        let cell_width = u16::from(cell.width.max(1));
        match current {
            Some(run)
                if run.id == id && run.row == cell.point.row && run.next_col == cell.point.col =>
            {
                run.cols = run.cols.saturating_add(cell_width);
                run.next_col = run.next_col.saturating_add(cell_width);
                None
            }
            _ => {
                let old = current.take();
                *current = Some(HyperlinkRunBuilder {
                    id,
                    row: cell.point.row,
                    start_col: cell.point.col,
                    next_col: cell.point.col.saturating_add(cell_width),
                    cols: cell_width,
                });
                old
            }
        }
    }

    fn flush_hyperlink_run(
        &self,
        underlines: &mut Vec<RectBatchItem>,
        hover: &mut Vec<RectBatchItem>,
        run: Option<HyperlinkRunBuilder>,
        hovered: Option<HyperlinkId>,
    ) {
        let Some(run) = run else {
            return;
        };
        let origin = self.cell_origin(CellPoint::new(run.row, run.start_col));
        let width = self.metrics.cell.width * f32::from(run.cols);

        if hovered == Some(run.id) {
            hover.push(RectBatchItem {
                origin,
                size: PixelSize {
                    width,
                    height: self.metrics.cell.height,
                },
                color: hyperlink_hover_color(),
            });
        }

        let height = self.hyperlink_underline_height();
        underlines.push(RectBatchItem {
            origin: PixelPoint {
                x: origin.x,
                y: origin.y + self.metrics.cell.height - height,
            },
            size: PixelSize { width, height },
            color: hyperlink_underline_color(hovered == Some(run.id)),
        });
    }

    fn hyperlink_underline_height(&self) -> f32 {
        (self.metrics.cell.height * 0.08).clamp(1.0, self.metrics.cell.height)
    }

    fn range_rects(&self, range: CellRange, cols: u16, color: Rgba) -> Vec<RectBatchItem> {
        if cols == 0 {
            return Vec::new();
        }

        let mut rects = Vec::new();
        for row in range.start.row..=range.end.row {
            let start_col = if row == range.start.row {
                range.start.col
            } else {
                0
            };
            let end_col = if row == range.end.row {
                range.end.col
            } else {
                cols.saturating_sub(1)
            };
            if end_col < start_col {
                continue;
            }
            rects.push(RectBatchItem {
                origin: self.cell_origin(CellPoint::new(row, start_col)),
                size: PixelSize {
                    width: self.metrics.cell.width * f32::from(end_col - start_col + 1),
                    height: self.metrics.cell.height,
                },
                color,
            });
        }
        rects
    }

    fn cell_origin(&self, point: CellPoint) -> PixelPoint {
        PixelPoint {
            x: self.metrics.padding.x + f32::from(point.col) * self.metrics.cell.width,
            y: self.metrics.padding.y + f32::from(point.row) * self.metrics.cell.height,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct PlannedRow {
    backgrounds: Vec<RectBatchItem>,
    glyphs: Vec<GlyphBatchItem>,
    text_decorations: Vec<RectBatchItem>,
}

impl PlannedRow {
    fn extend_frame(&self, frame: &mut FramePlan) {
        frame.backgrounds.extend(self.backgrounds.clone());
        frame.glyphs.extend(self.glyphs.clone());
        frame.text_decorations.extend(self.text_decorations.clone());
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RetainedFramePlanner {
    planner: FramePlanner,
    cached_size: Option<GridSize>,
    cached_rows: Vec<Option<PlannedRow>>,
}

impl RetainedFramePlanner {
    pub fn new(metrics: CellMetrics) -> Self {
        Self {
            planner: FramePlanner::new(metrics),
            cached_size: None,
            cached_rows: Vec::new(),
        }
    }

    pub fn set_blink_visible(&mut self, visible: bool) {
        if self.planner.blink_visible == visible {
            return;
        }

        self.planner.blink_visible = visible;
        self.cached_size = None;
        self.cached_rows.clear();
    }

    pub fn plan(&mut self, snapshot: &RenderSnapshot) -> FramePlan {
        if self.requires_full_rebuild(snapshot) {
            return self.rebuild_all(snapshot);
        }

        let DamageRegion::Rows(rows) = &snapshot.damage else {
            return self.rebuild_all(snapshot);
        };

        let mut rebuilt_rows = 0;
        for row in rows {
            let Some(index) = snapshot
                .rows
                .iter()
                .position(|snapshot_row| snapshot_row.row == *row)
            else {
                return self.rebuild_all(snapshot);
            };
            self.cached_rows[index] = Some(self.planner.plan_row(&snapshot.rows[index]));
            rebuilt_rows += 1;
        }

        self.compose_cached_frame(
            snapshot,
            snapshot.rows.len().saturating_sub(rebuilt_rows),
            rebuilt_rows,
        )
    }

    fn requires_full_rebuild(&self, snapshot: &RenderSnapshot) -> bool {
        snapshot.damage.is_full()
            || self.cached_size != Some(snapshot.size)
            || self.cached_rows.len() != snapshot.rows.len()
            || self.cached_rows.iter().any(Option::is_none)
    }

    fn rebuild_all(&mut self, snapshot: &RenderSnapshot) -> FramePlan {
        self.cached_size = Some(snapshot.size);
        self.cached_rows = snapshot
            .rows
            .iter()
            .map(|row| Some(self.planner.plan_row(row)))
            .collect();
        self.compose_cached_frame(snapshot, 0, snapshot.rows.len())
    }

    fn compose_cached_frame(
        &self,
        snapshot: &RenderSnapshot,
        reused_rows: usize,
        rebuilt_rows: usize,
    ) -> FramePlan {
        let mut frame = FramePlan::default();
        for row in self.cached_rows.iter().flatten() {
            row.extend_frame(&mut frame);
        }

        self.planner.apply_dynamic_overlays(&mut frame, snapshot);
        frame.damage = snapshot.damage.clone();
        frame.refresh_stats_with_rows(
            snapshot.rows.len() as u16,
            snapshot.size.cols,
            reused_rows,
            rebuilt_rows,
        );
        frame
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BackgroundRunBuilder {
    row: u16,
    start_col: u16,
    next_col: u16,
    cols: u16,
    color: Rgba,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GlyphRunBuilder {
    row: u16,
    start_col: u16,
    next_col: u16,
    text: String,
    char_count: usize,
    foreground: Rgba,
    flags: CellFlags,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HyperlinkRunBuilder {
    id: HyperlinkId,
    row: u16,
    start_col: u16,
    next_col: u16,
    cols: u16,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct TextDecorationRuns {
    underline: Option<TextDecorationRunBuilder>,
    strike: Option<TextDecorationRunBuilder>,
    frame: Option<TextDecorationRunBuilder>,
    encircle: Option<TextDecorationRunBuilder>,
    overline: Option<TextDecorationRunBuilder>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TextDecorationRunBuilder {
    kind: TextDecorationKind,
    row: u16,
    start_col: u16,
    next_col: u16,
    cols: u16,
    color: Rgba,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TextDecorationKind {
    SingleUnderline,
    DoubleUnderline,
    DottedUnderline,
    DashedUnderline,
    CurlyUnderline,
    Strikethrough,
    Frame,
    Encircle,
    Overline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SearchHighlightKind {
    Regular,
    Active,
}

fn underline_decoration_kind(style: UnderlineStyle) -> Option<TextDecorationKind> {
    match style {
        UnderlineStyle::Single => Some(TextDecorationKind::SingleUnderline),
        UnderlineStyle::Double => Some(TextDecorationKind::DoubleUnderline),
        UnderlineStyle::Dotted => Some(TextDecorationKind::DottedUnderline),
        UnderlineStyle::Dashed => Some(TextDecorationKind::DashedUnderline),
        UnderlineStyle::Curly => Some(TextDecorationKind::CurlyUnderline),
    }
}

fn effective_foreground(cell: &RenderCell) -> Rgba {
    if cell.style.flags.faint {
        dim_color(cell.style.foreground, cell.style.background)
    } else {
        cell.style.foreground
    }
}

fn dim_color(foreground: Rgba, background: Rgba) -> Rgba {
    Rgba::with_alpha(
        mix_channel(foreground.r, background.r),
        mix_channel(foreground.g, background.g),
        mix_channel(foreground.b, background.b),
        foreground.a,
    )
}

fn mix_channel(foreground: u8, background: u8) -> u8 {
    ((u16::from(foreground) + u16::from(background)) / 2) as u8
}

fn search_highlight_color(kind: SearchHighlightKind) -> Rgba {
    match kind {
        SearchHighlightKind::Regular => Rgba::with_alpha(168, 130, 48, 110),
        SearchHighlightKind::Active => Rgba::with_alpha(247, 202, 87, 190),
    }
}

fn hyperlink_hover_color() -> Rgba {
    Rgba::with_alpha(76, 148, 216, 48)
}

fn hyperlink_underline_color(hovered: bool) -> Rgba {
    if hovered {
        Rgba::with_alpha(96, 188, 255, 230)
    } else {
        Rgba::with_alpha(96, 188, 255, 170)
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
}

impl Vertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

pub struct WgpuRectRenderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    font_system: FontSystem,
    font_config: RendererFontConfig,
    swash_cache: SwashCache,
    text_atlas: TextAtlas,
    text_renderers: Vec<TextRenderer>,
    viewport: Viewport,
    text_buffer_cache: TextBufferCache,
    last_text_buffer_sync: TextBufferSyncStats,
    last_timing_stats: RendererTimingStats,
    rect_vertex_buffer: Option<wgpu::Buffer>,
    rect_vertex_capacity: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RendererFontConfig {
    family: Option<String>,
    font_size: u16,
    font_scale_factor: f32,
    terminal_padding: f32,
}

impl RendererFontConfig {
    pub fn new(family: Option<String>) -> Self {
        Self::with_font_size(family, DEFAULT_TERMINAL_FONT_SIZE)
    }

    pub fn with_font_size(family: Option<String>, font_size: u16) -> Self {
        Self {
            family: normalize_font_family(family),
            font_size,
            font_scale_factor: 1.0,
            terminal_padding: DEFAULT_TERMINAL_PADDING.x,
        }
    }

    pub fn with_scale_factor(mut self, scale_factor: f32) -> Self {
        self.font_scale_factor = sane_font_scale_factor(scale_factor);
        self
    }

    pub fn with_terminal_padding(mut self, padding: f32) -> Self {
        self.terminal_padding = sane_terminal_padding(padding);
        self
    }

    pub fn family(&self) -> Option<&str> {
        self.family.as_deref()
    }

    pub fn font_size(&self) -> u16 {
        self.font_size
    }

    pub fn font_scale_factor(&self) -> f32 {
        self.font_scale_factor
    }

    pub fn terminal_padding(&self) -> f32 {
        self.terminal_padding
    }

    pub fn effective_font_size(&self) -> f32 {
        f32::from(self.font_size) * self.font_scale_factor
    }

    pub fn line_height(&self) -> f32 {
        DEFAULT_TERMINAL_LINE_HEIGHT
            * (self.effective_font_size() / f32::from(DEFAULT_TERMINAL_FONT_SIZE))
    }

    pub fn cell_metrics(&self) -> CellMetrics {
        let mut metrics = CellMetrics::for_font_size(self.font_size);
        metrics.padding = PixelPoint {
            x: self.terminal_padding,
            y: self.terminal_padding,
        };
        metrics.scale(self.font_scale_factor)
    }
}

impl Default for RendererFontConfig {
    fn default() -> Self {
        Self::new(None)
    }
}

pub fn available_font_families() -> Vec<String> {
    font_family_names_from_system(FontSystem::new())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeWgpuBackendPolicy {
    backends: wgpu::Backends,
    label: &'static str,
    honors_wgpu_backend_env: bool,
}

impl NativeWgpuBackendPolicy {
    pub fn backends(self) -> wgpu::Backends {
        self.backends
    }

    pub fn label(self) -> &'static str {
        self.label
    }

    pub fn honors_wgpu_backend_env(self) -> bool {
        self.honors_wgpu_backend_env
    }

    pub fn is_opengl_only(self) -> bool {
        self.backends == wgpu::Backends::GL
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RendererCacheStats {
    pub text_buffers_reused: usize,
    pub text_buffers_rebuilt: usize,
    pub text_buffers_retired: usize,
    pub text_buffer_count: usize,
    pub text_renderer_count: usize,
    pub rect_vertex_capacity: usize,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RendererTimingStats {
    pub cpu_prepare_us: u64,
    pub text_buffer_sync_us: u64,
    pub glyph_prepare_us: u64,
    pub rect_vertex_sync_us: u64,
}

impl WgpuRectRenderer {
    pub async fn new(
        surface_target: impl Into<wgpu::SurfaceTarget<'static>>
            + wgpu::rwh::HasDisplayHandle
            + Debug
            + Send
            + Sync
            + Clone
            + 'static,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        Self::new_with_font_config(surface_target, width, height, RendererFontConfig::default())
            .await
    }

    pub async fn new_with_font_config(
        surface_target: impl Into<wgpu::SurfaceTarget<'static>>
            + wgpu::rwh::HasDisplayHandle
            + Debug
            + Send
            + Sync
            + Clone
            + 'static,
        width: u32,
        height: u32,
        font_config: RendererFontConfig,
    ) -> Result<Self> {
        Self::new_with_font_config_and_data(surface_target, width, height, font_config, Vec::new())
            .await
    }

    pub async fn new_with_font_config_and_data(
        surface_target: impl Into<wgpu::SurfaceTarget<'static>>
            + wgpu::rwh::HasDisplayHandle
            + Debug
            + Send
            + Sync
            + Clone
            + 'static,
        width: u32,
        height: u32,
        font_config: RendererFontConfig,
        font_data: Vec<Vec<u8>>,
    ) -> Result<Self> {
        let mut instance_descriptor =
            wgpu::InstanceDescriptor::new_with_display_handle(Box::new(surface_target.clone()));
        instance_descriptor.backends = native_wgpu_backends();
        Self::from_surface_target_with_fonts(
            instance_descriptor,
            surface_target,
            width,
            height,
            font_sources_from_data(font_data),
            font_config,
        )
        .await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn new_for_canvas(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
    ) -> Result<Self> {
        Self::from_surface_target(
            wasm_instance_descriptor(),
            wgpu::SurfaceTarget::Canvas(canvas),
            width,
            height,
        )
        .await
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn new_for_canvas_with_font_data(
        canvas: web_sys::HtmlCanvasElement,
        width: u32,
        height: u32,
        font_data: Vec<u8>,
    ) -> Result<Self> {
        Self::from_surface_target_with_fonts(
            wasm_instance_descriptor(),
            wgpu::SurfaceTarget::Canvas(canvas),
            width,
            height,
            font_sources_from_data(vec![font_data]),
            RendererFontConfig::default(),
        )
        .await
    }

    async fn from_surface_target_with_fonts(
        instance_descriptor: wgpu::InstanceDescriptor,
        surface_target: impl Into<wgpu::SurfaceTarget<'static>>,
        width: u32,
        height: u32,
        font_sources: Vec<fontdb::Source>,
        font_config: RendererFontConfig,
    ) -> Result<Self> {
        let instance = wgpu::Instance::new(instance_descriptor);
        let surface = instance
            .create_surface(surface_target)
            .context("failed to create wgpu surface")?;

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("failed to find a usable wgpu adapter")?;

        let mut limits =
            wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits());
        limits.max_mesh_output_layers = 0;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_limits: limits,
                ..Default::default()
            })
            .await
            .context("failed to create wgpu device")?;

        let config = surface
            .get_default_config(&adapter, width.max(1), height.max(1))
            .context("failed to create default surface config")?;
        surface.configure(&device, &config);

        let pipeline = create_pipeline(&device, config.format);
        let text_cache = Cache::new(&device);
        let viewport = Viewport::new(&device, &text_cache);
        let mut text_atlas = TextAtlas::new(&device, &queue, &text_cache, config.format);
        let text_renderer = TextRenderer::new(
            &mut text_atlas,
            &device,
            wgpu::MultisampleState::default(),
            None,
        );

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            font_system: font_system_with_sources(font_sources),
            font_config,
            swash_cache: SwashCache::new(),
            text_atlas,
            text_renderers: vec![text_renderer],
            viewport,
            text_buffer_cache: TextBufferCache::default(),
            last_text_buffer_sync: TextBufferSyncStats::default(),
            last_timing_stats: RendererTimingStats::default(),
            rect_vertex_buffer: None,
            rect_vertex_capacity: 0,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn cache_stats(&self) -> RendererCacheStats {
        renderer_cache_stats(
            self.last_text_buffer_sync,
            self.text_buffer_cache.items.len(),
            self.text_renderers.len(),
            self.rect_vertex_capacity,
        )
    }

    pub fn timing_stats(&self) -> RendererTimingStats {
        self.last_timing_stats
    }

    pub fn set_font_config(&mut self, font_config: RendererFontConfig) {
        if self.font_config == font_config {
            return;
        }

        self.font_config = font_config;
        self.text_buffer_cache.clear();
    }

    pub fn render(&mut self, frame: &FramePlan) -> Result<()> {
        self.viewport.update(
            &self.queue,
            Resolution {
                width: self.config.width,
                height: self.config.height,
            },
        );
        let text_buffer_sync_timer = RenderTimer::start();
        self.last_text_buffer_sync = self.text_buffer_cache.sync(
            &mut self.font_system,
            &self.font_config,
            self.config.width as f32,
            frame.stats.visible_cols,
            &frame.glyphs,
        );
        let text_buffer_sync_us = text_buffer_sync_timer.elapsed_us();
        let text_item_chunks = text_buffer_item_chunks(&self.text_buffer_cache.items);
        self.ensure_text_renderer_count(text_item_chunks.len());
        let glyph_prepare_timer = RenderTimer::start();
        for (renderer, range) in self.text_renderers.iter_mut().zip(text_item_chunks.iter()) {
            renderer
                .prepare(
                    &self.device,
                    &self.queue,
                    &mut self.font_system,
                    &mut self.text_atlas,
                    &self.viewport,
                    self.text_buffer_cache.items[range.clone()]
                        .iter()
                        .map(|item| {
                            text_area_for_item(item, self.config.width, self.config.height)
                        }),
                    &mut self.swash_cache,
                )
                .context("prepare glyphon text")?;
        }
        let glyph_prepare_us = glyph_prepare_timer.elapsed_us();

        let surface_texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                anyhow::bail!("failed to acquire surface texture: validation error");
            }
        };
        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let rect_vertex_sync_timer = RenderTimer::start();
        let vertices = self.vertices_for_frame(frame);
        self.sync_rect_vertex_buffer(&vertices);
        let rect_vertex_sync_us = rect_vertex_sync_timer.elapsed_us();
        self.last_timing_stats =
            renderer_timing_stats(text_buffer_sync_us, glyph_prepare_us, rect_vertex_sync_us);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Witty static renderer encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Witty static render pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu_clear_color(DEFAULT_SURFACE_CLEAR_COLOR)),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });

            if !vertices.is_empty() {
                let vertex_buffer = self
                    .rect_vertex_buffer
                    .as_ref()
                    .expect("non-empty rect vertices should have a synced vertex buffer");
                pass.set_pipeline(&self.pipeline);
                pass.set_vertex_buffer(0, vertex_buffer.slice(..));
                pass.draw(0..vertices.len() as u32, 0..1);
            }
            for renderer in self.text_renderers.iter().take(text_item_chunks.len()) {
                renderer
                    .render(&self.text_atlas, &self.viewport, &mut pass)
                    .context("render glyphon text")?;
            }
        }

        self.queue.submit(Some(encoder.finish()));
        surface_texture.present();
        self.text_atlas.trim();
        Ok(())
    }

    fn ensure_text_renderer_count(&mut self, count: usize) {
        while self.text_renderers.len() < count.max(1) {
            self.text_renderers.push(TextRenderer::new(
                &mut self.text_atlas,
                &self.device,
                wgpu::MultisampleState::default(),
                None,
            ));
        }
    }

    fn vertices_for_frame(&self, frame: &FramePlan) -> Vec<Vertex> {
        let mut vertices = Vec::new();
        for rect in frame
            .backgrounds
            .iter()
            .chain(frame.search_highlights.iter())
            .chain(frame.selection.iter())
            .chain(frame.hyperlink_hover.iter())
            .chain(frame.hyperlink_underlines.iter())
            .chain(frame.text_decorations.iter())
            .chain(frame.ime_preedit.iter())
            .chain(frame.cursor.iter())
        {
            push_rect_vertices(
                &mut vertices,
                rect.origin.x,
                rect.origin.y,
                rect.size.width,
                rect.size.height,
                rect.color,
                self.config.width as f32,
                self.config.height as f32,
            );
        }
        vertices
    }

    fn sync_rect_vertex_buffer(&mut self, vertices: &[Vertex]) {
        if vertices.is_empty() {
            return;
        }

        if self.rect_vertex_capacity < vertices.len() || self.rect_vertex_buffer.is_none() {
            self.rect_vertex_capacity = rect_vertex_capacity_for_len(vertices.len());
            self.rect_vertex_buffer = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Witty static rect vertex cache"),
                size: (self.rect_vertex_capacity * std::mem::size_of::<Vertex>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }

        if let Some(buffer) = &self.rect_vertex_buffer {
            self.queue
                .write_buffer(buffer, 0, bytemuck::cast_slice(vertices));
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn wasm_instance_descriptor() -> wgpu::InstanceDescriptor {
    let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    instance_descriptor.backends = (wgpu::Backends::PRIMARY | wgpu::Backends::SECONDARY).with_env();
    instance_descriptor
}

#[cfg(target_os = "linux")]
pub fn native_wgpu_backend_policy() -> NativeWgpuBackendPolicy {
    NativeWgpuBackendPolicy {
        backends: wgpu::Backends::GL,
        label: "gl",
        honors_wgpu_backend_env: false,
    }
}

#[cfg(not(target_os = "linux"))]
pub fn native_wgpu_backend_policy() -> NativeWgpuBackendPolicy {
    NativeWgpuBackendPolicy {
        backends: (wgpu::Backends::PRIMARY | wgpu::Backends::SECONDARY).with_env(),
        label: "primary-secondary-with-env",
        honors_wgpu_backend_env: true,
    }
}

fn native_wgpu_backends() -> wgpu::Backends {
    native_wgpu_backend_policy().backends()
}

fn font_system_with_sources(font_sources: Vec<fontdb::Source>) -> FontSystem {
    if font_sources.is_empty() {
        FontSystem::new()
    } else {
        FontSystem::new_with_fonts(font_sources)
    }
}

fn font_family_names_from_system(font_system: FontSystem) -> Vec<String> {
    sorted_unique_font_family_names(
        font_system
            .db()
            .faces()
            .flat_map(|face| face.families.iter().map(|(family, _)| family.as_str())),
    )
}

fn sorted_unique_font_family_names<'a>(families: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    families
        .into_iter()
        .map(str::trim)
        .filter(|family| !family.is_empty())
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn font_sources_from_data(font_data: Vec<Vec<u8>>) -> Vec<fontdb::Source> {
    font_data
        .into_iter()
        .map(|data| fontdb::Source::Binary(Arc::new(data)))
        .collect()
}

fn normalize_font_family(family: Option<String>) -> Option<String> {
    family
        .map(|family| family.trim().to_owned())
        .filter(|family| !family.is_empty())
}

fn sane_font_scale_factor(scale_factor: f32) -> f32 {
    if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    }
}

fn sane_terminal_padding(padding: f32) -> f32 {
    if padding.is_finite() && padding >= 0.0 {
        padding
    } else {
        DEFAULT_TERMINAL_PADDING.x
    }
}

#[derive(Default)]
struct TextBufferCache {
    items: Vec<TextBufferItem>,
}

impl TextBufferCache {
    fn clear(&mut self) {
        self.items.clear();
    }

    fn sync(
        &mut self,
        font_system: &mut FontSystem,
        font_config: &RendererFontConfig,
        surface_width: f32,
        visible_cols: u16,
        glyphs: &[GlyphBatchItem],
    ) -> TextBufferSyncStats {
        let previous_len = self.items.len();
        let mut stats = TextBufferSyncStats::default();

        for (index, glyph) in glyphs.iter().enumerate() {
            let key = TextBufferKey::new(surface_width, visible_cols, glyph, font_config);
            match self.items.get_mut(index) {
                Some(item) if item.key == key => {
                    item.left = glyph.origin.x;
                    item.top = glyph.origin.y;
                    item.color = glyph.color;
                    stats.reused += 1;
                }
                Some(item) => {
                    *item = TextBufferItem::new(font_system, key, glyph);
                    stats.rebuilt += 1;
                }
                None => {
                    self.items
                        .push(TextBufferItem::new(font_system, key, glyph));
                    stats.rebuilt += 1;
                }
            }
        }

        stats.retired = previous_len.saturating_sub(glyphs.len());
        self.items.truncate(glyphs.len());
        stats
    }
}

#[derive(Clone, Debug, PartialEq)]
struct TextBufferKey {
    text: String,
    width_px: u32,
    style_flags: CellFlags,
    font_family: Option<String>,
    font_size: u16,
    font_scale_factor: f32,
}

impl TextBufferKey {
    fn new(
        surface_width: f32,
        visible_cols: u16,
        glyph: &GlyphBatchItem,
        font_config: &RendererFontConfig,
    ) -> Self {
        Self {
            text: glyph.text.clone(),
            width_px: text_buffer_width_px(surface_width, visible_cols, glyph, font_config),
            style_flags: glyph.style_flags,
            font_family: font_config.family.clone(),
            font_size: font_config.font_size,
            font_scale_factor: font_config.font_scale_factor,
        }
    }
}

struct TextBufferItem {
    key: TextBufferKey,
    buffer: Buffer,
    left: f32,
    top: f32,
    color: Rgba,
}

impl TextBufferItem {
    fn new(font_system: &mut FontSystem, key: TextBufferKey, glyph: &GlyphBatchItem) -> Self {
        let metrics =
            text_metrics_for_style_flags(glyph.style_flags, key.font_size, key.font_scale_factor);
        let mut buffer = Buffer::new(font_system, metrics);
        buffer.set_size(
            font_system,
            Some(key.width_px as f32),
            Some(metrics.line_height * 1.5),
        );
        let attrs = text_attrs_for_style_flags(glyph.style_flags, key.font_family.as_deref());
        buffer.set_text(font_system, &key.text, &attrs, Shaping::Basic, None);
        buffer.shape_until_scroll(font_system, false);

        Self {
            key,
            buffer,
            left: glyph.origin.x,
            top: glyph.origin.y,
            color: glyph.color,
        }
    }
}

fn text_attrs_for_style_flags<'a>(flags: CellFlags, font_family: Option<&'a str>) -> Attrs<'a> {
    let mut attrs = Attrs::new().family(match font_family {
        Some(font_family) => Family::Name(font_family),
        None => Family::Monospace,
    });
    if flags.bold {
        attrs = attrs.weight(Weight::BOLD);
    }
    if flags.italic {
        attrs = attrs.style(Style::Italic);
    }
    attrs
}

fn text_metrics_for_style_flags(
    flags: CellFlags,
    font_size: u16,
    font_scale_factor: f32,
) -> Metrics {
    let font_size = f32::from(font_size) * sane_font_scale_factor(font_scale_factor);
    let line_height =
        DEFAULT_TERMINAL_LINE_HEIGHT * (font_size / f32::from(DEFAULT_TERMINAL_FONT_SIZE));
    let metrics = Metrics::new(font_size, line_height);
    match flags.baseline_shift {
        BaselineShift::Normal => metrics,
        BaselineShift::Superscript | BaselineShift::Subscript => {
            metrics.scale(BASELINE_SHIFT_FONT_SCALE)
        }
    }
}

fn text_buffer_item_chunks(items: &[TextBufferItem]) -> Vec<Range<usize>> {
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut chars = 0usize;

    for (index, item) in items.iter().enumerate() {
        let item_chars = item.key.text.chars().count().max(1);
        if index > start && chars.saturating_add(item_chars) > MAX_GLYPHON_RENDERER_CHARS {
            chunks.push(start..index);
            start = index;
            chars = 0;
        }
        chars = chars.saturating_add(item_chars);
    }

    if start < items.len() {
        chunks.push(start..items.len());
    }

    chunks
}

fn text_area_for_item(
    item: &TextBufferItem,
    surface_width: u32,
    surface_height: u32,
) -> TextArea<'_> {
    TextArea {
        buffer: &item.buffer,
        left: item.left,
        top: item.top,
        scale: 1.0,
        bounds: TextBounds {
            left: 0,
            top: 0,
            right: surface_width as i32,
            bottom: surface_height as i32,
        },
        default_color: glyphon_color(item.color),
        custom_glyphs: &[],
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct TextBufferSyncStats {
    reused: usize,
    rebuilt: usize,
    retired: usize,
}

fn renderer_cache_stats(
    text_buffer_sync: TextBufferSyncStats,
    text_buffer_count: usize,
    text_renderer_count: usize,
    rect_vertex_capacity: usize,
) -> RendererCacheStats {
    RendererCacheStats {
        text_buffers_reused: text_buffer_sync.reused,
        text_buffers_rebuilt: text_buffer_sync.rebuilt,
        text_buffers_retired: text_buffer_sync.retired,
        text_buffer_count,
        text_renderer_count,
        rect_vertex_capacity,
    }
}

fn renderer_timing_stats(
    text_buffer_sync_us: u64,
    glyph_prepare_us: u64,
    rect_vertex_sync_us: u64,
) -> RendererTimingStats {
    RendererTimingStats {
        cpu_prepare_us: text_buffer_sync_us
            .saturating_add(glyph_prepare_us)
            .saturating_add(rect_vertex_sync_us),
        text_buffer_sync_us,
        glyph_prepare_us,
        rect_vertex_sync_us,
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct RenderTimer(std::time::Instant);

#[cfg(not(target_arch = "wasm32"))]
impl RenderTimer {
    fn start() -> Self {
        Self(std::time::Instant::now())
    }

    fn elapsed_us(&self) -> u64 {
        self.0.elapsed().as_micros().try_into().unwrap_or(u64::MAX)
    }
}

#[cfg(target_arch = "wasm32")]
struct RenderTimer(f64);

#[cfg(target_arch = "wasm32")]
impl RenderTimer {
    fn start() -> Self {
        Self(browser_now_ms())
    }

    fn elapsed_us(&self) -> u64 {
        ((browser_now_ms() - self.0).max(0.0) * 1000.0) as u64
    }
}

#[cfg(target_arch = "wasm32")]
fn browser_now_ms() -> f64 {
    web_sys::window()
        .and_then(|window| window.performance())
        .map(|performance| performance.now())
        .unwrap_or(0.0)
}

fn text_buffer_width_px(
    surface_width: f32,
    visible_cols: u16,
    glyph: &GlyphBatchItem,
    font_config: &RendererFontConfig,
) -> u32 {
    let line_height = font_config.line_height();
    let available_width = (surface_width - glyph.origin.x).max(line_height);
    let estimated_cell_width = if visible_cols == 0 {
        font_config.effective_font_size()
    } else {
        (surface_width / f32::from(visible_cols)).max(1.0)
    };
    let text_cols = UnicodeWidthStr::width(glyph.text.as_str()).max(1) as f32;
    let desired_width = (text_cols + 1.0) * estimated_cell_width;

    desired_width.clamp(line_height, available_width).ceil() as u32
}

fn create_pipeline(device: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Witty static rect shader"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("rect_static.wgsl"))),
    });

    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Witty static rect pipeline layout"),
        bind_group_layouts: &[],
        immediate_size: 0,
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Witty static rect pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[Vertex::desc()],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::all(),
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

#[allow(clippy::too_many_arguments)]
fn push_rect_vertices(
    vertices: &mut Vec<Vertex>,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: Rgba,
    surface_width: f32,
    surface_height: f32,
) {
    let left = pixel_x_to_ndc(x, surface_width);
    let right = pixel_x_to_ndc(x + width, surface_width);
    let top = pixel_y_to_ndc(y, surface_height);
    let bottom = pixel_y_to_ndc(y + height, surface_height);
    let color = rgba_to_linear(color);

    vertices.extend_from_slice(&[
        Vertex {
            position: [left, top],
            color,
        },
        Vertex {
            position: [left, bottom],
            color,
        },
        Vertex {
            position: [right, bottom],
            color,
        },
        Vertex {
            position: [left, top],
            color,
        },
        Vertex {
            position: [right, bottom],
            color,
        },
        Vertex {
            position: [right, top],
            color,
        },
    ]);
}

const RECT_VERTICES: usize = 6;

fn rect_vertex_capacity_for_len(required: usize) -> usize {
    required.max(RECT_VERTICES).next_power_of_two()
}

fn pixel_x_to_ndc(x: f32, surface_width: f32) -> f32 {
    (x / surface_width) * 2.0 - 1.0
}

fn pixel_y_to_ndc(y: f32, surface_height: f32) -> f32 {
    1.0 - (y / surface_height) * 2.0
}

fn rgba_to_linear(color: Rgba) -> [f32; 4] {
    [
        f32::from(color.r) / 255.0,
        f32::from(color.g) / 255.0,
        f32::from(color.b) / 255.0,
        f32::from(color.a) / 255.0,
    ]
}

fn wgpu_clear_color(color: Rgba) -> wgpu::Color {
    let [r, g, b, a] = rgba_to_linear(color);
    wgpu::Color {
        r: f64::from(r),
        g: f64::from(g),
        b: f64::from(b),
        a: f64::from(a),
    }
}

fn glyphon_color(color: Rgba) -> Color {
    Color::rgb(color.r, color.g, color.b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use witty_core::{
        BasicTerminal, CellPoint, CellRange, CellStyle, CursorShape, GridSize, RenderSnapshot,
        SearchHighlight,
    };

    #[cfg(target_os = "linux")]
    #[test]
    fn native_renderer_defaults_to_opengl_backend_only() {
        let policy = native_wgpu_backend_policy();

        assert_eq!(native_wgpu_backends(), wgpu::Backends::GL);
        assert_eq!(policy.label(), "gl");
        assert!(policy.is_opengl_only());
        assert!(!policy.honors_wgpu_backend_env());
    }

    #[test]
    fn frame_plan_has_backgrounds_and_glyphs() {
        let snapshot = RenderSnapshot::from_plain_lines(&["hi"]);
        let planner = FramePlanner::new(CellMetrics::default());
        let frame = planner.plan(&snapshot);

        assert_eq!(frame.backgrounds.len(), 1);
        assert_eq!(frame.glyphs.len(), 1);
        assert_eq!(frame.glyphs[0].text, "hi");
        assert!(frame.cursor.is_some());
    }

    #[test]
    fn surface_clear_color_matches_default_terminal_background() {
        let clear = wgpu_clear_color(DEFAULT_SURFACE_CLEAR_COLOR);

        assert_eq!(DEFAULT_SURFACE_CLEAR_COLOR, CellStyle::default().background);
        assert_eq!(clear.r, 0.0);
        assert_eq!(clear.g, 0.0);
        assert_eq!(clear.b, 0.0);
        assert_eq!(clear.a, 1.0);
    }

    #[test]
    fn default_cell_metrics_use_zero_terminal_padding() {
        assert_eq!(
            CellMetrics::default().padding,
            PixelPoint { x: 0.0, y: 0.0 }
        );
    }

    #[test]
    fn planner_uses_cursor_shape_for_cursor_rect() {
        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        let planner = FramePlanner::new(metrics);
        let mut snapshot = RenderSnapshot::from_plain_lines(&["hi"]);

        snapshot.cursor.shape = CursorShape::Bar;
        let bar = planner.plan(&snapshot).cursor.unwrap();
        assert!((bar.size.width - 1.8).abs() < f32::EPSILON * 8.0);
        assert_eq!(bar.size.height, 20.0);

        snapshot.cursor.shape = CursorShape::Underline;
        let underline = planner.plan(&snapshot).cursor.unwrap();
        assert_eq!(underline.origin, PixelPoint { x: 0.0, y: 17.0 });
        assert_eq!(
            underline.size,
            PixelSize {
                width: 10.0,
                height: 3.0,
            }
        );

        snapshot.cursor.shape = CursorShape::Block;
        let block = planner.plan(&snapshot).cursor.unwrap();
        assert_eq!(block.size, metrics.cell);
    }

    #[test]
    fn planner_uses_snapshot_cursor_color_override() {
        let planner = FramePlanner::new(CellMetrics::default());
        let mut snapshot = RenderSnapshot::from_plain_lines(&["hi"]);
        snapshot.cursor_color = Some(Rgba::rgb(1, 2, 3));

        let cursor = planner.plan(&snapshot).cursor.unwrap();

        assert_eq!(cursor.color, Rgba::rgb(1, 2, 3));
    }

    #[test]
    fn planner_merges_background_runs_and_splits_on_color() {
        let mut snapshot = RenderSnapshot::from_plain_lines(&["abc"]);
        snapshot.rows[0].cells[0].style.background = Rgba::rgb(10, 10, 10);
        snapshot.rows[0].cells[1].style.background = Rgba::rgb(10, 10, 10);
        snapshot.rows[0].cells[2].style.background = Rgba::rgb(20, 20, 20);
        let planner = FramePlanner::new(CellMetrics::default());

        let frame = planner.plan(&snapshot);

        assert_eq!(frame.backgrounds.len(), 2);
        assert_eq!(
            frame.backgrounds[0].size.width,
            2.0 * planner.metrics.cell.width
        );
        assert_eq!(
            frame.backgrounds[1].origin,
            planner.cell_origin(CellPoint::new(0, 2))
        );
    }

    #[test]
    fn planner_merges_text_runs_and_splits_on_blank_or_style() {
        let mut snapshot = RenderSnapshot::from_plain_lines(&["ab cd"]);
        snapshot.rows[0].cells[4].style.foreground = Rgba::rgb(120, 180, 240);
        let planner = FramePlanner::new(CellMetrics::default());

        let frame = planner.plan(&snapshot);

        assert_eq!(frame.glyphs.len(), 3);
        assert_eq!(frame.glyphs[0].text, "ab");
        assert_eq!(frame.glyphs[1].text, "c");
        assert_eq!(frame.glyphs[2].text, "d");
        assert_eq!(
            frame.glyphs[1].origin,
            planner.cell_origin(CellPoint::new(0, 3))
        );
    }

    #[test]
    fn planner_carries_style_flags_on_glyph_runs() {
        let mut snapshot = RenderSnapshot::from_plain_lines(&["abc"]);
        snapshot.rows[0].cells[1].style.flags.bold = true;
        snapshot.rows[0].cells[2].style.flags.italic = true;
        let planner = FramePlanner::new(CellMetrics::default());

        let frame = planner.plan(&snapshot);

        assert_eq!(frame.glyphs.len(), 3);
        assert_eq!(frame.glyphs[0].text, "a");
        assert_eq!(frame.glyphs[0].style_flags, CellFlags::default());
        assert_eq!(frame.glyphs[1].text, "b");
        assert!(frame.glyphs[1].style_flags.bold);
        assert!(!frame.glyphs[1].style_flags.italic);
        assert_eq!(frame.glyphs[2].text, "c");
        assert!(frame.glyphs[2].style_flags.italic);
        assert!(!frame.glyphs[2].style_flags.bold);
    }

    #[test]
    fn planner_splits_long_text_runs_for_webgpu_glyphon_batches() {
        let line = "x".repeat(MAX_GLYPHON_TEXT_AREA_CHARS + 5);
        let snapshot = RenderSnapshot::from_plain_lines(&[&line]);
        let planner = FramePlanner::new(CellMetrics::default());

        let frame = planner.plan(&snapshot);

        assert_eq!(frame.glyphs.len(), 2);
        assert_eq!(
            frame.glyphs[0].text.chars().count(),
            MAX_GLYPHON_TEXT_AREA_CHARS
        );
        assert_eq!(frame.stats.glyph_prepare_batches, 2);
        assert_eq!(frame.stats.max_glyph_run_chars, MAX_GLYPHON_TEXT_AREA_CHARS);
        assert_eq!(frame.glyphs[1].text, "xxxxx");
        assert_eq!(
            frame.glyphs[1].origin,
            planner.cell_origin(CellPoint::new(0, MAX_GLYPHON_TEXT_AREA_CHARS as u16))
        );
    }

    #[test]
    fn planner_skips_concealed_glyphs_without_collapsing_columns() {
        let mut snapshot = RenderSnapshot::from_plain_lines(&["abc"]);
        snapshot.rows[0].cells[1].style.flags.conceal = true;
        let planner = FramePlanner::new(CellMetrics::default());

        let frame = planner.plan(&snapshot);

        assert_eq!(frame.glyphs.len(), 2);
        assert_eq!(frame.glyphs[0].text, "a");
        assert_eq!(frame.glyphs[1].text, "c");
        assert_eq!(
            frame.glyphs[1].origin,
            planner.cell_origin(CellPoint::new(0, 2))
        );
        assert_eq!(frame.stats.glyph_chars, 2);
    }

    #[test]
    fn planner_hides_blinking_glyphs_and_decorations_when_phase_hidden() {
        let mut snapshot = RenderSnapshot::from_plain_lines(&["ab"]);
        snapshot.rows[0].cells[0].style.flags.blink = true;
        snapshot.rows[0].cells[0].style.flags.underline = true;
        snapshot.rows[0].cells[1].style.flags.underline = true;
        let planner = FramePlanner::new(CellMetrics::default()).with_blink_visible(false);

        let frame = planner.plan(&snapshot);

        assert_eq!(frame.glyphs.len(), 1);
        assert_eq!(frame.glyphs[0].text, "b");
        assert_eq!(
            frame.glyphs[0].origin,
            planner.cell_origin(CellPoint::new(0, 1))
        );
        assert_eq!(frame.text_decorations.len(), 1);
        assert_eq!(
            frame.text_decorations[0].origin.x,
            planner.cell_origin(CellPoint::new(0, 1)).x
        );
        assert_eq!(
            frame.text_decorations[0].size.width,
            planner.metrics.cell.width
        );
        assert_eq!(frame.stats.glyph_chars, 1);
    }

    #[test]
    fn planner_keeps_blinking_cells_visible_by_default() {
        let mut snapshot = RenderSnapshot::from_plain_lines(&["ab"]);
        snapshot.rows[0].cells[0].style.flags.blink = true;
        snapshot.rows[0].cells[0].style.flags.underline = true;
        snapshot.rows[0].cells[1].style.flags.underline = true;
        let planner = FramePlanner::new(CellMetrics::default());

        let frame = planner.plan(&snapshot);

        assert_eq!(
            frame
                .glyphs
                .iter()
                .map(|glyph| glyph.text.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert_eq!(frame.text_decorations.len(), 1);
        assert_eq!(
            frame.text_decorations[0].size.width,
            2.0 * planner.metrics.cell.width
        );
    }

    #[test]
    fn planner_offsets_superscript_and_subscript_glyph_runs() {
        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 10.0 },
        };
        let mut snapshot = RenderSnapshot::from_plain_lines(&["abc"]);
        snapshot.rows[0].cells[1].style.flags.baseline_shift = BaselineShift::Superscript;
        snapshot.rows[0].cells[2].style.flags.baseline_shift = BaselineShift::Subscript;
        let planner = FramePlanner::new(metrics);

        let frame = planner.plan(&snapshot);

        assert_eq!(frame.glyphs.len(), 3);
        assert_eq!(frame.glyphs[0].origin, PixelPoint { x: 0.0, y: 10.0 });
        assert_eq!(frame.glyphs[1].origin, PixelPoint { x: 10.0, y: 5.0 });
        assert_eq!(frame.glyphs[2].origin.x, 20.0);
        assert!((frame.glyphs[2].origin.y - 13.6).abs() < 0.001);
    }

    #[test]
    fn planner_batches_basic_text_decoration_rects() {
        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        let mut snapshot = RenderSnapshot::from_plain_lines(&["abcde"]);
        snapshot.rows[0].cells[0].style.flags.underline = true;
        snapshot.rows[0].cells[1].style.flags.underline = true;
        snapshot.rows[0].cells[2].style.flags.underline = true;
        snapshot.rows[0].cells[2].style.flags.underline_style = UnderlineStyle::Double;
        snapshot.rows[0].cells[2].style.underline_color = Some(Rgba::rgb(240, 80, 40));
        snapshot.rows[0].cells[3].style.flags.overline = true;
        snapshot.rows[0].cells[4].style.flags.overline = true;
        let planner = FramePlanner::new(metrics);

        let frame = planner.plan(&snapshot);

        assert_eq!(frame.text_decorations.len(), 4);
        assert_eq!(frame.text_decorations[0].origin.x, 0.0);
        assert_eq!(frame.text_decorations[0].origin.y, 18.0);
        assert_eq!(frame.text_decorations[0].size.width, 20.0);
        assert_eq!(frame.text_decorations[0].size.height, 2.0);
        assert_eq!(frame.text_decorations[0].color, Rgba::WHITE);
        assert_eq!(frame.text_decorations[1].origin.x, 20.0);
        assert_eq!(frame.text_decorations[1].origin.y, 14.0);
        assert_eq!(frame.text_decorations[1].color, Rgba::rgb(240, 80, 40));
        assert_eq!(frame.text_decorations[2].origin.x, 20.0);
        assert_eq!(frame.text_decorations[2].origin.y, 18.0);
        assert_eq!(frame.text_decorations[2].color, Rgba::rgb(240, 80, 40));
        assert_eq!(
            frame.text_decorations[3].origin,
            PixelPoint { x: 30.0, y: 0.0 }
        );
        assert_eq!(frame.text_decorations[3].size.width, 20.0);
        assert_eq!(frame.stats.text_decoration_rects, 4);
    }

    #[test]
    fn planner_snaps_manual_underline_to_single_pixel() {
        let mut snapshot = RenderSnapshot::from_plain_lines(&["mingxu@host >"]);
        for cell in snapshot.rows[0].cells.iter_mut().take(6) {
            cell.style.flags.underline = true;
        }
        let planner = FramePlanner::new(CellMetrics::default());

        let frame = planner.plan(&snapshot);

        assert_eq!(frame.text_decorations.len(), 1);
        assert_eq!(
            frame.text_decorations[0].origin,
            PixelPoint { x: 8.0, y: 25.0 }
        );
        assert_eq!(
            frame.text_decorations[0].size,
            PixelSize {
                width: 54.0,
                height: 1.0,
            }
        );
        assert_eq!(frame.text_decorations[0].color, Rgba::WHITE);
        assert_eq!(frame.stats.text_decoration_rects, 1);
    }

    #[test]
    fn fish_keyboard_modifier_control_does_not_create_text_decoration() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 16));

        terminal.feed(b"\x1b[>4;1mmingxu\x1b[m");
        let frame = FramePlanner::new(CellMetrics::default()).plan(&terminal.snapshot());

        assert_eq!(frame.text_decorations, Vec::new());
        assert_eq!(frame.stats.text_decoration_rects, 0);
    }

    #[test]
    fn planner_batches_strikethrough_decoration_rects() {
        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        let mut snapshot = RenderSnapshot::from_plain_lines(&["abcd"]);
        snapshot.rows[0].cells[0].style.flags.strike = true;
        snapshot.rows[0].cells[1].style.flags.strike = true;
        snapshot.rows[0].cells[1].style.flags.faint = true;
        snapshot.rows[0].cells[1].style.foreground = Rgba::rgb(200, 100, 50);
        snapshot.rows[0].cells[1].style.background = Rgba::BLACK;
        snapshot.rows[0].cells[2].style.flags.strike = true;
        snapshot.rows[0].cells[2].style.flags.faint = true;
        snapshot.rows[0].cells[2].style.foreground = Rgba::rgb(200, 100, 50);
        snapshot.rows[0].cells[2].style.background = Rgba::BLACK;
        let planner = FramePlanner::new(metrics);

        let frame = planner.plan(&snapshot);

        assert_eq!(frame.text_decorations.len(), 2);
        assert_eq!(frame.text_decorations[0].origin.x, 0.0);
        assert_eq!(frame.text_decorations[0].origin.y, 9.0);
        assert_eq!(frame.text_decorations[0].size.width, 10.0);
        assert_eq!(frame.text_decorations[0].size.height, 2.0);
        assert_eq!(frame.text_decorations[0].color, Rgba::WHITE);
        assert_eq!(frame.text_decorations[1].origin.x, 10.0);
        assert_eq!(frame.text_decorations[1].origin.y, 9.0);
        assert_eq!(frame.text_decorations[1].size.width, 20.0);
        assert_eq!(frame.text_decorations[1].color, Rgba::rgb(100, 50, 25));
        assert_eq!(frame.stats.text_decoration_rects, 2);
    }

    #[test]
    fn planner_batches_framed_decoration_rects() {
        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        let mut snapshot = RenderSnapshot::from_plain_lines(&["abcd"]);
        snapshot.rows[0].cells[0].style.flags.framed = true;
        snapshot.rows[0].cells[1].style.flags.framed = true;
        snapshot.rows[0].cells[2].style.flags.framed = true;
        snapshot.rows[0].cells[2].style.flags.faint = true;
        snapshot.rows[0].cells[2].style.foreground = Rgba::rgb(200, 100, 50);
        snapshot.rows[0].cells[2].style.background = Rgba::BLACK;
        let planner = FramePlanner::new(metrics);

        let frame = planner.plan(&snapshot);

        assert_eq!(frame.text_decorations.len(), 8);
        assert_eq!(
            frame.text_decorations[0].origin,
            PixelPoint { x: 0.0, y: 0.0 }
        );
        assert_eq!(frame.text_decorations[0].size.width, 20.0);
        assert_eq!(frame.text_decorations[0].size.height, 2.0);
        assert_eq!(frame.text_decorations[1].origin.y, 18.0);
        assert_eq!(frame.text_decorations[2].size.height, 20.0);
        assert_eq!(frame.text_decorations[3].origin.x, 18.0);
        assert_eq!(
            frame.text_decorations[4].origin,
            PixelPoint { x: 20.0, y: 0.0 }
        );
        assert_eq!(frame.text_decorations[4].color, Rgba::rgb(100, 50, 25));
        assert_eq!(frame.text_decorations[4].size.width, 10.0);
        assert_eq!(frame.stats.text_decoration_rects, 8);
    }

    #[test]
    fn planner_batches_encircled_decoration_rects() {
        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        let mut snapshot = RenderSnapshot::from_plain_lines(&["abcd"]);
        snapshot.rows[0].cells[0].style.flags.encircled = true;
        snapshot.rows[0].cells[1].style.flags.encircled = true;
        snapshot.rows[0].cells[2].style.flags.encircled = true;
        snapshot.rows[0].cells[2].style.flags.faint = true;
        snapshot.rows[0].cells[2].style.foreground = Rgba::rgb(200, 100, 50);
        snapshot.rows[0].cells[2].style.background = Rgba::BLACK;
        let planner = FramePlanner::new(metrics);

        let frame = planner.plan(&snapshot);

        assert_eq!(frame.text_decorations.len(), 8);
        assert!((frame.text_decorations[0].origin.x - 3.5).abs() < 0.001);
        assert_eq!(frame.text_decorations[0].origin.y, 0.0);
        assert_eq!(frame.text_decorations[0].size.width, 13.0);
        assert_eq!(frame.text_decorations[1].origin.y, 18.0);
        assert_eq!(frame.text_decorations[2].origin.x, 0.0);
        assert_eq!(frame.text_decorations[2].origin.y, 5.0);
        assert_eq!(frame.text_decorations[2].size.height, 10.0);
        assert_eq!(frame.text_decorations[3].origin.x, 18.0);
        assert!((frame.text_decorations[4].origin.x - 23.5).abs() < 0.001);
        assert_eq!(frame.text_decorations[4].color, Rgba::rgb(100, 50, 25));
        assert_eq!(frame.text_decorations[4].size.width, 3.0);
        assert_eq!(frame.stats.text_decoration_rects, 8);
    }

    #[test]
    fn planner_dims_faint_glyphs_and_implicit_decorations() {
        let mut snapshot = RenderSnapshot::from_plain_lines(&["abc"]);
        for cell in &mut snapshot.rows[0].cells {
            cell.style.foreground = Rgba::rgb(200, 100, 50);
            cell.style.background = Rgba::BLACK;
        }
        snapshot.rows[0].cells[1].style.flags.faint = true;
        snapshot.rows[0].cells[1].style.flags.underline = true;
        snapshot.rows[0].cells[2].style.flags.faint = true;
        snapshot.rows[0].cells[2].style.flags.underline = true;
        snapshot.rows[0].cells[2].style.underline_color = Some(Rgba::rgb(80, 200, 20));
        let planner = FramePlanner::new(CellMetrics::default());

        let frame = planner.plan(&snapshot);

        assert_eq!(frame.glyphs.len(), 2);
        assert_eq!(frame.glyphs[0].color, Rgba::rgb(200, 100, 50));
        assert_eq!(frame.glyphs[0].text, "a");
        assert_eq!(frame.glyphs[1].color, Rgba::rgb(100, 50, 25));
        assert_eq!(frame.glyphs[1].text, "bc");
        assert_eq!(frame.text_decorations.len(), 2);
        assert_eq!(frame.text_decorations[0].color, Rgba::rgb(100, 50, 25));
        assert_eq!(frame.text_decorations[1].color, Rgba::rgb(80, 200, 20));
    }

    #[test]
    fn planner_segments_dotted_and_dashed_underlines() {
        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        let mut snapshot = RenderSnapshot::from_plain_lines(&["abcde"]);
        for cell in &mut snapshot.rows[0].cells {
            cell.style.flags.underline = true;
            cell.style.foreground = Rgba::rgb(210, 210, 210);
        }
        snapshot.rows[0].cells[0].style.flags.underline_style = UnderlineStyle::Dotted;
        snapshot.rows[0].cells[1].style.flags.underline_style = UnderlineStyle::Dotted;
        snapshot.rows[0].cells[2].style.flags.underline_style = UnderlineStyle::Dashed;
        snapshot.rows[0].cells[3].style.flags.underline_style = UnderlineStyle::Dashed;
        snapshot.rows[0].cells[4].style.flags.underline_style = UnderlineStyle::Curly;
        let planner = FramePlanner::new(metrics);

        let frame = planner.plan(&snapshot);

        assert_eq!(frame.text_decorations.len(), 7);
        assert_eq!(frame.text_decorations[0].origin.x, 4.0);
        assert_eq!(frame.text_decorations[0].origin.y, 18.0);
        assert_eq!(frame.text_decorations[0].size.width, 2.0);
        assert_eq!(frame.text_decorations[1].origin.x, 14.0);
        assert!((frame.text_decorations[2].origin.x - 21.9).abs() < 0.001);
        assert!((frame.text_decorations[2].size.width - 6.2).abs() < 0.001);
        assert!((frame.text_decorations[3].origin.x - 31.9).abs() < 0.001);
        assert_eq!(frame.text_decorations[4].origin.x, 40.0);
        assert_eq!(frame.text_decorations[4].origin.y, 14.0);
        assert_eq!(frame.text_decorations[4].size.width, 5.0);
        assert_eq!(
            frame.text_decorations[5].origin,
            PixelPoint { x: 45.0, y: 18.0 }
        );
        assert_eq!(frame.text_decorations[6].origin.x, 44.0);
        assert_eq!(frame.stats.text_decoration_rects, 7);
    }

    #[test]
    fn frame_stats_report_glyph_prepare_batch_count() {
        let color = Rgba::rgb(240, 240, 240);
        let mut frame = FramePlan {
            glyphs: vec![
                glyph_item(0.0, 0.0, &"a".repeat(60), color),
                glyph_item(0.0, 20.0, &"b".repeat(50), color),
                glyph_item(0.0, 40.0, &"c".repeat(20), color),
            ],
            ..FramePlan::default()
        };

        frame.refresh_stats(3, 80);

        assert_eq!(frame.stats.glyph_runs, 3);
        assert_eq!(frame.stats.glyph_chars, 130);
        assert_eq!(frame.stats.glyph_prepare_batches, 2);
        assert_eq!(frame.stats.max_glyph_run_chars, 60);
    }

    #[test]
    fn text_buffer_item_chunks_keep_glyphon_prepare_batches_bounded() {
        let mut font_system = FontSystem::new();
        let font_config = RendererFontConfig::default();
        let items = ["a".repeat(60), "b".repeat(50), "c".repeat(20)]
            .into_iter()
            .map(|text| {
                let glyph = glyph_item(0.0, 0.0, &text, Rgba::rgb(240, 240, 240));
                let key = TextBufferKey::new(1000.0, 160, &glyph, &font_config);
                TextBufferItem::new(&mut font_system, key, &glyph)
            })
            .collect::<Vec<_>>();

        let chunks = text_buffer_item_chunks(&items);
        assert_eq!(chunks, vec![0..2, 2..3]);
        for chunk in chunks {
            let chunk_chars = items[chunk]
                .iter()
                .map(|item| item.key.text.chars().count())
                .sum::<usize>();
            assert!(chunk_chars <= MAX_GLYPHON_RENDERER_CHARS);
        }
    }

    #[test]
    fn text_attrs_for_style_flags_map_bold_and_italic_to_font_attrs() {
        let normal = text_attrs_for_style_flags(CellFlags::default(), None);
        assert_eq!(normal.weight, Weight::NORMAL);
        assert_eq!(normal.style, Style::Normal);

        let mut flags = CellFlags::default();
        flags.bold = true;
        flags.italic = true;
        let styled = text_attrs_for_style_flags(flags, None);

        assert_eq!(styled.weight, Weight::BOLD);
        assert_eq!(styled.style, Style::Italic);
        assert_eq!(styled.family, Family::Monospace);
    }

    #[test]
    fn text_attrs_for_style_flags_use_configured_font_family() {
        let attrs =
            text_attrs_for_style_flags(CellFlags::default(), Some("JetBrainsMono Nerd Font"));

        assert_eq!(attrs.family, Family::Name("JetBrainsMono Nerd Font"));
    }

    #[test]
    fn renderer_font_config_trims_and_ignores_empty_family() {
        assert_eq!(
            RendererFontConfig::new(Some("  FiraCode Nerd Font  ".to_owned())).family(),
            Some("FiraCode Nerd Font")
        );
        assert_eq!(
            RendererFontConfig::new(Some("   ".to_owned())).family(),
            None
        );
    }

    #[test]
    fn renderer_font_config_scales_metrics_from_font_size() {
        let config = RendererFontConfig::with_font_size(None, 21);
        let metrics = config.cell_metrics();

        assert_eq!(config.font_size(), 21);
        assert_eq!(config.font_scale_factor(), 1.0);
        assert_eq!(config.effective_font_size(), 21.0);
        assert!((config.line_height() - 27.0).abs() < 0.001);
        assert!((metrics.cell.width - 13.5).abs() < 0.001);
        assert!((metrics.cell.height - 27.0).abs() < 0.001);
    }

    #[test]
    fn renderer_font_config_scales_metrics_from_scale_factor() {
        let config = RendererFontConfig::with_font_size(None, 14).with_scale_factor(2.0);
        let metrics = config.cell_metrics();

        assert_eq!(config.font_size(), 14);
        assert_eq!(config.font_scale_factor(), 2.0);
        assert_eq!(config.effective_font_size(), 28.0);
        assert!((config.line_height() - 36.0).abs() < 0.001);
        assert!((metrics.cell.width - 18.0).abs() < 0.001);
        assert!((metrics.cell.height - 36.0).abs() < 0.001);
        assert!((metrics.padding.x - 0.0).abs() < 0.001);
        assert!((metrics.padding.y - 0.0).abs() < 0.001);
    }

    #[test]
    fn renderer_font_config_uses_configured_terminal_padding() {
        let config = RendererFontConfig::with_font_size(None, 14)
            .with_terminal_padding(8.0)
            .with_scale_factor(2.0);
        let metrics = config.cell_metrics();

        assert_eq!(config.terminal_padding(), 8.0);
        assert!((metrics.padding.x - 16.0).abs() < 0.001);
        assert!((metrics.padding.y - 16.0).abs() < 0.001);
    }

    #[test]
    fn sorted_unique_font_family_names_trims_sorts_and_deduplicates() {
        assert_eq!(
            sorted_unique_font_family_names([
                "  JetBrainsMono Nerd Font  ",
                "",
                "FiraCode Nerd Font",
                "JetBrainsMono Nerd Font",
                "  ",
                "Symbols Nerd Font Mono",
            ]),
            vec![
                "FiraCode Nerd Font".to_owned(),
                "JetBrainsMono Nerd Font".to_owned(),
                "Symbols Nerd Font Mono".to_owned(),
            ]
        );
    }

    #[test]
    fn font_sources_from_data_uses_binary_sources() {
        let sources = font_sources_from_data(vec![vec![1_u8, 2, 3]]);

        assert_eq!(sources.len(), 1);
        match &sources[0] {
            fontdb::Source::Binary(data) => assert_eq!(data.as_ref().as_ref(), &[1, 2, 3]),
            source => panic!("expected binary font source, got {source:?}"),
        }
    }

    #[test]
    fn text_metrics_for_style_flags_scales_baseline_shift_runs() {
        let normal =
            text_metrics_for_style_flags(CellFlags::default(), DEFAULT_TERMINAL_FONT_SIZE, 1.0);
        assert_eq!(normal, Metrics::new(14.0, DEFAULT_TERMINAL_LINE_HEIGHT));

        let mut superscript = CellFlags::default();
        superscript.baseline_shift = BaselineShift::Superscript;
        let superscript_metrics =
            text_metrics_for_style_flags(superscript, DEFAULT_TERMINAL_FONT_SIZE, 1.0);

        assert!((superscript_metrics.font_size - 10.08).abs() < 0.001);
        assert!((superscript_metrics.line_height - 12.96).abs() < 0.001);

        let mut subscript = CellFlags::default();
        subscript.baseline_shift = BaselineShift::Subscript;
        assert_eq!(
            text_metrics_for_style_flags(subscript, DEFAULT_TERMINAL_FONT_SIZE, 1.0),
            superscript_metrics
        );
    }

    #[test]
    fn text_metrics_for_style_flags_uses_configured_font_size() {
        let metrics = text_metrics_for_style_flags(CellFlags::default(), 21, 1.0);

        assert_eq!(metrics.font_size, 21.0);
        assert!((metrics.line_height - 27.0).abs() < 0.001);
    }

    #[test]
    fn text_metrics_for_style_flags_uses_font_scale_factor() {
        let metrics = text_metrics_for_style_flags(CellFlags::default(), 14, 1.5);

        assert_eq!(metrics.font_size, 21.0);
        assert!((metrics.line_height - 27.0).abs() < 0.001);
    }

    #[test]
    fn frame_stats_report_run_counts_and_cursor_state() {
        let mut snapshot = RenderSnapshot::from_plain_lines(&["ab cd"]);
        snapshot.rows[0].cells[4].style.foreground = Rgba::rgb(120, 180, 240);
        snapshot.selection = Some(CellRange {
            start: CellPoint::new(0, 0),
            end: CellPoint::new(0, 1),
        });
        let planner = FramePlanner::new(CellMetrics::default());

        let frame = planner.plan(&snapshot);

        assert_eq!(
            frame.stats,
            FrameStats {
                visible_rows: 1,
                visible_cols: 5,
                background_runs: 1,
                glyph_runs: 3,
                glyph_chars: 4,
                glyph_prepare_batches: 1,
                max_glyph_run_chars: 2,
                selection_rects: 1,
                search_highlight_rects: 0,
                hyperlink_hover_rects: 0,
                hyperlink_underline_rects: 0,
                text_decoration_rects: 0,
                ime_preedit_rects: 0,
                search_active_visible: false,
                cursor_visible: true,
                rect_vertices: 18,
                rect_vertex_capacity: 32,
                full_damage: true,
                damage_regions: 1,
                reused_rows: 0,
                rebuilt_rows: 1,
            }
        );
    }

    #[test]
    fn frame_plan_carries_snapshot_damage_contract() {
        let mut snapshot = RenderSnapshot::from_plain_lines(&["abc", "def"]);
        snapshot.damage = DamageRegion::Rows(vec![1]);
        let planner = FramePlanner::new(CellMetrics::default());

        let frame = planner.plan(&snapshot);

        assert_eq!(frame.damage, DamageRegion::Rows(vec![1]));
        assert!(!frame.stats.full_damage);
        assert_eq!(frame.stats.damage_regions, 1);
    }

    #[test]
    fn retained_planner_rebuilds_only_damaged_rows() {
        let first = RenderSnapshot::from_plain_lines(&["abc", "def"]);
        let mut planner = RetainedFramePlanner::new(CellMetrics::default());

        let first_frame = planner.plan(&first);

        assert_eq!(first_frame.stats.reused_rows, 0);
        assert_eq!(first_frame.stats.rebuilt_rows, 2);

        let mut second = RenderSnapshot::from_plain_lines(&["abc", "xyz"]);
        second.damage = DamageRegion::Rows(vec![1]);
        let second_frame = planner.plan(&second);

        assert_eq!(
            second_frame
                .glyphs
                .iter()
                .map(|glyph| glyph.text.as_str())
                .collect::<Vec<_>>(),
            vec!["abc", "xyz"]
        );
        assert_eq!(second_frame.stats.reused_rows, 1);
        assert_eq!(second_frame.stats.rebuilt_rows, 1);
        assert_eq!(second_frame.damage, DamageRegion::Rows(vec![1]));
    }

    #[test]
    fn retained_planner_reuses_all_rows_for_empty_row_damage() {
        let first = RenderSnapshot::from_plain_lines(&["abc", "def"]);
        let mut planner = RetainedFramePlanner::new(CellMetrics::default());
        planner.plan(&first);

        let mut second = RenderSnapshot::from_plain_lines(&["abc", "def"]);
        second.damage = DamageRegion::Rows(vec![]);
        let second_frame = planner.plan(&second);

        assert_eq!(second_frame.stats.reused_rows, 2);
        assert_eq!(second_frame.stats.rebuilt_rows, 0);
    }

    #[test]
    fn planner_draws_regular_and_active_search_highlights() {
        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        let mut snapshot = RenderSnapshot::from_plain_lines(&["error error"]);
        snapshot.search_highlights = vec![
            SearchHighlight {
                range: CellRange {
                    start: CellPoint::new(0, 0),
                    end: CellPoint::new(0, 4),
                },
                active: false,
            },
            SearchHighlight {
                range: CellRange {
                    start: CellPoint::new(0, 6),
                    end: CellPoint::new(0, 10),
                },
                active: true,
            },
        ];
        let planner = FramePlanner::new(metrics);

        let frame = planner.plan(&snapshot);

        assert_eq!(frame.search_highlights.len(), 2);
        assert_eq!(
            frame.search_highlights[0].origin,
            PixelPoint { x: 0.0, y: 0.0 }
        );
        assert_eq!(
            frame.search_highlights[0].size,
            PixelSize {
                width: 50.0,
                height: 20.0,
            }
        );
        assert_eq!(
            frame.search_highlights[0].color,
            search_highlight_color(SearchHighlightKind::Regular)
        );
        assert_eq!(
            frame.search_highlights[1].origin,
            PixelPoint { x: 60.0, y: 0.0 }
        );
        assert_eq!(
            frame.search_highlights[1].color,
            search_highlight_color(SearchHighlightKind::Active)
        );
        assert_eq!(frame.stats.search_highlight_rects, 2);
        assert!(frame.stats.search_active_visible);
    }

    #[test]
    fn planner_draws_hyperlink_underlines_for_contiguous_spans() {
        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        let mut snapshot = RenderSnapshot::from_plain_lines(&["ab cd"]);
        snapshot.rows[0].cells[0].hyperlink = Some(7);
        snapshot.rows[0].cells[1].hyperlink = Some(7);
        let planner = FramePlanner::new(metrics);

        let frame = planner.plan(&snapshot);

        assert_eq!(frame.hyperlink_underlines.len(), 1);
        assert_eq!(frame.hyperlink_hover.len(), 0);
        assert_eq!(
            frame.hyperlink_underlines[0].origin.x,
            planner.cell_origin(CellPoint::new(0, 0)).x
        );
        assert!((frame.hyperlink_underlines[0].origin.y - 18.4).abs() < 0.001);
        assert_eq!(frame.hyperlink_underlines[0].size.width, 20.0);
        assert!((frame.hyperlink_underlines[0].size.height - 1.6).abs() < 0.001);
        assert_eq!(frame.stats.hyperlink_underline_rects, 1);
        assert_eq!(frame.stats.hyperlink_hover_rects, 0);
    }

    #[test]
    fn planner_underlines_wide_linked_cells_across_their_full_width() {
        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        let mut snapshot = RenderSnapshot::from_plain_lines(&["a界b"]);
        snapshot.rows[0].cells[1].hyperlink = Some(7);
        let planner = FramePlanner::new(metrics);

        let frame = planner.plan(&snapshot);

        assert_eq!(frame.hyperlink_underlines.len(), 1);
        assert_eq!(frame.hyperlink_underlines[0].origin.x, 10.0);
        assert!((frame.hyperlink_underlines[0].origin.y - 18.4).abs() < 0.001);
        assert_eq!(frame.hyperlink_underlines[0].size.width, 20.0);
    }

    #[test]
    fn planner_draws_hover_overlay_only_for_hovered_hyperlink() {
        let metrics = CellMetrics {
            cell: PixelSize {
                width: 10.0,
                height: 20.0,
            },
            padding: PixelPoint { x: 0.0, y: 0.0 },
        };
        let mut snapshot = RenderSnapshot::from_plain_lines(&["ab cd"]);
        snapshot.rows[0].cells[0].hyperlink = Some(7);
        snapshot.rows[0].cells[1].hyperlink = Some(7);
        snapshot.rows[0].cells[3].hyperlink = Some(8);
        snapshot.rows[0].cells[4].hyperlink = Some(8);
        snapshot.hovered_hyperlink = Some(8);
        let planner = FramePlanner::new(metrics);

        let frame = planner.plan(&snapshot);

        assert_eq!(frame.hyperlink_underlines.len(), 2);
        assert_eq!(frame.hyperlink_hover.len(), 1);
        assert_eq!(
            frame.hyperlink_hover[0].origin,
            planner.cell_origin(CellPoint::new(0, 3))
        );
        assert_eq!(
            frame.hyperlink_hover[0].size,
            PixelSize {
                width: 20.0,
                height: 20.0,
            }
        );
        assert_eq!(frame.hyperlink_hover[0].color, hyperlink_hover_color());
        assert_eq!(frame.stats.hyperlink_hover_rects, 1);
    }

    #[test]
    fn retained_planner_reuses_rows_for_search_overlay_only_changes() {
        let first = RenderSnapshot::from_plain_lines(&["abc", "def"]);
        let mut planner = RetainedFramePlanner::new(CellMetrics::default());
        planner.plan(&first);

        let mut second = RenderSnapshot::from_plain_lines(&["abc", "def"]);
        second.damage = DamageRegion::Rows(vec![]);
        second.search_highlights = vec![SearchHighlight {
            range: CellRange {
                start: CellPoint::new(1, 0),
                end: CellPoint::new(1, 2),
            },
            active: true,
        }];

        let frame = planner.plan(&second);

        assert_eq!(frame.stats.reused_rows, 2);
        assert_eq!(frame.stats.rebuilt_rows, 0);
        assert_eq!(frame.stats.search_highlight_rects, 1);
        assert!(frame.stats.search_active_visible);
        assert_eq!(
            frame
                .glyphs
                .iter()
                .map(|glyph| glyph.text.as_str())
                .collect::<Vec<_>>(),
            vec!["abc", "def"]
        );
    }

    #[test]
    fn retained_planner_reuses_rows_for_selection_only_changes() {
        let first = RenderSnapshot::from_plain_lines(&["abc", "def"]);
        let mut planner = RetainedFramePlanner::new(CellMetrics::default());
        planner.plan(&first);

        let mut second = RenderSnapshot::from_plain_lines(&["abc", "def"]);
        second.damage = DamageRegion::Rows(vec![]);
        second.selection = Some(CellRange {
            start: CellPoint::new(0, 1),
            end: CellPoint::new(1, 1),
        });

        let frame = planner.plan(&second);

        assert_eq!(frame.stats.reused_rows, 2);
        assert_eq!(frame.stats.rebuilt_rows, 0);
        assert_eq!(frame.stats.selection_rects, 2);
        assert_eq!(
            frame
                .glyphs
                .iter()
                .map(|glyph| glyph.text.as_str())
                .collect::<Vec<_>>(),
            vec!["abc", "def"]
        );
    }

    #[test]
    fn retained_planner_reuses_rows_for_cursor_only_changes() {
        let first = RenderSnapshot::from_plain_lines(&["abc", "def"]);
        let mut planner = RetainedFramePlanner::new(CellMetrics::default());
        planner.plan(&first);

        let mut second = RenderSnapshot::from_plain_lines(&["abc", "def"]);
        second.damage = DamageRegion::Rows(vec![]);
        second.cursor.position = CellPoint::new(1, 2);

        let frame = planner.plan(&second);

        assert_eq!(frame.stats.reused_rows, 2);
        assert_eq!(frame.stats.rebuilt_rows, 0);
        assert!(frame.stats.cursor_visible);
        assert_eq!(
            frame
                .glyphs
                .iter()
                .map(|glyph| glyph.text.as_str())
                .collect::<Vec<_>>(),
            vec!["abc", "def"]
        );
    }

    #[test]
    fn retained_planner_rebuilds_rows_when_blink_phase_changes() {
        let mut snapshot = RenderSnapshot::from_plain_lines(&["ab"]);
        snapshot.rows[0].cells[0].style.flags.blink = true;
        let mut planner = RetainedFramePlanner::new(CellMetrics::default());
        let first = planner.plan(&snapshot);
        assert_eq!(
            first
                .glyphs
                .iter()
                .map(|glyph| glyph.text.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );

        let mut hidden = snapshot;
        hidden.damage = DamageRegion::Rows(vec![]);
        planner.set_blink_visible(false);
        let second = planner.plan(&hidden);

        assert_eq!(second.stats.reused_rows, 0);
        assert_eq!(second.stats.rebuilt_rows, 1);
        assert_eq!(second.glyphs.len(), 1);
        assert_eq!(second.glyphs[0].text, "b");
    }

    #[test]
    fn retained_planner_reuses_rows_for_hyperlink_hover_only_changes() {
        let mut first = RenderSnapshot::from_plain_lines(&["abc", "def"]);
        first.rows[1].cells[0].hyperlink = Some(9);
        first.rows[1].cells[1].hyperlink = Some(9);
        first.rows[1].cells[2].hyperlink = Some(9);
        let mut planner = RetainedFramePlanner::new(CellMetrics::default());
        planner.plan(&first);

        let mut second = first.clone();
        second.damage = DamageRegion::Rows(vec![]);
        second.hovered_hyperlink = Some(9);

        let frame = planner.plan(&second);

        assert_eq!(frame.stats.reused_rows, 2);
        assert_eq!(frame.stats.rebuilt_rows, 0);
        assert_eq!(frame.stats.hyperlink_hover_rects, 1);
        assert_eq!(frame.stats.hyperlink_underline_rects, 1);
        assert_eq!(
            frame
                .glyphs
                .iter()
                .map(|glyph| glyph.text.as_str())
                .collect::<Vec<_>>(),
            vec!["abc", "def"]
        );
    }

    #[test]
    fn retained_planner_falls_back_to_full_rebuild_for_size_change() {
        let first = RenderSnapshot::from_plain_lines(&["abc", "def"]);
        let mut planner = RetainedFramePlanner::new(CellMetrics::default());
        planner.plan(&first);

        let mut second = RenderSnapshot::from_plain_lines(&["abc", "def", "ghi"]);
        second.damage = DamageRegion::Rows(vec![2]);
        let second_frame = planner.plan(&second);

        assert_eq!(second_frame.stats.reused_rows, 0);
        assert_eq!(second_frame.stats.rebuilt_rows, 3);
    }

    #[test]
    fn text_buffer_cache_reuses_stable_runs_and_rebuilds_changed_runs() {
        let mut font_system = FontSystem::new();
        let mut cache = TextBufferCache::default();
        let font_config = RendererFontConfig::default();
        let glyphs = vec![
            glyph_item(0.0, 0.0, "prompt", Rgba::WHITE),
            glyph_item(54.0, 0.0, "value", Rgba::rgb(120, 180, 240)),
        ];

        assert_eq!(
            cache.sync(&mut font_system, &font_config, 800.0, 80, &glyphs),
            TextBufferSyncStats {
                reused: 0,
                rebuilt: 2,
                retired: 0,
            }
        );
        assert_eq!(
            cache.sync(&mut font_system, &font_config, 800.0, 80, &glyphs),
            TextBufferSyncStats {
                reused: 2,
                rebuilt: 0,
                retired: 0,
            }
        );

        let changed = vec![
            glyph_item(0.0, 0.0, "prompt", Rgba::WHITE),
            glyph_item(54.0, 0.0, "changed", Rgba::rgb(120, 180, 240)),
        ];

        assert_eq!(
            cache.sync(&mut font_system, &font_config, 800.0, 80, &changed),
            TextBufferSyncStats {
                reused: 1,
                rebuilt: 1,
                retired: 0,
            }
        );
    }

    #[test]
    fn text_buffer_cache_rebuilds_on_style_flag_change() {
        let mut font_system = FontSystem::new();
        let mut cache = TextBufferCache::default();
        let font_config = RendererFontConfig::default();
        let glyphs = vec![glyph_item(0.0, 0.0, "styled", Rgba::WHITE)];

        cache.sync(&mut font_system, &font_config, 800.0, 80, &glyphs);

        let mut changed = glyph_item(0.0, 0.0, "styled", Rgba::WHITE);
        changed.style_flags.bold = true;

        assert_eq!(
            cache.sync(&mut font_system, &font_config, 800.0, 80, &[changed]),
            TextBufferSyncStats {
                reused: 0,
                rebuilt: 1,
                retired: 0,
            }
        );
    }

    #[test]
    fn text_buffer_cache_rebuilds_on_width_change_and_retires_old_items() {
        let mut font_system = FontSystem::new();
        let mut cache = TextBufferCache::default();
        let font_config = RendererFontConfig::default();
        let glyphs = vec![
            glyph_item(0.0, 0.0, "one", Rgba::WHITE),
            glyph_item(18.0, 0.0, "two", Rgba::WHITE),
        ];

        cache.sync(&mut font_system, &font_config, 800.0, 80, &glyphs);

        assert_eq!(
            cache.sync(&mut font_system, &font_config, 640.0, 80, &glyphs[..1]),
            TextBufferSyncStats {
                reused: 0,
                rebuilt: 1,
                retired: 1,
            }
        );
        assert_eq!(cache.items.len(), 1);
    }

    #[test]
    fn text_buffer_cache_rebuilds_on_font_family_change() {
        let mut font_system = FontSystem::new();
        let mut cache = TextBufferCache::default();
        let glyphs = vec![glyph_item(0.0, 0.0, "icons", Rgba::WHITE)];
        let default_font = RendererFontConfig::default();
        let nerd_font = RendererFontConfig::new(Some("JetBrainsMono Nerd Font".to_owned()));

        cache.sync(&mut font_system, &default_font, 800.0, 80, &glyphs);

        assert_eq!(
            cache.sync(&mut font_system, &nerd_font, 800.0, 80, &glyphs),
            TextBufferSyncStats {
                reused: 0,
                rebuilt: 1,
                retired: 0,
            }
        );
    }

    #[test]
    fn text_buffer_cache_rebuilds_on_font_size_change() {
        let mut font_system = FontSystem::new();
        let mut cache = TextBufferCache::default();
        let glyphs = vec![glyph_item(0.0, 0.0, "icons", Rgba::WHITE)];
        let default_font = RendererFontConfig::default();
        let larger_font = RendererFontConfig::with_font_size(None, 18);

        cache.sync(&mut font_system, &default_font, 800.0, 80, &glyphs);

        assert_eq!(
            cache.sync(&mut font_system, &larger_font, 800.0, 80, &glyphs),
            TextBufferSyncStats {
                reused: 0,
                rebuilt: 1,
                retired: 0,
            }
        );
    }

    #[test]
    fn text_buffer_cache_rebuilds_on_font_scale_factor_change() {
        let mut font_system = FontSystem::new();
        let mut cache = TextBufferCache::default();
        let glyphs = vec![glyph_item(0.0, 0.0, "icons", Rgba::WHITE)];
        let default_font = RendererFontConfig::default();
        let hidpi_font = RendererFontConfig::default().with_scale_factor(2.0);

        cache.sync(&mut font_system, &default_font, 800.0, 80, &glyphs);

        assert_eq!(
            cache.sync(&mut font_system, &hidpi_font, 800.0, 80, &glyphs),
            TextBufferSyncStats {
                reused: 0,
                rebuilt: 1,
                retired: 0,
            }
        );
    }

    #[test]
    fn renderer_cache_stats_reports_text_buffer_sync_counts() {
        assert_eq!(
            renderer_cache_stats(
                TextBufferSyncStats {
                    reused: 3,
                    rebuilt: 2,
                    retired: 1,
                },
                5,
                2,
                128,
            ),
            RendererCacheStats {
                text_buffers_reused: 3,
                text_buffers_rebuilt: 2,
                text_buffers_retired: 1,
                text_buffer_count: 5,
                text_renderer_count: 2,
                rect_vertex_capacity: 128,
            }
        );
    }

    #[test]
    fn renderer_timing_stats_sum_cpu_prepare_sections() {
        assert_eq!(
            renderer_timing_stats(2, 3, 5),
            RendererTimingStats {
                cpu_prepare_us: 10,
                text_buffer_sync_us: 2,
                glyph_prepare_us: 3,
                rect_vertex_sync_us: 5,
            }
        );
    }

    #[test]
    fn text_buffer_width_tracks_text_columns_instead_of_surface_remainder() {
        let glyph = glyph_item(8.0, 0.0, "abc", Rgba::WHITE);
        let font_config = RendererFontConfig::default();

        assert_eq!(text_buffer_width_px(800.0, 80, &glyph, &font_config), 40);
    }

    #[test]
    fn text_buffer_width_counts_wide_text_and_clamps_to_available_surface() {
        let wide = glyph_item(8.0, 0.0, "界a", Rgba::WHITE);
        let near_right_edge = glyph_item(780.0, 0.0, "abcdef", Rgba::WHITE);
        let font_config = RendererFontConfig::default();

        assert_eq!(text_buffer_width_px(800.0, 80, &wide, &font_config), 40);
        assert_eq!(
            text_buffer_width_px(800.0, 80, &near_right_edge, &font_config),
            20
        );
    }

    #[test]
    fn rect_vertices_are_in_ndc_space() {
        let mut vertices = Vec::new();
        push_rect_vertices(
            &mut vertices,
            0.0,
            0.0,
            50.0,
            50.0,
            Rgba::WHITE,
            100.0,
            100.0,
        );

        assert_eq!(vertices.len(), 6);
        assert_eq!(vertices[0].position, [-1.0, 1.0]);
        assert_eq!(vertices[2].position, [0.0, 0.0]);
    }

    #[test]
    fn rect_vertex_capacity_grows_in_power_of_two_buckets() {
        assert_eq!(rect_vertex_capacity_for_len(1), 8);
        assert_eq!(rect_vertex_capacity_for_len(6), 8);
        assert_eq!(rect_vertex_capacity_for_len(9), 16);
        assert_eq!(rect_vertex_capacity_for_len(65), 128);
    }

    #[test]
    fn frame_stats_report_rect_vertex_capacity_bucket() {
        let mut frame = FramePlan {
            backgrounds: vec![
                RectBatchItem {
                    origin: PixelPoint { x: 0.0, y: 0.0 },
                    size: PixelSize {
                        width: 9.0,
                        height: 18.0,
                    },
                    color: Rgba::rgb(0, 0, 0),
                },
                RectBatchItem {
                    origin: PixelPoint { x: 9.0, y: 0.0 },
                    size: PixelSize {
                        width: 9.0,
                        height: 18.0,
                    },
                    color: Rgba::rgb(16, 16, 16),
                },
            ],
            cursor: Some(RectBatchItem {
                origin: PixelPoint { x: 18.0, y: 0.0 },
                size: PixelSize {
                    width: 9.0,
                    height: 18.0,
                },
                color: Rgba::rgb(240, 240, 240),
            }),
            ..FramePlan::default()
        };

        frame.refresh_stats(1, 3);

        assert_eq!(frame.stats.rect_vertices, 18);
        assert_eq!(frame.stats.rect_vertex_capacity, 32);
    }

    #[test]
    fn multi_line_selection_extends_middle_rows_to_line_end() {
        let mut snapshot = RenderSnapshot::from_plain_lines(&["abcde", "fghij", "klmno"]);
        snapshot.selection = Some(CellRange {
            start: CellPoint::new(0, 2),
            end: CellPoint::new(2, 1),
        });
        let planner = FramePlanner::new(CellMetrics::default());
        let frame = planner.plan(&snapshot);

        assert_eq!(frame.selection.len(), 3);
        assert_eq!(
            frame.selection[0].origin,
            planner.cell_origin(CellPoint::new(0, 2))
        );
        assert_eq!(
            frame.selection[0].size.width,
            3.0 * planner.metrics.cell.width
        );
        assert_eq!(
            frame.selection[1].origin,
            planner.cell_origin(CellPoint::new(1, 0))
        );
        assert_eq!(
            frame.selection[1].size.width,
            5.0 * planner.metrics.cell.width
        );
        assert_eq!(
            frame.selection[2].origin,
            planner.cell_origin(CellPoint::new(2, 0))
        );
        assert_eq!(
            frame.selection[2].size.width,
            2.0 * planner.metrics.cell.width
        );
    }

    fn glyph_item(x: f32, y: f32, text: &str, color: Rgba) -> GlyphBatchItem {
        GlyphBatchItem {
            origin: PixelPoint { x, y },
            text: text.to_owned(),
            color,
            style_flags: CellFlags::default(),
        }
    }
}

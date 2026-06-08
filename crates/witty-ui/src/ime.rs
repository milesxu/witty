use witty_core::{terminal_char_width, CellFlags, CellPoint, CursorState, GridSize, Rgba};
use witty_render_wgpu::{
    CellMetrics, FramePlan, GlyphBatchItem, PixelPoint, PixelSize, RectBatchItem,
};

const PREEDIT_BACKGROUND: Rgba = Rgba::with_alpha(54, 89, 140, 150);
const PREEDIT_UNDERLINE: Rgba = Rgba::rgb(142, 184, 255);
const PREEDIT_TEXT: Rgba = Rgba::WHITE;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImeComposition {
    enabled: bool,
    preedit: String,
    caret: Option<(usize, usize)>,
}

impl ImeComposition {
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
        self.clear_preedit();
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn is_active(&self) -> bool {
        !self.preedit.is_empty()
    }

    pub fn preedit(&self) -> &str {
        &self.preedit
    }

    pub fn caret(&self) -> Option<(usize, usize)> {
        self.caret
    }

    pub fn set_preedit(&mut self, text: impl Into<String>, caret: Option<(usize, usize)>) {
        self.enabled = true;
        self.preedit = text.into();
        self.caret = normalize_caret(self.preedit.len(), caret);
    }

    pub fn clear_preedit(&mut self) {
        self.preedit.clear();
        self.caret = None;
    }

    pub fn commit_text(&mut self, text: impl Into<String>) -> Option<String> {
        self.clear_preedit();
        let text = text.into();
        (!text.is_empty()).then_some(text)
    }

    pub fn preedit_cell_width(&self) -> u16 {
        self.preedit
            .chars()
            .map(terminal_char_width)
            .map(u16::from)
            .sum()
    }

    pub fn preedit_caret_cell_width(&self) -> u16 {
        let Some((caret_start, _)) = self.caret else {
            return self.preedit_cell_width();
        };

        text_prefix_cell_width(&self.preedit, caret_start)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImePreeditOverlay {
    pub background: RectBatchItem,
    pub underline: RectBatchItem,
    pub glyph: GlyphBatchItem,
}

pub fn ime_preedit_overlay(
    composition: &ImeComposition,
    cursor: CursorState,
    metrics: CellMetrics,
    grid_size: GridSize,
) -> Option<ImePreeditOverlay> {
    if !composition.is_active() || grid_size.rows == 0 || grid_size.cols == 0 {
        return None;
    }

    let point = CellPoint::new(
        cursor.position.row.min(grid_size.rows.saturating_sub(1)),
        cursor.position.col.min(grid_size.cols.saturating_sub(1)),
    );
    let origin = cell_origin(point, metrics);
    let remaining_cols = grid_size.cols.saturating_sub(point.col).max(1);
    let preedit_cols = composition.preedit_cell_width().max(1).min(remaining_cols);
    let width = f32::from(preedit_cols) * metrics.cell.width;
    let underline_height = (metrics.cell.height * 0.12).clamp(1.0, metrics.cell.height);

    Some(ImePreeditOverlay {
        background: RectBatchItem {
            origin,
            size: PixelSize {
                width,
                height: metrics.cell.height,
            },
            color: PREEDIT_BACKGROUND,
        },
        underline: RectBatchItem {
            origin: PixelPoint {
                x: origin.x,
                y: origin.y + metrics.cell.height - underline_height,
            },
            size: PixelSize {
                width,
                height: underline_height,
            },
            color: PREEDIT_UNDERLINE,
        },
        glyph: GlyphBatchItem {
            origin,
            text: composition.preedit().to_owned(),
            color: PREEDIT_TEXT,
            style_flags: CellFlags::default(),
        },
    })
}

pub fn apply_ime_preedit_overlay(
    frame: &mut FramePlan,
    composition: &ImeComposition,
    cursor: CursorState,
    metrics: CellMetrics,
    grid_size: GridSize,
) -> bool {
    let Some(overlay) = ime_preedit_overlay(composition, cursor, metrics, grid_size) else {
        return false;
    };

    frame.cursor = None;
    frame.ime_preedit.push(overlay.background);
    frame.ime_preedit.push(overlay.underline);
    frame.glyphs.push(overlay.glyph);
    true
}

fn cell_origin(point: CellPoint, metrics: CellMetrics) -> PixelPoint {
    PixelPoint {
        x: metrics.padding.x + f32::from(point.col) * metrics.cell.width,
        y: metrics.padding.y + f32::from(point.row) * metrics.cell.height,
    }
}

fn normalize_caret(text_len: usize, caret: Option<(usize, usize)>) -> Option<(usize, usize)> {
    if text_len == 0 {
        return None;
    }

    let (start, end) = caret?;
    let start = start.min(text_len);
    let end = end.min(text_len);
    if start <= end {
        Some((start, end))
    } else {
        Some((end, start))
    }
}

fn text_prefix_cell_width(text: &str, byte_index: usize) -> u16 {
    let mut end = byte_index.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }

    text[..end].chars().fold(0u16, |width, ch| {
        width.saturating_add(u16::from(terminal_char_width(ch)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composition_state_normalizes_caret_and_clears_on_disable() {
        let mut composition = ImeComposition::default();

        composition.set_preedit("nihao", Some((9, 2)));

        assert!(composition.is_enabled());
        assert!(composition.is_active());
        assert_eq!(composition.preedit(), "nihao");
        assert_eq!(composition.caret(), Some((2, 5)));

        composition.disable();

        assert!(!composition.is_enabled());
        assert!(!composition.is_active());
        assert_eq!(composition.preedit(), "");
        assert_eq!(composition.caret(), None);
    }

    #[test]
    fn commit_text_clears_preedit_and_skips_empty_commits() {
        let mut composition = ImeComposition::default();
        composition.set_preedit("ni", Some((2, 2)));

        assert_eq!(composition.commit_text("你"), Some("你".to_owned()));
        assert!(composition.is_enabled());
        assert!(!composition.is_active());
        assert_eq!(composition.caret(), None);
        assert_eq!(composition.commit_text(""), None);
    }

    #[test]
    fn preedit_caret_cell_width_tracks_wide_text_and_char_boundaries() {
        let mut composition = ImeComposition::default();
        composition.set_preedit("a你b", Some((1, 1)));
        assert_eq!(composition.preedit_caret_cell_width(), 1);

        composition.set_preedit("a你b", Some(("a你".len(), "a你".len())));
        assert_eq!(composition.preedit_caret_cell_width(), 3);

        composition.set_preedit("a你b", Some((2, 2)));
        assert_eq!(composition.preedit_caret_cell_width(), 1);

        composition.set_preedit("a你b", None);
        assert_eq!(composition.preedit_caret_cell_width(), 4);
    }

    #[test]
    fn preedit_overlay_uses_cursor_cell_and_hides_frame_cursor() {
        let mut composition = ImeComposition::default();
        composition.set_preedit("你a", Some((0, 0)));
        let metrics = CellMetrics::default();
        let cursor = CursorState {
            position: CellPoint::new(2, 3),
            ..CursorState::default()
        };
        let mut frame = FramePlan {
            cursor: Some(RectBatchItem {
                origin: PixelPoint { x: 0.0, y: 0.0 },
                size: metrics.cell,
                color: Rgba::WHITE,
            }),
            ..FramePlan::default()
        };

        assert!(apply_ime_preedit_overlay(
            &mut frame,
            &composition,
            cursor,
            metrics,
            GridSize::new(5, 10)
        ));

        assert!(frame.cursor.is_none());
        assert_eq!(frame.ime_preedit.len(), 2);
        assert_eq!(frame.glyphs.last().unwrap().text, "你a");
        assert_eq!(frame.ime_preedit[0].origin, PixelPoint { x: 35.0, y: 44.0 });
        assert_eq!(frame.ime_preedit[0].size.width, 27.0);
    }

    #[test]
    fn preedit_overlay_clamps_to_grid_width() {
        let mut composition = ImeComposition::default();
        composition.set_preedit("abcdef", None);
        let overlay = ime_preedit_overlay(
            &composition,
            CursorState {
                position: CellPoint::new(0, 3),
                ..CursorState::default()
            },
            CellMetrics::default(),
            GridSize::new(1, 5),
        )
        .unwrap();

        assert_eq!(overlay.background.size.width, 18.0);
    }
}

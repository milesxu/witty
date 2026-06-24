use std::collections::{BTreeMap, BTreeSet, VecDeque};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use vte::{Params, Parser, Perform};

use crate::{
    find_search_matches, grapheme_cluster_spans, terminal_char_width, BaselineShift, CellFlags,
    CellPoint, CellRange, CellStyle, CursorShape, CursorState, DamageRegion, GridSize, HyperlinkId,
    MouseEncodingMode, MouseTrackingMode, RenderCell, RenderRow, RenderSnapshot, Rgba,
    SearchHighlight, SearchMatch, SearchOptions, SearchRowId, SearchRowKind, SearchTextColumn,
    SearchTextRow, TerminalClipboardSelection, TerminalClipboardWrite, TerminalCurrentDirectory,
    TerminalHostAction, TerminalHostReply, TerminalHyperlink, TerminalInputModes,
    TerminalMouseModes, TerminalPointAnchor, TerminalRowAnchor, TerminalScreen,
    TerminalShellIntegrationEvent, TerminalShellIntegrationMarker, TerminalTextRange,
    TerminalVisibleRowAnchor, UnderlineStyle, DEFAULT_MAX_SCROLLBACK_LINES,
    KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES, KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES,
    KITTY_KEYBOARD_REPORT_ASSOCIATED_TEXT, MAX_OSC52_DECODED_BYTES,
};

const MAX_OSC7_URI_BYTES: usize = 4096;
const MAX_OSC8_URI_BYTES: usize = 2048;
const MAX_OSC8_ID_BYTES: usize = 256;
const MAX_OSC52_ENCODED_BYTES: usize = MAX_OSC52_DECODED_BYTES.div_ceil(3) * 4;
const MAX_DCS_REQUEST_BYTES: usize = 64;
const MAX_XTGETTCAP_REQUEST_BYTES: usize = 512;
const DEFAULT_CURSOR_COLOR: Rgba = Rgba::rgb(180, 180, 180);
const CURSOR_KEY_APPLICATION_MODE: u16 = 1;
const KEYBOARD_ACTION_MODE: u16 = 2;
const INSERT_MODE: u16 = 4;
const LINE_FEED_NEW_LINE_MODE: u16 = 20;
const REVERSE_VIDEO_MODE: u16 = 5;
const ORIGIN_MODE: u16 = 6;
const AUTOWRAP_MODE: u16 = 7;
const X10_MOUSE_MODE: u16 = 9;
const CURSOR_BLINK_MODE: u16 = 12;
const BRACKETED_PASTE_MODE: u16 = 2004;
const SYNCHRONIZED_OUTPUT_MODE: u16 = 2026;
const CURSOR_VISIBLE_MODE: u16 = 25;
const REVERSE_WRAPAROUND_MODE: u16 = 45;
const LEGACY_ALTERNATE_SCREEN_MODE: u16 = 47;
const APPLICATION_KEYPAD_MODE: u16 = 66;
const BACKARROW_KEY_MODE: u16 = 67;
const NORMAL_MOUSE_MODE: u16 = 1000;
const BUTTON_EVENT_MOUSE_MODE: u16 = 1002;
const ANY_EVENT_MOUSE_MODE: u16 = 1003;
const FOCUS_EVENT_MODE: u16 = 1004;
const UTF8_MOUSE_MODE: u16 = 1005;
const SGR_MOUSE_MODE: u16 = 1006;
const ALTERNATE_SCROLL_MODE: u16 = 1007;
const URXVT_MOUSE_MODE: u16 = 1015;
const ALTERNATE_SCREEN_MODE: u16 = 1047;
const SAVE_CURSOR_MODE: u16 = 1048;
const ALTERNATE_SCREEN_WITH_CURSOR_MODE: u16 = 1049;
const SGR_PIXEL_MOUSE_MODE: u16 = 1016;
const DEVICE_STATUS_OK: u16 = 5;
const CURSOR_POSITION_REPORT: u16 = 6;
const REPORT_TEXT_AREA_SIZE_CHARS: u16 = 18;
const REPORT_SCREEN_SIZE_CHARS: u16 = 19;
const SUPPORTED_KITTY_KEYBOARD_FLAGS: u16 =
    KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES
        | KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES
        | KITTY_KEYBOARD_REPORT_ASSOCIATED_TEXT;
const MAX_KITTY_KEYBOARD_STACK_DEPTH: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalColorTheme {
    pub foreground: Rgba,
    pub background: Rgba,
    pub cursor_color: Option<Rgba>,
    pub palette: [Rgba; Self::ANSI_COLOR_COUNT],
}

impl TerminalColorTheme {
    pub const ANSI_COLOR_COUNT: usize = 16;

    pub fn builtin_palette() -> [Rgba; Self::ANSI_COLOR_COUNT] {
        std::array::from_fn(|index| default_ansi_color(index as u8))
    }
}

impl Default for TerminalColorTheme {
    fn default() -> Self {
        Self {
            foreground: CellStyle::default().foreground,
            background: CellStyle::default().background,
            cursor_color: None,
            palette: Self::builtin_palette(),
        }
    }
}

pub fn parse_terminal_color(text: &str) -> Option<Rgba> {
    osc_color(text.as_bytes())
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum CharsetIndex {
    #[default]
    G0,
    G1,
    G2,
    G3,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum StandardCharset {
    #[default]
    Ascii,
    DecSpecialGraphics,
    UkNational,
}

pub struct BasicTerminal {
    parser: Parser,
    c1_aliases: C1AliasNormalizer,
    state: BasicTerminalState,
}

impl BasicTerminal {
    pub fn new(size: GridSize) -> Self {
        Self {
            parser: Parser::new(),
            c1_aliases: C1AliasNormalizer::default(),
            state: BasicTerminalState::new(size),
        }
    }

    pub fn with_scrollback_limit(size: GridSize, max_scrollback_lines: usize) -> Self {
        let mut terminal = Self::new(size);
        terminal.set_max_scrollback_lines(max_scrollback_lines);
        terminal
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        if let Some(normalized) = self.c1_aliases.normalize(bytes) {
            self.parser.advance(&mut self.state, &normalized);
        } else {
            self.parser.advance(&mut self.state, bytes);
        }
    }

    pub fn snapshot(&self) -> RenderSnapshot {
        self.state.snapshot()
    }

    pub fn take_snapshot(&mut self) -> RenderSnapshot {
        self.state.take_snapshot()
    }

    pub fn resize(&mut self, size: GridSize) {
        self.state.resize(size);
    }

    pub fn set_selection(&mut self, selection: Option<CellRange>) {
        self.state.set_selection(selection);
    }

    pub fn selected_text(&self) -> Option<String> {
        self.state.selected_text()
    }

    pub fn text_for_range(&self, range: TerminalTextRange) -> Option<String> {
        self.state.text_for_range(range)
    }

    pub fn word_range_at(&self, point: CellPoint) -> Option<CellRange> {
        self.state.word_range_at(point)
    }

    pub fn search_text_rows(&self) -> Vec<SearchTextRow> {
        self.state.search_text_rows()
    }

    pub fn find_matches(&self, query: &str, options: SearchOptions) -> Vec<SearchMatch> {
        find_search_matches(&self.search_text_rows(), query, options)
    }

    pub fn visible_search_matches(&self, matches: &[SearchMatch]) -> Vec<CellRange> {
        self.state.visible_search_matches(matches)
    }

    pub fn visible_search_highlights(
        &self,
        matches: &[SearchMatch],
        active: Option<SearchMatch>,
    ) -> Vec<SearchHighlight> {
        self.state.visible_search_highlights(matches, active)
    }

    pub fn scroll_to_search_match(&mut self, row: SearchRowId, buffer_rows: u16) -> bool {
        self.state.scroll_to_search_match(row, buffer_rows)
    }

    pub fn scroll_viewport_lines(&mut self, lines: i16) {
        self.state.scroll_viewport_lines(lines);
    }

    pub fn viewport_offset(&self) -> usize {
        self.state.viewport_offset
    }

    pub fn active_screen(&self) -> TerminalScreen {
        self.state.terminal_screen()
    }

    pub fn max_scrollback_lines(&self) -> usize {
        self.state.max_scrollback_lines
    }

    pub fn scrollback_line_count(&self) -> usize {
        self.state.scrollback.len()
    }

    pub fn set_max_scrollback_lines(&mut self, max_scrollback_lines: usize) {
        self.state.set_max_scrollback_lines(max_scrollback_lines);
    }

    pub fn set_default_cursor_style(&mut self, shape: CursorShape, blink: bool) {
        self.state.set_default_cursor_style(shape, blink);
    }

    pub fn default_cursor_style(&self) -> (CursorShape, bool) {
        self.state.default_cursor_style()
    }

    pub fn set_color_theme(&mut self, theme: TerminalColorTheme) {
        self.state.set_color_theme(theme);
    }

    pub fn color_theme(&self) -> TerminalColorTheme {
        self.state.color_theme()
    }

    pub fn visible_row_anchors(&self) -> Vec<TerminalVisibleRowAnchor> {
        self.state.visible_row_anchors()
    }

    pub fn bracketed_paste_enabled(&self) -> bool {
        self.state.bracketed_paste
    }

    pub fn synchronized_output_enabled(&self) -> bool {
        self.state.synchronized_output
    }

    pub fn application_keypad_enabled(&self) -> bool {
        self.state.application_keypad
    }

    pub fn application_cursor_keys_enabled(&self) -> bool {
        self.state.application_cursor_keys
    }

    pub fn input_modes(&self) -> TerminalInputModes {
        self.state.input_modes()
    }

    pub fn mouse_modes(&self) -> TerminalMouseModes {
        self.state.mouse_modes()
    }

    pub fn title(&self) -> Option<&str> {
        self.state.title.as_deref()
    }

    pub fn drain_host_actions(&mut self) -> Vec<TerminalHostAction> {
        self.state.drain_host_actions()
    }
}

#[derive(Clone, Copy, Debug)]
struct C1AliasNormalizer {
    utf8_remaining: u8,
    utf8_min: u8,
    utf8_max: u8,
    pending_c2: bool,
}

impl Default for C1AliasNormalizer {
    fn default() -> Self {
        Self {
            utf8_remaining: 0,
            utf8_min: 0x80,
            utf8_max: 0xbf,
            pending_c2: false,
        }
    }
}

impl C1AliasNormalizer {
    fn normalize(&mut self, bytes: &[u8]) -> Option<Vec<u8>> {
        let mut normalized = None;
        let mut passthrough_start = 0;

        for (index, byte) in bytes.iter().copied().enumerate() {
            let Some(replacement) = self.replacement_for(byte) else {
                continue;
            };

            let output = normalized.get_or_insert_with(|| Vec::with_capacity(bytes.len() + 2));
            output.extend_from_slice(&bytes[passthrough_start..index]);
            output.extend_from_slice(&replacement);
            passthrough_start = index + 1;
        }

        if let Some(output) = &mut normalized {
            output.extend_from_slice(&bytes[passthrough_start..]);
        }

        normalized
    }

    fn replacement_for(&mut self, byte: u8) -> Option<Vec<u8>> {
        if self.pending_c2 {
            self.pending_c2 = false;
            if (0x80..=0x9f).contains(&byte) {
                if let Some(replacement) = c1_encoded_control_alias(byte) {
                    return Some(replacement.to_vec());
                }
            }
            if (0x80..=0xbf).contains(&byte) {
                return Some(vec![0xc2, byte]);
            }
            return Some(vec![0xc2, byte]);
        }

        loop {
            if self.utf8_remaining == 0 {
                break;
            }

            if (self.utf8_min..=self.utf8_max).contains(&byte) {
                self.utf8_remaining -= 1;
                if self.utf8_remaining == 0 {
                    self.clear_utf8_state();
                } else {
                    self.utf8_min = 0x80;
                    self.utf8_max = 0xbf;
                }
                return None;
            }

            self.clear_utf8_state();
        }

        match byte {
            0xc2 => {
                self.pending_c2 = true;
                return Some(Vec::new());
            }
            0xc3..=0xdf => self.expect_utf8_continuations(1, 0x80, 0xbf),
            0xe0 => self.expect_utf8_continuations(2, 0xa0, 0xbf),
            0xe1..=0xec | 0xee..=0xef => {
                self.expect_utf8_continuations(2, 0x80, 0xbf);
            }
            0xed => self.expect_utf8_continuations(2, 0x80, 0x9f),
            0xf0 => self.expect_utf8_continuations(3, 0x90, 0xbf),
            0xf1..=0xf3 => self.expect_utf8_continuations(3, 0x80, 0xbf),
            0xf4 => self.expect_utf8_continuations(3, 0x80, 0x8f),
            _ => {}
        }

        c1_sequence_alias(byte).map(|replacement| replacement.to_vec())
    }

    fn expect_utf8_continuations(&mut self, remaining: u8, min: u8, max: u8) {
        self.utf8_remaining = remaining;
        self.utf8_min = min;
        self.utf8_max = max;
    }

    fn clear_utf8_state(&mut self) {
        self.utf8_remaining = 0;
        self.utf8_min = 0x80;
        self.utf8_max = 0xbf;
    }
}

fn c1_encoded_control_alias(byte: u8) -> Option<&'static [u8]> {
    match byte {
        0x84 => Some(b"\x84"),
        0x85 => Some(b"\x85"),
        0x88 => Some(b"\x88"),
        0x8d => Some(b"\x8d"),
        0x8e => Some(b"\x8e"),
        0x8f => Some(b"\x8f"),
        0x96 => Some(b"\x96"),
        0x97 => Some(b"\x97"),
        0x9a => Some(b"\x9a"),
        _ => c1_sequence_alias(byte),
    }
}

fn c1_sequence_alias(byte: u8) -> Option<&'static [u8]> {
    match byte {
        0x90 => Some(b"\x1bP"),
        0x98 => Some(b"\x1bX"),
        0x9b => Some(b"\x1b["),
        0x9c => Some(b"\x1b\\"),
        0x9d => Some(b"\x1b]"),
        0x9e => Some(b"\x1b^"),
        0x9f => Some(b"\x1b_"),
        _ => None,
    }
}

#[derive(Clone, Debug)]
struct BasicCell {
    text: String,
    width: u8,
    style: BasicCellStyle,
    hyperlink: Option<HyperlinkId>,
    protected: bool,
}

impl Default for BasicCell {
    fn default() -> Self {
        Self {
            text: " ".to_owned(),
            width: 1,
            style: BasicCellStyle::default(),
            hyperlink: None,
            protected: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CellColor {
    DefaultForeground,
    DefaultBackground,
    Indexed(u8),
    Direct(Rgba),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BasicCellStyle {
    foreground: CellColor,
    background: CellColor,
    underline_color: Option<CellColor>,
    flags: CellFlags,
}

impl Default for BasicCellStyle {
    fn default() -> Self {
        Self {
            foreground: CellColor::DefaultForeground,
            background: CellColor::DefaultBackground,
            underline_color: None,
            flags: CellFlags::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DcsRequestKind {
    RequestStatusString,
    TermcapQuery,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DcsRequest {
    kind: DcsRequestKind,
    payload: Vec<u8>,
    overflowed: bool,
}

impl DcsRequest {
    fn new(kind: DcsRequestKind) -> Self {
        Self {
            kind,
            payload: Vec::new(),
            overflowed: false,
        }
    }

    fn push(&mut self, byte: u8) {
        if self.payload.len() >= self.max_payload_len() {
            self.overflowed = true;
            return;
        }
        self.payload.push(byte);
    }

    fn max_payload_len(&self) -> usize {
        match self.kind {
            DcsRequestKind::RequestStatusString => MAX_DCS_REQUEST_BYTES,
            DcsRequestKind::TermcapQuery => MAX_XTGETTCAP_REQUEST_BYTES,
        }
    }
}

#[derive(Clone, Debug)]
struct ScreenBuffer {
    cells: Vec<Vec<BasicCell>>,
    row_anchors: Vec<u64>,
    cursor: CursorState,
}

impl ScreenBuffer {
    fn new(size: GridSize, next_row_anchor: &mut u64) -> Self {
        Self {
            cells: blank_cells(size),
            row_anchors: allocate_row_anchors(size.rows, next_row_anchor),
            cursor: CursorState::default(),
        }
    }

    fn reset(&mut self, size: GridSize, cursor: CursorState, next_row_anchor: &mut u64) {
        self.cells = blank_cells(size);
        self.row_anchors = allocate_row_anchors(size.rows, next_row_anchor);
        self.cursor = cursor;
    }

    fn resize(&mut self, size: GridSize, next_row_anchor: &mut u64) {
        let old = std::mem::replace(&mut self.cells, blank_cells(size));
        let old_anchors = std::mem::replace(
            &mut self.row_anchors,
            allocate_row_anchors(size.rows, next_row_anchor),
        );
        for (row_index, row) in old.into_iter().enumerate().take(size.rows as usize) {
            for (col_index, cell) in row.into_iter().enumerate().take(size.cols as usize) {
                self.cells[row_index][col_index] = cell;
            }
        }
        for (row_index, anchor) in old_anchors.into_iter().enumerate().take(size.rows as usize) {
            self.row_anchors[row_index] = anchor;
        }
        for row in &mut self.cells {
            repair_wide_cells(row, BasicCell::default());
        }
        self.cursor.position.row = self.cursor.position.row.min(size.rows.saturating_sub(1));
        self.cursor.position.col = self.cursor.position.col.min(size.cols.saturating_sub(1));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ActiveScreen {
    Main,
    Alternate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SavedCursor {
    screen: ActiveScreen,
    position: CellPoint,
    origin_mode: bool,
    autowrap: bool,
    pending_wrap: bool,
    active_charset: CharsetIndex,
    charsets: [StandardCharset; 4],
    current_style: BasicCellStyle,
    current_protected: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ScrollRegion {
    top: u16,
    bottom: u16,
}

#[derive(Clone, Debug)]
struct BasicTerminalState {
    size: GridSize,
    main: ScreenBuffer,
    alternate: ScreenBuffer,
    active_screen: ActiveScreen,
    saved_main_cursor: Option<CellPoint>,
    saved_cursor: Option<SavedCursor>,
    scroll_region: Option<ScrollRegion>,
    origin_mode: bool,
    autowrap: bool,
    insert_mode: bool,
    linefeed_newline: bool,
    keyboard_locked: bool,
    reverse_video: bool,
    reverse_wraparound: bool,
    application_keypad: bool,
    application_cursor_keys: bool,
    backarrow_sends_backspace: bool,
    main_kitty_keyboard_flags: u16,
    alternate_kitty_keyboard_flags: u16,
    main_kitty_keyboard_stack: Vec<u16>,
    alternate_kitty_keyboard_stack: Vec<u16>,
    mouse_tracking: MouseTrackingMode,
    mouse_utf8_encoding: bool,
    mouse_urxvt_encoding: bool,
    mouse_sgr_encoding: bool,
    mouse_sgr_pixel_encoding: bool,
    mouse_focus_events: bool,
    mouse_alternate_scroll: bool,
    synchronized_output: bool,
    pending_wrap: bool,
    last_printed_graphic: Option<char>,
    current_protected: bool,
    active_charset: CharsetIndex,
    single_shift_charset: Option<CharsetIndex>,
    charsets: [StandardCharset; 4],
    tab_stops: BTreeSet<u16>,
    current_style: BasicCellStyle,
    color_theme: TerminalColorTheme,
    palette_overrides: BTreeMap<u8, Rgba>,
    default_foreground: Rgba,
    default_background: Rgba,
    default_cursor_shape: CursorShape,
    default_cursor_blink: bool,
    cursor_color: Option<Rgba>,
    scrollback: VecDeque<Vec<BasicCell>>,
    scrollback_row_anchors: VecDeque<u64>,
    next_main_row_anchor: u64,
    next_alternate_row_anchor: u64,
    max_scrollback_lines: usize,
    viewport_offset: usize,
    selection: Option<CellRange>,
    title: Option<String>,
    hyperlinks: Vec<TerminalHyperlink>,
    active_hyperlink: Option<HyperlinkId>,
    next_hyperlink_id: HyperlinkId,
    pending_host_actions: Vec<TerminalHostAction>,
    dcs_request: Option<DcsRequest>,
    bracketed_paste: bool,
    dirty_rows: BTreeSet<u16>,
    full_damage: bool,
    damage_cleared_once: bool,
}

impl BasicTerminalState {
    fn new(size: GridSize) -> Self {
        let mut next_main_row_anchor = 0;
        let mut next_alternate_row_anchor = 0;
        let color_theme = TerminalColorTheme::default();
        Self {
            size,
            main: ScreenBuffer::new(size, &mut next_main_row_anchor),
            alternate: ScreenBuffer::new(size, &mut next_alternate_row_anchor),
            active_screen: ActiveScreen::Main,
            saved_main_cursor: None,
            saved_cursor: None,
            scroll_region: None,
            origin_mode: false,
            autowrap: true,
            insert_mode: false,
            linefeed_newline: false,
            keyboard_locked: false,
            reverse_video: false,
            reverse_wraparound: false,
            application_keypad: false,
            application_cursor_keys: false,
            backarrow_sends_backspace: false,
            main_kitty_keyboard_flags: 0,
            alternate_kitty_keyboard_flags: 0,
            main_kitty_keyboard_stack: Vec::new(),
            alternate_kitty_keyboard_stack: Vec::new(),
            mouse_tracking: MouseTrackingMode::None,
            mouse_utf8_encoding: false,
            mouse_urxvt_encoding: false,
            mouse_sgr_encoding: false,
            mouse_sgr_pixel_encoding: false,
            mouse_focus_events: false,
            mouse_alternate_scroll: false,
            synchronized_output: false,
            pending_wrap: false,
            last_printed_graphic: None,
            current_protected: false,
            active_charset: CharsetIndex::G0,
            single_shift_charset: None,
            charsets: [StandardCharset::Ascii; 4],
            tab_stops: default_tab_stops(size),
            current_style: BasicCellStyle::default(),
            color_theme,
            palette_overrides: BTreeMap::new(),
            default_foreground: color_theme.foreground,
            default_background: color_theme.background,
            default_cursor_shape: CursorShape::Block,
            default_cursor_blink: true,
            cursor_color: color_theme.cursor_color,
            scrollback: VecDeque::new(),
            scrollback_row_anchors: VecDeque::new(),
            next_main_row_anchor,
            next_alternate_row_anchor,
            max_scrollback_lines: DEFAULT_MAX_SCROLLBACK_LINES,
            viewport_offset: 0,
            selection: None,
            title: None,
            hyperlinks: Vec::new(),
            active_hyperlink: None,
            next_hyperlink_id: 1,
            pending_host_actions: Vec::new(),
            dcs_request: None,
            bracketed_paste: false,
            dirty_rows: BTreeSet::new(),
            full_damage: false,
            damage_cleared_once: false,
        }
    }

    fn resize(&mut self, size: GridSize) {
        self.main.resize(size, &mut self.next_main_row_anchor);
        self.alternate
            .resize(size, &mut self.next_alternate_row_anchor);
        self.size = size;
        self.scroll_region = None;
        self.pending_wrap = false;
        self.tab_stops.retain(|col| *col < size.cols);
        self.viewport_offset = self.viewport_offset.min(self.scrollback.len());
        self.mark_full_damage();
    }

    fn snapshot(&self) -> RenderSnapshot {
        let visible_rows = self.visible_rows();
        let rows = visible_rows
            .iter()
            .enumerate()
            .map(|(row_index, row)| RenderRow {
                row: row_index as u16,
                cells: row
                    .iter()
                    .enumerate()
                    .filter_map(|(col_index, cell)| {
                        if cell.width == 0 {
                            return None;
                        }
                        Some(RenderCell {
                            point: CellPoint::new(row_index as u16, col_index as u16),
                            text: cell.text.clone(),
                            width: cell.width,
                            style: self.resolve_style(&cell.style),
                            hyperlink: cell.hyperlink,
                        })
                    })
                    .collect(),
            })
            .collect();
        let hyperlinks = visible_hyperlinks(&visible_rows, &self.hyperlinks);

        let mut cursor = self.cursor();
        if self.viewport_offset > 0 {
            cursor.visible = false;
        }

        RenderSnapshot {
            size: self.size,
            rows,
            default_background: self.visible_default_background(),
            cursor,
            cursor_color: self.cursor_color,
            selection: self.selection,
            search_highlights: Vec::new(),
            damage: self.damage_region(),
            title: self.title.clone(),
            hyperlinks,
            hovered_hyperlink: None,
        }
    }

    fn take_snapshot(&mut self) -> RenderSnapshot {
        let snapshot = self.snapshot();
        self.clear_damage();
        snapshot
    }

    fn drain_host_actions(&mut self) -> Vec<TerminalHostAction> {
        std::mem::take(&mut self.pending_host_actions)
    }

    fn push_terminal_reply(&mut self, bytes: impl Into<Vec<u8>>) {
        self.pending_host_actions
            .push(TerminalHostAction::TerminalReply(TerminalHostReply {
                bytes: bytes.into(),
            }));
    }

    fn ring_bell(&mut self) {
        self.pending_host_actions.push(TerminalHostAction::Bell);
    }

    fn active_buffer(&self) -> &ScreenBuffer {
        match self.active_screen {
            ActiveScreen::Main => &self.main,
            ActiveScreen::Alternate => &self.alternate,
        }
    }

    fn active_buffer_mut(&mut self) -> &mut ScreenBuffer {
        match self.active_screen {
            ActiveScreen::Main => &mut self.main,
            ActiveScreen::Alternate => &mut self.alternate,
        }
    }

    fn cursor(&self) -> CursorState {
        self.active_buffer().cursor
    }

    fn input_modes(&self) -> TerminalInputModes {
        TerminalInputModes {
            application_cursor_keys: self.application_cursor_keys,
            application_keypad: self.application_keypad,
            keyboard_locked: self.keyboard_locked,
            backarrow_sends_backspace: self.backarrow_sends_backspace,
            kitty_keyboard_flags: self.active_kitty_keyboard_flags(),
            mouse: self.mouse_modes(),
        }
    }

    fn active_kitty_keyboard_flags(&self) -> u16 {
        match self.active_screen {
            ActiveScreen::Main => self.main_kitty_keyboard_flags,
            ActiveScreen::Alternate => self.alternate_kitty_keyboard_flags,
        }
    }

    fn set_active_kitty_keyboard_flags(&mut self, flags: u16) {
        let flags = flags & SUPPORTED_KITTY_KEYBOARD_FLAGS;
        match self.active_screen {
            ActiveScreen::Main => self.main_kitty_keyboard_flags = flags,
            ActiveScreen::Alternate => self.alternate_kitty_keyboard_flags = flags,
        }
    }

    fn active_kitty_keyboard_stack_mut(&mut self) -> &mut Vec<u16> {
        match self.active_screen {
            ActiveScreen::Main => &mut self.main_kitty_keyboard_stack,
            ActiveScreen::Alternate => &mut self.alternate_kitty_keyboard_stack,
        }
    }

    fn mouse_modes(&self) -> TerminalMouseModes {
        TerminalMouseModes {
            tracking: self.mouse_tracking,
            encoding: self.mouse_encoding_mode(),
            focus_events: self.mouse_focus_events,
            alternate_scroll: self.mouse_alternate_scroll,
        }
    }

    fn mouse_encoding_mode(&self) -> MouseEncodingMode {
        if self.mouse_sgr_pixel_encoding {
            MouseEncodingMode::SgrPixels
        } else if self.mouse_sgr_encoding {
            MouseEncodingMode::Sgr
        } else if self.mouse_urxvt_encoding {
            MouseEncodingMode::Urxvt
        } else if self.mouse_utf8_encoding {
            MouseEncodingMode::Utf8
        } else {
            MouseEncodingMode::X10
        }
    }

    fn cursor_mut(&mut self) -> &mut CursorState {
        &mut self.active_buffer_mut().cursor
    }

    fn cells(&self) -> &[Vec<BasicCell>] {
        &self.active_buffer().cells
    }

    fn cells_mut(&mut self) -> &mut [Vec<BasicCell>] {
        &mut self.active_buffer_mut().cells
    }

    fn row_anchor_for_point(&self, point: CellPoint) -> Option<TerminalPointAnchor> {
        if point.row >= self.size.rows || point.col >= self.size.cols {
            return None;
        }
        let row_anchor = self.active_buffer().row_anchors.get(point.row as usize)?;
        Some(TerminalPointAnchor {
            row: TerminalRowAnchor {
                screen: self.terminal_screen(),
                row: *row_anchor,
            },
            col: point.col,
        })
    }

    fn active_row_anchor_at(&self, row: usize) -> Option<u64> {
        self.active_buffer().row_anchors.get(row).copied()
    }

    fn visible_row_anchors(&self) -> Vec<TerminalVisibleRowAnchor> {
        if self.active_screen == ActiveScreen::Alternate {
            return self
                .alternate
                .row_anchors
                .iter()
                .enumerate()
                .filter_map(|(visible_row, anchor)| {
                    Some(TerminalVisibleRowAnchor {
                        visible_row: u16::try_from(visible_row).ok()?,
                        anchor: TerminalRowAnchor {
                            screen: TerminalScreen::Alternate,
                            row: *anchor,
                        },
                    })
                })
                .collect();
        }

        let (start, end) = self.visible_logical_row_window();
        (start..end)
            .enumerate()
            .filter_map(|(visible_row, logical_index)| {
                Some(TerminalVisibleRowAnchor {
                    visible_row: u16::try_from(visible_row).ok()?,
                    anchor: TerminalRowAnchor {
                        screen: TerminalScreen::Main,
                        row: self.logical_row_anchor(logical_index)?,
                    },
                })
            })
            .collect()
    }

    fn logical_row_anchor(&self, logical_index: usize) -> Option<u64> {
        let scrollback_len = self.scrollback_row_anchors.len();
        if logical_index < scrollback_len {
            self.scrollback_row_anchors.get(logical_index).copied()
        } else {
            self.main
                .row_anchors
                .get(logical_index.saturating_sub(scrollback_len))
                .copied()
        }
    }

    fn row_index_for_anchor(&self, anchor: TerminalRowAnchor) -> Option<usize> {
        match anchor.screen {
            TerminalScreen::Main => self
                .scrollback_row_anchors
                .iter()
                .position(|row| *row == anchor.row)
                .or_else(|| {
                    self.main
                        .row_anchors
                        .iter()
                        .position(|row| *row == anchor.row)
                        .map(|index| self.scrollback.len() + index)
                }),
            TerminalScreen::Alternate => self
                .alternate
                .row_anchors
                .iter()
                .position(|row| *row == anchor.row),
        }
    }

    fn row_for_text_screen(&self, screen: TerminalScreen, index: usize) -> Option<&[BasicCell]> {
        match screen {
            TerminalScreen::Main => self.logical_row(index),
            TerminalScreen::Alternate => self.alternate.cells.get(index).map(Vec::as_slice),
        }
    }

    fn row_count_for_text_screen(&self, screen: TerminalScreen) -> usize {
        match screen {
            TerminalScreen::Main => self.combined_logical_row_count(),
            TerminalScreen::Alternate => self.alternate.cells.len(),
        }
    }

    fn rotate_active_row_anchors_left_and_replace_tail(
        &mut self,
        start: usize,
        end: usize,
        count: usize,
    ) {
        match self.active_screen {
            ActiveScreen::Main => rotate_row_anchors_left_and_replace_tail(
                &mut self.main.row_anchors,
                &mut self.next_main_row_anchor,
                start,
                end,
                count,
            ),
            ActiveScreen::Alternate => rotate_row_anchors_left_and_replace_tail(
                &mut self.alternate.row_anchors,
                &mut self.next_alternate_row_anchor,
                start,
                end,
                count,
            ),
        }
    }

    fn rotate_active_row_anchors_right_and_replace_head(
        &mut self,
        start: usize,
        end: usize,
        count: usize,
    ) {
        match self.active_screen {
            ActiveScreen::Main => rotate_row_anchors_right_and_replace_head(
                &mut self.main.row_anchors,
                &mut self.next_main_row_anchor,
                start,
                end,
                count,
            ),
            ActiveScreen::Alternate => rotate_row_anchors_right_and_replace_head(
                &mut self.alternate.row_anchors,
                &mut self.next_alternate_row_anchor,
                start,
                end,
                count,
            ),
        }
    }

    fn replace_active_row_anchors(&mut self) {
        match self.active_screen {
            ActiveScreen::Main => {
                self.main.row_anchors =
                    allocate_row_anchors(self.size.rows, &mut self.next_main_row_anchor);
            }
            ActiveScreen::Alternate => {
                self.alternate.row_anchors =
                    allocate_row_anchors(self.size.rows, &mut self.next_alternate_row_anchor);
            }
        }
    }

    fn print_char(&mut self, ch: char) {
        self.follow_tail();
        if ch == '\n' {
            self.linefeed();
            return;
        }

        if self.size.rows == 0 || self.size.cols == 0 {
            return;
        }

        let char_width = terminal_char_width(ch);
        if char_width == 0 {
            self.append_combining_char(ch);
            return;
        }

        if self.pending_wrap {
            self.pending_wrap = false;
            self.carriage_return();
            self.linefeed();
        }

        let last_col = self.size.cols.saturating_sub(1);
        let mut col = self.cursor().position.col.min(last_col);
        if char_width > 1
            && col.saturating_add(u16::from(char_width)) > self.size.cols
            && self.autowrap
            && col > 0
        {
            self.carriage_return();
            self.linefeed();
            col = self.cursor().position.col.min(last_col);
        }

        let row = self.cursor().position.row as usize;
        if let Some(row_cells) = self.cells().get(row) {
            col = cell_start_for_col(row_cells, usize::from(col))
                .and_then(|col| u16::try_from(col).ok())
                .unwrap_or(col);
        }
        let remaining_cols = self.size.cols.saturating_sub(col).max(1);
        let cell_width = char_width.min(u8::try_from(remaining_cols).unwrap_or(u8::MAX));
        let style = self.current_cell_style();
        if self.insert_mode {
            for _ in 0..cell_width {
                self.insert_cell_space(row, col);
            }
        }
        let mapped_ch = self.map_next_printable_char(ch);
        self.write_printable_cell(row, col, mapped_ch, cell_width, style);
        self.last_printed_graphic = Some(mapped_ch);

        let last_written_col = col.saturating_add(u16::from(cell_width).saturating_sub(1));
        if self.autowrap && last_written_col >= last_col {
            self.cursor_mut().position.col = last_col;
            self.pending_wrap = true;
        } else {
            self.cursor_mut().position.col =
                col.saturating_add(u16::from(cell_width)).min(last_col);
            self.pending_wrap = false;
        }
    }

    fn append_combining_char(&mut self, ch: char) {
        let row = self.cursor().position.row as usize;
        let start_col = if self.pending_wrap {
            self.cursor().position.col
        } else {
            self.cursor().position.col.saturating_sub(1)
        } as usize;

        let changed = self
            .cells_mut()
            .get_mut(row)
            .and_then(|row_cells| {
                (0..=start_col.min(row_cells.len().saturating_sub(1)))
                    .rev()
                    .find(|col| {
                        row_cells[*col].width > 0 && !row_cells[*col].text.trim().is_empty()
                    })
                    .map(|col| {
                        row_cells[col].text.push(ch);
                    })
            })
            .is_some();

        if changed {
            self.mark_row_dirty(row as u16);
        }
    }

    fn write_printable_cell(
        &mut self,
        row: usize,
        col: u16,
        ch: char,
        width: u8,
        style: BasicCellStyle,
    ) {
        let active_hyperlink = self.active_hyperlink;
        let current_protected = self.current_protected;
        let Some(row_cells) = self.cells_mut().get_mut(row) else {
            return;
        };

        let start = usize::from(col).min(row_cells.len().saturating_sub(1));
        let blank_cell = BasicCell {
            text: " ".to_owned(),
            width: 1,
            style: style.clone(),
            hyperlink: None,
            protected: false,
        };
        row_cells[start] = BasicCell {
            text: ch.to_string(),
            width,
            style: style.clone(),
            hyperlink: active_hyperlink,
            protected: current_protected,
        };

        for offset in 1..usize::from(width) {
            if let Some(cell) = row_cells.get_mut(start + offset) {
                *cell = BasicCell {
                    text: String::new(),
                    width: 0,
                    style: style.clone(),
                    hyperlink: active_hyperlink,
                    protected: current_protected,
                };
            }
        }
        repair_wide_cells(row_cells, blank_cell);

        self.mark_row_dirty(row as u16);
    }

    fn linefeed(&mut self) {
        self.follow_tail();
        self.pending_wrap = false;
        let old_position = self.cursor().position;
        let scroll_region = self.effective_scroll_region();
        if self.cursor().position.row == scroll_region.bottom {
            self.scroll_up_region(scroll_region);
        } else {
            self.cursor_mut().position.row = self
                .cursor()
                .position
                .row
                .saturating_add(1)
                .min(self.size.rows.saturating_sub(1));
            self.mark_cursor_rows_dirty(old_position, self.cursor().position);
        }
    }

    fn linefeed_control(&mut self) {
        if self.linefeed_newline {
            self.carriage_return();
        }
        self.linefeed();
    }

    fn reverse_index(&mut self) {
        self.follow_tail();
        self.pending_wrap = false;
        let old_position = self.cursor().position;
        let scroll_region = self.effective_scroll_region();
        if self.cursor().position.row == scroll_region.top {
            self.scroll_down_region(scroll_region);
        } else {
            self.cursor_mut().position.row = self.cursor().position.row.saturating_sub(1);
            self.mark_cursor_rows_dirty(old_position, self.cursor().position);
        }
    }

    fn carriage_return(&mut self) {
        self.follow_tail();
        self.pending_wrap = false;
        let old_position = self.cursor().position;
        self.cursor_mut().position.col = 0;
        self.mark_cursor_rows_dirty(old_position, self.cursor().position);
    }

    fn backspace(&mut self) {
        self.follow_tail();
        self.pending_wrap = false;
        let old_position = self.cursor().position;
        let position = self.cursor().position;
        if self.reverse_wraparound && position.col == 0 && position.row > 0 {
            self.cursor_mut().position.row = position.row - 1;
            self.cursor_mut().position.col = self.size.cols.saturating_sub(1);
        } else {
            self.cursor_mut().position.col = position.col.saturating_sub(1);
        }
        self.mark_cursor_rows_dirty(old_position, self.cursor().position);
    }

    fn horizontal_tab(&mut self) {
        self.cursor_forward_tabulation(1);
    }

    fn cursor_forward_tabulation(&mut self, count: u16) {
        self.follow_tail();
        self.pending_wrap = false;
        let old_position = self.cursor().position;
        let last_col = self.size.cols.saturating_sub(1);
        let mut col = self.cursor().position.col.min(last_col);
        for _ in 0..count.max(1) {
            let next_tab = self
                .tab_stops
                .range(col.saturating_add(1)..)
                .next()
                .copied()
                .unwrap_or(last_col)
                .min(last_col);
            if next_tab == col {
                break;
            }
            col = next_tab;
        }
        self.cursor_mut().position.col = col;
        self.mark_cursor_rows_dirty(old_position, self.cursor().position);
    }

    fn cursor_backward_tabulation(&mut self, count: u16) {
        self.follow_tail();
        self.pending_wrap = false;
        let old_position = self.cursor().position;
        let last_col = self.size.cols.saturating_sub(1);
        let mut col = self.cursor().position.col.min(last_col);
        for _ in 0..count.max(1) {
            let previous_tab = self
                .tab_stops
                .range(..col)
                .next_back()
                .copied()
                .unwrap_or(0);
            if previous_tab == col {
                break;
            }
            col = previous_tab;
        }
        self.cursor_mut().position.col = col;
        self.mark_cursor_rows_dirty(old_position, self.cursor().position);
    }

    fn set_active_charset(&mut self, charset: CharsetIndex) {
        self.active_charset = charset;
        self.single_shift_charset = None;
    }

    fn set_single_shift_charset(&mut self, charset: CharsetIndex) {
        self.single_shift_charset = Some(charset);
    }

    fn designate_charset(&mut self, charset: CharsetIndex, standard_charset: StandardCharset) {
        self.charsets[charset as usize] = standard_charset;
    }

    fn map_next_printable_char(&mut self, ch: char) -> char {
        let charset = self
            .single_shift_charset
            .take()
            .unwrap_or(self.active_charset);
        self.map_charset(charset, ch)
    }

    fn map_charset(&self, charset: CharsetIndex, ch: char) -> char {
        match self.charsets[charset as usize] {
            StandardCharset::Ascii => ch,
            StandardCharset::DecSpecialGraphics => dec_special_graphics_char(ch),
            StandardCharset::UkNational => uk_national_char(ch),
        }
    }

    fn index(&mut self) {
        self.linefeed();
    }

    fn next_line(&mut self) {
        self.carriage_return();
        self.linefeed();
    }

    fn set_horizontal_tab_stop(&mut self) {
        let col = self.cursor().position.col;
        if col < self.size.cols {
            self.tab_stops.insert(col);
        }
    }

    fn clear_tab_stop(&mut self, mode: u16) {
        match mode {
            0 => {
                self.tab_stops.remove(&self.cursor().position.col);
            }
            3 => {
                self.tab_stops.clear();
            }
            _ => {}
        }
    }

    fn reset_tab_stops_to_default(&mut self) {
        self.tab_stops = default_tab_stops(self.size);
    }

    fn move_cursor_addressed(&mut self, row: u16, col: u16) {
        let row = if self.origin_mode {
            let region = self.effective_scroll_region();
            region.top.saturating_add(row).min(region.bottom)
        } else {
            row.min(self.size.rows.saturating_sub(1))
        };
        self.move_cursor_absolute(row, col);
    }

    fn move_cursor_absolute(&mut self, row: u16, col: u16) {
        self.follow_tail();
        self.pending_wrap = false;
        let old_position = self.cursor().position;
        self.cursor_mut().position.row = row.min(self.size.rows.saturating_sub(1));
        self.cursor_mut().position.col = col.min(self.size.cols.saturating_sub(1));
        self.mark_cursor_rows_dirty(old_position, self.cursor().position);
    }

    fn move_relative(&mut self, row_delta: i16, col_delta: i16) {
        let row = self.cursor().position.row.saturating_add_signed(row_delta);
        let col = self.cursor().position.col.saturating_add_signed(col_delta);
        self.move_cursor_absolute(self.clamp_movement_row(row), col);
    }

    fn move_line_relative(&mut self, row_delta: i16) {
        let row = self.cursor().position.row.saturating_add_signed(row_delta);
        self.move_cursor_absolute(self.clamp_movement_row(row), 0);
    }

    fn clamp_movement_row(&self, row: u16) -> u16 {
        let row = row.min(self.size.rows.saturating_sub(1));
        if self.origin_mode {
            let region = self.effective_scroll_region();
            row.clamp(region.top, region.bottom)
        } else {
            row
        }
    }

    fn cursor_home(&mut self) {
        let row = if self.origin_mode {
            self.effective_scroll_region().top
        } else {
            0
        };
        self.move_cursor_absolute(row, 0);
    }

    fn soft_reset(&mut self) {
        self.insert_mode = false;
        self.origin_mode = false;
        self.autowrap = true;
        self.linefeed_newline = false;
        self.keyboard_locked = false;
        self.reverse_video = false;
        self.reverse_wraparound = false;
        self.application_keypad = false;
        self.application_cursor_keys = false;
        self.backarrow_sends_backspace = false;
        self.main_kitty_keyboard_flags = 0;
        self.alternate_kitty_keyboard_flags = 0;
        self.main_kitty_keyboard_stack.clear();
        self.alternate_kitty_keyboard_stack.clear();
        self.scroll_region = None;
        self.pending_wrap = false;
        self.last_printed_graphic = None;
        self.current_protected = false;
        self.active_charset = CharsetIndex::G0;
        self.single_shift_charset = None;
        self.charsets = [StandardCharset::Ascii; 4];
        self.current_style = BasicCellStyle::default();
        self.active_hyperlink = None;
        self.restore_color_theme_defaults();
        self.main.cursor.shape = self.default_cursor_shape;
        self.main.cursor.visible = true;
        self.main.cursor.blink = self.default_cursor_blink;
        self.alternate.cursor.shape = self.default_cursor_shape;
        self.alternate.cursor.visible = true;
        self.alternate.cursor.blink = self.default_cursor_blink;
        self.saved_main_cursor = None;
        self.saved_cursor = Some(self.saved_cursor_state(CellPoint::new(0, 0)));
        self.mark_full_damage();
    }

    fn reset(&mut self) {
        let cursor = self.default_cursor_state();
        self.main.reset(
            self.size,
            cursor,
            &mut self.next_main_row_anchor,
        );
        self.alternate.reset(
            self.size,
            cursor,
            &mut self.next_alternate_row_anchor,
        );
        self.active_screen = ActiveScreen::Main;
        self.saved_main_cursor = None;
        self.saved_cursor = None;
        self.scroll_region = None;
        self.origin_mode = false;
        self.autowrap = true;
        self.insert_mode = false;
        self.linefeed_newline = false;
        self.keyboard_locked = false;
        self.reverse_video = false;
        self.reverse_wraparound = false;
        self.application_keypad = false;
        self.application_cursor_keys = false;
        self.backarrow_sends_backspace = false;
        self.main_kitty_keyboard_flags = 0;
        self.alternate_kitty_keyboard_flags = 0;
        self.main_kitty_keyboard_stack.clear();
        self.alternate_kitty_keyboard_stack.clear();
        self.mouse_tracking = MouseTrackingMode::None;
        self.mouse_utf8_encoding = false;
        self.mouse_urxvt_encoding = false;
        self.mouse_sgr_encoding = false;
        self.mouse_sgr_pixel_encoding = false;
        self.mouse_focus_events = false;
        self.mouse_alternate_scroll = false;
        self.synchronized_output = false;
        self.pending_wrap = false;
        self.last_printed_graphic = None;
        self.current_protected = false;
        self.active_charset = CharsetIndex::G0;
        self.single_shift_charset = None;
        self.charsets = [StandardCharset::Ascii; 4];
        self.tab_stops = default_tab_stops(self.size);
        self.restore_color_theme_defaults();
        self.current_style = BasicCellStyle::default();
        self.scrollback.clear();
        self.scrollback_row_anchors.clear();
        self.viewport_offset = 0;
        self.selection = None;
        self.title = None;
        self.hyperlinks.clear();
        self.active_hyperlink = None;
        self.next_hyperlink_id = 1;
        self.bracketed_paste = false;
        self.mark_full_damage();
    }

    fn erase_in_display(&mut self, mode: u16) {
        self.follow_tail();
        self.pending_wrap = false;

        match mode {
            0 => {
                let start_row = self.cursor().position.row;
                for row in start_row..self.size.rows {
                    let start_col = if row == start_row {
                        self.cursor().position.col
                    } else {
                        0
                    };
                    self.erase_line_range(row, start_col, self.size.cols);
                }
            }
            1 => {
                let end_row = self
                    .cursor()
                    .position
                    .row
                    .min(self.size.rows.saturating_sub(1));
                for row in 0..=end_row {
                    let end_col = if row == end_row {
                        self.cursor().position.col.saturating_add(1)
                    } else {
                        self.size.cols
                    };
                    self.erase_line_range(row, 0, end_col);
                }
            }
            2 => self.erase_screen(),
            3 => self.erase_scrollback(),
            _ => {}
        }
    }

    fn selective_erase_in_display(&mut self, mode: u16) {
        self.follow_tail();
        self.pending_wrap = false;

        match mode {
            0 => {
                let start_row = self.cursor().position.row;
                for row in start_row..self.size.rows {
                    let start_col = if row == start_row {
                        self.cursor().position.col
                    } else {
                        0
                    };
                    self.erase_line_range_selective(row, start_col, self.size.cols, true);
                }
            }
            1 => {
                let end_row = self
                    .cursor()
                    .position
                    .row
                    .min(self.size.rows.saturating_sub(1));
                for row in 0..=end_row {
                    let end_col = if row == end_row {
                        self.cursor().position.col.saturating_add(1)
                    } else {
                        self.size.cols
                    };
                    self.erase_line_range_selective(row, 0, end_col, true);
                }
            }
            2 => {
                for row in 0..self.size.rows {
                    self.erase_line_range_selective(row, 0, self.size.cols, true);
                }
            }
            _ => {}
        }
    }

    fn erase_in_line(&mut self, mode: u16) {
        self.follow_tail();
        self.pending_wrap = false;

        match mode {
            0 => self.erase_line_range(
                self.cursor().position.row,
                self.cursor().position.col,
                self.size.cols,
            ),
            1 => self.erase_line_range(
                self.cursor().position.row,
                0,
                self.cursor().position.col.saturating_add(1),
            ),
            2 => self.erase_line_range(self.cursor().position.row, 0, self.size.cols),
            _ => {}
        }
    }

    fn selective_erase_in_line(&mut self, mode: u16) {
        self.follow_tail();
        self.pending_wrap = false;

        match mode {
            0 => self.erase_line_range_selective(
                self.cursor().position.row,
                self.cursor().position.col,
                self.size.cols,
                true,
            ),
            1 => self.erase_line_range_selective(
                self.cursor().position.row,
                0,
                self.cursor().position.col.saturating_add(1),
                true,
            ),
            2 => {
                self.erase_line_range_selective(self.cursor().position.row, 0, self.size.cols, true)
            }
            _ => {}
        }
    }

    fn erase_screen(&mut self) {
        let cell = self.erased_cell();
        self.active_buffer_mut().cells = (0..self.size.rows)
            .map(|_| vec![cell.clone(); self.size.cols as usize])
            .collect();
        self.replace_active_row_anchors();
        self.mark_full_damage();
    }

    fn screen_alignment_test(&mut self) {
        self.follow_tail();
        self.pending_wrap = false;
        let cell = BasicCell {
            text: "E".to_owned(),
            ..BasicCell::default()
        };
        self.active_buffer_mut().cells = (0..self.size.rows)
            .map(|_| vec![cell.clone(); self.size.cols as usize])
            .collect();
        self.replace_active_row_anchors();
        self.mark_full_damage();
    }

    fn erase_scrollback(&mut self) {
        if self.active_screen == ActiveScreen::Alternate {
            return;
        }
        if !self.scrollback.is_empty() || self.viewport_offset != 0 {
            self.scrollback.clear();
            self.scrollback_row_anchors.clear();
            self.viewport_offset = 0;
            self.mark_full_damage();
        }
    }

    fn erase_line_range(&mut self, row: u16, start_col: u16, end_col_exclusive: u16) {
        self.erase_line_range_selective(row, start_col, end_col_exclusive, false);
    }

    fn erase_line_range_selective(
        &mut self,
        row: u16,
        start_col: u16,
        end_col_exclusive: u16,
        selective: bool,
    ) {
        let cell = self.erased_cell();
        let changed = self
            .cells_mut()
            .get_mut(row as usize)
            .map(|row_cells| {
                let Some((start, end)) = expanded_cell_edit_range(
                    row_cells,
                    usize::from(start_col),
                    usize::from(end_col_exclusive),
                ) else {
                    return false;
                };
                if selective {
                    for row_cell in &mut row_cells[start..end] {
                        if !row_cell.protected {
                            *row_cell = cell.clone();
                        }
                    }
                } else {
                    row_cells[start..end].fill(cell.clone());
                }
                repair_wide_cells(row_cells, cell);
                true
            })
            .unwrap_or(false);

        if changed {
            self.mark_row_dirty(row);
        }
    }

    fn insert_blank_chars(&mut self, count: u16) {
        self.follow_tail();
        self.pending_wrap = false;
        let row = self.cursor().position.row;
        let col = self.cursor().position.col;
        let cell = self.erased_cell();
        let changed = self
            .cells_mut()
            .get_mut(row as usize)
            .map(|row_cells| {
                let start = edit_start_col(row_cells, usize::from(col));
                if start >= row_cells.len() {
                    return false;
                }
                let count = usize::from(count).max(1).min(row_cells.len() - start);
                row_cells[start..].rotate_right(count);
                row_cells[start..start + count].fill(cell.clone());
                repair_wide_cells(row_cells, cell);
                true
            })
            .unwrap_or(false);

        if changed {
            self.mark_row_dirty(row);
        }
    }

    fn insert_cell_space(&mut self, row: usize, col: u16) {
        let Some(row_cells) = self.cells_mut().get_mut(row) else {
            return;
        };
        let start = edit_start_col(row_cells, usize::from(col));
        if start >= row_cells.len() {
            return;
        }

        row_cells[start..].rotate_right(1);
    }

    fn delete_chars(&mut self, count: u16) {
        self.follow_tail();
        self.pending_wrap = false;
        let row = self.cursor().position.row;
        let col = self.cursor().position.col;
        let cell = self.erased_cell();
        let changed = self
            .cells_mut()
            .get_mut(row as usize)
            .map(|row_cells| {
                let Some((start, end)) = expanded_cell_edit_range(
                    row_cells,
                    usize::from(col),
                    usize::from(col) + usize::from(count).max(1),
                ) else {
                    return false;
                };
                let count = end - start;
                row_cells[start..].rotate_left(count);
                let blank_start = row_cells.len() - count;
                row_cells[blank_start..].fill(cell.clone());
                repair_wide_cells(row_cells, cell);
                true
            })
            .unwrap_or(false);

        if changed {
            self.mark_row_dirty(row);
        }
    }

    fn erase_chars(&mut self, count: u16) {
        self.follow_tail();
        self.pending_wrap = false;
        let start_col = self.cursor().position.col;
        let count = count.max(1);
        self.erase_line_range(
            self.cursor().position.row,
            start_col,
            start_col.saturating_add(count),
        );
    }

    fn repeat_preceding_graphic(&mut self, count: u16) {
        let Some(ch) = self.last_printed_graphic else {
            return;
        };
        for _ in 0..count.max(1) {
            self.print_char(ch);
        }
    }

    fn insert_blank_lines(&mut self, count: u16) {
        self.edit_lines(count, true);
    }

    fn delete_lines(&mut self, count: u16) {
        self.edit_lines(count, false);
    }

    fn edit_lines(&mut self, count: u16, insert: bool) {
        self.follow_tail();
        self.pending_wrap = false;
        let row = self.cursor().position.row;
        let region = self.effective_scroll_region();
        if row < region.top || row > region.bottom {
            return;
        }

        let cell = self.erased_cell();
        let cols = usize::from(self.size.cols);
        let count = usize::from(count)
            .max(1)
            .min(usize::from(region.bottom - row + 1));
        let cells = self.cells_mut();
        let start = usize::from(row).min(cells.len());
        let end = usize::from(region.bottom)
            .saturating_add(1)
            .min(cells.len());
        if start >= end {
            return;
        }

        if insert {
            cells[start..end].rotate_right(count);
            for line in &mut cells[start..start + count] {
                *line = vec![cell.clone(); cols];
            }
            self.rotate_active_row_anchors_right_and_replace_head(start, end, count);
        } else {
            cells[start..end].rotate_left(count);
            for line in &mut cells[end - count..end] {
                *line = vec![cell.clone(); cols];
            }
            self.rotate_active_row_anchors_left_and_replace_tail(start, end, count);
        }

        for dirty_row in row..=region.bottom {
            self.mark_row_dirty(dirty_row);
        }
    }

    fn erased_cell(&self) -> BasicCell {
        BasicCell {
            text: " ".to_owned(),
            width: 1,
            style: self.current_cell_style(),
            hyperlink: None,
            protected: false,
        }
    }

    fn scroll_up_region(&mut self, region: ScrollRegion) {
        if self.cells().is_empty() {
            return;
        }
        let full_screen = region.top == 0 && region.bottom >= self.size.rows.saturating_sub(1);
        if full_screen && self.active_screen == ActiveScreen::Main && self.max_scrollback_lines > 0
        {
            if let Some(anchor) = self.active_row_anchor_at(0) {
                self.push_scrollback_row(self.main.cells[0].clone(), anchor);
            }
        }
        let cols = self.size.cols;
        let cells = self.cells_mut();
        let start = usize::from(region.top).min(cells.len());
        let end = usize::from(region.bottom)
            .saturating_add(1)
            .min(cells.len());
        if start >= end {
            return;
        }
        cells[start..end].rotate_left(1);
        if let Some(last) = cells.get_mut(end - 1) {
            *last = blank_row(cols);
        }
        self.rotate_active_row_anchors_left_and_replace_tail(start, end, 1);
        if full_screen {
            self.mark_full_damage();
        } else {
            for row in region.top..=region.bottom {
                self.mark_row_dirty(row);
            }
        }
    }

    fn scroll_down_region(&mut self, region: ScrollRegion) {
        if self.cells().is_empty() {
            return;
        }
        let cols = self.size.cols;
        let cells = self.cells_mut();
        let start = usize::from(region.top).min(cells.len());
        let end = usize::from(region.bottom)
            .saturating_add(1)
            .min(cells.len());
        if start >= end {
            return;
        }
        cells[start..end].rotate_right(1);
        if let Some(first) = cells.get_mut(start) {
            *first = blank_row(cols);
        }
        self.rotate_active_row_anchors_right_and_replace_head(start, end, 1);
        for row in region.top..=region.bottom {
            self.mark_row_dirty(row);
        }
    }

    fn scroll_up_lines(&mut self, count: u16) {
        self.follow_tail();
        self.pending_wrap = false;
        let region = self.effective_scroll_region();
        let count = count
            .max(1)
            .min(region.bottom.saturating_sub(region.top) + 1);
        for _ in 0..count {
            self.scroll_up_region(region);
        }
    }

    fn scroll_down_lines(&mut self, count: u16) {
        self.follow_tail();
        self.pending_wrap = false;
        let region = self.effective_scroll_region();
        let count = count
            .max(1)
            .min(region.bottom.saturating_sub(region.top) + 1);
        for _ in 0..count {
            self.scroll_down_region(region);
        }
    }

    fn set_scroll_region(&mut self, params: &Params) {
        if self.size.rows < 2 {
            return;
        }

        let top = defaulting_zero_param(params, 0, 1).saturating_sub(1);
        let bottom = defaulting_zero_param(params, 1, self.size.rows)
            .saturating_sub(1)
            .min(self.size.rows.saturating_sub(1));
        if top >= bottom {
            return;
        }

        self.scroll_region = if top == 0 && bottom == self.size.rows.saturating_sub(1) {
            None
        } else {
            Some(ScrollRegion { top, bottom })
        };
        self.cursor_home();
    }

    fn effective_scroll_region(&self) -> ScrollRegion {
        self.scroll_region.unwrap_or(ScrollRegion {
            top: 0,
            bottom: self.size.rows.saturating_sub(1),
        })
    }

    fn visible_rows(&self) -> Vec<Vec<BasicCell>> {
        if self.active_screen == ActiveScreen::Alternate {
            return self.alternate.cells.clone();
        }

        let visible_count = self.size.rows as usize;
        let combined_len = self.combined_logical_row_count();
        let max_offset = combined_len.saturating_sub(visible_count);
        let offset = self.viewport_offset.min(max_offset);
        let end = combined_len.saturating_sub(offset);
        let start = end.saturating_sub(visible_count);

        (start..end)
            .filter_map(|logical_index| self.logical_row(logical_index))
            .map(<[BasicCell]>::to_vec)
            .collect()
    }

    fn logical_row(&self, logical_index: usize) -> Option<&[BasicCell]> {
        let scrollback_len = self.scrollback.len();
        if logical_index < scrollback_len {
            self.scrollback.get(logical_index).map(Vec::as_slice)
        } else {
            self.main
                .cells
                .get(logical_index.saturating_sub(scrollback_len))
                .map(Vec::as_slice)
        }
    }

    fn search_text_rows(&self) -> Vec<SearchTextRow> {
        if self.active_screen == ActiveScreen::Alternate {
            return self
                .alternate
                .cells
                .iter()
                .enumerate()
                .map(|(index, row)| {
                    let searchable = searchable_row(row);
                    SearchTextRow::with_columns(
                        SearchRowId::screen(index),
                        u16::try_from(index).ok(),
                        searchable.text,
                        searchable.columns,
                    )
                })
                .collect();
        }

        let (visible_start, visible_end) = self.visible_logical_row_window();
        let mut rows = Vec::with_capacity(self.scrollback.len() + self.main.cells.len());

        rows.extend(self.scrollback.iter().enumerate().map(|(index, row)| {
            let searchable = searchable_row(row);
            SearchTextRow::with_columns(
                SearchRowId::scrollback(index),
                visible_row_for_logical_index(index, visible_start, visible_end),
                searchable.text,
                searchable.columns,
            )
        }));

        let scrollback_len = self.scrollback.len();
        rows.extend(self.main.cells.iter().enumerate().map(|(index, row)| {
            let logical_index = scrollback_len + index;
            let searchable = searchable_row(row);
            SearchTextRow::with_columns(
                SearchRowId::screen(index),
                visible_row_for_logical_index(logical_index, visible_start, visible_end),
                searchable.text,
                searchable.columns,
            )
        }));

        rows
    }

    fn visible_search_matches(&self, matches: &[SearchMatch]) -> Vec<CellRange> {
        if self.size.cols == 0 {
            return Vec::new();
        }

        matches
            .iter()
            .filter_map(|search_match| {
                let row = self.visible_row_for_search_row(search_match.row)?;
                let start_col = search_match.start_col.min(self.size.cols.saturating_sub(1));
                let end_col = search_match.end_col.min(self.size.cols.saturating_sub(1));
                if end_col < start_col {
                    return None;
                }
                Some(CellRange {
                    start: CellPoint::new(row, start_col),
                    end: CellPoint::new(row, end_col),
                })
            })
            .collect()
    }

    fn visible_search_highlights(
        &self,
        matches: &[SearchMatch],
        active: Option<SearchMatch>,
    ) -> Vec<SearchHighlight> {
        if self.size.cols == 0 {
            return Vec::new();
        }

        matches
            .iter()
            .filter_map(|search_match| {
                let row = self.visible_row_for_search_row(search_match.row)?;
                let start_col = search_match.start_col.min(self.size.cols.saturating_sub(1));
                let end_col = search_match.end_col.min(self.size.cols.saturating_sub(1));
                if end_col < start_col {
                    return None;
                }
                Some(SearchHighlight {
                    range: CellRange {
                        start: CellPoint::new(row, start_col),
                        end: CellPoint::new(row, end_col),
                    },
                    active: active == Some(*search_match),
                })
            })
            .collect()
    }

    fn scroll_to_search_match(&mut self, row: SearchRowId, buffer_rows: u16) -> bool {
        if self.active_screen == ActiveScreen::Alternate {
            return matches!(row.kind, SearchRowKind::Screen)
                && row.index < self.alternate.cells.len();
        }

        let Some(target) = self.logical_index_for_search_row(row) else {
            return false;
        };
        if self.visible_row_for_search_row(row).is_some() {
            return true;
        }

        let visible_count = usize::from(self.size.rows);
        let combined_len = self.combined_logical_row_count();
        if visible_count == 0 || combined_len == 0 {
            return false;
        }

        let max_start = combined_len.saturating_sub(visible_count);
        let buffer = usize::from(buffer_rows).min(visible_count.saturating_sub(1));
        let desired_start = target.saturating_sub(buffer).min(max_start);
        let desired_end = desired_start
            .saturating_add(visible_count)
            .min(combined_len);
        let next_offset = combined_len.saturating_sub(desired_end);

        if self.viewport_offset != next_offset {
            self.viewport_offset = next_offset;
            self.mark_full_damage();
        }
        true
    }

    fn visible_row_for_search_row(&self, row: SearchRowId) -> Option<u16> {
        if self.active_screen == ActiveScreen::Alternate {
            return match row.kind {
                SearchRowKind::Screen if row.index < self.alternate.cells.len() => {
                    u16::try_from(row.index).ok()
                }
                _ => None,
            };
        }

        let logical_index = self.logical_index_for_search_row(row)?;
        let (visible_start, visible_end) = self.visible_logical_row_window();
        visible_row_for_logical_index(logical_index, visible_start, visible_end)
    }

    fn logical_index_for_search_row(&self, row: SearchRowId) -> Option<usize> {
        match row.kind {
            SearchRowKind::Scrollback if row.index < self.scrollback.len() => Some(row.index),
            SearchRowKind::Screen if row.index < self.main.cells.len() => {
                Some(self.scrollback.len() + row.index)
            }
            _ => None,
        }
    }

    fn visible_logical_row_window(&self) -> (usize, usize) {
        let visible_count = usize::from(self.size.rows);
        let combined_len = self.combined_logical_row_count();
        let max_offset = combined_len.saturating_sub(visible_count);
        let offset = self.viewport_offset.min(max_offset);
        let end = combined_len.saturating_sub(offset);
        let start = end.saturating_sub(visible_count);
        (start, end)
    }

    fn combined_logical_row_count(&self) -> usize {
        self.scrollback.len() + self.main.cells.len()
    }

    fn scroll_viewport_lines(&mut self, lines: i16) {
        if self.active_screen == ActiveScreen::Alternate {
            if self.viewport_offset != 0 {
                self.viewport_offset = 0;
                self.mark_full_damage();
            }
            return;
        }

        let max_offset = self.scrollback.len();
        let previous_offset = self.viewport_offset;
        if lines >= 0 {
            self.viewport_offset = self
                .viewport_offset
                .saturating_add(lines as usize)
                .min(max_offset);
        } else {
            self.viewport_offset = self.viewport_offset.saturating_sub((-lines) as usize);
        }
        if self.viewport_offset != previous_offset {
            self.mark_full_damage();
        }
    }

    fn follow_tail(&mut self) {
        if self.viewport_offset != 0 {
            self.viewport_offset = 0;
            self.mark_full_damage();
        }
    }

    fn push_scrollback_row(&mut self, row: Vec<BasicCell>, row_anchor: u64) {
        self.scrollback.push_back(row);
        self.scrollback_row_anchors.push_back(row_anchor);
        self.trim_scrollback_overflow();
    }

    fn set_max_scrollback_lines(&mut self, max_scrollback_lines: usize) {
        if self.max_scrollback_lines == max_scrollback_lines {
            return;
        }
        self.max_scrollback_lines = max_scrollback_lines;
        self.trim_scrollback_overflow();
    }

    fn trim_scrollback_overflow(&mut self) {
        let overflow = self
            .scrollback
            .len()
            .saturating_sub(self.max_scrollback_lines);
        if overflow == 0 {
            return;
        }
        for _ in 0..overflow {
            self.scrollback.pop_front();
            self.scrollback_row_anchors.pop_front();
        }
        self.viewport_offset = self.viewport_offset.min(self.scrollback.len());
        self.mark_full_damage();
    }

    fn set_selection(&mut self, selection: Option<CellRange>) {
        if self.selection != selection {
            self.selection = selection;
            self.mark_full_damage();
        }
    }

    fn set_private_mode(&mut self, params: &Params, mode: u16, enabled: bool) {
        if !params_contains(params, mode) {
            return;
        }

        match mode {
            CURSOR_KEY_APPLICATION_MODE => {
                self.application_cursor_keys = enabled;
            }
            APPLICATION_KEYPAD_MODE => {
                self.set_application_keypad(enabled);
            }
            BACKARROW_KEY_MODE => {
                self.backarrow_sends_backspace = enabled;
            }
            ORIGIN_MODE => {
                self.set_origin_mode(enabled);
            }
            AUTOWRAP_MODE => {
                self.autowrap = enabled;
                if !enabled {
                    self.pending_wrap = false;
                }
            }
            REVERSE_VIDEO_MODE if self.reverse_video != enabled => {
                self.reverse_video = enabled;
                self.mark_full_damage();
            }
            REVERSE_WRAPAROUND_MODE => {
                self.reverse_wraparound = enabled;
            }
            X10_MOUSE_MODE => {
                self.set_mouse_tracking_mode(MouseTrackingMode::X10, enabled);
            }
            BRACKETED_PASTE_MODE => {
                self.bracketed_paste = enabled;
            }
            SYNCHRONIZED_OUTPUT_MODE => {
                self.synchronized_output = enabled;
            }
            CURSOR_BLINK_MODE => {
                self.set_cursor_blink(enabled);
            }
            CURSOR_VISIBLE_MODE if self.cursor().visible != enabled => {
                self.main.cursor.visible = enabled;
                self.alternate.cursor.visible = enabled;
                self.mark_row_dirty(self.cursor().position.row);
            }
            NORMAL_MOUSE_MODE => {
                self.set_mouse_tracking_mode(MouseTrackingMode::Normal, enabled);
            }
            BUTTON_EVENT_MOUSE_MODE => {
                self.set_mouse_tracking_mode(MouseTrackingMode::ButtonEvent, enabled);
            }
            ANY_EVENT_MOUSE_MODE => {
                self.set_mouse_tracking_mode(MouseTrackingMode::AnyEvent, enabled);
            }
            FOCUS_EVENT_MODE => {
                self.mouse_focus_events = enabled;
            }
            UTF8_MOUSE_MODE => {
                self.mouse_utf8_encoding = enabled;
            }
            URXVT_MOUSE_MODE => {
                self.mouse_urxvt_encoding = enabled;
            }
            SGR_MOUSE_MODE => {
                self.mouse_sgr_encoding = enabled;
            }
            ALTERNATE_SCROLL_MODE => {
                self.mouse_alternate_scroll = enabled;
            }
            SGR_PIXEL_MOUSE_MODE => {
                self.mouse_sgr_pixel_encoding = enabled;
            }
            LEGACY_ALTERNATE_SCREEN_MODE | ALTERNATE_SCREEN_MODE => {
                self.set_alternate_screen_compat(enabled);
            }
            SAVE_CURSOR_MODE => {
                self.set_saved_cursor(enabled);
            }
            ALTERNATE_SCREEN_WITH_CURSOR_MODE => {
                self.set_alternate_screen_with_cursor(enabled);
            }
            _ => {}
        }
    }

    fn set_mouse_tracking_mode(&mut self, mode: MouseTrackingMode, enabled: bool) {
        if enabled {
            self.mouse_tracking = mode;
        } else if self.mouse_tracking == mode {
            self.mouse_tracking = MouseTrackingMode::None;
        }
    }

    fn set_mode(&mut self, params: &Params, mode: u16, enabled: bool) {
        if !params_contains(params, mode) {
            return;
        }

        match mode {
            KEYBOARD_ACTION_MODE => self.keyboard_locked = enabled,
            INSERT_MODE => self.insert_mode = enabled,
            LINE_FEED_NEW_LINE_MODE => self.linefeed_newline = enabled,
            _ => {}
        }
    }

    fn set_origin_mode(&mut self, enabled: bool) {
        self.origin_mode = enabled;
        self.cursor_home();
    }

    fn set_cursor_style(&mut self, shape: CursorShape, blink: bool) {
        if self.cursor().shape != shape || self.cursor().blink != blink {
            self.main.cursor.shape = shape;
            self.main.cursor.blink = blink;
            self.alternate.cursor.shape = shape;
            self.alternate.cursor.blink = blink;
            self.mark_row_dirty(self.cursor().position.row);
        }
    }

    fn set_default_cursor_style(&mut self, shape: CursorShape, blink: bool) {
        self.default_cursor_shape = shape;
        self.default_cursor_blink = blink;
        self.set_cursor_style(shape, blink);
    }

    fn default_cursor_style(&self) -> (CursorShape, bool) {
        (self.default_cursor_shape, self.default_cursor_blink)
    }

    fn default_cursor_state(&self) -> CursorState {
        CursorState {
            shape: self.default_cursor_shape,
            blink: self.default_cursor_blink,
            ..CursorState::default()
        }
    }

    fn set_cursor_blink(&mut self, blink: bool) {
        if self.cursor().blink != blink {
            self.main.cursor.blink = blink;
            self.alternate.cursor.blink = blink;
            self.mark_row_dirty(self.cursor().position.row);
        }
    }

    fn set_alternate_screen_compat(&mut self, enabled: bool) {
        if enabled {
            self.enter_alternate_screen(false);
        } else {
            self.leave_alternate_screen(false);
        }
    }

    fn set_alternate_screen_with_cursor(&mut self, enabled: bool) {
        if enabled {
            if self.active_screen == ActiveScreen::Main {
                self.saved_main_cursor = Some(self.main.cursor.position);
            }
            self.enter_alternate_screen(true);
        } else {
            self.leave_alternate_screen(true);
        }
    }

    fn set_saved_cursor(&mut self, enabled: bool) {
        if enabled {
            self.save_cursor();
        } else if let Some(saved_cursor) = self.saved_cursor.clone() {
            self.restore_cursor(saved_cursor);
        }
    }

    fn save_cursor(&mut self) {
        self.saved_cursor = Some(self.saved_cursor_state(self.cursor().position));
    }

    fn saved_cursor_state(&self, position: CellPoint) -> SavedCursor {
        SavedCursor {
            screen: self.active_screen,
            position,
            origin_mode: self.origin_mode,
            autowrap: self.autowrap,
            pending_wrap: self.pending_wrap,
            active_charset: self.active_charset,
            charsets: self.charsets,
            current_style: self.current_style.clone(),
            current_protected: self.current_protected,
        }
    }

    fn restore_cursor(&mut self, saved_cursor: SavedCursor) {
        if saved_cursor.screen == self.active_screen {
            let old_position = self.cursor().position;
            self.cursor_mut().position = clamp_cell_point(saved_cursor.position, self.size);
            self.origin_mode = saved_cursor.origin_mode;
            self.autowrap = saved_cursor.autowrap;
            self.pending_wrap = saved_cursor.pending_wrap && self.autowrap;
            self.active_charset = saved_cursor.active_charset;
            self.single_shift_charset = None;
            self.charsets = saved_cursor.charsets;
            self.current_style = saved_cursor.current_style;
            self.current_protected = saved_cursor.current_protected;
            self.mark_cursor_rows_dirty(old_position, self.cursor().position);
        }
    }

    fn enter_alternate_screen(&mut self, clear: bool) {
        if self.active_screen == ActiveScreen::Alternate {
            return;
        }

        self.follow_tail();
        self.pending_wrap = false;
        if clear {
            self.alternate.reset(
                self.size,
                CursorState {
                    position: CellPoint::new(0, 0),
                    shape: self.main.cursor.shape,
                    visible: self.main.cursor.visible,
                    blink: self.main.cursor.blink,
                },
                &mut self.next_alternate_row_anchor,
            );
        }
        self.active_screen = ActiveScreen::Alternate;
        self.selection = None;
        self.mark_full_damage();
    }

    fn leave_alternate_screen(&mut self, restore_cursor: bool) {
        if self.active_screen == ActiveScreen::Main {
            return;
        }

        self.pending_wrap = false;
        self.active_screen = ActiveScreen::Main;
        if restore_cursor {
            if let Some(saved_cursor) = self.saved_main_cursor.take() {
                self.main.cursor.position = clamp_cell_point(saved_cursor, self.size);
            }
        } else {
            self.saved_main_cursor = None;
        }
        self.viewport_offset = 0;
        self.selection = None;
        self.mark_full_damage();
    }

    fn set_title(&mut self, title: String) {
        if self.title.as_deref() != Some(title.as_str()) {
            self.title = Some(title);
        }
    }

    fn set_color_theme(&mut self, theme: TerminalColorTheme) {
        let previous = self.color_theme;
        if previous == theme {
            return;
        }

        let mut full_damage = false;
        let cursor_row = self.cursor().position.row;
        let cursor_color_was_theme = self.cursor_color == previous.cursor_color;

        self.color_theme = theme;
        if self.default_foreground == previous.foreground {
            self.default_foreground = theme.foreground;
            full_damage = true;
        }
        if self.default_background == previous.background {
            self.default_background = theme.background;
            full_damage = true;
        }
        if previous.palette != theme.palette {
            full_damage = true;
        }
        if cursor_color_was_theme && self.cursor_color != theme.cursor_color {
            self.cursor_color = theme.cursor_color;
            self.mark_row_dirty(cursor_row);
        }
        if full_damage {
            self.mark_full_damage();
        }
    }

    fn color_theme(&self) -> TerminalColorTheme {
        self.color_theme
    }

    fn restore_color_theme_defaults(&mut self) {
        self.palette_overrides.clear();
        self.default_foreground = self.color_theme.foreground;
        self.default_background = self.color_theme.background;
        self.cursor_color = self.color_theme.cursor_color;
    }

    fn set_palette_color(&mut self, index: u8, color: Rgba) {
        if self.palette_overrides.insert(index, color) != Some(color) {
            self.mark_full_damage();
        }
    }

    fn reset_palette_color(&mut self, index: u8) {
        if self.palette_overrides.remove(&index).is_some() {
            self.mark_full_damage();
        }
    }

    fn reset_palette(&mut self) {
        if !self.palette_overrides.is_empty() {
            self.palette_overrides.clear();
            self.mark_full_damage();
        }
    }

    fn set_default_foreground(&mut self, color: Rgba) {
        if self.default_foreground != color {
            self.default_foreground = color;
            self.mark_full_damage();
        }
    }

    fn set_default_background(&mut self, color: Rgba) {
        if self.default_background != color {
            self.default_background = color;
            self.mark_full_damage();
        }
    }

    fn reset_default_foreground(&mut self) {
        self.set_default_foreground(self.color_theme.foreground);
    }

    fn reset_default_background(&mut self) {
        self.set_default_background(self.color_theme.background);
    }

    fn effective_cursor_color(&self) -> Rgba {
        self.cursor_color.unwrap_or(DEFAULT_CURSOR_COLOR)
    }

    fn set_cursor_color(&mut self, color: Rgba) {
        if self.cursor_color != Some(color) {
            self.cursor_color = Some(color);
            self.mark_row_dirty(self.cursor().position.row);
        }
    }

    fn reset_cursor_color(&mut self) {
        let cursor_color = self.color_theme.cursor_color;
        if self.cursor_color != cursor_color {
            self.cursor_color = cursor_color;
            self.mark_row_dirty(self.cursor().position.row);
        }
    }

    fn indexed_color(&self, index: u8) -> Rgba {
        self.palette_overrides
            .get(&index)
            .copied()
            .unwrap_or_else(|| match index {
                0..=15 => self.color_theme.palette[index as usize],
                _ => default_ansi_256_color(index),
            })
    }

    fn resolve_style(&self, style: &BasicCellStyle) -> CellStyle {
        let mut style = CellStyle {
            foreground: self.resolve_color(style.foreground),
            background: self.resolve_color(style.background),
            underline_color: style.underline_color.map(|color| self.resolve_color(color)),
            flags: style.flags,
        };
        if self.reverse_video {
            std::mem::swap(&mut style.foreground, &mut style.background);
        }
        style
    }

    fn resolve_color(&self, color: CellColor) -> Rgba {
        match color {
            CellColor::DefaultForeground => self.default_foreground,
            CellColor::DefaultBackground => self.default_background,
            CellColor::Indexed(index) => self.indexed_color(index),
            CellColor::Direct(color) => color,
        }
    }

    fn visible_default_background(&self) -> Rgba {
        if self.reverse_video {
            self.default_foreground
        } else {
            self.default_background
        }
    }

    fn apply_osc_palette_control(&mut self, params: &[&[u8]]) -> bool {
        let Some((code, rest)) = params
            .split_first()
            .and_then(|(code, rest)| Some((osc_code(code)?, rest)))
        else {
            return false;
        };

        match code {
            4 => {
                for pair in rest.chunks_exact(2) {
                    let Some(index) = osc_palette_index(pair[0]) else {
                        continue;
                    };
                    if is_osc_query_field(pair[1]) {
                        self.push_palette_reply(4, index, self.indexed_color(index));
                    } else if let Some(color) = osc_color(pair[1]) {
                        self.set_palette_color(index, color);
                    }
                }
                true
            }
            10 => {
                if let Some(field) = rest.first() {
                    if is_osc_query_field(field) {
                        self.push_color_reply(10, self.default_foreground);
                    } else if let Some(color) = osc_color(field) {
                        self.set_default_foreground(color);
                    }
                }
                true
            }
            11 => {
                if let Some(field) = rest.first() {
                    if is_osc_query_field(field) {
                        self.push_color_reply(11, self.default_background);
                    } else if let Some(color) = osc_color(field) {
                        self.set_default_background(color);
                    }
                }
                true
            }
            12 => {
                if let Some(field) = rest.first() {
                    if is_osc_query_field(field) {
                        self.push_color_reply(12, self.effective_cursor_color());
                    } else if let Some(color) = osc_color(field) {
                        self.set_cursor_color(color);
                    }
                }
                true
            }
            104 => {
                if rest.is_empty() {
                    self.reset_palette();
                } else {
                    for index in rest.iter().filter_map(|part| osc_palette_index(part)) {
                        self.reset_palette_color(index);
                    }
                }
                true
            }
            110 => {
                self.reset_default_foreground();
                true
            }
            111 => {
                self.reset_default_background();
                true
            }
            112 => {
                self.reset_cursor_color();
                true
            }
            _ => false,
        }
    }

    fn push_palette_reply(&mut self, osc_code: u16, index: u8, color: Rgba) {
        self.push_terminal_reply(format!("\x1b]{osc_code};{index};{}\x1b\\", osc_rgb(color)));
    }

    fn push_color_reply(&mut self, osc_code: u16, color: Rgba) {
        self.push_terminal_reply(format!("\x1b]{osc_code};{}\x1b\\", osc_rgb(color)));
    }

    fn start_hyperlink(&mut self, uri: String, osc8_id: Option<String>) {
        let hyperlink_id = self.ensure_hyperlink(uri, osc8_id);
        self.active_hyperlink = Some(hyperlink_id);
    }

    fn end_hyperlink(&mut self) {
        self.active_hyperlink = None;
    }

    fn ensure_hyperlink(&mut self, uri: String, osc8_id: Option<String>) -> HyperlinkId {
        if let Some(existing) = self
            .hyperlinks
            .iter()
            .find(|link| link.uri == uri && link.osc8_id == osc8_id)
        {
            return existing.id;
        }

        let id = self.next_hyperlink_id;
        self.next_hyperlink_id = self.next_hyperlink_id.saturating_add(1).max(1);
        self.hyperlinks.push(TerminalHyperlink { id, uri, osc8_id });
        id
    }

    fn shell_integration_event(
        &self,
        marker: TerminalShellIntegrationMarker,
        exit_code: Option<i32>,
    ) -> TerminalShellIntegrationEvent {
        TerminalShellIntegrationEvent {
            marker,
            screen: self.terminal_screen(),
            point: self.cursor().position,
            anchor: self.row_anchor_for_point(self.cursor().position),
            exit_code,
        }
    }

    fn current_directory_action(&self, directory: OscCurrentDirectory) -> TerminalCurrentDirectory {
        TerminalCurrentDirectory {
            uri: directory.uri,
            host: directory.host,
            path: directory.path,
            screen: self.terminal_screen(),
            point: self.cursor().position,
            anchor: self.row_anchor_for_point(self.cursor().position),
        }
    }

    fn terminal_screen(&self) -> TerminalScreen {
        match self.active_screen {
            ActiveScreen::Main => TerminalScreen::Main,
            ActiveScreen::Alternate => TerminalScreen::Alternate,
        }
    }

    fn set_application_keypad(&mut self, enabled: bool) {
        self.application_keypad = enabled;
    }

    fn set_character_protection(&mut self, mode: u16) {
        match mode {
            0 | 2 => self.current_protected = false,
            1 => self.current_protected = true,
            _ => {}
        }
    }

    fn apply_sgr(&mut self, params: &Params) {
        let groups = sgr_param_groups(params);
        if groups.is_empty() {
            self.current_style = BasicCellStyle::default();
            return;
        }

        let mut index = 0;
        while index < groups.len() {
            let code = sgr_group_code(&groups[index]);
            match code {
                0 => self.current_style = BasicCellStyle::default(),
                1 => self.current_style.flags.bold = true,
                2 => self.current_style.flags.faint = true,
                3 => self.current_style.flags.italic = true,
                4 => self.apply_underline_sgr(&groups[index]),
                5 | 6 => self.current_style.flags.blink = true,
                7 => self.current_style.flags.reverse = true,
                8 => self.current_style.flags.conceal = true,
                9 => self.current_style.flags.strike = true,
                21 => {
                    self.current_style.flags.underline = true;
                    self.current_style.flags.underline_style = UnderlineStyle::Double;
                }
                22 => {
                    self.current_style.flags.bold = false;
                    self.current_style.flags.faint = false;
                }
                23 => self.current_style.flags.italic = false,
                24 => self.clear_underline_sgr(),
                25 => self.current_style.flags.blink = false,
                27 => self.current_style.flags.reverse = false,
                28 => self.current_style.flags.conceal = false,
                29 => self.current_style.flags.strike = false,
                30..=37 => self.current_style.foreground = CellColor::Indexed((code - 30) as u8),
                39 => self.current_style.foreground = CellColor::DefaultForeground,
                40..=47 => self.current_style.background = CellColor::Indexed((code - 40) as u8),
                49 => self.current_style.background = CellColor::DefaultBackground,
                90..=97 => {
                    self.current_style.foreground = CellColor::Indexed((code - 90 + 8) as u8)
                }
                100..=107 => {
                    self.current_style.background = CellColor::Indexed((code - 100 + 8) as u8)
                }
                51 => {
                    self.current_style.flags.framed = true;
                    self.current_style.flags.encircled = false;
                }
                52 => {
                    self.current_style.flags.framed = false;
                    self.current_style.flags.encircled = true;
                }
                53 => self.current_style.flags.overline = true,
                54 => {
                    self.current_style.flags.framed = false;
                    self.current_style.flags.encircled = false;
                }
                55 => self.current_style.flags.overline = false,
                58 => {
                    if let Some((color, consumed)) = self.extended_sgr_color(&groups[index..]) {
                        self.current_style.underline_color = Some(color);
                        index += consumed.saturating_sub(1);
                    }
                }
                59 => self.current_style.underline_color = None,
                73 => self.current_style.flags.baseline_shift = BaselineShift::Superscript,
                74 => self.current_style.flags.baseline_shift = BaselineShift::Subscript,
                75 => self.current_style.flags.baseline_shift = BaselineShift::Normal,
                38 | 48 => {
                    if let Some((color, consumed)) = self.extended_sgr_color(&groups[index..]) {
                        if code == 38 {
                            self.current_style.foreground = color;
                        } else {
                            self.current_style.background = color;
                        }
                        index += consumed.saturating_sub(1);
                    }
                }
                _ => {}
            }
            index += 1;
        }
    }

    fn apply_underline_sgr(&mut self, group: &[u16]) {
        match group.get(1).copied() {
            None | Some(1) => self.set_underline_sgr(UnderlineStyle::Single),
            Some(0) => self.clear_underline_sgr(),
            Some(2) => self.set_underline_sgr(UnderlineStyle::Double),
            Some(3) => self.set_underline_sgr(UnderlineStyle::Curly),
            Some(4) => self.set_underline_sgr(UnderlineStyle::Dotted),
            Some(5) => self.set_underline_sgr(UnderlineStyle::Dashed),
            Some(_) => {}
        }
    }

    fn set_underline_sgr(&mut self, style: UnderlineStyle) {
        self.current_style.flags.underline = true;
        self.current_style.flags.underline_style = style;
    }

    fn clear_underline_sgr(&mut self) {
        self.current_style.flags.underline = false;
        self.current_style.flags.underline_style = UnderlineStyle::Single;
    }

    fn extended_sgr_color(&self, groups: &[Vec<u16>]) -> Option<(CellColor, usize)> {
        let first = groups.first()?;
        if first.len() > 1 {
            return extended_sgr_color_from_subparams(&first[1..]).map(|color| (color, 1));
        }

        let kind = sgr_group_code(groups.get(1)?);
        match kind {
            2 => {
                let red = sgr_group_code(groups.get(2)?);
                let green = sgr_group_code(groups.get(3)?);
                let blue = sgr_group_code(groups.get(4)?);
                Some((
                    CellColor::Direct(Rgba::rgb(
                        sgr_rgb_component(red),
                        sgr_rgb_component(green),
                        sgr_rgb_component(blue),
                    )),
                    5,
                ))
            }
            5 => u8::try_from(sgr_group_code(groups.get(2)?))
                .ok()
                .map(|index| (CellColor::Indexed(index), 3)),
            _ => None,
        }
    }

    fn current_cell_style(&self) -> BasicCellStyle {
        let mut style = self.current_style.clone();
        if style.flags.reverse {
            std::mem::swap(&mut style.foreground, &mut style.background);
        }
        style
    }

    fn selected_text(&self) -> Option<String> {
        let selection = ordered_cell_range(self.selection?);
        if self.size.rows == 0 || self.size.cols == 0 {
            return Some(String::new());
        }

        let rows = self.visible_rows();
        let start_row = selection.start.row.min(self.size.rows.saturating_sub(1));
        let end_row = selection.end.row.min(self.size.rows.saturating_sub(1));
        let mut lines = Vec::new();

        for row_index in start_row..=end_row {
            let Some(row) = rows.get(row_index as usize) else {
                continue;
            };
            let start_col = if row_index == start_row {
                selection.start.col
            } else {
                0
            };
            let end_col = if row_index == end_row {
                selection.end.col
            } else {
                self.size.cols.saturating_sub(1)
            };
            lines.push(selected_row_text(row, start_col, end_col));
        }

        Some(lines.join("\n"))
    }

    fn text_for_range(&self, range: TerminalTextRange) -> Option<String> {
        if range.start_anchor.is_some() || range.end_exclusive_anchor.is_some() {
            return self.anchored_text_for_range(range);
        }
        self.visible_text_for_range(range)
    }

    fn anchored_text_for_range(&self, range: TerminalTextRange) -> Option<String> {
        if range.end_exclusive.row < range.start.row {
            return None;
        }

        let start_anchor = range
            .start_anchor
            .filter(|anchor| anchor.row.screen == range.screen);
        let end_anchor = range
            .end_exclusive_anchor
            .filter(|anchor| anchor.row.screen == range.screen);
        if range.start_anchor.is_some() && start_anchor.is_none()
            || range.end_exclusive_anchor.is_some() && end_anchor.is_none()
        {
            return None;
        }

        let row_delta = usize::from(range.end_exclusive.row - range.start.row);
        let start_index = match start_anchor {
            Some(anchor) => Some(self.row_index_for_anchor(anchor.row)?),
            None => None,
        };
        let end_index = match end_anchor {
            Some(anchor) => Some(self.row_index_for_anchor(anchor.row)?),
            None => None,
        };
        let row_count = self.row_count_for_text_screen(range.screen);

        let (start_row_index, end_row_index) = match (start_index, end_index) {
            (Some(start), Some(end)) => (start, end),
            (Some(start), None) => (start, start.checked_add(row_delta)?),
            (None, Some(end)) => (end.checked_sub(row_delta)?, end),
            (None, None) => return self.visible_text_for_range(range),
        };
        if end_row_index < start_row_index || end_row_index >= row_count {
            return None;
        }

        let start_col = start_anchor
            .map(|anchor| anchor.col)
            .unwrap_or(range.start.col);
        let end_col_exclusive = end_anchor
            .map(|anchor| anchor.col)
            .unwrap_or(range.end_exclusive.col);
        let rows = (start_row_index..=end_row_index)
            .map(|index| self.row_for_text_screen(range.screen, index))
            .collect::<Option<Vec<_>>>()?;

        Some(text_from_consecutive_rows(
            &rows,
            start_col,
            end_col_exclusive,
        ))
    }

    fn visible_text_for_range(&self, range: TerminalTextRange) -> Option<String> {
        if range.screen != self.terminal_screen() || range.end_exclusive.row < range.start.row {
            return None;
        }

        let rows = self.visible_rows();
        let start_row = usize::from(range.start.row);
        let end_row = usize::from(range.end_exclusive.row);
        if end_row >= rows.len() {
            return None;
        }
        let row_refs = rows[start_row..=end_row]
            .iter()
            .map(Vec::as_slice)
            .collect::<Vec<_>>();

        Some(text_from_consecutive_rows(
            &row_refs,
            range.start.col,
            range.end_exclusive.col,
        ))
    }

    fn word_range_at(&self, point: CellPoint) -> Option<CellRange> {
        if point.row >= self.size.rows || point.col >= self.size.cols {
            return None;
        }

        let rows = self.visible_rows();
        let row = rows.get(point.row as usize)?;
        let col = cell_start_for_col(row, point.col as usize)?;
        if !row.get(col).is_some_and(is_word_cell) {
            return None;
        }

        let mut start = col;
        while let Some(previous) = previous_cell_start(row, start) {
            if row.get(previous).is_some_and(is_word_cell) {
                start = previous;
            } else {
                break;
            }
        }

        let mut end = col;
        while let Some(next) = next_cell_start(row, end) {
            if row.get(next).is_some_and(is_word_cell) {
                end = next;
            } else {
                break;
            }
        }
        let end = cell_end_exclusive(row, end).saturating_sub(1);

        Some(CellRange {
            start: CellPoint::new(point.row, start as u16),
            end: CellPoint::new(point.row, end as u16),
        })
    }

    fn mark_cursor_rows_dirty(&mut self, old_position: CellPoint, new_position: CellPoint) {
        self.mark_row_dirty(old_position.row);
        self.mark_row_dirty(new_position.row);
    }

    fn mark_row_dirty(&mut self, row: u16) {
        if !self.full_damage && row < self.size.rows {
            self.dirty_rows.insert(row);
        }
    }

    fn mark_full_damage(&mut self) {
        self.full_damage = true;
        self.dirty_rows.clear();
    }

    fn clear_damage(&mut self) {
        self.full_damage = false;
        self.dirty_rows.clear();
        self.damage_cleared_once = true;
    }

    fn damage_region(&self) -> DamageRegion {
        if self.full_damage || (!self.damage_cleared_once && self.dirty_rows.is_empty()) {
            DamageRegion::Full
        } else {
            DamageRegion::Rows(self.dirty_rows.iter().copied().collect())
        }
    }

    fn primary_device_attributes(&mut self, params: &Params) {
        if param(params, 0, 0) == 0 {
            self.push_terminal_reply(b"\x1b[?1;2c".to_vec());
        }
    }

    fn secondary_device_attributes(&mut self, params: &Params) {
        if param(params, 0, 0) == 0 {
            self.push_terminal_reply(b"\x1b[>0;1;0c".to_vec());
        }
    }

    fn tertiary_device_attributes(&mut self, params: &Params) {
        if param(params, 0, 0) == 0 {
            self.push_terminal_reply(b"\x1bP!|00000000\x1b\\".to_vec());
        }
    }

    fn terminal_name_and_version(&mut self, params: &Params) {
        if param(params, 0, 0) == 0 {
            self.push_terminal_reply(
                format!("\x1bP>|Witty {}\x1b\\", env!("CARGO_PKG_VERSION")).into_bytes(),
            );
        }
    }

    fn terminal_parameters_report(&mut self, params: &Params) {
        match param(params, 0, 0) {
            0 => self.push_terminal_reply(b"\x1b[2;1;1;128;128;1;0x".to_vec()),
            1 => self.push_terminal_reply(b"\x1b[3;1;1;128;128;1;0x".to_vec()),
            _ => {}
        }
    }

    fn device_status_report(&mut self, params: &Params) {
        match param(params, 0, 0) {
            DEVICE_STATUS_OK => self.push_terminal_reply(b"\x1b[0n".to_vec()),
            CURSOR_POSITION_REPORT => self.push_cursor_position_report(false),
            _ => {}
        }
    }

    fn private_device_status_report(&mut self, params: &Params) {
        if param(params, 0, 0) == CURSOR_POSITION_REPORT {
            self.push_cursor_position_report(true);
        }
    }

    fn push_cursor_position_report(&mut self, private: bool) {
        let cursor = self.cursor().position;
        let row = cursor.row.saturating_add(1);
        let col = cursor.col.saturating_add(1);
        let private_marker = if private { "?" } else { "" };
        self.push_terminal_reply(format!("\x1b[{private_marker}{row};{col}R").into_bytes());
    }

    fn request_mode_report(&mut self, params: &Params, private: bool) {
        let mode = param(params, 0, 0);
        let status = if private {
            self.private_mode_report_status(mode)
        } else {
            self.mode_report_status(mode)
        };
        let private_marker = if private { "?" } else { "" };
        self.push_terminal_reply(format!("\x1b[{private_marker}{mode};{status}$y").into_bytes());
    }

    fn query_kitty_keyboard_flags(&mut self) {
        self.push_terminal_reply(format!("\x1b[?{}u", self.active_kitty_keyboard_flags()));
    }

    fn push_kitty_keyboard_flags(&mut self, params: &Params) {
        let current = self.active_kitty_keyboard_flags();
        {
            let stack = self.active_kitty_keyboard_stack_mut();
            if stack.len() == MAX_KITTY_KEYBOARD_STACK_DEPTH {
                stack.remove(0);
            }
            stack.push(current);
        }
        self.set_active_kitty_keyboard_flags(param(params, 0, 0));
    }

    fn pop_kitty_keyboard_flags(&mut self, params: &Params) {
        let count = defaulting_zero_param(params, 0, 1);
        let mut restored = None;
        {
            let stack = self.active_kitty_keyboard_stack_mut();
            for _ in 0..count {
                restored = stack.pop();
                if restored.is_none() {
                    break;
                }
            }
        }
        self.set_active_kitty_keyboard_flags(restored.unwrap_or(0));
    }

    fn set_kitty_keyboard_flags(&mut self, params: &Params) {
        let flags = param(params, 0, 0) & SUPPORTED_KITTY_KEYBOARD_FLAGS;
        let mode = defaulting_zero_param(params, 1, 1);
        let current = self.active_kitty_keyboard_flags();
        let next = match mode {
            1 => flags,
            2 => current | flags,
            3 => current & !flags,
            _ => current,
        };
        self.set_active_kitty_keyboard_flags(next);
    }

    fn mode_report_status(&self, mode: u16) -> u16 {
        match mode {
            KEYBOARD_ACTION_MODE => mode_status(self.keyboard_locked),
            INSERT_MODE => mode_status(self.insert_mode),
            LINE_FEED_NEW_LINE_MODE => mode_status(self.linefeed_newline),
            _ => 0,
        }
    }

    fn private_mode_report_status(&self, mode: u16) -> u16 {
        match mode {
            CURSOR_KEY_APPLICATION_MODE => mode_status(self.application_cursor_keys),
            REVERSE_VIDEO_MODE => mode_status(self.reverse_video),
            ORIGIN_MODE => mode_status(self.origin_mode),
            AUTOWRAP_MODE => mode_status(self.autowrap),
            X10_MOUSE_MODE => mode_status(self.mouse_tracking == MouseTrackingMode::X10),
            BRACKETED_PASTE_MODE => mode_status(self.bracketed_paste),
            CURSOR_BLINK_MODE => mode_status(self.cursor().blink),
            CURSOR_VISIBLE_MODE => mode_status(self.cursor().visible),
            REVERSE_WRAPAROUND_MODE => mode_status(self.reverse_wraparound),
            NORMAL_MOUSE_MODE => mode_status(self.mouse_tracking == MouseTrackingMode::Normal),
            BUTTON_EVENT_MOUSE_MODE => {
                mode_status(self.mouse_tracking == MouseTrackingMode::ButtonEvent)
            }
            ANY_EVENT_MOUSE_MODE => mode_status(self.mouse_tracking == MouseTrackingMode::AnyEvent),
            FOCUS_EVENT_MODE => mode_status(self.mouse_focus_events),
            UTF8_MOUSE_MODE => mode_status(self.mouse_utf8_encoding),
            SGR_MOUSE_MODE => mode_status(self.mouse_sgr_encoding),
            ALTERNATE_SCROLL_MODE => mode_status(self.mouse_alternate_scroll),
            URXVT_MOUSE_MODE => mode_status(self.mouse_urxvt_encoding),
            SGR_PIXEL_MOUSE_MODE => mode_status(self.mouse_sgr_pixel_encoding),
            SYNCHRONIZED_OUTPUT_MODE => mode_status(self.synchronized_output),
            APPLICATION_KEYPAD_MODE => mode_status(self.application_keypad),
            BACKARROW_KEY_MODE => mode_status(self.backarrow_sends_backspace),
            LEGACY_ALTERNATE_SCREEN_MODE
            | ALTERNATE_SCREEN_MODE
            | ALTERNATE_SCREEN_WITH_CURSOR_MODE => {
                mode_status(self.active_screen == ActiveScreen::Alternate)
            }
            _ => 0,
        }
    }

    fn window_manipulation_report(&mut self, params: &Params) {
        match param(params, 0, 0) {
            REPORT_TEXT_AREA_SIZE_CHARS | REPORT_SCREEN_SIZE_CHARS => {
                self.push_terminal_reply(
                    format!("\x1b[8;{};{}t", self.size.rows, self.size.cols).into_bytes(),
                );
            }
            _ => {}
        }
    }

    fn begin_dcs(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        self.dcs_request = None;
        if ignore || !params_are_default(params) {
            return;
        }

        match (intermediates, action) {
            ([b'$'], 'q') => {
                self.dcs_request = Some(DcsRequest::new(DcsRequestKind::RequestStatusString));
            }
            ([b'+'], 'q') => {
                self.dcs_request = Some(DcsRequest::new(DcsRequestKind::TermcapQuery));
            }
            _ => {}
        }
    }

    fn put_dcs_byte(&mut self, byte: u8) {
        if let Some(request) = &mut self.dcs_request {
            request.push(byte);
        }
    }

    fn finish_dcs(&mut self) {
        let Some(request) = self.dcs_request.take() else {
            return;
        };
        if request.overflowed {
            return;
        }

        match request.kind {
            DcsRequestKind::RequestStatusString => self.reply_decrqss(&request.payload),
            DcsRequestKind::TermcapQuery => self.reply_xtgettcap(&request.payload),
        }
    }

    fn reply_decrqss(&mut self, request: &[u8]) {
        if let Some(status) = self.decrqss_status_string(request) {
            self.push_terminal_reply(format!("\x1bP1$r{status}\x1b\\").into_bytes());
        } else if decrqss_request_is_reply_safe(request) {
            self.push_terminal_reply(format!(
                "\x1bP0$r{}\x1b\\",
                String::from_utf8_lossy(request)
            ));
        }
    }

    fn decrqss_status_string(&self, request: &[u8]) -> Option<String> {
        match request {
            b"\"p" => Some("65;1\"p".to_owned()),
            b"m" => Some(format!("{}m", self.current_sgr_status_params().join(";"))),
            b"r" => {
                let region = self.effective_scroll_region();
                Some(format!(
                    "{};{}r",
                    region.top.saturating_add(1),
                    region.bottom.saturating_add(1)
                ))
            }
            b"s" => Some(format!("1;{}s", self.size.cols)),
            b" q" => Some(format!("{} q", cursor_style_parameter(self.cursor()))),
            b"\"q" => Some(format!("{}\"q", if self.current_protected { 1 } else { 0 })),
            _ => None,
        }
    }

    fn reply_xtgettcap(&mut self, request: &[u8]) {
        for encoded_name in request.split(|byte| *byte == b';') {
            let Some(encoded_name) = xtgettcap_encoded_name(encoded_name) else {
                return;
            };
            let Some(name) = xtgettcap_decode_name(encoded_name.as_bytes()) else {
                return;
            };

            match xtgettcap_capability(&name) {
                Some(XtGetTcapValue::Boolean) => {
                    self.push_terminal_reply(format!("\x1bP1+r{encoded_name}\x1b\\").into_bytes());
                }
                Some(XtGetTcapValue::String(value)) => {
                    let mut reply = format!("\x1bP1+r{encoded_name}=");
                    push_hex_upper(&mut reply, value);
                    reply.push_str("\x1b\\");
                    self.push_terminal_reply(reply.into_bytes());
                }
                None => {
                    self.push_terminal_reply(format!("\x1bP0+r{encoded_name}\x1b\\").into_bytes());
                }
            }
        }
    }

    fn current_sgr_status_params(&self) -> Vec<String> {
        let mut params = Vec::new();
        let flags = self.current_style.flags;
        if flags.bold {
            params.push("1".to_owned());
        }
        if flags.faint {
            params.push("2".to_owned());
        }
        if flags.italic {
            params.push("3".to_owned());
        }
        if flags.underline {
            params.push(match flags.underline_style {
                UnderlineStyle::Single => "4".to_owned(),
                UnderlineStyle::Double => "4:2".to_owned(),
                UnderlineStyle::Curly => "4:3".to_owned(),
                UnderlineStyle::Dotted => "4:4".to_owned(),
                UnderlineStyle::Dashed => "4:5".to_owned(),
            });
        }
        if flags.blink {
            params.push("5".to_owned());
        }
        if flags.reverse {
            params.push("7".to_owned());
        }
        if flags.conceal {
            params.push("8".to_owned());
        }
        if flags.strike {
            params.push("9".to_owned());
        }
        if flags.framed {
            params.push("51".to_owned());
        }
        if flags.encircled {
            params.push("52".to_owned());
        }
        if flags.overline {
            params.push("53".to_owned());
        }
        match flags.baseline_shift {
            BaselineShift::Normal => {}
            BaselineShift::Superscript => params.push("73".to_owned()),
            BaselineShift::Subscript => params.push("74".to_owned()),
        }

        push_sgr_color_params(&mut params, 38, self.current_style.foreground);
        push_sgr_color_params(&mut params, 48, self.current_style.background);
        if let Some(underline_color) = self.current_style.underline_color {
            push_sgr_color_params(&mut params, 58, underline_color);
        }

        if params.is_empty() {
            params.push("0".to_owned());
        }
        params
    }
}

impl Perform for BasicTerminalState {
    fn print(&mut self, ch: char) {
        self.print_char(ch);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' | 0x0b | 0x0c => self.linefeed_control(),
            b'\r' => self.carriage_return(),
            0x08 => self.backspace(),
            b'\t' => self.horizontal_tab(),
            0x07 => self.ring_bell(),
            0x84 => self.index(),
            0x85 => self.next_line(),
            0x88 => self.set_horizontal_tab_stop(),
            0x8d => self.reverse_index(),
            0x96 => self.current_protected = true,
            0x97 => self.current_protected = false,
            0x9a => self.push_terminal_reply(b"\x1b[?1;2c".to_vec()),
            0x0e => self.set_active_charset(CharsetIndex::G1),
            0x0f => self.set_active_charset(CharsetIndex::G0),
            0x8e => self.set_single_shift_charset(CharsetIndex::G2),
            0x8f => self.set_single_shift_charset(CharsetIndex::G3),
            _ => {}
        }
    }

    fn hook(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        self.begin_dcs(params, intermediates, ignore, action);
    }

    fn put(&mut self, byte: u8) {
        self.put_dcs_byte(byte);
    }

    fn unhook(&mut self) {
        self.finish_dcs();
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if let Some(title) = osc_window_title(params) {
            self.set_title(title);
        } else if let Some(directory) = osc_current_directory(params) {
            let action = self.current_directory_action(directory);
            self.pending_host_actions
                .push(TerminalHostAction::CurrentDirectory(action));
        } else if let Some(action) = osc_hyperlink(params) {
            match action {
                OscHyperlinkAction::Start { uri, osc8_id } => self.start_hyperlink(uri, osc8_id),
                OscHyperlinkAction::End => self.end_hyperlink(),
            }
        } else if let Some(OscClipboardAction::Write(write)) = osc_clipboard(params) {
            self.pending_host_actions
                .push(TerminalHostAction::ClipboardWrite(write));
        } else if let Some(marker) = osc_shell_integration(params) {
            let event = self.shell_integration_event(marker.marker, marker.exit_code);
            self.pending_host_actions
                .push(TerminalHostAction::ShellIntegration(event));
        } else {
            self.apply_osc_palette_control(params);
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        if ignore {
            return;
        }

        match action {
            'h' if intermediates == [b'?'] => {
                self.set_private_mode(params, CURSOR_KEY_APPLICATION_MODE, true);
                self.set_private_mode(params, REVERSE_VIDEO_MODE, true);
                self.set_private_mode(params, ORIGIN_MODE, true);
                self.set_private_mode(params, AUTOWRAP_MODE, true);
                self.set_private_mode(params, X10_MOUSE_MODE, true);
                self.set_private_mode(params, CURSOR_BLINK_MODE, true);
                self.set_private_mode(params, BRACKETED_PASTE_MODE, true);
                self.set_private_mode(params, CURSOR_VISIBLE_MODE, true);
                self.set_private_mode(params, REVERSE_WRAPAROUND_MODE, true);
                self.set_private_mode(params, APPLICATION_KEYPAD_MODE, true);
                self.set_private_mode(params, BACKARROW_KEY_MODE, true);
                self.set_private_mode(params, NORMAL_MOUSE_MODE, true);
                self.set_private_mode(params, BUTTON_EVENT_MOUSE_MODE, true);
                self.set_private_mode(params, ANY_EVENT_MOUSE_MODE, true);
                self.set_private_mode(params, FOCUS_EVENT_MODE, true);
                self.set_private_mode(params, UTF8_MOUSE_MODE, true);
                self.set_private_mode(params, SGR_MOUSE_MODE, true);
                self.set_private_mode(params, ALTERNATE_SCROLL_MODE, true);
                self.set_private_mode(params, URXVT_MOUSE_MODE, true);
                self.set_private_mode(params, SGR_PIXEL_MOUSE_MODE, true);
                self.set_private_mode(params, SYNCHRONIZED_OUTPUT_MODE, true);
                self.set_private_mode(params, ALTERNATE_SCREEN_MODE, true);
                self.set_private_mode(params, LEGACY_ALTERNATE_SCREEN_MODE, true);
                self.set_private_mode(params, SAVE_CURSOR_MODE, true);
                self.set_private_mode(params, ALTERNATE_SCREEN_WITH_CURSOR_MODE, true);
            }
            'l' if intermediates == [b'?'] => {
                self.set_private_mode(params, CURSOR_KEY_APPLICATION_MODE, false);
                self.set_private_mode(params, REVERSE_VIDEO_MODE, false);
                self.set_private_mode(params, ORIGIN_MODE, false);
                self.set_private_mode(params, AUTOWRAP_MODE, false);
                self.set_private_mode(params, X10_MOUSE_MODE, false);
                self.set_private_mode(params, CURSOR_BLINK_MODE, false);
                self.set_private_mode(params, BRACKETED_PASTE_MODE, false);
                self.set_private_mode(params, CURSOR_VISIBLE_MODE, false);
                self.set_private_mode(params, REVERSE_WRAPAROUND_MODE, false);
                self.set_private_mode(params, APPLICATION_KEYPAD_MODE, false);
                self.set_private_mode(params, BACKARROW_KEY_MODE, false);
                self.set_private_mode(params, NORMAL_MOUSE_MODE, false);
                self.set_private_mode(params, BUTTON_EVENT_MOUSE_MODE, false);
                self.set_private_mode(params, ANY_EVENT_MOUSE_MODE, false);
                self.set_private_mode(params, FOCUS_EVENT_MODE, false);
                self.set_private_mode(params, UTF8_MOUSE_MODE, false);
                self.set_private_mode(params, SGR_MOUSE_MODE, false);
                self.set_private_mode(params, ALTERNATE_SCROLL_MODE, false);
                self.set_private_mode(params, URXVT_MOUSE_MODE, false);
                self.set_private_mode(params, SGR_PIXEL_MOUSE_MODE, false);
                self.set_private_mode(params, SYNCHRONIZED_OUTPUT_MODE, false);
                self.set_private_mode(params, ALTERNATE_SCREEN_MODE, false);
                self.set_private_mode(params, LEGACY_ALTERNATE_SCREEN_MODE, false);
                self.set_private_mode(params, SAVE_CURSOR_MODE, false);
                self.set_private_mode(params, ALTERNATE_SCREEN_WITH_CURSOR_MODE, false);
            }
            'h' if intermediates.is_empty() => {
                self.set_mode(params, KEYBOARD_ACTION_MODE, true);
                self.set_mode(params, INSERT_MODE, true);
                self.set_mode(params, LINE_FEED_NEW_LINE_MODE, true);
            }
            'l' if intermediates.is_empty() => {
                self.set_mode(params, KEYBOARD_ACTION_MODE, false);
                self.set_mode(params, INSERT_MODE, false);
                self.set_mode(params, LINE_FEED_NEW_LINE_MODE, false);
            }
            'c' if intermediates.is_empty() => self.primary_device_attributes(params),
            'c' if intermediates == [b'>'] => self.secondary_device_attributes(params),
            'c' if intermediates == [b'='] => self.tertiary_device_attributes(params),
            'n' if intermediates.is_empty() => self.device_status_report(params),
            'n' if intermediates == [b'?'] => self.private_device_status_report(params),
            'p' if intermediates == [b'$'] => self.request_mode_report(params, false),
            'p' if intermediates == [b'?', b'$'] => self.request_mode_report(params, true),
            'p' if intermediates == [b'!'] => self.soft_reset(),
            'p' if intermediates == [b'"'] => {},
            'u' if intermediates == [b'?'] => self.query_kitty_keyboard_flags(),
            'u' if intermediates == [b'>'] => self.push_kitty_keyboard_flags(params),
            'u' if intermediates == [b'='] => self.set_kitty_keyboard_flags(params),
            'u' if intermediates == [b'<'] => self.pop_kitty_keyboard_flags(params),
            'q' if intermediates == [b'>'] => self.terminal_name_and_version(params),
            'q' if intermediates == [b' '] => match param(params, 0, 1) {
                0 | 1 => self.set_cursor_style(CursorShape::Block, true),
                2 => self.set_cursor_style(CursorShape::Block, false),
                3 => self.set_cursor_style(CursorShape::Underline, true),
                4 => self.set_cursor_style(CursorShape::Underline, false),
                5 => self.set_cursor_style(CursorShape::Bar, true),
                6 => self.set_cursor_style(CursorShape::Bar, false),
                _ => {}
            },
            'q' if intermediates == [b'"'] => {
                self.set_character_protection(param(params, 0, 0));
            }
            'H' | 'f' => {
                let row = param(params, 0, 1).saturating_sub(1);
                let col = param(params, 1, 1).saturating_sub(1);
                self.move_cursor_addressed(row, col);
            }
            'A' => self.move_relative(-movement_count(params), 0),
            'B' => self.move_relative(movement_count(params), 0),
            'C' => self.move_relative(0, movement_count(params)),
            'D' => self.move_relative(0, -movement_count(params)),
            'E' => self.move_line_relative(movement_count(params)),
            'F' => self.move_line_relative(-movement_count(params)),
            'G' | '`' => {
                let col = param(params, 0, 1).saturating_sub(1);
                self.move_cursor_absolute(self.cursor().position.row, col);
            }
            'a' => self.move_relative(0, movement_count(params)),
            'b' => self.repeat_preceding_graphic(param(params, 0, 1)),
            'd' => {
                let row = param(params, 0, 1).saturating_sub(1);
                self.move_cursor_addressed(row, self.cursor().position.col);
            }
            'e' => self.move_relative(movement_count(params), 0),
            's' if intermediates.is_empty() && params_are_default(params) => self.save_cursor(),
            'u' if intermediates.is_empty() && params_are_default(params) => {
                if let Some(saved_cursor) = self.saved_cursor.clone() {
                    self.restore_cursor(saved_cursor);
                }
            }
            'I' => self.cursor_forward_tabulation(param(params, 0, 1)),
            'Z' => self.cursor_backward_tabulation(param(params, 0, 1)),
            '@' => self.insert_blank_chars(param(params, 0, 1)),
            'L' => self.insert_blank_lines(param(params, 0, 1)),
            'M' => self.delete_lines(param(params, 0, 1)),
            'P' => self.delete_chars(param(params, 0, 1)),
            'S' => self.scroll_up_lines(param(params, 0, 1)),
            'T' => self.scroll_down_lines(param(params, 0, 1)),
            'X' => self.erase_chars(param(params, 0, 1)),
            'W' if intermediates == [b'?'] && param(params, 0, 0) == 5 => {
                self.reset_tab_stops_to_default();
            }
            'x' if intermediates.is_empty() => self.terminal_parameters_report(params),
            'J' if intermediates == [b'?'] => self.selective_erase_in_display(param(params, 0, 0)),
            'K' if intermediates == [b'?'] => self.selective_erase_in_line(param(params, 0, 0)),
            'J' => self.erase_in_display(param(params, 0, 0)),
            'K' => self.erase_in_line(param(params, 0, 0)),
            'g' => self.clear_tab_stop(param(params, 0, 0)),
            'm' if intermediates.is_empty() => self.apply_sgr(params),
            'r' => self.set_scroll_region(params),
            't' if intermediates.is_empty() => self.window_manipulation_report(params),
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        match (byte, intermediates) {
            (b'6', []) => self.move_relative(0, -1),
            (b'9', []) => self.move_relative(0, 1),
            (b'D', []) => self.index(),
            (b'E', []) => self.next_line(),
            (b'F' | b'G', [b' ']) => {}
            (b'@' | b'G', [b'%']) => {}
            (b'A', [b'(']) => self.designate_charset(CharsetIndex::G0, StandardCharset::UkNational),
            (b'A', [b')']) => self.designate_charset(CharsetIndex::G1, StandardCharset::UkNational),
            (b'A', [b'*']) => self.designate_charset(CharsetIndex::G2, StandardCharset::UkNational),
            (b'A', [b'+']) => self.designate_charset(CharsetIndex::G3, StandardCharset::UkNational),
            (b'B', [b'(']) => self.designate_charset(CharsetIndex::G0, StandardCharset::Ascii),
            (b'B', [b')']) => self.designate_charset(CharsetIndex::G1, StandardCharset::Ascii),
            (b'B', [b'*']) => self.designate_charset(CharsetIndex::G2, StandardCharset::Ascii),
            (b'B', [b'+']) => self.designate_charset(CharsetIndex::G3, StandardCharset::Ascii),
            (b'0', [b'(']) => {
                self.designate_charset(CharsetIndex::G0, StandardCharset::DecSpecialGraphics);
            }
            (b'0', [b')']) => {
                self.designate_charset(CharsetIndex::G1, StandardCharset::DecSpecialGraphics);
            }
            (b'0', [b'*']) => {
                self.designate_charset(CharsetIndex::G2, StandardCharset::DecSpecialGraphics);
            }
            (b'0', [b'+']) => {
                self.designate_charset(CharsetIndex::G3, StandardCharset::DecSpecialGraphics);
            }
            (b'N', []) => self.set_single_shift_charset(CharsetIndex::G2),
            (b'O', []) => self.set_single_shift_charset(CharsetIndex::G3),
            (b'n', []) => self.set_active_charset(CharsetIndex::G2),
            (b'o', []) => self.set_active_charset(CharsetIndex::G3),
            (b'7', []) => self.save_cursor(),
            (b'8', [b'#']) => self.screen_alignment_test(),
            (b'8', []) => {
                if let Some(saved_cursor) = self.saved_cursor.clone() {
                    self.restore_cursor(saved_cursor);
                }
            }
            (b'H', []) => self.set_horizontal_tab_stop(),
            (b'M', []) => self.reverse_index(),
            (b'V', []) => self.current_protected = true,
            (b'W', []) => self.current_protected = false,
            (b'Z', []) => self.push_terminal_reply(b"\x1b[?1;2c".to_vec()),
            (b'=', []) => self.set_application_keypad(true),
            (b'>', []) => self.set_application_keypad(false),
            (b'c', []) => self.reset(),
            _ => {}
        }
    }
}

fn param(params: &Params, index: usize, default: u16) -> u16 {
    params
        .iter()
        .nth(index)
        .and_then(|values| values.first())
        .copied()
        .unwrap_or(default)
}

fn defaulting_zero_param(params: &Params, index: usize, default: u16) -> u16 {
    match param(params, index, default) {
        0 => default,
        value => value,
    }
}

fn movement_count(params: &Params) -> i16 {
    i16::try_from(defaulting_zero_param(params, 0, 1)).unwrap_or(i16::MAX)
}

fn params_contains(params: &Params, needle: u16) -> bool {
    params
        .iter()
        .any(|values| values.first().is_some_and(|value| *value == needle))
}

fn params_are_default(params: &Params) -> bool {
    params.is_empty()
        || (params.len() == 1
            && params
                .iter()
                .next()
                .is_some_and(|values| values.first().copied().unwrap_or(0) == 0))
}

fn dec_special_graphics_char(ch: char) -> char {
    match ch {
        '`' => '◆',
        'a' => '▒',
        'b' => '\t',
        'c' => '\u{000c}',
        'd' => '\r',
        'e' => '\n',
        'f' => '°',
        'g' => '±',
        'h' => '\u{2424}',
        'i' => '\u{000b}',
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'o' => '⎺',
        'p' => '⎻',
        'q' => '─',
        'r' => '⎼',
        's' => '⎽',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        'y' => '≤',
        'z' => '≥',
        '{' => 'π',
        '|' => '≠',
        '}' => '£',
        '~' => '·',
        _ => ch,
    }
}

fn uk_national_char(ch: char) -> char {
    match ch {
        '#' => '£',
        _ => ch,
    }
}

fn mode_status(enabled: bool) -> u16 {
    if enabled {
        1
    } else {
        2
    }
}

fn cursor_style_parameter(cursor: CursorState) -> u16 {
    match (cursor.shape, cursor.blink) {
        (CursorShape::Block, true) => 1,
        (CursorShape::Block, false) => 2,
        (CursorShape::Underline, true) => 3,
        (CursorShape::Underline, false) => 4,
        (CursorShape::Bar, true) => 5,
        (CursorShape::Bar, false) => 6,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum XtGetTcapValue {
    Boolean,
    String(&'static [u8]),
}

fn xtgettcap_capability(name: &str) -> Option<XtGetTcapValue> {
    match name {
        "TN" | "name" => Some(XtGetTcapValue::String(b"xterm-256color")),
        "Co" | "colors" => Some(XtGetTcapValue::String(b"256")),
        "RGB" => Some(XtGetTcapValue::String(b"8/8/8")),
        "Tc" => Some(XtGetTcapValue::Boolean),
        "Cs" => Some(XtGetTcapValue::String(b"\x1b]12;%p1%s\x07")),
        "Cr" => Some(XtGetTcapValue::String(b"\x1b]112\x07")),
        "Ms" => Some(XtGetTcapValue::String(b"\x1b]52;%p1%s;%p2%s\x07")),
        "Ss" => Some(XtGetTcapValue::String(b"\x1b[%p1%d q")),
        "Se" => Some(XtGetTcapValue::String(b"\x1b[2 q")),
        "sitm" => Some(XtGetTcapValue::String(b"\x1b[3m")),
        "ritm" => Some(XtGetTcapValue::String(b"\x1b[23m")),
        "Smulx" => Some(XtGetTcapValue::String(b"\x1b[4:%p1%dm")),
        "Setulc" => Some(XtGetTcapValue::String(
            b"\x1b[58:2::%p1%{65536}%/%d:%p1%{256}%/%{255}%&%d:%p1%{255}%&%d%;m",
        )),
        "Sync" => Some(XtGetTcapValue::String(b"\x1b[?2026%?%p1%{1}%-%tl%eh%;")),
        "BE" => Some(XtGetTcapValue::String(b"\x1b[?2004h")),
        "BD" => Some(XtGetTcapValue::String(b"\x1b[?2004l")),
        "fe" => Some(XtGetTcapValue::String(b"\x1b[?1004h")),
        "fd" => Some(XtGetTcapValue::String(b"\x1b[?1004l")),
        "kxIN" => Some(XtGetTcapValue::String(b"\x1b[I")),
        "kxOUT" => Some(XtGetTcapValue::String(b"\x1b[O")),
        _ => None,
    }
}

fn xtgettcap_encoded_name(bytes: &[u8]) -> Option<&str> {
    if bytes.is_empty() || !bytes.iter().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    std::str::from_utf8(bytes).ok()
}

fn xtgettcap_decode_name(encoded: &[u8]) -> Option<String> {
    if encoded.len() % 2 != 0 {
        return None;
    }

    let mut bytes = Vec::with_capacity(encoded.len() / 2);
    for pair in encoded.chunks_exact(2) {
        let high = hex_nibble(pair[0])?;
        let low = hex_nibble(pair[1])?;
        let byte = (high << 4) | low;
        if !byte.is_ascii_graphic() {
            return None;
        }
        bytes.push(byte);
    }

    String::from_utf8(bytes).ok()
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn push_hex_upper(output: &mut String, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
}

fn push_sgr_color_params(params: &mut Vec<String>, base: u16, color: CellColor) {
    match color {
        CellColor::DefaultForeground | CellColor::DefaultBackground => {}
        CellColor::Indexed(index) if base == 38 && index < 8 => {
            params.push((30 + u16::from(index)).to_string());
        }
        CellColor::Indexed(index) if base == 38 && index < 16 => {
            params.push((90 + u16::from(index - 8)).to_string());
        }
        CellColor::Indexed(index) if base == 48 && index < 8 => {
            params.push((40 + u16::from(index)).to_string());
        }
        CellColor::Indexed(index) if base == 48 && index < 16 => {
            params.push((100 + u16::from(index - 8)).to_string());
        }
        CellColor::Indexed(index) => {
            params.push(format!("{base};5;{index}"));
        }
        CellColor::Direct(color) => {
            params.push(format!("{base};2;{};{};{}", color.r, color.g, color.b));
        }
    }
}

fn decrqss_request_is_reply_safe(request: &[u8]) -> bool {
    !request.is_empty() && request.iter().all(|byte| matches!(byte, 0x20..=0x7e))
}

fn clamp_cell_point(point: CellPoint, size: GridSize) -> CellPoint {
    CellPoint {
        row: point.row.min(size.rows.saturating_sub(1)),
        col: point.col.min(size.cols.saturating_sub(1)),
    }
}

fn osc_window_title(params: &[&[u8]]) -> Option<String> {
    let (code, title_parts) = params.split_first()?;
    let code = osc_code(code)?;
    if !matches!(code, 0 | 2) || title_parts.is_empty() {
        return None;
    }

    let mut bytes = Vec::new();
    for (index, part) in title_parts.iter().enumerate() {
        if index > 0 {
            bytes.push(b';');
        }
        bytes.extend_from_slice(part);
    }

    Some(sanitize_title(&String::from_utf8_lossy(&bytes)))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OscCurrentDirectory {
    uri: String,
    host: Option<String>,
    path: String,
}

fn osc_current_directory(params: &[&[u8]]) -> Option<OscCurrentDirectory> {
    let (code, uri_parts) = params.split_first()?;
    if osc_code(code)? != 7 {
        return None;
    }

    let uri_bytes = join_osc_semicolon_parts(uri_parts);
    let uri = decode_osc_text(&uri_bytes, MAX_OSC7_URI_BYTES)?;
    let (host, path) = file_uri_host_path(&uri)?;
    Some(OscCurrentDirectory { uri, host, path })
}

fn file_uri_host_path(uri: &str) -> Option<(Option<String>, String)> {
    let rest = uri.strip_prefix("file://")?;
    let slash = rest.find('/')?;
    let (host, path) = rest.split_at(slash);
    let host = if host.is_empty() {
        None
    } else {
        Some(percent_decode_uri_component(host)?)
    };
    let path = percent_decode_uri_component(path)?;
    (!path.is_empty()).then_some((host, path))
}

fn percent_decode_uri_component(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = *bytes.get(index + 1)?;
            let low = *bytes.get(index + 2)?;
            output.push((hex_nibble(high)? << 4) | hex_nibble(low)?);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    let text = String::from_utf8(output).ok()?;
    (!text.chars().any(char::is_control)).then_some(text)
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum OscHyperlinkAction {
    Start {
        uri: String,
        osc8_id: Option<String>,
    },
    End,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum OscClipboardAction {
    Write(TerminalClipboardWrite),
    Ignore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct OscShellIntegrationMarker {
    marker: TerminalShellIntegrationMarker,
    exit_code: Option<i32>,
}

fn osc_hyperlink(params: &[&[u8]]) -> Option<OscHyperlinkAction> {
    let (code, rest) = params.split_first()?;
    if osc_code(code)? != 8 {
        return None;
    }

    let Some((params_field, uri_parts)) = rest.split_first() else {
        return Some(OscHyperlinkAction::End);
    };
    let uri_bytes = join_osc_semicolon_parts(uri_parts);
    if uri_bytes.is_empty() {
        return Some(OscHyperlinkAction::End);
    }

    let Some(uri) = decode_osc_text(&uri_bytes, MAX_OSC8_URI_BYTES) else {
        return Some(OscHyperlinkAction::End);
    };
    Some(OscHyperlinkAction::Start {
        uri,
        osc8_id: osc8_id(params_field),
    })
}

fn osc_clipboard(params: &[&[u8]]) -> Option<OscClipboardAction> {
    let (code, rest) = params.split_first()?;
    if osc_code(code)? != 52 {
        return None;
    }

    let Some((selection_field, payload_parts)) = rest.split_first() else {
        return Some(OscClipboardAction::Ignore);
    };
    let payload = join_osc_semicolon_parts(payload_parts);
    if payload.is_empty() || payload == b"?" || payload.len() > MAX_OSC52_ENCODED_BYTES {
        return Some(OscClipboardAction::Ignore);
    }

    let Some(selection) = osc52_selection(selection_field) else {
        return Some(OscClipboardAction::Ignore);
    };
    let Ok(decoded) = BASE64_STANDARD.decode(&payload) else {
        return Some(OscClipboardAction::Ignore);
    };
    if decoded.len() > MAX_OSC52_DECODED_BYTES {
        return Some(OscClipboardAction::Ignore);
    }

    let decoded_bytes = decoded.len();
    let Ok(text) = String::from_utf8(decoded) else {
        return Some(OscClipboardAction::Ignore);
    };
    if !osc52_text_is_allowed(&text) {
        return Some(OscClipboardAction::Ignore);
    }

    Some(OscClipboardAction::Write(TerminalClipboardWrite {
        selection,
        text,
        decoded_bytes,
    }))
}

fn osc_shell_integration(params: &[&[u8]]) -> Option<OscShellIntegrationMarker> {
    let (code, rest) = params.split_first()?;
    if osc_code(code)? != 133 {
        return None;
    }

    let marker = match osc_text_field(rest.first()?)? {
        "A" => TerminalShellIntegrationMarker::PromptStart,
        "B" => TerminalShellIntegrationMarker::CommandStart,
        "C" => TerminalShellIntegrationMarker::OutputStart,
        "D" => TerminalShellIntegrationMarker::CommandFinished,
        _ => return None,
    };
    let exit_code = (marker == TerminalShellIntegrationMarker::CommandFinished)
        .then(|| {
            rest.get(1)
                .and_then(|field| osc_text_field(field))
                .and_then(|field| field.parse::<i32>().ok())
        })
        .flatten();

    Some(OscShellIntegrationMarker { marker, exit_code })
}

fn osc52_selection(selection_field: &[u8]) -> Option<TerminalClipboardSelection> {
    let selection = std::str::from_utf8(selection_field).ok()?;
    if selection.is_empty() || selection.bytes().any(|byte| byte == b'c') {
        return Some(TerminalClipboardSelection::Clipboard);
    }
    (selection == "p").then_some(TerminalClipboardSelection::Primary)
}

fn osc52_text_is_allowed(text: &str) -> bool {
    text.chars()
        .all(|ch| matches!(ch, '\t' | '\n' | '\r') || !ch.is_control())
}

fn join_osc_semicolon_parts(parts: &[&[u8]]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            bytes.push(b';');
        }
        bytes.extend_from_slice(part);
    }
    bytes
}

fn decode_osc_text(bytes: &[u8], max_bytes: usize) -> Option<String> {
    if bytes.len() > max_bytes {
        return None;
    }
    let text = String::from_utf8_lossy(bytes).into_owned();
    (!text.chars().any(char::is_control)).then_some(text)
}

fn osc8_id(params_field: &[u8]) -> Option<String> {
    let params = decode_osc_text(params_field, MAX_OSC8_ID_BYTES)?;
    params
        .split(':')
        .filter_map(|param| param.strip_prefix("id="))
        .find(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn osc_code(bytes: &[u8]) -> Option<u16> {
    std::str::from_utf8(bytes).ok()?.parse().ok()
}

fn osc_palette_index(bytes: &[u8]) -> Option<u8> {
    u8::try_from(osc_code(bytes)?).ok()
}

fn is_osc_query_field(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok_and(|text| text.trim() == "?")
}

fn osc_text_field(bytes: &[u8]) -> Option<&str> {
    std::str::from_utf8(bytes)
        .ok()
        .filter(|text| !text.chars().any(char::is_control))
}

fn osc_rgb(color: Rgba) -> String {
    format!(
        "rgb:{:04x}/{:04x}/{:04x}",
        u16::from(color.r) * 257,
        u16::from(color.g) * 257,
        u16::from(color.b) * 257
    )
}

fn osc_color(bytes: &[u8]) -> Option<Rgba> {
    let text = std::str::from_utf8(bytes).ok()?.trim();
    if text == "?" {
        return None;
    }

    if let Some(hex) = text.strip_prefix('#') {
        return parse_hex_color(hex);
    }

    if text
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("rgb:"))
    {
        return parse_rgb_color(text.get(4..)?);
    }

    None
}

fn parse_hex_color(hex: &str) -> Option<Rgba> {
    let component_len = match hex.len() {
        3 | 6 | 9 | 12 => hex.len() / 3,
        _ => return None,
    };
    Some(Rgba::rgb(
        parse_hex_component(&hex[0..component_len])?,
        parse_hex_component(&hex[component_len..component_len * 2])?,
        parse_hex_component(&hex[component_len * 2..component_len * 3])?,
    ))
}

fn parse_rgb_color(rgb: &str) -> Option<Rgba> {
    let mut parts = rgb.split('/');
    let red = parse_hex_component(parts.next()?)?;
    let green = parse_hex_component(parts.next()?)?;
    let blue = parse_hex_component(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some(Rgba::rgb(red, green, blue))
}

fn parse_hex_component(hex: &str) -> Option<u8> {
    if hex.is_empty() || hex.len() > 4 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }

    let value = u32::from_str_radix(hex, 16).ok()?;
    let max = (1u32 << (hex.len() * 4)) - 1;
    Some(((value * 255 + max / 2) / max) as u8)
}

fn sanitize_title(title: &str) -> String {
    title.chars().filter(|ch| !ch.is_control()).collect()
}

fn visible_hyperlinks(
    rows: &[Vec<BasicCell>],
    hyperlinks: &[TerminalHyperlink],
) -> Vec<TerminalHyperlink> {
    let referenced = rows
        .iter()
        .flat_map(|row| row.iter().filter_map(|cell| cell.hyperlink))
        .collect::<BTreeSet<_>>();

    hyperlinks
        .iter()
        .filter(|link| referenced.contains(&link.id))
        .cloned()
        .collect()
}

fn sgr_param_groups(params: &Params) -> Vec<Vec<u16>> {
    if params.is_empty() {
        return Vec::new();
    }

    params
        .iter()
        .map(|values| {
            if values.is_empty() {
                vec![0]
            } else {
                values.to_vec()
            }
        })
        .collect()
}

fn sgr_group_code(group: &[u16]) -> u16 {
    group.first().copied().unwrap_or(0)
}

fn extended_sgr_color_from_subparams(values: &[u16]) -> Option<CellColor> {
    match values {
        [2, _color_space, red, green, blue, ..] => Some(sgr_truecolor(*red, *green, *blue)),
        [2, red, green, blue, ..] => Some(sgr_truecolor(*red, *green, *blue)),
        [5, index, ..] => u8::try_from(*index).ok().map(CellColor::Indexed),
        _ => None,
    }
}

fn sgr_truecolor(red: u16, green: u16, blue: u16) -> CellColor {
    CellColor::Direct(Rgba::rgb(
        sgr_rgb_component(red),
        sgr_rgb_component(green),
        sgr_rgb_component(blue),
    ))
}

fn sgr_rgb_component(value: u16) -> u8 {
    value.min(255) as u8
}

fn default_ansi_color(index: u8) -> Rgba {
    const ANSI_COLORS: [Rgba; 16] = [
        Rgba::rgb(0, 0, 0),
        Rgba::rgb(205, 0, 0),
        Rgba::rgb(0, 205, 0),
        Rgba::rgb(205, 205, 0),
        Rgba::rgb(0, 0, 238),
        Rgba::rgb(205, 0, 205),
        Rgba::rgb(0, 205, 205),
        Rgba::rgb(229, 229, 229),
        Rgba::rgb(127, 127, 127),
        Rgba::rgb(255, 0, 0),
        Rgba::rgb(0, 255, 0),
        Rgba::rgb(255, 255, 0),
        Rgba::rgb(92, 92, 255),
        Rgba::rgb(255, 0, 255),
        Rgba::rgb(0, 255, 255),
        Rgba::rgb(255, 255, 255),
    ];

    ANSI_COLORS
        .get(index as usize)
        .copied()
        .unwrap_or(CellStyle::default().foreground)
}

fn default_ansi_256_color(index: u8) -> Rgba {
    match index {
        0..=15 => default_ansi_color(index),
        16..=231 => {
            let cube_index = index - 16;
            let red = cube_index / 36;
            let green = (cube_index % 36) / 6;
            let blue = cube_index % 6;
            Rgba::rgb(
                color_cube_component(red),
                color_cube_component(green),
                color_cube_component(blue),
            )
        }
        232..=255 => {
            let value = 8 + (index - 232) * 10;
            Rgba::rgb(value, value, value)
        }
    }
}

fn color_cube_component(index: u8) -> u8 {
    match index {
        0 => 0,
        1 => 95,
        2 => 135,
        3 => 175,
        4 => 215,
        _ => 255,
    }
}

fn blank_cells(size: GridSize) -> Vec<Vec<BasicCell>> {
    (0..size.rows).map(|_| blank_row(size.cols)).collect()
}

fn allocate_row_anchor(next_row_anchor: &mut u64) -> u64 {
    let anchor = *next_row_anchor;
    *next_row_anchor = next_row_anchor.saturating_add(1);
    anchor
}

fn allocate_row_anchors(rows: u16, next_row_anchor: &mut u64) -> Vec<u64> {
    (0..rows)
        .map(|_| allocate_row_anchor(next_row_anchor))
        .collect()
}

fn rotate_row_anchors_left_and_replace_tail(
    row_anchors: &mut [u64],
    next_row_anchor: &mut u64,
    start: usize,
    end: usize,
    count: usize,
) {
    if start >= end || count == 0 {
        return;
    }
    let end = end.min(row_anchors.len());
    let count = count.min(end.saturating_sub(start));
    row_anchors[start..end].rotate_left(count);
    for anchor in &mut row_anchors[end - count..end] {
        *anchor = allocate_row_anchor(next_row_anchor);
    }
}

fn rotate_row_anchors_right_and_replace_head(
    row_anchors: &mut [u64],
    next_row_anchor: &mut u64,
    start: usize,
    end: usize,
    count: usize,
) {
    if start >= end || count == 0 {
        return;
    }
    let end = end.min(row_anchors.len());
    let count = count.min(end.saturating_sub(start));
    row_anchors[start..end].rotate_right(count);
    for anchor in &mut row_anchors[start..start + count] {
        *anchor = allocate_row_anchor(next_row_anchor);
    }
}

fn default_tab_stops(size: GridSize) -> BTreeSet<u16> {
    (8..size.cols).step_by(8).collect()
}

fn blank_row(cols: u16) -> Vec<BasicCell> {
    (0..cols).map(|_| BasicCell::default()).collect()
}

fn ordered_cell_range(range: CellRange) -> CellRange {
    if (range.end.row, range.end.col) < (range.start.row, range.start.col) {
        CellRange {
            start: range.end,
            end: range.start,
        }
    } else {
        range
    }
}

fn selected_row_text(row: &[BasicCell], start_col: u16, end_col: u16) -> String {
    let Some(max_col) = row.len().checked_sub(1) else {
        return String::new();
    };
    let start_col = usize::from(start_col).min(max_col);
    let end_col = usize::from(end_col).min(max_col);
    if end_col < start_col {
        return String::new();
    }

    let searchable = searchable_row(row);
    let search_row = SearchTextRow::with_columns(
        SearchRowId::screen(0),
        None,
        searchable.text,
        searchable.columns,
    );
    grapheme_cluster_spans(&search_row.text)
        .into_iter()
        .filter_map(|span| {
            let (cluster_start_col, cluster_end_col) = search_row.cell_range_for_char_range(
                span.char_start,
                span.char_end_exclusive.saturating_sub(1),
            )?;
            let intersects = usize::from(cluster_start_col) <= end_col
                && usize::from(cluster_end_col) >= start_col;
            intersects.then(|| {
                search_row
                    .text
                    .chars()
                    .skip(span.char_start)
                    .take(span.char_end_exclusive - span.char_start)
                    .collect::<String>()
            })
        })
        .collect::<String>()
        .trim_end_matches(' ')
        .to_owned()
}

fn row_text_for_col_range(row: &[BasicCell], start_col: u16, end_col_exclusive: u16) -> String {
    let start_col = usize::from(start_col).min(row.len());
    let end_col_exclusive = usize::from(end_col_exclusive).min(row.len());
    if end_col_exclusive <= start_col {
        return String::new();
    }

    selected_row_text(
        row,
        u16::try_from(start_col).unwrap_or(u16::MAX),
        u16::try_from(end_col_exclusive.saturating_sub(1)).unwrap_or(u16::MAX),
    )
}

fn text_from_consecutive_rows(
    rows: &[&[BasicCell]],
    start_col: u16,
    end_col_exclusive: u16,
) -> String {
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            let row_start_col = if index == 0 { start_col } else { 0 };
            let row_end_col_exclusive = if index + 1 == rows.len() {
                end_col_exclusive
            } else {
                u16::try_from(row.len()).unwrap_or(u16::MAX)
            };
            row_text_for_col_range(row, row_start_col, row_end_col_exclusive)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn cell_end_exclusive(row: &[BasicCell], col: usize) -> usize {
    let width = row
        .get(col)
        .map(|cell| usize::from(cell.width.max(1)))
        .unwrap_or(1);
    col.saturating_add(width).min(row.len())
}

fn cell_span_intersects(
    row: &[BasicCell],
    cell_col: usize,
    start_col: usize,
    end_col_exclusive: usize,
) -> bool {
    cell_col < end_col_exclusive && cell_end_exclusive(row, cell_col) > start_col
}

fn cell_start_for_col(row: &[BasicCell], col: usize) -> Option<usize> {
    let max_col = row.len().checked_sub(1)?;
    let col = col.min(max_col);
    if row[col].width > 0 {
        return Some(col);
    }

    (0..col)
        .rev()
        .find(|candidate| row[*candidate].width > 1 && cell_end_exclusive(row, *candidate) > col)
}

fn edit_start_col(row: &[BasicCell], col: usize) -> usize {
    if col >= row.len() {
        row.len()
    } else {
        cell_start_for_col(row, col).unwrap_or(col)
    }
}

fn expanded_cell_edit_range(
    row: &[BasicCell],
    start_col: usize,
    end_col_exclusive: usize,
) -> Option<(usize, usize)> {
    if row.is_empty() {
        return None;
    }

    let start_col = start_col.min(row.len());
    let end_col_exclusive = end_col_exclusive.min(row.len());
    if start_col >= end_col_exclusive {
        return None;
    }

    let mut expanded_start = usize::MAX;
    let mut expanded_end = 0usize;
    for (col, cell) in row.iter().enumerate() {
        if cell.width == 0 {
            continue;
        }
        if cell_span_intersects(row, col, start_col, end_col_exclusive) {
            expanded_start = expanded_start.min(col);
            expanded_end = expanded_end.max(cell_end_exclusive(row, col));
        }
    }

    (expanded_start != usize::MAX).then_some((expanded_start, expanded_end))
}

fn previous_cell_start(row: &[BasicCell], col: usize) -> Option<usize> {
    if col == 0 {
        return None;
    }
    (0..col).rev().find(|candidate| row[*candidate].width > 0)
}

fn next_cell_start(row: &[BasicCell], col: usize) -> Option<usize> {
    (cell_end_exclusive(row, col)..row.len()).find(|candidate| row[*candidate].width > 0)
}

fn repair_wide_cells(row: &mut [BasicCell], blank_cell: BasicCell) {
    let mut col = 0usize;
    while col < row.len() {
        let width = row[col].width;
        if width == 0 {
            row[col] = blank_cell.clone();
            col += 1;
            continue;
        }

        let width = usize::from(width);
        if width <= 1 {
            col += 1;
            continue;
        }

        if col + width > row.len() {
            row[col] = blank_cell.clone();
            col += 1;
            continue;
        }

        let style = row[col].style.clone();
        let hyperlink = row[col].hyperlink;
        let protected = row[col].protected;
        for offset in 1..width {
            row[col + offset] = BasicCell {
                text: String::new(),
                width: 0,
                style: style.clone(),
                hyperlink,
                protected,
            };
        }
        col += width;
    }
}

#[derive(Debug, Eq, PartialEq)]
struct SearchableRow {
    text: String,
    columns: Vec<SearchTextColumn>,
}

fn searchable_row(row: &[BasicCell]) -> SearchableRow {
    let Some(end_col) = row
        .iter()
        .rposition(|cell| !cell.text.is_empty() && cell.text != " ")
    else {
        return SearchableRow {
            text: String::new(),
            columns: Vec::new(),
        };
    };

    let mut text = String::new();
    let mut columns = Vec::new();
    for (col, cell) in row.iter().enumerate().take(end_col + 1) {
        if cell.width == 0 {
            continue;
        }
        let span = SearchTextColumn::new(
            col as u16,
            col.saturating_add(usize::from(cell.width).saturating_sub(1)) as u16,
        );
        for ch in cell.text.chars() {
            text.push(ch);
            columns.push(span);
        }
    }

    let default_columns = columns
        .iter()
        .enumerate()
        .all(|(index, span)| span.start_col == index as u16 && span.end_col == index as u16);
    if default_columns {
        columns.clear();
    }

    SearchableRow { text, columns }
}

fn visible_row_for_logical_index(
    logical_index: usize,
    visible_start: usize,
    visible_end: usize,
) -> Option<u16> {
    if (visible_start..visible_end).contains(&logical_index) {
        u16::try_from(logical_index - visible_start).ok()
    } else {
        None
    }
}

fn is_word_cell(cell: &BasicCell) -> bool {
    !cell.text.is_empty()
        && cell.text.chars().any(is_terminal_word_char)
        && cell
            .text
            .chars()
            .all(|ch| is_terminal_word_char(ch) || terminal_char_width(ch) == 0)
}

fn is_terminal_word_char(ch: char) -> bool {
    ch.is_alphanumeric()
        || matches!(
            ch,
            '_' | '-' | '.' | '/' | '\\' | ':' | '@' | '~' | '+' | '=' | '%' | '$'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_printable_text_and_newline() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.feed(b"hi\r\nok");
        let snapshot = terminal.snapshot();

        assert_eq!(snapshot.rows[0].cells[0].text, "h");
        assert_eq!(snapshot.rows[0].cells[1].text, "i");
        assert_eq!(snapshot.rows[1].cells[0].text, "o");
        assert_eq!(snapshot.rows[1].cells[1].text, "k");
        assert_eq!(snapshot.damage, DamageRegion::Rows(vec![0, 1]));
    }

    #[test]
    fn sgr_applies_basic_colors_and_flags() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 8));

        terminal.feed(b"\x1b[31;44;1;2;3;4;5;8;9;53mX\x1b[22;23;24;25;28;29;39;49;55mY");
        let snapshot = terminal.snapshot();
        let styled = &snapshot.rows[0].cells[0].style;
        let reset = &snapshot.rows[0].cells[1].style;

        assert_eq!(styled.foreground, Rgba::rgb(205, 0, 0));
        assert_eq!(styled.background, Rgba::rgb(0, 0, 238));
        assert!(styled.flags.bold);
        assert!(styled.flags.faint);
        assert!(styled.flags.italic);
        assert!(styled.flags.underline);
        assert!(styled.flags.blink);
        assert!(styled.flags.conceal);
        assert!(styled.flags.strike);
        assert!(styled.flags.overline);
        assert_eq!(reset, &CellStyle::default());
    }

    #[test]
    fn sgr_fast_blink_sets_blink_and_22_clears_bold_and_faint() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"\x1b[1;2;6mA\x1b[22mB\x1b[25mC");
        let snapshot = terminal.snapshot();

        assert!(snapshot.rows[0].cells[0].style.flags.bold);
        assert!(snapshot.rows[0].cells[0].style.flags.faint);
        assert!(snapshot.rows[0].cells[0].style.flags.blink);
        assert!(!snapshot.rows[0].cells[1].style.flags.bold);
        assert!(!snapshot.rows[0].cells[1].style.flags.faint);
        assert!(snapshot.rows[0].cells[1].style.flags.blink);
        assert!(!snapshot.rows[0].cells[2].style.flags.blink);
    }

    #[test]
    fn sgr_applies_underline_style_variants() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal
            .feed(b"\x1b[4mA\x1b[4:2mB\x1b[4:3mC\x1b[4:4mD\x1b[4:5mE\x1b[4:0mF\x1b[21mG\x1b[24mH");
        let snapshot = terminal.snapshot();

        let flags = |col: usize| snapshot.rows[0].cells[col].style.flags;
        assert!(flags(0).underline);
        assert_eq!(flags(0).underline_style, UnderlineStyle::Single);
        assert!(flags(1).underline);
        assert_eq!(flags(1).underline_style, UnderlineStyle::Double);
        assert!(flags(2).underline);
        assert_eq!(flags(2).underline_style, UnderlineStyle::Curly);
        assert!(flags(3).underline);
        assert_eq!(flags(3).underline_style, UnderlineStyle::Dotted);
        assert!(flags(4).underline);
        assert_eq!(flags(4).underline_style, UnderlineStyle::Dashed);
        assert!(!flags(5).underline);
        assert_eq!(flags(5).underline_style, UnderlineStyle::Single);
        assert!(flags(6).underline);
        assert_eq!(flags(6).underline_style, UnderlineStyle::Double);
        assert!(!flags(7).underline);
        assert_eq!(flags(7).underline_style, UnderlineStyle::Single);
    }

    #[test]
    fn sgr_applies_underline_color_variants() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b[4:2;58;5;9mA\x1b[58:2::1:2:3mB\x1b[59mC");
        let snapshot = terminal.snapshot();

        assert_eq!(
            snapshot.rows[0].cells[0].style.underline_color,
            Some(Rgba::rgb(255, 0, 0))
        );
        assert_eq!(
            snapshot.rows[0].cells[1].style.underline_color,
            Some(Rgba::rgb(1, 2, 3))
        );
        assert_eq!(snapshot.rows[0].cells[2].style.underline_color, None);
        assert!(snapshot.rows[0].cells[2].style.flags.underline);
        assert_eq!(
            snapshot.rows[0].cells[2].style.flags.underline_style,
            UnderlineStyle::Double
        );
    }

    #[test]
    fn private_final_m_csi_does_not_apply_sgr() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b[>4;1mA\x1b[4mB\x1b[mC");
        let snapshot = terminal.snapshot();

        assert_eq!(snapshot.rows[0].cells[0].text, "A");
        assert_eq!(snapshot.rows[0].cells[0].style, CellStyle::default());
        assert!(snapshot.rows[0].cells[1].style.flags.underline);
        assert_eq!(
            snapshot.rows[0].cells[1].style.flags.underline_style,
            UnderlineStyle::Single
        );
        assert_eq!(snapshot.rows[0].cells[2].style, CellStyle::default());
    }

    #[test]
    fn sgr_applies_frame_and_encircle_flags() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b[51mA\x1b[52mB\x1b[54mC");
        let snapshot = terminal.snapshot();

        assert!(snapshot.rows[0].cells[0].style.flags.framed);
        assert!(!snapshot.rows[0].cells[0].style.flags.encircled);
        assert!(!snapshot.rows[0].cells[1].style.flags.framed);
        assert!(snapshot.rows[0].cells[1].style.flags.encircled);
        assert!(!snapshot.rows[0].cells[2].style.flags.framed);
        assert!(!snapshot.rows[0].cells[2].style.flags.encircled);
    }

    #[test]
    fn sgr_applies_superscript_and_subscript_flags() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b[73mA\x1b[74mB\x1b[75mC");
        let snapshot = terminal.snapshot();

        assert_eq!(
            snapshot.rows[0].cells[0].style.flags.baseline_shift,
            BaselineShift::Superscript
        );
        assert_eq!(
            snapshot.rows[0].cells[1].style.flags.baseline_shift,
            BaselineShift::Subscript
        );
        assert_eq!(
            snapshot.rows[0].cells[2].style.flags.baseline_shift,
            BaselineShift::Normal
        );
    }

    #[test]
    fn sgr_conceal_flag_preserves_cell_text_for_selection() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"\x1b[8mX\x1b[28mY");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "XY      ");
        assert!(snapshot.rows[0].cells[0].style.flags.conceal);
        assert!(!snapshot.rows[0].cells[1].style.flags.conceal);
    }

    #[test]
    fn sgr_reset_without_params_restores_default_style() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 8));

        terminal.feed(b"\x1b[32mG\x1b[mD");
        let snapshot = terminal.snapshot();

        assert_eq!(
            snapshot.rows[0].cells[0].style.foreground,
            Rgba::rgb(0, 205, 0)
        );
        assert_eq!(snapshot.rows[0].cells[1].style, CellStyle::default());
    }

    #[test]
    fn sgr_applies_truecolor_and_256_color() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 8));

        terminal.feed(b"\x1b[38;2;1;2;3;48;2;4;5;6mT\x1b[38;5;196;48;5;244mC");
        let snapshot = terminal.snapshot();
        let truecolor = &snapshot.rows[0].cells[0].style;
        let indexed = &snapshot.rows[0].cells[1].style;

        assert_eq!(truecolor.foreground, Rgba::rgb(1, 2, 3));
        assert_eq!(truecolor.background, Rgba::rgb(4, 5, 6));
        assert_eq!(indexed.foreground, Rgba::rgb(255, 0, 0));
        assert_eq!(indexed.background, Rgba::rgb(128, 128, 128));
    }

    #[test]
    fn sgr_applies_colon_truecolor_and_256_color() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"\x1b[38:2::1:2:3;48:2:4:5:6mT");
        terminal.feed(b"\x1b[38:5:196;48:5:244mC");
        let snapshot = terminal.snapshot();
        let truecolor = &snapshot.rows[0].cells[0].style;
        let indexed = &snapshot.rows[0].cells[1].style;

        assert_eq!(truecolor.foreground, Rgba::rgb(1, 2, 3));
        assert_eq!(truecolor.background, Rgba::rgb(4, 5, 6));
        assert_eq!(indexed.foreground, Rgba::rgb(255, 0, 0));
        assert_eq!(indexed.background, Rgba::rgb(128, 128, 128));
    }

    #[test]
    fn sgr_semicolon_truecolor_keeps_following_codes_separate() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"\x1b[38;2;0;1;2;3mT");
        let snapshot = terminal.snapshot();
        let style = &snapshot.rows[0].cells[0].style;

        assert_eq!(style.foreground, Rgba::rgb(0, 1, 2));
        assert!(style.flags.italic);
    }

    #[test]
    fn sgr_reverse_swaps_cell_colors_for_rendering() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 8));

        terminal.feed(b"\x1b[31;44;7mR\x1b[27mN");
        let snapshot = terminal.snapshot();
        let reversed = &snapshot.rows[0].cells[0].style;
        let normal = &snapshot.rows[0].cells[1].style;

        assert_eq!(reversed.foreground, Rgba::rgb(0, 0, 238));
        assert_eq!(reversed.background, Rgba::rgb(205, 0, 0));
        assert!(reversed.flags.reverse);
        assert_eq!(normal.foreground, Rgba::rgb(205, 0, 0));
        assert_eq!(normal.background, Rgba::rgb(0, 0, 238));
        assert!(!normal.flags.reverse);
    }

    #[test]
    fn parse_terminal_color_accepts_xterm_color_syntax() {
        assert_eq!(
            parse_terminal_color("#abc"),
            Some(Rgba::rgb(0xaa, 0xbb, 0xcc))
        );
        assert_eq!(
            parse_terminal_color("#112233"),
            Some(Rgba::rgb(0x11, 0x22, 0x33))
        );
        assert_eq!(
            parse_terminal_color("rgb:1111/2222/3333"),
            Some(Rgba::rgb(0x11, 0x22, 0x33))
        );
        assert_eq!(parse_terminal_color("blue"), None);
    }

    #[test]
    fn configured_color_theme_applies_default_indexed_and_cursor_colors() {
        let mut theme = TerminalColorTheme::default();
        theme.foreground = Rgba::rgb(1, 2, 3);
        theme.background = Rgba::rgb(4, 5, 6);
        theme.cursor_color = Some(Rgba::rgb(7, 8, 9));
        theme.palette[1] = Rgba::rgb(0x11, 0x22, 0x33);

        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));
        terminal.set_color_theme(theme);
        terminal.feed(b"D\x1b[31mR\x1b[mN");
        let snapshot = terminal.snapshot();

        assert_eq!(snapshot.default_background, Rgba::rgb(4, 5, 6));
        assert_eq!(snapshot.cursor_color, Some(Rgba::rgb(7, 8, 9)));
        assert_eq!(
            snapshot.rows[0].cells[0].style.foreground,
            Rgba::rgb(1, 2, 3)
        );
        assert_eq!(
            snapshot.rows[0].cells[0].style.background,
            Rgba::rgb(4, 5, 6)
        );
        assert_eq!(
            snapshot.rows[0].cells[1].style.foreground,
            Rgba::rgb(0x11, 0x22, 0x33)
        );
        assert_eq!(
            snapshot.rows[0].cells[2].style.foreground,
            Rgba::rgb(1, 2, 3)
        );
    }

    #[test]
    fn color_control_resets_restore_configured_theme() {
        let mut theme = TerminalColorTheme::default();
        theme.foreground = Rgba::rgb(1, 2, 3);
        theme.background = Rgba::rgb(4, 5, 6);
        theme.cursor_color = Some(Rgba::rgb(7, 8, 9));
        theme.palette[1] = Rgba::rgb(0x11, 0x22, 0x33);

        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));
        terminal.set_color_theme(theme);
        terminal.feed(
            b"\x1b]4;1;#445566\x1b\\\
              \x1b]10;#0a0b0c\x1b\\\
              \x1b]11;#0d0e0f\x1b\\\
              \x1b]12;#101112\x1b\\",
        );
        terminal.feed(b"\x1b]104\x1b\\\x1b]110\x1b\\\x1b]111\x1b\\\x1b]112\x1b\\\x1b[31mR\x1b[mD");
        let snapshot = terminal.snapshot();

        assert_eq!(
            snapshot.rows[0].cells[0].style.foreground,
            Rgba::rgb(0x11, 0x22, 0x33)
        );
        assert_eq!(
            snapshot.rows[0].cells[1].style.foreground,
            Rgba::rgb(1, 2, 3)
        );
        assert_eq!(
            snapshot.rows[0].cells[1].style.background,
            Rgba::rgb(4, 5, 6)
        );
        assert_eq!(snapshot.cursor_color, Some(Rgba::rgb(7, 8, 9)));
    }

    #[test]
    fn full_reset_restores_configured_color_theme() {
        let mut theme = TerminalColorTheme::default();
        theme.foreground = Rgba::rgb(1, 2, 3);
        theme.background = Rgba::rgb(4, 5, 6);
        theme.cursor_color = Some(Rgba::rgb(7, 8, 9));
        theme.palette[1] = Rgba::rgb(0x11, 0x22, 0x33);

        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));
        terminal.set_color_theme(theme);
        terminal.feed(b"\x1b]4;1;#445566\x1b\\\x1b]10;#0a0b0c\x1b\\\x1b]11;#0d0e0f\x1b\\\x1b]12;#101112\x1b\\");
        terminal.feed(b"\x1bc\x1b[31mR\x1b[mD");
        let snapshot = terminal.snapshot();

        assert_eq!(
            snapshot.rows[0].cells[0].style.foreground,
            Rgba::rgb(0x11, 0x22, 0x33)
        );
        assert_eq!(
            snapshot.rows[0].cells[1].style.foreground,
            Rgba::rgb(1, 2, 3)
        );
        assert_eq!(
            snapshot.rows[0].cells[1].style.background,
            Rgba::rgb(4, 5, 6)
        );
        assert_eq!(snapshot.cursor_color, Some(Rgba::rgb(7, 8, 9)));
    }

    #[test]
    fn osc4_updates_palette_for_future_sgr_indexed_colors() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"\x1b]4;1;rgb:12/34/56;9;#abcdef\x1b\\");
        terminal.feed(b"\x1b[31mR\x1b[91mB\x1b[38;5;1mI\x1b[38;5;196mX");
        let snapshot = terminal.snapshot();

        assert_eq!(
            snapshot.rows[0].cells[0].style.foreground,
            Rgba::rgb(0x12, 0x34, 0x56)
        );
        assert_eq!(
            snapshot.rows[0].cells[1].style.foreground,
            Rgba::rgb(0xab, 0xcd, 0xef)
        );
        assert_eq!(
            snapshot.rows[0].cells[2].style.foreground,
            Rgba::rgb(0x12, 0x34, 0x56)
        );
        assert_eq!(
            snapshot.rows[0].cells[3].style.foreground,
            Rgba::rgb(255, 0, 0)
        );
    }

    #[test]
    fn osc4_palette_query_reports_builtin_color() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b]4;1;?\x1b\\\x1b[31mR");
        let snapshot = terminal.snapshot();

        assert_eq!(
            terminal.drain_host_actions(),
            vec![terminal_reply(b"\x1b]4;1;rgb:cdcd/0000/0000\x1b\\")]
        );
        assert_eq!(
            snapshot.rows[0].cells[0].style.foreground,
            Rgba::rgb(205, 0, 0)
        );
    }

    #[test]
    fn osc4_palette_query_reports_current_palette_color_without_rendering_text() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"\x1b]4;1;#112233;2;?\x1b\\visible");
        terminal.feed(b"\x1b]4;1;?;2;?\x1b\\");
        let snapshot = terminal.snapshot();

        assert_eq!(
            terminal.drain_host_actions(),
            vec![
                terminal_reply(b"\x1b]4;2;rgb:0000/cdcd/0000\x1b\\"),
                terminal_reply(b"\x1b]4;1;rgb:1111/2222/3333\x1b\\"),
                terminal_reply(b"\x1b]4;2;rgb:0000/cdcd/0000\x1b\\"),
            ]
        );
        assert_eq!(row_text(&snapshot, 0).trim_end(), "visible");
    }

    #[test]
    fn osc104_resets_palette_slots_and_repaints_indexed_cells() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b]4;1;#112233\x1b\\\x1b[31mA");
        terminal.feed(b"\x1b]104;1\x1b\\\x1b[31mB");
        let snapshot = terminal.snapshot();

        assert_eq!(
            snapshot.rows[0].cells[0].style.foreground,
            Rgba::rgb(205, 0, 0)
        );
        assert_eq!(
            snapshot.rows[0].cells[1].style.foreground,
            Rgba::rgb(205, 0, 0)
        );
    }

    #[test]
    fn osc4_palette_updates_repaint_already_written_indexed_cells() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 6));

        terminal.feed(b"\x1b[31mA\x1b[38;5;2mB");
        terminal.take_snapshot();
        terminal.feed(b"\x1b]4;1;#112233;2;#445566\x1b\\");
        let snapshot = terminal.snapshot();

        assert_eq!(
            snapshot.rows[0].cells[0].style.foreground,
            Rgba::rgb(0x11, 0x22, 0x33)
        );
        assert_eq!(
            snapshot.rows[0].cells[1].style.foreground,
            Rgba::rgb(0x44, 0x55, 0x66)
        );
        assert_eq!(snapshot.damage, DamageRegion::Full);
    }

    #[test]
    fn direct_truecolor_cells_ignore_palette_repaint() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b[38;2;1;2;3mT\x1b]4;1;#112233\x1b\\");
        let snapshot = terminal.snapshot();

        assert_eq!(
            snapshot.rows[0].cells[0].style.foreground,
            Rgba::rgb(1, 2, 3)
        );
    }

    #[test]
    fn osc10_and_osc11_update_default_colors_and_repaint_default_cells() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 6));

        terminal.feed(b"\x1b]10;#010203\x1b\\\x1b]11;rgb:04/05/06\x1b\\X");
        terminal.feed(b"\x1b[31;44mY\x1b[39;49mZ");
        terminal.feed(b"\x1b]10;#070809\x1b\\\x1b]11;#0a0b0c\x1b\\");
        let snapshot = terminal.snapshot();

        assert_eq!(
            snapshot.rows[0].cells[0].style.foreground,
            Rgba::rgb(7, 8, 9)
        );
        assert_eq!(
            snapshot.rows[0].cells[0].style.background,
            Rgba::rgb(0x0a, 0x0b, 0x0c)
        );
        assert_eq!(
            snapshot.rows[0].cells[1].style.foreground,
            Rgba::rgb(205, 0, 0)
        );
        assert_eq!(
            snapshot.rows[0].cells[1].style.background,
            Rgba::rgb(0, 0, 238)
        );
        assert_eq!(
            snapshot.rows[0].cells[2].style.foreground,
            Rgba::rgb(7, 8, 9)
        );
        assert_eq!(
            snapshot.rows[0].cells[2].style.background,
            Rgba::rgb(0x0a, 0x0b, 0x0c)
        );
        assert_eq!(snapshot.default_background, Rgba::rgb(0x0a, 0x0b, 0x0c));
    }

    #[test]
    fn osc10_and_osc11_query_report_current_default_colors() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b]10;#010203\x1b\\\x1b]11;rgb:04/05/06\x1b\\");
        terminal.feed(b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![
                terminal_reply(b"\x1b]10;rgb:0101/0202/0303\x1b\\"),
                terminal_reply(b"\x1b]11;rgb:0404/0505/0606\x1b\\"),
            ]
        );
    }

    #[test]
    fn osc12_sets_queries_and_resets_cursor_color() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b]12;#112233\x1b\\\x1b]12;?\x1b\\");
        assert_eq!(
            terminal.snapshot().cursor_color,
            Some(Rgba::rgb(0x11, 0x22, 0x33))
        );
        assert_eq!(
            terminal.drain_host_actions(),
            vec![terminal_reply(b"\x1b]12;rgb:1111/2222/3333\x1b\\")]
        );

        terminal.feed(b"\x1b]112\x1b\\\x1b]12;?\x1b\\");
        assert_eq!(terminal.snapshot().cursor_color, None);
        assert_eq!(
            terminal.drain_host_actions(),
            vec![terminal_reply(b"\x1b]12;rgb:b4b4/b4b4/b4b4\x1b\\")]
        );
    }

    #[test]
    fn cursor_color_changes_damage_only_cursor_row_and_resets() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 4));

        terminal.feed(b"\x1b[2;1H");
        terminal.take_snapshot();
        terminal.feed(b"\x1b]12;#010203\x1b\\");

        let changed = terminal.take_snapshot();
        assert_eq!(changed.cursor_color, Some(Rgba::rgb(1, 2, 3)));
        assert_eq!(changed.damage, DamageRegion::Rows(vec![1]));

        terminal.feed(b"\x1b[!p");
        assert_eq!(terminal.snapshot().cursor_color, None);
    }

    #[test]
    fn full_reset_restores_builtin_palette_and_default_colors() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 6));

        terminal.feed(b"\x1b]4;1;#112233\x1b\\\x1b]10;#010203\x1b\\\x1b]11;#040506\x1b\\\x1b]12;#070809\x1b\\");
        terminal.feed(b"\x1bc\x1b[31mR\x1b[mD");
        let snapshot = terminal.snapshot();

        assert_eq!(
            snapshot.rows[0].cells[0].style.foreground,
            Rgba::rgb(205, 0, 0)
        );
        assert_eq!(snapshot.rows[0].cells[1].style, CellStyle::default());
        assert_eq!(snapshot.cursor_color, None);
    }

    #[test]
    fn decstr_soft_reset_restores_runtime_modes_without_clearing_screen() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 8));

        terminal.feed(
            b"\x1b]2;title\x07\
              \x1b]4;1;#112233\x1b\\\
              \x1b]10;#010203\x1b\\\
              \x1b]11;#040506\x1b\\\
              \x1b]8;;https://example.com\x1b\\\
              \x1b[31mR\
              \x1b[?25l\x1b[5 q\
              \x1b[?1;6h\x1b[?7l\x1b[4h\x1b=\x1b[?2004h",
        );
        terminal.feed(b"\x1b[!p\x1b[1;2HD");
        terminal.feed(b"\x1b[4$p\x1b[?1$p\x1b[?6$p\x1b[?7$p\x1b[?25$p\x1b[?2004$p");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "RD      ");
        assert_eq!(snapshot.title.as_deref(), Some("title"));
        assert_eq!(
            snapshot.rows[0].cells[0].style.foreground,
            Rgba::rgb(205, 0, 0)
        );
        assert_eq!(snapshot.rows[0].cells[1].style, CellStyle::default());
        assert_eq!(
            hyperlink_at(&snapshot, 0, 0),
            Some(snapshot.hyperlinks[0].id)
        );
        assert_eq!(hyperlink_at(&snapshot, 0, 1), None);
        assert_eq!(snapshot.cursor.shape, CursorShape::Block);
        assert!(snapshot.cursor.visible);
        assert!(snapshot.cursor.blink);
        assert!(!terminal.application_cursor_keys_enabled());
        assert!(!terminal.application_keypad_enabled());
        assert!(terminal.bracketed_paste_enabled());
        assert_eq!(
            terminal.drain_host_actions(),
            vec![
                terminal_reply(b"\x1b[4;2$y"),
                terminal_reply(b"\x1b[?1;2$y"),
                terminal_reply(b"\x1b[?6;2$y"),
                terminal_reply(b"\x1b[?7;1$y"),
                terminal_reply(b"\x1b[?25;1$y"),
                terminal_reply(b"\x1b[?2004;1$y"),
            ]
        );
    }

    #[test]
    fn decstr_soft_reset_clears_scroll_region_and_restores_wraparound() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 4));

        terminal.feed(b"A\x1b[2;1HB\x1b[3;1HC");
        terminal.feed(b"\x1b[2;3r\x1b[?7l\x1b[!p\x1b[3;1H\n");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "B   ");
        assert_eq!(row_text(&snapshot, 1), "C   ");
        assert_eq!(row_text(&snapshot, 2), "    ");
        terminal.scroll_viewport_lines(1);
        assert_eq!(row_text(&terminal.snapshot(), 0), "A   ");

        let mut wrap = BasicTerminal::new(GridSize::new(2, 4));
        wrap.feed(b"\x1b[?7l\x1b[!pabcdE");
        let wrapped = wrap.snapshot();

        assert_eq!(row_text(&wrapped, 0), "abcd");
        assert_eq!(row_text(&wrapped, 1), "E   ");
    }

    #[test]
    fn linefeed_newline_mode_tracks_ansi_mode_20() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 4));

        terminal.feed(b"\x1b[20hA\nB\x1b[20lC\nD");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "A   ");
        assert_eq!(row_text(&snapshot, 1), "BC  ");
        assert_eq!(row_text(&snapshot, 2), "  D ");
    }

    #[test]
    fn linefeed_newline_mode_is_cleared_by_soft_and_full_reset() {
        let mut soft = BasicTerminal::new(GridSize::new(2, 4));
        soft.feed(b"\x1b[20h\x1b[!pA\nB");
        assert_eq!(row_text(&soft.snapshot(), 1), " B  ");

        let mut full = BasicTerminal::new(GridSize::new(2, 4));
        full.feed(b"\x1b[20h\x1bcA\nB");
        assert_eq!(row_text(&full.snapshot(), 1), " B  ");
    }

    #[test]
    fn decstr_soft_reset_resets_saved_cursor_to_home() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"abcd\x1b[1;4H\x1b7\x1b[!p\x1b8Z");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "Zbcd ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 1));
    }

    #[test]
    fn decaln_fills_screen_with_default_e_cells_and_preserves_cursor() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 4));

        terminal.feed(b"\x1b[31mab\x1b[2;3H\x1b#8");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "EEEE");
        assert_eq!(row_text(&snapshot, 1), "EEEE");
        assert_eq!(snapshot.cursor.position, CellPoint::new(1, 2));
        assert_eq!(snapshot.rows[0].cells[0].style, CellStyle::default());
        assert_eq!(snapshot.rows[1].cells[3].style, CellStyle::default());
        assert_eq!(snapshot.damage, DamageRegion::Full);
    }

    #[test]
    fn decaln_targets_active_screen_only() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"main\x1b[?1049h\x1b#8");
        assert_eq!(row_text(&terminal.snapshot(), 0), "EEEE");

        terminal.feed(b"\x1b[?1049l");
        assert_eq!(row_text(&terminal.snapshot(), 0), "main");
    }

    #[test]
    fn decaln_uses_esc_intermediate_without_restoring_saved_cursor() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"\x1b[1;2H\x1b7\x1b[1;4H\x1b#8X\x1b8Y");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "EYEXE");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 2));
    }

    #[test]
    fn dec_special_graphics_maps_g0_line_drawing_chars() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 10));

        terminal.feed(b"\x1b(0lqk\x1b(Babc");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "┌─┐abc    ");
    }

    #[test]
    fn dec_special_graphics_uses_so_and_si_for_g1_switching() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"\x1b)0a\x0elqk\x0flqk");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "a┌─┐lqk ");
    }

    #[test]
    fn dec_special_graphics_is_reset_by_decstr() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b(0l\x1b[!pl");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "┌l  ");
    }

    #[test]
    fn dec_special_graphics_maps_g2_and_g3_single_shift_chars() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"\x1b*0\x1b+0a\x1bNl\x1bOqk");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "a┌─k    ");
    }

    #[test]
    fn dec_special_graphics_uses_g2_and_g3_locking_shifts() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"\x1b*0\x1b+0a\x1bnlq\x1bolq");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "a┌─┌─   ");
    }

    #[test]
    fn utf8_mode_selection_sequences_are_noops() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed("A£".as_bytes());
        terminal.feed(b"\x1b%@B\x1b%GC");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "A£BC    ");
    }

    #[test]
    fn utf8_mode_selection_preserves_charset_state() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"\x1b(0\x1b%Gl\x1b%@q\x1b(Bk");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "┌─k     ");
    }

    #[test]
    fn uk_national_charset_maps_number_sign() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"\x1b(A#A\x1b)A\x0e#\x0f\x1b(B#");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "£A£#    ");
    }

    #[test]
    fn uk_national_charset_works_through_g2_g3_and_reset() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"\x1b*A\x1b+A\x1bN#\x1bO#\x1b[!p#");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "££#     ");
    }

    #[test]
    fn g2_g3_locking_shifts_clear_pending_single_shift() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b*0\x1bN\x1bol");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "l   ");
    }

    #[test]
    fn eight_bit_ss2_and_ss3_map_only_next_printable_char() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 6));

        terminal.feed(b"\x1b*0\x1b+0\x8elq\x8fqk");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "┌q─k  ");
    }

    #[test]
    fn decstr_clears_single_shift_and_g2_g3_designations() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b*0\x1bN\x1b[!pl\x1bNl");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "ll  ");
    }

    #[test]
    fn printable_text_reports_dirty_row_damage() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.feed(b"ok");

        assert_eq!(terminal.snapshot().damage, DamageRegion::Rows(vec![0]));
    }

    #[test]
    fn take_snapshot_consumes_damage_epoch() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        assert_eq!(terminal.take_snapshot().damage, DamageRegion::Full);
        assert_eq!(terminal.take_snapshot().damage, DamageRegion::Rows(vec![]));

        terminal.feed(b"ok");
        assert_eq!(terminal.take_snapshot().damage, DamageRegion::Rows(vec![0]));
        assert_eq!(terminal.take_snapshot().damage, DamageRegion::Rows(vec![]));
    }

    #[test]
    fn full_damage_is_cleared_after_consuming_snapshot() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.feed(b"ok");
        terminal.resize(GridSize::new(4, 12));

        assert_eq!(terminal.take_snapshot().damage, DamageRegion::Full);
        assert_eq!(terminal.take_snapshot().damage, DamageRegion::Rows(vec![]));
    }

    #[test]
    fn handles_basic_cursor_position() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.feed(b"\x1b[2;3H!");
        let snapshot = terminal.snapshot();

        assert_eq!(snapshot.rows[1].cells[2].text, "!");
        assert_eq!(snapshot.damage, DamageRegion::Rows(vec![0, 1]));
    }

    #[test]
    fn csi_cursor_next_previous_line_moves_to_column_zero() {
        let mut terminal = BasicTerminal::new(GridSize::new(4, 8));

        terminal.feed(b"\x1b[1;4H\x1b[2EY\x1b[1FX");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 1), "X       ");
        assert_eq!(row_text(&snapshot, 2), "Y       ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(1, 1));
    }

    #[test]
    fn csi_horizontal_and_vertical_absolute_positioning() {
        let mut terminal = BasicTerminal::new(GridSize::new(4, 8));

        terminal.feed(b"abcd\x1b[1GZ\x1b[4GQ\x1b[3dV\x1b[2`H");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "ZbcQ    ");
        assert_eq!(row_text(&snapshot, 2), " H  V   ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(2, 2));
    }

    #[test]
    fn csi_relative_hpr_and_vpr_positioning() {
        let mut terminal = BasicTerminal::new(GridSize::new(4, 8));

        terminal.feed(b"A\x1b[3aB\x1b[2eC");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "A   B   ");
        assert_eq!(row_text(&snapshot, 2), "     C  ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(2, 6));
    }

    #[test]
    fn csi_relative_cursor_zero_params_default_to_one() {
        let mut terminal = BasicTerminal::new(GridSize::new(5, 5));

        terminal.feed(b"\x1b[3;3H\x1b[0AX");
        terminal.feed(b"\x1b[3;3H\x1b[0BY");
        terminal.feed(b"\x1b[3;3H\x1b[0CZ");
        terminal.feed(b"\x1b[3;3H\x1b[0DW");
        terminal.feed(b"\x1b[3;3H\x1b[0aR");
        terminal.feed(b"\x1b[3;3H\x1b[0eV");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 1), "  X  ");
        assert_eq!(row_text(&snapshot, 2), " W R ");
        assert_eq!(row_text(&snapshot, 3), "  V  ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(3, 3));
    }

    #[test]
    fn csi_line_relative_zero_params_default_to_one() {
        let mut terminal = BasicTerminal::new(GridSize::new(5, 5));

        terminal.feed(b"\x1b[3;3H\x1b[0ED");
        terminal.feed(b"\x1b[3;3H\x1b[0FU");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 1), "U    ");
        assert_eq!(row_text(&snapshot, 3), "D    ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(1, 1));
    }

    #[test]
    fn esc_index_and_next_line_controls() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.feed(b"\x1b[1;4H\x1bDX\x1bEX");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 1), "   X    ");
        assert_eq!(row_text(&snapshot, 2), "X       ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(2, 1));
    }

    #[test]
    fn esc_backward_and_forward_index_move_one_column() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"abc\x1b6X\x1b9Y");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "abX Y");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 4));
    }

    #[test]
    fn esc_backward_and_forward_index_clamp_and_clear_pending_wrap() {
        let mut left = BasicTerminal::new(GridSize::new(1, 4));
        left.feed(b"\x1b6X");
        let left_snapshot = left.snapshot();

        assert_eq!(row_text(&left_snapshot, 0), "X   ");
        assert_eq!(left_snapshot.cursor.position, CellPoint::new(0, 1));

        let mut right = BasicTerminal::new(GridSize::new(1, 4));
        right.feed(b"\x1b[1;4H\x1b9X");
        let right_snapshot = right.snapshot();

        assert_eq!(row_text(&right_snapshot, 0), "   X");
        assert_eq!(right_snapshot.cursor.position, CellPoint::new(0, 3));

        let mut pending_wrap = BasicTerminal::new(GridSize::new(2, 4));
        pending_wrap.feed(b"abcd\x1b6E");
        let pending_snapshot = pending_wrap.snapshot();

        assert_eq!(row_text(&pending_snapshot, 0), "abEd");
        assert_eq!(row_text(&pending_snapshot, 1), "    ");
        assert_eq!(pending_snapshot.cursor.position, CellPoint::new(0, 3));
    }

    #[test]
    fn c1_ind_and_nel_alias_esc_controls() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.feed(b"\x1b[1;4H\x84X\x85X");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 1), "   X    ");
        assert_eq!(row_text(&snapshot, 2), "X       ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(2, 1));
    }

    #[test]
    fn horizontal_tab_uses_default_tab_stops() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"a\tb");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "a       b   ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 9));
    }

    #[test]
    fn hts_sets_custom_tab_stop() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"\x1b[1;5H\x1bH\x1b[1;1Ha\tb");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "a   b       ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 5));
    }

    #[test]
    fn c1_hts_alias_sets_custom_tab_stop() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"\x1b[1;5H\x88\x1b[1;1Ha\tb");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "a   b       ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 5));
    }

    #[test]
    fn c1_csi_alias_dispatches_csi_sequences() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"\x9b31mR\x9bmD");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "RD      ");
        assert_eq!(
            snapshot.rows[0].cells[0].style.foreground,
            Rgba::rgb(205, 0, 0)
        );
        assert_eq!(snapshot.rows[0].cells[1].style, CellStyle::default());
    }

    #[test]
    fn c1_osc_and_st_alias_set_title_without_rendering_text() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"\x9d2;Witty\x9cX");
        let snapshot = terminal.snapshot();

        assert_eq!(snapshot.title.as_deref(), Some("Witty"));
        assert_eq!(row_text(&snapshot, 0), "X       ");
    }

    #[test]
    fn c1_st_alias_terminates_split_osc_sequence() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"\x9d2;split");
        terminal.feed(b"\x9cZ");
        let snapshot = terminal.snapshot();

        assert_eq!(snapshot.title.as_deref(), Some("split"));
        assert_eq!(row_text(&snapshot, 0), "Z       ");
    }

    #[test]
    fn c1_dcs_alias_uses_passthrough_string_state() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"A\x90ignored payload\x9cB");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "AB          ");
    }

    #[test]
    fn c1_sos_pm_and_apc_aliases_use_ignored_string_state() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"A\x98sos\x9cB\x9epm\x9cC\x9fapc\x9cD");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "ABCD        ");
    }

    #[test]
    fn c1_sequence_aliases_do_not_corrupt_utf8_text() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed("ĐŘɛɜɝŞş".as_bytes());
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "ĐŘɛɜɝŞş ");
    }

    #[test]
    fn c1_sequence_aliases_do_not_corrupt_split_utf8_text() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(&[0xc4]);
        terminal.feed(&[0x90]);
        terminal.feed(b"X");
        terminal.feed(&[0xc9]);
        terminal.feed(&[0x9b]);
        terminal.feed(b"Y");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "ĐXɛY    ");
    }

    #[test]
    fn utf8_encoded_c1_sequence_aliases_dispatch_controls() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 8));

        terminal.feed(b"old");
        terminal.feed(&[0xc2, 0x9b]);
        terminal.feed(b"2J");
        terminal.feed(&[0xc2, 0x9d]);
        terminal.feed(b"2;utf8-title");
        terminal.feed(&[0xc2, 0x9c]);
        terminal.feed(b"X");
        let snapshot = terminal.snapshot();

        assert_eq!(snapshot.title.as_deref(), Some("utf8-title"));
        assert_eq!(row_text(&snapshot, 0), "   X    ");
        assert_eq!(row_text(&snapshot, 1), "        ");
    }

    #[test]
    fn utf8_encoded_c1_execute_aliases_dispatch_controls() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 8));

        terminal.feed(b"A");
        terminal.feed(&[0xc2, 0x85]);
        terminal.feed(b"B");
        terminal.feed(&[0xc2, 0x8d]);
        terminal.feed(b"C");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "AC      ");
        assert_eq!(row_text(&snapshot, 1), "B       ");
    }

    #[test]
    fn utf8_encoded_c1_aliases_preserve_latin1_text() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(&[0xc2]);
        terminal.feed(&[0xa3]);
        terminal.feed("¿X".as_bytes());
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "£¿X     ");
    }

    #[test]
    fn c1_transmission_mode_sequences_are_noops() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"A\x1b FB\x1b GC");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "ABC     ");
    }

    #[test]
    fn c1_transmission_mode_does_not_disable_supported_aliases() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"old");
        terminal.feed(b"\x1b F\x9b2J\x9bH");
        terminal.feed(b"A");
        terminal.feed(b"\x1b G");
        terminal.feed(&[0xc2, 0x9b]);
        terminal.feed(b"2J");
        terminal.feed(&[0xc2, 0x9b]);
        terminal.feed(b"H");
        terminal.feed(b"B");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "B       ");
    }

    #[test]
    fn tbc_clears_current_and_all_tab_stops() {
        let mut clear_current = BasicTerminal::new(GridSize::new(1, 12));
        clear_current.feed(b"\x1b[1;9H\x1b[g\x1b[1;1Ha\tb");
        assert_eq!(row_text(&clear_current.snapshot(), 0), "a          b");

        let mut clear_all = BasicTerminal::new(GridSize::new(1, 12));
        clear_all.feed(b"\x1b[3g\x1b[1;1Ha\tb");
        assert_eq!(row_text(&clear_all.snapshot(), 0), "a          b");
    }

    #[test]
    fn full_reset_restores_default_tab_stops() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"\x1b[3g\x1bc");
        terminal.feed(b"a\tb");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "a       b   ");
    }

    #[test]
    fn decst8c_resets_default_tab_stops() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"\x1b[3g\x1b[1;5H\x1bH\x1b[?5W\x1b[1;1Ha\tb");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "a       b   ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 9));
    }

    #[test]
    fn cursor_forward_tabulation_uses_default_and_custom_tab_stops() {
        let mut default_stops = BasicTerminal::new(GridSize::new(1, 24));
        default_stops.feed(b"\x1b[1;2H\x1b[I\x1b[2I");
        assert_eq!(
            default_stops.snapshot().cursor.position,
            CellPoint::new(0, 23)
        );

        let mut custom_stops = BasicTerminal::new(GridSize::new(1, 16));
        custom_stops.feed(b"\x1b[3g\x1b[1;5H\x1bH\x1b[1;11H\x1bH");
        custom_stops.feed(b"\x1b[1;1H\x1b[2I");
        assert_eq!(
            custom_stops.snapshot().cursor.position,
            CellPoint::new(0, 10)
        );
    }

    #[test]
    fn cursor_backward_tabulation_uses_default_and_custom_tab_stops() {
        let mut default_stops = BasicTerminal::new(GridSize::new(1, 24));
        default_stops.feed(b"\x1b[1;21H\x1b[Z");
        assert_eq!(
            default_stops.snapshot().cursor.position,
            CellPoint::new(0, 16)
        );
        default_stops.feed(b"\x1b[3Z");
        assert_eq!(
            default_stops.snapshot().cursor.position,
            CellPoint::new(0, 0)
        );

        let mut custom_stops = BasicTerminal::new(GridSize::new(1, 16));
        custom_stops.feed(b"\x1b[3g\x1b[1;5H\x1bH\x1b[1;11H\x1bH");
        custom_stops.feed(b"\x1b[1;14H\x1b[2Z");
        assert_eq!(
            custom_stops.snapshot().cursor.position,
            CellPoint::new(0, 4)
        );
    }

    #[test]
    fn cursor_tabulation_zero_param_defaults_to_one_and_clamps_to_edges() {
        let mut forward = BasicTerminal::new(GridSize::new(1, 10));
        forward.feed(b"\x1b[1;2H\x1b[0I\x1b[0I");
        assert_eq!(forward.snapshot().cursor.position, CellPoint::new(0, 9));

        let mut backward = BasicTerminal::new(GridSize::new(1, 10));
        backward.feed(b"\x1b[1;10H\x1b[0Z\x1b[0Z");
        assert_eq!(backward.snapshot().cursor.position, CellPoint::new(0, 0));
    }

    #[test]
    fn autowrap_waits_until_next_print_after_right_margin() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 4));

        terminal.feed(b"abcd");
        let pending = terminal.snapshot();
        terminal.feed(b"E");
        let wrapped = terminal.snapshot();

        assert_eq!(row_text(&pending, 0), "abcd");
        assert_eq!(pending.cursor.position, CellPoint::new(0, 3));
        assert_eq!(row_text(&wrapped, 0), "abcd");
        assert_eq!(row_text(&wrapped, 1), "E   ");
        assert_eq!(wrapped.cursor.position, CellPoint::new(1, 1));
    }

    #[test]
    fn autowrap_pending_survives_sgr_until_next_print() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 4));

        terminal.feed(b"abcd\x1b[31mE");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "abcd");
        assert_eq!(row_text(&snapshot, 1), "E   ");
        assert_eq!(
            snapshot.rows[1].cells[0].style.foreground,
            Rgba::rgb(205, 0, 0)
        );
    }

    #[test]
    fn disabling_autowrap_overwrites_right_margin() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 4));

        terminal.feed(b"\x1b[?7labcde");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "abce");
        assert_eq!(row_text(&snapshot, 1), "    ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 3));
    }

    #[test]
    fn reenabled_autowrap_wraps_after_right_margin_again() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 4));

        terminal.feed(b"\x1b[?7labcd\x1b[?7hXY");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "abcX");
        assert_eq!(row_text(&snapshot, 1), "Y   ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(1, 1));
    }

    #[test]
    fn full_reset_restores_autowrap_mode() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 4));

        terminal.feed(b"\x1b[?7l\x1bcabcdE");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "abcd");
        assert_eq!(row_text(&snapshot, 1), "E   ");
    }

    #[test]
    fn reverse_wraparound_mode_wraps_backspace_to_previous_row() {
        let mut disabled = BasicTerminal::new(GridSize::new(2, 4));
        disabled.feed(b"\x1b[2;1H\x08");
        assert_eq!(disabled.snapshot().cursor.position, CellPoint::new(1, 0));

        let mut enabled = BasicTerminal::new(GridSize::new(2, 4));
        enabled.feed(b"\x1b[?45h\x1b[2;1H\x08");
        assert_eq!(enabled.snapshot().cursor.position, CellPoint::new(0, 3));

        enabled.feed(b"\x1b[1;1H\x08");
        assert_eq!(enabled.snapshot().cursor.position, CellPoint::new(0, 0));
    }

    #[test]
    fn reverse_wraparound_backspace_clears_pending_wrap() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 4));

        terminal.feed(b"\x1b[?45habcd\x08E");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "abEd");
        assert_eq!(row_text(&snapshot, 1), "    ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 3));
    }

    #[test]
    fn reverse_wraparound_mode_resets_with_soft_and_full_reset() {
        let mut soft = BasicTerminal::new(GridSize::new(2, 4));
        soft.feed(b"\x1b[?45h\x1b[!p\x1b[2;1H\x08");
        assert_eq!(soft.snapshot().cursor.position, CellPoint::new(1, 0));

        let mut full = BasicTerminal::new(GridSize::new(2, 4));
        full.feed(b"\x1b[?45h\x1bc\x1b[2;1H\x08");
        assert_eq!(full.snapshot().cursor.position, CellPoint::new(1, 0));
    }

    #[test]
    fn insert_mode_shifts_printable_cells_right() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"abcd\x1b[1;2H\x1b[4hZ");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "aZbcd");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 2));
    }

    #[test]
    fn replace_mode_overwrites_printable_cells() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"abcd\x1b[1;2HZ");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "aZcd ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 2));
    }

    #[test]
    fn disabling_insert_mode_restores_replace_behavior() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"abcd\x1b[1;2H\x1b[4hZ\x1b[1;2H\x1b[4lY");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "aYbcd");
    }

    #[test]
    fn insert_mode_at_right_margin_drops_final_cell() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"abcd\x1b[1;4H\x1b[4hZ");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "abcZ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 3));
    }

    #[test]
    fn full_reset_restores_replace_mode() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"\x1b[4h\x1bcabcd\x1b[1;2HZ");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "aZcd ");
    }

    #[test]
    fn application_keypad_mode_tracks_decpam_and_decpnm() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        assert!(!terminal.application_keypad_enabled());
        terminal.feed(b"\x1b=");
        assert!(terminal.application_keypad_enabled());
        terminal.feed(b"\x1b>");
        assert!(!terminal.application_keypad_enabled());
    }

    #[test]
    fn application_keypad_mode_tracks_decnkm_private_mode() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"\x1b[?66h");
        assert!(terminal.application_keypad_enabled());
        terminal.feed(b"\x1b[?66l");
        assert!(!terminal.application_keypad_enabled());
    }

    #[test]
    fn application_keypad_mode_survives_alternate_screen_switch() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"\x1b=\x1b[?1049h\x1b[?1049l");

        assert!(terminal.application_keypad_enabled());
    }

    #[test]
    fn full_reset_restores_numeric_keypad_mode() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"\x1b=\x1bc");

        assert!(!terminal.application_keypad_enabled());
    }

    #[test]
    fn application_cursor_keys_track_decckm_private_mode() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        assert!(!terminal.application_cursor_keys_enabled());
        terminal.feed(b"\x1b[?1h");
        assert!(terminal.application_cursor_keys_enabled());
        terminal.feed(b"\x1b[?1l");
        assert!(!terminal.application_cursor_keys_enabled());
    }

    #[test]
    fn application_cursor_keys_survive_alternate_screen_switch() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"\x1b[?1h\x1b[?1049h\x1b[?1049l");

        assert!(terminal.application_cursor_keys_enabled());
    }

    #[test]
    fn full_reset_restores_normal_cursor_keys() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"\x1b[?1h\x1bc");

        assert!(!terminal.application_cursor_keys_enabled());
    }

    #[test]
    fn input_modes_report_cursor_and_keypad_state() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"\x1b[?1h\x1b=\x1b[2h\x1b[?67h");

        assert_eq!(
            terminal.input_modes(),
            TerminalInputModes {
                application_cursor_keys: true,
                application_keypad: true,
                keyboard_locked: true,
                backarrow_sends_backspace: true,
                kitty_keyboard_flags: 0,
                mouse: TerminalMouseModes::default(),
            }
        );
    }

    #[test]
    fn keyboard_action_mode_tracks_ansi_mode_2() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        assert!(!terminal.input_modes().keyboard_locked);
        terminal.feed(b"\x1b[2h");
        assert!(terminal.input_modes().keyboard_locked);
        terminal.feed(b"\x1b[2l");
        assert!(!terminal.input_modes().keyboard_locked);
    }

    #[test]
    fn keyboard_action_mode_resets_with_soft_and_full_reset() {
        let mut soft = BasicTerminal::new(GridSize::new(1, 5));
        soft.feed(b"\x1b[2h\x1b[!p");
        assert!(!soft.input_modes().keyboard_locked);

        let mut full = BasicTerminal::new(GridSize::new(1, 5));
        full.feed(b"\x1b[2h\x1bc");
        assert!(!full.input_modes().keyboard_locked);
    }

    #[test]
    fn backarrow_key_mode_tracks_decbkm_private_mode() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        assert!(!terminal.input_modes().backarrow_sends_backspace);
        terminal.feed(b"\x1b[?67h");
        assert!(terminal.input_modes().backarrow_sends_backspace);
        terminal.feed(b"\x1b[?67l");
        assert!(!terminal.input_modes().backarrow_sends_backspace);
    }

    #[test]
    fn backarrow_key_mode_resets_with_soft_and_full_reset() {
        let mut soft = BasicTerminal::new(GridSize::new(1, 5));
        soft.feed(b"\x1b[?67h\x1b[!p");
        assert!(!soft.input_modes().backarrow_sends_backspace);

        let mut full = BasicTerminal::new(GridSize::new(1, 5));
        full.feed(b"\x1b[?67h\x1bc");
        assert!(!full.input_modes().backarrow_sends_backspace);
    }

    #[test]
    fn kitty_keyboard_protocol_queries_and_pushes_supported_flags() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"\x1b[?u");
        assert_eq!(
            terminal.drain_host_actions(),
            vec![terminal_reply(b"\x1b[?0u")]
        );

        terminal.feed(b"\x1b[>1u\x1b[?u");
        assert_eq!(
            terminal.input_modes().kitty_keyboard_flags,
            KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES
        );
        assert_eq!(
            terminal.drain_host_actions(),
            vec![terminal_reply(b"\x1b[?1u")]
        );

        terminal.feed(b"\x1b[<u");
        assert_eq!(terminal.input_modes().kitty_keyboard_flags, 0);

        terminal.feed(b"\x1b[>9u\x1b[?u");
        assert_eq!(
            terminal.input_modes().kitty_keyboard_flags,
            KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES
                | KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES
        );
        assert_eq!(
            terminal.drain_host_actions(),
            vec![terminal_reply(b"\x1b[?9u")]
        );

        terminal.feed(b"\x1b[>25u\x1b[?u");
        assert_eq!(
            terminal.input_modes().kitty_keyboard_flags,
            KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES
                | KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES
                | KITTY_KEYBOARD_REPORT_ASSOCIATED_TEXT
        );
        assert_eq!(
            terminal.drain_host_actions(),
            vec![terminal_reply(b"\x1b[?25u")]
        );
    }

    #[test]
    fn kitty_keyboard_protocol_set_mode_only_tracks_supported_flags() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"\x1b[=2u");
        assert_eq!(terminal.input_modes().kitty_keyboard_flags, 0);

        terminal.feed(b"\x1b[=1u");
        assert_eq!(
            terminal.input_modes().kitty_keyboard_flags,
            KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES
        );

        terminal.feed(b"\x1b[=8u");
        assert_eq!(
            terminal.input_modes().kitty_keyboard_flags,
            KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES
        );

        terminal.feed(b"\x1b[=16;2u");
        assert_eq!(
            terminal.input_modes().kitty_keyboard_flags,
            KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES | KITTY_KEYBOARD_REPORT_ASSOCIATED_TEXT
        );

        terminal.feed(b"\x1b[=1;3u");
        assert_eq!(
            terminal.input_modes().kitty_keyboard_flags,
            KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES | KITTY_KEYBOARD_REPORT_ASSOCIATED_TEXT
        );

        terminal.feed(b"\x1b[=1;2u");
        assert_eq!(
            terminal.input_modes().kitty_keyboard_flags,
            KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES
                | KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES
                | KITTY_KEYBOARD_REPORT_ASSOCIATED_TEXT
        );
    }

    #[test]
    fn kitty_keyboard_protocol_keeps_main_and_alternate_screen_state_separate() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"\x1b[>1u\x1b[?1049h");
        assert_eq!(terminal.input_modes().kitty_keyboard_flags, 0);

        terminal.feed(b"\x1b[>1u\x1b[<u");
        assert_eq!(terminal.input_modes().kitty_keyboard_flags, 0);

        terminal.feed(b"\x1b[?1049l");
        assert_eq!(
            terminal.input_modes().kitty_keyboard_flags,
            KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES
        );
    }

    #[test]
    fn kitty_keyboard_protocol_resets_with_soft_and_full_reset() {
        let mut soft = BasicTerminal::new(GridSize::new(1, 5));
        soft.feed(b"\x1b[>1u\x1b[!p");
        assert_eq!(soft.input_modes().kitty_keyboard_flags, 0);

        let mut full = BasicTerminal::new(GridSize::new(1, 5));
        full.feed(b"\x1b[>1u\x1bc");
        assert_eq!(full.input_modes().kitty_keyboard_flags, 0);
    }

    #[test]
    fn reverse_video_mode_swaps_snapshot_colors() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 3));

        terminal.feed(b"A\x1b[31;44mB\x1b[?5h");
        let reversed = terminal.take_snapshot();
        terminal.feed(b"\x1b[?5l");
        let normal = terminal.snapshot();

        assert_eq!(reversed.rows[0].cells[0].style.foreground, Rgba::BLACK);
        assert_eq!(reversed.rows[0].cells[0].style.background, Rgba::WHITE);
        assert_eq!(
            reversed.rows[0].cells[1].style.foreground,
            Rgba::rgb(0, 0, 238)
        );
        assert_eq!(
            reversed.rows[0].cells[1].style.background,
            Rgba::rgb(205, 0, 0)
        );
        assert_eq!(reversed.damage, DamageRegion::Full);

        assert_eq!(normal.rows[0].cells[0].style.foreground, Rgba::WHITE);
        assert_eq!(normal.rows[0].cells[0].style.background, Rgba::BLACK);
        assert_eq!(
            normal.rows[0].cells[1].style.foreground,
            Rgba::rgb(205, 0, 0)
        );
        assert_eq!(
            normal.rows[0].cells[1].style.background,
            Rgba::rgb(0, 0, 238)
        );
        assert_eq!(normal.damage, DamageRegion::Full);
    }

    #[test]
    fn reverse_video_mode_combines_with_sgr_reverse() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 3));

        terminal.feed(b"\x1b[31;44;7mR\x1b[?5h");
        let snapshot = terminal.snapshot();
        let cell = &snapshot.rows[0].cells[0].style;

        assert_eq!(cell.foreground, Rgba::rgb(205, 0, 0));
        assert_eq!(cell.background, Rgba::rgb(0, 0, 238));
        assert!(cell.flags.reverse);
    }

    #[test]
    fn reverse_video_mode_resets_with_soft_and_full_reset() {
        let mut soft = BasicTerminal::new(GridSize::new(1, 2));
        soft.feed(b"A\x1b[?5h\x1b[!p");
        assert_eq!(
            soft.snapshot().rows[0].cells[0].style.foreground,
            Rgba::WHITE
        );
        assert_eq!(
            soft.snapshot().rows[0].cells[0].style.background,
            Rgba::BLACK
        );

        let mut full = BasicTerminal::new(GridSize::new(1, 2));
        full.feed(b"\x1b[?5h\x1bcA");
        assert_eq!(
            full.snapshot().rows[0].cells[0].style.foreground,
            Rgba::WHITE
        );
        assert_eq!(
            full.snapshot().rows[0].cells[0].style.background,
            Rgba::BLACK
        );
    }

    #[test]
    fn mouse_modes_track_xterm_private_modes() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        assert_eq!(terminal.mouse_modes(), TerminalMouseModes::default());

        terminal.feed(b"\x1b[?1000h");
        assert_eq!(
            terminal.mouse_modes(),
            TerminalMouseModes {
                tracking: MouseTrackingMode::Normal,
                ..TerminalMouseModes::default()
            }
        );

        terminal.feed(b"\x1b[?1002h");
        assert_eq!(
            terminal.mouse_modes().tracking,
            MouseTrackingMode::ButtonEvent
        );

        terminal.feed(b"\x1b[?1003h");
        assert_eq!(terminal.mouse_modes().tracking, MouseTrackingMode::AnyEvent);

        terminal.feed(b"\x1b[?1003l");
        assert_eq!(terminal.mouse_modes().tracking, MouseTrackingMode::None);

        terminal.feed(b"\x1b[?9h");
        assert_eq!(terminal.mouse_modes().tracking, MouseTrackingMode::X10);
    }

    #[test]
    fn mouse_modes_track_encoding_focus_and_alternate_scroll() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"\x1b[?1004;1005;1007h");
        assert_eq!(
            terminal.mouse_modes(),
            TerminalMouseModes {
                tracking: MouseTrackingMode::None,
                encoding: MouseEncodingMode::Utf8,
                focus_events: true,
                alternate_scroll: true,
            }
        );

        terminal.feed(b"\x1b[?1006h");
        assert_eq!(
            terminal.mouse_modes(),
            TerminalMouseModes {
                tracking: MouseTrackingMode::None,
                encoding: MouseEncodingMode::Sgr,
                focus_events: true,
                alternate_scroll: true,
            }
        );

        terminal.feed(b"\x1b[?1016h");
        assert_eq!(
            terminal.mouse_modes().encoding,
            MouseEncodingMode::SgrPixels
        );

        terminal.feed(b"\x1b[?1016l");
        assert_eq!(terminal.mouse_modes().encoding, MouseEncodingMode::Sgr);

        terminal.feed(b"\x1b[?1006l");
        terminal.feed(b"\x1b[?1015h");
        assert_eq!(terminal.mouse_modes().encoding, MouseEncodingMode::Urxvt);

        terminal.feed(b"\x1b[?1015l");
        assert_eq!(terminal.mouse_modes().encoding, MouseEncodingMode::Utf8);

        terminal.feed(b"\x1b[?1005l");
        assert_eq!(terminal.mouse_modes().encoding, MouseEncodingMode::X10);

        terminal.feed(b"\x1b[?1004;1007l");
        assert_eq!(terminal.mouse_modes(), TerminalMouseModes::default());
    }

    #[test]
    fn full_reset_clears_mouse_modes() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"\x1b[?1003;1004;1006;1007;1016h\x1bc");

        assert_eq!(terminal.mouse_modes(), TerminalMouseModes::default());
    }

    #[test]
    fn cursor_visibility_tracks_private_mode_25() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.take_snapshot();
        terminal.feed(b"\x1b[?25l");
        let hidden = terminal.take_snapshot();
        terminal.feed(b"\x1b[?25h");
        let visible = terminal.snapshot();

        assert!(!hidden.cursor.visible);
        assert_eq!(hidden.damage, DamageRegion::Rows(vec![0]));
        assert!(visible.cursor.visible);
        assert_eq!(visible.damage, DamageRegion::Rows(vec![0]));
    }

    #[test]
    fn cursor_blink_tracks_private_mode_12() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.feed(b"\x1b[6 q");
        terminal.take_snapshot();
        terminal.feed(b"\x1b[?12h");
        let blinking = terminal.take_snapshot();
        terminal.feed(b"\x1b[?12l");
        let steady = terminal.snapshot();

        assert_eq!(blinking.cursor.shape, CursorShape::Bar);
        assert!(blinking.cursor.blink);
        assert_eq!(blinking.damage, DamageRegion::Rows(vec![0]));
        assert_eq!(steady.cursor.shape, CursorShape::Bar);
        assert!(!steady.cursor.blink);
        assert_eq!(steady.damage, DamageRegion::Rows(vec![0]));
    }

    #[test]
    fn cursor_shape_tracks_decscusr() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.take_snapshot();
        terminal.feed(b"\x1b[5 q");
        let bar = terminal.take_snapshot();
        terminal.feed(b"\x1b[4 q");
        let underline = terminal.take_snapshot();
        terminal.feed(b"\x1b[0 q");
        let block = terminal.snapshot();

        assert_eq!(bar.cursor.shape, CursorShape::Bar);
        assert!(bar.cursor.blink);
        assert_eq!(bar.damage, DamageRegion::Rows(vec![0]));
        assert_eq!(underline.cursor.shape, CursorShape::Underline);
        assert!(!underline.cursor.blink);
        assert_eq!(underline.damage, DamageRegion::Rows(vec![0]));
        assert_eq!(block.cursor.shape, CursorShape::Block);
        assert!(block.cursor.blink);
        assert_eq!(block.damage, DamageRegion::Rows(vec![0]));
    }

    #[test]
    fn cursor_shape_tracks_decscusr_blink_variants() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.feed(b"\x1b[2 q");
        let steady_block = terminal.take_snapshot();
        terminal.feed(b"\x1b[3 q");
        let blinking_underline = terminal.take_snapshot();
        terminal.feed(b"\x1b[6 q");
        let steady_bar = terminal.snapshot();

        assert_eq!(steady_block.cursor.shape, CursorShape::Block);
        assert!(!steady_block.cursor.blink);
        assert_eq!(blinking_underline.cursor.shape, CursorShape::Underline);
        assert!(blinking_underline.cursor.blink);
        assert_eq!(steady_bar.cursor.shape, CursorShape::Bar);
        assert!(!steady_bar.cursor.blink);
    }

    #[test]
    fn configured_default_cursor_style_survives_resets() {
        let mut soft = BasicTerminal::new(GridSize::new(3, 8));
        soft.set_default_cursor_style(CursorShape::Bar, false);
        assert_eq!(soft.default_cursor_style(), (CursorShape::Bar, false));
        soft.feed(b"\x1b[3 q\x1b[!p");
        let soft_reset = soft.snapshot();

        let mut full = BasicTerminal::new(GridSize::new(3, 8));
        full.set_default_cursor_style(CursorShape::Bar, false);
        full.feed(b"\x1b[3 q\x1bc");
        let full_reset = full.snapshot();

        assert_eq!(soft_reset.cursor.shape, CursorShape::Bar);
        assert!(!soft_reset.cursor.blink);
        assert_eq!(full_reset.cursor.shape, CursorShape::Bar);
        assert!(!full_reset.cursor.blink);
    }

    #[test]
    fn osc_zero_and_two_set_window_title() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.take_snapshot();
        terminal.feed(b"\x1b]0;Witty\x07");
        let icon_and_window = terminal.take_snapshot();
        terminal.feed(b"\x1b]2;shell\x1b\\");
        let window = terminal.snapshot();

        assert_eq!(terminal.title(), Some("shell"));
        assert_eq!(icon_and_window.title.as_deref(), Some("Witty"));
        assert_eq!(icon_and_window.damage, DamageRegion::Rows(vec![]));
        assert_eq!(window.title.as_deref(), Some("shell"));
        assert_eq!(window.damage, DamageRegion::Rows(vec![]));
    }

    #[test]
    fn osc_title_preserves_semicolons_and_ignores_other_codes() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.feed(b"\x1b]2;alpha;beta\x07");
        terminal.feed(b"\x1b]1;icon-only\x07");
        terminal.feed(b"\x1b]4;1;rgb:ff/00/00\x07");
        let snapshot = terminal.snapshot();

        assert_eq!(snapshot.title.as_deref(), Some("alpha;beta"));
    }

    #[test]
    fn osc_title_is_cleared_by_full_reset() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.feed(b"\x1b]2;before reset\x07");
        terminal.feed(b"\x1bc");

        assert_eq!(terminal.title(), None);
        assert_eq!(terminal.snapshot().title, None);
    }

    #[test]
    fn osc8_attaches_hyperlink_to_printed_cells_and_closes() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"\x1b]8;id=docs;https://example.com/docs\x1b\\Link\x1b]8;;\x1b\\ plain");
        let snapshot = terminal.snapshot();
        let link = snapshot.hyperlinks.first().expect("hyperlink should exist");

        assert_eq!(link.uri, "https://example.com/docs");
        assert_eq!(link.osc8_id.as_deref(), Some("docs"));
        assert_eq!(hyperlink_at(&snapshot, 0, 0), Some(link.id));
        assert_eq!(hyperlink_at(&snapshot, 0, 3), Some(link.id));
        assert_eq!(hyperlink_at(&snapshot, 0, 4), None);
    }

    #[test]
    fn osc8_preserves_uri_semicolons_and_accepts_bel_termination() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b]8;;https://example.com/a;b\x07A");
        let snapshot = terminal.snapshot();

        assert_eq!(snapshot.hyperlinks.len(), 1);
        assert_eq!(snapshot.hyperlinks[0].uri, "https://example.com/a;b");
        assert_eq!(
            hyperlink_at(&snapshot, 0, 0),
            Some(snapshot.hyperlinks[0].id)
        );
    }

    #[test]
    fn osc8_erase_removes_visible_stale_hyperlink_metadata() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b]8;;https://example.com\x1b\\AB\x1b]8;;\x1b\\");
        terminal.feed(b"\r\x1b[2K");
        let snapshot = terminal.snapshot();

        assert!(snapshot.hyperlinks.is_empty());
        assert_eq!(hyperlink_at(&snapshot, 0, 0), None);
        assert_eq!(hyperlink_at(&snapshot, 0, 1), None);
    }

    #[test]
    fn osc8_links_survive_scrollback_visibility() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b]8;;https://example.com\x1b\\A\x1b]8;;\x1b\\\nB");
        assert_eq!(terminal.snapshot().hyperlinks, Vec::new());

        terminal.scroll_viewport_lines(1);
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0).trim_end(), "A");
        assert_eq!(snapshot.hyperlinks.len(), 1);
        assert_eq!(snapshot.hyperlinks[0].uri, "https://example.com");
        assert_eq!(
            hyperlink_at(&snapshot, 0, 0),
            Some(snapshot.hyperlinks[0].id)
        );
    }

    #[test]
    fn osc8_alternate_screen_keeps_visible_link_metadata_isolated() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b]8;;https://example.com\x1b\\M\x1b]8;;\x1b\\");
        terminal.feed(b"\x1b[?1049hA");
        let alternate = terminal.snapshot();

        assert!(alternate.hyperlinks.is_empty());
        assert_eq!(row_text(&alternate, 0).trim_end(), "A");

        terminal.feed(b"\x1b[?1049l");
        let main = terminal.snapshot();

        assert_eq!(main.hyperlinks.len(), 1);
        assert_eq!(row_text(&main, 0).trim_end(), "M");
        assert_eq!(hyperlink_at(&main, 0, 0), Some(main.hyperlinks[0].id));
    }

    #[test]
    fn osc8_hyperlinks_are_cleared_by_full_reset() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b]8;;https://example.com\x1b\\A");
        terminal.feed(b"\x1bc");

        assert!(terminal.snapshot().hyperlinks.is_empty());
    }

    #[test]
    fn osc52_queues_clipboard_write_action_without_changing_cells() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"\x1b]52;c;aGVsbG8=\x1b\\visible");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0).trim_end(), "visible");
        assert_eq!(
            terminal.drain_host_actions(),
            vec![clipboard_write(
                TerminalClipboardSelection::Clipboard,
                "hello",
            )]
        );
        assert_eq!(terminal.drain_host_actions(), Vec::new());
    }

    #[test]
    fn osc52_empty_target_defaults_to_clipboard_and_accepts_bel() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b]52;;dGV4dA==\x07");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![clipboard_write(
                TerminalClipboardSelection::Clipboard,
                "text",
            )]
        );
    }

    #[test]
    fn osc52_primary_target_is_preserved_for_host_policy() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b]52;p;cHJpbWFyeQ==\x1b\\");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![clipboard_write(
                TerminalClipboardSelection::Primary,
                "primary",
            )]
        );
    }

    #[test]
    fn osc52_multiple_targets_prefer_clipboard_when_present() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b]52;pc;bXVsdGk=\x1b\\");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![clipboard_write(
                TerminalClipboardSelection::Clipboard,
                "multi",
            )]
        );
    }

    #[test]
    fn osc52_ignores_queries_invalid_base64_and_unsupported_targets() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b]52;c;?\x1b\\");
        terminal.feed(b"\x1b]52;c;not-base64!\x1b\\");
        terminal.feed(b"\x1b]52;s;c2Vjb25kYXJ5\x1b\\");

        assert_eq!(terminal.drain_host_actions(), Vec::new());
    }

    #[test]
    fn osc52_rejects_oversized_and_non_utf8_payloads() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));
        let oversized = BASE64_STANDARD.encode(vec![b'a'; MAX_OSC52_DECODED_BYTES + 1]);

        terminal.feed(format!("\x1b]52;c;{oversized}\x1b\\").as_bytes());
        terminal.feed(b"\x1b]52;c;/w==\x1b\\");

        assert_eq!(terminal.drain_host_actions(), Vec::new());
    }

    #[test]
    fn osc52_allows_common_text_controls_but_rejects_nul() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));
        let text_payload = BASE64_STANDARD.encode("a\tb\nc\r".as_bytes());
        let nul_payload = BASE64_STANDARD.encode([b'a', 0, b'b']);

        terminal.feed(format!("\x1b]52;c;{text_payload}\x1b\\").as_bytes());
        terminal.feed(format!("\x1b]52;c;{nul_payload}\x1b\\").as_bytes());

        assert_eq!(
            terminal.drain_host_actions(),
            vec![clipboard_write(
                TerminalClipboardSelection::Clipboard,
                "a\tb\nc\r",
            )]
        );
    }

    #[test]
    fn osc7_current_directory_emits_host_action_without_cells() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"A\x1b]7;file://aibookmx/home/mingxu/project\x1b\\B");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "AB      ");
        assert_eq!(
            terminal.drain_host_actions(),
            vec![current_directory(
                "file://aibookmx/home/mingxu/project",
                Some("aibookmx"),
                "/home/mingxu/project",
                TerminalScreen::Main,
                CellPoint::new(0, 1),
            )]
        );
    }

    #[test]
    fn osc7_current_directory_decodes_percent_escapes_and_semicolons() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"\x1b]7;file:///home/mingxu/space%20dir;child\x07");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![current_directory(
                "file:///home/mingxu/space%20dir;child",
                None,
                "/home/mingxu/space dir;child",
                TerminalScreen::Main,
                CellPoint::new(0, 0),
            )]
        );
        assert_eq!(row_text(&terminal.snapshot(), 0), "        ");
    }

    #[test]
    fn osc7_ignores_non_file_and_unsafe_paths_without_printing_payload() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"A\x1b]7;https://example.test/project\x1b\\");
        terminal.feed(b"\x1b]7;file:///tmp/bad%0aname\x1b\\B");

        assert_eq!(terminal.drain_host_actions(), Vec::new());
        assert_eq!(row_text(&terminal.snapshot(), 0), "AB      ");
    }

    #[test]
    fn osc133_shell_integration_markers_emit_host_actions_without_cells() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 16));

        terminal.feed(
            b"\x1b]133;A\x1b\\$\x1b]133;B\x1b\\ echo\x1b]133;C\x1b\\\r\nok\x1b]133;D;7\x1b\\",
        );
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0).trim_end(), "$ echo");
        assert_eq!(row_text(&snapshot, 1).trim_end(), "ok");
        assert_eq!(
            terminal.drain_host_actions(),
            vec![
                shell_integration_event(
                    TerminalShellIntegrationMarker::PromptStart,
                    TerminalScreen::Main,
                    CellPoint::new(0, 0),
                    None,
                ),
                shell_integration_event(
                    TerminalShellIntegrationMarker::CommandStart,
                    TerminalScreen::Main,
                    CellPoint::new(0, 1),
                    None,
                ),
                shell_integration_event(
                    TerminalShellIntegrationMarker::OutputStart,
                    TerminalScreen::Main,
                    CellPoint::new(0, 6),
                    None,
                ),
                shell_integration_event(
                    TerminalShellIntegrationMarker::CommandFinished,
                    TerminalScreen::Main,
                    CellPoint::new(1, 2),
                    Some(7),
                ),
            ]
        );
    }

    #[test]
    fn osc133_shell_integration_handles_bel_and_alternate_screen() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"\x1b[?1049h\x1b]133;A\x07");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![shell_integration_event_with_anchor_row(
                TerminalShellIntegrationMarker::PromptStart,
                TerminalScreen::Alternate,
                CellPoint::new(0, 0),
                1,
                None,
            )]
        );
    }

    #[test]
    fn osc133_shell_integration_ignores_unknown_markers_and_invalid_exit_codes() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 16));

        terminal.feed(b"\x1b]133;X;ignored\x1b\\visible\x1b]133;D;bad\x1b\\");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0).trim_end(), "visible");
        assert_eq!(
            terminal.drain_host_actions(),
            vec![shell_integration_event(
                TerminalShellIntegrationMarker::CommandFinished,
                TerminalScreen::Main,
                CellPoint::new(0, 7),
                None,
            )]
        );
    }

    #[test]
    fn primary_device_attributes_reply_is_host_action_not_screen_text() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 12));

        terminal.feed(b"before\x1b[cafter");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![terminal_reply(b"\x1b[?1;2c")]
        );
        let screen_text = row_text(&terminal.snapshot(), 0);
        assert!(screen_text.contains("before"));
        assert!(screen_text.contains("after"));
        assert!(!screen_text.contains("[?1;2c"));
    }

    #[test]
    fn device_attributes_accept_zero_parameter_and_decid_alias() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"\x1b[0c\x1bZ\x1b[1c");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![terminal_reply(b"\x1b[?1;2c"), terminal_reply(b"\x1b[?1;2c"),]
        );
    }

    #[test]
    fn c1_decid_alias_reports_primary_device_attributes() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"\x9a");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![terminal_reply(b"\x1b[?1;2c")]
        );
    }

    #[test]
    fn secondary_device_attributes_report_witty_compatible_version() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"\x1b[>c");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![terminal_reply(b"\x1b[>0;1;0c")]
        );
    }

    #[test]
    fn tertiary_device_attributes_report_zero_unit_id() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"\x1b[=c\x1b[=0c\x1b[=1c");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![
                terminal_reply(b"\x1bP!|00000000\x1b\\"),
                terminal_reply(b"\x1bP!|00000000\x1b\\"),
            ]
        );
    }

    #[test]
    fn xtversion_reports_terminal_name_and_version() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"\x1b[>q\x1b[>0q\x1b[>1q");

        let expected = format!("\x1bP>|Witty {}\x1b\\", env!("CARGO_PKG_VERSION")).into_bytes();
        assert_eq!(
            terminal.drain_host_actions(),
            vec![terminal_reply(&expected), terminal_reply(&expected)]
        );
    }

    #[test]
    fn terminal_parameters_report_accepts_default_and_one() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"\x1b[x\x1b[0x\x1b[1x\x1b[2x");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![
                terminal_reply(b"\x1b[2;1;1;128;128;1;0x"),
                terminal_reply(b"\x1b[2;1;1;128;128;1;0x"),
                terminal_reply(b"\x1b[3;1;1;128;128;1;0x"),
            ]
        );
    }

    #[test]
    fn device_status_report_replies_ok_and_cursor_position() {
        let mut terminal = BasicTerminal::new(GridSize::new(4, 12));

        terminal.feed(b"\x1b[3;5H\x1b[5n\x1b[6n");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![terminal_reply(b"\x1b[0n"), terminal_reply(b"\x1b[3;5R"),]
        );
    }

    #[test]
    fn private_cursor_position_report_preserves_dec_private_marker() {
        let mut terminal = BasicTerminal::new(GridSize::new(4, 12));

        terminal.feed(b"\x1b[2;4H\x1b[?6n");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![terminal_reply(b"\x1b[?2;4R")]
        );
    }

    #[test]
    fn request_mode_report_replies_with_ansi_mode_state() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"\x1b[2$p\x1b[2h\x1b[2$p\x1b[4$p\x1b[4h\x1b[4$p\x1b[20$p\x1b[20h\x1b[20$p");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![
                terminal_reply(b"\x1b[2;2$y"),
                terminal_reply(b"\x1b[2;1$y"),
                terminal_reply(b"\x1b[4;2$y"),
                terminal_reply(b"\x1b[4;1$y"),
                terminal_reply(b"\x1b[20;2$y"),
                terminal_reply(b"\x1b[20;1$y"),
            ]
        );
    }

    #[test]
    fn private_request_mode_report_replies_with_dec_mode_state() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(
            b"\x1b[?1$p\x1b[?1h\x1b[?1$p\x1b[?5$p\x1b[?5h\x1b[?5$p\x1b[?7l\x1b[?7$p\x1b[?999$p",
        );

        assert_eq!(
            terminal.drain_host_actions(),
            vec![
                terminal_reply(b"\x1b[?1;2$y"),
                terminal_reply(b"\x1b[?1;1$y"),
                terminal_reply(b"\x1b[?5;2$y"),
                terminal_reply(b"\x1b[?5;1$y"),
                terminal_reply(b"\x1b[?7;2$y"),
                terminal_reply(b"\x1b[?999;0$y"),
            ]
        );
    }

    #[test]
    fn private_request_mode_report_covers_mouse_clipboard_and_alt_screen_modes() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(
            b"\x1b[?66h\x1b[?1002h\x1b[?1005h\x1b[?1006h\x1b[?1015h\x1b[?2004h\x1b[?1049h\
              \x1b[?66$p\x1b[?1002$p\x1b[?1000$p\x1b[?1005$p\x1b[?1006$p\x1b[?1015$p\x1b[?2004$p\x1b[?1049$p\
              \x1b[?66l\x1b[?66$p\
              \x1b[?1049l\x1b[?1049$p",
        );

        assert_eq!(
            terminal.drain_host_actions(),
            vec![
                terminal_reply(b"\x1b[?66;1$y"),
                terminal_reply(b"\x1b[?1002;1$y"),
                terminal_reply(b"\x1b[?1000;2$y"),
                terminal_reply(b"\x1b[?1005;1$y"),
                terminal_reply(b"\x1b[?1006;1$y"),
                terminal_reply(b"\x1b[?1015;1$y"),
                terminal_reply(b"\x1b[?2004;1$y"),
                terminal_reply(b"\x1b[?1049;1$y"),
                terminal_reply(b"\x1b[?66;2$y"),
                terminal_reply(b"\x1b[?1049;2$y"),
            ]
        );
    }

    #[test]
    fn private_request_mode_report_covers_cursor_blink_mode() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"\x1b[?12$p\x1b[?12l\x1b[?12$p\x1b[?12h\x1b[?12$p");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![
                terminal_reply(b"\x1b[?12;1$y"),
                terminal_reply(b"\x1b[?12;2$y"),
                terminal_reply(b"\x1b[?12;1$y"),
            ]
        );
    }

    #[test]
    fn private_request_mode_report_covers_backarrow_key_mode() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"\x1b[?67$p\x1b[?67h\x1b[?67$p\x1b[?67l\x1b[?67$p");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![
                terminal_reply(b"\x1b[?67;2$y"),
                terminal_reply(b"\x1b[?67;1$y"),
                terminal_reply(b"\x1b[?67;2$y"),
            ]
        );
    }

    #[test]
    fn private_request_mode_report_covers_reverse_wraparound_mode() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"\x1b[?45$p\x1b[?45h\x1b[?45$p\x1b[?45l\x1b[?45$p");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![
                terminal_reply(b"\x1b[?45;2$y"),
                terminal_reply(b"\x1b[?45;1$y"),
                terminal_reply(b"\x1b[?45;2$y"),
            ]
        );
    }

    #[test]
    fn synchronized_output_mode_tracks_private_mode_and_report_state() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        assert!(!terminal.synchronized_output_enabled());
        terminal.feed(b"\x1b[?2026h\x1b[?2026$p");
        assert!(terminal.synchronized_output_enabled());
        terminal.feed(b"\x1b[?2026l\x1b[?2026$p");
        assert!(!terminal.synchronized_output_enabled());

        assert_eq!(
            terminal.drain_host_actions(),
            vec![
                terminal_reply(b"\x1b[?2026;1$y"),
                terminal_reply(b"\x1b[?2026;2$y"),
            ]
        );
    }

    #[test]
    fn decrqss_reports_current_sgr_status_string() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"\x1bP$qm\x1b\\");
        terminal.feed(b"\x1b[1;2;3;4:4;5;7;8;9;51;53;73;38;2;1;2;3;48;5;12;58;5;9m");
        terminal.feed(b"\x1bP$qm\x1b\\");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![
                terminal_reply(b"\x1bP1$r0m\x1b\\"),
                terminal_reply(b"\x1bP1$r1;2;3;4:4;5;7;8;9;51;53;73;38;2;1;2;3;104;58;5;9m\x1b\\"),
            ]
        );
    }

    #[test]
    fn decrqss_reports_terminal_conformance_status_string() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"\x1bP$q\"p\x1b\\");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![terminal_reply(b"\x1bP1$r65;1\"p\x1b\\")]
        );
    }

    #[test]
    fn decscl_selection_is_explicit_noop() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"A\x1b[61;0\"pB\x1b[65;1\"pC\x1bP$q\"p\x1b\\");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "ABC         ");
        assert_eq!(
            terminal.drain_host_actions(),
            vec![terminal_reply(b"\x1bP1$r65;1\"p\x1b\\")]
        );
    }

    #[test]
    fn decrqss_reports_cursor_style_and_scroll_region() {
        let mut terminal = BasicTerminal::new(GridSize::new(5, 12));

        terminal.feed(b"\x1b[2;4r\x1b[6 q\x1bP$q q\x1b\\\x1bP$qr\x1b\\");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![
                terminal_reply(b"\x1bP1$r6 q\x1b\\"),
                terminal_reply(b"\x1bP1$r2;4r\x1b\\"),
            ]
        );
    }

    #[test]
    fn decrqss_reports_full_width_left_right_margin_status_string() {
        let mut terminal = BasicTerminal::new(GridSize::new(5, 12));

        terminal.feed(b"\x1bP$qs\x1b\\");
        terminal.resize(GridSize::new(5, 16));
        terminal.feed(b"\x1bP$qs\x1b\\");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![
                terminal_reply(b"\x1bP1$r1;12s\x1b\\"),
                terminal_reply(b"\x1bP1$r1;16s\x1b\\"),
            ]
        );
    }

    #[test]
    fn decrqss_reports_character_protection_attribute() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"\x1bP$q\"q\x1b\\\x1b[1\"q\x1bP$q\"q\x1b\\");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![
                terminal_reply(b"\x1bP1$r0\"q\x1b\\"),
                terminal_reply(b"\x1bP1$r1\"q\x1b\\"),
            ]
        );
    }

    #[test]
    fn decrqss_replies_invalid_for_unknown_request_without_printing_payload() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"A\x1bP$qbogus\x1b\\B");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![terminal_reply(b"\x1bP0$rbogus\x1b\\")]
        );
        assert_eq!(row_text(&terminal.snapshot(), 0), "AB      ");
    }

    #[test]
    fn decrqss_ignores_unsafe_or_oversized_unknown_requests() {
        let mut control = BasicTerminal::new(GridSize::new(1, 8));
        control.feed(b"A\x1bP$qbad\x07name\x1b\\B");

        assert_eq!(control.drain_host_actions(), Vec::new());
        assert_eq!(row_text(&control.snapshot(), 0), "AB      ");

        let mut oversized = BasicTerminal::new(GridSize::new(1, 8));
        oversized.feed(b"A\x1bP$q");
        oversized.feed(&[b'x'; MAX_DCS_REQUEST_BYTES + 1]);
        oversized.feed(b"\x1b\\B");

        assert_eq!(oversized.drain_host_actions(), Vec::new());
        assert_eq!(row_text(&oversized.snapshot(), 0), "AB      ");
    }

    #[test]
    fn decrqss_works_through_c1_dcs_and_st_aliases() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"\x90$qm\x9c");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![terminal_reply(b"\x1bP1$r0m\x1b\\")]
        );
    }

    #[test]
    fn xtgettcap_reports_static_terminal_capabilities() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"A\x1bP+q544e;5463;524742;436f;4373;4372;4d73;5365\x1b\\B");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![
                terminal_reply(b"\x1bP1+r544e=787465726D2D323536636F6C6F72\x1b\\"),
                terminal_reply(b"\x1bP1+r5463\x1b\\"),
                terminal_reply(b"\x1bP1+r524742=382F382F38\x1b\\"),
                terminal_reply(b"\x1bP1+r436f=323536\x1b\\"),
                terminal_reply(b"\x1bP1+r4373=1B5D31323B257031257307\x1b\\"),
                terminal_reply(b"\x1bP1+r4372=1B5D31313207\x1b\\"),
                terminal_reply(b"\x1bP1+r4d73=1B5D35323B25703125733B257032257307\x1b\\"),
                terminal_reply(b"\x1bP1+r5365=1B5B322071\x1b\\"),
            ]
        );
        assert_eq!(row_text(&terminal.snapshot(), 0), "AB      ");
    }

    #[test]
    fn xtgettcap_reports_unknown_valid_names_as_absent() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"\x1bP+q626f677573\x1b\\");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![terminal_reply(b"\x1bP0+r626f677573\x1b\\")]
        );
    }

    #[test]
    fn xtgettcap_ignores_malformed_names_without_echoing_payload() {
        let mut control = BasicTerminal::new(GridSize::new(1, 8));
        control.feed(b"A\x1bP+q07\x1b\\B");

        assert_eq!(control.drain_host_actions(), Vec::new());
        assert_eq!(row_text(&control.snapshot(), 0), "AB      ");

        let mut odd = BasicTerminal::new(GridSize::new(1, 8));
        odd.feed(b"A\x1bP+q546\x1b\\B");

        assert_eq!(odd.drain_host_actions(), Vec::new());
        assert_eq!(row_text(&odd.snapshot(), 0), "AB      ");
    }

    #[test]
    fn xtgettcap_allows_multi_capability_queries_over_decrqss_limit() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));
        let query = std::iter::repeat_n("5463", 20)
            .collect::<Vec<_>>()
            .join(";");
        assert!(query.len() > MAX_DCS_REQUEST_BYTES);
        assert!(query.len() < MAX_XTGETTCAP_REQUEST_BYTES);

        terminal.feed(format!("\x1bP+q{query}\x1b\\").as_bytes());

        let replies = terminal.drain_host_actions();
        assert_eq!(replies.len(), 20);
        assert!(replies
            .iter()
            .all(|reply| *reply == terminal_reply(b"\x1bP1+r5463\x1b\\")));
    }

    #[test]
    fn xtgettcap_works_through_c1_dcs_and_st_aliases() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"\x90+q5463\x9c");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![terminal_reply(b"\x1bP1+r5463\x1b\\")]
        );
    }

    #[test]
    fn synchronized_output_mode_is_cleared_by_full_reset() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 12));

        terminal.feed(b"\x1b[?2026h");
        assert!(terminal.synchronized_output_enabled());

        terminal.feed(b"\x1bc");

        assert!(!terminal.synchronized_output_enabled());
    }

    #[test]
    fn window_manipulation_reports_character_grid_size() {
        let mut terminal = BasicTerminal::new(GridSize::new(24, 80));

        terminal.feed(b"\x1b[18t\x1b[19t\x1b[14t");

        assert_eq!(
            terminal.drain_host_actions(),
            vec![
                terminal_reply(b"\x1b[8;24;80t"),
                terminal_reply(b"\x1b[8;24;80t"),
            ]
        );
    }

    #[test]
    fn scroll_region_constrains_linefeed_scroll() {
        let mut terminal = BasicTerminal::new(GridSize::new(5, 4));

        terminal.feed(b"A\x1b[2;1HB\x1b[3;1HC\x1b[4;1HD\x1b[5;1HE");
        terminal.feed(b"\x1b[2;4r\x1b[4;1H");
        terminal.take_snapshot();
        terminal.feed(b"\n");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "A   ");
        assert_eq!(row_text(&snapshot, 1), "C   ");
        assert_eq!(row_text(&snapshot, 2), "D   ");
        assert_eq!(row_text(&snapshot, 3), "    ");
        assert_eq!(row_text(&snapshot, 4), "E   ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(3, 0));
        assert_eq!(snapshot.damage, DamageRegion::Rows(vec![1, 2, 3]));
    }

    #[test]
    fn scroll_region_reset_restores_full_screen_scrollback_scroll() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 4));

        terminal.feed(b"A\x1b[2;1HB\x1b[3;1HC");
        terminal.feed(b"\x1b[2;2r\x1b[r\x1b[3;1H\n");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "B   ");
        assert_eq!(row_text(&snapshot, 1), "C   ");
        assert_eq!(row_text(&snapshot, 2), "    ");
        terminal.scroll_viewport_lines(1);
        assert_eq!(row_text(&terminal.snapshot(), 0), "A   ");
    }

    #[test]
    fn scroll_region_zero_params_reset_to_full_screen() {
        let mut terminal = BasicTerminal::new(GridSize::new(4, 4));

        terminal.feed(b"AAAA\x1b[2;1HBBBB\x1b[3;1HCCCC\x1b[4;1HDDDD");
        terminal.feed(b"\x1b[2;3r\x1b[0;0r\x1b[4;1H\n");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "BBBB");
        assert_eq!(row_text(&snapshot, 1), "CCCC");
        assert_eq!(row_text(&snapshot, 2), "DDDD");
        assert_eq!(row_text(&snapshot, 3), "    ");
    }

    #[test]
    fn reverse_index_scrolls_down_at_top_margin() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 4));

        terminal.feed(b"A\x1b[2;1HB\x1b[3;1HC\x1b[1;1H\x1bM");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "    ");
        assert_eq!(row_text(&snapshot, 1), "A   ");
        assert_eq!(row_text(&snapshot, 2), "B   ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 0));
    }

    #[test]
    fn c1_reverse_index_alias_scrolls_down_at_top_margin() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 4));

        terminal.feed(b"A\x1b[2;1HB\x1b[3;1HC\x1b[1;1H\x8d");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "    ");
        assert_eq!(row_text(&snapshot, 1), "A   ");
        assert_eq!(row_text(&snapshot, 2), "B   ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 0));
    }

    #[test]
    fn reverse_index_respects_scroll_region() {
        let mut terminal = BasicTerminal::new(GridSize::new(5, 4));

        terminal.feed(b"A\x1b[2;1HB\x1b[3;1HC\x1b[4;1HD\x1b[5;1HE");
        terminal.feed(b"\x1b[2;4r\x1b[2;1H\x1bM");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "A   ");
        assert_eq!(row_text(&snapshot, 1), "    ");
        assert_eq!(row_text(&snapshot, 2), "B   ");
        assert_eq!(row_text(&snapshot, 3), "C   ");
        assert_eq!(row_text(&snapshot, 4), "E   ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(1, 0));
    }

    #[test]
    fn reverse_index_moves_cursor_up_away_from_top_margin() {
        let mut terminal = BasicTerminal::new(GridSize::new(5, 4));

        terminal.feed(b"A\x1b[2;1HB\x1b[3;1HC\x1b[4;1HD\x1b[5;1HE");
        terminal.feed(b"\x1b[2;4r\x1b[3;1H\x1bM");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "A   ");
        assert_eq!(row_text(&snapshot, 1), "B   ");
        assert_eq!(row_text(&snapshot, 2), "C   ");
        assert_eq!(row_text(&snapshot, 3), "D   ");
        assert_eq!(row_text(&snapshot, 4), "E   ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(1, 0));
    }

    #[test]
    fn csi_scroll_up_moves_rows_and_preserves_cursor() {
        let mut terminal = BasicTerminal::new(GridSize::new(4, 4));

        terminal.feed(b"A\x1b[2;1HB\x1b[3;1HC\x1b[4;1HD\x1b[2;3H\x1b[S");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "B   ");
        assert_eq!(row_text(&snapshot, 1), "C   ");
        assert_eq!(row_text(&snapshot, 2), "D   ");
        assert_eq!(row_text(&snapshot, 3), "    ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(1, 2));

        terminal.scroll_viewport_lines(1);
        assert_eq!(row_text(&terminal.snapshot(), 0), "A   ");
    }

    #[test]
    fn csi_scroll_down_moves_rows_and_preserves_cursor() {
        let mut terminal = BasicTerminal::new(GridSize::new(4, 4));

        terminal.feed(b"A\x1b[2;1HB\x1b[3;1HC\x1b[4;1HD\x1b[2;3H\x1b[T");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "    ");
        assert_eq!(row_text(&snapshot, 1), "A   ");
        assert_eq!(row_text(&snapshot, 2), "B   ");
        assert_eq!(row_text(&snapshot, 3), "C   ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(1, 2));
    }

    #[test]
    fn csi_scroll_up_and_down_respect_scroll_region() {
        let mut up = BasicTerminal::new(GridSize::new(5, 4));
        up.feed(b"A\x1b[2;1HB\x1b[3;1HC\x1b[4;1HD\x1b[5;1HE");
        up.feed(b"\x1b[2;4r\x1b[2S");
        let scrolled_up = up.snapshot();

        assert_eq!(row_text(&scrolled_up, 0), "A   ");
        assert_eq!(row_text(&scrolled_up, 1), "D   ");
        assert_eq!(row_text(&scrolled_up, 2), "    ");
        assert_eq!(row_text(&scrolled_up, 3), "    ");
        assert_eq!(row_text(&scrolled_up, 4), "E   ");

        let mut down = BasicTerminal::new(GridSize::new(5, 4));
        down.feed(b"A\x1b[2;1HB\x1b[3;1HC\x1b[4;1HD\x1b[5;1HE");
        down.feed(b"\x1b[2;4r\x1b[2T");
        let scrolled_down = down.snapshot();

        assert_eq!(row_text(&scrolled_down, 0), "A   ");
        assert_eq!(row_text(&scrolled_down, 1), "    ");
        assert_eq!(row_text(&scrolled_down, 2), "    ");
        assert_eq!(row_text(&scrolled_down, 3), "B   ");
        assert_eq!(row_text(&scrolled_down, 4), "E   ");
    }

    #[test]
    fn full_reset_clears_scroll_region() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 4));

        terminal.feed(b"A\x1b[2;1HB\x1b[3;1HC");
        terminal.feed(b"\x1b[2;2r\x1bcA\x1b[2;1HB\x1b[3;1HC\x1b[3;1H\n");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "B   ");
        assert_eq!(row_text(&snapshot, 1), "C   ");
        assert_eq!(row_text(&snapshot, 2), "    ");
    }

    #[test]
    fn origin_mode_addresses_relative_to_scroll_region() {
        let mut terminal = BasicTerminal::new(GridSize::new(5, 6));

        terminal.feed(b"\x1b[2;4r\x1b[?6hX\x1b[2;3HY");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "      ");
        assert_eq!(row_text(&snapshot, 1), "X     ");
        assert_eq!(row_text(&snapshot, 2), "  Y   ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(2, 3));
    }

    #[test]
    fn origin_mode_clamps_addressing_and_relative_movement_to_region() {
        let mut terminal = BasicTerminal::new(GridSize::new(5, 4));

        terminal.feed(b"\x1b[2;3r\x1b[?6h\x1b[99;1HB\x1b[99AX");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 1), " X  ");
        assert_eq!(row_text(&snapshot, 2), "B   ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(1, 2));
    }

    #[test]
    fn disabling_origin_mode_homes_to_absolute_top_left() {
        let mut terminal = BasicTerminal::new(GridSize::new(5, 4));

        terminal.feed(b"\x1b[2;4r\x1b[?6hX\x1b[?6lY");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "Y   ");
        assert_eq!(row_text(&snapshot, 1), "X   ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 1));
    }

    #[test]
    fn setting_scroll_region_homes_to_region_top_when_origin_mode_is_enabled() {
        let mut terminal = BasicTerminal::new(GridSize::new(5, 4));

        terminal.feed(b"\x1b[?6h\x1b[3;4rX");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 2), "X   ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(2, 1));
    }

    #[test]
    fn full_reset_clears_origin_mode() {
        let mut terminal = BasicTerminal::new(GridSize::new(5, 4));

        terminal.feed(b"\x1b[2;4r\x1b[?6h\x1bc\x1b[2;4r\x1b[1;1HZ");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "Z   ");
        assert_eq!(row_text(&snapshot, 1), "    ");
    }

    #[test]
    fn alternate_screen_hides_and_restores_main_screen() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 5));

        assert_eq!(terminal.active_screen(), TerminalScreen::Main);
        terminal.feed(b"main\x1b[2;3H\x1b[?1049h");
        let entered = terminal.snapshot();
        assert_eq!(terminal.active_screen(), TerminalScreen::Alternate);
        terminal.feed(b"alt");
        let alternate = terminal.snapshot();
        terminal.feed(b"\x1b[?1049l");
        let restored = terminal.snapshot();
        assert_eq!(terminal.active_screen(), TerminalScreen::Main);

        assert_eq!(row_text(&entered, 0), "     ");
        assert_eq!(row_text(&entered, 1), "     ");
        assert_eq!(entered.cursor.position, CellPoint::new(0, 0));
        assert_eq!(entered.damage, DamageRegion::Full);
        assert_eq!(row_text(&alternate, 0), "alt  ");
        assert_eq!(row_text(&restored, 0), "main ");
        assert_eq!(row_text(&restored, 1), "     ");
        assert_eq!(restored.cursor.position, CellPoint::new(1, 2));
        assert_eq!(restored.damage, DamageRegion::Full);
    }

    #[test]
    fn alternate_screen_scroll_does_not_extend_main_scrollback() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 4));

        terminal.feed(b"m1\r\nm2\r\nm3");
        terminal.feed(b"\x1b[?1049ha\r\nb\r\nc");
        let alternate = terminal.snapshot();
        terminal.scroll_viewport_lines(1);
        terminal.feed(b"\x1b[?1049l");
        terminal.scroll_viewport_lines(2);
        let main_viewport = terminal.snapshot();

        assert_eq!(row_text(&alternate, 0), "b   ");
        assert_eq!(row_text(&alternate, 1), "c   ");
        assert_eq!(terminal.viewport_offset(), 1);
        assert_eq!(row_text(&main_viewport, 0), "m1  ");
        assert_eq!(row_text(&main_viewport, 1), "m2  ");
    }

    #[test]
    fn alternate_and_main_buffers_resize_independently() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 4));

        terminal.feed(b"main\x1b[?1049halt");
        terminal.resize(GridSize::new(2, 6));
        let alternate = terminal.snapshot();
        terminal.feed(b"\x1b[?1049l");
        let main = terminal.snapshot();

        assert_eq!(row_text(&alternate, 0), "alt   ");
        assert_eq!(row_text(&main, 0), "main  ");
    }

    #[test]
    fn alternate_screen_preserves_global_title_style_and_cursor_visuals() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 4));

        terminal.feed(b"\x1b]2;title\x07\x1b[31m\x1b[?25l\x1b[5 q\x1b[?1049hX");
        let alternate = terminal.snapshot();
        terminal.feed(b"\x1b[?1049lY");
        let main = terminal.snapshot();

        assert_eq!(alternate.title.as_deref(), Some("title"));
        assert_eq!(alternate.cursor.shape, CursorShape::Bar);
        assert!(!alternate.cursor.visible);
        assert_eq!(
            alternate.rows[0].cells[0].style.foreground,
            Rgba::rgb(205, 0, 0)
        );
        assert_eq!(main.title.as_deref(), Some("title"));
        assert_eq!(main.cursor.shape, CursorShape::Bar);
        assert!(!main.cursor.visible);
        assert_eq!(main.rows[0].cells[0].text, "Y");
        assert_eq!(main.rows[0].cells[0].style.foreground, Rgba::rgb(205, 0, 0));
    }

    #[test]
    fn full_reset_exits_alternate_screen_and_clears_both_buffers() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 5));

        terminal.feed(b"main\x1b[?1049halt\x1bc");
        let reset = terminal.snapshot();
        terminal.feed(b"z");
        let after_output = terminal.snapshot();

        assert!(reset
            .rows
            .iter()
            .flat_map(|row| &row.cells)
            .all(|cell| cell.text == " "));
        assert_eq!(reset.cursor, CursorState::default());
        assert_eq!(row_text(&after_output, 0), "z    ");
    }

    #[test]
    fn alternate_screen_1047_preserves_alternate_buffer() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"main\x1b[?1047halt\x1b[?1047l\x1b[?1047h");
        let alternate = terminal.snapshot();
        terminal.feed(b"\x1b[?1047l");
        let main = terminal.snapshot();

        assert_eq!(row_text(&alternate, 0), "alt  ");
        assert_eq!(alternate.cursor.position, CellPoint::new(0, 3));
        assert_eq!(row_text(&main, 0), "main ");
        assert_eq!(main.cursor.position, CellPoint::new(0, 4));
    }

    #[test]
    fn legacy_alternate_screen_47_maps_to_1047_behavior() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"main\x1b[?47hold\x1b[?47l\x1b[?47h");
        let alternate = terminal.snapshot();

        assert_eq!(row_text(&alternate, 0), "old  ");
        assert_eq!(alternate.cursor.position, CellPoint::new(0, 3));
    }

    #[test]
    fn alternate_screen_1049_clears_retained_alternate_buffer() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"\x1b[?1047hold\x1b[?1047l\x1b[?1049h");
        let alternate = terminal.snapshot();

        assert_eq!(row_text(&alternate, 0), "     ");
        assert_eq!(alternate.cursor.position, CellPoint::new(0, 0));
    }

    #[test]
    fn private_mode_1048_saves_and_restores_active_cursor() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"abcd\x1b[1;2H\x1b[?1048h\x1b[1;4H\x1b[?1048lZ");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "aZcd ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 2));
    }

    #[test]
    fn esc_7_and_8_save_and_restore_active_cursor() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"abcd\x1b[1;2H\x1b7\x1b[1;4H\x1b8Z");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "aZcd ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 2));
    }

    #[test]
    fn csi_cursor_s_and_u_save_and_restore_active_cursor() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"abcd\x1b[1;2H\x1b[s\x1b[1;4H\x1b[uZ");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "aZcd ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 2));
    }

    #[test]
    fn csi_cursor_restore_is_screen_local() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"abcd\x1b[1;2H\x1b[s\x1b[?1049h\x1b[1;4H\x1b[uZ");
        let alternate = terminal.snapshot();

        assert_eq!(row_text(&alternate, 0), "   Z ");
        assert_eq!(alternate.cursor.position, CellPoint::new(0, 4));
    }

    #[test]
    fn csi_cursor_save_restore_ignores_params_and_private_u_variants() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"abcd\x1b[1;2H\x1b[s\x1b[1;4H\x1b[1s\x1b[?uZ\x1b[uY");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "aYcZ ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 2));
    }

    #[test]
    fn cursor_charset_save_restore_preserves_active_charset_state() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b)0\x0e\x1b7\x0f\x1b8lq");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "┌─  ");
    }

    #[test]
    fn cursor_charset_save_restore_preserves_g2_designation() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b*0\x1b[s\x1b*B\x1b[u\x1bNlq");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "┌q  ");
    }

    #[test]
    fn cursor_charset_restore_clears_pending_single_shift() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b*0\x1b[s\x1bN\x1b[ul");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "l   ");
    }

    #[test]
    fn cursor_save_restore_preserves_current_style_and_protection() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b[31;1m\x1b[1\"q\x1b[1;2H\x1b[s");
        terminal.feed(b"\x1b[0m\x1b[0\"q\x1b[1;1HA\x1b[uB\r\x1b[?K");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), " B  ");
        let restored_cell = snapshot.rows[0]
            .cells
            .iter()
            .find(|cell| cell.point.col == 1)
            .unwrap();
        assert_eq!(restored_cell.style.foreground, Rgba::rgb(205, 0, 0));
        assert!(restored_cell.style.flags.bold);
    }

    #[test]
    fn cursor_save_restore_preserves_origin_mode() {
        let mut terminal = BasicTerminal::new(GridSize::new(4, 4));

        terminal.feed(b"\x1b[2;4r\x1b[?6h\x1b[s\x1b[?6l\x1b[u\x1b[1;1HX");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "    ");
        assert_eq!(row_text(&snapshot, 1), "X   ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(1, 1));
    }

    #[test]
    fn cursor_save_restore_preserves_autowrap_mode() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 4));

        terminal.feed(b"\x1b[?7l\x1b[s\x1b[?7h\x1b[uabcdE");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "abcE");
        assert_eq!(row_text(&snapshot, 1), "    ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 3));
    }

    #[test]
    fn cursor_save_restore_preserves_pending_wrap_state() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 4));

        terminal.feed(b"abcd\x1b[s\x1b[1;1HX\x1b[uE");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "Xbcd");
        assert_eq!(row_text(&snapshot, 1), "E   ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(1, 1));
    }

    #[test]
    fn private_mode_1048_restore_is_screen_local() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"abcd\x1b[1;2H\x1b[?1048h\x1b[?1049h\x1b[1;4H\x1b[?1048lZ");
        let alternate = terminal.snapshot();

        assert_eq!(row_text(&alternate, 0), "   Z ");
        assert_eq!(alternate.cursor.position, CellPoint::new(0, 4));
    }

    #[test]
    fn esc_cursor_restore_is_screen_local() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"abcd\x1b[1;2H\x1b7\x1b[?1049h\x1b[1;4H\x1b8Z");
        let alternate = terminal.snapshot();

        assert_eq!(row_text(&alternate, 0), "   Z ");
        assert_eq!(alternate.cursor.position, CellPoint::new(0, 4));
    }

    #[test]
    fn esc_cursor_restore_clamps_after_resize() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"abcd\x1b[1;5H\x1b7");
        terminal.resize(GridSize::new(1, 3));
        terminal.feed(b"\x1b8Z");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "abZ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 2));
    }

    #[test]
    fn alternate_screen_1049_restores_clamped_cursor_after_resize() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"main\x1b[1;5H\x1b[?1049h");
        terminal.resize(GridSize::new(1, 3));
        terminal.feed(b"\x1b[?1049l");
        let main = terminal.snapshot();

        assert_eq!(row_text(&main, 0), "mai");
        assert_eq!(main.cursor.position, CellPoint::new(0, 2));
    }

    #[test]
    fn clears_screen() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.feed(b"hello\x1b[2J");
        let snapshot = terminal.snapshot();

        assert!(snapshot
            .rows
            .iter()
            .flat_map(|row| &row.cells)
            .all(|cell| cell.text == " "));
        assert_eq!(snapshot.damage, DamageRegion::Full);
    }

    #[test]
    fn erase_in_display_zero_clears_cursor_to_end() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 5));

        terminal.feed(b"abcde\r\nfghij\r\nklmno");
        terminal.take_snapshot();
        terminal.feed(b"\x1b[2;3H\x1b[J");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "abcde");
        assert_eq!(row_text(&snapshot, 1), "fg   ");
        assert_eq!(row_text(&snapshot, 2), "     ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(1, 2));
        assert_eq!(snapshot.damage, DamageRegion::Rows(vec![1, 2]));
    }

    #[test]
    fn erase_in_display_one_clears_start_to_cursor() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 5));

        terminal.feed(b"abcde\r\nfghij\r\nklmno");
        terminal.feed(b"\x1b[2;3H\x1b[1J");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "     ");
        assert_eq!(row_text(&snapshot, 1), "   ij");
        assert_eq!(row_text(&snapshot, 2), "klmno");
        assert_eq!(snapshot.cursor.position, CellPoint::new(1, 2));
    }

    #[test]
    fn erase_in_display_two_clears_screen_without_moving_cursor() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 5));

        terminal.feed(b"abcde\r\nfghij\r\nklmno");
        terminal.feed(b"\x1b[2;3H\x1b[2J");
        let snapshot = terminal.snapshot();

        assert!(snapshot
            .rows
            .iter()
            .flat_map(|row| &row.cells)
            .all(|cell| cell.text == " "));
        assert_eq!(snapshot.cursor.position, CellPoint::new(1, 2));
        assert_eq!(snapshot.damage, DamageRegion::Full);
    }

    #[test]
    fn erase_in_display_three_clears_scrollback_only() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 4));

        terminal.feed(b"a\r\nb\r\nc");
        terminal.scroll_viewport_lines(1);
        assert_eq!(terminal.viewport_offset(), 1);
        terminal.feed(b"\x1b[3J");
        terminal.scroll_viewport_lines(1);
        let snapshot = terminal.snapshot();

        assert_eq!(terminal.viewport_offset(), 0);
        assert_eq!(row_text(&snapshot, 0), "b   ");
        assert_eq!(row_text(&snapshot, 1), "c   ");
        assert!(snapshot.cursor.visible);
        assert_eq!(snapshot.damage, DamageRegion::Full);
    }

    #[test]
    fn erase_in_line_variants_clear_expected_ranges() {
        let mut suffix = BasicTerminal::new(GridSize::new(1, 8));
        suffix.feed(b"abcdefgh\x1b[1;4H\x1b[K");

        let mut prefix = BasicTerminal::new(GridSize::new(1, 8));
        prefix.feed(b"abcdefgh\x1b[1;4H\x1b[1K");

        let mut whole = BasicTerminal::new(GridSize::new(1, 8));
        whole.feed(b"abcdefgh\x1b[1;4H\x1b[2K");

        assert_eq!(row_text(&suffix.snapshot(), 0), "abc     ");
        assert_eq!(suffix.snapshot().cursor.position, CellPoint::new(0, 3));
        assert_eq!(row_text(&prefix.snapshot(), 0), "    efgh");
        assert_eq!(prefix.snapshot().cursor.position, CellPoint::new(0, 3));
        assert_eq!(row_text(&whole.snapshot(), 0), "        ");
        assert_eq!(whole.snapshot().cursor.position, CellPoint::new(0, 3));
    }

    #[test]
    fn erase_uses_current_style_for_blank_cells() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"abcd\x1b[31;44;1m\x1b[1;2H\x1b[K");
        let snapshot = terminal.snapshot();
        let erased_style = &snapshot.rows[0].cells[1].style;

        assert_eq!(row_text(&snapshot, 0), "a   ");
        assert_eq!(snapshot.rows[0].cells[0].style, CellStyle::default());
        assert_eq!(erased_style.foreground, Rgba::rgb(205, 0, 0));
        assert_eq!(erased_style.background, Rgba::rgb(0, 0, 238));
        assert!(erased_style.flags.bold);
        assert_eq!(snapshot.rows[0].cells[3].style, *erased_style);
    }

    #[test]
    fn selective_erase_in_line_preserves_protected_cells() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"a\x1b[1\"qbc\x1b[0\"qde\r\x1b[?K");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), " bc  ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 0));
        assert_eq!(snapshot.damage, DamageRegion::Rows(vec![0]));
    }

    #[test]
    fn guarded_area_controls_mark_future_cells_protected() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"a\x1bVbc\x1bWde\r\x1b[?K");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), " bc  ");
    }

    #[test]
    fn c1_guarded_area_controls_mark_future_cells_protected() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"a\x96bc\x97de\r\x1b[?K");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), " bc  ");
    }

    #[test]
    fn ordinary_erase_in_line_clears_protected_cells() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 5));

        terminal.feed(b"a\x1b[1\"qbc\x1b[0\"qde\r\x1b[K");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "     ");
    }

    #[test]
    fn selective_erase_in_display_preserves_protected_cells() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 5));

        terminal.feed(b"a\x1b[1\"qB\x1b[0\"qcde\r\nfg\x1b[1\"qH\x1b[0\"qij");
        terminal.feed(b"\x1b[1;3H\x1b[?J");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "aB   ");
        assert_eq!(row_text(&snapshot, 1), "  H  ");
        assert_eq!(snapshot.cursor.position, CellPoint::new(0, 2));
    }

    #[test]
    fn selective_erase_in_display_two_preserves_only_protected_cells() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 6));

        terminal.feed(b"ab\x1b[1\"qCD\x1b[2\"qef\x1b[?2J");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "  CD  ");
    }

    #[test]
    fn soft_reset_clears_character_protection_attribute() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 3));

        terminal.feed(b"\x1b[1\"q\x1b[!pX\r\x1b[?2K");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "   ");
    }

    #[test]
    fn character_editing_controls_shift_or_erase_line_cells() {
        let mut insert = BasicTerminal::new(GridSize::new(1, 8));
        insert.feed(b"abcdef\x1b[1;3H\x1b[2@");
        assert_eq!(row_text(&insert.snapshot(), 0), "ab  cdef");

        let mut delete = BasicTerminal::new(GridSize::new(1, 8));
        delete.feed(b"abcdef\x1b[1;3H\x1b[2P");
        assert_eq!(row_text(&delete.snapshot(), 0), "abef    ");

        let mut erase = BasicTerminal::new(GridSize::new(1, 8));
        erase.feed(b"abcdef\x1b[1;3H\x1b[3X");
        assert_eq!(row_text(&erase.snapshot(), 0), "ab   f  ");
    }

    #[test]
    fn character_editing_controls_preserve_wide_cell_boundaries() {
        let mut insert = BasicTerminal::new(GridSize::new(1, 6));
        insert.feed("a界bc\x1b[1;3H\x1b[@".as_bytes());
        assert_eq!(row_text(&insert.snapshot(), 0), "a 界bc");

        let mut delete = BasicTerminal::new(GridSize::new(1, 6));
        delete.feed("a界bc\x1b[1;3H\x1b[P".as_bytes());
        assert_eq!(row_text(&delete.snapshot(), 0), "abc   ");

        let mut erase = BasicTerminal::new(GridSize::new(1, 6));
        erase.feed("a界bc\x1b[1;3H\x1b[X".as_bytes());
        assert_eq!(row_text(&erase.snapshot(), 0), "a  bc ");
    }

    #[test]
    fn repeat_preceding_graphic_defaults_and_counts() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 8));

        terminal.feed(b"A\x1b[bB\x1b[3b");

        assert_eq!(row_text(&terminal.snapshot(), 0), "AABBBB  ");
    }

    #[test]
    fn repeat_preceding_graphic_ignores_missing_predecessor() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"\x1b[3bA");

        assert_eq!(row_text(&terminal.snapshot(), 0), "A   ");
        assert_eq!(terminal.snapshot().cursor.position, CellPoint::new(0, 1));
    }

    #[test]
    fn repeat_preceding_graphic_zero_param_repeats_once() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed(b"Z\x1b[0b");

        assert_eq!(row_text(&terminal.snapshot(), 0), "ZZ  ");
    }

    #[test]
    fn repeat_preceding_graphic_resets_with_soft_and_full_reset() {
        let mut soft = BasicTerminal::new(GridSize::new(1, 4));
        soft.feed(b"A\x1b[!p\x1b[b");
        assert_eq!(row_text(&soft.snapshot(), 0), "A   ");

        let mut full = BasicTerminal::new(GridSize::new(1, 4));
        full.feed(b"A\x1bc\x1b[bB");
        assert_eq!(row_text(&full.snapshot(), 0), "B   ");
    }

    #[test]
    fn repeat_preceding_graphic_uses_print_path_for_wrap_and_wide_cells() {
        let mut wrapped = BasicTerminal::new(GridSize::new(2, 4));
        wrapped.feed(b"abcd\x1b[2b");
        let wrapped_snapshot = wrapped.snapshot();
        assert_eq!(row_text(&wrapped_snapshot, 0), "abcd");
        assert_eq!(row_text(&wrapped_snapshot, 1), "dd  ");

        let mut wide = BasicTerminal::new(GridSize::new(1, 6));
        wide.feed("界\x1b[2b".as_bytes());
        assert_eq!(row_text(&wide.snapshot(), 0), "界界界");
    }

    #[test]
    fn printing_on_wide_cell_halves_replaces_the_whole_cell() {
        let mut base = BasicTerminal::new(GridSize::new(1, 6));
        base.feed("a界b\x1b[1;2HX".as_bytes());
        assert_eq!(row_text(&base.snapshot(), 0), "aX b  ");

        let mut continuation = BasicTerminal::new(GridSize::new(1, 6));
        continuation.feed("a界b\x1b[1;3HY".as_bytes());
        assert_eq!(row_text(&continuation.snapshot(), 0), "aY b  ");
    }

    #[test]
    fn resize_drops_truncated_wide_cell_instead_of_orphaning_it() {
        let mut terminal = BasicTerminal::new(GridSize::new(1, 4));

        terminal.feed("ab界".as_bytes());
        terminal.resize(GridSize::new(1, 3));

        assert_eq!(row_text(&terminal.snapshot(), 0), "ab ");
    }

    #[test]
    fn line_editing_controls_respect_scroll_region() {
        let mut insert = BasicTerminal::new(GridSize::new(5, 4));
        insert.feed(b"A\x1b[2;1HB\x1b[3;1HC\x1b[4;1HD\x1b[5;1HE");
        insert.feed(b"\x1b[2;4r\x1b[3;1H\x1b[L");
        let inserted = insert.snapshot();
        assert_eq!(row_text(&inserted, 0), "A   ");
        assert_eq!(row_text(&inserted, 1), "B   ");
        assert_eq!(row_text(&inserted, 2), "    ");
        assert_eq!(row_text(&inserted, 3), "C   ");
        assert_eq!(row_text(&inserted, 4), "E   ");

        let mut delete = BasicTerminal::new(GridSize::new(5, 4));
        delete.feed(b"A\x1b[2;1HB\x1b[3;1HC\x1b[4;1HD\x1b[5;1HE");
        delete.feed(b"\x1b[2;4r\x1b[2;1H\x1b[M");
        let deleted = delete.snapshot();
        assert_eq!(row_text(&deleted, 0), "A   ");
        assert_eq!(row_text(&deleted, 1), "C   ");
        assert_eq!(row_text(&deleted, 2), "D   ");
        assert_eq!(row_text(&deleted, 3), "    ");
        assert_eq!(row_text(&deleted, 4), "E   ");
    }

    #[test]
    fn line_editing_controls_ignore_cursor_outside_scroll_region() {
        let mut terminal = BasicTerminal::new(GridSize::new(5, 4));

        terminal.feed(b"A\x1b[2;1HB\x1b[3;1HC\x1b[4;1HD\x1b[5;1HE");
        terminal.feed(b"\x1b[2;4r\x1b[5;1H\x1b[M");
        let snapshot = terminal.snapshot();

        assert_eq!(row_text(&snapshot, 0), "A   ");
        assert_eq!(row_text(&snapshot, 1), "B   ");
        assert_eq!(row_text(&snapshot, 2), "C   ");
        assert_eq!(row_text(&snapshot, 3), "D   ");
        assert_eq!(row_text(&snapshot, 4), "E   ");
    }

    #[test]
    fn resize_reports_full_damage() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.feed(b"ok");
        terminal.resize(GridSize::new(4, 12));

        assert_eq!(terminal.snapshot().damage, DamageRegion::Full);
    }

    #[test]
    fn keeps_scrollback_and_can_scroll_viewport() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 4));

        terminal.feed(b"a\r\nb\r\nc\r\nd");
        terminal.scroll_viewport_lines(1);
        let snapshot = terminal.snapshot();

        assert_eq!(terminal.viewport_offset(), 1);
        assert_eq!(row_text(&snapshot, 0), "a   ");
        assert_eq!(row_text(&snapshot, 1), "b   ");
        assert_eq!(row_text(&snapshot, 2), "c   ");
        assert!(!snapshot.cursor.visible);
        assert_eq!(snapshot.damage, DamageRegion::Full);
    }

    #[test]
    fn visible_row_anchors_follow_scrollback_viewport() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 4));

        terminal.feed(b"a\r\nb\r\nc\r\nd");
        assert_eq!(
            terminal.visible_row_anchors(),
            vec![
                TerminalVisibleRowAnchor {
                    visible_row: 0,
                    anchor: TerminalRowAnchor {
                        screen: TerminalScreen::Main,
                        row: 1,
                    },
                },
                TerminalVisibleRowAnchor {
                    visible_row: 1,
                    anchor: TerminalRowAnchor {
                        screen: TerminalScreen::Main,
                        row: 2,
                    },
                },
                TerminalVisibleRowAnchor {
                    visible_row: 2,
                    anchor: TerminalRowAnchor {
                        screen: TerminalScreen::Main,
                        row: 3,
                    },
                },
            ]
        );

        terminal.scroll_viewport_lines(1);
        assert_eq!(
            terminal.visible_row_anchors(),
            vec![
                TerminalVisibleRowAnchor {
                    visible_row: 0,
                    anchor: TerminalRowAnchor {
                        screen: TerminalScreen::Main,
                        row: 0,
                    },
                },
                TerminalVisibleRowAnchor {
                    visible_row: 1,
                    anchor: TerminalRowAnchor {
                        screen: TerminalScreen::Main,
                        row: 1,
                    },
                },
                TerminalVisibleRowAnchor {
                    visible_row: 2,
                    anchor: TerminalRowAnchor {
                        screen: TerminalScreen::Main,
                        row: 2,
                    },
                },
            ]
        );
    }

    #[test]
    fn capped_scrollback_trims_oldest_rows_and_keeps_recent_history() {
        let mut terminal = BasicTerminal::with_scrollback_limit(GridSize::new(2, 4), 3);

        for line in 0..6 {
            terminal.feed(format!("{line}\r\n").as_bytes());
        }

        assert_eq!(terminal.max_scrollback_lines(), 3);
        assert_eq!(terminal.scrollback_line_count(), 3);
        let history = terminal
            .state
            .scrollback
            .iter()
            .map(|row| cell_row_text(row).trim_end().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(history, vec!["2", "3", "4"]);

        terminal.scroll_viewport_lines(3);
        let snapshot = terminal.snapshot();
        assert_eq!(terminal.viewport_offset(), 3);
        assert_eq!(row_text(&snapshot, 0), "2   ");
        assert_eq!(row_text(&snapshot, 1), "3   ");
    }

    #[test]
    fn reducing_scrollback_limit_trims_existing_history_and_clamps_viewport() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 4));

        terminal.feed(b"a\r\nb\r\nc\r\nd\r\ne");
        terminal.scroll_viewport_lines(3);
        terminal.take_snapshot();
        terminal.set_max_scrollback_lines(1);

        assert_eq!(terminal.max_scrollback_lines(), 1);
        assert_eq!(terminal.scrollback_line_count(), 1);
        assert_eq!(terminal.viewport_offset(), 1);
        let snapshot = terminal.snapshot();
        assert_eq!(row_text(&snapshot, 0), "c   ");
        assert_eq!(row_text(&snapshot, 1), "d   ");
        assert_eq!(snapshot.damage, DamageRegion::Full);
    }

    #[test]
    fn zero_scrollback_limit_discards_history() {
        let mut terminal = BasicTerminal::with_scrollback_limit(GridSize::new(2, 4), 0);

        terminal.feed(b"a\r\nb\r\nc");
        terminal.scroll_viewport_lines(1);

        assert_eq!(terminal.scrollback_line_count(), 0);
        assert_eq!(terminal.viewport_offset(), 0);
        assert_eq!(row_text(&terminal.snapshot(), 0), "b   ");
    }

    #[test]
    fn output_returns_viewport_to_tail() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 4));

        terminal.feed(b"a\r\nb\r\nc\r\nd");
        terminal.scroll_viewport_lines(1);
        terminal.feed(b"!");

        assert_eq!(terminal.viewport_offset(), 0);
        assert_eq!(row_text(&terminal.snapshot(), 2), "d!  ");
    }

    #[test]
    fn selection_is_preserved_in_snapshot() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));
        let selection = CellRange {
            start: CellPoint::new(0, 0),
            end: CellPoint::new(0, 2),
        };

        terminal.set_selection(Some(selection));

        let snapshot = terminal.snapshot();

        assert_eq!(snapshot.selection, Some(selection));
        assert_eq!(snapshot.damage, DamageRegion::Full);
    }

    #[test]
    fn selection_change_does_not_mutate_cells() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));
        let selection = CellRange {
            start: CellPoint::new(0, 0),
            end: CellPoint::new(0, 1),
        };

        terminal.feed(b"ok");
        terminal.set_selection(Some(selection));

        let snapshot = terminal.snapshot();
        assert_eq!(row_text(&snapshot, 0), "ok      ");
        assert_eq!(snapshot.damage, DamageRegion::Full);
    }

    #[test]
    fn selected_text_returns_none_without_selection() {
        let terminal = BasicTerminal::new(GridSize::new(3, 8));

        assert_eq!(terminal.selected_text(), None);
    }

    #[test]
    fn selected_text_extracts_single_line_range() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.feed(b"hello");
        terminal.set_selection(Some(CellRange {
            start: CellPoint::new(0, 1),
            end: CellPoint::new(0, 3),
        }));

        assert_eq!(terminal.selected_text().as_deref(), Some("ell"));
    }

    #[test]
    fn selected_text_includes_wide_cell_when_either_half_is_selected() {
        let mut base = BasicTerminal::new(GridSize::new(2, 6));
        base.feed("a界b".as_bytes());
        base.set_selection(Some(CellRange {
            start: CellPoint::new(0, 1),
            end: CellPoint::new(0, 1),
        }));
        assert_eq!(base.selected_text().as_deref(), Some("界"));

        let mut continuation = BasicTerminal::new(GridSize::new(2, 6));
        continuation.feed("a界b".as_bytes());
        continuation.set_selection(Some(CellRange {
            start: CellPoint::new(0, 2),
            end: CellPoint::new(0, 2),
        }));
        assert_eq!(continuation.selected_text().as_deref(), Some("界"));

        continuation.set_selection(Some(CellRange {
            start: CellPoint::new(0, 0),
            end: CellPoint::new(0, 2),
        }));
        assert_eq!(continuation.selected_text().as_deref(), Some("a界"));
    }

    #[test]
    fn selected_text_includes_grapheme_cluster_when_any_cell_is_selected() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 10));

        terminal.feed("a👩\u{200d}💻z".as_bytes());
        let laptop_col = 1 + u16::from(terminal_char_width('👩'));
        terminal.set_selection(Some(CellRange {
            start: CellPoint::new(0, laptop_col),
            end: CellPoint::new(0, laptop_col),
        }));

        assert_eq!(terminal.selected_text().as_deref(), Some("👩\u{200d}💻"));
    }

    #[test]
    fn selected_text_extracts_multiline_range_without_screen_padding() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 5));

        terminal.feed(b"abc\r\ndef");
        terminal.set_selection(Some(CellRange {
            start: CellPoint::new(0, 1),
            end: CellPoint::new(1, 1),
        }));

        assert_eq!(terminal.selected_text().as_deref(), Some("bc\nde"));
    }

    #[test]
    fn selected_text_normalizes_reversed_range() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 5));

        terminal.feed(b"abc\r\ndef");
        terminal.set_selection(Some(CellRange {
            start: CellPoint::new(1, 1),
            end: CellPoint::new(0, 1),
        }));

        assert_eq!(terminal.selected_text().as_deref(), Some("bc\nde"));
    }

    #[test]
    fn selected_text_uses_current_viewport_rows() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 4));

        terminal.feed(b"a\r\nb\r\nc\r\nd");
        terminal.scroll_viewport_lines(1);
        terminal.set_selection(Some(CellRange {
            start: CellPoint::new(0, 0),
            end: CellPoint::new(1, 0),
        }));

        assert_eq!(terminal.selected_text().as_deref(), Some("a\nb"));
    }

    #[test]
    fn text_for_range_extracts_visible_end_exclusive_range() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 5));

        terminal.feed(b"abc\r\ndef");

        assert_eq!(
            terminal.text_for_range(TerminalTextRange {
                screen: TerminalScreen::Main,
                start: CellPoint::new(0, 1),
                end_exclusive: CellPoint::new(1, 2),
                start_anchor: None,
                end_exclusive_anchor: None,
            }),
            Some("bc\nde".to_owned())
        );
    }

    #[test]
    fn text_for_range_uses_row_anchors_after_scroll() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 8));

        terminal.feed(b"abcd\r\nefgh");
        let first_row_anchor = terminal.visible_row_anchors()[0].anchor;
        terminal.feed(b"\r\nijkl");

        assert_eq!(
            terminal.text_for_range(TerminalTextRange {
                screen: TerminalScreen::Main,
                start: CellPoint::new(0, 1),
                end_exclusive: CellPoint::new(0, 4),
                start_anchor: Some(TerminalPointAnchor {
                    row: first_row_anchor,
                    col: 1,
                }),
                end_exclusive_anchor: Some(TerminalPointAnchor {
                    row: first_row_anchor,
                    col: 4,
                }),
            }),
            Some("bcd".to_owned())
        );
    }

    #[test]
    fn text_for_range_returns_none_when_anchor_was_trimmed() {
        let mut terminal = BasicTerminal::with_scrollback_limit(GridSize::new(2, 8), 0);

        terminal.feed(b"abcd\r\nefgh");
        let first_row_anchor = terminal.visible_row_anchors()[0].anchor;
        terminal.feed(b"\r\nijkl");

        assert_eq!(
            terminal.text_for_range(TerminalTextRange {
                screen: TerminalScreen::Main,
                start: CellPoint::new(0, 1),
                end_exclusive: CellPoint::new(0, 4),
                start_anchor: Some(TerminalPointAnchor {
                    row: first_row_anchor,
                    col: 1,
                }),
                end_exclusive_anchor: Some(TerminalPointAnchor {
                    row: first_row_anchor,
                    col: 4,
                }),
            }),
            None
        );
    }

    #[test]
    fn word_range_at_expands_shell_path_word() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 20));

        terminal.feed(b"cat src/main.rs");
        let range = terminal.word_range_at(CellPoint::new(0, 8)).unwrap();
        terminal.set_selection(Some(range));

        assert_eq!(
            range,
            CellRange {
                start: CellPoint::new(0, 4),
                end: CellPoint::new(0, 14),
            }
        );
        assert_eq!(terminal.selected_text().as_deref(), Some("src/main.rs"));
    }

    #[test]
    fn word_range_at_treats_wide_continuation_as_base_cell() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.feed("a界b z".as_bytes());
        let range = terminal.word_range_at(CellPoint::new(0, 2)).unwrap();
        terminal.set_selection(Some(range));

        assert_eq!(
            range,
            CellRange {
                start: CellPoint::new(0, 0),
                end: CellPoint::new(0, 3),
            }
        );
        assert_eq!(terminal.selected_text().as_deref(), Some("a界b"));
    }

    #[test]
    fn word_range_at_keeps_combining_marks_with_base_cell() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.feed("e\u{0301}cho".as_bytes());
        let range = terminal.word_range_at(CellPoint::new(0, 0)).unwrap();
        terminal.set_selection(Some(range));

        assert_eq!(
            range,
            CellRange {
                start: CellPoint::new(0, 0),
                end: CellPoint::new(0, 3),
            }
        );
        assert_eq!(terminal.selected_text().as_deref(), Some("e\u{0301}cho"));
    }

    #[test]
    fn word_range_at_skips_separators_and_blank_padding() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 12));

        terminal.feed(b"foo; bar");

        assert_eq!(terminal.word_range_at(CellPoint::new(0, 3)), None);
        assert_eq!(terminal.word_range_at(CellPoint::new(0, 4)), None);
        assert_eq!(terminal.word_range_at(CellPoint::new(0, 11)), None);
        assert_eq!(
            terminal.word_range_at(CellPoint::new(0, 6)),
            Some(CellRange {
                start: CellPoint::new(0, 5),
                end: CellPoint::new(0, 7),
            })
        );
    }

    #[test]
    fn word_range_at_uses_current_viewport_rows() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.feed(b"a\r\nb word\r\nc\r\nd");
        terminal.scroll_viewport_lines(1);
        let range = terminal.word_range_at(CellPoint::new(1, 3)).unwrap();
        terminal.set_selection(Some(range));

        assert_eq!(
            range,
            CellRange {
                start: CellPoint::new(1, 2),
                end: CellPoint::new(1, 5),
            }
        );
        assert_eq!(terminal.selected_text().as_deref(), Some("word"));
    }

    #[test]
    fn word_range_at_rejects_out_of_bounds_points() {
        let terminal = BasicTerminal::new(GridSize::new(3, 8));

        assert_eq!(terminal.word_range_at(CellPoint::new(3, 0)), None);
        assert_eq!(terminal.word_range_at(CellPoint::new(0, 8)), None);
    }

    #[test]
    fn search_text_rows_include_scrollback_and_visible_rows() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 4));

        terminal.feed(b"a\r\nb\r\nc\r\nd");
        let rows = terminal.search_text_rows();

        assert_eq!(
            rows,
            vec![
                SearchTextRow {
                    id: SearchRowId::scrollback(0),
                    visible_row: None,
                    text: "a".to_owned(),
                    columns: Vec::new(),
                },
                SearchTextRow {
                    id: SearchRowId::screen(0),
                    visible_row: Some(0),
                    text: "b".to_owned(),
                    columns: Vec::new(),
                },
                SearchTextRow {
                    id: SearchRowId::screen(1),
                    visible_row: Some(1),
                    text: "c".to_owned(),
                    columns: Vec::new(),
                },
                SearchTextRow {
                    id: SearchRowId::screen(2),
                    visible_row: Some(2),
                    text: "d".to_owned(),
                    columns: Vec::new(),
                },
            ]
        );

        terminal.scroll_viewport_lines(1);
        let rows = terminal.search_text_rows();

        assert_eq!(rows[0].visible_row, Some(0));
        assert_eq!(rows[1].visible_row, Some(1));
        assert_eq!(rows[2].visible_row, Some(2));
        assert_eq!(rows[3].visible_row, None);
    }

    #[test]
    fn find_matches_maps_visible_ranges_and_can_scroll_to_history_match() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.feed(b"alpha\r\nbeta\r\nalpha\r\ngamma");
        let matches = terminal.find_matches("alpha", SearchOptions::default());

        assert_eq!(
            matches,
            vec![
                SearchMatch {
                    row: SearchRowId::scrollback(0),
                    start_col: 0,
                    end_col: 4,
                },
                SearchMatch {
                    row: SearchRowId::screen(1),
                    start_col: 0,
                    end_col: 4,
                },
            ]
        );
        assert_eq!(
            terminal.visible_search_matches(&matches),
            vec![CellRange {
                start: CellPoint::new(1, 0),
                end: CellPoint::new(1, 4),
            }]
        );

        assert!(terminal.scroll_to_search_match(SearchRowId::scrollback(0), 1));
        assert_eq!(terminal.viewport_offset(), 1);
        assert_eq!(
            terminal.visible_search_matches(&matches),
            vec![
                CellRange {
                    start: CellPoint::new(0, 0),
                    end: CellPoint::new(0, 4),
                },
                CellRange {
                    start: CellPoint::new(2, 0),
                    end: CellPoint::new(2, 4),
                },
            ]
        );
        assert_eq!(terminal.snapshot().damage, DamageRegion::Full);
    }

    #[test]
    fn visible_search_highlights_mark_active_visible_match() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.feed(b"alpha\r\nbeta\r\nalpha\r\ngamma");
        let matches = terminal.find_matches("alpha", SearchOptions::default());
        let highlights = terminal.visible_search_highlights(&matches, Some(matches[1]));

        assert_eq!(
            highlights,
            vec![SearchHighlight {
                range: CellRange {
                    start: CellPoint::new(1, 0),
                    end: CellPoint::new(1, 4),
                },
                active: true,
            }]
        );

        assert!(terminal.scroll_to_search_match(matches[0].row, 1));
        let highlights = terminal.visible_search_highlights(&matches, Some(matches[1]));
        assert_eq!(
            highlights,
            vec![
                SearchHighlight {
                    range: CellRange {
                        start: CellPoint::new(0, 0),
                        end: CellPoint::new(0, 4),
                    },
                    active: false,
                },
                SearchHighlight {
                    range: CellRange {
                        start: CellPoint::new(2, 0),
                        end: CellPoint::new(2, 4),
                    },
                    active: true,
                },
            ]
        );
    }

    #[test]
    fn search_uses_current_alternate_screen_only() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 12));

        terminal.feed(b"main\r\n");
        terminal.feed(b"\x1b[?1049h");
        terminal.feed(b"altfind");

        let rows = terminal.search_text_rows();
        assert_eq!(rows[0].text, "altfind");
        assert_eq!(rows[0].id, SearchRowId::screen(0));
        assert_eq!(rows[0].visible_row, Some(0));
        assert!(terminal
            .find_matches("main", SearchOptions::default())
            .is_empty());
        assert_eq!(
            terminal.find_matches("alt", SearchOptions::default()),
            vec![SearchMatch {
                row: SearchRowId::screen(0),
                start_col: 0,
                end_col: 2,
            }]
        );
        assert!(terminal.scroll_to_search_match(SearchRowId::screen(0), 1));
        assert!(!terminal.scroll_to_search_match(SearchRowId::scrollback(0), 1));
    }

    #[test]
    fn snapshot_records_wide_cells_and_skips_continuations() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 6));

        terminal.feed("a界b".as_bytes());
        let snapshot = terminal.snapshot();

        assert_eq!(snapshot.rows[0].cells[0].point, CellPoint::new(0, 0));
        assert_eq!(snapshot.rows[0].cells[0].text, "a");
        assert_eq!(snapshot.rows[0].cells[0].width, 1);
        assert_eq!(snapshot.rows[0].cells[1].point, CellPoint::new(0, 1));
        assert_eq!(snapshot.rows[0].cells[1].text, "界");
        assert_eq!(snapshot.rows[0].cells[1].width, 2);
        assert_eq!(snapshot.rows[0].cells[2].point, CellPoint::new(0, 3));
        assert_eq!(snapshot.rows[0].cells[2].text, "b");
        assert_eq!(snapshot.rows[0].cells[2].width, 1);
    }

    #[test]
    fn search_maps_wide_char_to_terminal_cell_span() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 8));

        terminal.feed("a界b".as_bytes());

        assert_eq!(
            terminal.search_text_rows()[0],
            SearchTextRow::with_columns(
                SearchRowId::screen(0),
                Some(0),
                "a界b",
                vec![
                    SearchTextColumn::new(0, 0),
                    SearchTextColumn::new(1, 2),
                    SearchTextColumn::new(3, 3),
                ],
            )
        );

        let matches = terminal.find_matches("界", SearchOptions::default());
        assert_eq!(
            matches,
            vec![SearchMatch {
                row: SearchRowId::screen(0),
                start_col: 1,
                end_col: 2,
            }]
        );
        assert_eq!(
            terminal.visible_search_matches(&matches),
            vec![CellRange {
                start: CellPoint::new(0, 1),
                end: CellPoint::new(0, 2),
            }]
        );
    }

    #[test]
    fn search_maps_combining_mark_sequence_to_base_cell() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 8));

        terminal.feed("e\u{0301}x".as_bytes());

        assert_eq!(
            terminal.search_text_rows()[0],
            SearchTextRow::with_columns(
                SearchRowId::screen(0),
                Some(0),
                "e\u{0301}x",
                vec![
                    SearchTextColumn::new(0, 0),
                    SearchTextColumn::new(0, 0),
                    SearchTextColumn::new(1, 1),
                ],
            )
        );

        let matches = terminal.find_matches("e\u{0301}", SearchOptions::default());
        assert_eq!(
            matches,
            vec![SearchMatch {
                row: SearchRowId::screen(0),
                start_col: 0,
                end_col: 0,
            }]
        );
        assert_eq!(
            terminal.visible_search_matches(&matches),
            vec![CellRange {
                start: CellPoint::new(0, 0),
                end: CellPoint::new(0, 0),
            }]
        );
    }

    #[test]
    fn bracketed_paste_mode_tracks_private_mode_sequence() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        assert!(!terminal.bracketed_paste_enabled());

        terminal.feed(b"\x1b[?2004h");
        assert!(terminal.bracketed_paste_enabled());

        terminal.feed(b"\x1b[?2004l");
        assert!(!terminal.bracketed_paste_enabled());
    }

    #[test]
    fn bracketed_paste_mode_is_cleared_by_full_reset() {
        let mut terminal = BasicTerminal::new(GridSize::new(3, 8));

        terminal.feed(b"\x1b[?2004h");
        terminal.feed(b"\x1bc");

        assert!(!terminal.bracketed_paste_enabled());
    }

    #[test]
    fn bell_queues_host_action_without_rendering_text_or_damage() {
        let mut terminal = BasicTerminal::new(GridSize::new(2, 8));
        terminal.feed(b"ab");
        assert_eq!(row_text(&terminal.take_snapshot(), 0), "ab      ");

        terminal.feed(b"\x07");
        let snapshot = terminal.take_snapshot();

        assert_eq!(
            terminal.drain_host_actions(),
            vec![TerminalHostAction::Bell]
        );
        assert_eq!(row_text(&snapshot, 0), "ab      ");
        assert_eq!(snapshot.damage, DamageRegion::Rows(Vec::new()));
    }

    fn row_text(snapshot: &RenderSnapshot, row: usize) -> String {
        snapshot.rows[row]
            .cells
            .iter()
            .map(|cell| cell.text.as_str())
            .collect()
    }

    fn cell_row_text(row: &[BasicCell]) -> String {
        row.iter().map(|cell| cell.text.as_str()).collect()
    }

    fn hyperlink_at(snapshot: &RenderSnapshot, row: usize, col: u16) -> Option<HyperlinkId> {
        snapshot.rows[row]
            .cells
            .iter()
            .find(|cell| cell.point.col == col)
            .and_then(|cell| cell.hyperlink)
    }

    fn clipboard_write(selection: TerminalClipboardSelection, text: &str) -> TerminalHostAction {
        TerminalHostAction::ClipboardWrite(TerminalClipboardWrite {
            selection,
            text: text.to_owned(),
            decoded_bytes: text.len(),
        })
    }

    fn terminal_reply(bytes: &[u8]) -> TerminalHostAction {
        TerminalHostAction::TerminalReply(TerminalHostReply {
            bytes: bytes.to_vec(),
        })
    }

    fn current_directory(
        uri: &str,
        host: Option<&str>,
        path: &str,
        screen: TerminalScreen,
        point: CellPoint,
    ) -> TerminalHostAction {
        TerminalHostAction::CurrentDirectory(TerminalCurrentDirectory {
            uri: uri.to_owned(),
            host: host.map(ToOwned::to_owned),
            path: path.to_owned(),
            screen,
            point,
            anchor: Some(TerminalPointAnchor {
                row: TerminalRowAnchor {
                    screen,
                    row: u64::from(point.row),
                },
                col: point.col,
            }),
        })
    }

    fn shell_integration_event(
        marker: TerminalShellIntegrationMarker,
        screen: TerminalScreen,
        point: CellPoint,
        exit_code: Option<i32>,
    ) -> TerminalHostAction {
        shell_integration_event_with_anchor_row(
            marker,
            screen,
            point,
            u64::from(point.row),
            exit_code,
        )
    }

    fn shell_integration_event_with_anchor_row(
        marker: TerminalShellIntegrationMarker,
        screen: TerminalScreen,
        point: CellPoint,
        anchor_row: u64,
        exit_code: Option<i32>,
    ) -> TerminalHostAction {
        TerminalHostAction::ShellIntegration(TerminalShellIntegrationEvent {
            marker,
            screen,
            point,
            anchor: Some(TerminalPointAnchor {
                row: TerminalRowAnchor {
                    screen,
                    row: anchor_row,
                },
                col: point.col,
            }),
            exit_code,
        })
    }
}

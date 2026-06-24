//! Core terminal data structures shared by parser, renderer, and UI layers.

mod basic_terminal;
mod mouse;

use std::{error::Error, fmt};

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;

pub use basic_terminal::{parse_terminal_color, BasicTerminal, TerminalColorTheme};
pub use mouse::{
    encode_terminal_focus_event, encode_terminal_mouse_event, FocusEventKind, MouseButtonCode,
    MouseEventKind, MouseModifiers, PixelMousePosition, TerminalMouseEvent,
};

pub fn paste_payload(text: &str, bracketed_paste: bool) -> Vec<u8> {
    if !bracketed_paste {
        return text.as_bytes().to_vec();
    }

    let mut payload = Vec::with_capacity(text.len() + b"\x1b[200~".len() + b"\x1b[201~".len());
    payload.extend_from_slice(b"\x1b[200~");
    payload.extend_from_slice(text.as_bytes());
    payload.extend_from_slice(b"\x1b[201~");
    payload
}

pub const MAX_EXTERNAL_URL_BYTES: usize = 2048;
pub const MAX_OSC52_DECODED_BYTES: usize = 64 * 1024;
pub const DEFAULT_MAX_SCROLLBACK_LINES: usize = 10_000;
pub const KITTY_KEYBOARD_DISAMBIGUATE_ESC_CODES: u16 = 1;
pub const KITTY_KEYBOARD_REPORT_EVENT_TYPES: u16 = 2;
pub const KITTY_KEYBOARD_REPORT_ALL_KEYS_AS_ESC_CODES: u16 = 8;
pub const KITTY_KEYBOARD_REPORT_ASSOCIATED_TEXT: u16 = 16;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExternalUrlError {
    Empty,
    TooLong { max_bytes: usize },
    ControlCharacter,
    InvalidScheme,
    UnsupportedScheme { scheme: String },
}

impl fmt::Display for ExternalUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "URL is empty"),
            Self::TooLong { max_bytes } => write!(f, "URL exceeds {max_bytes} bytes"),
            Self::ControlCharacter => write!(f, "URL contains control characters"),
            Self::InvalidScheme => write!(f, "URL is missing a valid scheme"),
            Self::UnsupportedScheme { scheme } => {
                write!(f, "URL scheme is not allowed: {scheme}")
            }
        }
    }
}

impl Error for ExternalUrlError {}

pub fn validate_external_url(uri: &str) -> Result<(), ExternalUrlError> {
    if uri.is_empty() {
        return Err(ExternalUrlError::Empty);
    }
    if uri.len() > MAX_EXTERNAL_URL_BYTES {
        return Err(ExternalUrlError::TooLong {
            max_bytes: MAX_EXTERNAL_URL_BYTES,
        });
    }
    if uri.chars().any(char::is_control) {
        return Err(ExternalUrlError::ControlCharacter);
    }

    let Some(scheme) = external_url_scheme(uri) else {
        return Err(ExternalUrlError::InvalidScheme);
    };
    if !matches!(
        scheme.to_ascii_lowercase().as_str(),
        "http" | "https" | "mailto"
    ) {
        return Err(ExternalUrlError::UnsupportedScheme {
            scheme: scheme.to_owned(),
        });
    }

    Ok(())
}

fn external_url_scheme(uri: &str) -> Option<&str> {
    let separator = uri.find(':')?;
    let scheme = &uri[..separator];
    if scheme.is_empty() {
        return None;
    }
    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.')) {
        return None;
    }
    Some(scheme)
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Osc52ClipboardPolicy {
    #[default]
    Disabled,
    Confirm,
    Allow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalClipboardSelection {
    Clipboard,
    Primary,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalClipboardWrite {
    pub selection: TerminalClipboardSelection,
    pub text: String,
    pub decoded_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalHostReply {
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalScreen {
    Main,
    Alternate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalShellIntegrationMarker {
    PromptStart,
    CommandStart,
    OutputStart,
    CommandFinished,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalRowAnchor {
    pub screen: TerminalScreen,
    pub row: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalPointAnchor {
    pub row: TerminalRowAnchor,
    pub col: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalVisibleRowAnchor {
    pub visible_row: u16,
    pub anchor: TerminalRowAnchor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalShellIntegrationEvent {
    pub marker: TerminalShellIntegrationMarker,
    pub screen: TerminalScreen,
    pub point: CellPoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<TerminalPointAnchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalCurrentDirectory {
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    pub path: String,
    pub screen: TerminalScreen,
    pub point: CellPoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<TerminalPointAnchor>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TerminalHostAction {
    ClipboardWrite(TerminalClipboardWrite),
    TerminalReply(TerminalHostReply),
    ShellIntegration(TerminalShellIntegrationEvent),
    CurrentDirectory(TerminalCurrentDirectory),
    Bell,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalInputModes {
    pub application_cursor_keys: bool,
    pub application_keypad: bool,
    #[serde(default)]
    pub keyboard_locked: bool,
    #[serde(default)]
    pub backarrow_sends_backspace: bool,
    #[serde(default)]
    pub kitty_keyboard_flags: u16,
    pub mouse: TerminalMouseModes,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalMouseModes {
    pub tracking: MouseTrackingMode,
    pub encoding: MouseEncodingMode,
    pub focus_events: bool,
    pub alternate_scroll: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum MouseTrackingMode {
    #[default]
    None,
    X10,
    Normal,
    ButtonEvent,
    AnyEvent,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum MouseEncodingMode {
    #[default]
    X10,
    Utf8,
    Urxvt,
    Sgr,
    SgrPixels,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct GridSize {
    pub rows: u16,
    pub cols: u16,
}

impl GridSize {
    pub const fn new(rows: u16, cols: u16) -> Self {
        Self { rows, cols }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CellPoint {
    pub row: u16,
    pub col: u16,
}

impl CellPoint {
    pub const fn new(row: u16, col: u16) -> Self {
        Self { row, col }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CellRange {
    pub start: CellPoint,
    pub end: CellPoint,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalTextRange {
    pub screen: TerminalScreen,
    pub start: CellPoint,
    pub end_exclusive: CellPoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_anchor: Option<TerminalPointAnchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_exclusive_anchor: Option<TerminalPointAnchor>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SearchRowKind {
    Scrollback,
    Screen,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SearchRowId {
    pub kind: SearchRowKind,
    pub index: usize,
}

impl SearchRowId {
    pub const fn scrollback(index: usize) -> Self {
        Self {
            kind: SearchRowKind::Scrollback,
            index,
        }
    }

    pub const fn screen(index: usize) -> Self {
        Self {
            kind: SearchRowKind::Screen,
            index,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SearchTextRow {
    pub id: SearchRowId,
    pub visible_row: Option<u16>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<SearchTextColumn>,
}

impl SearchTextRow {
    pub fn new(id: SearchRowId, visible_row: Option<u16>, text: impl Into<String>) -> Self {
        Self {
            id,
            visible_row,
            text: text.into(),
            columns: Vec::new(),
        }
    }

    pub fn with_columns(
        id: SearchRowId,
        visible_row: Option<u16>,
        text: impl Into<String>,
        columns: Vec<SearchTextColumn>,
    ) -> Self {
        Self {
            id,
            visible_row,
            text: text.into(),
            columns,
        }
    }

    pub(crate) fn cell_range_for_char_range(&self, start: usize, end: usize) -> Option<(u16, u16)> {
        let (start, end_exclusive) =
            expand_char_range_to_grapheme_clusters(&self.text, start, end)?;
        let end = end_exclusive.checked_sub(1)?;
        if self.columns.is_empty() {
            return Some((u16::try_from(start).ok()?, u16::try_from(end).ok()?));
        }

        let start_col = self.columns.get(start)?.start_col;
        let end_col = self
            .columns
            .get(start..=end)?
            .iter()
            .map(|span| span.end_col)
            .max()?;
        Some((start_col, end_col))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TextClusterSpan {
    pub char_start: usize,
    pub char_end_exclusive: usize,
}

pub(crate) fn grapheme_cluster_spans(text: &str) -> Vec<TextClusterSpan> {
    let mut spans = Vec::new();
    let mut char_start = 0usize;

    for grapheme in UnicodeSegmentation::graphemes(text, true) {
        let char_end_exclusive = char_start + grapheme.chars().count();
        spans.push(TextClusterSpan {
            char_start,
            char_end_exclusive,
        });
        char_start = char_end_exclusive;
    }

    spans
}

fn expand_char_range_to_grapheme_clusters(
    text: &str,
    start: usize,
    end: usize,
) -> Option<(usize, usize)> {
    if start > end {
        return None;
    }

    let spans = grapheme_cluster_spans(text);
    let first = spans.iter().find(|span| span.char_end_exclusive > start)?;
    let last = spans.iter().rev().find(|span| span.char_start <= end)?;

    Some((first.char_start, last.char_end_exclusive))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SearchTextColumn {
    pub start_col: u16,
    pub end_col: u16,
}

impl SearchTextColumn {
    pub const fn new(start_col: u16, end_col: u16) -> Self {
        Self { start_col, end_col }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SearchMatch {
    pub row: SearchRowId,
    pub start_col: u16,
    pub end_col: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SearchHighlight {
    pub range: CellRange,
    pub active: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub regex: bool,
    pub whole_word: bool,
    #[serde(default)]
    pub normalize_nfc: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SearchError {
    InvalidRegex { message: String },
}

impl SearchError {
    pub fn invalid_regex(message: impl Into<String>) -> Self {
        Self::InvalidRegex {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::InvalidRegex { message } => message,
        }
    }
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRegex { message } => write!(f, "invalid regex: {message}"),
        }
    }
}

impl Error for SearchError {}

pub type SearchResult<T> = Result<T, SearchError>;

enum SearchMatcher {
    Literal {
        query: String,
        options: SearchOptions,
    },
    Regex {
        regex: Regex,
        options: SearchOptions,
    },
}

pub fn find_search_matches(
    rows: &[SearchTextRow],
    query: &str,
    options: SearchOptions,
) -> Vec<SearchMatch> {
    try_find_search_matches(rows, query, options).unwrap_or_default()
}

pub fn try_find_search_matches(
    rows: &[SearchTextRow],
    query: &str,
    options: SearchOptions,
) -> SearchResult<Vec<SearchMatch>> {
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let matcher = SearchMatcher::new(query, options)?;
    Ok(rows
        .iter()
        .flat_map(|row| {
            matcher
                .match_ranges(&row.text)
                .into_iter()
                .filter_map(|(start_char, end_char)| {
                    let (start_col, end_col) =
                        row.cell_range_for_char_range(start_char, end_char)?;
                    Some(SearchMatch {
                        row: row.id,
                        start_col,
                        end_col,
                    })
                })
        })
        .collect())
}

impl SearchMatcher {
    fn new(query: &str, options: SearchOptions) -> SearchResult<Self> {
        if options.regex {
            let regex = RegexBuilder::new(query)
                .case_insensitive(!options.case_sensitive)
                .build()
                .map_err(|err| SearchError::invalid_regex(err.to_string()))?;

            Ok(Self::Regex { regex, options })
        } else {
            Ok(Self::Literal {
                query: query.to_owned(),
                options,
            })
        }
    }

    fn match_ranges(&self, text: &str) -> Vec<(usize, usize)> {
        match self {
            Self::Literal { query, options } if options.normalize_nfc => {
                normalized_literal_match_ranges(text, query, *options)
            }
            Self::Literal { query, options } => {
                literal_match_ranges(text, &query.chars().collect::<Vec<_>>(), *options)
            }
            Self::Regex { regex, options } => regex_match_ranges(text, regex, *options),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct NormalizedCharSource {
    original_char_start: usize,
    original_char_end_exclusive: usize,
}

fn normalized_literal_match_ranges(
    text: &str,
    query: &str,
    options: SearchOptions,
) -> Vec<(usize, usize)> {
    let normalized_query = query.nfc().collect::<String>();
    let normalized_query_chars = normalized_query.chars().collect::<Vec<_>>();
    let Some((normalized_text, source_map)) = normalized_search_projection(text) else {
        return Vec::new();
    };

    literal_match_ranges(&normalized_text, &normalized_query_chars, options)
        .into_iter()
        .filter_map(|(normalized_start, normalized_end)| {
            let original_start = source_map.get(normalized_start)?.original_char_start;
            let original_end = source_map.get(normalized_end)?.original_char_end_exclusive;
            Some((original_start, original_end.checked_sub(1)?))
        })
        .collect()
}

fn normalized_search_projection(text: &str) -> Option<(String, Vec<NormalizedCharSource>)> {
    let spans = grapheme_cluster_spans(text);
    if spans.is_empty() {
        return None;
    }

    let chars = text.chars().collect::<Vec<_>>();
    let mut normalized_text = String::new();
    let mut source_map = Vec::new();
    for span in spans {
        let cluster_text = chars[span.char_start..span.char_end_exclusive]
            .iter()
            .collect::<String>();
        for normalized_char in cluster_text.nfc() {
            normalized_text.push(normalized_char);
            source_map.push(NormalizedCharSource {
                original_char_start: span.char_start,
                original_char_end_exclusive: span.char_end_exclusive,
            });
        }
    }

    Some((normalized_text, source_map))
}

fn literal_match_ranges(
    text: &str,
    query_chars: &[char],
    options: SearchOptions,
) -> Vec<(usize, usize)> {
    let text_chars = text.chars().collect::<Vec<_>>();
    if text_chars.is_empty() || query_chars.is_empty() || query_chars.len() > text_chars.len() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    let mut start = 0;
    while start + query_chars.len() <= text_chars.len() {
        let matched = query_chars.iter().enumerate().all(|(offset, query_char)| {
            search_chars_equal(text_chars[start + offset], *query_char, options)
        });

        if matched {
            let end = start + query_chars.len() - 1;
            if is_allowed_search_word_range(&text_chars, start, end + 1, options) {
                matches.push((start, end));
            }
            start += query_chars.len();
        } else {
            start += 1;
        }
    }
    matches
}

fn regex_match_ranges(text: &str, regex: &Regex, options: SearchOptions) -> Vec<(usize, usize)> {
    let text_chars = text.chars().collect::<Vec<_>>();
    regex
        .find_iter(text)
        .filter_map(|regex_match| {
            byte_range_to_char_range(text, regex_match.start(), regex_match.end())
        })
        .filter(|(start, end)| is_allowed_search_word_range(&text_chars, *start, end + 1, options))
        .collect()
}

fn byte_range_to_char_range(
    text: &str,
    byte_start: usize,
    byte_end: usize,
) -> Option<(usize, usize)> {
    if byte_start >= byte_end || byte_end > text.len() {
        return None;
    }

    let start = text[..byte_start].chars().count();
    let end = text[..byte_end].chars().count().checked_sub(1)?;
    Some((start, end))
}

fn is_allowed_search_word_range(
    text_chars: &[char],
    start: usize,
    end_exclusive: usize,
    options: SearchOptions,
) -> bool {
    if !options.whole_word {
        return true;
    }

    let before_is_word = start
        .checked_sub(1)
        .and_then(|index| text_chars.get(index))
        .is_some_and(|ch| is_search_word_char(*ch));
    let after_is_word = text_chars
        .get(end_exclusive)
        .is_some_and(|ch| is_search_word_char(*ch));
    !before_is_word && !after_is_word
}

fn search_chars_equal(left: char, right: char, options: SearchOptions) -> bool {
    if options.case_sensitive {
        left == right
    } else {
        left.to_lowercase().eq(right.to_lowercase())
    }
}

fn is_search_word_char(ch: char) -> bool {
    ch.is_alphanumeric()
        || matches!(
            ch,
            '_' | '-' | '.' | '/' | '\\' | ':' | '@' | '~' | '+' | '=' | '%' | '$'
        )
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum DamageRegion {
    #[default]
    Full,
    Rows(Vec<u16>),
    Rects(Vec<CellRange>),
}

impl DamageRegion {
    pub fn is_full(&self) -> bool {
        matches!(self, Self::Full)
    }

    pub fn region_count(&self) -> usize {
        match self {
            Self::Full => 1,
            Self::Rows(rows) => rows.len(),
            Self::Rects(rects) => rects.len(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum CursorShape {
    Block,
    Bar,
    Underline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CursorState {
    pub position: CellPoint,
    pub shape: CursorShape,
    pub visible: bool,
    #[serde(default = "default_cursor_blink")]
    pub blink: bool,
}

fn default_cursor_blink() -> bool {
    true
}

impl Default for CursorState {
    fn default() -> Self {
        Self {
            position: CellPoint::new(0, 0),
            shape: CursorShape::Block,
            visible: true,
            blink: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn with_alpha(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnderlineStyle {
    #[default]
    Single,
    Double,
    Curly,
    Dotted,
    Dashed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineShift {
    #[default]
    Normal,
    Superscript,
    Subscript,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CellFlags {
    pub bold: bool,
    #[serde(default)]
    pub faint: bool,
    pub italic: bool,
    pub underline: bool,
    #[serde(default)]
    pub underline_style: UnderlineStyle,
    pub strike: bool,
    pub reverse: bool,
    #[serde(default)]
    pub blink: bool,
    #[serde(default)]
    pub overline: bool,
    #[serde(default)]
    pub conceal: bool,
    #[serde(default)]
    pub framed: bool,
    #[serde(default)]
    pub encircled: bool,
    #[serde(default)]
    pub baseline_shift: BaselineShift,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CellStyle {
    pub foreground: Rgba,
    pub background: Rgba,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underline_color: Option<Rgba>,
    pub flags: CellFlags,
}

impl Default for CellStyle {
    fn default() -> Self {
        Self {
            foreground: Rgba::WHITE,
            background: Rgba::BLACK,
            underline_color: None,
            flags: CellFlags::default(),
        }
    }
}

pub type HyperlinkId = u32;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TerminalHyperlink {
    pub id: HyperlinkId,
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub osc8_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RenderCell {
    pub point: CellPoint,
    pub text: String,
    pub width: u8,
    pub style: CellStyle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hyperlink: Option<HyperlinkId>,
}

impl RenderCell {
    pub fn new(point: CellPoint, text: impl Into<String>) -> Self {
        Self {
            point,
            text: text.into(),
            width: 1,
            style: CellStyle::default(),
            hyperlink: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RenderRow {
    pub row: u16,
    pub cells: Vec<RenderCell>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RenderSnapshot {
    pub size: GridSize,
    pub rows: Vec<RenderRow>,
    #[serde(default = "default_cell_background")]
    pub default_background: Rgba,
    pub cursor: CursorState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor_color: Option<Rgba>,
    pub selection: Option<CellRange>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub search_highlights: Vec<SearchHighlight>,
    pub damage: DamageRegion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hyperlinks: Vec<TerminalHyperlink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hovered_hyperlink: Option<HyperlinkId>,
}

impl RenderSnapshot {
    pub fn empty(size: GridSize) -> Self {
        Self {
            size,
            rows: Vec::new(),
            default_background: default_cell_background(),
            cursor: CursorState::default(),
            cursor_color: None,
            selection: None,
            search_highlights: Vec::new(),
            damage: DamageRegion::Full,
            title: None,
            hyperlinks: Vec::new(),
            hovered_hyperlink: None,
        }
    }

    pub fn from_plain_lines(lines: &[&str]) -> Self {
        let rows = lines
            .iter()
            .enumerate()
            .map(|(row, line)| RenderRow {
                row: row as u16,
                cells: line
                    .chars()
                    .scan(0u16, |col, ch| {
                        let width = terminal_char_width(ch).max(1);
                        let cell = RenderCell {
                            point: CellPoint::new(row as u16, *col),
                            text: ch.to_string(),
                            width,
                            style: CellStyle::default(),
                            hyperlink: None,
                        };
                        *col = col.saturating_add(u16::from(width));
                        Some(cell)
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();

        let cols = lines
            .iter()
            .map(|line| line.chars().map(terminal_char_width).map(u16::from).sum())
            .max()
            .unwrap_or(0);

        Self {
            size: GridSize::new(lines.len() as u16, cols),
            rows,
            default_background: default_cell_background(),
            cursor: CursorState::default(),
            cursor_color: None,
            selection: None,
            search_highlights: Vec::new(),
            damage: DamageRegion::Full,
            title: None,
            hyperlinks: Vec::new(),
            hovered_hyperlink: None,
        }
    }

    pub fn hyperlink_id_at(&self, point: CellPoint) -> Option<HyperlinkId> {
        self.rows
            .iter()
            .find(|row| row.row == point.row)?
            .cells
            .iter()
            .find(|cell| {
                let width = u16::from(cell.width.max(1));
                point.col >= cell.point.col && point.col < cell.point.col.saturating_add(width)
            })?
            .hyperlink
    }

    pub fn hyperlink_at(&self, point: CellPoint) -> Option<&TerminalHyperlink> {
        let id = self.hyperlink_id_at(point)?;
        self.hyperlinks.iter().find(|link| link.id == id)
    }
}

fn default_cell_background() -> Rgba {
    CellStyle::default().background
}

pub fn terminal_char_width(ch: char) -> u8 {
    UnicodeWidthChar::width(ch).unwrap_or(1).min(2) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_lines_build_cells() {
        let snapshot = RenderSnapshot::from_plain_lines(&["$ echo ok", "ok"]);

        assert_eq!(snapshot.size, GridSize::new(2, 9));
        assert_eq!(snapshot.rows[0].cells[0].text, "$");
        assert_eq!(snapshot.rows[1].cells[1].text, "k");
        assert_eq!(snapshot.damage, DamageRegion::Full);
    }

    #[test]
    fn external_url_policy_allows_initial_hyperlink_schemes() {
        for uri in [
            "http://example.com",
            "https://example.com/docs?q=1",
            "mailto:dev@example.com",
            "HTTPS://example.com",
        ] {
            validate_external_url(uri).unwrap();
        }
    }

    #[test]
    fn external_url_policy_rejects_unsafe_or_unsupported_values() {
        assert_eq!(validate_external_url(""), Err(ExternalUrlError::Empty));
        assert_eq!(
            validate_external_url("example.com/no-scheme"),
            Err(ExternalUrlError::InvalidScheme)
        );
        assert_eq!(
            validate_external_url("file:///tmp/example"),
            Err(ExternalUrlError::UnsupportedScheme {
                scheme: "file".to_owned(),
            })
        );
        assert_eq!(
            validate_external_url("https://example.com/\nnext"),
            Err(ExternalUrlError::ControlCharacter)
        );

        let oversized = format!("https://example.com/{}", "a".repeat(MAX_EXTERNAL_URL_BYTES));
        assert_eq!(
            validate_external_url(&oversized),
            Err(ExternalUrlError::TooLong {
                max_bytes: MAX_EXTERNAL_URL_BYTES,
            })
        );
    }

    #[test]
    fn plain_lines_build_wide_cells() {
        let snapshot = RenderSnapshot::from_plain_lines(&["a界b"]);

        assert_eq!(snapshot.size, GridSize::new(1, 4));
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
    fn cursor_state_deserializes_legacy_json_with_blink_default() {
        let json = r#"{"position":{"row":2,"col":3},"shape":"Bar","visible":true}"#;

        let cursor: CursorState = serde_json::from_str(json).unwrap();

        assert_eq!(cursor.position, CellPoint::new(2, 3));
        assert_eq!(cursor.shape, CursorShape::Bar);
        assert!(cursor.visible);
        assert!(cursor.blink);
    }

    #[test]
    fn cell_flags_deserialize_legacy_json_with_new_flags_defaulted() {
        let json = r#"{"bold":true,"italic":true,"underline":false,"strike":false,"reverse":true}"#;

        let flags: CellFlags = serde_json::from_str(json).unwrap();

        assert!(flags.bold);
        assert!(flags.italic);
        assert!(flags.reverse);
        assert_eq!(flags.underline_style, UnderlineStyle::Single);
        assert!(!flags.faint);
        assert!(!flags.blink);
        assert!(!flags.overline);
        assert!(!flags.conceal);
        assert!(!flags.framed);
        assert!(!flags.encircled);
        assert_eq!(flags.baseline_shift, BaselineShift::Normal);
    }

    #[test]
    fn cell_style_deserializes_legacy_json_without_underline_color() {
        let json = r#"{"foreground":{"r":1,"g":2,"b":3,"a":255},"background":{"r":4,"g":5,"b":6,"a":255},"flags":{"bold":false,"italic":false,"underline":false,"strike":false,"reverse":false}}"#;

        let style: CellStyle = serde_json::from_str(json).unwrap();

        assert_eq!(style.foreground, Rgba::rgb(1, 2, 3));
        assert_eq!(style.background, Rgba::rgb(4, 5, 6));
        assert_eq!(style.underline_color, None);
    }

    #[test]
    fn snapshot_hyperlink_hit_test_uses_cell_spans_and_visible_table() {
        let mut snapshot = RenderSnapshot::from_plain_lines(&["a界b"]);
        snapshot.rows[0].cells[1].hyperlink = Some(42);
        snapshot.hyperlinks = vec![TerminalHyperlink {
            id: 42,
            uri: "https://example.com".to_owned(),
            osc8_id: Some("doc".to_owned()),
        }];

        assert_eq!(snapshot.hyperlink_id_at(CellPoint::new(0, 1)), Some(42));
        assert_eq!(snapshot.hyperlink_id_at(CellPoint::new(0, 2)), Some(42));
        assert_eq!(snapshot.hyperlink_id_at(CellPoint::new(0, 3)), None);
        assert_eq!(
            snapshot
                .hyperlink_at(CellPoint::new(0, 2))
                .map(|link| link.uri.as_str()),
            Some("https://example.com")
        );
    }

    #[test]
    fn damage_region_reports_scope() {
        assert!(DamageRegion::Full.is_full());
        assert_eq!(DamageRegion::Full.region_count(), 1);
        assert_eq!(DamageRegion::Rows(vec![1, 3]).region_count(), 2);
        assert_eq!(
            DamageRegion::Rects(vec![CellRange {
                start: CellPoint::new(0, 1),
                end: CellPoint::new(2, 3),
            }])
            .region_count(),
            1
        );
    }

    #[test]
    fn search_literal_matches_rows_with_case_options() {
        let rows = vec![SearchTextRow {
            id: SearchRowId::screen(0),
            visible_row: Some(0),
            text: "Error error".to_owned(),
            columns: Vec::new(),
        }];

        assert_eq!(
            find_search_matches(&rows, "error", SearchOptions::default()),
            vec![
                SearchMatch {
                    row: SearchRowId::screen(0),
                    start_col: 0,
                    end_col: 4,
                },
                SearchMatch {
                    row: SearchRowId::screen(0),
                    start_col: 6,
                    end_col: 10,
                },
            ]
        );

        assert_eq!(
            find_search_matches(
                &rows,
                "error",
                SearchOptions {
                    case_sensitive: true,
                    ..SearchOptions::default()
                },
            ),
            vec![SearchMatch {
                row: SearchRowId::screen(0),
                start_col: 6,
                end_col: 10,
            }]
        );
    }

    #[test]
    fn search_literal_ignores_empty_query() {
        let rows = vec![SearchTextRow {
            id: SearchRowId::screen(0),
            visible_row: Some(0),
            text: "abc".to_owned(),
            columns: Vec::new(),
        }];

        assert!(find_search_matches(&rows, "", SearchOptions::default()).is_empty());
    }

    #[test]
    fn search_literal_whole_word_filters_embedded_matches() {
        let rows = vec![SearchTextRow {
            id: SearchRowId::screen(0),
            visible_row: Some(0),
            text: "alpha alpha_beta beta alpha".to_owned(),
            columns: Vec::new(),
        }];

        assert_eq!(
            find_search_matches(
                &rows,
                "alpha",
                SearchOptions {
                    whole_word: true,
                    ..SearchOptions::default()
                },
            ),
            vec![
                SearchMatch {
                    row: SearchRowId::screen(0),
                    start_col: 0,
                    end_col: 4,
                },
                SearchMatch {
                    row: SearchRowId::screen(0),
                    start_col: 22,
                    end_col: 26,
                },
            ]
        );
    }

    #[test]
    fn search_regex_matches_rows_with_case_and_whole_word_options() {
        let rows = vec![SearchTextRow {
            id: SearchRowId::screen(0),
            visible_row: Some(0),
            text: "ERR42 err7 ferr8".to_owned(),
            columns: Vec::new(),
        }];

        assert_eq!(
            try_find_search_matches(
                &rows,
                r"err\d+",
                SearchOptions {
                    regex: true,
                    whole_word: true,
                    ..SearchOptions::default()
                },
            )
            .unwrap(),
            vec![
                SearchMatch {
                    row: SearchRowId::screen(0),
                    start_col: 0,
                    end_col: 4,
                },
                SearchMatch {
                    row: SearchRowId::screen(0),
                    start_col: 6,
                    end_col: 9,
                },
            ]
        );

        assert_eq!(
            try_find_search_matches(
                &rows,
                r"err\d+",
                SearchOptions {
                    case_sensitive: true,
                    regex: true,
                    ..SearchOptions::default()
                },
            )
            .unwrap(),
            vec![
                SearchMatch {
                    row: SearchRowId::screen(0),
                    start_col: 6,
                    end_col: 9,
                },
                SearchMatch {
                    row: SearchRowId::screen(0),
                    start_col: 12,
                    end_col: 15,
                },
            ]
        );
    }

    #[test]
    fn search_maps_character_ranges_to_terminal_columns() {
        let rows = vec![SearchTextRow::with_columns(
            SearchRowId::screen(0),
            Some(0),
            "a界e\u{0301}",
            vec![
                SearchTextColumn::new(0, 0),
                SearchTextColumn::new(1, 2),
                SearchTextColumn::new(3, 3),
                SearchTextColumn::new(3, 3),
            ],
        )];

        assert_eq!(
            find_search_matches(&rows, "界", SearchOptions::default()),
            vec![SearchMatch {
                row: SearchRowId::screen(0),
                start_col: 1,
                end_col: 2,
            }]
        );
        assert_eq!(
            find_search_matches(&rows, "e\u{0301}", SearchOptions::default()),
            vec![SearchMatch {
                row: SearchRowId::screen(0),
                start_col: 3,
                end_col: 3,
            }]
        );
        assert_eq!(
            try_find_search_matches(
                &rows,
                r"界e\p{M}",
                SearchOptions {
                    regex: true,
                    ..SearchOptions::default()
                },
            )
            .unwrap(),
            vec![SearchMatch {
                row: SearchRowId::screen(0),
                start_col: 1,
                end_col: 3,
            }]
        );
    }

    #[test]
    fn search_expands_matches_to_grapheme_cluster_spans() {
        let rows = vec![
            SearchTextRow::with_columns(
                SearchRowId::screen(0),
                Some(0),
                "a👩\u{200d}💻z",
                vec![
                    SearchTextColumn::new(0, 0),
                    SearchTextColumn::new(1, 2),
                    SearchTextColumn::new(1, 2),
                    SearchTextColumn::new(3, 4),
                    SearchTextColumn::new(5, 5),
                ],
            ),
            SearchTextRow::with_columns(
                SearchRowId::screen(1),
                Some(1),
                "🇺🇸",
                vec![SearchTextColumn::new(0, 0), SearchTextColumn::new(1, 1)],
            ),
            SearchTextRow::with_columns(
                SearchRowId::screen(2),
                Some(2),
                "👍🏽",
                vec![SearchTextColumn::new(0, 1), SearchTextColumn::new(0, 1)],
            ),
        ];

        assert_eq!(
            find_search_matches(&rows, "💻", SearchOptions::default()),
            vec![SearchMatch {
                row: SearchRowId::screen(0),
                start_col: 1,
                end_col: 4,
            }]
        );
        assert_eq!(
            find_search_matches(&rows, "🇺", SearchOptions::default()),
            vec![SearchMatch {
                row: SearchRowId::screen(1),
                start_col: 0,
                end_col: 1,
            }]
        );
        assert_eq!(
            find_search_matches(&rows, "🏽", SearchOptions::default()),
            vec![SearchMatch {
                row: SearchRowId::screen(2),
                start_col: 0,
                end_col: 1,
            }]
        );
    }

    #[test]
    fn search_literal_can_use_optional_nfc_projection() {
        let rows = vec![
            SearchTextRow::with_columns(
                SearchRowId::screen(0),
                Some(0),
                "e\u{0301}x",
                vec![
                    SearchTextColumn::new(0, 0),
                    SearchTextColumn::new(0, 0),
                    SearchTextColumn::new(1, 1),
                ],
            ),
            SearchTextRow::new(SearchRowId::screen(1), Some(1), "\u{00e9}x"),
        ];

        assert_eq!(
            find_search_matches(&rows, "\u{00e9}", SearchOptions::default()),
            vec![SearchMatch {
                row: SearchRowId::screen(1),
                start_col: 0,
                end_col: 0,
            }]
        );
        assert_eq!(
            find_search_matches(
                &rows,
                "\u{00e9}",
                SearchOptions {
                    normalize_nfc: true,
                    ..SearchOptions::default()
                },
            ),
            vec![
                SearchMatch {
                    row: SearchRowId::screen(0),
                    start_col: 0,
                    end_col: 0,
                },
                SearchMatch {
                    row: SearchRowId::screen(1),
                    start_col: 0,
                    end_col: 0,
                },
            ]
        );
        assert_eq!(
            find_search_matches(
                &rows,
                "e\u{0301}",
                SearchOptions {
                    normalize_nfc: true,
                    ..SearchOptions::default()
                },
            ),
            vec![
                SearchMatch {
                    row: SearchRowId::screen(0),
                    start_col: 0,
                    end_col: 0,
                },
                SearchMatch {
                    row: SearchRowId::screen(1),
                    start_col: 0,
                    end_col: 0,
                },
            ]
        );
    }

    #[test]
    fn search_regex_ignores_nfc_projection_option() {
        let rows = vec![SearchTextRow::with_columns(
            SearchRowId::screen(0),
            Some(0),
            "e\u{0301}",
            vec![SearchTextColumn::new(0, 0), SearchTextColumn::new(0, 0)],
        )];

        assert!(try_find_search_matches(
            &rows,
            "\u{00e9}",
            SearchOptions {
                regex: true,
                normalize_nfc: true,
                ..SearchOptions::default()
            },
        )
        .unwrap()
        .is_empty());
    }

    #[test]
    fn search_regex_reports_invalid_patterns_without_matches() {
        let rows = vec![SearchTextRow {
            id: SearchRowId::screen(0),
            visible_row: Some(0),
            text: "abc".to_owned(),
            columns: Vec::new(),
        }];

        let err = try_find_search_matches(
            &rows,
            "[",
            SearchOptions {
                regex: true,
                ..SearchOptions::default()
            },
        )
        .unwrap_err();

        assert!(err.to_string().contains("invalid regex"));
        assert!(find_search_matches(
            &rows,
            "[",
            SearchOptions {
                regex: true,
                ..SearchOptions::default()
            },
        )
        .is_empty());
    }

    #[test]
    fn paste_payload_leaves_plain_text_unwrapped() {
        assert_eq!(paste_payload("echo ok\n", false), b"echo ok\n");
    }

    #[test]
    fn paste_payload_wraps_bracketed_text() {
        assert_eq!(
            paste_payload("echo ok\n", true),
            b"\x1b[200~echo ok\n\x1b[201~"
        );
    }
}

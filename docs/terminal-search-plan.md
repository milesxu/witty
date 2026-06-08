# Terminal Search Plan

Updated: 2026-05-30

m114 plans the next product-level terminal feature after selection,
clipboard, mouse, keyboard, browser, and product launcher parity. The goal is
screen plus scrollback search with renderer highlights, native and browser UI
entry points, and a plugin-safe boundary.

Reference points from the local Warp checkout:

- `app/src/terminal/model/find.rs` keeps terminal find as a model-level search
  service rather than renderer-only highlighting.
- `app/src/terminal/block_list_viewport.rs` treats "scroll to find match" as a
  viewport update, with a buffer so the match is not hidden by UI chrome.
- `app/src/terminal/block_list_element.rs` renders find matches as grid
  overlay data, not by mutating terminal cells.

Witty should copy the shape of those boundaries, not AGPL source code.

## Current State

Witty already has the pieces needed to add search without a large
architecture break:

| Area | Current support | Search implication |
| --- | --- | --- |
| terminal state | `BasicTerminal` owns visible screen, private scrollback, viewport offset, and selection | search needs a read-only text/history view that includes scrollback |
| selection text | `BasicTerminal::selected_text()` extracts text from the current viewport | search can reuse the same visible-row normalization rules |
| word selection | `BasicTerminal::word_range_at()` maps viewport points to text ranges | search match ranges should use the same cell coordinate semantics |
| renderer | `FramePlanner` already separates background, glyph, selection, cursor, and damage | search highlights should be another overlay layer |
| retained planner | row damage caches terminal rows, dynamic overlays are composed later | query changes should avoid rebuilding terminal row caches |
| native UI | command palette and diagnostics overlays prove text-panel drawing | find bar can reuse this overlay style at smaller scope |
| browser UI | wasm session exposes selection and clipboard methods through `app.js` | browser search should expose synchronous wasm state updates plus JS shortcuts |
| plugin host | commands and permission-gated terminal writes exist | search commands can be registered without exposing terminal text by default |

The main missing primitive is a stable, read-only terminal text coordinate model
that can address scrollback rows as well as visible screen rows.

## Product Scope

First version:

| Feature | Decision |
| --- | --- |
| open shortcut | `Ctrl+Shift+F` in native and browser |
| query mode | literal substring search |
| case mode | case-insensitive by default, case-sensitive option in the model |
| result scope | main-screen scrollback plus current visible screen |
| alternate screen | search current alternate screen only, no main scrollback while alternate screen is active |
| navigation | next, previous, active match index, total match count |
| viewport behavior | navigating to a non-visible match scrolls it into view with a small row buffer |
| highlights | all visible matches highlighted, active match highlighted differently |
| close behavior | Escape closes the find bar and clears highlights |
| terminal input | find bar consumes text/edit/navigation keys while open |

Deferred:

- regex and whole-word options.
- multi-line matches.
- command-block scoped search.
- search result list panel.
- persistent search history.
- streaming index for very large scrollback.
- remote text indexing outside the terminal process.

## Data Model

Add terminal text coordinates that do not overload visible `CellPoint` rows:

```rust
pub enum SearchRowKind {
    Scrollback,
    Screen,
}

pub struct SearchRowId {
    pub kind: SearchRowKind,
    pub index: usize,
}

pub struct SearchTextRow {
    pub id: SearchRowId,
    pub visible_row: Option<u16>,
    pub text: String,
    pub columns: Vec<SearchTextColumn>,
}

pub struct SearchTextColumn {
    pub start_col: u16,
    pub end_col: u16,
}

pub struct SearchMatch {
    pub row: SearchRowId,
    pub start_col: u16,
    pub end_col: u16,
}
```

Rules:

- `SearchTextRow::text` is trimmed the same way selection extraction trims
  right-side terminal padding.
- `visible_row` is `Some(row)` only when the row is currently visible in the
  active viewport.
- `SearchMatch` uses inclusive cell columns, matching `CellRange` and renderer
  overlay conventions.
- `SearchTextColumn` is empty for simple one-scalar/one-cell rows and present
  when the searchable text needs explicit character-to-cell spans for wide
  cells or combining marks.
- A row id must remain stable across a single search pass. It does not need to
  survive new PTY output, resize, or scrollback truncation.

Initial matching should be row-local. This avoids surprising matches across
prompt/output line boundaries and keeps highlight geometry simple.

## Core Boundary

Add read-only APIs to `BasicTerminal`:

```rust
impl BasicTerminal {
    pub fn search_text_rows(&self) -> Vec<SearchTextRow>;
    pub fn visible_search_matches(&self, matches: &[SearchMatch]) -> Vec<CellRange>;
    pub fn scroll_to_search_match(&mut self, row: SearchRowId, buffer_rows: u16) -> bool;
}
```

The exact names can change during implementation, but the boundary should hold:

- `witty-core` owns conversion from screen/scrollback buffers to searchable rows.
- `witty-core` owns mapping a history match back into a visible viewport.
- `witty-ui` owns query state, option state, active match selection, and commands.
- `witty-render-wgpu` owns drawing visible highlight rectangles.
- native/browser shells own shortcut routing and visual find-bar input.

Avoid exposing raw `BasicCell` or the private scrollback vector. Search is a
good reason to define a deliberate terminal-read API.

## Search State

Add a small UI-level state machine, likely in `witty-ui`:

```rust
pub struct TerminalSearch {
    open: bool,
    query: String,
    options: SearchOptions,
    matches: Vec<SearchMatch>,
    active: Option<usize>,
}

pub struct SearchOptions {
    pub case_sensitive: bool,
    pub regex: bool,
    pub whole_word: bool,
}
```

Initial behavior:

- Opening search seeds the query from selected text if available, otherwise an
  empty query.
- Query changes rebuild matches synchronously for the current scrollback size.
- Empty query means no matches and no highlights.
- `next()` wraps from last to first; `previous()` wraps from first to last.
- New terminal output invalidates matches and triggers a rebuild when the find
  bar is open.
- Scrollback truncation can invalidate active row ids, so rebuild rather than
  attempting to preserve exact match identity.

For MVP scrollback sizes, a synchronous linear scan is acceptable. Introduce an
incremental index only after there is measured latency.

## Renderer Overlay

Extend `RenderSnapshot` or `FramePlan` with visible search ranges:

```rust
pub struct SearchHighlight {
    pub range: CellRange,
    pub active: bool,
}
```

Preferred placement:

- Keep terminal content rows unchanged.
- Compose search highlights as dynamic overlays in the retained frame planner.
- Draw regular search matches above cell backgrounds and below glyph text.
- Draw active match with a stronger color, still below glyph text.
- Keep selection visually dominant over regular matches; active match can use a
  distinct outline/underline later if needed.

Frame diagnostics should gain:

- `search_match_rects`
- `search_active_visible`

Changing the query should not force a full row rebuild. It should only change
overlay damage unless the viewport scrolls to a new match.

## Native UI

Native window mode should add:

| Input | Behavior |
| --- | --- |
| `Ctrl+Shift+F` | open search bar |
| printable text | append to query |
| Backspace | edit query |
| Enter | next match |
| Shift+Enter | previous match |
| Escape | close search |
| Ctrl+Shift+C/V | keep clipboard behavior only when search is closed, or explicitly decide search-text copy/paste later |

Find bar rendering can start as a bottom overlay:

```text
Find: query_text                         3/17
```

Use the existing overlay text path from command palette and diagnostics. The
bar must not write bytes to the PTY while open.

## Browser UI

Browser mode should mirror native behavior:

- Intercept `Ctrl+Shift+F` in `app.js` before `session.handle_key()`.
- Add wasm methods for open, close, query input, backspace, next, previous, and
  a compact `search_status_text()`.
- Re-render after every search state change.
- Keep browser clipboard shortcuts unchanged while search is closed.

The Playwright smoke can assert:

- open search prevents terminal input.
- typing a query highlights and reports the expected match count.
- Enter navigates and scrolls to a later scrollback match.
- Escape clears highlights.

## Plugin Boundary

Search should be command-addressable but not silently content-readable.

Initial plugin-safe commands:

- `witty.search.open`
- `witty.search.close`
- `witty.search.next`
- `witty.search.previous`

Future read APIs must require `TerminalReadPermission`:

- current query.
- match count.
- selected match text.
- export matches.

Do not emit terminal contents in plugin events without an explicit read
permission. Search is a likely integration point for AI plugins, so the
permission boundary should be visible from the start.

## Implementation Milestones

### m115 Terminal Search Core

Status: implemented.

Write scope:

- `crates/witty-core/src/lib.rs`
- `crates/witty-core/src/basic_terminal.rs`
- focused tests
- this plan status update

Deliverables:

- `SearchTextRow`, `SearchRowId`, `SearchMatch`, and `SearchOptions` or a
  minimal equivalent in `witty-core`.
- Read-only searchable row extraction across scrollback and current screen.
- Literal row-local match helper with case-sensitive and insensitive modes.
- Mapping visible matches to `CellRange`.
- Scroll-to-match helper that moves `viewport_offset` when the match is in
  scrollback.

Acceptance:

- unit tests cover visible screen matches, scrollback matches, case handling,
  empty query, alternate screen behavior, and viewport scrolling.
- `cargo fmt --all -- --check`
- `cargo test -p witty-core`
- `cargo test --workspace`

Implementation note: `witty-core` now exposes `SearchRowId`,
`SearchTextRow`, `SearchMatch`, `SearchOptions`, and
`find_search_matches()`. `BasicTerminal` can extract searchable rows across
main-screen scrollback plus screen rows, search literal row-local matches with
case sensitivity control, map visible matches to `CellRange`, and scroll a
history match into the viewport. Alternate screen search is scoped to the
active alternate buffer. Native workspace tests, clippy, and `witty-core`
wasm32 check passed.

### m116 Terminal Search UI State

Status: implemented.

Write scope:

- `crates/witty-ui/src/search.rs`
- `crates/witty-ui/src/lib.rs`
- focused tests

Deliverables:

- `TerminalSearch` open/query/options/matches/active state machine.
- open seeded from selected text when present.
- next/previous wrapping behavior.
- rebuild-on-output integration points for `TerminalApp`.

Acceptance:

- unit tests cover query editing, match rebuild, active navigation, wrapping,
  and selected-text seed.
- workspace tests pass.

Implementation note: `witty-ui` now exports `TerminalSearch`, a pure state
machine for open/close, selected-text query seed, printable query edits,
backspace, case-sensitive option rebuilds, match storage, active index, and
wrapping `next_match()`/`previous_match()` navigation. It consumes
`SearchTextRow` input from `witty-core`, so native and browser shells can wire it
without duplicating search logic. Focused `witty-ui` tests, workspace tests,
wasm32 `witty-ui` check, and workspace clippy passed.

### m117 Search Highlight Frame Overlay

Status: implemented.

Write scope:

- `crates/witty-core`
- `crates/witty-render-wgpu`
- focused renderer tests

Deliverables:

- visible search highlight ranges in `RenderSnapshot` or `FramePlan`.
- regular and active highlight rects.
- frame stats for search overlays.
- no terminal-row cache rebuild for query-only overlay changes.

Acceptance:

- renderer tests prove highlight rect geometry and active match styling.
- retained planner tests prove search overlay changes do not rebuild cached
  terminal rows.

Implementation note: `witty-core` now exposes `SearchHighlight` and
`BasicTerminal::visible_search_highlights()` so UI search matches can be mapped
into visible `CellRange` overlays with an active-match flag. `RenderSnapshot`
now carries `search_highlights`; `FramePlanner` renders regular and active
search highlight rectangles above terminal backgrounds and below glyph text,
and `FrameStats` reports search overlay rect count plus active-match
visibility. `RetainedFramePlanner` composes these highlights as dynamic
overlays, so query-only highlight changes with empty row damage reuse terminal
row caches. Focused core/renderer tests, wasm checks, workspace tests, and
workspace clippy passed.

### m118 Native Find Bar

Status: implemented.

Write scope:

- `crates/witty-app/src/window.rs`
- optional `witty-ui` helpers
- docs update

Deliverables:

- `Ctrl+Shift+F` opens find bar.
- text/backspace/Enter/Shift+Enter/Escape behavior.
- native overlay drawing with query and count.
- active match navigation scrolls into view.

Acceptance:

- focused key-routing tests.
- deterministic smoke, preferably CLI-level first and GUI screenshot later.
- workspace tests and clippy pass.

Implementation note: native window mode now owns a `TerminalSearch` state
machine and routes `Ctrl+Shift+F` to a bottom find bar. While the find bar is
open it consumes printable text, Backspace, Enter, Shift+Enter, and Escape
without sending those keys to the PTY; opening search and the command palette
are mutually exclusive. Query changes rebuild search matches, visible matches
are mapped into `RenderSnapshot::search_highlights`, active navigation scrolls
history matches into view with a small bottom-bar buffer, and terminal output
or resize refreshes open search results. The overlay draws `Find: <query>` and
the active/total count while removing covered terminal glyphs, selection,
cursor, and search highlights from the bar row. Focused key-routing and overlay
tests, a deterministic `--native-search-smoke`, native workspace tests, and
workspace clippy passed. A direct `witty-app` wasm check remains a known
native-app build-surface issue because `witty-launcher -> getrandom` needs wasm
backend configuration; browser search should continue through `witty-web`.

### m119 Browser Search Bridge

Status: implemented.

Write scope:

- `crates/witty-web/src/lib.rs`
- `crates/witty-web/static/app.js`
- `scripts/run-witty-web-smoke.mjs`

Deliverables:

- browser `Ctrl+Shift+F` shortcut.
- wasm search methods and status diagnostics.
- Playwright smoke for open, query, highlight count, navigation, and close.

Acceptance:

- `cargo check -p witty-web --target wasm32-unknown-unknown`
- node browser smoke.
- Rust PTY and launcher browser smoke regression paths.

Implementation note: browser wasm sessions now own `TerminalSearch` alongside
the terminal and project visible search highlights into render snapshots before
each frame. The wasm boundary exposes open/close, query input, Backspace,
next/previous navigation, status/count diagnostics, visible-highlight count,
active-visible state, and a small `clear_selection()` helper for deterministic
browser smoke setup. `app.js` intercepts `Ctrl+Shift+F`, routes search text,
Enter, Shift+Enter, Backspace, and Escape to wasm while search is open, and
keeps those keys from becoming terminal input. The Playwright node-gateway
smoke verifies shortcut open, typed query, visible highlights, next navigation,
search consuming `Ctrl+Shift+C` while open, Escape close, and no outbound
terminal input. Rust PTY and launcher browser smoke paths remain product
regressions and skip the direct search interaction to avoid racing the async
WebSocket message handler against synchronous wasm session calls. Focused
`witty-web` tests, `witty-web` wasm32 check, node/rust/launcher browser smokes,
workspace tests, and workspace clippy passed.

### m120 Regex And Product Polish

Status: implemented.

Write scope:

- `witty-core`
- native/browser UI toggles
- docs and smokes

Deliverables:

- regex option, using `regex` or `regex-automata` after measuring complexity.
- whole-word option if needed.
- case toggle UI.
- match count UX for zero results and invalid regex.

Acceptance:

- invalid regex is shown in the find bar without panicking.
- regex and literal paths have focused tests.

Implementation note: `witty-core` now uses the Rust `regex` crate behind a
shared matcher boundary and preserves the compatibility `find_search_matches()`
API while adding `try_find_search_matches()` for UI error handling. Search
options now include case-sensitive, regex, and whole-word modes; whole-word
uses the same terminal-oriented word character policy as word selection.
`TerminalSearch` stores invalid-regex errors, clears them when the query or mode
becomes valid, and keeps matches empty while an invalid pattern is active.
Native and browser search bars show option state as `[aa|Aa lit|.* part|word]`,
report `No results` for zero matches, and surface invalid regex text without
panicking. Native uses `Alt+C`, `Alt+R`, and `Alt+W` while the find bar is open;
browser JS mirrors those shortcuts and the Playwright node-gateway smoke
verifies option toggles, invalid-regex status, and no outbound terminal input.

### m121 Search Plugin Command Boundary

Status: implemented.

Write scope:

- `witty-ui`
- `witty-app`
- docs and focused tests

Deliverables:

- Register privacy-safe command IDs:
  - `witty.search.open`
  - `witty.search.close`
  - `witty.search.next`
  - `witty.search.previous`
- Keep command execution local to the native window/search layer.
- Do not expose terminal content, query text, match counts, active match text,
  or selected text through plugin events or command arguments.

Acceptance:

- search commands appear as built-in command registrations.
- invoking search commands from the native command path updates search state.
- external plugins are not needed to implement these commands and do not receive
  search content.

Implementation note: `witty-ui` now exports shared search command IDs and
`search_command_registrations()` so shells can expose command metadata without
duplicating strings. Native window startup registers those commands as built-in
commands before external Wasm plugins are loaded, preventing plugin command-id
squatting. `TerminalWindowApp::invoke_window_command()` intercepts these command
IDs before calling `TerminalApp::invoke_command()`, so search open/close/next/
previous are handled as local UI state transitions and are not broadcast as
`PluginEvent::CommandInvoked` to installed plugins. The commands carry no args,
do not expose query or match data, and leave future content-reading APIs behind
the existing `TerminalReadPermission` boundary. Focused tests cover built-in
registration metadata, command shortcut behavior, and local state changes.

### m122 Search History And Repeat Polish

Status: implemented.

Write scope:

- `witty-ui`
- `witty-app`
- `witty-web`
- docs and focused tests

Deliverables:

- keep a bounded local search-query history in UI state.
- allow native and browser find bars to browse local history with Up/Down.
- let repeat next/previous reuse the most recent local query after the find bar
  has been closed.
- keep history outside plugin events, command args, and browser diagnostics.

Acceptance:

- closing a valid search commits it to bounded local history.
- invalid regex queries are not committed.
- native and browser history navigation is covered by focused tests.
- search next/previous commands can reopen the last local query without
  exposing query/history to plugins.

Implementation note: `TerminalSearch` now owns a bounded, deduplicated
search-query history with draft restoration while browsing. `close()` and
explicit next/previous navigation commit the current valid non-empty query, but
history remains private to the UI state object and is not included in plugin
command arguments, plugin events, or browser status diagnostics. Native find
bar handling maps Up/Down to local history browsing, and browser JavaScript
mirrors those keys through wasm methods that return only the normal find status
string. The built-in search next/previous command path now performs
repeat-find: if the find bar is closed, it reopens the last local query and
selects the first or last match depending on direction.

### m123 Search Unicode Wide Cell Correctness

Status: implemented.

Write scope:

- `witty-core`
- search tests
- docs and supervisor records

Deliverables:

- add focused Unicode, combining-mark, and wide-cell search fixtures.
- map search match character ranges back to terminal cell columns instead of
  assuming one Unicode scalar equals one terminal cell.
- document remaining Unicode limitations.

Acceptance:

- wide character search highlights the full terminal cell span.
- combining-mark sequences search as text while mapping back to their base
  cell span.
- regex and literal match paths use the same character-to-cell mapping.
- workspace tests, wasm check, browser smokes, and clippy pass.

Implementation note: `witty-core` now carries optional per-character terminal
cell spans on `SearchTextRow` through `SearchTextColumn`. Literal and regex
matchers return character ranges, then `SearchTextRow` maps those ranges back
to inclusive terminal cell columns. `BasicTerminal` now stores a cell width for
each printable cell, skips wide-cell continuation cells in render/search rows,
attaches zero-width combining marks to the previous printable cell, and uses
`unicode-width` for printable width decisions. Simple ASCII rows still omit the
column map for compact snapshots and compatibility.

Focused tests cover explicit search-column mapping, `RenderSnapshot` wide-cell
construction, `BasicTerminal` wide-cell rendering/search, and combining-mark
search mapping. Later milestones closed the main remaining Unicode gaps:
m124 hardened wide-cell editing/selection invariants, m126 added grapheme
cluster spans, and m127-m129 added opt-in literal NFC matching and UI exposure.

### m124 Terminal Wide Cell Selection Editing Correctness

Status: implemented.

Write scope:

- `witty-core`
- focused selection, word picking, and editing tests
- docs and supervisor records

Deliverables:

- selection should copy a wide cell exactly once if either half is selected.
- word picking should map a continuation cell back to its base cell before
  expanding the word range.
- erase, insert, delete, overwrite, and resize should not leave orphan
  continuation cells or split a wide cell.

Acceptance:

- focused `witty-core` tests cover selection, word picking, overwrite,
  insert/delete/erase, combining marks, and resize truncation.
- workspace tests, wasm check, browser smokes, and clippy pass.

Implementation note: `BasicTerminal` now has internal helpers for visible-cell
spans, continuation-to-base lookup, edit-range expansion, and row repair after
mechanical cell shifts. Selection uses span intersection so selecting either
half of a wide cell copies the character once. Word picking starts from the
base cell when the point lands on a continuation cell, and combining marks stay
with their base word cell. Character editing controls expand partial ranges to
full visible cells, repair orphan continuations after shifts, and drop a wide
cell if resize truncates its continuation.

### m125 Terminal Grapheme Normalization Plan

Status: implemented.

Write scope:

- docs and focused verification

Deliverables:

- evaluate `unicode-segmentation` and `unicode-normalization`.
- define the boundary between original terminal text, grapheme geometry, and
  optional normalized search projections.
- decide how normalized search maps back to original cell spans.

Implementation note: the plan selected `unicode-segmentation` for grapheme
cluster spans and `unicode-normalization` for an explicit literal-search NFC
projection. It kept original PTY text as the canonical buffer form, kept regex
on original text, and required normalized matches to map back through original
clusters before becoming terminal cell spans.

### m126 Terminal Grapheme Cluster Spans

Status: implemented.

Write scope:

- `witty-core`
- search and selection tests
- docs

Deliverables:

- expand search match geometry to whole extended grapheme clusters.
- expand selected text extraction so partial cluster selections copy the full
  original cluster once.
- cover emoji ZWJ, regional indicator, emoji modifier, wide-cell, and
  combining-mark regression cases.

Implementation note: `SearchTextRow` processing now builds internal grapheme
cluster spans with `unicode-segmentation`, expands match character ranges to
whole clusters, then maps those clusters through existing terminal cell-span
logic. Terminal buffers still preserve original PTY text and `unicode-width`
remains the cell-width source.

### m127 Literal NFC Search Option

Status: implemented.

Write scope:

- `witty-core`
- focused search tests
- docs

Deliverables:

- add `SearchOptions::normalize_nfc`, defaulting to `false`.
- add opt-in literal-search NFC projection.
- map normalized match offsets back to original grapheme clusters and terminal
  cell spans.
- keep regex search on original text.

Implementation note: literal search can now match canonically equivalent forms
such as `e\u{0301}` and `\u{00e9}` when `normalize_nfc` is enabled. Stored
terminal text, selected text, regex input, and plugin-visible content stay in
their original PTY form.

### m128 Unicode Word Boundary Evaluation

Status: implemented.

Write scope:

- docs and focused verification

Deliverables:

- compare the existing terminal token word policy with UAX #29 word
  boundaries.
- decide whether Unicode natural-word behavior should replace whole-word
  search or double-click selection.

Implementation note: terminal token semantics remain the default. They model
paths, flags, URLs, environment assignments, and shell symbols better than UAX
#29 natural words. A separate `UnicodeWord` search mode remains a possible
future option, but it should not replace the default behavior.

### m129 Search Normalize NFC UI Toggle

Status: implemented.

Write scope:

- `witty-ui`
- `witty-app`
- `witty-web`
- browser smoke script
- docs

Deliverables:

- expose `SearchOptions::normalize_nfc` in native and browser search UI.
- keep the option default-off.
- show normalization state in search status labels.
- preserve regex original-text behavior and plugin privacy boundaries.

Implementation note: native and browser find UI now use `Alt+N` to toggle
literal NFC matching. Search status labels include `raw` or `nfc`; browser
state captures `normalizeNfc`; the node-gateway browser smoke verifies the
shortcut, status label, and absence of terminal input. The toggle is local UI
state and does not add plugin events, command arguments, or normalized text
exports.

## Risks

| Risk | Mitigation |
| --- | --- |
| scrollback row identity changes on truncation | rebuild matches after output/truncation rather than preserving stale ids |
| Unicode and wide cells | width-aware mapping, wide-cell repair, grapheme spans, and opt-in NFC literal matching are implemented; keep regex-normalization and UAX #29 word mode deferred until there is a concrete product need |
| query changes causing full re-render | keep highlights as dynamic overlays and retain terminal row caches |
| browser shortcut conflicts | intercept only `Ctrl+Shift+F`; keep plain `Ctrl+F` available for future platform-specific policy |
| plugin privacy | keep content reads behind `TerminalReadPermission` |

## Selected Next Task

The search line is complete enough for the current Witty line: literal, regex,
whole-word, history, repeat-find, grapheme-aware geometry, and opt-in NFC
matching are implemented across native and browser UI.

The next implementation line should move to another high-value modern terminal
feature:

`m131-terminal-hyperlink-plan`: plan OSC 8 hyperlink parsing, terminal snapshot
storage, renderer styling, hover/click behavior, browser/native opening policy,
and plugin/privacy boundaries before implementing hyperlink rendering.

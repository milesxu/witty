# Terminal Command Block Gutter Overlay

Updated: 2026-05-31

`m196-command-block-gutter-overlay` adds the first always-visible rendered
command-block gutter layer.
`m197-command-block-gutter-hit-test` adds shared gutter hit testing and browser
diagnostics for future hover/click actions.
`m198-command-block-gutter-hover` adds native/browser hover state and a shared
hover overlay over the same row-anchor-mapped gutter boundary.
`m199-command-block-gutter-click-select` connects left-click selection to the
same native/browser gutter hit-test boundary.
`m200-command-block-text-ranges` exposes selected-block command/output typed
cell ranges as the next boundary for copy/action features.
`m201-anchored-text-range-extraction` adds the terminal-core extraction API
behind those ranges and exposes browser selected-block command/output text
diagnostics.
`m202-command-block-copy-actions` wires those extracted ranges into native and
browser command-palette copy actions for the selected block.
`m203-command-block-action-menu` adds the local selected-block action menu.
`m204-command-block-status-labels` adds selected/hovered exit-status labels.
`m205-command-block-timing-metadata` adds native/browser timing metadata to the
same command-block diagnostics.
`m206-command-block-duration-labels` renders available duration metadata in the
same selected/hovered status label.
`m207-command-block-fold-state` adds local fold/unfold state and menu/palette
commands without changing row layout yet.
`m208-command-block-folded-status-label` makes that folded state visible in the
existing selected/hovered status label.
`m209-command-block-folded-hidden-spans` adds shared folded hidden-row span
planning and browser diagnostics for the future collapsed layout pass.
`m210-command-block-folded-row-mask` consumes those spans in native/browser
frame composition to visually hide folded non-summary rows without changing
terminal coordinates.
`m211-command-block-folded-summary-chrome` keeps folded block gutter, hover,
selection, and gutter hit testing on the folded summary row.
`m212-command-block-folded-compact-row-map` adds compact visual-row mapping
diagnostics for the later row-height compaction pass.
`m213-command-block-folded-frame-remap` adds the reusable frame-plan compaction
primitive, and `m214-command-block-folded-coordinate-remap` adds the inverse
visual-to-terminal coordinate mapper.
`m215-command-block-folded-product-wiring` enables those helpers in native and
browser product rendering, hit testing, mouse reporting, local selection, and
IME cursor placement.
`m216-command-block-folded-compact-product-smoke` adds native/browser
product-level smoke coverage for folded compact layout, including frame
compaction diagnostics and gutter click selection after hidden rows are
collapsed.
`m217-command-block-folded-summary-count-label` adds hidden-row counts to the
folded summary status label, so a selected or hovered folded block can show
labels such as `folded 2 rows ok 1.3s`.

## Scope

The prior command-block UI only drew a gutter bar while a block was selected.
This task adds a shared `witty-ui` overlay helper that renders thin status bars
for completed OSC 133 command blocks that are visible in the current viewport.

Native and browser frames now apply command-block overlays in this order:

1. base terminal frame.
2. unselected completed-block gutter bars, using summary-row chrome spans for
   folded blocks.
3. hovered completed-block row highlight and stronger gutter bar, using
   summary-row chrome spans for folded blocks.
4. selected-block row highlight and selected gutter bar, using summary-row
   chrome spans for folded blocks.
5. status labels, selected-block action menu, and terminal-owned IME preedit.
6. folded compact frame remap.
7. screen-fixed overlays such as search, command palette, and diagnostics.

## Layout

The gutter overlay is intentionally conservative:

- one narrow rectangle per visible row in each completed command block.
- stable viewport placement via the row anchors introduced in `m195`.
- selected blocks are skipped by the generic gutter helper because the selected
  overlay already draws a stronger gutter.
- rows outside the visible grid are clipped.

Status colors are derived from the command exit code:

- exit code `0`: success gutter.
- non-zero exit code: failure gutter.
- missing exit code: neutral gutter.

The gutter lives in `FramePlan::backgrounds` so it shares the existing wgpu
rectangle batch and works in native and browser render paths without a new
renderer channel.

## Hover Overlay

`witty-ui` now exposes
`apply_command_block_gutter_hover_overlay_with_anchors()` for native and browser
render paths. The helper:

- accepts the current hovered command-block id from the host shell.
- maps the hovered block through the same visible row anchors as the gutter and
  selected-block overlays.
- draws a subtle full-row background plus a wider gutter for every visible row
  in the hovered block.
- suppresses hover rendering for the selected block, because the selected-block
  overlay remains the stronger visual state.

Native window mode updates `hovered_command_block_id` during cursor movement by
calling the shared gutter hit-test helper. Browser wasm exposes
`update_command_block_gutter_hover(offsetX, offsetY)`, and the product
JavaScript bridge calls it from pointer events. Browser JavaScript also uses
`command_block_gutter_hit_json(offsetX, offsetY)` to set a pointer cursor while
the mouse is inside the command-block gutter hit area.

## Click Selection

Native and browser host shells now consume a left click inside the command-block
gutter hit area and select that block through
`ShellIntegrationState::select_completed_block(id)`.

Native window mode handles this before local selection, primary paste, and
terminal mouse reporting, so the gutter behaves as product chrome instead of
terminal cell content. Hyperlink modifier-click activation still gets first
chance because it requires an explicit modifier gesture.

Browser wasm exposes `select_command_block_gutter_hit_json(offsetX, offsetY)`.
The JavaScript pointer bridge calls it from `pointerdown`, prevents the default
event when a block is selected, and keeps the selection diagnostics in
`window.wittyCommandBlocks()`.

## Text Ranges

`TerminalCommandBlock::text_ranges()` splits a completed command block into:

- `command`: start at `CommandStart` when present, otherwise prompt start; end
  at `OutputStart` when present, otherwise command finish.
- `output`: start at `OutputStart` and end at command finish; omitted when the
  shell did not emit an output-start marker.

The ranges use end-exclusive `CellPoint`s and preserve optional start/end
anchors. They intentionally describe text boundaries without reading clipboard
or sending plugin events.

Browser wasm exposes `selected_command_block_text_ranges_json()`, and
`window.wittyCommandBlocks()` includes `selectedTextRanges` for smoke tests
and future JavaScript action surfaces.

## Text Extraction

`witty-core` now exposes `TerminalTextRange` and
`BasicTerminal::text_for_range(range)`. The API accepts a target screen, start
and end-exclusive cell points, and optional start/end row anchors.

When anchors are present, extraction resolves rows against main scrollback,
the current main screen, or the alternate screen by stable row anchor. If the
anchored row has already been trimmed from scrollback, the method returns
`None` instead of falling back to stale screen coordinates. Without anchors,
the method extracts from the active visible viewport.

`witty-ui` converts selected command-block command/output ranges into
`TerminalTextRange`, and browser wasm exposes
`selected_command_block_text_json()`. `window.wittyCommandBlocks()` includes
`selectedText`, so smoke coverage now verifies both the typed range coordinates
and the extracted selected-block command/output text.

## Copy Actions

`witty-ui` now registers two additional built-in command-block commands:

- `witty.command_block.copy_command`
- `witty.command_block.copy_output`

Both commands are local UI commands. Native window mode handles them before
plugin dispatch and writes through the existing clipboard sink. Browser mode
handles the command locally in Rust, exposes the selected-block copy text to
the JavaScript command-palette flow, and then writes through the browser
Clipboard API. The copied output text removes one leading shell newline and
trailing newlines so a block whose OSC 133 output marker lands before the shell
line break copies as `ok` rather than `\nok`.

The plugin privacy boundary is unchanged: command-block copy commands do not
send terminal output, selected text, command text, or clipboard contents through
plugin command arguments or plugin events.

## Action Menu

`m203-command-block-action-menu` adds a local selected-block action menu that
reuses the existing command-block commands:

- `witty.command_block.actions`
- `witty.command_block.copy_output`
- `witty.command_block.copy_command`
- `witty.command_block.clear`
- `witty.command_block.toggle_fold`

The command palette can open the menu with `Command Block: Actions` for the
currently selected block. Native and browser gutter right-clicks also select the
hit block and open the same menu. The first pass is deliberately local and
small: it renders `Copy Output`, `Copy Command`, `Clear Selection`, and
`Toggle Fold`, supports up/down/enter/escape keyboard handling, and dispatches
those existing commands before plugin dispatch.

Browser copy still goes through the JavaScript Clipboard API from the explicit
menu or command-palette gesture. The menu state exposes only command ids,
titles, indexes, and open/selected state; it does not expose terminal command
or output text to plugin command arguments, plugin events, or menu diagnostics.

## Fold State

`m207-command-block-fold-state` adds a boolean `folded` field to completed
command blocks. It defaults to `false` and is skipped in JSON unless a block is
actually folded.

Native and browser command handling now recognizes
`witty.command_block.toggle_fold` as a local command for the selected
completed block on the active screen. The same command appears in the command
palette as `Command Block: Toggle Fold` and in the selected-block action menu
as `Toggle Fold`. It is handled before plugin dispatch, so folded state changes
do not expose terminal command or output text to plugins.

This is a state boundary only. Folded blocks still render with their existing
rows; hiding output, preserving scrollback geometry, and drawing a collapsed
summary row are deferred to a later layout task.

`m209-command-block-folded-hidden-spans` adds the next data boundary:
`TerminalCommandBlockFoldedHiddenRowSpan`. For every folded visible block that
spans more than one visible row, it records:

- `summary_row`: the first visible row to keep.
- `hidden_start_row` and `hidden_end_row`: the visible row range a future
  collapsed renderer can hide.
- block id, screen, and optional exit code.

Browser wasm exposes this through
`folded_command_block_hidden_row_spans_json()`, and product diagnostics include
`foldedHiddenRowSpans`.

`m210-command-block-folded-row-mask` consumes those spans as a deliberately
lightweight visual pass. For each hidden visible row, it removes terminal glyphs
whose origins fall inside that row and paints a full-row background mask. The
summary row is kept. The mask runs before gutter, hover, selection, status
labels, action menus, search, command palette, and IME overlays, so local
command-block chrome can still render above folded rows.

`m211-command-block-folded-summary-chrome` adds the matching chrome boundary:
for unfolded blocks, gutter/hover/selection/hit-test geometry still uses the
full visible block span; for folded multi-row blocks, that geometry is reduced
to `summary_row..summary_row`. Folded hidden rows therefore remain blank after
the mask and no longer show multi-row gutter/hover/selection feedback or accept
gutter clicks.

`m212-command-block-folded-compact-row-map` adds the next layout-planning
boundary. `ShellIntegrationState::folded_compact_visual_rows()` returns one
entry per visible row:

- `visible_row`: the current terminal viewport row.
- `hidden`: whether that row is hidden by a folded block.
- `hidden_rows_before`: how many folded hidden rows precede it.
- `compact_row`: the compact visual row for non-hidden rows.
- `hidden_by_block_id`: the folded block id for hidden rows.

Browser wasm exposes the same data through
`folded_command_block_compact_rows_json()`, and JavaScript diagnostics include
`foldedCompactRows`. At the `m212` boundary this still did not translate
frame-plan glyphs, rectangles, cursor, search highlights, or selection overlays.

`m213-command-block-folded-frame-remap` adds the first reusable translation
primitive. `apply_command_block_folded_frame_remap_with_anchors()` consumes the
compact visual-row map and mechanically edits a `FramePlan`: glyphs and
rectangles whose origins are on folded hidden rows are removed, while
backgrounds, glyphs, cursor rectangles, selection rectangles, search
highlights, hyperlink overlays, and IME preedit rectangles on later rows are
moved upward by the hidden-row count before them.

`m214-command-block-folded-coordinate-remap` adds that inverse primitive.
`command_block_folded_terminal_row_for_compact_visual_row_with_anchors()` maps a
compact visual row back to the original visible terminal row, and
`command_block_folded_visual_pixel_to_terminal_pixel_with_anchors()` maps a
pixel point from compact visual layout back to terminal-layout pixels while
preserving the within-row offset. This gives future native/browser pointer,
hyperlink, mouse-reporting, and IME cursor-area paths a shared coordinate
boundary.

`m215-command-block-folded-product-wiring` enables the compact frame remap in
both native and browser composition after command-block chrome and
terminal-owned IME preedit have been drawn. Search bars, command palettes, and
diagnostics remain screen-fixed and are applied after compaction. Pointer
paths now first map compact visual pixels back to terminal pixels before
hyperlink hover/activation, gutter hover/click, mouse reporting, and local
selection. Native and browser IME cursor-area placement maps the terminal cursor
cell forward into the compact visual row, so hidden inputs/candidate windows
track folded output correctly.

This is a visual collapsed layout, not a terminal-buffer rewrite. It does not
remove rows from scrollback, alter copy ranges, alter command-block JSON
diagnostics, or expose folded command/output text to plugins. Compact blank
rows below the remapped content do not resolve to terminal rows and therefore
do not hit-test as terminal content.

`m216-command-block-folded-compact-product-smoke` adds coverage at the product
boundary rather than only the helper boundary. The native
`--native-command-block-smoke` path now constructs two completed OSC 133 command
blocks, folds the first, verifies two hidden compact rows, confirms that the
second block's glyphs move to compact visual row 1, and selects the second
block through the remapped gutter hit path. The browser node-gateway smoke
drives the same scenario through wasm diagnostics and a synthetic `pointerdown`
event, checking that `foldedCompactRows`, `foldedHiddenRowSpans`, direct gutter
hit JSON, and product click selection all agree.

## Status Labels

`m204-command-block-status-labels` adds a small selected/hovered block status
label on the right edge of the block's first visible row. The label is derived
only from existing OSC 133 finish metadata:

- `ok` for exit code `0`.
- `exit N` for nonzero exit codes.
- `done` when no exit code was provided.

When timing metadata is available, `m206-command-block-duration-labels` appends
a compact duration to the same label, for example `ok 42ms`, `ok 1.3s`, or
`exit 2 12s`.

When `folded` is true, `m208-command-block-folded-status-label` prefixes the
same label with `folded`, and `m217-command-block-folded-summary-count-label`
includes the number of currently hidden visible rows when folded compaction has
something to hide, for example `folded 2 rows ok 1.3s`. This makes the compact
summary row communicate how much output is collapsed without changing terminal
buffer rows or command-block copy ranges.

The label overlay is shared by native and browser rendering and uses the same
row-anchor viewport mapping as gutter, hover, and selection overlays. In folded
compact layout it is remapped with the command-block frame chrome, so it stays
on the visible summary row. It is a visual-only overlay; it does not mutate
terminal cells, search rows, clipboard data, command arguments, plugin events,
or browser diagnostics containing terminal text.

## Timing Metadata

`m205-command-block-timing-metadata` stores optional session-relative timing on
completed command blocks:

- `started_at_ms`
- `finished_at_ms`
- `duration_ms`

The timing source stays in the host shell instead of `witty-core`. Native window
mode uses elapsed milliseconds from the native app start, while browser wasm
uses `window.performance().now()` relative to session creation. The OSC 133
state machine records command start when a `B` marker is observed, falls back
to the prompt or first observed marker for partial integrations, and computes
duration with saturating subtraction at command finish.

The metadata is exposed through existing native/browser command-block JSON and
diagnostics. No terminal command/output text is added to plugin arguments,
plugin events, clipboard state, or search state.

## Hit Testing

`witty-ui` exposes `command_block_gutter_hit_test_with_anchors()` as the shared
geometry boundary for native and browser UI code. The helper:

- accepts the current `ShellIntegrationState`, active screen, visible row
  anchors, pixel point, cell metrics, and grid size.
- treats the first terminal cell column as the gutter hit area, giving future
  click targets a practical width while the visual bar remains thin.
- maps command-block spans through row anchors before checking the visible row.
- returns block id, screen, visible row, visible span, selected state, and exit
  code.
- prefers the newest matching block if spans ever overlap.

Browser wasm exposes `command_block_gutter_hit_json(offsetX, offsetY)` for
diagnostics and JavaScript event routing. The method scales CSS offsets by
device pixel ratio, maps compact visual pixels back to terminal-layout pixels
when folded blocks have hidden rows, and then calls the shared helper. Native
uses the same remap before gutter hover/click, hyperlink activation, mouse
reporting, and local selection.

## Boundary

This is still a visual scaffold, not a full Warp-like block product surface.
It does not yet add hover toolbars, share actions, persisted command history,
or dedicated folded-layout visual polish beyond compact row remapping.

Good follow-up tasks:

- duration/status metadata display polish in richer block toolbars.
- folded-block compact layout visual polish beyond the current row compaction.

## Verification

Covered by:

- `cargo test -p witty-ui shell_integration --quiet`
- `cargo test -p witty-core text_for_range --quiet`
- `cargo test -p witty-core --quiet`
- `cargo test -p witty-app native_command_block --quiet`
- `cargo test -p witty-web command_block --quiet`
- `cargo test -p witty-web command_palette --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `cargo clippy -p witty-ui -p witty-app -p witty-web --all-targets -- -D warnings`
- `node --check crates/witty-web/static/app.js`
- `node --check scripts/run-witty-web-smoke.mjs`
- `cargo run -p witty-app -- --native-command-block-smoke`
- `WITTY_WEB_SMOKE_GATEWAY=node scripts/run-witty-web-smoke.sh`

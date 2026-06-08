# Terminal Shell Integration OSC 133

Updated: 2026-05-31

`m187-terminal-shell-integration-osc133-core` adds the first shell-integration
primitive for future Warp-like command blocks.
`m188-terminal-shell-integration-block-state` stores those events as UI-layer
command block state.
`m189-browser-shell-integration-smoke` exposes browser diagnostics and adds
Playwright coverage for block-state formation.
`m190-shell-integration-command-block-queries` adds query helpers and browser
diagnostics for active-screen blocks, visible row spans, and the last completed
block.
`m191-shell-integration-command-block-navigation` adds selected-block state and
screen-scoped latest/previous/next navigation primitives.
`m192-browser-command-block-selection-overlay` renders the selected visible
command block in the browser with a subtle row highlight and left gutter bar.
`m193-command-block-navigation-commands` registers command-block navigation
commands and wires them into the browser command palette.
`m194-native-command-block-command-overlay-parity` moves command handling and
the selected-block overlay to shared UI helpers and wires the same behavior into
the native window.
`m195-terminal-command-block-row-anchors` adds core row anchors and uses them to
map selected command blocks into the current scrollback viewport.
`m196-command-block-gutter-overlay` adds always-visible native/browser gutter
bars for unselected completed blocks, with success/failure/neutral status
colors.
`m197-command-block-gutter-hit-test` adds a shared gutter hit-test helper and a
browser diagnostic JSON method for future hover/click actions.
`m198-command-block-gutter-hover` adds native/browser command-block gutter hover
state and a shared row-anchor-mapped hover overlay.
`m199-command-block-gutter-click-select` lets native/browser left-clicks in the
gutter select the hit command block.
`m200-command-block-text-ranges` exposes selected-block command/output typed
cell ranges with optional anchors for future copy/action features.
`m201-anchored-text-range-extraction` adds anchored core text extraction for
those ranges and browser diagnostics for the extracted selected-block text.
`m202-command-block-copy-actions` adds local native/browser command-palette
actions for copying the selected command block's command or output text.
`m203-command-block-action-menu` adds a local native/browser selected-block
action menu that reuses copy output, copy command, and clear selection.
`m204-command-block-status-labels` adds native/browser selected and hovered
command-block status labels derived from OSC 133 exit-code metadata.
`m205-command-block-timing-metadata` records native/browser command-block timing
metadata from host marker observation times without adding clock logic to
`witty-core`.
`m206-command-block-duration-labels` renders that duration metadata in the
existing selected/hovered command-block status label overlay.
`m207-command-block-fold-state` adds command-block fold/unfold state plus local
native/browser commands as the data boundary for a later collapsed layout pass.
`m208-command-block-folded-status-label` shows folded state in the existing
selected/hovered status label overlay.
`m209-command-block-folded-hidden-spans` adds shared hidden-row span planning
and browser diagnostics for future collapsed layout rendering.
`m210-command-block-folded-row-mask` consumes those spans in native/browser
frame composition to visually mask folded hidden rows without changing terminal
coordinates.
`m211-command-block-folded-summary-chrome` constrains folded command-block
gutter, hover, selection, and gutter hit testing to the summary row.
`m212-command-block-folded-compact-row-map` adds shared compact visual-row
mapping and browser diagnostics for the later frame-remapping pass.
`m213-command-block-folded-frame-remap` and
`m214-command-block-folded-coordinate-remap` add the compact frame and inverse
coordinate primitives.
`m215-command-block-folded-product-wiring` enables compact folded command-block
rendering and native/browser interaction remapping in the product paths.
`m216-command-block-folded-compact-product-smoke` verifies that folded compact
layout works through native and browser product smoke paths, including
diagnostics and remapped gutter clicks.

## Scope

`BasicTerminal` now parses the OSC 133 marker family used by modern shell
integration scripts:

- `OSC 133 ; A ST`: prompt start.
- `OSC 133 ; B ST`: command line start, which also marks prompt end.
- `OSC 133 ; C ST`: command output start.
- `OSC 133 ; D [; exit-code] ST`: command finished.

BEL-terminated OSC sequences are accepted through the existing parser path.

The parser emits `TerminalHostAction::ShellIntegration` with:

- marker kind.
- active screen, `main` or `alternate`.
- current cursor cell at marker time.
- optional stable row anchor for that cursor cell.
- optional exit code for `D`.

The markers do not mutate terminal cells, renderer snapshots, clipboard state,
or PTY/gateway input. Unknown OSC 133 markers are ignored.

## Block State

`witty-ui` now owns a `ShellIntegrationState` state machine:

- `PromptStart` opens a pending command block.
- `CommandStart` records where the editable command text begins.
- `OutputStart` records where command output begins.
- `CommandFinished` completes the pending block with the final cursor point and
  optional exit code.

If a shell emits partial markers, the state machine creates a conservative
fallback block at the first marker point instead of failing. A new `PromptStart`
replaces any incomplete pending block.

Native and browser host-action drain paths now feed `ShellIntegration` events
into this state. Browser sessions expose completed block count and JSON methods
for future smoke tests and UI work.

Browser JavaScript exposes `window.wittyCommandBlocks()` for diagnostics and
updates the last block snapshot after each gateway output drain.

`witty-ui` also exposes block query helpers:

- completed-block count by screen.
- lookup by command-block id.
- last completed block.
- completed blocks for a screen.
- completed blocks and row-span summaries intersecting a row window.

Browser sessions expose the same diagnostics for the active screen:

- `active_screen()`.
- `completed_command_blocks_for_active_screen_json()`.
- `completed_command_block_count_for_active_screen()`.
- `visible_command_blocks_json()`.
- `visible_command_block_row_spans_json()`.
- `folded_command_block_hidden_row_spans_json()`.
- `last_completed_command_block_json()`.
- `selected_command_block_json()`.
- `select_latest_command_block_for_active_screen_json()`.
- `select_previous_command_block_for_active_screen_json()`.
- `select_next_command_block_for_active_screen_json()`.
- `toggle_selected_command_block_fold_json()`.
- `clear_selected_command_block()`.

Selected and completed block JSON includes `folded: true` only when a completed
block has been locally folded. Unfolded blocks omit the field, preserving the
existing compact diagnostics shape.

Folded hidden-row diagnostics report, for each visible folded block that spans
multiple rows, the first visible row to keep as the summary row and the visible
row range hidden by the lightweight row-mask pass. The current mask removes
terminal glyphs from those rows and paints row backgrounds in native/browser
frame composition. It does not compress row height, remap scrollback, remap
search rows, alter selection coordinates, or alter copy ranges.

Folded command-block chrome uses a separate visible chrome span. For unfolded
blocks, the chrome span matches the full visible block span. For folded blocks
that span multiple visible rows, the chrome span is reduced to the summary row.
Native/browser gutter bars, hover highlights, selected-block highlights, and
gutter hit testing use that chrome span, so folded hidden rows stay visually
quiet and are no longer clickable through the gutter.

`witty-ui` also exposes a folded compact visual-row map. For each visible row, it
reports whether the row would be hidden by a folded block, how many hidden rows
precede it, and which compact visual row a non-hidden row would occupy after
row-height compaction. Browser diagnostics expose this as
`folded_command_block_compact_rows_json()` and JavaScript `foldedCompactRows`.
This remains a diagnostics boundary; the rendered compact layout is applied by
the frame remap rather than by changing command-block JSON.

`witty-ui` now also has a frame-plan remap primitive for that compact map. It can
remove hidden-row frame primitives and move later terminal, search, hyperlink,
selection, cursor, and IME preedit primitives upward inside a `FramePlan`.
Native/browser rendering now calls it after command-block chrome and
terminal-owned IME preedit have been drawn, but before screen-fixed overlays
such as search, command palette, and diagnostics.

The matching coordinate mapper now exists in `witty-ui`: one helper maps compact
visual rows back to original visible terminal rows, and another maps compact
visual pixel points back to terminal pixel points while preserving the row-local
y offset. Native/browser pointer paths use it before hyperlink
hover/activation, gutter hover/click, mouse reporting, and local selection.
Native/browser IME cursor-area placement maps the terminal cursor forward into
the compact visual row so candidate windows and hidden inputs track folded
output.

Command blocks keep the original marker cursor rows for compatibility and also
store optional marker anchors. Anchor-aware row-span helpers intersect those
stored anchors with `BasicTerminal::visible_row_anchors()`, so a selected block
can still be highlighted after it has scrolled into history.

The older screen-coordinate row-span diagnostics remain as fallback helpers.
Browser and native rendering now use the anchor-aware overlay path.

Command-block navigation is stateful but still non-rendering. Previous
navigation starts from the latest completed block when nothing is selected;
next navigation starts from the first completed block. Navigation is scoped to
the active screen, and attempts to move past either end keep the current
selection.

Native and browser rendering now apply the same selected-command-block overlay
before glyphs are drawn. The overlay is clipped to the visible grid and uses:

- a translucent full-row background for every visible row in the selected block.
- a narrow left gutter bar on those same rows.

Native and browser rendering also apply a shared completed-block gutter overlay
before the selected-block overlay. It renders only unselected completed blocks,
maps block spans through the current visible row anchors, clips to the grid, and
uses the block exit code to choose success, failure, or neutral gutter colors.

The shared gutter hit-test helper treats the first terminal cell column as the
interactive gutter zone, maps the hit row through the same row-anchor block
spans, and returns block id, visible row/span, selected state, and exit code.
Browser wasm exposes this as `command_block_gutter_hit_json(offsetX, offsetY)`.

Native and browser rendering now apply a shared hover overlay between the
generic completed-block gutter and selected-block overlay. Hosts store the
currently hovered command-block id, use the shared gutter hit-test boundary to
update it from pointer movement, and redraw a subtle full-row background plus a
stronger gutter for the hovered unselected block. Browser wasm exposes
`update_command_block_gutter_hover(offsetX, offsetY)`, while JavaScript reuses
the diagnostic hit JSON to switch the gutter cursor to `pointer`.

Native and browser shells now also consume left-clicks in that gutter hit area
and select the matching block locally. Native handles the gutter click before
local selection or terminal mouse reporting, while browser wasm exposes
`select_command_block_gutter_hit_json(offsetX, offsetY)` and JavaScript calls it
from `pointerdown`.

Selected command blocks now expose typed command/output text ranges. The
command range starts at `CommandStart` when present and ends at `OutputStart`
or command finish; the output range starts at `OutputStart` and ends at command
finish. Both ranges use end-exclusive cell points and preserve optional row
anchors. Browser wasm exposes
`selected_command_block_text_ranges_json()`, and the browser smoke validates
the command/output range coordinates after gutter selection.

Those ranges can now be converted into `witty-core::TerminalTextRange`.
`BasicTerminal::text_for_range()` resolves anchored rows against scrollback,
main-screen rows, or alternate-screen rows and returns `None` if the anchor has
already been trimmed. Browser wasm exposes
`selected_command_block_text_json()`, and `window.wittyCommandBlocks()`
includes `selectedText` for diagnostics and smoke coverage. Clipboard copy and
block action UI now consume the same range/extraction boundary.

Native and browser builds now register local copy commands for selected command
blocks:

- `witty.command_block.copy_command`: copy the selected block's command
  range.
- `witty.command_block.copy_output`: copy the selected block's output range
  with one leading shell newline and trailing newlines removed for user-facing
  clipboard text.

Native writes through the existing clipboard sink. Browser Rust keeps the
command local and lets JavaScript perform the Clipboard API write from the
command-palette gesture. These commands are intentionally handled before plugin
dispatch, so terminal command/output text is not exposed in plugin command
arguments or events.

Selecting latest/previous/next or clearing the selected command block triggers
an immediate frame rebuild so the canvas or native window reflects the state
change.

The command palette now includes command-block navigation commands:

- `witty.command_block.actions`: open the selected block action menu.
- `witty.command_block.latest`: select the latest completed block.
- `witty.command_block.previous`: select the previous completed block.
- `witty.command_block.next`: select the next completed block.
- `witty.command_block.clear`: clear the selected block.
- `witty.command_block.copy_command`: copy the selected block command.
- `witty.command_block.copy_output`: copy the selected block output.
- `witty.command_block.toggle_fold`: toggle the selected block's folded
  state.

In both browser and native window builds, these commands are handled locally
before plugin dispatch and update the selected-block overlay immediately.

Native and browser builds also expose a selected command-block action menu.
The menu can be opened from `witty.command_block.actions` or by
right-clicking a command-block gutter hit. Right-click first selects the hit
block, then opens the menu. The menu renders `Copy Output`, `Copy Command`, and
`Clear Selection`, supports up/down/enter/escape keyboard interaction, and
dispatches those existing local commands before plugin dispatch. Browser
diagnostics expose only menu ids, titles, indexes, and selected state; command
text and output text are still only read by the local copy path and are not sent
to plugins.

`m207-command-block-fold-state` extends that local action surface with
`Toggle Fold`. The command toggles a boolean on the selected completed block,
is scoped to the active screen, is serialized in command-block JSON, and is
handled before plugin dispatch. The current native/browser product path now
uses that state to compact folded command blocks visually while keeping terminal
buffer rows, scrollback, selected-block text extraction, copy ranges, and JSON
diagnostics unchanged.

Selected and hovered command blocks now also render a small right-aligned status
label using the block's exit code: `ok`, `exit N`, or `done`. When timing
metadata is available, the same label appends a compact duration such as
`42ms`, `1.3s`, or `1m05s`. When a block is folded, the label is prefixed with
`folded`, and folded compact layout labels include the currently hidden visible
row count when available, for example `folded 2 rows ok 1.3s`. Native and
browser renderers share the same row-anchor-mapped overlay helper, so the label
follows the visible scrollback viewport just like the gutter, hover, and
selection overlays. The label is visual metadata only and does not add terminal
text to plugin command arguments, plugin events, search state, or clipboard
state.

Folded frame compaction runs after the base terminal frame, command-block
gutter, hover, selection, status-label, action-menu, and terminal-owned IME
preedit are planned. Hidden folded rows are removed from the frame, later frame
primitives move upward, and search/command-palette/diagnostics overlays remain
screen-fixed because they are applied after the remap. The remap is
frame-plan/UI geometry only and leaves terminal buffer, search indexing,
selection/copy extraction, plugin boundaries, and browser JSON diagnostics
unchanged.

Product smoke now covers that boundary end to end. Native
`--native-command-block-smoke` folds a multi-row block, checks the compact-row
diagnostics against the moved frame glyphs, and selects the following block via
the remapped gutter hit path. Browser smoke exposes
`window.wittyToggleSelectedCommandBlockFold()` and
`window.wittyCommandBlockGutterHit()` for diagnostics, then verifies that a
synthetic `pointerdown` at compact visual row 1 selects the second command
block after the first block's hidden rows have collapsed.

Command blocks now also keep optional timing metadata:

- `started_at_ms`: session-relative millisecond timestamp for command start.
- `finished_at_ms`: session-relative millisecond timestamp for command finish.
- `duration_ms`: saturating difference between finish and start.

`witty-ui::ShellIntegrationState::apply_event_at_ms()` accepts host-observed
marker times, while the existing `apply_event()` path remains available for
tests and sources that do not have a clock. Native window mode derives these
timestamps from its process-local `Instant`; browser wasm derives them from
`window.performance().now()` relative to session creation. Partial marker
streams fall back to the prompt or first observed marker time, so incomplete
OSC 133 integrations can still produce conservative timing diagnostics.

## Boundary

This is intentionally not a full rendered command-block product surface yet.
The current boundary is typed parser events, UI-layer command block state,
queryable diagnostics, durable row anchors, native/browser selected-block
highlighting, an always-visible gutter scaffold, shared gutter hit testing,
native/browser gutter hover and click-to-select feedback, typed selected-block
command/output text ranges, anchored selected-block text extraction, and local
selected-block copy command/output actions plus a local selected-block action
menu, selected/hovered exit-status labels, native/browser timing metadata, and
compact folded command-block rendering with inverse interaction remapping.
Persisted command history, richer hover toolbars, block-scoped share, folded
layout polish, and plugin exposure remain follow-up work.

## References

- Microsoft Terminal shell integration marks prompt, input, command execution,
  and command finish using OSC 133:
  <https://learn.microsoft.com/en-us/windows/terminal/tutorials/shell-integration>.
- VS Code terminal shell integration documents compatible finalterm-style OSC
  133 sequences:
  <https://code.visualstudio.com/docs/terminal/shell-integration>.

## Verification

Covered by:

- `cargo test -p witty-core osc133 --quiet`
- `cargo test -p witty-core text_for_range --quiet`
- `cargo test -p witty-core visible_row_anchors --quiet`
- `cargo test -p witty-ui shell_integration --quiet`
- `cargo test -p witty-app native_command_block --quiet`
- `cargo test -p witty-web command_block --quiet`
- `cargo test -p witty-web command_palette --quiet`
- `cargo test -p witty-core --quiet`
- `cargo test -p witty-ui --quiet`
- `cargo test -p witty-app --quiet`
- `cargo test -p witty-web --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `cargo clippy -p witty-core -p witty-ui -p witty-app -p witty-web --all-targets -- -D warnings`
- `cargo run -p witty-app -- --native-command-block-smoke`
- `node --check crates/witty-web/static/app.js`
- `node --check scripts/run-witty-web-smoke.mjs`
- `WITTY_WEB_SMOKE_GATEWAY=node scripts/run-witty-web-smoke.sh`

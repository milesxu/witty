# Terminal Compatibility Roadmap

Updated: 2026-06-02

m136 refreshes the implementation roadmap after completing terminal search and
OSC 8 hyperlink support. Witty now has enough native/browser UI surface to
shift the next work line back to terminal compatibility.

## Current Baseline

Witty now has:

- native `winit` + `wgpu` terminal window backed by `portable-pty`.
- native Linux/M1000 development is pinned to the `wgpu` OpenGL backend. Vulkan
  and local Playwright/Chromium WebGPU smokes are suspended on this machine
  after the 2026-06-01 driver-level hang; browser/WebGPU work is deferred to
  other platforms.
- browser WebGPU/wasm path served by the loopback product launcher and gateway,
  kept as existing code but no longer the local priority path.
- retained renderer planning with row damage reuse and dynamic overlays.
- selection, scrollback, primary selection, clipboard copy/paste, and bracketed
  paste.
- command palette, local plugin command boundary, and Wasm component plugin
  fixture support.
- SGR colors/styles, cursor visibility/shape, erase variants, OSC title, main
  and alternate screen buffers, scroll regions, cursor save/restore, line
  editing controls, reverse index, tab stops, origin/autowrap/insert modes.
- cursor positioning controls including CNL/CPL/CHA/HPA/VPA/HPR/VPR and
  IND/NEL.
- scroll up/down controls for `CSI Ps S` and `CSI Ps T` over the effective
  scroll region.
- repeat preceding graphic support for `CSI Ps b`.
- cursor tabulation controls for `CSI Ps I` and `CSI Ps Z`.
- DECALN screen alignment test for `ESC # 8`.
- common 8-bit C1 aliases for IND, NEL, HTS, and RI.
- ANSI cursor save/restore controls for `CSI s` and `CSI u`.
- DEC special graphics line drawing via G0/G1 designation and `SI`/`SO`.
- G2/G3 charset designation plus `SS2`/`SS3` one-shot DEC special graphics
  invocation.
- cursor save/restore preserves active charset and G0-G3 charset designations.
- protected-character state and DEC selective erase controls.
- cursor-key, keypad, function/navigation key, mouse, and focus-event reporting
  across native and browser paths.
- DECSTR soft reset for xterm-style parser cleanup without clearing visible
  buffers.
- scrollback search with regex, whole-word, Unicode width/grapheme/NFC polish.
- OSC 8 hyperlinks with renderer hover overlays and explicit modifier-click
  activation on native and browser.
- IME preedit overlay primitives plus native `winit` and browser input-shim IME
  event handling, search/command-palette text-owner routing where those shells
  exist, browser command palette shell support, with browser runtime smoke
  through node loopback, Rust PTY gateway, and product launcher paths.
- capped main-screen scrollback backed by deque storage and visible-window-only
  snapshot row cloning for the first large-history performance pass, plus a
  configurable local scrollback line budget for native and browser product
  entry points.

This is enough for the current Witty terminal line, but not yet enough to call
the core a serious xterm-compatible engine for daily TUI workloads.

## Product Criterion

The next milestone should prove that normal terminal applications work before
we spend more time on SSH inventory, SFTP, vaults, or AI workflows.

Target applications:

| App class | Reason |
| --- | --- |
| `tmux` | stresses alternate screen, mouse, focus, clipboard escapes, resize, and nested terminal identity |
| `vim`/`nvim` | stresses cursor movement, erase/edit controls, colors, mouse, bracketed paste, and terminal reports |
| `less`/`man` | stresses alternate screen, scrollback restoration, search, and resize |
| `htop`/`btop` | stresses high-frequency updates, colors, mouse, and function keys |
| `vttest` subset | catches protocol regressions that ordinary app smoke misses |

Browser and native should share the same terminal-core expectations. Browser
smokes can run through the Node loopback, Rust PTY gateway, and product
launcher paths when user-facing behavior matters.

## Remaining Compatibility Gaps

| Area | Why it matters | Initial stance |
| --- | --- | --- |
| OSC 52 clipboard | common in tmux/SSH workflows; named in the competitor feature matrix | implement next, with strict local policy |
| device/status reports | TUIs query cursor position and terminal identity | plan after OSC 52; reply bytes must go through the transport boundary |
| OSC palette/theme controls | apps and shell integrations may query or set palette slots | set/query and indexed/default repaint landed |
| compatibility smoke harness | current Playwright smoke is strong for browser features, weaker for real TUI apps | add scripted recordings and bounded PTY app smokes |
| IME/composition | needed for non-English interactive input | native/browser terminal-input paths done; native search/palette plus browser search/palette routing done |
| huge scrollback perf | product competitiveness for long sessions | first storage/snapshot hot-path pass landed; renderer and search measurement still needed |
| shell integration/blocks | Warp-like product UX | OSC 133 state/navigation/anchors, gutter overlay, hit testing, hover, click-to-select, selected-block text ranges, anchored text extraction, block-scoped copy/action menu, exit-status/duration labels, timing metadata, local fold/unfold state, folded status-label feedback with hidden-row counts, folded hidden-row span planning, folded summary-row chrome, compact visual-row mapping, folded frame remapping, native/browser interaction coordinate remapping, and product-level folded compact layout smoke landed; richer block actions and folded-layout polish remain product work |
| images (`sixel`, Kitty, iTerm2) | useful but large surface area | defer as a separate graphics-protocol line |

## OSC 52 Decision

Select OSC 52 as the next line because:

- it is one of the few remaining advanced terminal features called out in the
  paid-terminal research matrix.
- clipboard infrastructure already exists in native and browser shells.
- it is security-sensitive, so implementing it now forces the correct policy
  boundary before SSH and remote sessions expand.
- it improves real `tmux` workflows earlier than palette or report polish.

Security boundary:

- default policy should be `confirm` or `disabled` for remote/gateway sessions,
  and never silently grant arbitrary clipboard writes from terminal output.
- support clipboard write first; clipboard read/query should remain disabled
  until a later explicit user-gesture design exists.
- cap decoded payload size and reject invalid base64, NUL bytes, and non-text
  control characters while allowing normal text newlines and tabs.
- do not expose clipboard payloads through plugin events, smoke diagnostics, or
  command arguments.
- browser support must respect Clipboard API user-gesture constraints; if the
  browser cannot write, report a clear policy/permission failure rather than
  pretending success.

## Next Queue

1. `m137-terminal-osc52-clipboard-plan`
   - done. Documented OSC 52 protocol subset, policy model, native/browser
     boundaries, reply/query non-goals, tests, and follow-up implementation
     tasks. See `terminal-osc52-clipboard-plan.md`.
2. `m138-terminal-osc52-core-policy`
   - done. Parsed OSC 52 into bounded, sanitized host actions without mutating
     terminal cells or plugin-visible state. See `terminal-osc52-core-policy.md`.
3. `m139-native-osc52-clipboard`
   - done. Wired host actions to native clipboard handling with explicit
     `disabled|confirm|allow` policy and deterministic tests. See
     `terminal-osc52-native-clipboard.md`.
4. `m140-browser-osc52-clipboard-smoke`
   - done. Wired browser host actions through JavaScript clipboard policy and
     added Playwright coverage for disabled, allowed, unsupported target, no
     screen-text leak, and no gateway-input behavior. See
     `terminal-osc52-browser-clipboard.md`.
5. `m141-real-tui-compatibility-smoke-plan`
   - done. Defined layered L0-L3 real-TUI smoke strategy, app-specific case
     boundaries, skip policy for missing binaries, and follow-up implementation
     queue. See `real-tui-compatibility-smoke-plan.md`.
6. `m142-terminal-query-reply-path`
   - done. Implemented DA, DSR, and CPR replies as host-internal terminal reply
     actions and wired them to native PTY/browser gateway input boundaries. See
     `terminal-query-reply-path.md`.
7. `m143-real-tui-smoke-harness`
   - done. Added a reusable headless PTY smoke runner with JSON artifacts,
     explicit skip reporting, raw-capture opt-in, and the first
     `less-basic-restore` case. See `real-tui-smoke-harness.md`.
8. `m144-vim-less-real-tui-smokes`
   - done. Added real `vim-basic-edit` and optional `nvim-basic-edit` headless
     PTY cases, kept `less-basic-restore` as the pager baseline, and verified
     all three local real-application smokes. See `real-tui-smoke-harness.md`.
9. `m145-tmux-real-tui-smoke`
   - done. Added `tmux-basic-pane` with isolated socket/config, split-pane
     verification, OSC 52 forwarding host-action coverage, payload non-rendering
     checks, and detached-client cleanup. See `real-tui-smoke-harness.md`.
10. `m146-browser-real-tui-product-smoke`
   - done. Added focused Playwright coverage for real `less-basic-restore`
     through both Rust `witty-gateway` and product `witty --web`, including
     gateway input and nonblank canvas screenshots. See
     `browser-real-tui-product-smoke.md`.
11. `m147-vttest-subset-plan-and-runner`
   - done. Registered optional `vttest-subset` in the headless PTY runner,
     documented the recorded-command replay path and initial page subset, and
     verified the local missing-binary skip artifact. See
     `vttest-subset-smoke.md`.
12. `m148-htop-btop-real-tui-smoke`
   - done. Added optional `htop-or-btop-redraw` with htop-first/btop-fallback
     selection, isolated config dirs, process-table marker checks, output-burst
     assertion, `q` exit path, missing-tool skip reporting, and
     `htop-btop-redraw-smoke.md`.
13. `m149-browser-real-tui-vim-tmux-smokes`
   - done. Added a cached non-reentrant browser screen-read helper, generalized
     the browser real-TUI smoke runner across `less-basic-restore`,
     `vim-basic-edit`, and `tmux-basic-pane`, verified vim through Rust
     `witty-gateway`, tmux through product `witty --web`, and kept the less
     regression passing. See `browser-real-tui-product-smoke.md`.
14. `m150-terminal-palette-controls`
   - done. Added set-only OSC `4`, `10`, `11`, `104`, `110`, and `111`
     handling in `witty-core` so future SGR indexed/default colors can use
     terminal palette/default overrides. Queries and repainting already-written
     indexed cells remain deferred. See `terminal-palette-controls.md`.
15. `m151-terminal-palette-query-replies`
   - done. Added `OSC 4;idx;?`, `OSC 10;?`, and `OSC 11;?` query replies over
     the existing `TerminalReply` host-action path. Already-written
     indexed-color repaint remains deferred. See `terminal-palette-controls.md`.
16. `m152-terminal-palette-retroactive-repaint`
   - done. Added internal color references for indexed/default/direct colors so
     already-written indexed/default-color cells resolve through the current
     palette/default colors while snapshots still expose concrete `Rgba`. See
     `terminal-palette-controls.md`.
17. `m153-terminal-ime-composition-plan`
   - done. Planned native `winit` IME events, browser composition input shim,
     local preedit overlay, privacy boundaries, and m154-m159 follow-up tasks.
     See `terminal-ime-composition-plan.md`.
18. `m154-ime-state-and-overlay-primitives`
   - done. Added shared IME composition state, preedit overlay planning helpers,
     first-class `FramePlan::ime_preedit` rectangles, and focused tests. See
     `terminal-ime-state-overlay-primitives.md`.
19. `m155-native-winit-ime-events`
   - done. Wired native `winit` IME opt-in, cursor-area sync,
     `WindowEvent::Ime` state transitions, commit-to-PTY routing, and focused
     synthetic tests. See `terminal-native-ime-events.md`.
20. `m156-browser-ime-input-shim`
   - done. Added hidden browser input shim, wasm IME preedit/commit/clear
     methods, local preedit overlay rendering, duplicate composition event
     suppression, and node-gateway browser smoke coverage. See
     `terminal-browser-ime-input-shim.md`.
21. `m157-browser-ime-runtime-smoke`
   - done. Extended the browser IME smoke to the Rust PTY gateway and product
     `witty --web` launcher paths, preserving the node loopback assertions
     for preedit, single UTF-8 commit, and duplicate event suppression. See
     `terminal-browser-ime-runtime-smoke.md`.
22. `m158-search-command-palette-ime-routing`
   - done. Routed IME commits by active text owner so terminal commits write
     PTY/gateway bytes, native search and command palette commits update local
     query/filter state, browser search commits update local search state, and
     preedit remains plugin/transport-private. See
     `terminal-search-command-palette-ime-routing.md`.
23. `m159-ime-product-polish`
   - done. Candidate positioning now follows preedit caret offsets and clamps to
     grid bounds, browser IME diagnostics expose target/cursor/input-mode state,
     the browser hidden input responds to visual viewport changes for soft
     keyboards, and manual native/browser validation checklists are documented.
     See `terminal-ime-product-polish.md`.
24. `m160-compatibility-gap-selector`
   - done. Selected the browser command palette shell as the next bounded
     compatibility gap because it completes the remaining browser IME text-owner
     path and exposes plugin/search commands in the browser product shell.
25. `m161-browser-command-palette-shell`
   - done. Added browser `Ctrl+Shift+P` command palette open/filter/select/
     confirm/close behavior, builtin/search/web command registration,
     palette-owned IME preedit and commit routing, no gateway input for local
     palette edits, and node/Rust gateway/product launcher browser smoke
     coverage. See `browser-command-palette-shell.md`.
26. `m162-browser-command-palette-visible-windowing`
   - done. Exposed real command palette selected positions, made browser
     diagnostics use the same compact visible-window limit as the overlay, and
     covered `PageDown` movement so the three-row window follows the selected
     command. See `browser-command-palette-shell.md`.
27. `m163-browser-renderer-glyphon-buffer-width`
   - done. Reduced browser `glyphon` buffer widths from the full remaining
     surface width to the estimated text-run display width, lowering WebGPU
     staging-buffer pressure enough to restore full command-palette smoke
     coverage across node, Rust PTY, and product launcher paths. See
     `browser-command-palette-shell.md`.
28. `m164-browser-command-shortcuts`
   - done. Added browser command-palette `F1`/`F2` shortcut activation matching
     the palette labels while preserving plain and modified function-key input
     for terminal applications when the palette is closed. Browser smoke now
     verifies palette shortcuts across all gateway modes. See
     `browser-command-palette-shell.md`.
29. `m165-terminal-scrollback-storage-windowing`
   - done. Replaced capped main-screen scrollback storage with `VecDeque`,
     trims overflow rows with `pop_front()`, and makes snapshot visible-row
     construction clone only the requested logical window instead of allocating
     a full scrollback-plus-screen reference list each frame. See
     `terminal-scrollback-performance.md`.
30. `m166-terminal-scrollback-perf-example`
   - done. Added a dependency-free `witty-core` example that generates a large
     scrollback workload and reports feed, tail snapshot, history snapshot, and
     search timings as JSON for local before/after comparisons. See
     `terminal-scrollback-performance.md`.
31. `m167-terminal-scrollback-limit-config`
   - done. Exposed public `BasicTerminal` scrollback-limit APIs, added
     `witty --window/--web --scrollback-lines <N>`, passed web limits through
     launcher session JSON into the browser wasm session, and kept the limit as
     local terminal state outside the gateway protocol. See
     `terminal-scrollback-performance.md`.
32. `m168-browser-launcher-scrollback-config-smoke`
   - done. The launcher Playwright smoke now starts `witty --web` with an
     explicit scrollback line limit and asserts that both the JavaScript session
     helper and wasm `BasicTerminal` session report that limit. See
     `terminal-scrollback-performance.md`.
33. `m169-browser-local-scrollback-wheel`
   - done. Browser mode now handles local scrollback wheel gestures when mouse
     reporting is inactive and Shift-wheel local scrollback under the default
     `shift-select` override policy while preserving plain xterm wheel reports
     for mouse-aware terminal applications. See
     `terminal-scrollback-performance.md`.
34. `m170-browser-webgpu-glyphon-batch-budget`
   - done. Browser WebGPU text rendering now keeps long planned text runs and
     `glyphon` prepare batches under explicit character budgets, pools bounded
     text renderers per chunk, exposes the largest glyph run in
     `FrameStats`/diagnostics, and asserts that budget through browser smoke.
     See
     `browser-webgpu-glyphon-batch-budget.md`.
35. `m171-renderer-glyph-prepare-batch-stats`
   - done. `FrameStats` now reports the number of bounded glyph prepare batches,
     native diagnostics render that count, and browser frame stats/smoke assert
     the value is present and consistent. See
     `renderer-glyph-prepare-batch-stats.md`.
36. `m172-renderer-rect-vertex-capacity-stats`
   - done. `FrameStats` now reports the rect vertex buffer capacity bucket,
     native diagnostics render it, and browser frame stats/smoke assert the
     capacity is present and sufficient for the frame. See
     `renderer-rect-vertex-capacity-stats.md`.
37. `m173-renderer-text-buffer-cache-stats`
   - done. `WgpuRectRenderer` now exposes text buffer cache reuse/rebuild
     counts, browser frame stats include them, and browser smoke asserts the
     renderer cache counts are consistent with glyph runs and prepare batches.
     See `renderer-text-buffer-cache-stats.md`.
38. `m174-retained-planner-overlay-damage`
   - done. Added retained planner tests proving selection-only and cursor-only
     overlay changes reuse cached terminal rows and do not rebuild row content.
     See `retained-planner-overlay-damage.md`.
39. `m175-renderer-cpu-prepare-timing-stats`
   - done. `WgpuRectRenderer` now exposes CPU preparation timing for text buffer
     sync, glyph prepare, and rect vertex sync; browser frame stats and smoke
     output include and validate those timing fields. See
     `renderer-cpu-prepare-timing-stats.md`.
40. `m176-native-incremental-smoke-frame-stats`
   - done. Native `witty --incremental-smoke` now prints bounded JSON frame
     stats for retained planner smoke frames while preserving the existing
     compact row reuse summary. See
     `native-incremental-smoke-frame-stats.md`.
41. `m177-renderer-planner-scrollback-perf-example`
   - done. Added a no-window retained planner performance example for large
     scrollback workloads, reporting full, no-damage, and one-row viewport
     scroll planning timings plus bounded `FrameStats` JSON. See
     `renderer-planner-scrollback-perf-example.md`.
42. `m178-real-tui-smoke-case-list`
   - done. `witty --real-tui-smoke list` now prints the implemented real
     TUI smoke case ids as JSON without starting a PTY, so scripts can discover
     optional compatibility cases directly. See `real-tui-smoke-harness.md`.
43. `m179-real-tui-smoke-all-suite`
   - done. `witty --real-tui-smoke all` now runs the registered real-TUI
     smoke cases in order, preserves per-case artifacts, and writes a bounded
     `all.json` suite summary with pass/fail/skip counts. See
     `real-tui-smoke-harness.md`.
44. `m180-terminal-request-mode-report`
   - done. Added ANSI and DEC private request-mode-report replies for supported
     terminal modes through the existing `TerminalReply` host-action path. See
     `terminal-request-mode-report.md`.
45. `m181-terminal-window-size-query-report`
   - done. Added character-grid size replies for `CSI 18 t` and `CSI 19 t`
     through the existing terminal reply path, while leaving host window-control
     operations unsupported. See `terminal-window-size-query-report.md`.
46. `m182-terminal-synchronized-output-mode`
   - done. Added parser-level tracking for DEC private synchronized output mode
     `CSI ? 2026 h/l`, full-reset clearing, and request-mode-report coverage.
     See `terminal-synchronized-output-mode.md`.
47. `m183-browser-synchronized-output-coalescing`
   - done. Browser gateway output now defers frame rendering while synchronized
     output mode is enabled and renders accumulated damage when the mode is
     disabled, with Playwright smoke coverage. See
     `terminal-synchronized-output-mode.md`.
48. `m184-native-synchronized-output-redraw-gate`
   - done. Native PTY output now skips frame rebuild/redraw requests while
     synchronized output remains enabled and resumes rendering on disable,
     process exit, or transport error. See `terminal-synchronized-output-mode.md`.
49. `m185-browser-synchronized-output-timeout`
   - done. Browser sessions now force a frame flush after 150 ms if synchronized
     output remains enabled, preventing an app from suppressing rendering
     indefinitely while leaving the terminal mode state intact. See
     `terminal-synchronized-output-mode.md`.
50. `m186-native-synchronized-output-timeout`
   - done. Native sessions now keep the same 150 ms synchronized-output
     watchdog: PTY output remains coalesced while mode `2026` is enabled, but
     the event loop forces a frame rebuild/redraw when the deadline expires and
     leaves the mode enabled for later cleanup. See
     `terminal-synchronized-output-mode.md`.
51. `m187-terminal-shell-integration-osc133-core`
   - done. Added typed OSC 133 shell-integration host actions for prompt start,
     command line start, output start, and command finished markers, preserving
     active screen, cursor point, and optional exit code without mutating
     terminal cells. See `terminal-shell-integration-osc133.md`.
52. `m188-terminal-shell-integration-block-state`
   - done. Added `witty-ui` command block aggregation for OSC 133 events, wired
     native/browser host-action drain paths into that state, and exposed browser
     completed-block diagnostics for future UI and smoke coverage. See
     `terminal-shell-integration-osc133.md`.
53. `m189-browser-shell-integration-smoke`
   - done. Exposed browser command-block diagnostics in JavaScript and added a
     Playwright smoke path that injects OSC 133 shell-integration markers,
     verifies completed block metadata, and confirms markers do not leak into
     screen text. See `terminal-shell-integration-osc133.md`.
54. `m190-shell-integration-command-block-queries`
   - done. Added command-block query helpers for last block, block lookup,
     screen filtering, and row-window span summaries; exposed active-screen and
     visible-block browser diagnostics and extended the shell-integration smoke
     test to verify those summaries. See `terminal-shell-integration-osc133.md`.
55. `m191-shell-integration-command-block-navigation`
   - done. Added stateful selected-command-block navigation in `witty-ui`
     with active-screen latest/previous/next behavior, exposed browser
     navigation diagnostics, and extended the shell-integration smoke test to
     verify selected-block lifecycle. See
     `terminal-shell-integration-osc133.md`.
56. `m192-browser-command-block-selection-overlay`
   - done. Browser frames now render the selected visible command block with a
     translucent row highlight plus a left gutter bar, selection navigation
     triggers immediate redraws, and smoke coverage verifies the overlay changes
     frame background counts. See `terminal-shell-integration-osc133.md`.
57. `m193-command-block-navigation-commands`
   - done. Added built-in command registrations for command-block latest,
     previous, next, and clear selection; browser command dispatch handles them
     locally before plugin dispatch, and smoke coverage verifies command-palette
     invocation selects the latest completed block. See
     `terminal-shell-integration-osc133.md`.
58. `m194-native-command-block-command-overlay-parity`
   - done. Shared command-block command handling and selected-block overlay
     rendering through `witty-ui`, registered the same command group in native
     window mode, and added native smoke coverage for latest/clear selection
     plus selected-block overlay rendering. See
     `terminal-shell-integration-osc133.md`.
59. `m195-terminal-command-block-row-anchors`
   - done. Added durable row anchors in `witty-core`, attached optional point
     anchors to OSC 133 host actions, stored those anchors in `witty-ui` command
     blocks, and switched native/browser selected-block overlay rendering to
     intersect block anchors with the current visible viewport. See
     `terminal-command-block-row-anchors.md`.
60. `m196-command-block-gutter-overlay`
   - done. Added a shared native/browser completed-command-block gutter overlay
     for unselected visible blocks, using row anchors for viewport mapping and
     exit-code-based success/failure/neutral colors. See
     `terminal-command-block-gutter-overlay.md`.
61. `m197-command-block-gutter-hit-test`
   - done. Added shared `witty-ui` gutter hit testing over row-anchor-mapped
     command-block spans and exposed browser
     `command_block_gutter_hit_json(offsetX, offsetY)` diagnostics for future
     hover/click actions. See `terminal-command-block-gutter-overlay.md`.
62. `m198-command-block-gutter-hover`
   - done. Added shared native/browser command-block gutter hover overlay,
     native cursor-move hover state, browser
     `update_command_block_gutter_hover(offsetX, offsetY)`, and pointer cursor
     feedback over the gutter hit area. See
     `terminal-command-block-gutter-overlay.md`.
63. `m199-command-block-gutter-click-select`
   - done. Native and browser now consume left-clicks inside the command-block
     gutter hit area and select the matching block locally; browser exposes
     `select_command_block_gutter_hit_json(offsetX, offsetY)` and the smoke
     harness verifies the pointer event path. See
     `terminal-command-block-gutter-overlay.md`.
64. `m200-command-block-text-ranges`
   - done. Added shared selected command-block command/output typed text
     ranges with end-exclusive cell points and optional anchors, exposed browser
     `selected_command_block_text_ranges_json()`, and verified the range
     diagnostics in the command-block smoke path. See
     `terminal-command-block-gutter-overlay.md`.
65. `m201-anchored-text-range-extraction`
   - done. Added `TerminalTextRange` and `BasicTerminal::text_for_range()` for
     end-exclusive text extraction with optional row anchors, added `witty-ui`
     conversion helpers from selected command-block ranges, exposed browser
     `selected_command_block_text_json()`, and verified extracted command/output
     text in unit tests and browser smoke. See
     `terminal-command-block-gutter-overlay.md`.
66. `m202-command-block-copy-actions`
   - done. Added local copy-command and copy-output command-block actions on
     top of selected-block text ranges, wired native clipboard sink handling
     and browser Clipboard API handling before plugin dispatch, normalized
     copied output newlines, and verified native/browser command-palette copy
     flows. See `terminal-command-block-gutter-overlay.md`.
67. `m203-command-block-action-menu`
   - done. Added a local selected command-block action menu, opened from the
     command palette or native/browser gutter right-click, with Copy Output,
     Copy Command, and Clear Selection actions dispatched before plugin
     dispatch. Browser smoke covers right-click open, keyboard selection,
     clipboard writes, and clear-selection behavior. See
     `terminal-command-block-gutter-overlay.md`.
68. `m204-command-block-status-labels`
   - done. Added shared native/browser selected and hovered command-block
     status labels derived from OSC 133 exit-code metadata (`ok`, `exit N`, or
     `done`), using the same row-anchor viewport mapping as the gutter and
     selection overlays. See `terminal-command-block-gutter-overlay.md`.
69. `m205-command-block-timing-metadata`
   - done. Added optional `started_at_ms`, `finished_at_ms`, and `duration_ms`
     metadata to command blocks, recorded from native/browser host observation
     times while keeping clock logic out of `witty-core`. Existing browser
     command-block JSON exposes the fields for diagnostics. See
     `terminal-command-block-gutter-overlay.md`.
70. `m206-command-block-duration-labels`
   - done. Reused the selected/hovered command-block status label overlay to
     append compact duration text when `duration_ms` is available, preserving
     the existing native/browser row-anchor mapping and plugin privacy
     boundary. See `terminal-command-block-gutter-overlay.md`.
71. `m207-command-block-fold-state`
   - done. Added local command-block fold/unfold state with
     `witty.command_block.toggle_fold`, exposed it through the command
     palette and selected-block action menu, serialized folded selected blocks
     in diagnostics, and kept collapsed row layout as a deferred follow-up. See
     `terminal-command-block-gutter-overlay.md`.
72. `m208-command-block-folded-status-label`
   - done. Reused the selected/hovered status label overlay to show folded
     state with labels such as `folded ok 1.3s`, preserving the existing
     row-anchor mapping and leaving collapsed row layout deferred. See
     `terminal-command-block-gutter-overlay.md`.
73. `m209-command-block-folded-hidden-spans`
   - done. Added shared folded hidden-row span planning, exposed browser
     `folded_command_block_hidden_row_spans_json()`, and surfaced
     `foldedHiddenRowSpans` in JavaScript diagnostics without changing rendered
     layout. See `terminal-command-block-gutter-overlay.md`.
74. `m210-command-block-folded-row-mask`
   - done. Added a shared native/browser frame-plan mask that consumes folded
     hidden-row spans, removes terminal glyphs from folded non-summary rows, and
     paints row backgrounds while preserving scrollback, search, selection,
     copy, diagnostics, and row height. See
     `terminal-command-block-gutter-overlay.md`.
75. `m211-command-block-folded-summary-chrome`
   - done. Added folded summary-row chrome spans so gutter, hover, selection,
     and gutter hit testing remain on the summary row for folded multi-row
     blocks while full diagnostics, row height, scrollback, search, selection,
     and copy coordinates remain unchanged. See
     `terminal-command-block-gutter-overlay.md`.
76. `m212-command-block-folded-compact-row-map`
   - done. Added shared compact visual-row mapping for folded blocks and
     browser/JavaScript diagnostics (`folded_command_block_compact_rows_json()`
     and `foldedCompactRows`) without moving frame glyphs, overlays, cursor,
     search, selection, copy ranges, or terminal coordinates yet. See
     `terminal-command-block-gutter-overlay.md`.
77. `m213-command-block-folded-frame-remap`
   - done. Added a reusable `witty-ui` frame-plan remap primitive that consumes
     folded compact visual rows, removes hidden-row glyphs/rectangles, and moves
     later backgrounds, glyphs, cursor, selection/search, hyperlink, and IME
     preedit primitives upward in tests. Native/browser product rendering does
     not call it yet; visual compaction still needs inverse pointer/IME
     coordinate mapping before it is safe to enable. See
     `terminal-command-block-gutter-overlay.md`.
78. `m214-command-block-folded-coordinate-remap`
   - done. Added shared `witty-ui` helpers to map compact visual rows and pixels
     back to original visible terminal rows/pixels, preserving row-local pixel
     offsets. This prepares native/browser pointer, hyperlink, mouse-reporting,
     and IME cursor-area call sites for collapsed rendering without enabling the
     product path yet. See `terminal-command-block-gutter-overlay.md`.
79. `m215-command-block-folded-product-wiring`
   - done. Enabled the folded compact frame remap in native/browser rendering
     after command-block chrome and terminal-owned IME preedit, while keeping
     search/command-palette/diagnostics overlays screen-fixed. Native/browser
     pointer paths now map compact visual pixels back to terminal pixels before
     hyperlink hover/activation, gutter hover/click, mouse reporting, and local
     selection; IME cursor-area placement maps terminal cursor cells forward
     into compact visual rows. See `terminal-command-block-gutter-overlay.md`.
80. `m216-command-block-folded-compact-product-smoke`
   - done. Added native/browser product-level smoke coverage for folded compact
     command-block layout. Native smoke now checks compact-row diagnostics,
     moved frame glyphs, and remapped gutter selection after folding a
     multi-row block; browser node-gateway smoke verifies folded diagnostics,
     direct gutter hit JSON, and synthetic pointer selection of the following
     block through the compact visual row. See
     `terminal-command-block-gutter-overlay.md`.
81. `m217-command-block-folded-summary-count-label`
   - done. Folded selected/hovered command-block status labels now include the
     current hidden visible-row count, for example `folded 2 rows ok 1.3s`,
     while preserving terminal buffers, command-block copy ranges, diagnostics,
     and plugin privacy boundaries. See
     `terminal-command-block-gutter-overlay.md`.

## SSH/Profile Foundation Line

After the command-block folded layout line reached product-level native/browser
smoke coverage, the next implementation line pivots toward the commercial SSH
client surface while preserving the native/browser transport split.

82. `m218-openssh-profile-transport-plan`
   - done. Added `witty_transport::OpenSshProfile` as a serializable profile
     model and native-only `OpenSshProfile::to_local_pty_config(size)` builder
     for deterministic OpenSSH PTY command construction. The slice validates
     destination atoms, emits argv/env entries without shell string assembly,
     supports user/port/identity/config/jump-host/TERM/remote-command options,
     and avoids real network connection, SFTP, vault, inventory, or browser
     argv-control scope. See `ssh-profile-transport-plan.md`.
83. `m219-native-openssh-smoke`
   - done. Added a native no-network OpenSSH smoke path through
     `run_openssh_config_dump_smoke()` and `witty --openssh-profile-smoke`.
     The smoke builds an `OpenSshProfile`, converts it to `LocalPtyConfig`,
     spawns OpenSSH under `LocalPtyTransport` with `ssh -G -F none -o
     BatchMode=yes witty.invalid`, and verifies exit code plus deterministic
     config-dump output. See `ssh-profile-transport-plan.md`.
84. `m220-profile-schema-plan`
   - done. Added product-facing SSH profile schema types in `witty-transport`:
     `SshProfile`, `SshProfileTarget`, `SshCredentialRef`,
     `SshTerminalOptions`, and `OpenSshAdvancedOptions`. The schema separates
     id/name/description/tags from target, credential references, terminal
     options, and advanced OpenSSH argv; conversion to `OpenSshProfile` supports
     default-agent and identity-file references while rejecting unresolved vault
     secret references until a credential resolver exists. See
     `ssh-profile-transport-plan.md`.
85. `m221-browser-gateway-profile-launch`
   - done. Added trusted native launcher support for
     `witty --web --ssh-profile-json <path>`, converting the local
     `SshProfile` JSON into an OpenSSH-backed `LocalPtyConfig` for
     `witty-gateway`. Browser session JSON remains limited to gateway
     URL/token, protocol, input policy, scrollback limit, and expiry; it does
     not receive profile host/user/argv data. The gateway now accepts a trusted
     local PTY config template and applies the negotiated grid size at spawn.
     See `ssh-profile-transport-plan.md`.
86. `m222-profile-store-file-plan`
   - done. Added `profile-store-file-plan.md` and selected a secret-free
     `ProfileStoreV1` boundary with `schema`, `app`, `profiles:
     Vec<SshProfile>`, and optional `default_profile_id`. The plan defines
     platform store locations, validation limits, atomic write expectations,
     migration policy, plugin/sync privacy boundaries, and the next launcher
     shape: `witty --web --profile-store <path> --ssh-profile-id <id>`.
87. `m223-profile-store-types`
   - done. Implemented `witty_transport::ProfileStoreV1`,
     `ProfileStoreValidation`, `SshProfileLaunchability`, exported schema/app
     and limit constants, and added pure JSON parse/pretty serialization plus
     validation helpers. Validation rejects unsupported schema/app values,
     duplicate ids, missing defaults, unknown top-level fields, oversized JSON,
     and profile count/tag/OpenSSH argv limits while retaining unresolved
     `vault_secret` profiles as `RequiresCredentialResolver` records.
88. `m224-profile-store-launcher-selection`
   - done. Added `witty --web --profile-store <path> --ssh-profile-id <id>`
     forwarding and trusted native launcher selection from `ProfileStoreV1`.
     The launcher requires store path and id together, rejects missing ids and
     vault-secret profiles without a resolver, keeps raw `--program`/`--arg`
     mutually exclusive with profile launch, and preserves redacted browser
     session JSON.
89. `m225-profile-store-atomic-write-plan`
   - done. Added `profile-store-atomic-write-plan.md`, defining the native-only
     profile store read/write helper boundary, validate-before-touching policy,
     same-directory temporary-file replacement flow, Unix `0700`/`0600`
     permission expectations, Windows atomic-replace caveat, single-writer
     assumption, and m226 implementation/test scope.
90. `m226-profile-store-atomic-write-implementation`
   - done. Implemented native-only `read_profile_store()` and
     `write_profile_store_atomic()` in `witty-transport`, exported
     `ProfileStoreWriteReport`, migrated launcher store selection to the shared
     reader, and added focused tests for validate-before-touching, parent/file
     creation, existing-file preservation on validation failure, replacement,
     temp cleanup, and Unix `0700`/`0600` permissions.
91. `m227-profile-store-default-path`
   - done. Added native-only default profile store path resolution for
     Linux/non-macOS Unix, macOS, and Windows, exported
     `default_profile_store_path()`, and let `witty --web
     --ssh-profile-id <id>` use the default store when `--profile-store` is not
     supplied. Explicit `--profile-store <path>` still overrides the default
     and still requires `--ssh-profile-id`.
92. `m228-profile-store-locking`
   - done. Added a conservative write-only sibling lock to
     `write_profile_store_atomic()`: invalid stores still fail before touching
     the filesystem, lock acquisition uses create-new semantics, an existing
     lock preserves the current target and fails the write, successful writes
     remove their own lock marker, and Unix lock files use mode `0600`.
93. `m229-profile-store-edit-api-plan`
   - done. Added `profile-store-edit-api-plan.md`, selecting pure
     `ProfileStoreV1` add/update/remove/default mutation helpers and a native
     `edit_profile_store()` transaction that holds the sibling lock across
     read-modify-write. The plan keeps reports non-sensitive, reserves OpenSSH
     config import for a later preview/confirmation flow, and scopes m230 to
     helper implementation without changing launcher behavior.
94. `m230-profile-store-edit-helpers`
   - done. Implemented `ProfileStoreDefaultPolicy`,
     `ProfileStoreMutation`, native `ProfileStoreEditOpenMode`,
     `ProfileStoreEditReport`, and `edit_profile_store()`. Pure mutations cover
     add/update/remove/default behavior; native edits hold the sibling lock
     across read, mutation, validation, and atomic replace while reusing the
     existing temp-file replacement path. Launcher behavior is unchanged.
95. `m231-profile-store-cli-or-import-plan`
   - done. Added `profile-store-cli-plan.md` and selected native CLI profile
     management as the first product surface for the locked edit helpers.
     Initial commands will cover redacted list, add/update/remove, and
     set/clear default while keeping OpenSSH config import as a later
     preview-and-confirm flow.
96. `m232-profile-store-cli-list-add`
   - done. Added native `witty-app` profile-management parsing and execution for
     `witty --profile-store-list [--profile-store <path>]` and
     `witty --profile-store-add --ssh-profile-json <path> [--profile-store
     <path>] [--set-default]`. List output is redacted to default marker, id,
     name, launchability, and tags; add uses `edit_profile_store()` with
     `CreateIfMissing` so the sibling lock covers read, mutation, validation,
     and atomic replace. `witty-launcher` behavior is unchanged.
97. `m233-profile-store-cli-update-remove-default`
   - done. Completed the basic native profile-store CLI mutation surface with
     `--profile-store-update --ssh-profile-json <path>`, `--profile-store-remove
     <id>`, `--profile-store-set-default <id>`, and
     `--profile-store-clear-default`. These commands use
     `edit_profile_store(..., Existing, ...)`, keep mutation output
     non-sensitive, preserve `witty-launcher` behavior, and leave OpenSSH config
     import for a later preview/confirmation plan.
98. `m234-profile-store-openssh-import-preview-plan`
   - done. Added `profile-store-openssh-import-preview-plan.md`, selecting a
     two-phase OpenSSH config import flow: first a non-writing preview with
     structured candidates, warnings, source metadata, redacted output, and
     optional profile-id conflict detection; later an explicit confirmed write
     command with reject/replace conflict policy through `edit_profile_store()`.
     The first implementation slices are `m235` preview types, `m236`
     conservative parser subset, and `m237` redacted native CLI preview.
99. `m235-openssh-import-preview-types`
   - done. Added pure `witty-transport` import preview/candidate/source,
     warning, and conflict types, exported them through `witty_transport`,
     covered serde shape, warning counting, and pure conflict marking from an
     existing `ProfileStoreV1`, and preserved the no-parser/no-write boundary.
100. `m236-openssh-import-parser-subset`
   - done. Added pure `parse_openssh_import_preview(config, config_path)` in
     `witty-transport`, recognizing a conservative OpenSSH host-block subset
     (`Host`, `HostName`, `User`, `Port`, `IdentityFile`, `ProxyJump`,
     `RequestTTY`, unambiguous `SetEnv TERM=...`, and `RemoteCommand`) while
     keeping `Include` warning-only, `ProxyCommand` non-imported, token/env/tilde
     expansion unresolved with warnings, wildcard/negated patterns skipped, and
     profile-store writes out of scope.
101. `m237-profile-store-import-preview-cli`
   - done. Added native `witty --profile-store-import-openssh-preview
     <path> [--profile-store <path>]` in `witty-app`. The command reads only the
     supplied OpenSSH config, prints a redacted TSV-like candidate summary, marks
     profile-id conflicts only when an explicit profile store is supplied, skips
     default-store resolution otherwise, rejects profile-store/launcher/window
     mode mixtures, and performs no profile-store writes.
102. `m238-profile-store-import-confirmed-write-plan`
   - done. Added `profile-store-openssh-import-confirmed-write-plan.md`,
     specifying native-only confirmed OpenSSH import writes with mandatory
     `--confirm`, candidate-id selection, default `reject` and exact-id
     `replace` conflict policies, duplicate-selection rejection, default-profile
     preservation, aggregate redacted output, and locked
     `edit_profile_store(..., CreateIfMissing, ...)` batch semantics.
103. `m239-profile-store-import-confirmed-write-types`
   - done. Added pure confirmed-import apply types and helpers in
     `witty-transport`: `OpenSshImportConflictPolicy`,
     `OpenSshImportSelection`, `OpenSshImportApplyReport`, and
     `apply_openssh_import_preview(...)`. The helper selects by candidate id,
     rejects unknown/duplicate/empty selections, supports reject-vs-exact-id
     replace, preserves store bytes on reject conflicts, preserves default
     profile ids by default, and remains CLI/filesystem-write free.
104. `m240-profile-store-import-confirmed-write-cli`
   - done. Added native `witty --profile-store-import-openssh <path>
     --confirm [--profile-store <path>] [--conflict reject|replace]
     [--import-profile-id <id>]...` in `witty-app`. The command reads only the
     supplied OpenSSH config, resolves the default profile store only for the
     confirmed write path, uses `edit_profile_store(..., CreateIfMissing, ...)`
     for the locked batch mutation, supports all-candidate or selected-id
     imports, rejects missing confirmation and invalid command mixtures,
     preserves default profile ids by default, and prints aggregate redacted
     counts only.
105. `m241-profile-store-import-confirmed-write-review`
   - done. Reviewed the confirmed OpenSSH import write CLI for privacy,
     transaction safety, CLI-combination behavior, and test coverage. No
     blocking findings were found: success output remains aggregate-only,
     preview still avoids default-store resolution, confirmed writes use the
     locked edit transaction, reject/selection/lock failures preserve store
     bytes, and plugin/browser surfaces remain out of scope.
106. `m242-profile-store-host-owned-command-plan`
   - done. Added `profile-store-host-owned-command-plan.md`, selecting a
     trusted host-owned profile/store command UI boundary. The plan starts with
     pure redacted profile summary helpers, then launcher-owned pre-session
     profile picker routes, then selected-id gateway launch, while keeping raw
     profile stores, credential references, local paths, OpenSSH argv, import
     candidates, and writes out of plugin/browser-owned APIs.
107. `m243-profile-store-redacted-summary-types`
   - done. Added serializable `ProfileStoreSummary` / `ProfileSummary` and
     `ProfileStoreV1::redacted_summary()` in `witty-transport`. The helper
     validates the store, returns only id/name/tags/default marker/launchability
     plus launchable vs credential-resolver counts, and tests assert serialized
     summaries omit hosts, users, ports, jump hosts, identity/config paths,
     OpenSSH args, remote commands, credential secret ids, and raw profile/store
     internals.
108. `m244-launcher-profile-picker-plan`
   - done. Added `launcher-profile-picker-plan.md`, selecting an explicit
     `witty --web --profile-picker [--profile-store <path>]` entry point.
     The plan keeps direct launch behavior unchanged, uses a separate picker UI
     token returned only by same-origin bootstrap, serves
     `ProfileStoreSummary` through a redacted no-store route, re-reads the store
     on selected-id handoff, starts the gateway only after a valid selection,
     and defines browser state, failure handling, security rules, and
     fake-`ssh` product smoke coverage.
109. `m245-launcher-profile-picker-redacted-api`
   - done. Added launcher `--profile-picker` parsing and validation, default
     profile-store path resolution for picker mode, `ProfilePickerSession` and
     bootstrap JSON types, and one-use TTL-bound
     `GET /profile-picker/<id>.json` serving only redacted
     `ProfileStoreSummary`. `witty-app` forwards `--profile-picker` under
     `--web`; gateway spawn and selected-id handoff remain scoped to m246.
110. `m246-launcher-profile-picker-selection`
   - done. Added profile selection POST handling for the launcher-owned picker:
     the browser sends only picker UI token plus profile id, native code
     re-reads the profile store, rejects missing or resolver-required profiles,
     starts the pre-bound gateway only after the first valid selection, and
     returns the existing redacted `BrowserSessionConfig` shape. Browser
     `#profile_picker` startup now renders a redacted picker, disables
     unlaunchable profiles, discards the UI token after success, and connects
     through the normal token-protected gateway path. See
     `launcher-profile-picker-plan.md`.
111. `m247-profile-picker-product-smoke`
   - done. Added `WITTY_WEB_SMOKE_GATEWAY=profile-picker` coverage to the
     browser product smoke harness. The smoke creates a temporary profile
     store, puts a fake `ssh` first in `PATH`, verifies redacted DOM/URL
     exposure and stale picker bootstrap/selection behavior, checks the exact
     OpenSSH argv used for the selected profile, then runs the existing browser
     terminal smoke through the launched gateway. See
     `launcher-profile-picker-plan.md`.
112. `m248-host-owned-import-ui-plan`
   - done. Defined the next host-owned OpenSSH import preview/confirm UI
     boundary before any browser or plugin import surface is implemented. The
     plan reuses the proven picker token pattern, sends only redacted
     preview summaries to the browser shell, keeps local config-file reading and
     profile-store writes in native code, and preserve the existing confirmed
     CLI transaction semantics. See `host-owned-import-ui-plan.md`.
113. `m249-import-review-redacted-types`
   - done. Added `OpenSshImportReview` and
     `OpenSshImportCandidateSummary` plus
     `OpenSshImportPreview::redacted_review()` in `witty-transport`. The helper
     returns candidate id/name/tags, warning counts, conflict status, and a
     conservative default selection list while omitting raw `SshProfile`,
     source config metadata, target host/user/port/jump host, identity/config
     paths, OpenSSH args, remote commands, and credential secret ids. See
     `host-owned-import-ui-plan.md`.
114. `m250-launcher-import-review-api`
   - done. Added the explicit
     `witty --web --profile-import-openssh <path> [--profile-store <path>]`
     review entry point. Native launcher code reads the OpenSSH config, marks
     conflicts against an explicit or default profile store, builds a
     short-lived `ProfileImportReviewSession`, and serves one-use
     `GET /profile-import/<id>.json` bootstrap JSON containing only
     `OpenSshImportReview`. Confirmation writes and browser UI remain out of
     scope. See `host-owned-import-ui-plan.md`.
115. `m251-launcher-import-confirm-api`
   - done. Added the token-protected
     `POST /profile-import/<id>/confirm` route. Confirmation validates the UI
     token and selected ids, re-reads the OpenSSH config, re-parses the
     preview, applies selected ids through `apply_openssh_import_preview(...)`
     inside the existing locked `edit_profile_store(..., CreateIfMissing, ...)`
     transaction, and returns aggregate-only mutation JSON. Reject-conflict
     failures preserve store bytes, and route responses avoid local paths and
     raw profile/config details. See `host-owned-import-ui-plan.md`.
116. `m252-profile-import-product-smoke`
   - done. Added `WITTY_WEB_SMOKE_GATEWAY=profile-import` browser smoke
     coverage for the standalone OpenSSH import review/confirm flow. The smoke
     creates a temporary OpenSSH config and profile store, verifies redacted
     DOM/URL/report exposure, proves the bootstrap is one-use, confirms a
     replace import through the browser helper, checks exact aggregate mutation
     counts and written store contents, and observes clean launcher exit after
     confirmation. See `host-owned-import-ui-plan.md`.
117. `m253-profile-picker-import-entry-plan`
   - done. Added `profile-picker-import-entry-plan.md`, selecting a
     native-preauthorized import source binding for picker import entry rather
     than browser-owned local path input. The plan introduces
     `--profile-picker-import-openssh <config-path>` under `--profile-picker`,
     redacted picker bootstrap import actions, a token-protected picker import
     action route that returns only a random `#profile_import=<review-id>` URL,
     and a conservative one-terminal-launch-or-import lifecycle.
118. `m254-picker-import-source-binding`
   - done. Added the native `--profile-picker-import-openssh <path>` source
     binding under `--profile-picker`, forwarded it through `witty-app`, stored
     it as a native-owned picker import source, and extended picker bootstrap
     JSON with redacted `import_url` plus `import_actions`. Tests cover parser
     validation and serialization privacy so config/store paths and raw profile
     data stay out of the browser bootstrap.
119. `m255-picker-import-action-route`
   - done. Added the token-protected `POST /profile-picker/<id>/import`
     route. The browser sends only picker UI token plus native-issued action
     id; launcher code creates an import-review session from the native-owned
     source binding, consumes the picker session, returns only a random
     `#profile_import=<review-id>` URL, and reuses the existing import review
     bootstrap/confirmation routes.
120. `m256-picker-import-product-smoke`
   - done. Added `WITTY_WEB_SMOKE_GATEWAY=profile-picker-import` coverage
     that starts in the profile picker, verifies the redacted OpenSSH import
     action, enters the existing import review UI, confirms a replace import,
     checks stale picker/import routes, verifies exact aggregate counts and
     store contents, and observes clean launcher exit.
121. `m257-post-import-picker-refresh`
   - done. Picker-owned import confirmation now returns an aggregate-only
     report with `next_picker_url`, registers a fresh profile picker session
     from the updated store, keeps the original picker/import routes one-use,
     and extends the product smoke through refreshed picker selection and
     imported profile launch.
122. `m258-refreshed-picker-reimport-boundary`
   - done. Added focused launcher coverage proving that the refreshed picker
     keeps native-owned import actions, can start a second import review using
     the updated store, marks the imported ids as conflicts, and keeps the
     second picker/import entry redacted and one-use.
123. `m259-import-review-segmented-conflict-summary`
   - done. Added a compact `reject`/`replace` segmented conflict control and
     aggregate warning/conflict/global count display to the OpenSSH import
     review. Product smoke now verifies the default reject state, switching to
     replace before confirmation, and exact redacted summary counts.
124. `m260-import-review-conflict-selection-guard`
   - done. Conflict candidates remain unchecked and disabled in default reject
     mode, become selectable only after the user switches to replace, and the
     product smoke now confirms import through the actual review button path.
125. `m261-import-review-reject-product-smoke`
   - done. Added `WITTY_WEB_SMOKE_GATEWAY=profile-import-reject` coverage
     for the default reject path. It confirms only the non-conflicting staging
     candidate, verifies prod is preserved, and checks selected/added/replaced
     and warning aggregate counts.
126. `m262-import-review-completion-summary`
   - done. Successful OpenSSH import confirmation now displays an aggregate
     selected/added/replaced/warning result summary in the review panel.
     Product smoke verifies the visible summary for both reject and replace
     flows while preserving the redaction boundary.
127. `m263-import-review-next-picker-button-smoke`
   - done. Picker-owned import smoke now verifies the rendered post-confirm
     `Profiles` action and clicks it to reach the refreshed picker, keeping the
     return-to-picker path covered through visible UI.
128. `m264-import-review-standalone-next-picker-negative-smoke`
   - done. Standalone import smokes now assert the return-to-picker button
     remains hidden after confirmation, so only picker-owned imports expose the
     refreshed picker action.
129. `m265-import-review-accessibility-smoke`
   - done. Added accessible labels/live-region metadata to the import review
     conflict-policy group and aggregate result summary, with smoke assertions
     to preserve the UI semantics.
130. `m266-import-review-completed-control-lock-smoke`
   - done. Product smoke now verifies import completion leaves the review
     controls locked: candidate checkboxes, conflict-policy buttons, and import
     button are disabled while the selected conflict policy remains visible.
131. `m267-import-review-conflict-toggle-smoke`
   - done. Replace-flow smoke now covers toggling back to reject after selecting
     a conflict, proving the conflict checkbox is cleared and disabled before
     the user re-selects replace.
132. `m268-import-review-empty-selection-disable-smoke`
   - done. Import review product smoke now verifies the empty-selection guard:
     clearing the only selectable candidate disables Import, and re-selecting it
     restores the button before confirmation.
133. `m269-import-review-conflict-button-click-smoke`
   - done. Replace-flow smoke now switches conflict policy through the rendered
     segmented buttons instead of the helper, keeping the policy path covered as
     visible UI.
134. `m270-import-review-candidate-checkbox-click-smoke`
   - done. Import review smoke now toggles candidate selection through rendered
     checkbox clicks for empty-selection and replace-conflict paths, further
     reducing helper-only coverage.
135. `m271-import-review-completed-policy-helper-freeze`
   - done. Disabled/completed import reviews now freeze the selected conflict
     policy even if the helper is called, and product smoke verifies the visible
     segmented state remains unchanged.
136. `m272-import-review-completed-disabled-helper-freeze`
   - done. Completed import reviews now remain disabled even if the
     disabled-state helper is asked to re-enable controls, with smoke assertions
     covering candidates, conflict-policy buttons, and the import button.
137. `m273-import-review-next-picker-url-authorization`
   - done. The import review's return-to-picker helper is now gated by the
     server-provided `next_picker_url` in the completed report. Browser smoke
     rejects forged picker URLs, keeps standalone imports from showing the
     button, and verifies picker-owned imports still use the real refreshed
     picker route.
138. `m274-import-review-report-helper-authentication`
   - done. Import review rendering now clears stale completion helper state and
     the report helper accepts only the same aggregate report object installed
     by confirmation. Browser smoke proves a spoofed report cannot lock the
     review, render a fake summary, or authorize a forged picker URL.
139. `m275-import-review-report-helper-replay-freeze`
   - done. Import completion rendering now freezes the accepted aggregate
     report summary after the first helper call. Browser smoke mutates and
     replays the exposed report object and verifies result text, next-picker
     authorization, and button state stay unchanged.
140. `m276-import-review-report-weakset-auth`
   - done. Import report helper authorization now uses an internal WeakSet
     populated only by the confirmation flow, and exposed reports are frozen.
     Browser smoke proves writable window-pointer spoofing is rejected and
     completed report mutation does not change UI state.
141. `m277-import-review-result-summary-freeze`
   - done. Import completion now freezes the exposed aggregate result summary.
     Browser smoke mutates the summary object after completion and verifies the
     counts and visible result text remain unchanged.
142. `m278-import-review-preview-summary-freeze`
   - done. Import review preview candidate summaries and aggregate review
     counts are now frozen before exposure. Browser smoke mutates candidate
     arrays, nested tags, and review counts and verifies the redacted preview
     state remains stable.
143. `m279-profile-picker-exposed-summary-freeze`
   - done. Profile picker redacted summaries and import action metadata are now
     frozen before browser helper exposure, including nested tag arrays.
     Browser smoke covers standalone picker, picker import entry, and refreshed
     picker after import.
144. `m280-bootstrap-exposure-snapshot-freeze`
   - done. Profile picker and import review bootstrap helpers now expose frozen
     route/token snapshots rather than internal mutable bootstrap objects.
     Browser smoke mutates exposed URLs and tokens and verifies the native
     one-use flows still complete using private internal state.
145. `m281-picker-import-entry-freeze`
   - done. Picker-owned import entries are now frozen before browser helper
     exposure, and scheduled navigation uses a captured validated import URL.
     Browser smoke mutates the exposed entry before reload and verifies the
     real import review route is still used.
146. `m282-one-use-helper-token-guard`
   - done. One-use picker and import helpers now guard on the private internal
     token before changing UI state. Browser smoke confirms duplicate
     selection, import-entry, and confirmation helper calls are ignored after
     successful first use.
147. `m283-one-use-helper-in-flight-guard`
   - done. One-use picker selection/import and import confirmation helpers now
     guard concurrent in-flight calls before issuing native requests. Browser
     smoke races duplicate helper calls against the first request and verifies
     the first request remains the only visible state transition.
148. `m284-retry-clears-stale-helper-errors`
   - done. Profile picker and import review helper retries now clear stale
     error state when a new request starts. Browser smoke verifies a bad
     conflict policy can fail recoverably, then succeed without carrying the
     old error forward.
149. `m285-malformed-success-consumes-helper-token`
   - done. One-use browser helpers now consume their exposed UI token as soon
     as a successful HTTP response arrives, before parsing response JSON.
     Browser smoke injects malformed successful responses for picker selection,
     picker import-entry, and import confirmation and verifies controls remain
     locked with no token exposed.
150. `m286-next-picker-url-strict-validation`
   - done. Import completion now validates refreshed picker URLs as exact
     relative `/index.html#profile_picker=<32 lowercase hex>` values. Browser
     smoke rejects empty, non-hex, extra-parameter, and absolute URL variants
     while preserving the native refreshed picker link.
151. `m287-picker-import-url-strict-validation`
   - done. Picker import-entry responses now validate import review URLs as
     exact relative `/index.html#profile_import=<32 lowercase hex>` values.
     Browser smoke rejects a prefix-looking but non-hex review id and verifies
     the browser does not navigate to the forged import page.
152. `m288-bootstrap-action-url-strict-validation`
   - done. Browser bootstrap loading now rejects malformed launcher ids and
     accepts picker selection/import and import confirmation endpoints only
     when they exactly match the current hash id. Browser smoke uses legal
     32-hex fake ids for malformed-success tests and verifies real picker and
     picker-owned import flows still complete.
153. `m289-launcher-gateway-url-strict-validation`
   - done. Browser session config now accepts only tokenless loopback
     `ws://127.x.x.x:<port>/witty` or `ws://[::1]:<port>/witty`
     gateway URLs before appending the UI token. Browser smoke injects an
     external gateway URL through a malformed successful picker selection and
     verifies the token is consumed without connecting; direct launcher smoke
     still completes.
154. `m290-launcher-hash-exclusive-routing`
   - done. Launcher hash parsing now rejects route hashes that include a
     recognized launcher key plus extra hash parameters, so `#session`,
     `#profile_picker`, and `#profile_import` each own the page route
     exclusively. Browser smoke opens a mixed picker/import hash and verifies
     the page fails before fetching a bootstrap, while real launcher-backed
     flows still complete.
155. `m291-launcher-token-shape-validation`
   - done. Browser bootstrap/session validation now accepts only native-shaped
     64 lowercase hex one-use tokens for gateway sessions, profile pickers,
     and import reviews. Smoke mock responses now use legal fake token shapes,
     and the picker-owned import smoke verifies the real end-to-end flow still
     completes.
156. `m292-native-protected-route-id-validation`
   - done. Native launcher HTTP route parsing now accepts profile picker and
     profile import protected routes only when the route id is a 32 lowercase
     hex session id. Unit tests cover bootstrap/action routes and reject
     short, uppercase, non-hex, and path-segment ids; picker-owned import smoke
     verifies real random ids still work end to end.
157. `m293-redacted-bootstrap-field-validation`
   - done. Browser bootstrap loading now validates redacted picker/import
     summary fields before entering ready states: tag arrays must contain
     display-safe strings, default ids must agree with boolean default flags,
     launchability/credential/warning/conflict counts must match the listed
     items, and import default selections must not target missing, duplicate,
     or conflicting candidates. Picker import helpers also require a native
     allowlisted action id before sending the UI token, and import confirmation
     reports must pass aggregate schema validation before completion. Browser
     smoke injects malformed `200 OK` bootstrap/report JSON and verifies these
     flows fail closed while real picker/import paths still complete.
158. `m294-redacted-bootstrap-field-whitelist`
   - done. Browser profile picker/import loading now rejects unsupported JSON
     fields on bootstrap envelopes, redacted profile summaries, import actions,
     import review candidates, and confirmation reports. The allowed browser
     contract matches the Rust redacted structs, including `expires_at_ms`,
     and smoke injects path/host-like extra fields to verify accidental raw
     native data cannot enter ready or done states.
159. `m295-redacted-dto-deny-unknown-fields`
   - done. Rust redacted DTOs now use `serde(deny_unknown_fields)` for profile
     summaries, import reviews, candidate summaries, and import confirmation
     reports. Unit tests reject path/host-like extras at both top level and
     nested candidate/profile levels, keeping the native and browser redaction
     contracts aligned.
160. `m296-session-config-field-whitelist`
   - done. Browser gateway session config loading now uses the same strict
     envelope pattern as picker/import bootstraps: only protocol, loopback
     gateway URL, token, mouse policy, scrollback limit, and `expires_at_ms`
     are accepted. Rust `BrowserSessionConfig` also rejects unknown fields, and
     smoke covers malformed session JSON with extra profile-store data.
161. `m297-action-response-field-whitelist`
   - done. Browser action responses now share the strict redacted contract:
     picker selection rejects extra fields in returned session config before
     connecting, and picker-owned import entry rejects unsupported fields before
     storing or navigating to the review URL. Rust import-entry DTOs also use
     `serde(deny_unknown_fields)`, with smoke and unit coverage for path/profile
     extras.
162. `m298-bootstrap-envelope-deny-unknown-fields`
   - done. Rust picker/import bootstrap envelope DTOs and picker import-action
     metadata now deny unknown JSON fields during deserialization. Unit tests
     parse real serialized picker bootstraps, verify omitted empty
     `import_actions` defaults correctly, and reject path-like extras on
     picker/import envelopes and import actions.
163. `m299-confirm-request-local-validation`
   - done. Browser import confirmation helpers now validate the requested
     conflict policy and selected candidate ids before starting a one-use
     request. Invalid helper calls with empty, duplicate, unknown, or
     reject-conflicting ids leave the ready state, token, controls, report, and
     last confirmation promise unchanged, while valid button confirmation and
     in-flight duplicate guards still complete through the native route.
164. `m300-native-action-request-field-whitelist`
   - done. Native picker selection, picker import-action, and import
     confirmation request DTOs are locked with `serde(deny_unknown_fields)`,
     and unit coverage now rejects path/config/store-like extras in those
     browser-to-native request bodies. This keeps raw host metadata from
     slipping into token-protected action payloads if a browser helper or
     scripted caller is compromised.
165. `m301-native-action-json-content-type`
   - done. Native token-protected action POST routes now require an
     `application/json` content type before parsing picker selection,
     picker-owned import, or import confirmation bodies. Missing or non-JSON
     media types return 415; unit tests cover exact, parameterized, missing,
     form, and duplicate `Content-Type` cases, and browser smoke verifies
     text/plain probes do not consume picker/import tokens before normal
     helper flows complete.
166. `m302-native-picker-action-claim-before-side-effects`
   - done. Native profile picker selection/import actions now atomically claim
     the one-use picker state before launching a gateway or building an import
     review, closing the duplicate side-effect window under concurrent POSTs.
     Recoverable launch/build/serialization failures release the claim, with a
     unit test proving a failed import preview build can retry after the config
     becomes readable.
167. `m303-native-confirm-request-preflight`
   - done. Native import confirmation now preflights selected ids against the
     redacted review before claiming the one-use confirmation state. Empty,
     duplicate, unknown, duplicate-candidate, or reject-conflicting selections
     fail as bad requests without consuming the review, while a later valid
     confirmation still proceeds to the transactional native write path.
     Browser smoke also sends a direct JSON POST that bypasses helpers and
     verifies the preflight 400 leaves the UI token and ready state intact.
168. `m304-native-confirm-apply-retry`
   - done. Native import confirmation now releases the one-use confirmation
     claim when the transactional apply/write path fails before producing a
     confirmed write report, so transient config/store failures can be fixed
     and retried. Post-write continuation or response serialization failures
     remain consumed because the profile store may already have changed. Unit
     tests cover duplicate-candidate preflight rejection and native
     apply-failure retry after the OpenSSH config changes back to a valid
     state; browser smoke also forces a helper-visible 409 and verifies the UI
     token survives for the later successful confirmation.
169. `m305-picker-helper-local-validation`
   - done. Browser picker helpers now reject unknown or non-launchable profile
     selections and unknown import actions before assigning a last-promise or
     sending the one-use token. Product smoke covers missing and
     credential-resolver profile selections plus missing import actions,
     verifying ready state, token, controls, and last-promise diagnostics remain
     unchanged before the valid native flows continue.
170. `m306-native-protected-route-not-found`
   - done. Native launcher HTTP now returns no-store 404 for session,
     profile-picker, and profile-import protected route shapes that do not
     match an active valid route, including malformed ids and unknown valid
     ids. Exact active routes still return method-specific 405 where
     appropriate. Unit coverage exercises forged picker/import/session routes
     and verifies they no longer fall through to generic POST handling.
171. `m307-native-protected-prefix-not-found`
   - done. The native no-store 404 fallback now covers the entire
     `/session/`, `/profile-picker/`, and `/profile-import/` protected
     prefixes, not just exact known route suffixes. Unit coverage includes
     forged action/bootstrap subpaths with extra path segments so protected
     route probes cannot fall through to generic 405 or static asset handling.
172. `m308-native-malformed-http-bad-request`
   - done. Native launcher HTTP now returns a no-store 400 response when
     request parsing fails, instead of silently closing malformed connections.
     Unit coverage exercises duplicate `Content-Type`, duplicate/invalid
     `Content-Length`, oversized declared bodies, and malformed request lines
     against protected routes.
173. `m309-native-request-line-strictness`
   - done. Native launcher HTTP request-line parsing now accepts only
     `HTTP/1.0` or `HTTP/1.1` origin-form requests with exactly method, path,
     and version tokens. Unit coverage verifies unsupported HTTP versions,
     extra request-line tokens, and absolute-form URLs return no-store 400.
174. `m310-native-header-line-strictness`
   - done. Native launcher HTTP now rejects malformed header lines instead of
     ignoring them. Unit coverage verifies missing colons, folded headers,
     empty header names, and invalid field names all return no-store 400.
175. `m311-native-request-line-sp-strictness`
   - done. Native launcher HTTP request lines now require explicit single-space
     separators and a token-shaped method. Unit coverage verifies tab-separated,
     double-space, and invalid-method request lines return no-store 400.
176. `m312-native-header-terminator-required`
   - done. Native launcher HTTP now rejects connections that close before the
     `\r\n\r\n` header terminator, instead of treating partial headers as a
     complete request. Unit coverage includes missing-terminator and empty
     request probes returning no-store 400.
177. `m313-native-header-limit-after-terminator`
   - done. Native launcher HTTP now enforces the header byte limit even when
     the terminator is found in an already oversized buffer. Unit coverage
     includes a terminated oversized header returning no-store 400.
178. `m314-web-asset-manifest-field-whitelist`
   - done. Web asset manifest parsing now denies unknown fields while keeping
     the build-generated `generated_by` metadata explicit. Unit coverage
     rejects unknown top-level and per-asset fields so local paths cannot be
     silently added to the static asset contract.
179. `m315-native-opengl-default-backend`
   - done. Linux native `WgpuRectRenderer::new` now requests only the `wgpu`
     OpenGL backend by default instead of `PRIMARY | SECONDARY`, removing
     Vulkan from the local native renderer candidate set while leaving
     non-Linux native platforms on the prior platform-default selection. Unit
     coverage locks the Linux backend policy; local browser/Chromium/WebGPU
     smokes remain suspended on the M1000 Linux machine. See
     `docs/native-opengl-backend-policy.md`.
180. `m316-local-opengl-guard-and-launcher-template`
   - done. Added `.witty-local-opengl-only` to the ignored local marker
     set and guarded both browser smoke entry families so they exit before
     building assets or launching Chromium on this host unless
     `WITTY_ALLOW_LOCAL_CHROMIUM_SMOKE=1` is set deliberately. Removed
     explicit Vulkan enablement from Chromium smoke launch arguments and added
     a validated Linux OpenGL desktop entry template mirroring the aibookmx
     Warp `Exec=env WGPU_BACKEND=gl ...` pattern. See
     `docs/native-opengl-backend-policy.md` and
     `docs/linux-opengl-desktop-entry.md`.
181. `m317-native-opengl-startup-diagnostics`
   - done. Added `witty --renderer-backend-info`, a non-graphical policy
     report that does not open a window or enumerate GPU adapters. On Linux it
     reports `native_backend_policy=gl`, `opengl_only=true`, and
     `honors_wgpu_backend_env=false`, giving the local M1000 host a safe check
     for renderer backend drift without touching the display stack.
182. `m318-native-opengl-window-startup-hardening`
   - done. Added `witty --window --window-startup-report` for bounded
     native startup probes. The report is emitted after native window creation
     and before `wgpu` renderer initialization, includes the selected native
     backend policy, surface size, `will_request_adapter=true`, `chromium=false`,
     and `vulkan_enabled_by_witty=false`, and renderer initialization
     failures now include the same backend-policy fields in stderr. See
     `docs/native-opengl-window-startup.md`.
183. `m319-native-opengl-harness-selection`
   - done. Selected the existing Xvfb screenshot harness as the preferred
     local real-window probe path because it defaults to OpenGL, software GL,
     and X11 without touching Chromium or Vulkan. The harness now launches with
     `--window-startup-report`, writes a startup log, records software-Mesa
     variables in metadata, and keeps active-desktop capture as an explicit
     manual mode. See `docs/gui-screenshot-regression-harness.md`.
184. `m320-native-opengl-xvfb-probe-when-approved`
   - blocked locally. The approved Xvfb/software-GL probe reached
     `witty.native_window_startup` and confirmed `native_backend_policy=gl`,
     `opengl_only=true`, `chromium=false`, and
     `vulkan_enabled_by_witty=false`, then failed before screenshot capture
     because `wgpu` could not create a GL surface under Xvfb/EGL on this host.
     Startup evidence is preserved in
     `target/gui-regression/opengl-xvfb.xwd.startup.log`; see
     `docs/native-opengl-xvfb-probe-result.md`.
185. `m321-headless-renderer-test-harness-plan`
   - done. Planned a layered renderer validation strategy after the local Xvfb
     surface failure: L0-L2 pure/non-graphical checks remain default on the
     Linux/M1000 host, real window and Xvfb probes require deliberate approval,
     offscreen `wgpu` device probes are opt-in and reserved for safe hosts, and
     browser WebGPU smokes stay on other platforms. See
     `docs/headless-renderer-test-harness-plan.md`.
186. `m322-renderer-no-surface-diagnostics`
   - done. Added `witty --renderer-no-surface-diagnostics`, a pure CPU
     diagnostic that reports native backend policy plus representative retained
     frame-planner stats without opening a window, creating a surface,
     requesting an adapter, or creating a device. This is now the preferred
     local renderer diagnostic beyond `--renderer-backend-info`.
187. `m323-safe-host-native-window-validation-on-aibookmx`
   - preflight done. aibookmx has Rust installed and Warp OpenGL desktop
     entries using `Exec=env WGPU_BACKEND=gl ...`, but the SSH session has no
     graphical display variables and `~/src/witty` is not present there.
     Safe-host validation therefore needs an explicit sync/clone step plus a
     graphical-session launch plan. See
     `docs/aibookmx-native-opengl-validation-plan.md`.
188. `m324-prepare-safe-host-sync-plan`
   - done. Documented why Git commit/push/clone is the preferred safe-host
     validation baseline over rsync for the current 37-entry dirty worktree,
     listed aibookmx sync prerequisites, and kept the next remote write step
     behind explicit approval. See `docs/safe-host-sync-plan.md`.
189. `m326-profile-store-launch-check`
   - done. Added `witty --profile-store-check-launch <id>
     [--profile-store <path>]`, a local-safe diagnostic that validates a stored
     SSH profile through the trusted OpenSSH conversion path without launching
     SSH, browser runtime, `wgpu`, Vulkan, or a native window. Output stays
     redacted to ids, launchability, default marker, booleans, and arg counts;
     host/user/path/argv/remote-command values stay out of logs. See
     `docs/profile-store-launch-check.md`.
190. `m328-native-cursor-blink-state`
   - done. Preserved the DECSCUSR blink/steady split in `CursorState` and added
     a pure native `CursorBlinkState` that toggles only the frame cursor overlay
     on a 500 ms timer. It resets on cursor identity changes, disables itself
     for hidden/non-terminal/no-rect cursors, and lets synchronized output
     timeouts take precedence. Validation stayed non-graphical and avoided
     browser, Vulkan, `wgpu` device, and real-window probes. See
     `docs/native-cursor-blink-state.md`.
191. `m330-terminal-bell-host-action`
   - done. Added `TerminalHostAction::Bell` and mapped `BEL` (`0x07`) to that
     typed host action without rendering text, dirtying rows, or emitting
     terminal replies. Native and browser drains intentionally ignore bell until
     an explicit audible/visual policy is designed. Validation stayed
     non-graphical. See `docs/terminal-bell-host-action.md`.
192. `m332-terminal-decstr-soft-reset`
   - done. Added `CSI ! p` soft reset handling in `witty-core`, preserving
     screen contents while resetting current style, active hyperlink attribute,
     insert/origin/autowrap/application key modes, scroll region, cursor
     visuals, saved cursor state, and palette/default-color overrides. See
     `docs/terminal-decstr-soft-reset.md`.
193. `m334-terminal-cursor-positioning-controls`
   - done. Added `ESC D`/`ESC E` and CSI CNL/CPL/CHA/HPA/HPR/VPA/VPR cursor
     positioning controls in `witty-core` with focused parser tests. See
     `docs/terminal-cursor-positioning-controls.md`.
194. `m336-terminal-scroll-up-down-controls`
   - done. Added `CSI Ps S` and `CSI Ps T` scroll-up/down controls in
     `witty-core`, preserving cursor position, respecting the effective scroll
     region, clamping counts to region height, and reusing existing scrollback
     behavior for full-screen scroll-up. See
     `docs/terminal-scroll-up-down-controls.md`.
195. `m338-terminal-repeat-preceding-graphic`
   - done. Added `CSI Ps b` repeat-preceding-graphic support in `witty-core`.
     Repeats default to one, ignore missing predecessor state, clear on reset,
     and reuse the existing print path for autowrap, wide cells, style,
     hyperlink metadata, scrollback, cursor movement, and damage tracking. See
     `docs/terminal-repeat-preceding-graphic.md`.
196. `m340-terminal-cursor-tabulation-controls`
   - done. Added `CSI Ps I` and `CSI Ps Z` cursor forward/backward tabulation
     controls in `witty-core`, using the same default/custom tab stop set as
     `HT`, `HTS`, and `TBC`, defaulting missing/zero parameters to one, and
     clamping to the right edge or column zero when no further stop exists. See
     `docs/terminal-cursor-tabulation-controls.md`.
197. `m342-terminal-screen-alignment-test`
   - done. Added DECALN `ESC # 8` support in `witty-core`, filling the active
     screen with default-style `E` cells while preserving the cursor, leaving
     the inactive main/alternate screen untouched, and distinguishing the ESC
     intermediate form from `ESC 8` cursor restore. See
     `docs/terminal-screen-alignment-test.md`.
198. `m344-terminal-csi-save-restore-cursor`
   - done. Added default-parameter `CSI s` / `CSI u` save and restore cursor
     support in `witty-core`, reusing the existing screen-local saved-cursor
     state while leaving nonzero parameterized `CSI s` and prefixed/intermediate
     `CSI u` variants unclaimed for future margin and Kitty keyboard protocol
     work. See `docs/terminal-csi-save-restore-cursor.md`.
199. `m346-terminal-dec-special-graphics-charset`
   - done. Added G0/G1 charset designation for ASCII and DEC special graphics
     through `ESC ( B`, `ESC ( 0`, `ESC ) B`, and `ESC ) 0`, plus `SI`/`SO`
     active charset switching. Printable characters now pass through the active
     charset before normal cell writes, enabling common ACS box-drawing output
     without renderer changes. See
     `docs/terminal-dec-special-graphics-charset.md`.
200. `m348-terminal-g2-g3-single-shift-charsets`
   - done. Added G2/G3 charset designation for ASCII and DEC special graphics
     through `ESC * B`, `ESC * 0`, `ESC + B`, and `ESC + 0`, plus `ESC N`,
     `ESC O`, and 8-bit `SS2`/`SS3` single-shift invocation for the next
     printable character. `DECSTR` and full reset restore all four charset
     slots to ASCII and clear pending single-shift state. See
     `docs/terminal-g2-g3-single-shift-charsets.md`.
201. `m350-terminal-cursor-save-restore-charset-state`
   - done. Extended the shared saved-cursor state used by `ESC 7`/`ESC 8`,
     DEC private `?1048`, and default `CSI s`/`CSI u` so restore brings back
     the active charset plus G0-G3 charset designations. Pending `SS2`/`SS3`
     single-shift state is cleared on restore because it is transient parser
     input state, not saved cursor state. See
     `docs/terminal-cursor-save-restore-charset-state.md`.
202. `m352-terminal-selective-erase-protection`
   - done. Added DECSCA `CSI Ps " q` protected-character state plus DECSED
     `CSI ? Ps J` and DECSEL `CSI ? Ps K` selective erase handling in
     `witty-core`. The protection bit stays internal to cells; render snapshots,
     plugin events, and terminal replies remain unchanged. Normal ED/EL/ECH and
     character insert/delete controls still erase protected cells. See
     `docs/terminal-selective-erase-protection.md`.
203. `m354-terminal-c1-control-aliases`
   - done. Added 8-bit C1 aliases for IND (`0x84`), NEL (`0x85`), HTS
     (`0x88`), and RI (`0x8d`) by routing them through the same `witty-core`
     paths as `ESC D`, `ESC E`, `ESC H`, and `ESC M`. Existing SS2/SS3 C1
     handling is unchanged. See `docs/terminal-c1-control-aliases.md`.
204. `m356-terminal-c1-sequence-aliases`
   - done. Added a UTF-8 aware `witty-core` feed normalizer for standalone
     8-bit CSI (`0x9b`), ST (`0x9c`), and OSC (`0x9d`) aliases, mapping them
     to `ESC [`, `ESC \`, and `ESC ]` before `vte::Parser`. Legal UTF-8 text
     continuation bytes are preserved, including split `feed` calls. Broader DCS
     query/reply handling remains deferred. See
     `docs/terminal-c1-sequence-aliases.md`.
205. `m358-terminal-c1-string-aliases`
   - done. Extended the same UTF-8 aware feed normalizer for standalone 8-bit
     DCS (`0x90`), SOS (`0x98`), PM (`0x9e`), and APC (`0x9f`) aliases,
     mapping them to the existing 7-bit `vte` string states. Unsupported
     payloads are shielded from visible cells. See
     `docs/terminal-c1-sequence-aliases.md`.
206. `m360-terminal-sgr-colon-color-params`
   - done. Switched SGR color parsing from flattened values to `vte::Params`
     parameter groups so colon truecolor and indexed-color forms such as
     `38:2::r:g:b`, `48:2:r:g:b`, and `38:5:n` parse correctly while keeping
     semicolon color forms and following SGR attributes separate. See
     `docs/terminal-sgr-style-support.md`.
207. `m362-terminal-sgr-extended-text-flags`
   - done. Added core `CellFlags` state for faint/dim (`SGR 2/22`), blink
     (`SGR 5`, `6`, `25`), and overline (`SGR 53/55`) with legacy JSON
     defaults. These are typed parser/state flags only; renderer visual policy
     remains deferred. See `docs/terminal-sgr-style-support.md`.
208. `m364-terminal-sgr-conceal-render-planning`
   - done. Added core `CellFlags::conceal` for `SGR 8/28` and taught the
     renderer glyph planner to skip concealed glyphs without collapsing cell
     columns or removing terminal text from snapshots/selection. See
     `docs/terminal-sgr-style-support.md`.
209. `m366-terminal-sgr-underline-style-state`
   - done. Added typed core underline-style state for xterm/Kitty-style SGR
     underline variants: `4`, `4:0`-`4:5`, `21`, and `24`. The parser now
     preserves single, double, curly, dotted, and dashed underline intent in
     `CellFlags`; renderer-specific drawing policy remains deferred. See
     `docs/terminal-sgr-style-support.md`.
210. `m368-terminal-sgr-underline-color-state`
   - done. Added `CellStyle::underline_color` with legacy snapshot defaults
     and SGR `58`/`59` parsing for truecolor, 256-color, and colon-parameter
     underline-color forms. The renderer still defers actual underline drawing,
     but snapshots now preserve the color intent needed for that pass. See
     `docs/terminal-sgr-style-support.md`.
211. `m370-terminal-sgr-frame-encircle-state`
   - done. Added legacy-compatible `CellFlags` state for framed and encircled
     text using SGR `51`, `52`, and `54`. The parser treats framed and
     encircled as mutually exclusive decoration intents; renderer drawing
     remains deferred. See `docs/terminal-sgr-style-support.md`.
212. `m372-terminal-sgr-baseline-shift-state`
   - done. Added typed `BaselineShift` state for superscript and subscript SGR
     `73`, `74`, and `75`, with legacy JSON defaults. This preserves baseline
     intent in snapshots while leaving glyph scaling/offset policy to the
     renderer. See `docs/terminal-sgr-style-support.md`.
213. `m374-renderer-basic-text-decoration-planning`
   - done. Added a CPU-side `FramePlan::text_decorations` rectangle layer for
     basic underline, double underline, and overline spans. The planner batches
     adjacent cells by row, color, and decoration type, uses underline color
     overrides for underline rectangles, skips concealed cells, and exposes the
     count through frame stats/diagnostics. Curly, dotted, dashed, framed,
     encircled, and baseline-shift visuals remain deferred. See
     `docs/terminal-sgr-style-support.md`.
214. `m376-renderer-faint-color-policy`
   - done. Added CPU planner faint/dim rendering policy: glyph foregrounds and
     implicit underline/overline colors blend halfway from foreground toward
     background when `CellFlags::faint` is set. Explicit underline colors remain
     explicit. See `docs/terminal-sgr-style-support.md`.
215. `m378-renderer-segmented-underline-planning`
   - done. Extended `FramePlan::text_decorations` to render dotted and dashed
     underline as cellwise rectangle segments, while preserving batching by row,
     color, and decoration kind. Curly underline remains deferred because it
     needs a non-rectangular path or shader policy. See
     `docs/terminal-sgr-style-support.md`.
216. `m380-renderer-baseline-shift-planning`
   - done. Applied CPU-side glyph origin offsets for `BaselineShift::Superscript`
     and `BaselineShift::Subscript` runs, using conservative vertical offsets
     without changing glyph size. Font scaling remains deferred to a later
     glyphon/text-buffer policy pass. See `docs/terminal-sgr-style-support.md`.
217. `m382-renderer-strikethrough-planning`
   - done. Added `CellFlags::strike` support to the CPU-side
     `FramePlan::text_decorations` layer. Strikethrough spans batch by row,
     effective foreground color, and contiguous columns, inherit faint/dim
     foreground blending, and skip concealed cells. See
     `docs/terminal-sgr-style-support.md`.
218. `m384-renderer-framed-text-planning`
   - done. Added `CellFlags::framed` support to the CPU-side
     `FramePlan::text_decorations` layer by rendering four rectangle borders
     around each contiguous framed span. Framed spans batch by row, effective
     foreground color, and contiguous columns, inherit faint/dim blending, and
     skip concealed cells. Encircled text remains deferred until the renderer
     has a curve/path policy. See `docs/terminal-sgr-style-support.md`.
219. `m386-renderer-glyph-style-metadata`
   - done. Added `CellFlags` metadata to `GlyphBatchItem` and included it in
     the renderer text-buffer cache key. The CPU planner now carries the exact
     style flags used to split each glyph run through to the renderer layer, so
     later bold/italic/baseline font policy can be applied without guessing or
     reusing stale text buffers. Font selection policy remains unchanged. See
     `docs/terminal-sgr-style-support.md`.
220. `m388-renderer-bold-italic-font-attrs`
   - done. Mapped `CellFlags::bold` and `CellFlags::italic` to glyphon
     `Weight::BOLD` and `Style::Italic` when creating text buffers. The cache
     key already includes style flags, so bold/italic transitions rebuild text
     buffers instead of reusing a stale shaped run. See
     `docs/terminal-sgr-style-support.md`.
221. `m390-renderer-baseline-shift-font-metrics`
   - done. Applied a conservative glyphon metrics scale to superscript and
     subscript text buffers while preserving terminal cell width and the
     existing CPU-side vertical origin offsets. This makes baseline-shifted
     runs visibly smaller without changing row/column layout. See
     `docs/terminal-sgr-style-support.md`.
222. `m392-renderer-curly-underline-planning`
   - done. Added a rectangle-batch curly underline approximation for
     `UnderlineStyle::Curly`. The renderer emits a small stepped wave under the
     affected span, preserving the existing CPU decoration pipeline and avoiding
     new shader/path dependencies. See `docs/terminal-sgr-style-support.md`.
223. `m394-renderer-encircled-text-planning`
   - done. Added `CellFlags::encircled` support to the CPU-side
     `FramePlan::text_decorations` layer. Encircled spans batch by row,
     effective foreground color, and contiguous columns, inherit faint/dim
     blending, skip concealed cells, and render as a conservative rounded-box
     approximation using the existing rectangle batch. See
     `docs/terminal-sgr-style-support.md`.
224. `m396-renderer-blink-phase-planning`
   - done. Added an explicit `blink_visible` phase gate to `FramePlanner` and
     `RetainedFramePlanner`. Blinking glyphs and text decorations remain visible
     by default, but callers can hide them for the off phase; retained planning
     invalidates cached rows when the phase changes. Native/browser timer
     integration remains deferred. See `docs/terminal-sgr-style-support.md`.
225. `m398-native-text-blink-timer`
   - done. Wired the native app loop to the renderer blink phase with an
     independent 500ms text-blink state machine. The timer arms only while the
     visible snapshot contains non-concealed blinking cells, shares the event-loop
     deadline path with cursor blink, and leaves browser timer integration
     deferred. See `docs/terminal-sgr-style-support.md`.
226. `m400-terminal-dcs-decrqss-core`
   - done. Added a bounded `witty-core` DCS collector for DECRQSS
     `DCS $ q Pt ST`, with host-action replies for current SGR style, current
     scroll region, current DECSCUSR cursor style, and current DECSCA character
     protection state. Unsupported printable requests receive a negative
     status-string reply, while oversized/control requests stay ignored. See
     `docs/terminal-dcs-status-strings.md`.
227. `m402-terminal-cursor-save-restore-attributes`
   - done. Extended the shared cursor save/restore slot used by `ESC 7/8`,
     `CSI s/u`, and DEC private `?1048` to preserve current SGR style and the
     current DECSCA protected-character attribute, in addition to position and
     charset state. Restores remain active-screen scoped and clear pending
     single-shift state. See `docs/terminal-cursor-save-restore.md`.
228. `m404-terminal-cursor-save-restore-modes`
   - done. Extended the shared cursor save/restore slot to preserve DECOM
     origin mode and DECAWM autowrap mode. Restore assigns these mode flags
     without rehoming the cursor, and clears pending wrap when restoring a
     disabled-autowrap state. See `docs/terminal-cursor-save-restore.md`.
229. `m406-terminal-cursor-save-restore-pending-wrap`
   - done. Extended the shared cursor save/restore slot to preserve the pending
     autowrap state produced after printing at the right margin. Restored
     pending wrap is gated by the restored autowrap mode, matching Warp's saved
     `Cursor::input_needs_wrap` behavior. See
     `docs/terminal-cursor-save-restore.md`.
230. `m408-terminal-c1-guarded-area-and-decid`
   - done. Added `ESC V`/C1 `0x96` SPA and `ESC W`/C1 `0x97` EPA handling by
     wiring them to the existing protected-cell attribute used by selective
     erase, and added C1 `0x9a` DECID parity with `ESC Z` primary
     device-attributes replies. See `docs/terminal-c1-control-aliases.md` and
     `docs/terminal-selective-erase-protection.md`.
231. `m410-terminal-g2-g3-locking-shifts`
   - done. Added `ESC n` and `ESC o` locking invocation for G2 and G3 charsets,
     reusing the existing charset designation and printable mapping path.
     Locking shifts clear pending `SS2`/`SS3` state just like `SI`/`SO`. See
     `docs/terminal-g2-g3-single-shift-charsets.md`.
232. `m412-terminal-decnkm-keypad-mode`
   - done. Added DEC private `CSI ? 66 h/l` handling for application keypad
     mode, sharing the existing `DECPAM`/`DECPNM` state and exposing it through
     `DECRQM` mode report replies as `CSI ? 66 ; Pm $ y`. See
     `docs/terminal-keypad-application-mode.md` and
     `docs/terminal-request-mode-report.md`.
233. `m414-terminal-back-forward-index`
   - done. Added `ESC 6` DECBI and `ESC 9` DECFI handling as one-column
     backward/forward cursor moves, reusing existing clamping and pending-wrap
     clearing. Horizontal margin scrolling remains deferred until Witty has
     a left/right margin model. See
     `docs/terminal-cursor-positioning-controls.md`.
234. `m416-terminal-decst8c-tab-stop-reset`
   - done. Added `CSI ? 5 W` DECST8C handling in `witty-core`, resetting the
     terminal-global tab stop set back to the default 8-column stops through the
     same helper used by full terminal reset. See `docs/terminal-tab-stops.md`.
235. `m418-terminal-zero-parameter-defaults`
   - done. Tightened CSI zero-parameter defaults for relative cursor movement
     and DECSTBM scroll margins. Explicit `0` now behaves like the missing
     default count for `CUU`/`CUD`/`CUF`/`CUB`, `CNL`/`CPL`, `HPR`/`VPR`, and
     `CSI 0;0 r` restores the full-screen scroll region. See
     `docs/terminal-cursor-positioning-controls.md` and
     `docs/terminal-scroll-region-basics.md`.
236. `m420-terminal-linefeed-newline-mode`
   - done. Added ANSI `LNM` mode `20` handling in `witty-core`: `CSI 20 h/l`
     toggles whether LF/VT/FF perform carriage return before linefeed, `DECSTR`
     and full reset clear the mode, and `CSI 20 $ p` reports set/reset state.
     See `docs/terminal-linefeed-newline-mode.md` and
     `docs/terminal-request-mode-report.md`.
237. `m422-terminal-uk-national-charset`
   - done. Added VT100 UK national replacement charset designation for G0-G3
     through `ESC ( A`, `ESC ) A`, `ESC * A`, and `ESC + A`. The mapping is
     intentionally narrow: `#` becomes `£`, all other printable characters pass
     through ASCII, and existing reset/save/restore charset paths apply. See
     `docs/terminal-national-replacement-charsets.md`.
238. `m424-terminal-dec-private-cursor-blink`
   - done. Added DEC private `CSI ? 12 h/l` handling for cursor blink state,
     preserving the current cursor shape while toggling `CursorState::blink`.
     The mode is reset by existing soft/full reset behavior and is exposed via
     `CSI ? 12 $ p` request-mode reports. See
     `docs/terminal-cursor-visibility-shape.md` and
     `docs/terminal-request-mode-report.md`.
239. `m426-terminal-backarrow-key-mode`
   - done. Added DEC private `CSI ? 67 h/l` backarrow key mode tracking in
     `witty-core`, exposed it through `TerminalInputModes` and `CSI ? 67 $ p`,
     and wired native/browser Backspace encoders to emit BS when set or DEL
     when reset/default. See `docs/terminal-backarrow-key-mode.md`.
240. `m428-terminal-keyboard-action-mode`
   - done. Added ANSI `CSI 2 h/l` keyboard action mode tracking in `witty-core`,
     exposed the locked state through `TerminalInputModes`, reported it through
     `CSI 2 $ p`, and wired native/browser terminal input encoders to suppress
     PTY bytes while locked. See `docs/terminal-keyboard-action-mode.md`.
241. `m430-terminal-reverse-video-mode`
   - done. Added DEC private `CSI ? 5 h/l` reverse-video screen mode in
     `witty-core`, resolved it at snapshot time by swapping final foreground and
     background colors, reset it through DECSTR/full reset, and exposed it via
     `CSI ? 5 $ p`. See `docs/terminal-reverse-video-mode.md`.
242. `m432-terminal-utf8-mouse-encoding`
   - done. Added xterm `CSI ? 1005 h/l` UTF-8 legacy mouse encoding mode,
     exposed `MouseEncodingMode::Utf8`, encoded legacy `CSI M Cb Cx Cy` packets
     with UTF-8 `value + 32` fields, and kept `1016 > 1006 > 1005 > X10`
     encoding precedence. See `docs/terminal-utf8-mouse-encoding.md`.
243. `m434-terminal-reverse-wraparound-mode`
   - done. Added DEC private `CSI ? 45 h/l` reverse wraparound mode for
     received BS controls, wrapping from column 1 to the previous row's final
     column when enabled, clearing it on DECSTR/full reset, and reporting it
     through `CSI ? 45 $ p`. See `docs/terminal-reverse-wraparound-mode.md`.
244. `m436-terminal-urxvt-mouse-encoding`
   - done. Added xterm `CSI ? 1015 h/l` urxvt decimal legacy mouse encoding
     mode, exposed `MouseEncodingMode::Urxvt`, encoded packets as
     `CSI Cb ; Cx ; Cy M` with decimal coordinates, and kept
     `1016 > 1006 > 1015 > 1005 > X10` encoding precedence. See
     `docs/terminal-urxvt-mouse-encoding.md`.
245. `m438-terminal-utf8-encoded-c1-aliases`
   - done. Extended the UTF-8-aware C1 normalizer to treat supported C1
     controls encoded as UTF-8 control-code scalars, such as `C2 9B` CSI and
     `C2 9D` OSC, as protocol controls while preserving printable Latin-1 text
     such as split `C2 A3` `£`. See
     `docs/terminal-utf8-encoded-c1-aliases.md`.
246. `m440-terminal-utf8-mode-selection-noop`
   - done. Added explicit no-op handling for historical `ESC % G` UTF-8
     selection and `ESC % @` default/non-UTF-8 selection sequences. Witty
     remains UTF-8-only, consumes both sequences without rendering bytes, and
     preserves existing G0-G3 charset state. See
     `docs/terminal-utf8-mode-selection.md`.
247. `m442-terminal-c1-transmission-mode-noop`
   - done. Added explicit no-op handling for historical C1 transmission
     selection sequences `ESC SP F` (S7C1T) and `ESC SP G` (S8C1T). Witty
     keeps accepting supported 7-bit, raw 8-bit, and UTF-8 encoded C1 control
     forms concurrently, so these controls are consumed without rendering bytes
     or toggling parser compatibility. See
     `docs/terminal-c1-transmission-mode.md`.
248. `m444-terminal-decrqss-decscl-status`
   - done. Extended the minimal DECRQSS status-string implementation with
     `DCS $ q " p ST`, returning `DCS 1 $ r 65;1 " p ST` as a fixed
     VT500-level, 7-bit-compatible conformance report. This keeps DECSCL
     reporting useful without introducing mutable emulation-level state. See
     `docs/terminal-dcs-status-strings.md`.
249. `m446-terminal-decrqss-decslrm-status`
   - done. Extended DECRQSS with `DCS $ q s ST` for DECSLRM status, reporting
     the current effective full-width left/right boundary as `DCS 1 $ r
     1;cols s ST`. Witty still does not implement horizontal margin state,
     so this is deliberately a status-query compatibility reply rather than
     left/right margin behavior. See `docs/terminal-dcs-status-strings.md`.
250. `m448-terminal-decscl-selection-noop`
   - done. Added explicit no-op handling for DECSCL conformance-level
     selection `CSI Ps ; Ps " p`, keeping Witty on its fixed UTF-8,
     VT500-level-compatible parser policy while `DECRQSS "p` continues to
     report `65;1"p`. See `docs/terminal-decscl-conformance-level.md` and
     `docs/terminal-dcs-status-strings.md`.
251. `m450-terminal-tertiary-device-attributes`
   - done. Added DA3 / tertiary device-attributes replies for `CSI = c` and
     `CSI = 0 c`, returning `DCS ! | 00000000 ST` through the existing
     `TerminalReply` host-action path while ignoring nonzero parameters. See
     `docs/terminal-device-identity-replies.md`.
252. `m452-terminal-xtversion-reply`
   - done. Added XTVERSION replies for `CSI > q` and `CSI > 0 q`, returning
     `DCS > | Witty <crate-version> ST` without exposing host, profile,
     plugin, or renderer details. See
     `docs/terminal-device-identity-replies.md` and
     `docs/terminal-query-reply-path.md`.
253. `m454-terminal-parameters-report`
   - done. Added DECREQTPARM replies for `CSI x`, `CSI 0 x`, and `CSI 1 x`
     through the existing `TerminalReply` path. The reply is static
     (`CSI 2/3 ; 1 ; 1 ; 128 ; 128 ; 1 ; 0 x`) and deliberately does not
     expose PTY, serial, baud-rate, or host transport details. See
     `docs/terminal-parameters-report.md`.
254. `m456-terminal-xtgettcap-static-capabilities`
   - done. Added XTGETTCAP replies for `DCS + q Pt ST`, parsing hexadecimal
     capability names and returning a deterministic static subset for
     `xterm-256color`-compatible color, OSC 52, cursor-style, underline,
     synchronized-output, bracketed-paste, and focus-event probes. Unknown valid
     names receive `DCS 0 + r name ST`, while malformed payloads are ignored
     without rendering. See `docs/terminal-xtgettcap-capabilities.md` and
     `docs/terminal-query-reply-path.md`.
255. `m458-terminal-cursor-color-osc12`
   - done. Added `OSC 12` cursor-color set/query handling, `OSC 112` reset,
     `RenderSnapshot::cursor_color`, renderer cursor-color override planning,
     and XTGETTCAP `Cs`/`Cr` static capability replies. Cursor-color changes
     damage only the current cursor row, while reset/full reset restore the
     renderer default. See `docs/terminal-cursor-color.md`,
     `docs/terminal-cursor-visibility-shape.md`, and
     `docs/terminal-palette-controls.md`.
256. `m460-terminal-current-directory-osc7`
   - done. Added `OSC 7 ; file://host/path ST/BEL` current-directory parsing
     as a host-owned shell-integration signal. The core preserves the URI,
     percent-decodes safe file paths, rejects non-file or control-character
     paths, and emits `TerminalHostAction::CurrentDirectory` without rendering
     payload bytes. Native and browser drains store the latest directory in
     `ShellIntegrationState`, and command blocks capture that directory when
     OSC 133 markers are present. Plugin command invocations can receive this
     metadata through permission-gated command context. See
     `docs/terminal-current-directory-osc7.md` and
     `docs/plugin-command-context-current-directory.md`.
257. `m462-plugin-selected-command-block-context`
   - done. Extended plugin command invocation context with selected OSC 133
     command-block metadata for the active screen: block id, command/output
     ranges, exit code, timing, duration, and block cwd. Native/browser dispatch
     paths reuse the same context builder, Wasm WIT mirrors the record, and
     `PluginHost` filters this metadata by terminal-read tier (`None` empty,
     `SelectionOnly` selected block only, `CurrentScreen`/`FullScrollback`
     full context). `PluginHost` also filters read events so terminal output is
     delivered only to screen/full-scrollback readers and selection changes are
     withheld from no-read plugins. Command/output text is intentionally not
     included in command context.
258. `m464-plugin-command-owner-routing`
   - done. Tightened plugin command dispatch so `CommandInvoked` events are
     routed only to the plugin registered as `CommandRegistration::source_plugin`
     for the invoked command id. Other plugins no longer see command args or
     command context, and unknown command ids are dropped at the plugin-host
     boundary.
259. `m466-plugin-dynamic-command-routing`
   - done. Runtime `PluginAction::RegisterCommand` actions now update
     `PluginHost`'s command ownership table after host-side source-plugin and
     duplicate-id validation. Dynamically registered commands therefore use the
     same owner-only routing path as install-time commands, and duplicate
     runtime command ids are rejected before mutating host state.
260. `m468-plugin-dynamic-command-app-registry-consistency`
   - done. `TerminalApp` now dispatches plugin events with its current command
     registry as a reserved command set, so runtime `RegisterCommand` actions
     cannot collide with app-visible commands before plugin-host and app command
     state are mutated. Focused tests cover dynamic command routing through
     both registries and rejection of app-registry collisions.
261. `m470-plugin-runtime-error-isolation`
   - done. Added `PluginRuntimeFailure` classification for plugin event handler
     errors, disabled failing plugins after the first handler failure, continued
     dispatching the same event to healthy plugins, and kept host policy
     violations as hard dispatch errors. Disabled command owners are not
     retried and command invocations are still never broadcast. See
     `docs/plugin-runtime-error-isolation.md`.
262. `m472-native-window-wasm-plugin-startup`
   - done. Native window mode now uses the same `install_wasm_plugins` startup
     helper as non-GUI smoke mode for `--wasm-plugin` and `--plugin-dir`, while
     browser, diagnostic, profile-store, and bounded smoke modes continue to
     reject startup plugins. CLI tests pin the supported-mode matrix and
     `--window --wasm-plugin <file>` parsing. See
     `docs/wasmtime-runtime-spike.md`.
263. `m474-plugin-host-info-import`
   - done. Added the first non-empty Wasm host import,
     `host.get-host-info()`, returning only app name, app version, and plugin
     ABI version. The default Wasmtime linker registers this import, the fixture
     plugin exercises it via `fixture.host-info`, and the privacy boundary
     explicitly excludes profile, host, path, PTY, renderer, GPU, and system
     details. See `docs/plugin-host-info-import.md`.
264. `m476-plugin-profile-store-summary-import`
   - done. Added permission-gated `host.get-profile-store-summary()` for Wasm
     plugins. It returns `none` unless the manifest has `profile-read` and the
     host provided a summary, and the visible payload is limited to profile
     counts plus whether a default profile is configured. It explicitly excludes
     profile ids, names, targets, SSH paths, credential references, default
     profile id, store path, and raw profile-store content. See
     `docs/plugin-profile-store-summary-import.md`.
265. `m478-plugin-profile-store-summary-host-adapter`
   - done. Added the `witty-ui` adapter from transport-layer
     `ProfileStoreSummary` to plugin-facing `PluginProfileStoreSummary`, mapping
     only counts and default-profile presence. `PluginHost` and `TerminalApp`
     now have Wasm installation paths that inject this mapped summary into
     `WasmPluginState`, keeping `witty-plugin-wasm` independent from profile
     storage types and preserving the count-only plugin ABI boundary.
266. `m480-plugin-profile-picker-request-action`
   - done. Added host-owned `PluginAction::RequestProfilePicker` /
     `request-profile-picker`, gated by `profile-read` and validated before
     queuing. `PluginHost` records accepted requests as
     `PendingProfilePickerRequest` with the host-attached source plugin id, and
     the Wasm fixture exercises the action via `fixture.profile-picker`. The
     action does not expose profile inventory, selected profile id, launch
     status, credential references, target host/user/port, paths, or raw
     profile-store content. `PluginHost` and `TerminalApp` expose
     `take_profile_picker_requests()` so trusted UI can consume queued requests
     once without replaying stale actions. See
     `docs/plugin-profile-picker-request-action.md`.
267. `m482-plugin-profile-launch-request-action`
   - done. Added host-owned `PluginAction::RequestProfileLaunch` /
     `request-profile-launch`, gated by `profile-read` and validated before
     queuing. The action carries only an opaque profile id plus optional reason;
     `PluginHost` rejects empty, overlong, whitespace-bearing, or
     control-character profile ids and records accepted requests as
     `PendingProfileLaunchRequest` with the host-attached source plugin id.
     This slice does not launch SSH, validate profile existence, resolve
     credentials, return launch status, or expose profile metadata. `PluginHost`
     and `TerminalApp` expose `take_profile_launch_requests()` so trusted host
     UI can consume queued requests once and re-read the profile store at launch
     time. See `docs/plugin-profile-launch-request-action.md`.
268. `m484-plugin-profile-launch-request-review`
   - done. Added `witty-ui::review_profile_launch_requests()` as a pure
     host-side review helper for queued profile launch requests. The helper
     revalidates request syntax and a current `ProfileStoreV1`, then returns
     redacted rows with source plugin, requested id, optional reason,
     launchable/credential-resolver/not-found status, profile name, tags, and
     default marker. It does not expose SSH target host/user/port, paths,
     credential ids, OpenSSH arguments, raw store data, launch results, or start
     any connection. See `docs/plugin-profile-launch-request-action.md`.
269. `m486-plugin-profile-launch-request-resolution`
   - done. Added `witty-ui::resolve_profile_launch_request()` as the fail-closed
     host-only companion for consuming one queued launch request. It revalidates
     the current `ProfileStoreV1` and request syntax, errors for missing ids,
     credential-resolver profiles, unsafe request fields, or invalid stores, and
     returns a cloned `SshProfile` only for launchable profiles. It is not part
     of the plugin ABI and does not start PTY/SSH. See
     `docs/plugin-profile-launch-request-action.md`.
270. `m488-plugin-profile-launch-pty-config-resolution`
   - done. Added native-only
     `witty-ui::resolve_profile_launch_pty_config()`, which reuses the
     fail-closed launch request resolution and converts the launchable
     `SshProfile` to an SSH `LocalPtyConfig` without spawning a process. This
     provides a deterministic handoff for later app-owned confirmation,
     replacement, or new-tab policy while keeping PTY/SSH creation out of the
     plugin/runtime layer. See `docs/plugin-profile-launch-request-action.md`.
271. `m490-terminal-app-profile-launch-queue-helpers`
   - done. Added `TerminalApp` convenience helpers for pending profile launch
     requests: `review_pending_profile_launch_requests()` and native-only
     `resolve_pending_profile_launch_pty_configs()`. Both require a
     caller-provided current `ProfileStoreV1`, leave the queue intact, and do
     not start PTY/SSH, giving trusted UI a clean review/confirm/drain boundary.
     See `docs/plugin-profile-launch-request-action.md`.
272. `m492-terminal-app-profile-launch-confirmed-drain`
   - done. Added native-only
     `TerminalApp::take_resolved_profile_launch_pty_configs()`, which resolves
     the full pending launch request batch against a caller-provided current
     `ProfileStoreV1` before draining. Successful batches return SSH
     `LocalPtyConfig` values and clear the queue; any missing profile,
     credential-resolver profile, invalid request, or invalid store leaves the
     queue intact. The helper still does not start PTY/SSH. See
     `docs/plugin-profile-launch-request-action.md`.
273. `m494-plugin-profile-picker-request-review`
   - done. Added `witty-ui::review_profile_picker_requests()` and
     `TerminalApp::review_pending_profile_picker_requests()` for trusted host
     UI. The helpers combine pending picker requests with a current
     `ProfileStoreV1`, revalidate request reasons, and return host-only review
     rows containing source plugin, optional reason, and redacted
     `ProfileStoreSummary`. They do not drain the queue, select a profile,
     start PTY/SSH, or expose inventory through the plugin ABI. See
     `docs/plugin-profile-picker-request-action.md`.
274. `m496-plugin-profile-picker-selection-resolution`
   - done. Added host-only picker selection resolution:
     `witty-ui::resolve_profile_picker_selection()`,
     native-only `witty-ui::resolve_profile_picker_pty_config()`, and matching
     non-mutating `TerminalApp` wrappers by pending request index. The helpers
     revalidate the current `ProfileStoreV1`, queued picker request reason, and
     host-selected profile id, error for missing/resolver-required/unsafe
     selections, return launchable `SshProfile` or SSH `LocalPtyConfig`, leave
     the queue intact, and do not spawn PTY/SSH or expose the selected id back
     to the plugin. See `docs/plugin-profile-picker-request-action.md`.
275. `m498-terminal-app-profile-picker-confirmed-drain`
   - done. Added native-only
     `TerminalApp::take_resolved_profile_picker_pty_config()` plus a focused
     `PluginHost::take_profile_picker_request()` helper. The app helper
     resolves the trusted host-selected profile id to SSH `LocalPtyConfig`
     first, removes only that pending picker request after success, and leaves
     the queue intact on missing/resolver-required/unsafe selections or invalid
     stores. It still does not start PTY/SSH. See
     `docs/plugin-profile-picker-request-action.md`.
276. `m500-terminal-app-profile-launch-single-confirmed-drain`
   - done. Added native-only
     `TerminalApp::resolve_pending_profile_launch_pty_config()` and
     `TerminalApp::take_resolved_profile_launch_pty_config()` plus
     `PluginHost::take_profile_launch_request()` for trusted UI that confirms
     one queued launch request at a time. The take helper resolves the indexed
     request to SSH `LocalPtyConfig` first, removes only that request on
     success, leaves the queue intact on resolution errors, and still does not
     start PTY/SSH. The existing full-batch launch confirmed-drain remains
     available. See `docs/plugin-profile-launch-request-action.md`.
277. `m502-terminal-app-profile-request-dismissal`
   - done. Added app-owned cancellation helpers:
     `TerminalApp::dismiss_pending_profile_picker_request()` and
     `TerminalApp::dismiss_pending_profile_launch_request()`. Both remove only
     the indexed pending request, fail without mutating the queue on out-of-range
     indexes, do not resolve profiles, do not start PTY/SSH, and do not report
     rejection or status back to the plugin. See
     `docs/plugin-profile-picker-request-action.md` and
     `docs/plugin-profile-launch-request-action.md`.
278. `m504-terminal-app-pending-profile-action-model`
   - done. Added the app-level pending profile action model:
     `PendingProfileActionKind`, `PendingProfileActionKey`,
     `PendingProfileActionReview`, and `DismissedPendingProfileAction`.
     `TerminalApp::review_pending_profile_actions()` combines picker and launch
     reviews into UI-list rows with kind plus current queue-index keys, while
     `TerminalApp::dismiss_pending_profile_action()` consumes those keys for
     cancellation. Keys are queue positions, not global event ids, so trusted UI
     should review again after any dismissal or confirmed drain. The model does
     not resolve profiles, start PTY/SSH, or report status back to plugins. See
     `docs/plugin-runtime-selection.md`.
279. `m506-terminal-app-pending-profile-action-confirmed-drain`
   - done. Added native-only unified confirmation types:
     `PendingProfileActionConfirmation` and
     `ResolvedPendingProfileActionPtyConfig`, plus
     `TerminalApp::take_resolved_pending_profile_action_pty_config()`. Picker
     confirmations carry the trusted host-selected profile id; launch
     confirmations use the launch key alone. The helper rejects mismatched key
     kinds, resolves to SSH `LocalPtyConfig`, drains only after success, leaves
     queues intact on errors, and still does not start PTY/SSH. See
     `docs/plugin-runtime-selection.md`.
280. `m508-native-window-profile-action-bridge`
   - done. Added a native-window-side `NativeProfileActionBridge` that keeps a
     pending profile action snapshot and emits refresh, dismiss, and confirmed
     events around the unified `witty-ui` profile action model. The window app
     refreshes the bridge after startup and plugin command invocation, but the
     bridge itself still uses a caller-provided `ProfileStoreV1`, does not read
     profile stores from disk, does not render picker UI, does not start
     PTY/SSH, and does not replace the active transport. See
     `docs/plugin-runtime-selection.md`.
281. `m510-native-profile-action-feedback`
   - done. Native plugin command invocation now feeds a short pending profile
     action count into the local terminal display after picker or launch
     requests. This makes host-owned profile requests visible while preserving
     the current boundary: no profile store write, no picker UI, no PTY/SSH
     start, and no transport replacement yet. See
     `docs/plugin-runtime-selection.md`.
282. `m512-native-profile-action-store-snapshot`
   - done. Native window profile action refresh now reads the default
     `ProfileStoreV1` snapshot when the file exists and treats a missing store
     as empty, so picker and launch review rows can use current host-owned
     profile metadata. The read path remains non-mutating and still does not
     render picker UI, start PTY/SSH, or replace the active transport. See
     `docs/plugin-runtime-selection.md`.
283. `m514-native-profile-action-feedback-privacy`
   - done. Added a regression guard that native terminal feedback for pending
     profile actions stays count-only even when the bridge has richer review
     metadata. This avoids leaking profile ids, names, launchability, reasons,
     target hosts, credentials, or profile-store inventory through the terminal
     buffer to plugins that may later have terminal-read access. See
     `docs/plugin-runtime-selection.md`.
284. `m516-native-profile-action-display-rows`
   - done. Native profile action snapshots now include trusted display rows for
     future picker/launch UI binding. The rows convert host-only review data
     into title, detail, reason, status, and confirm/dismiss labels; launch rows
     disable confirmation for missing profiles or profiles that require a
     credential resolver. The rows stay inside native window state and are not
     written to terminal feedback or exposed through the plugin ABI. See
     `docs/plugin-runtime-selection.md`.
285. `m518-native-profile-action-frame-overlay`
   - done. Native window frame rebuilding now renders trusted pending profile
     action display rows as a `FramePlan` overlay. The overlay removes covered
     terminal glyphs, selection, search highlights, and cursor visuals in its
     panel area, but it does not write profile details into terminal scrollback,
     expose them through the plugin ABI, start PTY/SSH, or replace the active
     transport. See `docs/plugin-runtime-selection.md`.
286. `m520-native-profile-action-overlay-hit-test`
   - done. Added native profile action overlay hit-testing for row, confirm,
     and dismiss targets. Left-clicking dismiss consumes only the matched
     pending host-owned request and refreshes the overlay; row and confirm hits
     are captured so clicks do not pass through to terminal selection or mouse
     reporting, but confirm remains non-launching until trusted picker/launch
     policy is explicitly wired. See `docs/plugin-runtime-selection.md`.
287. `m522-native-profile-action-overlay-hover`
   - done. Added native profile action overlay hover state and row
     highlighting. Pointer movement over the overlay updates only trusted
     native window state, clears lower hyperlink/command-block hover state while
     the overlay is top-most, and does not pass motion through to terminal
     selection or mouse reporting. Hover rendering is a `FramePlan` background
     for visible rows only; hidden summary rows and stale keys are ignored, and
     no profile details are written to terminal scrollback or exposed through
     the plugin ABI. See `docs/plugin-runtime-selection.md`.
288. `m524-native-profile-action-launch-confirm-drain`
   - done. Wired native overlay Confirm clicks for launchable profile-launch
     rows into the trusted confirmed-drain path. The window reloads the current
     default profile store snapshot, resolves the queued request to
     `LocalPtyConfig`, drains only after success, clears stale overlay hover,
     and refreshes the trusted snapshot. It still does not spawn PTY/SSH,
     replace the active transport, report launch results to plugins, or write
     profile details to terminal scrollback. Picker Confirm remains captured
     until a host-owned profile selection UI exists. See
     `docs/plugin-runtime-selection.md`.
289. `m526-native-profile-picker-option-snapshot`
   - done. Native profile action snapshots now include trusted picker option
     rows for pending picker requests. Each row is derived from redacted profile
     summaries and carries only profile id, display name, tags, default marker,
     launchability, and whether selection is currently allowed. The rows stay in
     native host state for later profile-selection UI and are not written to
     terminal feedback, exposed through the plugin ABI, or enriched with SSH
     target host/user/port, credential ids, OpenSSH arguments, or launch
     results. See `docs/plugin-runtime-selection.md`.
290. `m528-native-profile-picker-option-frame-overlay`
   - done. Native profile action overlay rendering now includes trusted picker
     option rows after the pending action rows when space allows. The option
     rows render only redacted summary fields and explicitly avoid target hosts,
     credential ids, and OpenSSH details. Overlay hit-testing captures option
     rows as top-most native UI so clicks do not pass through to terminal
     selection or mouse reporting, but option clicks still do not select or
     launch until the host-owned picker selection policy is wired. See
     `docs/plugin-runtime-selection.md`.
291. `m530-native-profile-picker-option-select-drain`
   - done. Native overlay `[Select]` clicks on launchable picker option rows
     now map into the trusted confirmed-drain path with the selected profile
     id. The window reloads the current default profile store snapshot,
     resolves the selected profile to `LocalPtyConfig`, drains only after
     success, clears stale overlay hover, and refreshes the trusted snapshot.
     Credential-resolver-required options remain display-only, row/body clicks
     remain captured without selection, and the flow still does not spawn
     PTY/SSH, replace the active transport, report launch results to plugins,
     or write profile details to terminal scrollback. See
     `docs/plugin-runtime-selection.md`.
292. `m532-native-profile-action-resolved-handoff`
   - done. Confirmed native profile actions now normalize successful picker or
     launch resolution into a `NativeResolvedProfileActionHandoff` stored only
     in trusted window state. The handoff carries the action key/kind, source
     plugin, selected profile id, reason, and resolved `LocalPtyConfig` for
     later app-owned session policy. It still does not spawn PTY/SSH, replace
     the active transport, report launch results to plugins, or write profile
     details or resolved config data to terminal feedback. See
     `docs/plugin-runtime-selection.md`.
293. `m534-native-profile-action-handoff-queue`
   - done. Replaced the single resolved profile action slot with a trusted FIFO
     `NativeResolvedProfileActionHandoffQueue`. Confirmed picker or launch
     actions append resolved `LocalPtyConfig` handoffs, and app-owned policy can
     take the next handoff explicitly without losing consecutive
     confirmations. Queueing still does not spawn PTY/SSH, replace the active
     transport, report launch results to plugins, or write profile details or
     resolved config data to terminal feedback. See
     `docs/plugin-runtime-selection.md`.
294. `m536-native-profile-action-defer-start-policy`
   - done. Added the first app-owned policy for resolved native profile action
     handoffs: `DeferStart`. After a confirmed picker or launch action queues a
     resolved handoff, the native window consumes the next handoff into a
     trusted deferred-start queue. This preserves the resolved `LocalPtyConfig`
     for later session replacement, new-tab, or credential-resolver policy
     while still avoiding PTY/SSH spawn, active transport replacement, plugin
     launch-result reporting, and terminal feedback leaks. See
     `docs/plugin-runtime-selection.md`.
295. `m538-native-profile-action-start-plan`
   - done. Added a pure native `NativeProfileActionStartPlan` layer for
     deferred profile starts. The current plan mode is `ReplaceCurrentSession`,
     which preserves the selected action metadata and resolved `LocalPtyConfig`
     as trusted window data for later execution. Planning still does not spawn
     PTY/SSH, replace the active transport, reset terminal state, report launch
     results to plugins, or write profile details/resolved config data to
     terminal feedback. See `docs/plugin-runtime-selection.md`.
296. `m540-native-profile-action-default-start-plan`
   - done. Native overlay confirmation now advances the successful resolved
     handoff through `DeferStart` into a default `ReplaceCurrentSession`
     `NativeProfileActionStartPlan`. The plan is queued as trusted window data
     for a later execution step, preserving the resolved `LocalPtyConfig`
     without spawning PTY/SSH, replacing the active transport, resetting
     terminal state, reporting launch results to plugins, or writing profile
     details/resolved config data to terminal feedback. See
     `docs/plugin-runtime-selection.md`.
297. `m542-native-profile-action-replace-session-boundary`
   - done. Added the native execution boundary for
     `ReplaceCurrentSession` start plans. Given an already-created transport,
     the boundary replaces the active transport and resets terminal, search,
     and shell-integration state while preserving app-owned command/plugin
     state. This still does not spawn PTY/SSH, choose tab/session policy,
     report launch results to plugins, or write profile details/resolved config
     data to terminal feedback. See `docs/plugin-runtime-selection.md`.
298. `m544-native-profile-action-spawn-policy`
   - done. Wired native overlay confirmation through the app-owned start policy:
     after a confirmed picker or launch action resolves to a start plan, the
     native window calls `LocalPtyTransport::spawn(plan.config)` and passes the
     resulting transport into the replace-session boundary. Spawn failures keep
     the trusted plan queued and log only to stderr, without reporting launch
     success/failure through the plugin ABI or terminal scrollback. See
     `docs/plugin-runtime-selection.md`.
299. `m546-native-profile-action-start-failure-ui`
   - done. Added trusted native overlay state for failed profile-action starts.
     When spawn fails, the queued start plan remains available and the overlay
     shows a generic `[start failed]` row with `[Retry]` and `[Dismiss]`.
     Retry attempts the queued plan again; dismiss drops that queued start
     plan. Raw spawn errors, SSH targets, credentials, OpenSSH arguments, and
     launch success/failure still do not enter terminal scrollback or the plugin
     ABI. See `docs/plugin-runtime-selection.md`.
300. `m548-native-profile-action-start-success-ui`
   - done. Added trusted native overlay state for successful profile-action
     session replacement. When the replace-session boundary accepts the spawned
     transport, native UI records a dismissible `[started]` row in `FramePlan`
     overlay state. This keeps launch success out of terminal scrollback and the
     plugin ABI while still giving trusted native UI a visible session-start
     marker. See `docs/plugin-runtime-selection.md`.
301. `m550-native-profile-action-current-session-metadata`
   - done. Added app-owned current-session metadata for successful
     profile-action starts. After `ReplaceCurrentSession` succeeds, native
     window state records the action key, action kind, source plugin, selected
     profile id, reason, and start mode for future tab/session UI. The metadata
     deliberately excludes `LocalPtyConfig`, SSH targets, credentials, OpenSSH
     arguments, raw spawn diagnostics, and launch-result reporting, and it is
     not written to terminal scrollback or the plugin ABI. See
     `docs/plugin-runtime-selection.md`.
302. `m552-native-session-tab-strip-read-model`
   - done. Added the first host-owned native session UI surface derived from
     current-session metadata. The session strip renders active profile id,
     action kind, source plugin, and start mode in `FramePlan`, clears terminal
     glyphs under the strip, and still excludes `LocalPtyConfig`, SSH targets,
     credentials, OpenSSH arguments, raw spawn diagnostics, and terminal/plugin
     launch-result reporting. See `docs/plugin-runtime-selection.md`.
303. `m554-native-session-registry`
   - done. Promoted the single current-session field into a native
     `NativeSessionRegistry` with app-owned session ids, active-session state,
     and tab-row generation. The current `ReplaceCurrentSession` policy updates
     the active registry record, while the registry can already represent
     multiple session records for future tab policy. Registry and tab rows still
     exclude `LocalPtyConfig`, SSH targets, credentials, OpenSSH arguments, raw
     spawn diagnostics, and terminal/plugin launch-result reporting. See
     `docs/plugin-runtime-selection.md`.
304. `m556-native-session-tab-hit-switch`
   - done. Added trusted native hit-testing, hover state, and active-session
     switch policy for the session tab strip. Visible tab-span clicks update
     only the registry active-session id and are captured before terminal
     selection, hyperlink activation, or mouse reporting. Hit/hover state
     remains host-owned and still excludes tab inventory, selected tab id,
     `LocalPtyConfig`, SSH targets, credentials, OpenSSH arguments, raw spawn
     diagnostics, and terminal/plugin launch-result reporting. See
     `docs/plugin-runtime-selection.md`.
305. `m558-native-session-runtime-switch-boundary`
   - done. Added a trusted native runtime switch boundary for parked sessions.
     Inactive session records can now hold transport, terminal, search, and
     shell-integration state; switching to a parked session swaps those runtime
     states into the active window and parks the previous active runtime under
     its session id. The current profile-action policy still creates only
     `ReplaceCurrentSession` starts, so plugin requests do not create new
     parked sessions yet. Runtime switching remains host-owned and does not
     expose tab inventory, selected tab id, `LocalPtyConfig`, SSH targets,
     credentials, OpenSSH arguments, raw spawn diagnostics, or launch results to
     terminal scrollback or the plugin ABI. See `docs/plugin-runtime-selection.md`.
306. `m560-native-profile-action-new-tab-start-mode`
   - done. Added a trusted `NewTab` profile-action start mode to the native
     start executor. After native policy creates a transport, `NewTab` inserts
     an inactive native session record and parks transport, terminal, search,
     and shell-integration state under that session id without replacing the
     active runtime. The default confirmation path still uses
     `ReplaceCurrentSession`; the next step is a trusted native policy control
     for choosing replacement versus new-tab starts. New-tab state remains
     host-owned and still excludes tab inventory, selected tab id,
     `LocalPtyConfig`, SSH targets, credentials, OpenSSH arguments, raw spawn
     diagnostics, and launch results from terminal scrollback and the plugin
     ABI. See `docs/plugin-runtime-selection.md`.
307. `m562-native-profile-action-new-tab-ui-policy`
   - done. Wired the trusted native profile-action overlay to choose between
     replace-current and `NewTab` starts. Launch rows now expose separate
     `Launch` and `New Tab` hit targets, and launchable picker option rows
     expose separate `Select` and `New Tab` targets after the host-owned profile
     choice. The selected mode is passed only into native start-plan policy;
     plugins still receive no profile selection, tab inventory, selected tab
     id, `LocalPtyConfig`, SSH targets, credentials, raw spawn diagnostics, or
     launch result reporting. See `docs/plugin-runtime-selection.md`.
308. `m564-native-parked-session-close-boundary`
   - done. Added the first trusted tab lifecycle boundary for closing parked
     inactive native sessions. The helper removes an inactive registry record
     only when a matching parked runtime exists, drops that parked
     transport/terminal/search/shell state, and rejects active or inconsistent
     session ids. Active-session close policy is still future work. Closing
     remains host-owned and still exposes no tab inventory, selected tab id,
     `LocalPtyConfig`, SSH targets, credentials, raw spawn diagnostics, or
     launch results through terminal scrollback or the plugin ABI. See
     `docs/plugin-runtime-selection.md`.
309. `m566-native-active-session-close-switch-boundary`
   - done. Added a trusted active-session close boundary for the safe case where
     another inactive session has a parked runtime. Native code switches the
     target parked runtime into the active window, parks the old active runtime
     under its previous session id, then closes that old parked runtime and
     registry record. Closing the last active session or spawning a fallback
     local session remains future work. The policy still exposes no tab
     inventory, selected tab id, `LocalPtyConfig`, SSH targets, credentials, raw
     spawn diagnostics, or launch results through terminal scrollback or the
     plugin ABI. See `docs/plugin-runtime-selection.md`.
310. `m568-native-session-tab-close-affordance`
   - done. Added a trusted native close affordance to the session tab strip.
     The strip now renders a close marker per visible tab and hit-testing
     distinguishes `Select` from `Close`; close clicks call only host-owned
     parked-session or active-switch close policies. Closing the last active
     session remains future work. The affordance still exposes no tab inventory,
     selected tab id, `LocalPtyConfig`, SSH targets, credentials, raw spawn
     diagnostics, close results, or launch results through terminal scrollback
     or the plugin ABI. See `docs/plugin-runtime-selection.md`.
311. `m570-native-last-active-close-blocked-notice`
   - done. Added the first policy for closing the last active native session:
     the close is blocked rather than spawning a fallback PTY implicitly, and
     the tab strip shows a short trusted native-only notice. The notice contains
     no session id, profile target, credential detail, raw spawn diagnostic,
     close result, or launch result, and it is not written to terminal
     scrollback or the plugin ABI. See `docs/plugin-runtime-selection.md`.
312. `m572-native-session-tab-notice-lifecycle`
   - done. Centralized the trusted session-tab blocked-close notice lifecycle so
     successful native tab switches, successful session closes, and successful
     profile-action session starts clear stale blocked-close feedback. Ignored
     close hits preserve the current notice, while a last-active close block
     restores the short native-only notice. Notice state remains host-owned and
     still exposes no tab inventory, selected tab id, close result, launch
     result, target host, credentials, or raw spawn diagnostic through terminal
     scrollback or the plugin ABI. See `docs/plugin-runtime-selection.md`.
313. `m574-native-active-close-fallback-policy`
   - done. Made the active-session close fallback explicit in trusted native
     policy. When an active close has no parked runtime it can switch to, the
     current default policy still returns a blocked-last-active result; future
     close-window or fallback-session behavior has a single host-owned policy
     boundary to replace. The policy exposes no tab inventory, selected tab id,
     close result, target host, credentials, raw spawn diagnostic, or launch
     result through terminal scrollback or the plugin ABI. See
     `docs/plugin-runtime-selection.md`.
314. `m576-native-session-tab-close-hover-policy`
   - done. Split native session tab hover styling by trusted hit target so the
     select span and `[x]` close span use separate host-owned colors. Close
     hover highlights only the visible close affordance span and remains
     `FramePlan` state only; it exposes no tab inventory, selected tab id, close
     result, target host, credentials, raw spawn diagnostic, or launch result
     through terminal scrollback or the plugin ABI. See
     `docs/plugin-runtime-selection.md`.
315. `m578-native-session-tab-truncated-close-hit-guard`
   - done. Tightened native session tab hit-testing so a truncated close
     affordance never maps to a close action. The `[x]` marker must be fully
     visible before the host-owned close target is active; partially visible
     close text is display-only. This remains native hit-test state only and
     exposes no tab inventory, selected tab id, close result, target host,
     credentials, raw spawn diagnostic, or launch result through terminal
     scrollback or the plugin ABI. See `docs/plugin-runtime-selection.md`.
316. `m580-native-session-tab-notice-priority-hit-guard`
   - done. Made blocked-close notices reserve space in the native session tab
     strip when width allows, so narrow windows still show the native-only
     feedback instead of truncating it behind long tab summaries. Hit-testing now
     uses only the remaining tab-action span width, so the reserved notice area
     never maps to select or close. This remains `FramePlan` and native
     hit-test state only and exposes no tab inventory, selected tab id, close
     result, target host, credentials, raw spawn diagnostic, or launch result
     through terminal scrollback or the plugin ABI. See
     `docs/plugin-runtime-selection.md`.
317. `m582-native-session-tab-hover-refresh-after-notice`
   - done. Session tab click handling now recomputes native hover after close
     and notice state changes, so a blocked-close notice that reserves strip
     width cannot leave a stale pre-notice hover target over the notice area.
     The refreshed hover uses the same notice-aware native hit-test and remains
     `FramePlan` state only; it exposes no tab inventory, selected tab id, close
     result, target host, credentials, raw spawn diagnostic, or launch result
     through terminal scrollback or the plugin ABI. See
     `docs/plugin-runtime-selection.md`.
318. `m584-native-active-close-fallback-action-layer`
   - done. Split the active-session close fallback into a trusted native policy
     step and a native fallback action step before mapping to the existing
     close-result path. The default policy still yields a blocked-last-active
     action and preserves current behavior, while future close-window or
     fallback-session actions now have a host-owned boundary before event-loop
     wiring. The action layer exposes no tab inventory, selected tab id, target
     host, credentials, raw spawn diagnostic, close result, or launch result
     through terminal scrollback or the plugin ABI. See
     `docs/plugin-runtime-selection.md`.
319. `m586-native-active-close-window-request-action`
   - done. Added a non-default close-window fallback action behind the trusted
     active-close fallback policy. When selected, it maps to an internal
     window-close request that the native event loop consumes after mouse input;
     the default policy remains blocked-last-active, so current behavior is
     unchanged. The action clears stale blocked-close notice state and exposes no
     tab inventory, selected tab id, target host, credentials, raw spawn
     diagnostic, close result, or launch result through terminal scrollback or
     the plugin ABI. See `docs/plugin-runtime-selection.md`.
320. `m588-native-last-active-close-window-cli-policy`
   - done. Exposed the non-default close-window active-close fallback as an
     explicit native window CLI option:
     `witty --window --window-last-active-close close-window`. The default
     remains `block`, and the option is rejected outside native window mode.
     Parser coverage verifies accepted, missing, invalid, and non-window cases.
     The option only selects trusted native fallback policy and exposes no tab
     inventory, selected tab id, target host, credentials, raw spawn diagnostic,
     close result, or launch result through terminal scrollback or the plugin
     ABI. See `docs/plugin-runtime-selection.md`.
321. `m590-native-last-active-close-policy-config-values`
   - done. Added stable config-value helpers for native last-active close
     policy values and reused them in parser validation. This keeps
     `block`/`close-window` strings centralized for future diagnostics or config
     reporting while preserving the default `block` behavior. The helper exposes
     no tab inventory, selected tab id, target host, credentials, raw spawn
     diagnostic, close result, or launch result through terminal scrollback or
     the plugin ABI. See `docs/plugin-runtime-selection.md`.
322. `m592-native-last-active-close-fallback-local-session-policy`
   - done. Added the non-default `fallback-local-session` last-active close
     policy value and action. When selected, closing the last active
     profile-action session requests a normal local fallback PTY, replaces the
     active transport, resets terminal/search/shell-integration state, and
     clears the host-owned profile-action session/tab registry plus parked
     runtimes. Spawn failures are stderr-only and restore the trusted blocked
     close notice; the default remains `block`. This exposes no tab inventory,
     selected tab id, target host, credentials, raw spawn diagnostic, close
     result, or launch result through terminal scrollback or the plugin ABI. See
     `docs/plugin-runtime-selection.md`.
323. `m594-native-fallback-local-session-spawn-boundary`
   - done. Split the fallback-local-session PTY spawn boundary into an injected
     transport-spawner helper so pure tests can cover success and failure
     without starting a real PTY. The helper replaces the active transport and
     clears host-owned profile-action session/tab state only after transport
     creation succeeds; spawn failure preserves the existing active transport,
     terminal/search/shell state, session registry, and parked runtimes. This
     exposes no tab inventory, selected tab id, target host, credentials, raw
     spawn diagnostic, close result, or launch result through terminal
     scrollback or the plugin ABI. See `docs/plugin-runtime-selection.md`.
324. `m596-native-startup-report-last-active-close-policy`
   - done. Added the selected last-active close policy config value to the
     native startup report so smoke diagnostics can identify whether the window
     is running with `block`, `close-window`, or `fallback-local-session`.
     Reporting uses the same stable policy strings as CLI parsing and exposes no
     tab inventory, selected tab id, target host, credentials, `LocalPtyConfig`,
     raw spawn diagnostic, close result, or launch result through terminal
     scrollback or the plugin ABI. See `docs/plugin-runtime-selection.md`.
325. `m598-native-session-close-event-request-boundary`
   - done. Added a small native close-result event-request classifier and routed
     session-tab click handling through it. Only `RequestWindowClose` sets the
     internal close-window event-loop request, only
     `RequestFallbackLocalSession` sets the fallback-local-session request, and
     ordinary close, blocked, or ignored results produce no event request. This
     keeps non-blocking close policy wiring in host-owned native state and
     exposes no tab inventory, selected tab id, target host, credentials,
     `LocalPtyConfig`, raw spawn diagnostic, close result, or launch result
     through terminal scrollback or the plugin ABI. See
     `docs/plugin-runtime-selection.md`.
326. `m600-native-startup-report-policy-value-matrix`
   - done. Added pure startup-report coverage for every last-active close
     policy value: `block`, `close-window`, and `fallback-local-session`. The
     report continues to reuse the CLI-owned stable config strings and exposes
     no tab inventory, selected tab id, target host, credentials,
     `LocalPtyConfig`, raw spawn diagnostic, close result, or launch result
     through terminal scrollback or the plugin ABI. See
     `docs/plugin-runtime-selection.md`.
327. `m602-native-last-active-close-policy-value-list`
   - done. Centralized the allowed `--window-last-active-close` config-value
     list on the policy type and reused it in CLI invalid-value diagnostics.
     Parser tests now verify the error lists `block`, `close-window`, and
     `fallback-local-session`, so future policy additions have a single
     config-list source to update. This exposes no tab inventory, selected tab
     id, target host, credentials, `LocalPtyConfig`, raw spawn diagnostic, close
     result, or launch result through terminal scrollback or the plugin ABI. See
     `docs/plugin-runtime-selection.md`.
328. `m604-native-last-active-close-policy-parser-helper`
   - done. Moved last-active close policy config parsing onto
     `WindowLastActiveClosePolicy`, so CLI code no longer duplicates the
     `block`, `close-window`, and `fallback-local-session` string match. Tests
     cover all accepted config values plus invalid input, while CLI still owns
     the user-facing error wrapping. This exposes no tab inventory, selected tab
     id, target host, credentials, `LocalPtyConfig`, raw spawn diagnostic, close
     result, or launch result through terminal scrollback or the plugin ABI. See
     `docs/plugin-runtime-selection.md`.
329. `m606-native-last-active-close-cli-value-matrix`
   - done. Added end-to-end CLI parsing coverage for every
     `--window-last-active-close` value through `AppOptions`: `block`,
     `close-window`, and `fallback-local-session`. This protects the native
     window option surface without launching a window and exposes no tab
     inventory, selected tab id, target host, credentials, `LocalPtyConfig`, raw
     spawn diagnostic, close result, or launch result through terminal
     scrollback or the plugin ABI. See `docs/plugin-runtime-selection.md`.
330. `m608-native-last-active-close-cli-default-block`
   - done. Added a CLI regression test that `witty --window` without
     `--window-last-active-close` keeps the native last-active close policy at
     `block`. This protects the current product default while non-default
     `close-window` and `fallback-local-session` remain explicit opt-ins, and
     exposes no tab inventory, selected tab id, target host, credentials,
     `LocalPtyConfig`, raw spawn diagnostic, close result, or launch result
     through terminal scrollback or the plugin ABI. See
     `docs/plugin-runtime-selection.md`.
331. `m610-native-last-active-close-policy-bridge-config-values`
   - done. Added coverage that every CLI-facing `WindowLastActiveClosePolicy`
     maps to a native active-close fallback policy with the same stable config
     value. This protects the bridge used by native startup reporting and keeps
     `block`, `close-window`, and `fallback-local-session` consistent across
     CLI parsing and native policy state, while exposing no tab inventory,
     selected tab id, target host, credentials, `LocalPtyConfig`, raw spawn
     diagnostic, close result, or launch result through terminal scrollback or
     the plugin ABI. See `docs/plugin-runtime-selection.md`.
332. `m612-native-last-active-close-policy-all-list`
   - done. Added a canonical `WindowLastActiveClosePolicy::all()` list and
     reused it in CLI, startup-report, and native-policy bridge matrix tests.
     Future policy additions now have one variant-list source for these pure
     coverage paths while preserving the explicit config-value list and parser
     behavior. This exposes no tab inventory, selected tab id, target host,
     credentials, `LocalPtyConfig`, raw spawn diagnostic, close result, or
     launch result through terminal scrollback or the plugin ABI. See
     `docs/plugin-runtime-selection.md`.
333. `m614-native-last-active-close-policy-list-sync`
   - done. Added pure CLI coverage that the canonical
     `WindowLastActiveClosePolicy::all()` list and `config_values()` list stay
     ordered together, and that every listed config value round-trips through
     the policy parser. This keeps future policy additions from drifting across
     matrix tests, CLI diagnostics, and startup reporting while exposing no tab
     inventory, selected tab id, target host, credentials, `LocalPtyConfig`,
     raw spawn diagnostic, close result, or launch result through terminal
     scrollback or the plugin ABI. See `docs/plugin-runtime-selection.md`.
334. `m616-native-active-close-policy-all-list`
   - done. Added a test-only canonical list for the private
     `NativeActiveSessionCloseFallbackPolicy` and coverage that its config
     value order matches the CLI-facing `WindowLastActiveClosePolicy::all()`
     order. This protects the native execution-policy bridge from drifting
     when future last-active-close values are added, while exposing no tab
     inventory, selected tab id, target host, credentials, `LocalPtyConfig`,
     raw spawn diagnostic, close result, or launch result through terminal
     scrollback or the plugin ABI. See `docs/plugin-runtime-selection.md`.
335. `m618-native-session-close-result-all-list`
   - done. Added a test-only canonical list for `NativeSessionCloseResult` and
     changed event-request classification coverage to iterate every close
     result. The test verifies that only request-window-close sets the internal
     window-close boolean, only request-fallback-local-session sets the
     fallback-local-session boolean, and `any()` matches those two native-only
     requests. This exposes no tab inventory, selected tab id, target host,
     credentials, `LocalPtyConfig`, raw spawn diagnostic, close result, or
     launch result through terminal scrollback or the plugin ABI. See
     `docs/plugin-runtime-selection.md`.
336. `m620-native-event-request-one-shot`
   - done. Shared window-close and fallback-local-session pending request
     consumption through a small native helper and added pure coverage that the
     flag returns a request once, clears it, and does not repeat on the next
     event-loop check. This keeps native close-window and fallback-local-session
     event requests one-shot while exposing no tab inventory, selected tab id,
     target host, credentials, `LocalPtyConfig`, raw spawn diagnostic, close
     result, or launch result through terminal scrollback or the plugin ABI.
     See `docs/plugin-runtime-selection.md`.
337. `m622-native-event-request-apply-helper`
   - done. Centralized pending native event-request flag application on
     `NativeSessionCloseEventRequests` and covered that the helper sets only
     the requested window-close or fallback-local-session flags without clearing
     already queued requests. This keeps close-result classification, pending
     request application, and one-shot consumption separately testable while
     exposing no tab inventory, selected tab id, target host, credentials,
     `LocalPtyConfig`, raw spawn diagnostic, close result, or launch result
     through terminal scrollback or the plugin ABI. See
     `docs/plugin-runtime-selection.md`.
338. `m624-native-close-notice-result-matrix`
   - done. Converted blocked-close notice lifecycle coverage into a
     current-notice by close-result matrix over every `NativeSessionCloseResult`.
     The matrix keeps trusted notice behavior explicit: blocked close creates
     the native notice, ignored preserves the current notice, and closed,
     window-close, or fallback-local-session results clear it. This exposes no
     tab inventory, selected tab id, target host, credentials, `LocalPtyConfig`,
     raw spawn diagnostic, close result, or launch result through terminal
     scrollback or the plugin ABI. See `docs/plugin-runtime-selection.md`.
339. `m626-interactive-pty-smoke-daily-use`
   - done. Strengthened `witty --pty-smoke` from a one-shot command-output
     check into an interactive PTY smoke on Unix: it spawns the default local
     shell, resizes the PTY to 30x100, writes shell input, verifies command output and
     `stty size`, writes terminal query replies back to the PTY, drains output
     briefly after the exit event, and plans the
     captured terminal frame. This directly protects the local-shell spawn,
     input, resize, output, exit, and render-planning path needed for daily
     native terminal use without launching a GUI or exposing profile inventory,
     selected tab id, target host, credentials, `LocalPtyConfig`, raw spawn
     diagnostic, close result, or launch result through terminal scrollback or
     the plugin ABI.
340. `m628-default-local-shell-terminal-env`
   - done. Added default terminal environment for `LocalPtyConfig::new` so
     native window startup and fallback local sessions launch the user's
     default shell with `TERM=xterm-256color` and `COLORTERM=truecolor`.
     Transport-layer coverage protects those defaults, and `witty
     --pty-smoke` now uses the default shell path and verifies the shell
     observes both values while still checking interactive input, PTY resize,
     terminal host replies, output, exit, and render planning. This improves
     daily TUI behavior for tools such as `less`, `vim`, `nvim`, and `tmux`
     without launching a GUI or
     exposing profile inventory, selected tab id, target host, credentials,
     `LocalPtyConfig`, raw spawn diagnostic, close result, or launch result
     through terminal scrollback or the plugin ABI.
341. `m630-native-window-font-family-cli`
   - done. Added a native-window `--font-family` CLI override and threaded it
     through `TerminalWindowApp` into the wgpu/glyphon renderer. The renderer
     now maps configured font families to `Family::Name(...)`, keeps the
     default `Family::Monospace` path unchanged, trims empty values, and
     includes the font family in text-buffer cache keys so Nerd Font icon runs
     rebuild under the requested family instead of reusing stale monospace
     buffers. This targets daily native use where prompt, shell, and TUI glyphs
     rely on installed Nerd Font families while preserving the current OpenGL
     backend policy and without exposing profile inventory, selected tab id,
     target host, credentials, `LocalPtyConfig`, raw spawn diagnostic, close
     result, or launch result through terminal scrollback or the plugin ABI.
342. `m632-native-window-font-path-binary-sources`
   - done. Added repeatable native-window `--font-path` loading for local
     `.ttf`, `.otf`, and `.ttc` files so Nerd Font icon coverage can work even
     when the font is not installed or visible through fontconfig. The window
     layer reads each configured file as bytes, rejects missing or empty files
     loudly, and the renderer injects them into glyphon as `Source::Binary`
     rather than `Source::File`, matching cosmic-text's renderable source path.
     Startup diagnostics now include only redacted font metadata
     (`font_family`, `font_source_count`) while preserving the OpenGL backend
     policy and without exposing profile inventory, selected tab id, target
     host, credentials, `LocalPtyConfig`, raw spawn diagnostic, close result,
     or launch result through terminal scrollback or the plugin ABI.
343. `m634-native-window-font-env-defaults`
   - done. Added window-only `WITTY_FONT_FAMILY` and
     `WITTY_FONT_PATHS` defaults so daily launches can keep Nerd Font
     coverage without repeating CLI flags. Explicit `--font-family` and
     repeatable `--font-path` values take precedence, invalid font env values
     fail loudly only in native window mode, and web/profile/smoke paths ignore
     those env vars. README now documents both CLI and environment-variable
     launch forms while preserving the local OpenGL backend policy and without
     exposing profile inventory, selected tab id, target host, credentials,
     `LocalPtyConfig`, raw spawn diagnostic, close result, or launch result
     through terminal scrollback or the plugin ABI.
344. `m636-native-window-config-defaults`
   - done. Added optional native-window JSON config loading from
     `window.v1.json` in the Witty config directory, with explicit
     `--window-config <path>`, `WITTY_WINDOW_CONFIG`, and
     `--no-window-config` controls. The config can provide default
     `font_family`, `font_size`, `font_paths`, `scrollback_lines`,
     `mouse_selection_override`, `osc52_clipboard`, and
     `window_last_active_close` values for daily local launches. CLI flags and
     font env vars keep precedence, missing default config is ignored, explicit
     missing or invalid config fails loudly, and unknown JSON fields are
     rejected. README now documents the config shape while preserving the local
     OpenGL backend policy and without exposing profile inventory, selected tab
     id, target host, credentials, `LocalPtyConfig`, raw spawn diagnostic,
     close result, or launch result through terminal scrollback or the plugin
     ABI.
345. `m638-native-window-font-size-config`
   - done. Added native-window `--font-size` and `font_size` config support,
     with integer bounds from 6 to 96. The selected size now flows into the
     shared renderer font config, scales glyphon text metrics and native cell
     metrics together, participates in text-buffer cache invalidation, and is
     reported by native startup diagnostics. This lets daily local launches
     tune terminal density without touching browser/WebGPU/Vulkan paths and
     without exposing profile inventory, selected tab id, target host,
     credentials, `LocalPtyConfig`, raw spawn diagnostic, close result, or
     launch result through terminal scrollback or the plugin ABI.
346. `m640-native-runtime-font-size-shortcuts`
   - done. Added native-window runtime font-size shortcuts for daily use:
     `Ctrl+=`/`Ctrl++` increases, `Ctrl+-` decreases, and `Ctrl+0` resets to
     the default size. Runtime changes preserve the configured font family,
     update the renderer font config, resync `TerminalApp` cell metrics,
     recompute the native grid from the current window size, and resize the
     PTY. This keeps Nerd Font and density tuning interactive while preserving
     the local OpenGL backend policy and without touching browser/WebGPU/Vulkan
     paths or exposing profile inventory, selected tab id, target host,
     credentials, `LocalPtyConfig`, raw spawn diagnostic, close result, or
     launch result through terminal scrollback or the plugin ABI.
347. `m642-native-window-initial-grid-config`
   - done. Added native-window `window_cols` and `window_rows` config defaults
     for daily launches. When configured, the local PTY starts with the
     requested grid size and the window initial inner size is computed from the
     active font metrics; either dimension can be omitted to keep the default
     for that side. Invalid extreme sizes fail loudly during config loading.
     This preserves the default 960x540 startup when no grid size is configured
     and keeps the local OpenGL backend policy unchanged without touching
     browser/WebGPU/Vulkan paths or exposing profile inventory, selected tab id,
     target host, credentials, `LocalPtyConfig`, raw spawn diagnostic, close
     result, or launch result through terminal scrollback or the plugin ABI.
348. `m644-native-window-initial-grid-cli`
   - done. Added native-window `--window-cols <N>` and `--window-rows <N>`
     flags so daily launches can test a terminal grid size without editing
     `window.v1.json`. The flags validate the same bounded ranges as config,
     reject duplicates and non-window modes, and override config one dimension
     at a time so CLI cols can combine with config/default rows. This keeps the
     OpenGL-only native backend policy unchanged and does not touch
     browser/WebGPU/Vulkan paths or expose profile inventory, selected tab id,
     target host, credentials, `LocalPtyConfig`, raw spawn diagnostic, close
     result, or launch result through terminal scrollback or the plugin ABI.
349. `m646-native-window-launch-command-cli`
   - done. Added native-window `--program` plus repeatable `--arg` support so
     daily launches can start `zsh`, `fish`, `tmux`, or a focused diagnostic
     command instead of only the default shell. Native parsing shares the
     existing web launcher option names, rejects `--arg` without `--program`,
     keeps launcher-only flags web-only, and clears launcher args outside web
     mode. The explicit command path still starts with the same default
     terminal environment as the default shell path, preserving
     `TERM=xterm-256color` and `COLORTERM=truecolor` for common TUI programs.
350. `m648-native-window-cwd-cli-and-config`
   - done. Added native-window `--cwd <path>` and `cwd` config defaults so
     daily desktop launches can start local shells or explicit commands in a
     chosen working directory. CLI `--cwd` takes precedence over config, config
     rejects empty paths and unknown fields, and the cwd is applied inside the
     host-owned `LocalPtyConfig` before spawning the PTY. This keeps cwd
     handling out of browser/WebGPU/Vulkan paths and does not expose profile
     inventory, selected tab id, target host, credentials, `LocalPtyConfig`, raw
     spawn diagnostic, close result, or launch result through terminal
     scrollback or the plugin ABI.
351. `m650-native-window-title-cli-and-config`
   - done. Added native-window `--window-title <title>` and `window_title`
     config defaults for daily multi-window use. The configured title is a
     native fallback title used before the child process emits an OSC title, and
     again if the terminal title is later cleared; dynamic shell, editor, and
     tmux OSC titles still take precedence. Empty and duplicate CLI titles fail
     loudly, CLI values override config, config rejects empty titles and unknown
     fields, and the value is kept native-only. This preserves the local OpenGL
     backend policy and does not touch browser/WebGPU/Vulkan paths or expose
     profile inventory, selected tab id, target host, credentials,
     `LocalPtyConfig`, raw spawn diagnostic, close result, or launch result
     through terminal scrollback or the plugin ABI.
352. `m652-native-window-launch-command-config`
   - done. Added native-window `program` plus `args` config defaults so daily
     launches can persist a preferred shell, tmux session, or project command in
     `window.v1.json` rather than repeating `--program` and `--arg` flags. CLI
     launch command values take precedence as a single launch-command override,
     config `args` require a configured `program`, empty programs fail loudly,
     and JSON unknown fields remain rejected. The resulting command is still
     converted only into the host-owned `LocalPtyConfig` for the native window
     local PTY path, preserving the local OpenGL backend policy and avoiding
     browser/WebGPU/Vulkan paths, terminal scrollback disclosure, and plugin ABI
     exposure of profile inventory, selected tab id, target host, credentials,
     raw spawn diagnostic, close result, or launch result.
353. `m654-native-window-cwd-home-expansion`
   - done. Added `~` and `~/...` home expansion for native-window `cwd` parsing
     so `window.v1.json` can use daily-friendly paths such as `"~/src/project"`
     instead of requiring an absolute home directory path. The parser rejects
     unsupported `~user` forms and missing or empty `HOME` values when expansion
     is requested, while leaving relative and absolute non-tilde paths unchanged.
     This stays inside native launch config parsing, preserves the local OpenGL
     backend policy, avoids browser/WebGPU/Vulkan paths, and does not expose
     profile inventory, selected tab id, target host, credentials,
     `LocalPtyConfig`, raw spawn diagnostic, close result, or launch result
     through terminal scrollback or the plugin ABI.
354. `m656-native-window-launch-env-cli-and-config`
   - done. Added repeatable native-window `--env KEY=VALUE` and JSON config
     `env` defaults so daily local shells, tmux sessions, or project commands
     can receive explicit environment variables. CLI env pairs override config
     env defaults for the launch, duplicate keys replace prior values, and
     injected pairs replace matching default terminal env entries such as
     `TERM` while preserving `COLORTERM=truecolor` unless overridden. Invalid
     env keys, missing `=`, missing CLI values, and non-window use fail loudly.
     The env data is converted only into the host-owned `LocalPtyConfig` for the
     native local PTY path, preserving the OpenGL backend policy, avoiding
     browser/WebGPU/Vulkan paths, and exposing no profile inventory, selected
     tab id, target host, credentials, raw spawn diagnostic, close result, or
     launch result through terminal scrollback or the plugin ABI.
355. `m658-native-window-config-template-cli`
   - done. Added `witty --window-config-template`, a non-GUI helper that
     prints a valid starter `window.v1.json` for daily native use. The template
     covers font family/size, fallback title, cwd, env, grid size, scrollback,
     mouse override, OSC 52 policy, and last-active close policy while keeping
     `program` unset so the default shell remains safe. The helper does not load
     native window config defaults, open a window, enumerate GPU adapters, start
     Chromium, or touch browser/WebGPU/Vulkan paths, and it exposes no profile
     inventory, selected tab id, target host, credentials, raw spawn diagnostic,
     close result, or launch result through terminal scrollback or the plugin
     ABI.
356. `m660-native-window-config-default-path-cli`
   - done. Added `witty --window-config-default-path`, a non-GUI helper
     that prints the resolved default `window.v1.json` location used by native
     window mode. This pairs with the template command so daily local setup can
     discover the target path without remembering XDG config resolution rules.
     The helper only calls the existing default config path resolver; it does
     not read or write config files, open a native window, enumerate GPU
     adapters, start Chromium, touch browser/WebGPU/Vulkan paths, or expose
     profile inventory, selected tab id, target host, credentials, raw spawn
     diagnostic, close result, or launch result through terminal scrollback or
     the plugin ABI.
357. `m662-native-window-config-init-cli`
   - done. Added `witty --window-config-init`, a conservative setup helper
     that creates the default `window.v1.json` parent directory and writes the
     same starter JSON as `--window-config-template` only when the file does not
     already exist. Existing configs fail closed instead of being overwritten,
     and tests verify the generated file parses through the native config
     schema. The helper does not open a native window, enumerate GPU adapters,
     start Chromium, touch browser/WebGPU/Vulkan paths, or expose profile
     inventory, selected tab id, target host, credentials, raw spawn diagnostic,
     close result, or launch result through terminal scrollback or the plugin
     ABI.
358. `m664-native-window-config-check-cli`
   - done. Added `witty --window-config-check`, a non-GUI validator for
     daily native `window.v1.json` setup. It validates the resolved default
     config and also accepts `--window-config <path>` for candidate files before
     making them active. The checker reuses the native JSON schema and semantic
     field validation, including launch command, cwd, env, font defaults,
     scrollback, mouse override, OSC 52, and close-policy parsing, but it does
     not read font files, open a native window, enumerate GPU adapters, start
     Chromium, touch browser/WebGPU/Vulkan paths, or expose profile inventory,
     selected tab id, target host, credentials, `LocalPtyConfig`, raw spawn
     diagnostic, close result, or launch result through terminal scrollback or
     the plugin ABI.
359. `m666-native-window-effective-config-cli`
   - done. Added `witty --window-config-effective`, a non-GUI native-window
     startup summary for daily setup. It merges CLI flags, font environment
     defaults, and `window.v1.json` defaults using the same precedence as a real
     native launch, then prints JSON with config load status, OpenGL backend
     policy, fallback title, program presence, argv count, cwd, env key list,
     font metadata, grid size, scrollback, mouse override, OSC 52, and
     last-active-close policy. The summary intentionally redacts env values and
     argv contents, does not read font files, start a PTY, open a window,
     enumerate GPU adapters, start Chromium, touch browser/WebGPU/Vulkan paths,
     or expose profile inventory, selected tab id, target host, credentials,
     `LocalPtyConfig`, raw spawn diagnostic, close result, or launch result
     through terminal scrollback or the plugin ABI.
360. `m668-native-window-config-init-explicit-path`
   - done. Extended `witty --window-config-init` to accept
     `--window-config <path>` for candidate config setup. The command still
     creates missing parent directories, writes the same starter JSON as
     `--window-config-template`, and fails closed when the target already
     exists; it simply uses the explicit candidate path instead of the default
     `window.v1.json`. This keeps daily config experiments safe and scriptable
     without opening a window, starting a PTY, reading font files, enumerating
     GPU adapters, starting Chromium, touching browser/WebGPU/Vulkan paths, or
     exposing profile inventory, selected tab id, target host, credentials,
     `LocalPtyConfig`, raw spawn diagnostic, close result, or launch result
     through terminal scrollback or the plugin ABI.
361. `m670-native-window-smoke-exit-watchdog`
   - done. Replaced the native window smoke exit path with a smoke-only process
     watchdog for `--window-exit-after-ms`. Local OpenGL-only GUI probes on this
     Linux/M1000 machine showed that event-loop timers and user events can be
     starved once the window enters platform GL rendering, so smoke runs now
     terminate from a background watchdog thread after the requested interval.
     The verified probe used `WGPU_BACKEND=gl`, reported
     `native_backend_policy=gl`, `opengl_only=true`, and
     `vulkan_enabled_by_witty=false`, then exited with status 0 after the
     watchdog fired. This change only affects explicit smoke runs; normal daily
     windows still use the regular event loop and do not start Chromium,
     browser/WebGPU, or Vulkan paths.
362. `m672-native-window-first-frame-report`
   - done. Extended `--window-startup-report` to emit a second JSON line,
     `witty.native_window_first_frame`, after the first native redraw
     succeeds. The first line still proves startup policy before renderer
     initialization; the second line now proves that `render()` returned and
     includes basic frame stats such as visible rows/cols, glyph runs, glyph
     chars, rect vertices, cursor visibility, and damage state. This gives
     OpenGL-only local probes a clear distinction between renderer startup and
     actual first-frame completion without enabling Chromium, browser/WebGPU, or
     Vulkan paths.
363. `m674-native-opengl-daily-launch-script`
   - done. Added `scripts/run-witty-native-opengl.sh` as the preferred repo-local
     daily launch entry point for Linux/M1000 native development. The script
     exports `WGPU_BACKEND=gl`, runs `target/debug/witty --window` when the
     debug binary exists, falls back to `cargo run -p witty-app -- --window`, and
     accepts normal window flags such as `--window-startup-report`,
     `--window-exit-after-ms`, font overrides, cwd, env, and program args. It
     also supports `WITTY_NATIVE_BIN=/path/to/witty` for testing an
     installed binary while keeping the launcher-level OpenGL policy visible,
     plus `--print-command` for a no-GUI dry run of the selected binary or cargo
     fallback.
364. `m675-native-first-frame-font-report`
   - done. Added active font metadata to the
     `witty.native_window_first_frame` JSON line emitted by
     `--window-startup-report` after the first successful native redraw. Bounded
     OpenGL-only probes now report `font_family`, `font_size`, and
     `font_source_count` both before renderer initialization and at first-frame
     completion, which makes Nerd Font and font-file launch diagnostics directly
     visible without opening Chromium, browser/WebGPU, or Vulkan paths.
365. `m676-native-font-list-cli`
   - done. Added `witty --font-list` plus `--font-list-filter <text>` as a
     non-GUI daily setup helper for discovering exact installed font family
     names. The command loads the renderer font database, prints sorted unique
     family names one per line, and supports case-insensitive filtering such as
     `--font-list-filter nerd` before copying a family into `--font-family`,
     `WITTY_FONT_FAMILY`, or `window.v1.json`. It does not open a native
     window, create a wgpu surface, request an adapter, start a PTY, start
     Chromium, or touch browser/WebGPU/Vulkan paths.
366. `m678-native-template-nerd-font-mono-default`
   - done, superseded by the Witty migration `.wittyrc` default. The native
     daily config template and README examples now prefer `Maple Mono NF CN`.
     This is a template/documentation default only; explicit `--font-family`,
     `WITTY_FONT_FAMILY`, `.wittyrc`, and existing `window.v1.json` values
     keep their documented precedence.
367. `m680-native-opengl-script-helper-modes`
   - done. Extended `scripts/run-witty-native-opengl.sh` so the repo-local
     OpenGL launcher can also run daily non-graphical helpers without appending
     `--window`. The script now forwards `--font-list [filter]`,
     `.wittyrc` helpers, including `--wittyrc-template`,
     `--wittyrc-default-path`, `--wittyrc-init`, `--wittyrc-check`, and
     `--wittyrc-effective`,
     `--window-config-template`, `--window-config-default-path`,
     `--window-config-init`, `--window-config-check`,
     `--window-config-effective`, `--renderer-backend-info`, and
     `--renderer-no-surface-diagnostics` through the same binary selection and
     `WGPU_BACKEND=gl` boundary used by native window launches. Dry-run and
     real helper verification covered font listing, effective config reporting,
     custom `WITTY_NATIVE_BIN`, and default window command generation while
     avoiding native window, PTY, surface, adapter, Chromium, browser/WebGPU,
     and Vulkan paths for helper modes.
368. `m682-native-local-new-tab-shortcut`
   - done. Added a native-window `Ctrl+Shift+T` local new-tab path for daily
     use. The window stores the resolved launch template from `--program`,
     `--arg`, `--cwd`, `--env`, and config defaults, then opens a new local PTY
     with the current grid size, records host-owned `witty-local` tab
     metadata, parks the previous active runtime, and switches to the new tab.
     Focused tests verify the untracked initial local shell is safely added to
     the tab registry, the previous terminal/search/shell-integration state is
     parked, spawn failure leaves active state untouched, and the shortcut is
     recognized without touching browser/WebGPU/Vulkan paths.
369. `m684-wittyrc-developer-config`
   - done. Added `$HOME/.wittyrc` as Witty's preferred developer config file
     with TOML parsing for `font-family = "Maple Mono NF CN"`, a bundled
     repo-controlled template, and non-GUI helpers for template printing,
     default path discovery, init, check, and effective summaries. Native
     launches apply `.wittyrc` before the compatible `window.v1.json` layer,
     while CLI flags and `WITTY_FONT_FAMILY` / `WITTY_FONT_PATHS` remain the
     highest-precedence font overrides. The helpers do not open a window,
     create a surface, request an adapter, start a PTY, or touch
     browser/WebGPU/Vulkan paths.
370. `m686-terminal-kitty-keyboard-protocol-flags`
   - done. Corrected Witty's Kitty keyboard protocol subset so flag `1`
     (`DISAMBIGUATE_ESC_CODES`) keeps `Enter`, `Tab`, and `Backspace` on legacy
     byte sequences, while flag `8` (`REPORT_ALL_KEYS_AS_ESC_CODES`) adds CSI-u
     reporting for text-producing keys and those legacy C0 keys. `witty-core`
     now tracks both supported flags in query/push/set/pop state, and native
     plus browser key encoders share the same flag boundary. Named browser keys
     are guarded against accidental first-character CSI-u encoding. See
     `terminal-kitty-keyboard-protocol.md`.
371. `m688-terminal-kitty-associated-text`
   - done. Added Kitty keyboard protocol flag `16`
     (`REPORT_ASSOCIATED_TEXT`) to the core flag state and native/browser input
     encoders. When flags `8|16` are active, text-producing character keys now
     include safe associated text codepoints as the third CSI-u parameter, while
     `Ctrl`/`Meta` combinations and text containing C0, DEL, or C1 control
     codepoints omit that field. This keeps associated text tied to all-keys
     CSI-u mode and avoids adding release/repeat event-type semantics before
     the platform event path is ready. See `terminal-kitty-keyboard-protocol.md`.
372. `m690-terminal-kitty-event-types`
   - done. Added Kitty keyboard protocol flag `2` (`REPORT_EVENT_TYPES`) to
     core flag state and native/browser input encoders. CSI-u keys now include
     `:1`, `:2`, or `:3` in the modifier parameter for press, repeat, and
     release events when the flag is active. `Enter`, `Tab`, and `Backspace`
     release reporting stays gated by flag `8`, matching the existing boundary
     where those keys remain legacy under flag `1` alone. Browser input now
     forwards keydown repeat and keyup metadata into the shared encoder. See
     `terminal-kitty-keyboard-protocol.md`.
373. `m692-terminal-kitty-alternate-keys`
   - done. Added Kitty keyboard protocol flag `4`
     (`REPORT_ALTERNATE_KEYS`) to the core flag state and native/browser input
     encoders. Character keys that are already emitted as CSI-u can now include
     shifted-key and physical US-layout base-key sub-fields, such as
     `Shift-A` -> `CSI 97:65;2u`, `Shift-=` producing `+` ->
     `CSI 61:43;2u`, and non-US logical keys with browser/native physical-key
     metadata -> `CSI primary::base u`. Navigation, function, and keypad keys
     remain on the existing xterm/VT encoder path. See
     `terminal-kitty-keyboard-protocol.md`.
374. `m694-terminal-kitty-modifier-key-codes`
   - done. Added Kitty all-keys CSI-u reporting for physical modifier-key
     events in native and browser encoders. Witty now maps left/right
     Shift/Ctrl/Alt/Super from native `KeyLocation` / physical `KeyCode` and
     browser `KeyboardEvent.location` / `KeyboardEvent.code` into Kitty PUA
     functional key codes, including release event typing when flag `2` is
     active. Generic modifier events without side metadata remain unreported
     rather than being aliased to a left/right key. See
     `terminal-kitty-keyboard-protocol.md`.
375. `m696-terminal-kitty-keypad-codes`
   - done. Added Kitty keypad functional key reporting in native and browser
     encoders. When flags `1` or `8` are active, Witty maps detected numpad
     input to Kitty `KP_*` PUA codes, such as `KP_1` -> `CSI 57400u` and
     `KP_ENTER` -> `CSI 57414u`, while top-row digits and legacy/application
     keypad behavior stay unchanged outside Kitty protocol mode. Associated
     text and event-type sub-fields compose with the existing flags `16` and
     `2`. See `terminal-kitty-keyboard-protocol.md`.
376. `m698-terminal-kitty-functional-key-codes`
   - done. Added Kitty functional-key event-type reporting for native and
     browser navigation/function-key encoders. When flags `1|2` or `8|2` are
     active, keys such as `ArrowUp` and `F5` include Kitty press/repeat/release
     sub-fields in their functional escape forms. Witty also reports Kitty PUA
     functional key codes under flags `1` or `8` for keys without legacy xterm
     sequences, including `F13`-`F35`, lock keys, `PrintScreen`, `Pause`,
     `ContextMenu`, common media keys, volume keys, and `AltGraph`. Legacy
     xterm/VT navigation and function-key behavior remains unchanged when
     Kitty keyboard mode is inactive; Meta-modified functional keys use Kitty
     functional forms when Kitty mode is active because the xterm fallback has
     no Meta modifier parameter.
377. `m700-terminal-kitty-hyper-meta-modifiers`
   - done. Extended Kitty modifier-key reporting for native and browser input.
     Native input now reports sided `Hyper` and sided `NamedKey::Meta` when
     `winit` supplies left/right `KeyLocation`, using Kitty modifier bits
     `Hyper=16` and `Meta=32`. Browser input reports sided `Hyper` from
     `KeyboardEvent.code`/`location`; browser `Meta` continues to map to Kitty
     `Super`, preserving DOM semantics for Windows/Command keys. Generic
     modifier events without side metadata remain unreported.
378. `m702-terminal-kitty-keypad-navigation-codes`
   - done. Added Kitty keypad navigation reporting for native and browser
     input. When flags `1` or `8` are active and the platform identifies a
     numpad source, NumLock-off navigation keys now map to Kitty `KP_LEFT`
     through `KP_BEGIN` codes, such as `KP_LEFT` -> `CSI 57417u`. Legacy xterm
     navigation remains unchanged when Kitty keyboard mode is inactive.
379. `m704-nvim-kitty-keyboard-real-tui-smoke`
   - done. Added `witty --real-tui-smoke nvim-kitty-keyboard` as a real
     Neovim compatibility check for the Kitty keyboard protocol. The smoke
     waits for Neovim to enable Kitty flags through real PTY output, sends
     `Ctrl-I` through Witty's shared key encoder using current terminal input
     modes, verifies `CSI 105;5:1u` is emitted while event reporting is active,
     and confirms Neovim runs the `<C-I>` mapping instead of `<Tab>`. The
     keyboard protocol diagnostic tool remains a planned follow-up. See
     `real-tui-smoke-harness.md` and `terminal-kitty-keyboard-protocol.md`.

## Non-Goals For This Line

- Full SFTP, tunnels, host inventory, vault, sync, and team features.
- Plain URL autodetection; OSC 8 is complete enough for now.
- Clipboard synchronization between local and remote systems.
- Reading the local clipboard through OSC 52 query.
- Bell sound, visual flash, notification, and throttling policy.
- Full legacy emulation (`VT320`, `Wyse`, `TN3270/TN5250`).

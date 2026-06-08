# Terminal Hyperlink Plan

Updated: 2026-05-30

m131 scopes OSC 8 hyperlink support for Witty. The goal is to add modern
terminal hyperlinks without turning terminal output into plugin-visible or
automatically opened URLs.

## Current Code Shape

- OSC parsing already enters `BasicTerminalState::osc_dispatch()`.
- OSC `0` and `2` titles are parsed by `osc_window_title()` and stored in
  `RenderSnapshot.title`.
- `BasicCell` currently stores text, width, and `CellStyle`.
- `RenderCell` mirrors text, width, and style into renderer-facing snapshots.
- `FramePlanner` batches backgrounds and glyphs by row/style, then adds dynamic
  overlays for search, selection, and cursor.
- Native and browser mouse paths already map pixel positions to `CellPoint`.
- Native URL opening code exists only in `witty-launcher` for `--open-browser`;
  hyperlink opening should be factored into a shared, policy-checked helper
  before native window mode reuses it.

## OSC 8 Target

Parse:

```text
OSC 8 ; params ; uri ST
OSC 8 ; params ; uri BEL
OSC 8 ; ; ST
OSC 8 ; ; BEL
```

Rules:

- Non-empty `uri` starts or replaces the active hyperlink.
- Empty `uri` ends the active hyperlink.
- Preserve the raw visible terminal text. Hyperlinks are metadata attached to
  cells written while a hyperlink is active.
- Support `id=<value>` in the params field as an optional grouping hint.
- Treat semicolons after the URI field as part of the URI, matching the current
  title parser's rejoin behavior.
- Decode with UTF-8 loss replacement, reject control characters, cap URI and
  id lengths, and do not perform percent-decoding in the terminal core.

## Data Model

Add core snapshot types:

```rust
pub type HyperlinkId = u32;

pub struct TerminalHyperlink {
    pub id: HyperlinkId,
    pub uri: String,
    pub osc8_id: Option<String>,
}
```

Add:

- `BasicTerminalState::active_hyperlink: Option<HyperlinkId>`.
- `BasicTerminalState::hyperlinks: Vec<TerminalHyperlink>`.
- `BasicCell::hyperlink: Option<HyperlinkId>`.
- `RenderCell::hyperlink: Option<HyperlinkId>`.
- `RenderSnapshot::hyperlinks: Vec<TerminalHyperlink>`.

Keep hyperlink ids session-local and deterministic. Do not use the URI itself
as the cell payload, because renderer and hit-testing only need a compact id.

## Buffer Semantics

- Printable cells inherit `active_hyperlink`.
- Wide-cell continuations carry the same hyperlink id as their base cell.
- Combining marks stay attached to the base cell and inherit the base link.
- Erase, insert, delete, resize, and wide-cell repair must not leave orphan link
  ids on blank or continuation cells.
- Scrollback should preserve link ids and keep the snapshot hyperlink table
  reachable for visible scrollback rows.
- Alternate screen should keep hyperlink metadata isolated with its buffer.
- Full reset clears active hyperlink state and stored hyperlink tables.

## Rendering

First visual pass:

- Render linked text with the original foreground color.
- Add a subtle underline overlay for linked cell spans.
- Do not force blue text; terminal applications may already set colors.
- Keep hyperlink visuals in renderer overlay batches so glyph runs do not split
  for link metadata alone.

Hover pass:

- Track `hovered_hyperlink: Option<HyperlinkId>` in native/browser UI state.
- Add hover underline/background as a dynamic overlay so row caches can be
  reused when the pointer moves.
- Expose hyperlink overlay counts in `FrameStats`.

## Interaction Policy

Default activation should require an explicit modifier:

- Native: `Ctrl+LeftClick` on Linux/Windows and `Cmd+LeftClick` on macOS.
- Browser: `Ctrl+LeftClick` or `Meta+LeftClick`.

Reasons:

- Plain left drag must remain local selection.
- Terminal mouse-reporting applications must not lose ordinary mouse events.
- Opening external URLs should be a deliberate user action.

If mouse reporting is active and the modifier is not pressed, preserve existing
mouse reporting behavior. If the activation modifier is pressed on a hyperlink,
open the hyperlink and do not send terminal input bytes.

Opening policy:

- Allow `http`, `https`, and `mailto` initially.
- Reject empty, control-containing, and oversized URIs.
- Defer `file`, custom schemes, and workspace-relative links until there is a
  profile-level allowlist.
- Native opening should reuse a shared opener helper factored from
  `witty-launcher`.
- Browser opening should use `window.open(uri, "_blank", "noopener,noreferrer")`
  and report a blocked popup distinctly from an unsupported scheme.

## Plugin And Privacy Boundary

Hyperlink URIs are terminal output content. They should follow the same privacy
model as terminal text:

- Do not emit URI-bearing plugin events by default.
- Do not add hyperlinks to command arguments.
- Do not expose hyperlink URI reads unless a future API checks
  `TerminalReadPermission`.
- Opening a hyperlink is a local user action, not a plugin action.

Renderer snapshots may carry hyperlinks internally because the renderer is part
of the trusted host path.

## Smoke And Tests

Core tests:

- OSC 8 start/end attaches a URI to printed cells.
- Empty URI closes the active hyperlink.
- URI semicolons are preserved.
- Erase/delete/resize do not leave stale hyperlink ids on blank cells.
- Alternate screen and scrollback keep hyperlink metadata in the right buffer.
- Full reset clears active hyperlinks.

Renderer tests:

- Linked cells produce underline rectangles.
- Hover overlay changes do not rebuild retained terminal rows.
- Wide linked cells underline their full span once.

Native/browser tests:

- Modifier-click on a linked cell opens the sanitized URI.
- Plain click/drag still selects text when mouse reporting is inactive.
- Mouse reporting still receives ordinary clicks when activation modifiers are
  absent.
- Browser smoke stubs `window.open` and verifies the URI is opened without
  sending gateway input bytes.

## Follow-Up Tasks

1. `m132-terminal-osc8-core`: done. Implemented core OSC 8 parsing, cell
   metadata, visible snapshot hyperlink tables, and focused core tests. See
   `terminal-osc8-core.md`.
2. `m133-hyperlink-render-overlays`: done. Rendered hyperlink underlines and
   hover overlays in `FramePlanner`/retained planner with focused renderer
   tests. See `terminal-hyperlink-render-overlays.md`.
3. `m134-native-hyperlink-activation`: done. Added native snapshot hit-testing,
   hover state, modifier-click opening, and shared external URL opener policy.
   See `terminal-native-hyperlink-activation.md`.
4. `m135-browser-hyperlink-activation-smoke`: done. Added browser hit-testing,
   hover state, `window.open` policy handling, and Playwright smoke coverage.
   See `terminal-browser-hyperlink-activation.md`.
5. OSC 8 hyperlink support is now complete enough for the current Witty line. Defer
   plain URL auto-detection until a later compatibility pass.

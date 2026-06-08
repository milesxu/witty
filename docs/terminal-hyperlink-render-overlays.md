# Terminal Hyperlink Render Overlays

Updated: 2026-05-30

m133 adds the renderer-side visual pass for OSC 8 hyperlinks. It deliberately
stops before hit-testing, URL policy, and click activation.

## What Changed

- Added `RenderSnapshot::hovered_hyperlink` for UI-owned hover state.
- Added `FramePlan::hyperlink_underlines` and `FramePlan::hyperlink_hover`.
- Added hyperlink overlay counts to `FrameStats`.
- Planned underline rectangles from `RenderCell::hyperlink` spans.
- Planned hover background rectangles only for the hovered hyperlink id.
- Included hyperlink overlay rectangles in the wgpu rectangle vertex stream.

## Rendering Contract

Hyperlink text keeps its terminal-specified foreground color. The renderer adds
subtle underline rectangles under linked cell spans instead of forcing browser
style blue text.

Adjacent cells on the same row with the same `HyperlinkId` are coalesced into a
single underline rectangle. Wide cells contribute their full terminal cell
width. Hover overlays are grouped with the same span logic and are only emitted
when `RenderSnapshot::hovered_hyperlink` matches a visible linked span.

## Retained Row Cache

Hyperlink underlines and hover backgrounds are dynamic overlays, like search
highlights, selection, and the cursor. They are not stored in the retained row
cache. Pointer hover changes can therefore reuse cached row background and glyph
plans while still producing a fresh hover overlay.

## Out Of Scope

- Mapping mouse pixels to hyperlink ids.
- Opening external URLs.
- Scheme allowlist or popup-blocker handling.
- Plugin-facing hyperlink APIs or URI-bearing events.

Those stay in the native and browser activation follow-up tasks.

## Verification

- `cargo fmt --all -- --check`
- `cargo test -p witty-render-wgpu hyperlink --quiet`
- `cargo test -p witty-render-wgpu --quiet`
- `cargo test -p witty-core --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `cargo test --workspace --quiet`
- `cargo clippy --workspace --all-targets -- -D warnings`

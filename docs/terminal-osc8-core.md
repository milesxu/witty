# Terminal OSC 8 Core

Updated: 2026-05-30

m132 implements the core, non-visual OSC 8 hyperlink model. Rendering,
hovering, click activation, and URL opening remain follow-up work.

## What Changed

- Added `HyperlinkId` and `TerminalHyperlink` to `witty-core`.
- Added optional hyperlink ids to `RenderCell`.
- Added `RenderSnapshot::hyperlinks`, filtered to hyperlinks referenced by the
  currently visible rows.
- Added `BasicTerminalState` active hyperlink tracking.
- Added OSC 8 parsing for BEL-terminated and ST-terminated sequences.
- Linked printable cells inherit the active hyperlink id.
- Empty OSC 8 URI closes the active hyperlink.

## Parsing Boundary

Supported forms:

```text
OSC 8 ; params ; uri ST
OSC 8 ; params ; uri BEL
OSC 8 ; ; ST
OSC 8 ; ; BEL
```

The core:

- accepts `id=<value>` in the OSC 8 params field.
- preserves semicolons inside the URI field.
- decodes with UTF-8 loss replacement.
- rejects control-containing or oversized URI strings by closing the active
  hyperlink instead of extending stale metadata.
- does not validate URL schemes, percent-decode, open links, or expose links to
  plugins.

## Buffer Semantics

- Wide-cell continuations carry the same hyperlink id as their base cell.
- Erase operations clear visible hyperlink metadata from blank cells.
- Scrollback keeps hyperlink metadata when linked rows become visible again.
- Alternate screen snapshots only expose links attached to alternate-screen
  cells.
- Full reset clears active hyperlinks and stored hyperlink metadata.

## Verification

- `cargo fmt --all -- --check`
- `cargo test -p witty-core osc8 --quiet`
- `cargo test -p witty-core --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `cargo test --workspace --quiet`
- `cargo clippy --workspace --all-targets -- -D warnings`

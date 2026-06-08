# Terminal Grapheme Cluster Spans

Updated: 2026-05-30

m126 implements the first half of the Unicode plan from
`terminal-grapheme-normalization-plan.md`: search and selection geometry now
expand through extended grapheme clusters while preserving the original PTY text
in terminal cells.

## What Changed

- Added `unicode-segmentation` 1.13.2 to `witty-core`.
- Added internal `TextClusterSpan` construction over `SearchTextRow::text`.
- Expanded search match column mapping to whole grapheme clusters before
  converting character ranges to terminal cell ranges.
- Reworked selected-row text extraction so selecting any cell covered by a
  multi-cell grapheme cluster copies the full original cluster once.
- Added fixtures for emoji ZWJ, regional indicator, emoji modifier, and
  wide-cell/combining-mark regression paths.

## Boundary

- Terminal buffers still preserve original PTY text.
- `unicode-width` remains the terminal cell-width source.
- Regex still runs over original text.
- NFC/canonical-equivalence search remains deferred to m127.

## Verification

- `cargo fmt --all -- --check`
- `cargo test -p witty-core search --quiet`
- `cargo test -p witty-core --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `cargo test --workspace --quiet`
- `cargo clippy --workspace --all-targets -- -D warnings`

# Terminal Grapheme Normalization Plan

Updated: 2026-05-30

m125 defines the Unicode text boundary plan after m123 and m124 added
width-aware terminal cells. The next hard problem is not just "more width
rules"; it is deciding which user-visible text unit owns search, selection,
word picking, and plugin-safe text export.

## References

- `unicode-segmentation` 1.13.2 docs.rs:
  <https://docs.rs/unicode-segmentation/latest/unicode_segmentation/>
- `unicode-normalization` 0.1.25 docs.rs:
  <https://docs.rs/unicode-normalization/latest/unicode_normalization/>
- Unicode Standard Annex #29, Unicode Text Segmentation:
  <https://unicode.org/reports/tr29/>
- Unicode Standard Annex #15, Unicode Normalization Forms:
  <https://unicode.org/reports/tr15/>

## Current Boundary

Witty currently stores terminal cells as:

| Layer | Current model | Consequence |
| --- | --- | --- |
| `BasicCell` | one visible base cell plus optional zero-width marks in `text`; wide continuations are `width == 0` cells | good enough for m123/m124 width invariants |
| searchable row | `SearchTextRow::text` plus optional one-entry-per-`char` `SearchTextColumn` spans | match geometry is char-indexed, not grapheme-indexed |
| selection | selected cell spans are copied by visible cell intersection | wide cells copy once; grapheme clusters are not normalized |
| word picking | terminal word policy is char-based with zero-width mark tolerance | combining marks stay attached, but UAX #29 word boundaries are not used |
| renderer | glyph text still receives a cell text string and cell width | rendering can accept richer clusters if the terminal model supplies them |

This is a correct intermediate state for wide CJK and simple combining marks.
It is not sufficient for emoji ZWJ sequences, regional indicator flags,
skin-tone modifiers, canonical-equivalence search, or language-tailored word
navigation.

## Library Evaluation

`unicode-segmentation`

- Provides grapheme, word, and sentence boundary iterators based on UAX #29.
- Has `no_std` support, so it is compatible with shared core/wasm crates.
- Provides byte-offset iterators, which is important because the terminal model
  should map text spans back to original cells without lossy char math.
- Recommended use: extended grapheme clusters for visible text segmentation.
- Not enough by itself: it does not normalize text and should not decide
  terminal cell width alone.

`unicode-normalization`

- Provides composition/decomposition utilities based on UAX #15.
- Supports NFC/NFD/NFKC/NFKD and quick checks.
- Recommended use: optional search-key construction and tests for canonical
  equivalence, not mutation of stored terminal text.
- Not enough by itself: normalization can change byte/char offsets, so any
  normalized search path must carry a reversible mapping to original clusters.

`unicode-width`

- Already in `witty-core`.
- Keep it as the terminal cell-width source for now.
- Do not replace it with grapheme segmentation; a grapheme cluster can span
  multiple codepoints but still needs a terminal column width policy.

## Product Decisions

| Question | Decision |
| --- | --- |
| stored terminal text | preserve original PTY text; do not normalize cells in-place |
| search matching | add a normalized search key only when the option is enabled or when the literal matcher can do so without surprising byte/column mapping |
| selection copy | copy original terminal text, not normalized text |
| regex | keep regex operating on original text until a separate normalized-regex design exists |
| whole-word search | keep current terminal word policy for m126; evaluate UAX #29 words later |
| emoji ZWJ | do not special-case by hand; add fixtures after grapheme cluster spans exist |
| plugin reads | expose original text by default; normalized export must be explicit and permission-gated |

The central rule: normalization may create a search/index view, but the
terminal buffer remains a faithful representation of bytes emitted by the PTY.

## Proposed Data Model

Add an internal text-span layer in `witty-core`:

```rust
struct TextCellCluster {
    text: String,
    start_col: u16,
    end_col: u16,
    char_start: usize,
    char_end_exclusive: usize,
    byte_start: usize,
    byte_end: usize,
}
```

Then build searchable rows from clusters instead of raw chars:

```rust
struct SearchTextSegment {
    text: String,
    normalized_nfc: Option<String>,
    start_col: u16,
    end_col: u16,
    char_start: usize,
    char_end_exclusive: usize,
}
```

Implementation notes:

- A cluster can include base character, combining marks, variation selectors,
  emoji modifiers, ZWJ sequences, regional indicator pairs, and CRLF if such
  input appears in one searchable row.
- Cluster width should be derived from the cells it covers, not recomputed from
  normalized text.
- Search result mapping should become cluster-span based. Character mapping can
  remain a fast path for ASCII/default rows.
- Store byte offsets during segmentation so normalized search can map back to
  original clusters without guessing.

## Search Algorithm Plan

### Literal Search

1. Build clusters for each searchable row.
2. Build the original row text from clusters.
3. For default literal search, match original text as today.
4. For normalized search, build an NFC key from cluster text and keep a vector:

```text
normalized byte range -> original cluster range
```

5. Convert matched normalized ranges back to cluster ranges, then to cell
   ranges.

Initial normalized matching should be limited to literal search. Regex over a
normalized projection needs a separate design because regex captures and byte
offsets become hard to explain.

### Whole Word

Keep the current terminal-oriented word policy in the first implementation:

- shell path characters remain word characters.
- combining marks attached to a word base are accepted.
- CJK wide letters remain word cells when `char::is_alphanumeric()` says so.

UAX #29 word boundaries can be evaluated later for natural-language search, but
they are not a drop-in replacement for terminal shell/path word selection.

### Case Folding

Current case-insensitive matching uses `char::to_lowercase()`. That is not full
Unicode case folding. Do not broaden scope in m126. If this becomes important,
add a dedicated `unicode-casefold` or ICU4X evaluation rather than pretending
lowercase is full fold equivalence.

## Selection And Editing Plan

Selection should continue to be cell-span based and copy original text.
Grapheme clusters matter when:

- selecting either part of a grapheme's cell span should copy the full cluster.
- double-click word selection should not split a grapheme cluster.
- cursor movement by character is added later; it should move by grapheme
  cluster, not scalar value.

Editing should remain cell-operation based. Terminal escape operations such as
ICH, DCH, ECH, EL, ED, and resize should preserve visible-cell invariants. They
should not try to normalize or segment text; they only need the repair logic
from m124 plus tests for grapheme-bearing cells.

## Test Fixtures

Add these fixtures before claiming grapheme parity:

| Fixture | Purpose |
| --- | --- |
| `e\u{0301}` vs `\u{00e9}` | canonical equivalence for literal search |
| `a\u{0308}\u{0301}` | multiple combining marks in one cluster |
| `👩\u{200d}💻` | emoji ZWJ sequence as one extended grapheme |
| `👍🏽` | emoji modifier sequence |
| `🇺🇸` | regional indicator pair |
| `✈\u{fe0f}` | variation selector |
| Devanagari consonant/virama sequence | non-Latin cluster boundary |
| CJK plus combining mark edge case | width plus mark mapping |

For each fixture cover:

- `search_text_rows()` text and cell spans.
- literal search highlight range.
- selected text when the range touches any covered cell.
- word range if the base script should be word-like.
- resize or editing behavior when the cluster is at the right edge.

## Implementation Milestones

### m126 Grapheme Cluster Text Spans

Status: done.

Write scope:

- `Cargo.toml`
- `crates/witty-core/Cargo.toml`
- `crates/witty-core/src/lib.rs`
- `crates/witty-core/src/basic_terminal.rs`
- focused tests

Deliverables:

- add `unicode-segmentation`.
- build internal cluster spans from searchable rows.
- map search matches through cluster spans for ZWJ, variation-selector,
  modifier, and regional-indicator fixtures.
- preserve current ASCII and wide-cell behavior.

Completion note:

- Implemented in `/home/mingxu/src/witty/docs/terminal-grapheme-cluster-spans.md`.
- Search match mapping and selected text extraction now expand through extended
  grapheme clusters while preserving original PTY text.

Verification:

- `cargo fmt --all -- --check`
- `cargo test -p witty-core search`
- `cargo test -p witty-core`
- `cargo test --workspace`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `cargo clippy --workspace --all-targets -- -D warnings`

### m127 Literal NFC Search Option

Status: done.

Write scope:

- `witty-core`
- `witty-ui` only if exposing a user option is selected
- native/browser status labels if exposed

Deliverables:

- add `unicode-normalization`.
- add normalized literal-search projection with reversible mapping to original
  clusters.
- cover `e\u{0301}` matching `\u{00e9}` and the reverse.
- keep regex on original text.

Completion note:

- Implemented in `/home/mingxu/src/witty/docs/terminal-nfc-search-option.md`.
- Added `SearchOptions::normalize_nfc` with default `false`; regex ignores the
  option and still uses original text.

Decision point:

- The option can be implicit for literal search only, or explicit as
  `normalize_nfc`. Choose after measuring whether implicit matching surprises
  shell/path workflows.

### m128 Unicode Word Boundary Evaluation

Status: done.

Deliverables:

- compare current terminal word policy with UAX #29 word boundaries.
- decide whether to add a "natural word" mode separate from terminal word mode.
- avoid regressing path and command-token double click behavior.

Completion note:

- Implemented in `/home/mingxu/src/witty/docs/terminal-word-boundary-evaluation.md`.
- Decision: keep terminal token policy as the default and defer UAX #29 natural
  word mode as an optional future search mode.

### m129 Search Normalize NFC UI Toggle

Status: done.

Deliverables:

- expose `SearchOptions::normalize_nfc` in native and browser search UI.
- keep the option default-off and visible in search status labels.
- preserve regex original-text behavior and plugin privacy boundaries.

Completion note:

- Implemented in `/home/mingxu/src/witty/docs/terminal-nfc-search-ui-toggle.md`.
- Native and browser search UIs use `Alt+N`; status labels report `raw` or
  `nfc`.

## Risks

| Risk | Mitigation |
| --- | --- |
| normalized search loses original offsets | keep normalized-range to cluster-range mapping as a first-class artifact |
| regex over normalized text is ambiguous | defer normalized regex until a separate design exists |
| grapheme width differs from terminal emulator reality | use cell spans as source of truth, not normalized/grapheme text width |
| word boundaries conflict with shell paths | keep terminal word policy; treat UAX #29 word mode as optional future work |
| dependency drift | pin workspace dependencies and cover wasm target checks |

## Recommendation

Proceed with m126 first. It is a structural improvement that does not change
visible product semantics except making existing search/selection geometry
cluster-aware. Defer normalized matching to m127 so canonical equivalence does
not complicate the cluster-span refactor.

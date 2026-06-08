# Terminal NFC Search Option

Updated: 2026-05-30

m127 adds an opt-in NFC projection for literal search. It lets canonically
equivalent text forms match without changing the terminal buffer or regex
semantics.

## What Changed

- Added `unicode-normalization` to `witty-core`.
- Added `SearchOptions::normalize_nfc`, defaulting to `false`.
- Normalized literal search now builds an NFC projection per grapheme cluster
  and maps normalized match offsets back to original cluster character ranges.
- Regex search ignores `normalize_nfc` and continues to run on original text.

## Behavior

- Default literal search remains byte/scalar-form sensitive.
- With `normalize_nfc: true`, `e\u{0301}` can match `\u{00e9}` and the reverse.
- Search highlights still use original terminal cell spans.
- Stored terminal text and selected text remain original PTY text.

## UI Exposure

m129 exposes this option in native and browser search UI with `Alt+N`.
The status label reports `raw` when disabled and `nfc` when enabled.

## Deferred

- No normalized regex mode.
- No full Unicode case folding beyond the existing lowercase comparison.

## Verification

- `cargo fmt --all -- --check`
- `cargo test -p witty-core search --quiet`
- `cargo test -p witty-core --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `cargo test --workspace --quiet`
- `cargo clippy --workspace --all-targets -- -D warnings`

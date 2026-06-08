# Terminal Word Boundary Evaluation

Updated: 2026-05-30

m128 compares Witty's current terminal-oriented word policy with Unicode
UAX #29 word segmentation. The conclusion is that Unicode word boundaries
should not replace the default terminal behavior. They can be added later as a
separate natural-language search mode.

## Current Behavior

Witty currently uses a terminal token policy in two places:

- `BasicTerminal::word_range_at()` for double-click selection.
- `SearchOptions::whole_word` for find filtering.

The shared character class is effectively:

```text
alphanumeric plus _ - . / \ : @ ~ + = % $
```

This is deliberate. Common terminal tokens include:

- file paths: `src/main.rs`, `/var/log/syslog`
- flags and options: `--target=wasm32-unknown-unknown`
- environment variables: `$TERM`, `RUST_LOG=debug`
- URLs and remote targets: `ssh://user@host:22/path`
- package or symbol fragments: `witty-core::SearchOptions`

## UAX #29 Fit

`unicode-segmentation` already provides UAX #29 word iterators. They are useful
for natural-language text, but they do not model shell tokens.

Expected differences:

| Text | Terminal Token Policy | UAX #29 Natural Words |
| --- | --- | --- |
| `src/main.rs` | one token | `src`, `main`, `rs` |
| `--target=wasm32-unknown-unknown` | one token | likely split around punctuation |
| `user@example.com` | one token | likely split around punctuation |
| `hello,world` | two tokens | two words |
| `中文单词` | one alphanumeric run by current policy | natural segmentation depends on script rules |
| `e\u{0301}cho` | one token; combining mark stays attached | one natural word |

The natural-language behavior is better for prose search, but worse for the
terminal's most common selection workflow: copying paths, command arguments,
URLs, symbols, and environment assignments.

## Product Decision

- Keep terminal token policy as the default for double-click selection.
- Keep terminal token policy as the default for whole-word search.
- Do not replace `word_range_at()` with UAX #29.
- If natural-language matching is added, expose it as a separate search word
  mode, not as a hidden behavior change.

Recommended future API:

```rust
pub enum SearchWordMode {
    TerminalToken,
    UnicodeWord,
}

pub struct SearchOptions {
    pub case_sensitive: bool,
    pub regex: bool,
    pub whole_word: bool,
    pub normalize_nfc: bool,
    pub word_mode: SearchWordMode,
}
```

Default should remain `TerminalToken`.

## Implementation Notes

If `UnicodeWord` is added later:

- Keep regex and literal search on original text unless an explicit normalized
  projection is selected.
- Use `unicode-segmentation` word boundaries only for boundary filtering, not
  for terminal cell width or selection geometry.
- Map accepted match ranges through the existing grapheme cluster and
  `SearchTextColumn` machinery.
- Add UI copy that distinguishes "token" from "word" without exposing Unicode
  jargon.

## Test Matrix

Before shipping `UnicodeWord`, add fixtures for:

| Case | TerminalToken | UnicodeWord |
| --- | --- | --- |
| `src/main.rs`, query `main` whole-word | no match | match |
| `src/main.rs`, query `src/main.rs` whole-word | match | no match or exact mode only |
| `--target=wasm32-unknown-unknown`, query full flag | match | no match or exact mode only |
| `hello,world`, query `world` | match | match |
| `e\u{0301}cho`, query `echo` with NFC option | match only if normalized projection is enabled | same |
| emoji / grapheme clusters | no partial-cell highlight | no partial-cell highlight |

## Recommendation

Do not implement UAX #29 word mode immediately. The next higher-value task is
to expose the already implemented NFC literal-search option in native and
browser search UI, because that completes the m127 capability without changing
default terminal token behavior.

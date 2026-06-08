# Terminal NFC Search UI Toggle

Updated: 2026-05-30

m129 exposes the existing literal-search NFC projection through native and
browser find UI without changing the default terminal search behavior.

## What Changed

- Native find bar accepts `Alt+N` while search is open.
- Browser find UI accepts `Alt+N` while search is open.
- Search status labels now include the normalization mode:
  - `raw` means literal search uses original terminal text.
  - `nfc` means literal search uses the opt-in NFC projection.
- Browser search state now exposes `normalizeNfc` for smoke tests and UI state
  inspection.

## Behavior

- `normalize_nfc` remains off by default.
- The toggle rebuilds matches immediately and scrolls to the active match when
  needed.
- Regex search still ignores NFC normalization and runs against original text.
- Selected text, terminal buffer contents, and plugin-visible command events
  stay in original PTY text form.

## Privacy Boundary

This is a local UI option on `TerminalSearch`. It does not add plugin command
arguments, plugin events, terminal text exports, or normalized text exports.

## Verification

- `cargo fmt --all -- --check`
- `cargo test -p witty-ui normalize_nfc_option_rebuilds_literal_matches --quiet`
- `cargo test -p witty-app search_key_action_consumes_find_bar_keys_without_terminal_input --quiet`
- `cargo test -p witty-app search_count_label_reports_zero_and_invalid_regex_states --quiet`
- `cargo test -p witty-web browser_search_status_reports_zero_and_invalid_regex_states --quiet`
- `cargo test -p witty-web browser_search_history_keeps_queries_in_ui_state --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `cargo test --workspace --quiet`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `node --check scripts/run-witty-web-smoke.mjs`
- `scripts/run-witty-web-smoke.sh`

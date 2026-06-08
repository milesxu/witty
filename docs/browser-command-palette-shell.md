# Browser Command Palette Shell

Updated: 2026-05-30

`m161-browser-command-palette-shell` added the first browser-side command
palette surface. `m162-browser-command-palette-visible-windowing` keeps the
compact modal but makes the visible command window follow the selected item.
The goal is parity with the native command palette boundary: commands stay
local unless they explicitly write terminal input, and text input ownership is
shared with search and IME routing.

## Implemented

- Browser `Ctrl+Shift+P` opens a command palette modal.
- Palette text input, backspace, selection movement, enter confirm, and escape
  close are exposed through wasm session methods.
- Browser startup registers `witty.about`, search commands, and `web.echo`
  so the palette has builtin and plugin-like entries.
- Confirming `witty.search.open` invokes the local search shell without
  gateway input.
- Opening the palette closes browser search and clears active IME preedit.
- Browser IME preedit and commit route into palette query state when the
  palette owns text input.
- Browser diagnostics expose palette open/query/filter/selection index/status
  and compact visible items for Playwright coverage.
- The compact visible command list slides with selection movement; `PageDown`
  can select `web.echo` while the diagnostics and overlay window show the
  selected row instead of the first page.
- While the browser palette is open, the shortcut hints are actionable: `F1`
  invokes `witty.about`, and `F2` invokes the first non-builtin command.
  Plain and modified function keys still reach terminal applications when the
  palette is closed.

## Rendering Boundary

The browser palette is rendered as a modal overlay in the wasm frame path. The
overlay still draws a compact three-row command window and keeps the visible
window sliding with selection movement, but browser smoke tests now exercise the
same command-palette flow across node loopback, Rust PTY, and product launcher
gateways.

`m163-browser-renderer-glyphon-buffer-width` reduced `glyphon` buffer widths to
the text run's estimated display width instead of the full remaining surface
width. That lowers WebGPU staging-buffer pressure enough for modal renders to
coexist with accumulated PTY output.

## Verification

Covered by:

- `cargo test -p witty-web command_palette --quiet`
- `cargo test -p witty-ui command_palette --quiet`
- `cargo test -p witty-web ime --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `scripts/run-witty-web-smoke.sh`
- `WITTY_WEB_SMOKE_GATEWAY=rust scripts/run-witty-web-smoke.sh`
- `WITTY_WEB_SMOKE_GATEWAY=launcher scripts/run-witty-web-smoke.sh`
- `cargo test --workspace --quiet`
- `cargo clippy --workspace --all-targets -- -D warnings`

The browser smoke checks command palette open, `PageDown` windowing,
filter/IME preedit/IME commit, confirm-to-search, reopen-closes-search,
escape-close, no terminal input bytes during palette interaction, and direct
palette `F1`/`F2` command shortcuts on all three gateway modes.

## Follow-Ups

- Add browser shortcuts for any future native command shortcuts beyond `F1` and
  `F2`.

# Terminal Native Hyperlink Activation

Updated: 2026-05-30

m134 adds native-window OSC 8 hyperlink hit-testing and explicit activation.
Browser activation remains a follow-up task.

## What Changed

- Added `RenderSnapshot::hyperlink_id_at()` and `RenderSnapshot::hyperlink_at()`.
- Added native pointer hit-testing against snapshot hyperlink metadata.
- Wired native hover state into `RenderSnapshot::hovered_hyperlink`.
- Added explicit modifier-click activation:
  - Linux/Windows: `Ctrl+LeftClick`
  - macOS: `Cmd+LeftClick`
- Added a shared external URL opener policy in `witty-launcher`.
- Kept hyperlink URIs out of plugin events and command arguments.

## Opening Policy

Native hyperlink opening validates the URI before spawning a platform opener.

Allowed initial schemes:

- `http`
- `https`
- `mailto`

Rejected values include empty strings, invalid schemes, unsupported schemes such
as `file` and `javascript`, control-containing strings, and oversized URLs.

The platform opener remains the existing product opener path:

- Linux and other non-macOS Unix: `xdg-open`
- macOS: `open`
- Windows: `cmd /C start`

## Mouse Behavior

Activation is checked before terminal mouse reporting and local selection
fallbacks. If the user explicitly modifier-clicks a linked cell, Witty opens
the validated URI and does not send that click to the terminal application.

Without the activation modifier, existing behavior is preserved:

- mouse-reporting applications continue to receive ordinary clicks.
- local selection and primary paste behavior remain unchanged when reporting is
  inactive.
- Shift selection override remains the local selection escape hatch for mouse
  reporting mode.

## Verification

- `cargo fmt --all -- --check`
- `cargo test -p witty-core hyperlink --quiet`
- `cargo test -p witty-launcher external_url --quiet`
- `cargo test -p witty-launcher --quiet`
- `cargo test -p witty-app hyperlink --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown --quiet`
- `cargo test --workspace --quiet`
- `cargo clippy --workspace --all-targets -- -D warnings`

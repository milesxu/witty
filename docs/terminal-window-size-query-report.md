# Terminal Window Size Query Report

Updated: 2026-05-30

`m181-terminal-window-size-query-report` adds a bounded reply for xterm-style
window manipulation queries that ask for the terminal's character grid size.

## Implemented

`BasicTerminal` now queues `TerminalReply` actions for:

- `CSI 18 t`: report text-area size in characters
- `CSI 19 t`: report screen size in characters

Both return the terminal grid size as:

```text
CSI 8 ; rows ; cols t
```

## Boundary

Only character-grid size reports are implemented. Pixel window-size reports,
window movement/resizing commands, iconification, maximize/minimize operations,
and title-stack operations remain unsupported because those belong to a host UI
policy layer rather than terminal-core parsing.

Replies use the existing `TerminalHostAction::TerminalReply` path and are not
rendered as terminal text or exposed to plugins.

## Verification

Covered by:

- `cargo test -p witty-core window_manipulation_reports_character_grid_size --quiet`
- `cargo test -p witty-web browser_host_actions_forward_terminal_replies_and_return_clipboard_writes --quiet`
- `scripts/run-witty-web-smoke.sh`

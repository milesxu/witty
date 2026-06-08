# Terminal Synchronized Output Mode

Updated: 2026-05-30

`m182-terminal-synchronized-output-mode` adds parser-level support for the
DEC private synchronized output mode used by modern terminal applications.
`m183-browser-synchronized-output-coalescing` wires that state into the browser
gateway render loop.
`m184-native-synchronized-output-redraw-gate` applies the same render-request
gate to native PTY output.
`m185-browser-synchronized-output-timeout` adds a browser watchdog so a session
cannot suppress rendering forever if a program leaves mode `2026` enabled.
`m186-native-synchronized-output-timeout` applies the same watchdog limit to
native rendering.

## Implemented

`BasicTerminal` now tracks:

- `CSI ? 2026 h`: enable synchronized output mode
- `CSI ? 2026 l`: disable synchronized output mode

The state is queryable through the request-mode-report path:

- `CSI ? 2026 $ p` -> `CSI ? 2026 ; 1 $ y` when enabled
- `CSI ? 2026 $ p` -> `CSI ? 2026 ; 2 $ y` when disabled

`ESC c` full reset clears synchronized output mode.

Browser gateway rendering now coalesces synchronized output:

- while mode `2026` is enabled, gateway output is still parsed into terminal
  state and host actions are still drainable, but `render_current_frame()` is
  deferred.
- when later gateway output disables mode `2026`, the accumulated damage renders
  in one frame.
- if mode `2026` remains enabled, browser JavaScript forces a frame flush after
  150 ms without changing the terminal mode state.

Native PTY output uses the same policy:

- output parsed while mode `2026` remains enabled refreshes terminal/search state
  but does not rebuild the current frame or request a redraw.
- output that disables mode `2026`, process exit, and transport errors still
  rebuild/request redraw.
- if mode `2026` remains enabled, the native event loop forces one frame rebuild
  and redraw after 150 ms without changing the terminal mode state.

## Boundary

The terminal parser only tracks protocol state. Browser coalescing is a host
render-loop policy, and native redraw gating is a host event-loop policy.
Browser and native timeouts deliberately flush a frame without disabling mode
`2026`; a later disable sequence still clears the coalescing state normally.

## Verification

Covered by:

- `cargo test -p witty-core synchronized_output --quiet`
- `cargo test -p witty-app synchronized_output --quiet`
- `cargo test -p witty-app --quiet`
- `cargo check -p witty-web --target wasm32-unknown-unknown`
- `node --check crates/witty-web/static/app.js`
- `scripts/run-witty-web-smoke.sh`

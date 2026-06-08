# Browser Selection Clipboard Plan

Updated: 2026-05-30

m110 plans browser clipboard and selection parity after m108/m109 added
Shift-left local selection and a configurable mouse override policy.

Reference points:

- W3C Clipboard API and events: https://w3c.github.io/clipboard-apis/
- MDN Clipboard API overview: https://developer.mozilla.org/en-US/docs/Web/API/Clipboard_API

## Current State

Native window mode already has:

- `Ctrl+Shift+C` copy selection to the system clipboard.
- `Ctrl+Shift+V` paste system clipboard text to terminal input.
- bracketed paste wrapping when terminal mode `2004` is active.
- Linux primary selection publishing after completed drag selection.
- Linux middle-click primary selection paste.

Browser mode currently has:

- wasm local selection methods:
  - `begin_local_selection()`
  - `update_local_selection()`
  - `end_local_selection()`
  - `selected_text()`
  - `selection_range_text()`
- JavaScript Shift-left pointer routing for local selection while app mouse
  reporting is active and policy is `shift-select`.
- no browser system clipboard copy/paste path yet.
- no browser primary selection model.

## Product Decision

Implement explicit browser clipboard actions first:

| Gesture | Browser behavior |
| --- | --- |
| `Ctrl+Shift+C` with non-empty terminal selection | write selected text to `navigator.clipboard.writeText()` |
| `Ctrl+Shift+V` with readable clipboard text | paste text to terminal input |
| completed mouse selection | keep terminal-local selection only; do not auto-write clipboard |
| middle click | keep mouse reporting/local selection behavior; do not implement primary paste |

This mirrors the native clipboard shortcuts without pretending browsers expose an
X11-style primary selection. Browser primary selection is a non-goal unless a
future platform-specific bridge is added outside ordinary web APIs.

## Clipboard API Constraints

The browser clipboard path is asynchronous and permission-gated:

- The Clipboard interface is exposed only in secure contexts.
- Browser implementations may require user activation, an active focused
  document, and permission or Permissions Policy allowance.
- `readText()` is more sensitive than `writeText()` and should only run from an
  explicit paste gesture.
- The API returns promises and can fail with permission or platform errors.

Witty should therefore keep clipboard orchestration in JavaScript and keep
the wasm API synchronous for terminal-state changes.

## Architecture

Add browser-side clipboard helpers in `crates/witty-web/static/app.js`:

- `copySelectionToClipboard(session)`
  - read `session.selected_text()`
  - no-op with status if empty
  - call `navigator.clipboard.writeText(text)`
  - prevent default only for the explicit copy shortcut
  - report failures in status text, not by writing into the terminal
- `pasteClipboardToTerminal(session)`
  - call `navigator.clipboard.readText()`
  - no-op with status if empty
  - call a new wasm method such as `session.paste_text(text)`
  - flush gateway input after wasm writes bytes
  - report failures in status text

Add wasm-side paste support in `crates/witty-web/src/lib.rs`:

- `paste_text(text: String) -> Result<bool, JsValue>`
  - return `false` for empty text
  - wrap with bracketed paste when `BasicTerminal::bracketed_paste_enabled()`
    is true
  - write bytes through `TerminalApp::write_input()`
  - do not echo pasted bytes locally unless the gateway/PTY sends output back

Prefer moving the bracketed paste payload helper from native-only `witty-app`
into a shared crate (`witty-core` or `witty-ui`) before implementing browser paste.
That avoids maintaining duplicate `ESC[200~...ESC[201~` logic.

## Shortcut Handling

Browser `keydown` should intercept clipboard shortcuts before
`session.handle_key()`:

- `Ctrl+Shift+C` should copy selection and not send `ETX` or any key bytes.
- `Ctrl+Shift+V` should paste clipboard text and not send literal `V`/control
  bytes.
- Plain `Ctrl+C` and plain `Ctrl+V` should keep current terminal behavior.
- `Meta` shortcuts are deferred; the first target is parity with the native
  Witty `Ctrl+Shift+...` shortcuts across desktop browsers.

The handler must remain async-safe: call `event.preventDefault()` synchronously
for recognized clipboard shortcuts, then run the clipboard promise and update
status when it settles.

## Security Model

Clipboard integration must not become remote clipboard sync:

- Do not read clipboard in timers, focus events, mouse selection completion, or
  gateway messages.
- Do not automatically copy every terminal selection to system clipboard.
- Do not send clipboard contents to the gateway unless the user explicitly
  requested paste.
- Do not expose clipboard contents through smoke diagnostics except for bounded
  test assertions.
- Do not add a browser primary-selection emulation backed by ordinary clipboard;
  it would surprise users by overwriting the real clipboard.

## Runtime Smoke Scope

Extend the node-gateway browser smoke first because it can observe exact gateway
input frames without shell echo ambiguity.

Recommended smoke steps:

1. Enable normal terminal output and create a deterministic selection using the
   existing Shift-left local-selection path.
2. Grant Chromium clipboard permissions for the test origin or install a narrow
   page-side test clipboard shim.
3. Dispatch `Ctrl+Shift+C` and assert:
   - event default was prevented
   - browser clipboard now contains `session.selected_text()`
   - no gateway input frame was emitted
4. Seed browser clipboard text, dispatch `Ctrl+Shift+V`, and assert:
   - event default was prevented
   - gateway receives the pasted bytes
5. Enable bracketed paste mode with gateway output `CSI ? 2004 h`, dispatch
   `Ctrl+Shift+V`, and assert:
   - gateway receives `ESC[200~<text>ESC[201~`

After node-gateway coverage is stable, add a product launcher smoke that only
checks the high-level paste result through PTY output. Keep clipboard permission
setup explicit so local browser policy failures are reported as skipped or
failed with a clear reason.

## Implementation Milestones

### m111 Browser Copy Selection Shortcut

Status: implemented.

Write scope:

- `crates/witty-web/static/app.js`
- `scripts/run-witty-web-smoke.mjs`
- docs update if behavior differs from this plan

Acceptance:

- `Ctrl+Shift+C` copies `session.selected_text()` through
  `navigator.clipboard.writeText()`.
- empty selection does not throw and does not emit gateway input.
- node browser smoke verifies copy without gateway input.

Implementation note: browser keydown handling now intercepts `Ctrl+Shift+C`
before `session.handle_key()`, calls `preventDefault()` synchronously, and then
performs the async clipboard write. The node-gateway smoke grants Chromium
clipboard permissions and verifies both empty-selection no-op behavior and
selected-text copy behavior without emitting terminal input frames.

### m112 Browser Paste Shortcut

Status: implemented.

Write scope:

- shared paste payload helper in `witty-core` or `witty-ui`
- `crates/witty-web/src/lib.rs`
- `crates/witty-web/static/app.js`
- focused tests and browser smoke

Acceptance:

- `Ctrl+Shift+V` reads clipboard text and writes it as terminal input.
- empty clipboard text is a no-op.
- bracketed paste mode wraps browser pasted text exactly like native paste.
- wasm target check and node browser smoke pass.

Implementation note: bracketed paste payload generation now lives in
`witty-core::paste_payload()` and is shared by native and browser paste paths.
Browser `Ctrl+Shift+V` is intercepted before terminal key handling, reads
`navigator.clipboard.readText()` from the explicit shortcut gesture, writes
non-empty text through the wasm session input path, and flushes gateway input
without local echo. The node-gateway browser smoke verifies empty clipboard
no-op behavior, plain pasted bytes, and exact `ESC[200~...ESC[201~` wrapping
while bracketed paste mode is active. Rust PTY gateway and launcher smoke paths
still skip product-level paste assertions; that remains scoped to m113.

### m113 Browser Clipboard Product Smoke

Status: implemented.

Write scope:

- `scripts/run-witty-web-smoke.mjs`
- docs update

Acceptance:

- Rust PTY gateway and `witty --web` launcher paths verify paste output at
  product level.
- smoke output distinguishes permission skip from behavioral failure.

Implementation note: the browser smoke now runs a product-level paste check for
Rust PTY gateway and `witty --web` launcher modes. It seeds
`navigator.clipboard`, dispatches `Ctrl+Shift+V`, requires the browser shortcut
to be handled, and waits for the PTY shell output to include a unique pasted
token. Clipboard API/permission failures are reported as explicit
`skipped-...` results; once clipboard seeding succeeds, missing PTY output is a
behavioral failure. Node-gateway mode keeps the lower-level exact byte checks
from m112 instead of duplicating the product shell assertion.

## Non-Goals

- Browser X11 primary selection parity.
- Middle-click primary paste in ordinary browser mode.
- Background or remote clipboard synchronization.
- Rich HTML/image clipboard support.
- Mobile touch selection handles.
- Context menu UI.

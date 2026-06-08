# Terminal OSC 52 Clipboard Plan

Updated: 2026-05-30

m137 plans OSC 52 clipboard support after OSC 8 hyperlinks. The goal is to
support the common `tmux`/SSH clipboard workflow without turning terminal
output into an unrestricted local clipboard write channel.

## Existing Code Shape

- `witty-core` already parses OSC sequences in
  `BasicTerminalState::osc_dispatch()`.
- OSC `0`/`2` title parsing and OSC 8 hyperlink parsing are pure terminal-core
  state changes today.
- `BasicTerminal::feed()` currently returns `()`, so host actions produced by
  output bytes need a new drainable side channel rather than a direct return
  value.
- Native clipboard handling lives in `crates/witty-app/src/window.rs` behind the
  local `ClipboardSink` trait, with `Clipboard` and Linux `Primary` targets.
- Browser clipboard handling lives in `crates/witty-web/static/app.js` using
  explicit `navigator.clipboard.writeText()` and `readText()` calls for
  shortcuts.
- Browser gateway output currently flows through
  `WittyWebSession::push_gateway_message_json()` and then
  `poll_gateway_events()`, which feeds output bytes into `BasicTerminal`.
- Plugin events are only dispatched explicitly. Terminal output, search query,
  hyperlink URI, and clipboard payloads are not plugin-visible by default.

## OSC 52 Subset

Parse:

```text
OSC 52 ; Pc ; Pd ST
OSC 52 ; Pc ; Pd BEL
OSC 52 ; ; Pd ST
OSC 52 ; ; Pd BEL
```

Initial support:

| Field | Decision |
| --- | --- |
| `Pc` empty | treat as `c` clipboard target |
| `Pc` contains `c` | clipboard target |
| `Pc` is exactly `p` | primary-selection target, host may deny on non-Linux/browser |
| `Pc` contains multiple targets | choose clipboard if `c` is present; otherwise ignore unless exactly `p` |
| `Pd` base64 text | decode and request host clipboard write |
| `Pd` empty | request clearing the target only if policy explicitly allows clear; otherwise ignore in first pass |
| `Pd` is `?` | query/read is unsupported in first pass |
| unsupported targets | ignore and optionally record a bounded diagnostic status |

The common `tmux` path is `OSC 52 ; c ; <base64> ST`, so this subset is useful
without supporting cut buffers, secondary selections, or clipboard readback.

## Core Data Model

Add terminal-core types, names flexible during implementation:

```rust
pub const MAX_OSC52_DECODED_BYTES: usize = 64 * 1024;

pub enum TerminalHostAction {
    ClipboardWrite(TerminalClipboardWrite),
}

pub struct TerminalClipboardWrite {
    pub selection: TerminalClipboardSelection,
    pub text: String,
    pub decoded_bytes: usize,
}

pub enum TerminalClipboardSelection {
    Clipboard,
    Primary,
}
```

`BasicTerminalState` should own a private `pending_host_actions:
Vec<TerminalHostAction>`. Add:

```rust
impl BasicTerminal {
    pub fn drain_host_actions(&mut self) -> Vec<TerminalHostAction>;
}
```

Do not put OSC 52 payloads into `RenderSnapshot`, `FramePlan`,
`PluginEvent`, search state, hyperlinks, diagnostics text, or command args.

## Parsing And Validation

Core should:

- decode `Pd` using the `base64` crate, preferably `STANDARD`.
- reject invalid base64.
- reject decoded payloads larger than `MAX_OSC52_DECODED_BYTES`.
- require valid UTF-8 for the initial text clipboard path.
- reject NUL and non-text C0/C1 control characters while allowing common text
  controls: tab, carriage return, and line feed.
- preserve Unicode text exactly after UTF-8 validation; do not NFC-normalize.
- ignore unsupported query/read and unsupported target requests.
- never mutate terminal cells, selection, title, hyperlinks, or scrollback for
  OSC 52.

If parsing fails, the first implementation can silently ignore the request.
User-visible diagnostics are a host policy decision and must not include the
payload.

## Native Host Policy

Introduce an explicit policy before any host write:

```text
disabled | confirm | allow
```

Recommended initial behavior:

| Context | Default |
| --- | --- |
| native local window | `disabled` until there is a confirmation UI; tests can use `allow` |
| browser launcher/gateway | `disabled` by default |
| future SSH profile | profile-controlled, default `disabled` or `confirm` |

Rationale: terminal output can come from a remote SSH session even when the
local transport is just a normal shell. Silent clipboard writes therefore cross
a trust boundary.

Native implementation shape:

1. Feed PTY output into `BasicTerminal`.
2. Drain host actions immediately after feed.
3. For each clipboard write:
   - check policy.
   - check target support (`Primary` only on Linux-like Unix targets).
   - call `ClipboardSink::set_text()`.
   - surface failures as bounded status text without including payload.
4. Refresh search/frame after terminal content changes as today.

`confirm` can be a planned policy variant before it has a full UI. Until a
modal/notification command surface exists, `confirm` should behave like
`disabled` with a clear internal reason rather than silently allowing.

## Browser Host Policy

Browser OSC 52 cannot rely on the same user gesture that explicit
`Ctrl+Shift+C` uses. Gateway output arrives asynchronously, so
`navigator.clipboard.writeText()` may be rejected even when the policy says
`allow`.

Browser implementation shape:

1. wasm session drains host actions after `poll_gateway_events()`.
2. wasm exposes a compact JSON method such as
   `drain_clipboard_write_actions_json()`.
3. JavaScript applies browser policy and attempts `navigator.clipboard.writeText()`
   only for allowed actions.
4. JavaScript records a bounded result object for smoke tests:
   `written|denied|unsupported|permission-error|oversized`, without payload.
5. For product UI, denied/blocked actions should become a small host-rendered
   status/notification later, not terminal text injected into the PTY stream.

Do not add browser clipboard read/query support for OSC 52 in this line.

## Plugin Boundary

OSC 52 payloads are terminal output content and often contain secrets. Keep the
same privacy stance used for search and hyperlinks:

- no plugin events containing clipboard payloads.
- no command arguments containing clipboard payloads.
- no render snapshot fields containing clipboard payloads.
- no smoke diagnostics containing clipboard payloads.
- future plugin APIs may only observe aggregate policy results if there is a
  reason, and even that should avoid payloads.

## Tests

Core tests:

- `OSC 52;c;<base64> ST` queues a clipboard write action.
- empty `Pc` defaults to clipboard.
- BEL termination works.
- `Pc=p` queues a primary action.
- multiple targets choose clipboard if `c` is present.
- invalid base64 queues no action.
- query `Pd=?` queues no action and sends no reply bytes.
- oversized payload queues no action.
- invalid UTF-8 queues no action.
- NUL/non-text control payload queues no action; LF/CR/TAB text is allowed.
- feeding OSC 52 does not change snapshot text, title, search, hyperlink, or
  damage rows.

Native tests:

- policy `disabled` does not call `ClipboardSink`.
- policy `allow` writes clipboard target.
- unsupported primary target reports a bounded error without payload.
- malformed/oversized output produces no clipboard write.

Browser tests:

- wasm drains an action after gateway output with OSC 52.
- JS policy `disabled` records denied and does not call `navigator.clipboard`.
- JS policy `allow` with a stubbed clipboard records a write without gateway
  input bytes.
- browser smoke verifies no payload is written into terminal text or plugin
  diagnostics.

## Follow-Up Tasks

1. `m138-terminal-osc52-core-policy`: done. Implemented terminal-core OSC 52
   parsing, host action queue/drain API, validation tests, and shared policy
   enums. See `terminal-osc52-core-policy.md`.
2. `m139-native-osc52-clipboard`: done. Wired native output polling to drain
   host actions and apply `disabled|confirm|allow` policy through
   `ClipboardSink`. See `terminal-osc52-native-clipboard.md`.
3. `m140-browser-osc52-clipboard-smoke`: done. Exposed wasm action draining,
   applied browser JavaScript policy, and added Playwright coverage. See
   `terminal-osc52-browser-clipboard.md`.
4. `m141-real-tui-compatibility-smoke-plan`: done. Planned repeatable
   `tmux`/`vim`/`less`/`htop`/`vttest` smokes and selected terminal query
   replies as the next unblocker. See `real-tui-compatibility-smoke-plan.md`.

## Non-Goals

- OSC 52 clipboard read/query.
- Remote clipboard synchronization.
- Rich clipboard formats, HTML, images, or arbitrary bytes.
- User confirmation UI implementation in m138.
- Plugin-readable clipboard payloads.

# Plugin Runtime Selection

## Decision

Witty should use the Wasm Component Model as the primary plugin ABI and Wasmtime as the first native host runtime.

Extism remains useful as a later compatibility layer for simpler script-like plugins, but it should not be the primary contract because Witty needs typed interfaces, explicit worlds, and long-term ABI versioning.

## Why Wasm Component Model First

- WIT lets the project describe typed interfaces and worlds independently from any one guest language.
- Wasmtime has first-class component host APIs and bindgen support.
- The model maps cleanly to Witty's permission gate: plugins return requested actions; the host decides whether to apply them.
- Browser-capable architecture benefits from a portable guest ABI, even though native desktop and browser hosts may use different embedding layers.

## Initial ABI

The initial WIT file lives at:

```text
crates/witty-plugin-api/wit/witty-plugin.wit
```

`witty-plugin-api` embeds this file as `PLUGIN_WIT` and validates it in tests with `wit-parser`, so syntax errors in the checked-in ABI fail the workspace test suite.

The world is `terminal-plugin`.

Guest exports:

- `manifest() -> plugin-manifest`
- `commands() -> list<command-registration>`
- `handle-event(event: plugin-event) -> list<plugin-action>`

Important host-owned rules:

- The host attaches `PluginRuntime::WasmComponent`; Wasm guest manifests do not choose their own runtime.
- The host attaches `source_plugin` from the manifest id.
- The host rejects duplicate command ids.
- The host routes command invocations only to the plugin that registered the
  command id. Unknown command ids are not broadcast.
- Runtime `RegisterCommand` actions are validated and added to the same host
  ownership table so later invocations remain owner-routed. `TerminalApp`
  passes its current command registry as a reserved set during dispatch, so a
  dynamic plugin command cannot collide with app-owned or already surfaced
  commands before the action is applied.
- The host enforces `TerminalWritePermission`.
- The host filters command invocation context by permission. Plugins with
  `TerminalReadPermission::None` receive empty context; `SelectionOnly`
  receives selected-command-block metadata; `CurrentScreen` and
  `FullScrollback` receive live current-directory and selected-block metadata.
- The host also filters read events: terminal output is delivered only to
  `CurrentScreen`/`FullScrollback` plugins, while selection changes are
  delivered to `SelectionOnly` and broader read tiers.
- `confirm` write permission requires a future UI confirmation gate; until then it should not silently write.

## Native Runtime Spike

`witty-plugin-wasm` is the first native runtime crate. It depends on `wasmtime 45.0.0` with a narrow feature set: `anyhow`, `component-model`, `cranelift`, `runtime`, and `std`.

Current scope:

- generate bindings for `terminal-plugin` with `wasmtime::component::bindgen!`;
- construct an Engine with Component Model enabled;
- compile component bytes or load a component from disk;
- create an empty `Linker` and typed `Store`;
- adapt WIT/bindgen types into existing `witty-plugin-api` types.
- pass command invocation context through the Rust and Wasm plugin APIs, starting
  with permission-gated current-directory and selected-command-block metadata,
  including range coordinates but not command/output text.

## Implemented Integration

A real guest component fixture now exercises `TerminalPlugin::instantiate`,
`call_manifest`, `call_commands`, and `call_handle_event`. `PluginHost` can
prepare/install Wasm components, dispatch command events, attach host-owned
command ownership, and enforce terminal-write plus command-context permissions.

## Runtime Error Isolation

`PluginHost` now records event handler failures as `PluginRuntimeFailure`,
disables the failing plugin, and continues dispatching to other eligible
plugins. Host policy violations still fail loud instead of becoming recoverable
runtime failures. See `plugin-runtime-error-isolation.md`.

## Startup Loading

Smoke mode and native window mode both route `--wasm-plugin`/`--plugin-dir`
startup loading through the shared `witty-app::install_wasm_plugins` helper.
Other CLI modes reject startup plugins to keep diagnostics, browser launch, and
bounded smoke tests deterministic. See `wasmtime-runtime-spike.md`.

## Host Imports

The first native Wasm host import is `host.get-host-info()`. It returns only
app name, app version, and plugin ABI version, and is wired through
`WasmPluginRuntime::host_linker()` by default. See
`plugin-host-info-import.md`.

The first permission-gated product-data import is
`host.get-profile-store-summary()`. It requires `profile-read` and returns only
summary counts plus whether a default profile is configured. It does not expose
profile ids, names, targets, SSH paths, credential references, or raw store
content. See `plugin-profile-store-summary-import.md`.

The transport-layer profile summary remains richer than the plugin ABI. `witty-ui`
owns the adapter from `ProfileStoreSummary` to `PluginProfileStoreSummary`, so
the Wasm runtime does not depend on profile-store storage types and plugin-facing
data remains count-only.

The first host-owned profile flow is
`PluginAction::RequestProfilePicker`. It is gated by `profile-read`, queues a
`PendingProfilePickerRequest` with the host-attached source plugin id, and does
not return profile inventory, selected ids, or launch status to the plugin. See
`plugin-profile-picker-request-action.md`.

`witty-ui` provides `review_profile_picker_requests()` and `TerminalApp` exposes
`review_pending_profile_picker_requests()` for trusted host UI. These helpers
turn pending picker requests plus a current `ProfileStoreV1` into redacted
picker review rows. The redacted summary is host UI data only; it is not
returned through the plugin ABI.

After trusted host UI selects a profile id,
`resolve_profile_picker_selection()` revalidates the current store, queued
request, and selected id, returning a cloned `SshProfile` only for launchable
profiles. On native targets, `resolve_profile_picker_pty_config()` converts that
selection to `LocalPtyConfig` without spawning. `TerminalApp` exposes
non-mutating wrappers over pending picker requests by request index.

On native targets, `TerminalApp::take_resolved_profile_picker_pty_config()` is
the confirmed-drain helper for a selected picker request. It resolves the
selected profile id to `LocalPtyConfig` first, removes only that request on
success, and leaves the picker queue intact on any resolution error.
`dismiss_pending_profile_picker_request()` is the cancellation path for trusted
UI: it removes one queued picker request without resolving a profile or
reporting rejection to the plugin.

Plugins that already have an opaque profile id can request the narrower
host-owned `PluginAction::RequestProfileLaunch` flow. It is also gated by
`profile-read`, validates the id and reason, queues
`PendingProfileLaunchRequest`, and does not launch SSH or expose profile
metadata in this slice. See `plugin-profile-launch-request-action.md`.

`witty-ui` provides `review_profile_launch_requests()` as the first trusted
consumer helper. It revalidates queued launch requests against a current
`ProfileStoreV1` and returns only redacted UI review rows: source plugin,
requested id, optional reason, launchability/not-found status, profile name,
tags, and default marker. It still does not return targets, paths, credential
ids, raw store data, or launch results.

`resolve_profile_launch_request()` is the fail-closed launch-point helper. It
revalidates one queued request against a current `ProfileStoreV1` and returns a
cloned `SshProfile` only when the profile exists and is launchable. Missing ids,
credential-resolver profiles, unsafe request fields, and invalid stores are
errors. The helper is host-only and is not part of the plugin ABI.

On native targets, `resolve_profile_launch_pty_config()` adds the deterministic
conversion step from a resolved launchable profile to `LocalPtyConfig` for SSH.
It does not spawn the PTY process; session replacement, tab creation, or
confirmation remains app-owned policy.

`TerminalApp` exposes the same review and native PTY-config resolution as
non-mutating queue helpers. They require the caller to pass a current
`ProfileStoreV1`, leave the pending queue intact, and let trusted UI decide when
to drain or reject queued requests.

`take_resolved_profile_launch_pty_config()` is the native single-request
confirmed-drain helper: it resolves the request at a trusted host-selected index
first, removes only that request on success, and leaves the queue intact on any
resolution error. `take_resolved_profile_launch_pty_configs()` keeps the batch
path, resolving every pending request before clearing the queue.
`dismiss_pending_profile_launch_request()` provides the matching cancellation
path for one queued launch request without reading profile metadata or reporting
status back to the plugin.

For UI list binding, `TerminalApp::review_pending_profile_actions()` combines
picker and launch reviews into `PendingProfileActionReview` rows. The returned
`PendingProfileActionKey` stores the request kind plus the current queue index;
it is not a global event id. After any dismissal or confirmed drain, UI should
review again before using old keys. `dismiss_pending_profile_action()` consumes
the same key for app-owned cancellation without resolving profiles or launching
SSH.

On native targets, `take_resolved_pending_profile_action_pty_config()` consumes
`PendingProfileActionConfirmation` values for the same keys. Picker
confirmations include the trusted host-selected profile id; launch
confirmations use only the launch key. The helper checks that the key kind
matches the confirmation variant, resolves to `LocalPtyConfig`, drains only
after success, and still leaves actual PTY/SSH start to app policy.

`witty-app` native window code now has a non-launching
`NativeProfileActionBridge` around the same model. The bridge maintains a
review snapshot plus trusted display rows, emits refresh/dismiss/confirmed
events, and refreshes after plugin commands. Display rows map picker and launch
reviews into native UI labels, status, reason, and confirm/dismiss affordances;
they remain inside trusted native window state and are not returned through the
plugin ABI. The snapshot also carries trusted picker option rows derived from
the redacted profile summary: profile id, name, tags, default marker, and
launchability. These rows are for later host-owned selection UI only; they do
not include target host/user/port, credential ids, or OpenSSH arguments. The
native window can render action rows as a `FramePlan` overlay, so profile action
details are visible in trusted UI without entering terminal scrollback. Picker
option rows can also render in that native overlay and are captured by overlay
hit-testing so clicks do not pass through to terminal selection or mouse
reporting. Overlay hit-testing maps action positions to row, confirm, or
dismiss targets; dismiss clicks remove the corresponding pending request.
Confirm clicks on launchable profile-launch rows now run the same trusted
confirmed-drain path, resolving the request to `LocalPtyConfig` and removing it
from the queue without starting PTY/SSH or replacing the active transport.
For picker option rows, only the trailing `[Select]` hit area on launchable
options maps to trusted confirmed-drain; it resolves the chosen profile to
`LocalPtyConfig` and removes the picker request without starting PTY/SSH or
replacing the active transport. Credential-resolver-required options remain
display-only and captured so their clicks cannot fall through to terminal
selection or mouse reporting. A successful confirmed-drain is now normalized
into a native `NativeResolvedProfileActionHandoff` that holds the key, action
kind, source plugin, selected profile id, reason, and resolved `LocalPtyConfig`
inside trusted window state. Confirmed handoffs are stored in a trusted FIFO
queue so consecutive confirmations cannot overwrite earlier resolved configs,
and app-owned policy can take the next handoff explicitly. The native window's
current policy is `DeferStart`: it consumes the next resolved handoff into a
trusted deferred-start queue, preserving the `LocalPtyConfig` for a later
session replacement, new-tab, or credential-resolver policy. The default confirm
flow then turns that deferred start into a `NativeProfileActionStartPlan` with
mode `ReplaceCurrentSession`. Native code now has an explicit execution
boundary for that plan: given an already-created transport, it can replace the
active transport and reset terminal, search, and shell-integration state while
preserving app-owned command/plugin state. The native window policy now consumes
the next confirmed start plan, calls `LocalPtyTransport::spawn(plan.config)`,
and passes the resulting transport into that boundary. Spawning remains native
policy, not plugin/runtime behavior: launch results are not returned through the
plugin ABI or written into terminal scrollback. If spawn fails, the plan remains
queued and the raw diagnostic goes to stderr only. The trusted native overlay
also gets a generic start-failure row with `[Retry]` and `[Dismiss]`; retry
attempts the queued plan again, while dismiss drops that queued start plan. The
failure row is `FramePlan` overlay state only and does not include the raw spawn
error, SSH target, credentials, or OpenSSH arguments. Successful replacement
sets a matching native-only start-success row with a dismiss affordance; this is
also trusted `FramePlan` overlay state and does not write a launch-success
message into terminal output. The window app also records app-owned
current-session metadata after a successful profile-action start: key, action
kind, source plugin, selected profile id, reason, and start mode. That metadata
is stored in a trusted native `NativeSessionRegistry` with app-owned session ids
and active-session state. The current `ReplaceCurrentSession` policy updates
the active registry record, while the registry and tab read model can already
represent multiple sessions for future tab policy. The metadata deliberately
excludes the resolved `LocalPtyConfig`, SSH target, credentials, OpenSSH
arguments, and raw launch result. The first native session UI surface is a
host-owned session strip derived from that registry. It renders only profile id,
action kind, source plugin, start mode, and active/inactive status in
`FramePlan`, clears the terminal glyphs under that strip, and still does not
write profile identity or launch status to terminal scrollback or the plugin
ABI. The session strip also has trusted native hit-testing and hover state:
clicking a visible tab span switches only the registry active-session id and
captures the mouse event before terminal selection, hyperlink activation, or
mouse reporting can see it. Native code now also has a real per-session runtime
switch boundary: inactive sessions can park transport, terminal, search, and
shell-integration state under a trusted session id; switching to a parked
session swaps those four runtime states into the active window and parks the
old active runtime under its previous session id. The current profile-action
executor also supports a trusted `NewTab` start mode: once native policy has
created a transport, `NewTab` inserts an inactive session record and parks a
fresh terminal/search/shell runtime under that session id without replacing the
current active transport or terminal state. The trusted native profile-action
overlay now exposes that policy as host-owned controls: launch rows offer
replace-current and `New Tab` actions, and launchable picker option rows offer
the same split after a trusted profile choice. The selected start mode is kept
inside native window policy and becomes the mode on the queued
`NativeProfileActionStartPlan`; it is not reported back through the plugin ABI
or terminal output. It still does not expose tab inventory, selected tab id, or
profile details through terminal scrollback or the plugin ABI. Native code also
has the first tab lifecycle boundary for closing parked inactive sessions: the
close helper removes an inactive session record only when a matching parked
runtime exists, drops that parked transport/runtime state, and refuses active or
inconsistent session ids. Active-session close now has the first safe policy:
if another inactive session has a parked runtime, native code switches that
runtime into the active window, parks the old active runtime under its previous
session id, and then closes that old parked runtime and registry record. Closing
the last active session is explicitly blocked for now rather than spawning a
fallback PTY implicitly. The native session strip now renders a host-owned close
affordance for each visible session tab and maps hit-testing to either `Select`
or `Close`. Close hits call only the trusted native close policies above; when
the last active session is blocked, the strip shows a short native-only notice
without any session id, target host, credential, or launch result. That notice
is cleared by a successful tab switch, successful session close, or successful
profile-action session start so stale blocked-close feedback cannot survive
after native session state changes. The no-switch-target active close behavior
is now represented as an explicit native fallback policy whose default remains
`Block`; non-default close-window and fallback-local-session behavior both sit
behind that host-owned policy boundary without making tab inventory or launch
results plugin-visible. The policy now resolves first to a trusted native
fallback action and only then to a close result or event-loop request, giving
non-blocking actions a host-owned boundary before event-loop behavior is wired.
A non-default close-window action is translated into a native event-loop close
request rather than terminal output or plugin feedback; `witty --window
--window-last-active-close close-window` selects that behavior explicitly, while
the default policy still blocks. The policy exposes stable config values
(`block`, `close-window`, and `fallback-local-session`) through the same native
type used by CLI parsing, so future diagnostics can report the selected policy
without duplicating strings.
Tab
inventory, selected tab id, close results, target hosts, credentials, and launch
results still stay out of terminal scrollback and the plugin ABI. When a plugin
command requests a picker or launch action, the native terminal feeds only a
short pending-action count into the local display so the request is visible
before full trusted UI binding exists.
This feedback remains count-only because terminal output may later be visible
to plugins with terminal read permission; profile ids, names, review status,
selection, credentials, resolved configs, store inventory, and launch
success/failure stay inside trusted host state. The window app reads a fresh
default profile store snapshot for review when available, treats a missing
store as empty, and does not write the store.
Pointer hover over the native overlay is also host-owned: it highlights only
the trusted `FramePlan` row under the pointer, clears lower terminal/link hover
state while the overlay is top-most, and never writes profile details or hover
state into terminal output. Session tab strip hover follows the same rule:
select and close spans use separate native hover colors, including a distinct
close-target color for `[x]`, without writing hover state or tab actions into
terminal scrollback or the plugin ABI. The close affordance is only actionable
when the full `[x]` marker is visible; a width-truncated close marker does not
produce a close hit target. Blocked-close notices reserve native strip width
when possible so stale-action feedback stays visible on narrow windows, and the
reserved notice area is not mapped to any session select or close action. After
session tab clicks mutate close/notice state, native hover is recomputed from
the current pointer and current notice-aware hit-test rather than retaining a
pre-notice hit target.

The native last-active close policy also supports the non-default
`fallback-local-session` value:
`witty --window --window-last-active-close fallback-local-session`. When a
last active profile-action session is closed under that policy, native code
requests a normal local fallback PTY, replaces the active transport with that
local session, resets terminal/search/shell-integration state, and clears the
host-owned profile-action session/tab registry plus parked runtimes. PTY spawn
failure is written to stderr only and leaves the visible native blocked-close
notice in trusted frame state; no terminal/plugin launch result is produced.
The fallback PTY spawn boundary is testable with an injected transport spawner,
so failure preserves the existing active transport, terminal/search/shell state,
session registry, and parked runtimes until a real fallback transport exists.
When native startup reporting is enabled, the report includes only the selected
last-active close policy config value (`block`, `close-window`, or
`fallback-local-session`). It does not include profile ids, tab inventory,
selected tab ids, targets, credentials, PTY configs, raw spawn diagnostics, or
launch results. Pure startup-report coverage checks all three policy values use
the same stable config strings as CLI parsing. The CLI parser's invalid-value
error also reads the allowed-value list from the policy type, so future policy
additions have one config-list source to update. The policy type also owns the
config-value parser, leaving CLI code to wrap parse failures in user-facing
errors rather than duplicating policy string matches. CLI option tests cover
all accepted policy values end to end through `AppOptions`, and a default-window
CLI regression keeps `--window` without an explicit value on `block`. Native
fallback-policy coverage also verifies config values survive the
window-policy-to-native-policy conversion used by startup reporting. Policy
matrix tests now iterate the policy type's canonical `all()` list instead of
hand-repeating variant arrays, and pure CLI coverage checks that `all()` and
`config_values()` stay in the same order with parser round-trips for every
allowed value. The native active-close fallback policy also has a test-only
canonical list, with coverage that its config-value order matches the
CLI-facing window policy list exactly.
Session close results now also pass through a small native event-request
classifier before the window event loop consumes them. Only
`RequestWindowClose` can set the internal close-window request, and only
`RequestFallbackLocalSession` can set the fallback-local-session request;
ordinary close, blocked-close, and ignored results produce no event-loop
request. The classifier carries no profile, tab, transport, or launch-result
data. Pure tests now iterate the canonical close-result list so every result is
covered for both request booleans and the aggregate `any()` flag. Window-close
and fallback-local-session requests also share a small one-shot flag consumer,
with pure coverage that a request is returned once and cleared before the next
event-loop check. The classifier output also owns the pending-flag apply step,
with coverage that it sets only the requested native flags and never clears an
already queued request.
Blocked-close notice lifecycle is also covered as a current-notice by
close-result matrix: blocked close creates the trusted notice, ignored keeps
the current notice, and closed/window-close/fallback-local-session results
clear it.

## Next Implementation Step

Decide whether the product default should remain `block` or move to a
fallback-local-session behavior after more native-window testing. Keep
terminal/plugin feedback free of tab inventory, selected tab id, target hosts,
credentials, and launch results.

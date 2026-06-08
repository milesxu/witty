# Wasmtime Runtime Spike

`witty-plugin-wasm` is the native runtime adapter for Witty Wasm plugins.

## Scope

Implemented in this spike:

- `wasmtime::component::bindgen!` for the `terminal-plugin` world.
- `WasmPluginRuntime` with Component Model enabled.
- Component compilation from bytes and file loading.
- Empty linker and typed store construction.
- `WasmPluginState` for host-owned plugin state.
- WIT/bindgen to `witty-plugin-api` adapters.
- Real Rust guest component fixture under `crates/witty-plugin-wasm/fixtures/guest-plugin`.
- `WasmPluginInstance` for typed `manifest`, `commands`, and `handle_event` calls.
- Manifest-first plugin id capture before host-owned command/action fields are attached.
- `PluginHost` and `TerminalApp` installation path for Wasm component plugins.
- Non-GUI startup loading through `witty --wasm-plugin <file>` and `witty --plugin-dir <dir>`.
- Window-mode startup loading through the same `install_wasm_plugins` path as
  non-GUI smoke mode.
- Runtime event handler error classification and plugin disabling through
  `PluginRuntimeFailure`.
- Read-only `host.get-host-info()` import, registered by the default host
  linker and exercised by the real fixture component.
- Permission-gated `host.get-profile-store-summary()` import, returning only
  profile counts and default-profile presence when `profile-read` is granted.
- Host-owned `request-profile-picker` action, adapted through Wasm and queued
  by `PluginHost` without returning profile data to the plugin.
- Host-owned `request-profile-launch` action, adapted through Wasm and queued
  by `PluginHost` without launching SSH or returning profile details.

Not implemented yet:

- plugin reload/retry UI after runtime failure;
- credential-resolver UI for profiles that cannot resolve locally;
- app-owned session/tab policy for resolved profile launch configs.

## Host-Owned Fields

Wasm guest manifests do not choose their runtime. The adapter maps guest manifests to `PluginRuntime::WasmComponent`.

Guest command registrations omit `source_plugin`. The adapter attaches the manifest/plugin id before commands enter the host registry.

`WasmPluginInstance` requires `manifest()` before `commands()` or `handle_event()`, because those paths need the host-owned plugin id.

## Guest Fixture

The fixture is a `no_std` Rust component built for `wasm32-wasip2` with `wit-bindgen`.

It exports:

- manifest id `fixture`;
- command `fixture.echo`;
- command `fixture.host-info`;
- command `fixture.profile-summary`;
- command `fixture.profile-picker`;
- command `fixture.profile-launch`;
- `handle-event` behavior that returns terminal writes or host-owned profile
  request actions for fixture commands.

The fixture intentionally avoids WASI imports in this minimal invocation test.
It imports only Witty host functions through the default host linker.

## Host Installation

`witty-ui` depends on `witty-plugin-wasm` and exposes a prepare/install split:

- `PluginHost::prepare_wasm_component_from_file`
- `PluginHost::prepare_wasm_instance`
- `PluginHost::install_wasm`
- `PluginHost::install_wasm_component_from_file`
- `TerminalApp::install_wasm_plugin_from_file`

Preparation instantiates the component and reads `manifest`/`commands`. Installation mutates the host only after validation. This lets `TerminalApp` pre-check app-level command id collisions before the host is changed.

`WasmPluginRuntime::instantiate_component()` uses `host_linker()` by default,
so components can import `witty:plugin/host.get-host-info()` without custom
linker setup. `empty_linker()` remains available for explicit custom-linker or
negative tests.

The same host linker also provides
`witty:plugin/host.get-profile-store-summary()`. That import is filtered by
manifest permissions stored in `WasmPluginState`; without `profile-read`, or
without a host-provided summary, it returns `none`.

`witty-ui` provides the host/app injection path that maps the richer
transport-layer `ProfileStoreSummary` into the count-only plugin ABI summary
before instantiating a Wasm plugin.

Wasm plugins can also return `request-profile-picker`. `PluginHost` validates
the action against `profile-read`, attaches the source plugin id, and stores a
pending request for later trusted UI handling. This does not start a profile
picker or expose selected profile details yet. `PluginHost` and `TerminalApp`
also expose `take_profile_picker_requests()` so the future UI consumer can drain
the queue exactly once.

`witty-ui` and `TerminalApp` include non-mutating picker review helpers that
combine pending picker requests with a current `ProfileStoreV1` and return
redacted profile summaries for trusted host UI. This data is not exposed through
the Wasm ABI.

Trusted host UI can then call `resolve_profile_picker_selection()` or, on native
targets, `resolve_profile_picker_pty_config()` with the selected id. These
helpers revalidate the current store and pending request, return only launchable
selections, and still do not spawn PTY/SSH.
The native app helper can also resolve and drain one selected picker request in
one confirmed step, leaving the queue intact when resolution fails.

Wasm plugins can also return `request-profile-launch` with an opaque profile
id. `PluginHost` validates `profile-read`, rejects empty, whitespace-bearing,
control-character, or overlong profile ids, attaches the source plugin id, and
queues `PendingProfileLaunchRequest`. This does not start SSH, validate profile
existence, resolve credentials, or return launch status. `PluginHost` and
`TerminalApp` expose `take_profile_launch_requests()` for a future trusted
consumer that re-reads the profile store before launch.

`witty-ui` now includes `review_profile_launch_requests()` for that trusted
consumer. It revalidates queued requests against a current `ProfileStoreV1` and
returns only redacted review rows for host UI, without target host/user/path,
credential ids, OpenSSH arguments, raw store data, or launch results.

`resolve_profile_launch_request()` provides the next host-only step. It
revalidates one queued request against a current `ProfileStoreV1` and returns a
cloned launchable `SshProfile` for trusted host code. It errors for missing ids,
credential-resolver profiles, unsafe request fields, or invalid stores, and it
does not start PTY/SSH.

On native targets, `resolve_profile_launch_pty_config()` converts that
launchable profile to a `LocalPtyConfig` for SSH without spawning. This keeps
process creation out of the plugin/runtime layer.

`TerminalApp` wraps the pending queue with non-mutating review and native
PTY-config resolution helpers, so app code can inspect the current queue against
a caller-provided `ProfileStoreV1` before any drain or launch policy runs.
Native confirmed-drain helpers support both one selected request and the full
pending batch, leaving failed requests queued for UI error handling or retry.
Trusted UI can also dismiss one pending picker or launch request without
resolving a profile, launching SSH, or reporting rejection to the plugin.
For UI list binding, `review_pending_profile_actions()` combines picker and
launch review rows with kind plus current queue-index keys, and
`dismiss_pending_profile_action()` consumes those keys for cancellation.
On native targets, `take_resolved_pending_profile_action_pty_config()` consumes
matching confirmation values for the same keys and returns a resolved
`LocalPtyConfig` without starting PTY/SSH.
Native window code wraps this in `NativeProfileActionBridge`, which keeps a
pending action snapshot and trusted display rows, then produces
refresh/dismiss/confirmed events for later UI binding. The same snapshot now
includes trusted picker option rows built from redacted profile summaries, so
future host UI can render selectable profiles without exposing target hosts,
credentials, or OpenSSH details. The native window renders the trusted action
rows and picker option rows through a `FramePlan` overlay, keeping profile
details out of terminal scrollback and the plugin ABI. Overlay hit-testing can
classify action row, confirm, and dismiss targets, and it also captures picker
option rows so clicks do not fall through to terminal selection or mouse
reporting. Dismiss consumes the queued host-owned request, while launchable
launch confirm resolves to `LocalPtyConfig` and drains the request without
starting PTY/SSH. Launchable picker option `[Select]` clicks now take the same
trusted confirmed-drain path with the selected profile id, while
credential-resolver-required options remain display-only and captured. A
successful confirmed-drain is converted into a
`NativeResolvedProfileActionHandoff` held only in trusted native window state;
it preserves the resolved `LocalPtyConfig` for later app-owned session policy
but does not spawn PTY/SSH, replace the active transport, or report launch
results to plugins. Confirmed handoffs are queued FIFO so multiple
confirmations remain available to native policy without overwriting earlier
resolved configs. The current app-owned policy is `DeferStart`, which consumes
the next handoff into a trusted deferred-start queue and still does not start a
process. The default confirm flow then converts the deferred start into a
trusted `NativeProfileActionStartPlan` for `ReplaceCurrentSession`; this plans a
future session replacement. Native code now has an explicit execution boundary
for that plan: when caller-owned policy provides an already-created transport,
the boundary replaces the active transport and resets native terminal, search,
and shell-integration state while preserving app-owned command/plugin state. It
does not choose tab/session policy or report launch results to plugins or
terminal scrollback. The native window policy now consumes the next confirmed
start plan, spawns `LocalPtyTransport` from the trusted `LocalPtyConfig`, and
passes that transport into the boundary. Spawn failures keep the start plan
queued and write raw diagnostics to stderr only, not terminal scrollback. The
trusted native overlay also shows a generic start-failure row with retry and
dismiss affordances; retry uses the queued start plan again, and dismiss drops
that queued plan. The failure row does not include raw spawn errors, SSH
targets, credentials, or OpenSSH arguments. Successful replacement sets a
native-only start-success row with a dismiss affordance, also as trusted
`FramePlan` overlay state rather than terminal output. After a successful
profile-action replacement, native window state also records trusted current
session metadata for future tab/session UI: action key, action kind, source
plugin, selected profile id, reason, and start mode. That metadata now lives in
a trusted native session registry with app-owned session ids and active-session
state. The current replacement policy updates the active registry record, and
the tab read model already supports multiple registry records for future tab
policy. It excludes the resolved `LocalPtyConfig`, SSH target, credentials,
OpenSSH arguments, and raw launch result, and it is not exposed through terminal
output or the plugin ABI. The native window now renders a first host-owned
session strip from that registry, showing only profile id, action kind, source
plugin, start mode, and active/inactive status in `FramePlan` while clearing the
terminal glyphs under the strip. The strip has trusted native hit-testing and
hover state; visible tab-span clicks switch only the registry active-session id
and are captured before terminal selection, hyperlink activation, or mouse
reporting. Native code also has a real per-session runtime switch boundary for
parked sessions: transport, terminal, search, and shell-integration state move
between active window state and trusted session records without terminal or
plugin feedback. The profile-action start executor can also create a trusted
`NewTab` start: native policy still creates the transport, then the executor
inserts an inactive session record and parks a fresh terminal/search/shell
runtime under that session id without replacing the current active transport or
terminal state. The native profile-action overlay now exposes that as trusted
host UI: launch rows and launchable picker option rows both distinguish the
replace-current action from `New Tab`, and only native window policy receives
the selected start mode. Native session lifecycle policy now also has a first
parked-session close boundary: inactive records can be removed only when a
matching parked runtime exists, and active or inconsistent session ids are
rejected. Active-session close can now safely switch to another parked session:
native code swaps the target runtime into the active window, parks the old
active runtime, then drops that old parked runtime and registry record. The last
active session is blocked for now and reported only as a short native tab-strip
notice; no fallback PTY is spawned implicitly. That blocked behavior is now an
explicit native fallback policy for active close attempts that have no parked
runtime to switch to, and that policy now resolves to a trusted native fallback
action before mapping to a close result or event-loop request. Non-default
close-window and fallback-local-session actions are selected explicitly through
`--window-last-active-close close-window` or
`--window-last-active-close fallback-local-session`, while the default remains
blocked-last-active. The policy type owns the stable `block`/`close-window`/
`fallback-local-session` config strings for CLI validation and future native
diagnostics. Plugin commands that request picker/launch actions now also produce
a short native pending-action count in the terminal display. The
native session strip renders host-owned close
affordances and maps tab hits to select versus close targets, but those targets
remain native window policy only; select and close hover also use distinct
native `FramePlan` colors without surfacing hover state to plugins. Width-
truncated close markers are display-only and do not produce close hit targets.
Blocked-close notices reserve native strip width when possible and the notice
area is excluded from tab select/close hit-testing. Session-tab click handling
refreshes native hover after notice and close state changes so stale pre-notice
hit targets do not survive.
That feedback is intentionally count-only so
terminal-read plugins cannot infer profile ids, names, launchability, resolved
configs, launch success/failure, or store inventory from the terminal buffer.
The native window refresh path reads the default profile store snapshot when it
exists and treats a missing store as empty, without writing the store, starting
PTY/SSH, or replacing the active transport.
Overlay hover highlighting is kept in trusted native frame state as well: moving
the pointer over a profile action row adds a row background, suppresses lower
terminal hover handling while the overlay is top-most, and still does not write
profile data into scrollback or expose it through Wasm.

`witty-app` supports non-GUI and native-window startup loading:

- `--wasm-plugin <file>` loads one component plugin, repeatable.
- `--plugin-dir <dir>` scans direct child `.wasm` files and loads them in sorted order.

Only smoke and native window modes accept startup Wasm plugins. Browser,
diagnostic, profile-store, and bounded smoke modes reject startup plugins so
their test surfaces remain deterministic.

## Verification

The crate tests currently verify:

- generated `TerminalPlugin` world bindings exist;
- empty Component Model bytes compile;
- core wasm module bytes are rejected;
- store state is preserved;
- manifest, command, event, and action adapters preserve host-owned rules.
- the real fixture component instantiates and all three guest exports are callable.
- `commands()` and `handle_event()` fail clearly if called before `manifest()`.
- the fixture component can call the host-info import and receive default or
  custom store-backed host info.
- the fixture component can call the profile-store summary import and receives
  either `none` or permission-gated summary counts.
- `witty-ui` maps redacted transport profile summaries to the count-only plugin
  summary and can inject those counts through `PluginHost` and `TerminalApp`.
- `request-profile-picker` actions are adapted from Wasm, permission-checked,
  and queued with the host-owned source plugin id.
- profile picker request review returns host-only redacted profile summaries
  without draining the queue or exposing data through the plugin ABI.
- profile picker selection resolution fails closed and can convert a launchable
  host-selected profile id to SSH `LocalPtyConfig` without spawning.
- native confirmed-drain of one pending profile picker request clears only the
  selected request after it resolves to a `LocalPtyConfig`.
- app-owned dismissal of one pending profile picker request removes only that
  request without resolving a profile.
- unified pending profile action review returns picker and launch rows with
  app-owned queue keys for UI list binding.
- unified pending profile action confirmed-drain resolves picker or launch
  confirmations to `LocalPtyConfig` and drains only after success.
- native window profile action bridge maintains review state and emits
  non-launching refresh/dismiss/confirmed events for pending profile actions.
- native window profile action snapshots include trusted display rows for later
  picker/launch UI binding without exposing those rows through terminal
  feedback or the plugin ABI.
- native window profile action snapshots include trusted picker option rows
  derived from redacted summaries, without SSH targets or credential details.
- native window rendering can project trusted display rows and picker option
  rows into a `FramePlan` overlay without writing profile details into terminal
  scrollback.
- native overlay hit-testing classifies row/confirm/dismiss targets and supports
  dismissing a pending host-owned request without launching.
- native overlay hit-testing captures trusted picker option rows so option-area
  clicks do not pass through to terminal selection or mouse reporting.
- native overlay hover highlights only visible trusted rows and ignores hidden
  summary rows without touching terminal output.
- native overlay launch confirm maps only launchable launch rows to trusted
  confirmed-drain, resolving `LocalPtyConfig` without starting PTY/SSH.
- native overlay picker option `[Select]` maps only launchable picker options
  to trusted confirmed-drain, resolving `LocalPtyConfig` without starting
  PTY/SSH.
- native session-tab blocked-close notices are cleared by successful native tab
  switch, close, or profile-action start, without writing notice lifecycle state
  to terminal scrollback or the plugin ABI.
- native confirmed-drain results are normalized into a trusted native handoff
  that preserves `LocalPtyConfig` without writing profile details to terminal
  feedback.
- trusted native handoffs are stored in FIFO order for later app-owned session
  policy, without spawning PTY/SSH.
- the default native handoff policy consumes the next resolved handoff into a
  trusted deferred-start queue without spawning PTY/SSH.
- the default confirm flow converts trusted deferred starts into
  `ReplaceCurrentSession` start plans without spawning PTY/SSH or replacing the
  active transport.
- the `ReplaceCurrentSession` execution boundary can accept an already-created
  transport, replace the active transport, and reset native terminal/search/shell
  state without spawning PTY/SSH or reporting launch results to plugins.
- native window policy can spawn `LocalPtyTransport` from a confirmed trusted
  start plan, pass it into the replace-session boundary, and preserve the queued
  plan on spawn failure without writing launch results to terminal scrollback.
- trusted native overlay state can show a generic start-failure row with retry
  and dismiss affordances, without exposing raw spawn errors, SSH targets,
  credentials, OpenSSH arguments, or launch results through terminal scrollback.
- trusted native overlay state can show a dismissible start-success row after
  session replacement, without writing launch success to terminal scrollback or
  the plugin ABI.
- native plugin-command feedback reports pending picker/launch action counts
  after profile action requests.
- native window refresh reads an existing default profile store snapshot for
  pending action review and treats a missing store as empty.
- `request-profile-launch` actions are adapted from Wasm, permission-checked,
  profile-id validated, and queued with the host-owned source plugin id.
- profile launch request review revalidates the current profile store and
  returns launchable, credential-resolver-required, or not-found status without
  exposing SSH target details.
- profile launch request resolution fails closed and returns a cloned
  `SshProfile` only for launchable profiles, without starting SSH.
- native profile launch request PTY config resolution converts launchable
  requests to SSH `LocalPtyConfig` without spawning a process.
- `TerminalApp` exposes non-mutating pending profile launch review and
  native PTY-config resolution helpers over its queued plugin actions.
- native confirmed-drain of one pending profile launch request clears only that
  request after it resolves to a `LocalPtyConfig`.
- native confirmed-drain of pending profile launch requests clears the queue
  only after every request resolves to a `LocalPtyConfig`.
- app-owned dismissal of one pending profile launch request removes only that
  request without resolving the requested profile id.
- unified pending profile action dismissal consumes the same queue keys without
  resolving profiles or reporting status back to plugins.
- `PluginHost` can install the real fixture and dispatch command events through the existing permission gate.
- `TerminalApp` can install the fixture from a component path and apply returned `WriteTerminal` actions to its transport.
- `witty --wasm-plugin <fixture>` and `witty --plugin-dir <fixture-dir>` load the real fixture at startup.
- `--window --wasm-plugin <file>` is accepted by CLI parsing and routed to the
  shared native plugin installation helper before the window event loop starts.
- `--window --window-last-active-close fallback-local-session` selects a
  non-default trusted native policy that requests a normal local fallback PTY
  when the last active profile-action session is closed. The native boundary
  replaces the active transport, resets terminal/search/shell-integration state,
  and clears host-owned session/tab registry state; spawn failures are
  stderr-only and no terminal/plugin launch result is produced.
- the fallback-local-session spawn boundary can be tested with an injected
  transport spawner, so failure preserves the existing active transport,
  terminal/search/shell state, session registry, and parked runtimes until a
  fallback transport exists.
- native startup reporting includes only the selected last-active close policy
  config value, reusing the CLI strings without exposing profile ids, tab
  inventory, selected tab ids, targets, credentials, PTY configs, raw spawn
  diagnostics, close results, or launch results. Pure tests cover all three
  policy values in that report.
- the last-active close policy type owns both individual config values and the
  CLI allowed-value list, and invalid-value diagnostics reuse that list instead
  of duplicating policy strings.
- the last-active close policy type also owns config-value parsing; CLI parsing
  delegates to that helper and only adds user-facing error text.
- `AppOptions` tests cover every accepted `--window-last-active-close` value
  without launching the native window.
- `AppOptions` also verifies `--window` without an explicit last-active close
  value keeps the default `block` policy.
- native fallback-policy tests verify the CLI-facing policy maps into native
  active-close fallback policy without changing the stable config value.
- CLI, startup-report, and native-policy bridge matrix tests now iterate the
  policy type's canonical `all()` list instead of hand-repeating variant arrays.
- pure CLI policy tests verify the canonical `all()` list and `config_values()`
  list stay ordered together and parser round-trips preserve every allowed
  value.
- native active-close fallback policy tests also keep the private native policy
  list ordered with the CLI-facing window policy list.
- native session-tab close results are classified into internal event-loop
  requests before the event loop consumes them; window close and fallback-local
  session requests remain separate, native-only booleans and carry no profile,
  tab, transport, or launch-result data.
- native close-result event-request classification now iterates the canonical
  result list, so both request booleans and the aggregate `any()` flag are
  covered for every close result.
- window-close and fallback-local-session native event-loop requests share a
  one-shot flag consumer, with pure tests proving requests are returned once
  and cleared before the next event-loop check.
- native close event-request application is centralized on the classifier
  output, with tests proving it sets only requested pending flags and does not
  clear an already queued request.
- blocked-close notice lifecycle tests now cover every current-notice and
  close-result combination, keeping trusted notice creation, retention, and
  clearing explicit.

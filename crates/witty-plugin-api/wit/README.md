# Witty Plugin WIT

`witty-plugin.wit` is the initial Wasm Component Model contract for terminal plugins.

Design constraints:

- The host owns permissions and command ownership checks.
- Guest command registrations omit `source_plugin`; the host attaches the manifest id.
- Command arguments are `args-json` for now so the ABI can stay stable while app-level command schemas evolve.
- Command invocations carry a typed `context` record. The first context field is
  `current-directory`, populated from OSC 7 shell integration only when the
  host grants terminal read permission to the plugin.
- The context can also carry `selected-command-block` metadata for the active
  screen: block id, command/output ranges, exit code, timing, duration, and
  block cwd. It deliberately excludes command/output text.
- Permission filtering is host-side: `none` receives empty context,
  `selection-only` receives selected block metadata, and `current-screen` or
  `full-scrollback` receive the full context.
- Command invocations are host-routed to the registered `source_plugin`; other
  plugins do not receive the command id, args, or context.
- Runtime `register-command` actions extend the host routing table only after
  ownership and duplicate-id validation.
- `terminal-output` events are host-filtered to `current-screen` and
  `full-scrollback`; `selection-changed` events are filtered out only for
  `none`.
- Terminal writes remain action-based. The host still enforces `TerminalWritePermission`.
- The initial host import is read-only `host.get-host-info()`, returning only
  app name, app version, and plugin ABI version. It deliberately omits profile,
  host, path, PTY, renderer, and system details.
- `host.get-profile-store-summary()` is gated by `profile-read` and returns
  only counts plus whether a default profile is configured. It deliberately
  omits profile ids, names, target hosts, users, ports, SSH paths, credential
  references, and raw profile store content.
- `request-profile-picker` is a host-owned action, gated by `profile-read`,
  that asks the host to open or schedule trusted profile UI. The plugin receives
  no profile inventory, selected id, launch result, or raw profile details.
- `request-profile-launch` is a host-owned action, gated by `profile-read`,
  that queues an opaque profile id for trusted host launch policy. The plugin
  receives no profile metadata, existence check, credential details, launch
  result, or raw profile-store content.

Runtime direction:

- Primary runtime: Wasmtime Component Model.
- Secondary option: Extism for simpler script-like plugins after the core ABI stabilizes.
